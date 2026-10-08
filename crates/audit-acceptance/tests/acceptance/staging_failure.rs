//! T2 (design §14.3–§14.4): the audit staging and the relay registration
//! are part of the Document business transaction. When either fails, the
//! business operation fails with the repository's PostgreSQL failure and
//! nothing of it remains: no Document, version, folder, ACL, read-state,
//! schedule or domain-event row changes, no staging row and no delivery
//! registration; a file access or a comparison whose grant cannot be
//! staged returns nothing. Every relay-origin Document type of the catalog
//! is refused except `authorization.denied`, which Document stages outside
//! the business transaction (handoff §5.4), at every staging site
//! exercised (`document-repository-postgres`): the document creation
//! (`repository.rs`), the versioning mutation (new version, WORKING
//! update, rebase), the initial publication (manual and by the scheduler)
//! and the next-version publication (manual; the two sites of
//! `publish.rs`), the schedule writes (schedule, cancellation, terminal
//! outcome), the withdrawal, the publication end, the first read
//! confirmation, VIEW and RESET, the file-access, diff and
//! revision-comparison grants, and the management events
//! (`targeted_events.rs`: metadata, folder creation, rename and move,
//! document move, ACL change, and the root policy bootstrap, refused before
//! any setup). The scheduler's publication stays pending (only its retry
//! bookkeeping moves) and the scheduler's terminal outcome is not recorded.
//! Such a failure is invisible to the relay (health stays clean), unlike a
//! Store outage (T3), where the business commits and only the delivery
//! waits. Once the sabotage is removed, every refused operation is run
//! again, commits, and the relay delivers exactly what committed.
//!
//! Two Document writes happen outside the business transaction by design
//! and stay after a refusal: the versioning preflight stores the uploaded
//! file object and its semantic inspection before the version mutation (an
//! abandoned upload; the mutation is compared from after its preflight),
//! and publications and schedules fill the deterministic semantic
//! inspection cache (it may only gain rows).
//!
//! The failures are injected with test-only objects a superuser adds and
//! removes (no production code is changed): a BEFORE INSERT trigger on
//! `public.audit_outbox_events` raising SQLSTATE P0001 for the refused
//! event types (staging failure), and a CHECK on `audit_relay.deliveries`
//! refusing new registrations (registration trigger failure). The staging
//! failure is not a CHECK because the versioning producers report an
//! integrity SQLSTATE (23505, 23503, 23514) of their statements, the
//! staging INSERT included, as `Conflict`; with P0001 every site reports
//! the repository's generic PostgreSQL failure, which is what is asserted.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use audit_core::{Catalog, Origin};
use document_application::{
    ApplicationError, DueExecutionOutcome, PublishOperationId, ReadStateMutationKind,
    RepositoryError,
};
use document_domain::{DocumentId, DocumentVersionId, FolderId};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::document::{Platform, editor_ctx, reader_ctx};
use crate::support::*;

/// The types whose staging INSERT phase 1 refuses: every relay-origin
/// Document type of the catalog except `document.created` (it stages, then
/// the creation's version row is refused: a partial staging rolls back;
/// phase 2 refuses its registration) and `authorization.denied` (staged
/// outside any business transaction).
const REFUSED: [&str; 21] = [
    "access_policy.changed",
    "document.version.created",
    "document.version.updated",
    "document.version.rebased",
    "document.version.published",
    "document.version.publication.scheduled",
    "document.version.publication.cancelled",
    "document.version.publication.terminal",
    "document.version.read_confirmed",
    "document.version.detail_viewed",
    "document.version.marked_unread",
    "document.file.access_granted",
    "document.metadata.changed",
    "document.revision_comparison.result_access_granted",
    "folder.created",
    "folder.renamed",
    "folder.moved",
    "document.moved",
    "document.diff.result_access_granted",
    "document.version.withdrawn",
    "document.publication.ended",
];
const NOT_REFUSED: [&str; 2] = ["document.created", "authorization.denied"];

/// The repository's failure for any statement error (`map_statement_error`):
/// what a refused staging INSERT or registration trigger becomes.
const POSTGRES_FAILURE: &str = "postgres operation failed";

/// The semantic inspection cache: deterministic per file and profile
/// (insert or converge, `ON CONFLICT DO NOTHING`), filled by the
/// versioning preflight and before a publication or a schedule, outside the
/// business transaction. A refused operation may add to it; it never
/// changes or removes a row.
const INSPECTIONS: &str = "public.document_semantic_inspections";

/// The business state: a fingerprint of every Document table (`public`)
/// and of the relay's delivery registrations (row count and an
/// order-independent digest of the rows' text), and the rows of the
/// inspection cache.
struct Snapshot {
    tables: Value,
    inspections: BTreeSet<String>,
}

async fn business_snapshot(env: &Env) -> Snapshot {
    let inspections: Vec<String> =
        sqlx::query_scalar("SELECT md5(r::text) FROM public.document_semantic_inspections AS r")
            .fetch_all(&env.doc_admin)
            .await
            .expect("inspections");
    Snapshot {
        tables: business_tables(env).await,
        inspections: inspections.into_iter().collect(),
    }
}

async fn business_tables(env: &Env) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_object_agg(t.schema || '.' || t.name, t.fingerprint) FROM ( \
             SELECT n.nspname AS schema, c.relname AS name, \
                    (xpath('/row/f/text()', query_to_xml(format( \
                        'SELECT count(*) || '':'' || md5(coalesce(string_agg(r::text, ''|'' \
                         ORDER BY r::text), '''')) AS f FROM %I.%I AS r', n.nspname, c.relname), \
                        false, true, '')))[1]::text AS fingerprint \
             FROM pg_class AS c JOIN pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE c.relkind = 'r' AND (n.nspname = 'public' \
                   OR (n.nspname = 'audit_relay' AND c.relname = 'deliveries'))) AS t",
    )
    .fetch_one(&env.doc_admin)
    .await
    .expect("business snapshot")
}

/// The schedules without their retry bookkeeping (attempt count, last
/// attempt, next retry).
async fn schedules_without_retries(env: &Env) -> String {
    sqlx::query_scalar(
        "SELECT coalesce(md5(string_agg((to_jsonb(s) - 'attempt_count' - 'last_attempt_at' \
                - 'next_retry_at')::text, '|' ORDER BY s.publish_operation_id)), '') \
         FROM public.document_publish_schedules AS s",
    )
    .fetch_one(&env.doc_admin)
    .await
    .expect("schedules")
}

/// `(status, attempt_count, next_retry_at is set)` of one schedule.
async fn schedule_state(env: &Env, schedule: PublishOperationId) -> (String, i32, bool) {
    sqlx::query_as(
        "SELECT status, attempt_count, next_retry_at IS NOT NULL \
         FROM public.document_publish_schedules WHERE publish_operation_id = $1",
    )
    .bind(schedule.as_uuid())
    .fetch_one(&env.doc_admin)
    .await
    .expect("schedule")
}

/// Nothing changed but the tables in `except`; the inspection cache only
/// gained rows.
fn assert_unchanged_except(before: &Snapshot, after: &Snapshot, what: &str, except: &[&str]) {
    let tables = before.tables.as_object().expect("snapshot");
    let now = after.tables.as_object().expect("snapshot");
    assert!(tables.len() >= 20, "every Document table: {tables:?}");
    assert_eq!(tables.len(), now.len());
    let changed: Vec<&String> = tables
        .keys()
        .filter(|table| table.as_str() != INSPECTIONS && !except.contains(&table.as_str()))
        .filter(|table| tables.get(*table) != now.get(*table))
        .collect();
    assert!(changed.is_empty(), "{what} changed {changed:?}");
    assert!(
        before.inspections.is_subset(&after.inspections),
        "{what}: an inspection row changed or disappeared"
    );
}

fn assert_postgres_failure<T: Debug>(what: &str, result: &Result<T, ApplicationError>) {
    assert!(
        matches!(result, Err(ApplicationError::Internal(message)) if message == POSTGRES_FAILURE),
        "{what}: the repository's PostgreSQL failure, got {result:?}"
    );
}

/// `result` is the PostgreSQL failure of the sabotage and the business
/// state (and every registration) is what it was before.
async fn assert_refused<T: Debug>(
    env: &Env,
    before: &Snapshot,
    what: &str,
    result: Result<T, ApplicationError>,
) {
    assert_postgres_failure(what, &result);
    assert_unchanged_except(before, &business_snapshot(env).await, what, &[]);
}

/// The staging sabotage: a BEFORE INSERT trigger on
/// `public.audit_outbox_events` raising SQLSTATE P0001 for `types`.
async fn refuse_staging(env: &Env, types: &[&str]) {
    let list = types
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(", ");
    exec(
        &env.doc_admin,
        &format!(
            "CREATE FUNCTION public.acceptance_refuse_staging() RETURNS trigger \
             LANGUAGE plpgsql AS $$ BEGIN \
                 IF NEW.event_type = ANY (TG_ARGV) THEN \
                     RAISE EXCEPTION 'acceptance: staging refused' USING ERRCODE = 'P0001'; \
                 END IF; \
                 RETURN NEW; \
             END $$; \
             CREATE TRIGGER acceptance_refuse_staging \
             BEFORE INSERT ON public.audit_outbox_events FOR EACH ROW \
             EXECUTE FUNCTION public.acceptance_refuse_staging({list})"
        ),
    )
    .await;
}

async fn allow_staging(env: &Env) {
    exec(
        &env.doc_admin,
        "DROP TRIGGER acceptance_refuse_staging ON public.audit_outbox_events; \
         DROP FUNCTION public.acceptance_refuse_staging()",
    )
    .await;
}

async fn create(platform: &Platform, title: &str) -> (DocumentId, DocumentVersionId) {
    platform
        .create_document(
            Platform::root(),
            title,
            format!("A synthetic body of {title}\n").as_bytes(),
        )
        .await
        .expect("create")
}

async fn create_published(platform: &Platform, title: &str) -> (DocumentId, DocumentVersionId) {
    let (document, version) = create(platform, title).await;
    platform.publish(document, version).await.expect("publish");
    (document, version)
}

#[test]
fn a_failed_staging_or_registration_rolls_the_business_transaction_back() {
    run_scenario(staging_failure);
}

async fn staging_failure() {
    // The relay-origin Document types, as T1 selects them: a relay type of
    // another source is not a Document producer and is not named here.
    let catalog: BTreeSet<&str> = Catalog::embedded()
        .events()
        .iter()
        .filter(|spec| spec.origin == Origin::Relay && spec.source == DOCUMENT_SOURCE)
        .map(|spec| spec.event_type.as_str())
        .collect();
    let covered: BTreeSet<&str> = REFUSED.iter().chain(NOT_REFUSED.iter()).copied().collect();
    assert_eq!(covered, catalog, "every catalog type is refused or named");

    let env = Env::start().await;
    let platform = Platform::new(env.document.clone());

    // ------------------------------------------------------------------
    // 0. The root policy bootstrap (`access_policy.changed`, `bootstrap:
    //    true`) cannot stage: the repository's PostgreSQL failure, no
    //    policy, no access revision, no audit row. Once staging works, the
    //    same bootstrap commits (and is delivered with the rest below).
    // ------------------------------------------------------------------
    refuse_staging(&env, &["access_policy.changed"]).await;
    let before = business_snapshot(&env).await;
    let result = platform.try_bootstrap().await;
    assert!(
        matches!(&result, Err(RepositoryError::Internal(message)) if message == POSTGRES_FAILURE),
        "root policy bootstrap: the repository's PostgreSQL failure, got {result:?}"
    );
    assert_unchanged_except(
        &before,
        &business_snapshot(&env).await,
        "root policy bootstrap",
        &[],
    );
    let policies: i64 = sqlx::query_scalar("SELECT count(*) FROM public.access_policy_bindings")
        .fetch_one(&env.doc_admin)
        .await
        .expect("policies");
    assert_eq!(policies, 0, "no root policy");
    assert!(staged_rows(&env).await.is_empty(), "no audit row");
    allow_staging(&env).await;
    platform.bootstrap().await;

    // ------------------------------------------------------------------
    // Setup, committed while staging works: what each refused operation
    // needs.
    // ------------------------------------------------------------------
    let folder = FolderId::from_uuid(Uuid::now_v7());
    let target = FolderId::from_uuid(Uuid::now_v7());
    for (id, name) in [(folder, "Synthetic F"), (target, "Synthetic G")] {
        platform
            .create_folder(&editor_ctx(), id, Platform::root(), name, "synthetic setup")
            .await
            .expect("folder");
    }
    // `doc`: published, revision 1.1 by a metadata change (a same-version
    // revision comparison), viewed by the reader (a RESET).
    let (doc, doc_v1) = create_published(&platform, "Synthetic atomicity").await;
    platform
        .update_metadata(doc, "setup", "synthetic setup metadata")
        .await
        .expect("metadata");
    let r10 = platform.revision_id(doc, 1, 0).await;
    let r11 = platform.revision_id(doc, 1, 1).await;
    assert!(
        platform
            .read_state(&reader_ctx(), doc, doc_v1, 0, ReadStateMutationKind::View)
            .await
            .expect("view")
    );
    // `viewed`: published, never viewed (a detail view, a new version).
    let (viewed, viewed_v1) = create_published(&platform, "Synthetic viewed").await;
    // `ended`: published (a publication end).
    let (ended, ended_v1) = create_published(&platform, "Synthetic ended").await;
    // `withdrawn`: v1 and v2 published, the diff v1 → v2 computed once (a
    // later comparison is a cache hit that only stages its grant).
    let (withdrawn, w1) = create_published(&platform, "Synthetic withdrawn").await;
    let w2 = platform
        .create_version(withdrawn, "Synthetic withdrawn v2", b"B withdrawn v2\n")
        .await
        .expect("version");
    platform.publish(withdrawn, w2).await.expect("publish v2");
    assert!(
        !platform.compare_versions(withdrawn, w1, w2).await,
        "the first diff computes"
    );
    // `rebased`: v1 and v2 published, v3 WORKING on v2, v2 withdrawn: v3
    // must be rebased onto v1.
    let (rebased, rb1) = create_published(&platform, "Synthetic rebased").await;
    let rb2 = platform
        .create_version(rebased, "Synthetic rebased v2", b"B rebased v2\n")
        .await
        .expect("version");
    platform.publish(rebased, rb2).await.expect("publish v2");
    let rb3 = platform
        .create_version(rebased, "Synthetic rebased v3", b"C rebased v3\n")
        .await
        .expect("version");
    assert_eq!(
        platform
            .withdraw(rebased, rb2, "synthetic setup withdraw")
            .await
            .expect("withdraw"),
        Some(rb1)
    );
    // `next`: v1 published, v2 WORKING on it (a next-version publication).
    let (next, _) = create_published(&platform, "Synthetic next").await;
    let next_v2 = platform
        .create_version(next, "Synthetic next v2", b"B next v2\n")
        .await
        .expect("version");
    // WORKING originals: a content update, a manual publication, a new
    // schedule, a schedule to cancel.
    let (updated, updated_v1) = create(&platform, "Synthetic updated").await;
    let (published, published_v1) = create(&platform, "Synthetic published").await;
    let (scheduled, scheduled_v1) = create(&platform, "Synthetic scheduled").await;
    let (cancelled, cancelled_v1) = create(&platform, "Synthetic cancelled").await;
    let far = OffsetDateTime::now_utc() + time::Duration::hours(1);
    let to_cancel = platform
        .schedule_publish(cancelled, cancelled_v1, far)
        .await
        .expect("schedule");
    // Schedules the scheduler executes: one publishes, one ends terminal
    // because the requester lost Publish.
    let due_at = OffsetDateTime::now_utc() + time::Duration::seconds(2);
    let (due, due_v1) = create(&platform, "Synthetic due").await;
    let due_schedule = platform
        .schedule_publish(due, due_v1, due_at)
        .await
        .expect("schedule");
    let (terminal, terminal_v1) = create(&platform, "Synthetic terminal").await;
    let terminal_schedule = platform
        .schedule_publish(terminal, terminal_v1, due_at)
        .await
        .expect("schedule");
    platform
        .revoke_publish(terminal, "synthetic setup revoke")
        .await
        .expect("revoke Publish");
    let refused_folder = FolderId::from_uuid(Uuid::now_v7());
    let baseline = staged_rows(&env).await.len();

    // ------------------------------------------------------------------
    // 1. Staging failure: the staging INSERT of every refused type fails.
    // ------------------------------------------------------------------
    refuse_staging(&env, &REFUSED).await;
    let before = business_snapshot(&env).await;

    // Management: metadata, folders, ACL, document move.
    let result = platform
        .update_metadata(doc, "refused", "synthetic refused metadata")
        .await;
    assert_refused(&env, &before, "metadata change", result).await;
    let result = platform
        .create_folder(
            &editor_ctx(),
            refused_folder,
            Platform::root(),
            "Synthetic refused folder",
            "synthetic refused folder",
        )
        .await;
    assert_refused(&env, &before, "folder creation", result).await;
    let result = platform
        .rename_folder(folder, "Synthetic F refused", "synthetic refused rename")
        .await;
    assert_refused(&env, &before, "folder rename", result).await;
    let result = platform
        .move_folder(folder, Platform::root(), target, "synthetic refused move")
        .await;
    assert_refused(&env, &before, "folder move", result).await;
    let result = platform
        .set_folder_policy(folder, 0, "synthetic refused policy")
        .await;
    assert_refused(&env, &before, "ACL change", result).await;
    let result = platform
        .move_document(doc, Platform::root(), target, "synthetic refused move")
        .await;
    assert_refused(&env, &before, "document move", result).await;

    // Versioning: a creation with two audit rows (document.created would
    // stage, the version row cannot: the whole creation rolls back), a new
    // version, a WORKING update, a rebase, a manual publication of a first
    // version and of a next version, a schedule and its cancellation, a
    // withdrawal, a publication end.
    let result = platform
        .create_document(Platform::root(), "Synthetic created", b"A created body\n")
        .await;
    assert_refused(&env, &before, "document creation", result).await;
    // The versioning preflight stores the uploaded file object and its
    // semantic inspection before, and outside, the version mutation (an
    // abandoned upload leaves them unreferenced): the mutation is compared
    // from after its preflight.
    for (what, document, version) in [
        ("new version", viewed, None),
        ("WORKING update", updated, Some(updated_v1)),
    ] {
        let prepared = platform
            .prepare(&editor_ctx(), "Synthetic version", b"B synthetic version\n")
            .await
            .expect("preflight");
        let preflight = business_snapshot(&env).await;
        assert_unchanged_except(
            &before,
            &preflight,
            "the preflight",
            &["public.file_objects"],
        );
        match version {
            None => {
                let result = platform.create_version_from(document, prepared).await;
                assert_refused(&env, &preflight, what, result).await;
            }
            Some(version) => {
                let result = platform
                    .update_working_from(document, version, prepared)
                    .await;
                assert_refused(&env, &preflight, what, result).await;
            }
        }
    }
    // Later comparisons start from here.
    let before = business_snapshot(&env).await;
    let result = platform.rebase(rebased, rb3).await;
    assert_refused(&env, &before, "rebase", result).await;
    // A publication or a schedule first makes sure the version's semantic
    // inspection is cached (the only change they may leave).
    let result = platform.publish(published, published_v1).await;
    assert_refused(&env, &before, "manual publication", result).await;
    let result = platform.publish(next, next_v2).await;
    assert_refused(&env, &before, "next-version publication", result).await;
    let result = platform
        .schedule_publish(scheduled, scheduled_v1, far)
        .await;
    assert_refused(&env, &before, "schedule", result).await;
    let result = platform
        .cancel_schedule(cancelled, cancelled_v1, to_cancel)
        .await;
    assert_refused(&env, &before, "schedule cancellation", result).await;
    let result = platform
        .withdraw(withdrawn, w2, "synthetic refused withdraw")
        .await;
    assert_refused(&env, &before, "withdrawal", result).await;
    let result = platform
        .end_publication(ended, ended_v1, "synthetic refused end")
        .await;
    assert_refused(&env, &before, "publication end", result).await;

    // Read state: first read confirmation, detail view, RESET.
    let result = platform.mark_read(&editor_ctx(), doc, doc_v1).await;
    assert_refused(&env, &before, "read confirmation", result).await;
    let result = platform
        .read_state(
            &reader_ctx(),
            viewed,
            viewed_v1,
            0,
            ReadStateMutationKind::View,
        )
        .await;
    assert_refused(&env, &before, "detail view", result).await;
    let result = platform
        .read_state(&reader_ctx(), doc, doc_v1, 1, ReadStateMutationKind::Reset)
        .await;
    assert_refused(&env, &before, "RESET", result).await;

    // Grants: nothing is opened or compared without its audit row.
    let result = platform
        .try_open_original(doc, doc_v1, Uuid::now_v7())
        .await;
    assert_refused(&env, &before, "original file access", result).await;
    let result = platform.try_compare_versions(withdrawn, w1, w2).await;
    assert_refused(&env, &before, "diff (cache hit)", result).await;
    let result = platform.try_compare_revisions(doc, r10, r11).await;
    assert_refused(&env, &before, "revision comparison", result).await;

    // The scheduler: the due publication cannot stage its audit row, so it
    // is not published and the schedule stays pending (only its retry
    // bookkeeping moves: attempt 1, next retry); the terminal outcome
    // cannot be recorded and the schedule is untouched.
    let schedules_before = schedules_without_retries(&env).await;
    let outcomes = platform
        .run_due_once(&[due_schedule, terminal_schedule])
        .await;
    assert!(
        matches!(
            outcomes[&due_schedule.as_uuid()],
            Ok(DueExecutionOutcome::RetryScheduled(_))
        ),
        "{outcomes:?}"
    );
    assert_postgres_failure("terminal outcome", &outcomes[&terminal_schedule.as_uuid()]);
    assert_unchanged_except(
        &before,
        &business_snapshot(&env).await,
        "the scheduler",
        &["public.document_publish_schedules"],
    );
    assert_eq!(
        schedules_without_retries(&env).await,
        schedules_before,
        "only the retry bookkeeping of the schedules changed"
    );
    assert_eq!(
        schedule_state(&env, due_schedule).await,
        ("PENDING".to_owned(), 1, true),
        "the due publication stays pending and is retried"
    );
    assert_eq!(
        schedule_state(&env, terminal_schedule).await,
        ("PENDING".to_owned(), 0, false),
        "the terminal outcome is not recorded"
    );

    // The relay sees nothing: no staged row, no registration, no alarm.
    let report = env.health(false).await;
    assert_eq!(report["produced"]["staged"], json!(baseline), "{report}");
    assert_eq!(report["produced"]["registered"], json!(baseline));
    assert_eq!(report["delivered"]["pending"], json!(baseline));
    assert_eq!(report["alarms"], json!([]), "{report}");
    allow_staging(&env).await;

    // ------------------------------------------------------------------
    // 2. Registration failure: the relay's registration trigger (a definer
    //    function in the same transaction) cannot register new rows.
    // ------------------------------------------------------------------
    exec(
        &env.doc_admin,
        "DO $$ BEGIN EXECUTE format('ALTER TABLE audit_relay.deliveries \
             ADD CONSTRAINT acceptance_refuse_registration \
             CHECK (registered_at <= %L::timestamptz) NOT VALID', clock_timestamp()); END $$",
    )
    .await;
    let before = business_snapshot(&env).await;
    let result = platform
        .update_metadata(doc, "unregistered", "synthetic unregistered metadata")
        .await;
    assert_refused(&env, &before, "metadata change", result).await;
    let result = platform
        .create_folder(
            &editor_ctx(),
            refused_folder,
            Platform::root(),
            "Synthetic unregistered",
            "synthetic unregistered folder",
        )
        .await;
    assert_refused(&env, &before, "folder creation", result).await;
    let result = platform.mark_read(&editor_ctx(), doc, doc_v1).await;
    assert_refused(&env, &before, "read confirmation", result).await;
    let result = platform
        .create_document(Platform::root(), "Synthetic created", b"A created body\n")
        .await;
    assert_refused(&env, &before, "document creation", result).await;
    assert_eq!(staged_rows(&env).await.len(), baseline, "no staging row");
    exec(
        &env.doc_admin,
        "ALTER TABLE audit_relay.deliveries DROP CONSTRAINT acceptance_refuse_registration",
    )
    .await;

    // ------------------------------------------------------------------
    // 3. Contrast: every refused operation commits once staging works, and
    //    the relay delivers exactly what committed.
    // ------------------------------------------------------------------
    platform
        .update_metadata(doc, "accepted", "synthetic accepted metadata")
        .await
        .expect("metadata");
    platform
        .create_folder(
            &editor_ctx(),
            refused_folder,
            Platform::root(),
            "Synthetic accepted folder",
            "synthetic accepted folder",
        )
        .await
        .expect("folder");
    platform
        .rename_folder(folder, "Synthetic F renamed", "synthetic accepted rename")
        .await
        .expect("rename");
    platform
        .move_folder(folder, Platform::root(), target, "synthetic accepted move")
        .await
        .expect("folder move");
    platform
        .set_folder_policy(folder, 0, "synthetic accepted policy")
        .await
        .expect("policy");
    platform
        .move_document(doc, Platform::root(), target, "synthetic accepted move")
        .await
        .expect("document move");
    platform
        .create_document(Platform::root(), "Synthetic created", b"A created body\n")
        .await
        .expect("creation");
    assert!(
        platform
            .read_state(
                &reader_ctx(),
                viewed,
                viewed_v1,
                0,
                ReadStateMutationKind::View
            )
            .await
            .expect("detail view")
    );
    platform
        .create_version(viewed, "Synthetic version", b"B synthetic version\n")
        .await
        .expect("new version");
    platform
        .update_working(
            updated,
            updated_v1,
            "Synthetic version",
            b"B synthetic version\n",
        )
        .await
        .expect("WORKING update");
    platform.rebase(rebased, rb3).await.expect("rebase");
    platform
        .publish(published, published_v1)
        .await
        .expect("manual publication");
    platform
        .publish(next, next_v2)
        .await
        .expect("next-version publication");
    platform
        .schedule_publish(scheduled, scheduled_v1, far)
        .await
        .expect("schedule");
    platform
        .cancel_schedule(cancelled, cancelled_v1, to_cancel)
        .await
        .expect("cancellation");
    assert!(
        platform.compare_versions(withdrawn, w1, w2).await,
        "cache hit"
    );
    assert_eq!(
        platform
            .withdraw(withdrawn, w2, "synthetic accepted withdraw")
            .await
            .expect("withdrawal"),
        Some(w1)
    );
    platform
        .end_publication(ended, ended_v1, "synthetic accepted end")
        .await
        .expect("publication end");
    assert!(
        platform
            .mark_read(&editor_ctx(), doc, doc_v1)
            .await
            .expect("read")
    );
    assert!(
        platform
            .read_state(&reader_ctx(), doc, doc_v1, 1, ReadStateMutationKind::Reset)
            .await
            .expect("RESET")
    );
    assert!(
        !platform
            .open_original(doc, doc_v1, Uuid::now_v7())
            .await
            .is_empty()
    );
    assert!(
        !platform.compare_revisions(doc, r10, r11).await,
        "same version"
    );
    // The schedule that stayed pending is due again and now publishes; the
    // terminal outcome is recorded.
    let outcomes = platform
        .run_scheduler_until(&[due_schedule, terminal_schedule])
        .await;
    assert!(
        matches!(
            outcomes[&due_schedule.as_uuid()],
            DueExecutionOutcome::Published(_)
        ),
        "{outcomes:?}"
    );
    assert!(
        matches!(
            &outcomes[&terminal_schedule.as_uuid()],
            DueExecutionOutcome::Terminal(reason) if reason == "authorization_revoked"
        ),
        "{outcomes:?}"
    );
    assert_eq!(schedule_state(&env, due_schedule).await.0, "PUBLISHED");

    let staged = staged_rows(&env).await;
    let committed = &staged[baseline..];
    let committed_types: BTreeSet<&str> = committed
        .iter()
        .map(|row| row.event_type.as_str())
        .collect();
    let mut expected: BTreeSet<&str> = REFUSED.iter().copied().collect();
    expected.insert("document.created");
    assert_eq!(
        committed_types, expected,
        "every refused producer commits its audit row once staging works"
    );
    let by_type = |event_type: &str| {
        committed
            .iter()
            .filter(|r| r.event_type == event_type)
            .count()
    };
    let counts: BTreeMap<&str, usize> = expected.iter().map(|t| (*t, by_type(t))).collect();
    assert_eq!(by_type("document.created"), 1, "{counts:?}");
    let executed = committed
        .iter()
        .find(|row| {
            row.event_type == "document.version.published" && row.resource_id == due.as_uuid()
        })
        .expect("the scheduled publication");
    assert_eq!(
        executed.data["serviceExecutor"],
        json!({"identityProvider": "service", "principalId": "scheduler"})
    );
    assert!(committed.iter().any(
        |row| row.event_type == "document.version.publication.terminal"
            && row.resource_id == terminal.as_uuid()
    ));
    // The two refusals of the other publication site and of the bootstrap
    // commit exactly once each.
    let next_published: Vec<Uuid> = committed
        .iter()
        .filter(|row| {
            row.event_type == "document.version.published"
                && row.resource_id == next.as_uuid()
                && row.resource_version_id == Some(next_v2.as_uuid())
        })
        .map(|row| row.event_id)
        .collect();
    assert_eq!(next_published.len(), 1, "the next-version publication");
    let bootstrapped: Vec<Uuid> = staged
        .iter()
        .filter(|row| {
            row.event_type == "access_policy.changed" && row.data["bootstrap"] == json!(true)
        })
        .map(|row| row.event_id)
        .collect();
    assert_eq!(bootstrapped.len(), 1, "the root policy bootstrap");

    assert_relay_posture_clean(&env).await;
    let relay = RunningRelay::start(&env);
    wait_until("the relay drains", CONVERGE, || drained(&env)).await;
    relay.stop().await;
    let store = env.store_client().await;
    let stored = assert_delivered_exactly_once(&env, &env.store_admin, &store).await;
    assert_eq!(stored.len(), staged.len(), "nothing of the failed attempts");
    for id in next_published.iter().chain(&bootstrapped) {
        assert!(stored.contains_key(id), "{id} reaches the Store once");
    }
    assert_eq!(assert_store_chain(&env.store_admin).await, staged.len());
    let dump = env.store_dump_text(STORE_DB).await;
    assert_absent(
        &dump,
        "the Store",
        &[
            "synthetic refused".to_owned(),
            "synthetic unregistered".to_owned(),
            "synthetic accepted".to_owned(),
        ],
    );
    eprintln!(
        "staging_failure: baseline={baseline} refused_types={} committed_after={} {counts:?}",
        REFUSED.len(),
        committed.len()
    );
}
