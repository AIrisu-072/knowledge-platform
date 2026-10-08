//! T2 (design §14.3–§14.4): the audit staging and the relay registration
//! are part of the Document business transaction. When either fails, the
//! business operation fails and nothing of it remains: no Document,
//! version, folder, ACL, read-state or domain-event row changes, no
//! staging row and no delivery registration. Such a failure is invisible
//! to the relay (health stays clean), unlike a Store outage (T3), where the
//! business commits and only the delivery waits.
//!
//! The failures are injected with test-only constraints a superuser adds
//! and removes (no production code is changed): a CHECK on
//! `public.audit_outbox_events` refusing chosen event types (staging
//! failure), and a CHECK on `audit_relay.deliveries` refusing new
//! registrations (registration trigger failure).

use document_application::ReadStateMutationKind;
use document_domain::FolderId;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::document::{Platform, editor_ctx, reader_ctx};
use crate::support::*;

/// A fingerprint of every Document table (`public`) and of the relay's
/// delivery registrations: row count and an order-independent digest of
/// the rows' text.
async fn business_snapshot(env: &Env) -> Value {
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

fn assert_unchanged(before: &Value, after: &Value, what: &str) {
    let before = before.as_object().expect("snapshot");
    let after = after.as_object().expect("snapshot");
    assert!(before.len() >= 20, "every Document table: {before:?}");
    let changed: Vec<&String> = before
        .keys()
        .filter(|table| before.get(*table) != after.get(*table))
        .collect();
    assert!(changed.is_empty(), "{what} changed {changed:?}");
    assert_eq!(before.len(), after.len());
}

#[test]
fn a_failed_staging_or_registration_rolls_the_business_transaction_back() {
    run_scenario(staging_failure);
}

async fn staging_failure() {
    let env = Env::start().await;
    let platform = Platform::new(env.document.clone());
    platform.bootstrap().await;
    let (document, v1) = platform
        .create_document(
            Platform::root(),
            "Synthetic atomicity",
            b"A synthetic body for the atomicity test\n",
        )
        .await
        .expect("create");
    platform.publish(document, v1).await.expect("publish");
    let folder = FolderId::from_uuid(Uuid::now_v7());
    platform
        .create_folder(
            &editor_ctx(),
            folder,
            Platform::root(),
            "Synthetic F",
            "synthetic create",
        )
        .await
        .expect("folder");
    let baseline = staged_rows(&env).await.len();
    assert_eq!(
        baseline, 5,
        "bootstrap, created, version created, published, folder"
    );

    // ------------------------------------------------------------------
    // 1. Staging failure: the staging INSERT of these types fails.
    // ------------------------------------------------------------------
    exec(
        &env.doc_admin,
        "ALTER TABLE public.audit_outbox_events ADD CONSTRAINT acceptance_refuse_staging \
         CHECK (event_type NOT IN ('document.metadata.changed', 'folder.renamed', \
             'access_policy.changed', 'document.version.created', \
             'document.version.read_confirmed', 'document.version.detail_viewed', \
             'document.publication.ended')) NOT VALID",
    )
    .await;
    let before = business_snapshot(&env).await;

    let failed = platform
        .update_metadata(document, "refused", "synthetic refused metadata")
        .await;
    assert!(failed.is_err(), "metadata: {failed:?}");
    assert_unchanged(&before, &business_snapshot(&env).await, "metadata change");

    let failed = platform
        .rename_folder(folder, "Synthetic F refused", "synthetic refused rename")
        .await;
    assert!(failed.is_err(), "rename: {failed:?}");
    assert_unchanged(&before, &business_snapshot(&env).await, "folder rename");

    let failed = platform
        .set_folder_policy(folder, 0, "synthetic refused policy")
        .await;
    assert!(failed.is_err(), "policy: {failed:?}");
    assert_unchanged(&before, &business_snapshot(&env).await, "ACL change");

    // A producer with two audit rows: document.created would stage, the
    // version row cannot: the whole creation rolls back.
    let failed = platform
        .create_document(Platform::root(), "Synthetic refused", b"A refused body\n")
        .await;
    assert!(failed.is_err(), "create: {failed:?}");
    assert_unchanged(&before, &business_snapshot(&env).await, "document creation");

    let failed = platform.mark_read(&editor_ctx(), document, v1).await;
    assert!(failed.is_err(), "read confirmation: {failed:?}");
    let failed = platform
        .read_state(&reader_ctx(), document, v1, 0, ReadStateMutationKind::View)
        .await;
    assert!(failed.is_err(), "detail view: {failed:?}");
    assert_unchanged(&before, &business_snapshot(&env).await, "read state");

    let failed = platform
        .end_publication(document, v1, "synthetic refused end")
        .await;
    assert!(failed.is_err(), "publication end: {failed:?}");
    assert_unchanged(&before, &business_snapshot(&env).await, "publication end");

    // The relay sees nothing: no staged row, no registration, no alarm.
    let report = env.health(false).await;
    assert_eq!(report["produced"]["staged"], json!(baseline), "{report}");
    assert_eq!(report["produced"]["registered"], json!(baseline));
    assert_eq!(report["delivered"]["pending"], json!(baseline));
    assert_eq!(report["alarms"], json!([]), "{report}");
    exec(
        &env.doc_admin,
        "ALTER TABLE public.audit_outbox_events DROP CONSTRAINT acceptance_refuse_staging",
    )
    .await;

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
    let failed = platform
        .update_metadata(document, "unregistered", "synthetic unregistered metadata")
        .await;
    assert!(failed.is_err(), "metadata: {failed:?}");
    let failed = platform
        .create_folder(
            &editor_ctx(),
            FolderId::from_uuid(Uuid::now_v7()),
            Platform::root(),
            "Synthetic unregistered",
            "synthetic unregistered folder",
        )
        .await;
    assert!(failed.is_err(), "folder: {failed:?}");
    let failed = platform.mark_read(&editor_ctx(), document, v1).await;
    assert!(failed.is_err(), "read confirmation: {failed:?}");
    assert_unchanged(
        &before,
        &business_snapshot(&env).await,
        "a failed registration",
    );
    assert_eq!(staged_rows(&env).await.len(), baseline, "no staging row");
    exec(
        &env.doc_admin,
        "ALTER TABLE audit_relay.deliveries DROP CONSTRAINT acceptance_refuse_registration",
    )
    .await;

    // ------------------------------------------------------------------
    // 3. Contrast: the same operations commit once staging works, and the
    //    relay delivers exactly what committed.
    // ------------------------------------------------------------------
    platform
        .update_metadata(document, "accepted", "synthetic accepted metadata")
        .await
        .expect("metadata");
    platform
        .rename_folder(folder, "Synthetic F renamed", "synthetic accepted rename")
        .await
        .expect("rename");
    platform
        .set_folder_policy(folder, 0, "synthetic accepted policy")
        .await
        .expect("policy");
    assert!(
        platform
            .mark_read(&editor_ctx(), document, v1)
            .await
            .expect("read")
    );
    assert_ne!(
        business_snapshot(&env).await,
        before,
        "committed operations change the business state"
    );
    let staged = staged_rows(&env).await;
    assert_eq!(staged.len(), baseline + 4);
    assert_relay_posture_clean(&env).await;
    let relay = RunningRelay::start(&env);
    wait_until("the relay drains", CONVERGE, || drained(&env)).await;
    relay.stop().await;
    let store = env.store_client().await;
    let stored = assert_delivered_exactly_once(&env, &env.store_admin, &store).await;
    assert_eq!(stored.len(), baseline + 4, "nothing of the failed attempts");
    assert_eq!(assert_store_chain(&env.store_admin).await, baseline + 4);
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
}
