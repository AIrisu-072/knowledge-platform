//! Source side on the Document database (design §5, §10.1, §13; D4):
//! preflight, backfill without a gap, the separate ledger, the BEGIN ATOMIC
//! digest's column dependencies, atomic registration, staging and ledger
//! guards, the role matrix and posture, claim/lease fencing and the total
//! claim projection.

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use audit_core::{DocumentStagingProjection, IngestOutcome, IngestReceipt};
use audit_relay::ledger::{DeliveryLedger, FailureNote, Note};
use audit_relay::session::{StartupError, refuse_privileged_source};
use audit_relay::source::{RelayOutboxStore, RelayPolicy};
use audit_relay::{PreflightError, RelayMigrateError};
use outbox_delivery::{DeliveryError, ErrorCode, FenceResult, OutboxStore};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use support::*;
use uuid::Uuid;

const LEASE: Duration = Duration::from_secs(6);

fn outbox(pool: &PgPool) -> (RelayOutboxStore, Arc<DeliveryLedger>) {
    let ledger = Arc::new(DeliveryLedger::default());
    (
        RelayOutboxStore::new(pool.clone(), RelayPolicy::default(), ledger.clone()),
        ledger,
    )
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .expect("count")
}

#[tokio::test]
async fn preflight_refuses_a_database_without_the_document_schema() {
    let cluster = Cluster::start().await;
    let admin = &cluster.doc_admin;
    assert!(matches!(
        audit_relay::migrate(admin).await,
        Err(RelayMigrateError::Preflight(PreflightError::StagingMissing))
    ));
    exec(
        admin,
        "CREATE TABLE public.audit_outbox_events (event_id uuid PRIMARY KEY, data jsonb NOT NULL)",
    )
    .await;
    assert!(matches!(
        audit_relay::migrate(admin).await,
        Err(RelayMigrateError::Preflight(
            PreflightError::DocumentLedgerMissing
        ))
    ));
    exec(
        admin,
        "CREATE TABLE public._sqlx_migrations (version bigint PRIMARY KEY)",
    )
    .await;
    match audit_relay::migrate(admin).await {
        Err(RelayMigrateError::Preflight(PreflightError::Columns(missing))) => {
            assert!(missing.contains(&"resource_type".to_owned()), "{missing:?}");
            assert!(!missing.contains(&"data".to_owned()));
        }
        other => panic!("expected a column preflight failure, got {other:?}"),
    }
    // The SQL preflight inside the migration refuses too (no Rust preflight).
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.dangerous_set_table_name(audit_relay::MIGRATION_LEDGER);
    let error = migrator.run(admin).await.expect_err("SQL preflight");
    assert!(error.to_string().contains("preflight"), "{error}");
    assert_eq!(
        count(
            admin,
            "SELECT count(*) FROM pg_namespace WHERE nspname = 'audit_relay'"
        )
        .await,
        0,
        "nothing is left behind"
    );
}

#[tokio::test]
async fn backfill_and_concurrent_inserts_leave_no_unregistered_row() {
    let cluster = Cluster::start().await;
    let admin = cluster.doc_admin.clone();
    document_repository_postgres::migrate(&admin)
        .await
        .expect("document migrations");
    let ledger_before = count(&admin, "SELECT count(*) FROM _sqlx_migrations").await;
    // Existing rows, some carrying legacy attempt/delivered markers.
    let mut legacy = Vec::new();
    for n in 0..40 {
        let row = Staged::created();
        insert_staged(&admin, &row).await;
        if n % 10 == 0 {
            sqlx::query(
                "UPDATE public.audit_outbox_events SET attempt_count = 3, \
                 delivered_at = now() WHERE event_id = $1",
            )
            .bind(row.event_id)
            .execute(&admin)
            .await
            .expect("legacy marker");
            legacy.push(row.event_id);
        }
    }
    // A producer keeps inserting while the relay migration runs.
    let stop = Arc::new(AtomicBool::new(false));
    let inserted = Arc::new(AtomicUsize::new(0));
    let producer = {
        let (stop, inserted) = (stop.clone(), inserted.clone());
        let pool = support::connect_with_retry(&cluster.superuser_url(DOC_DB), 2).await;
        tokio::spawn(async move {
            while !stop.load(Ordering::SeqCst) {
                insert_staged(&pool, &Staged::created()).await;
                inserted.fetch_add(1, Ordering::SeqCst);
            }
        })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    audit_relay::migrate(&admin).await.expect("relay migration");
    let at_migration = inserted.load(Ordering::SeqCst);
    // Keep producing after the trigger exists.
    while inserted.load(Ordering::SeqCst) < at_migration + 20 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    stop.store(true, Ordering::SeqCst);
    producer.await.expect("producer task");

    let staged = count(&admin, "SELECT count(*) FROM public.audit_outbox_events").await;
    let registered = count(&admin, "SELECT count(*) FROM audit_relay.deliveries").await;
    assert_eq!(staged, 40 + inserted.load(Ordering::SeqCst) as i64);
    assert_eq!(registered, staged, "every staged row is registered");
    assert_eq!(
        count(
            &admin,
            "SELECT count(*) FROM public.audit_outbox_events o WHERE NOT EXISTS \
             (SELECT 1 FROM audit_relay.deliveries d WHERE d.event_id = o.event_id)"
        )
        .await,
        0,
        "zero unregistered"
    );
    let backfill = count(
        &admin,
        "SELECT count(*) FROM audit_relay.deliveries WHERE registration_kind = 'backfill'",
    )
    .await;
    let trigger = count(
        &admin,
        "SELECT count(*) FROM audit_relay.deliveries WHERE registration_kind = 'trigger'",
    )
    .await;
    assert!(backfill >= 40 && trigger >= 20, "{backfill} {trigger}");
    assert_eq!(backfill + trigger, registered);
    // Legacy markers are recorded, never trusted: every row is pending.
    for id in &legacy {
        let row = delivery(&admin, *id).await;
        assert_eq!(row["legacy_attempt_count"], json!(3));
        assert!(!row["legacy_delivered_at"].is_null());
        assert!(row["delivered_at"].is_null() && row["quarantined_at"].is_null());
        assert_eq!(row["attempt_count"], json!(0));
    }
    assert_eq!(
        count(
            &admin,
            "SELECT count(*) FROM audit_relay.deliveries \
             WHERE delivered_at IS NOT NULL OR quarantined_at IS NOT NULL"
        )
        .await,
        0
    );

    // The relay ledger is separate; Document's ledger and strict
    // compatibility check are untouched; re-running is a no-op.
    assert_eq!(
        count(&admin, "SELECT count(*) FROM _sqlx_migrations").await,
        ledger_before
    );
    document_repository_postgres::check_schema_compatibility(&admin)
        .await
        .expect("document compatibility holds");
    assert_eq!(
        count(
            &admin,
            "SELECT count(*) FROM audit_relay_sqlx_migrations WHERE success"
        )
        .await,
        1
    );
    audit_relay::migrate(&admin).await.expect("idempotent");
}

#[tokio::test]
async fn digest_function_tracks_digested_columns() {
    let env = DocEnv::start().await;
    let admin = env.admin();
    let before = Staged::created();
    insert_staged(admin, &before).await;
    for statement in [
        "ALTER TABLE public.audit_outbox_events DROP COLUMN data",
        "ALTER TABLE public.audit_outbox_events DROP COLUMN trace_id",
        "ALTER TABLE public.audit_outbox_events ALTER COLUMN subject TYPE varchar(4000)",
    ] {
        let error = sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(admin)
            .await
            .expect_err("a digested column cannot be dropped or retyped");
        let state = sqlstate(&error);
        assert!(
            state == "2BP01" || state == "0A000",
            "{statement}: {state} {error}"
        );
    }
    // Adding a nullable column is compatible: inserts register, digests hold.
    exec(
        admin,
        "ALTER TABLE public.audit_outbox_events ADD COLUMN future_note text NULL",
    )
    .await;
    let after = Staged::metadata_changed("added after a column");
    insert_staged(admin, &after).await;
    let intact: Vec<bool> = sqlx::query_scalar(
        "SELECT audit_relay.source_digest(o) = d.source_digest \
         FROM public.audit_outbox_events o JOIN audit_relay.deliveries d USING (event_id)",
    )
    .fetch_all(admin)
    .await
    .expect("digests");
    assert_eq!(intact, vec![true, true]);
    assert_eq!(
        delivery(admin, after.event_id).await["registration_kind"],
        json!("trigger")
    );
}

#[tokio::test]
async fn registration_failure_rolls_back_the_business_write() {
    let env = DocEnv::start().await;
    let admin = env.admin();
    exec(
        admin,
        "CREATE TABLE public.test_business (id uuid PRIMARY KEY)",
    )
    .await;
    // Make the registration INSERT fail (superuser test-only constraint).
    exec(
        admin,
        "ALTER TABLE audit_relay.deliveries ADD CONSTRAINT test_registration_fails \
         CHECK (registration_kind <> 'trigger') NOT VALID",
    )
    .await;
    let business = Uuid::now_v7();
    let row = Staged::created();
    let mut tx = admin.begin().await.expect("begin");
    sqlx::query("INSERT INTO public.test_business (id) VALUES ($1)")
        .bind(business)
        .execute(&mut *tx)
        .await
        .expect("business write");
    let staged = sqlx::query(
        "INSERT INTO public.audit_outbox_events (event_id, event_type, source, subject, \
             actor_identity_provider, actor_principal_id, resource_type, resource_id, result, \
             data, occurred_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'success', $9::text::jsonb, now())",
    )
    .bind(row.event_id)
    .bind(&row.event_type)
    .bind(&row.source)
    .bind(&row.subject)
    .bind(&row.actor_idp)
    .bind(&row.actor_pid)
    .bind(&row.resource_type)
    .bind(row.resource_id)
    .bind(row.data.to_string())
    .execute(&mut *tx)
    .await;
    assert_eq!(sqlstate(&staged.expect_err("registration fails")), "23514");
    drop(tx); // the producer rolls back
    assert_eq!(
        count(admin, "SELECT count(*) FROM public.test_business").await,
        0,
        "the business write did not commit"
    );
    assert_eq!(
        count(admin, "SELECT count(*) FROM public.audit_outbox_events").await,
        0
    );
    exec(
        admin,
        "ALTER TABLE audit_relay.deliveries DROP CONSTRAINT test_registration_fails",
    )
    .await;
    let mut tx = admin.begin().await.expect("begin");
    sqlx::query("INSERT INTO public.test_business (id) VALUES ($1)")
        .bind(business)
        .execute(&mut *tx)
        .await
        .expect("business write");
    tx.commit().await.expect("commit");
    insert_staged(admin, &row).await;
    assert_eq!(
        delivery(admin, row.event_id).await["registration_kind"],
        json!("trigger")
    );
}

#[tokio::test]
async fn staging_and_ledger_guards_refuse_mutation() {
    let env = DocEnv::start().await;
    let admin = env.admin();
    let row = Staged::created();
    insert_staged(admin, &row).await;
    let id = row.event_id;

    // Staging is append-only, also for the owner/superuser.
    for (statement, states) in [
        (
            format!(
                "UPDATE public.audit_outbox_events SET result = 'failure' WHERE event_id = '{id}'"
            ),
            &["55000"][..],
        ),
        (
            format!("DELETE FROM public.audit_outbox_events WHERE event_id = '{id}'"),
            &["55000"][..],
        ),
        (
            "TRUNCATE public.audit_outbox_events".to_owned(),
            &["55000", "0A000"][..],
        ),
    ] {
        let error = sqlx::query(sqlx::AssertSqlSafe(statement.clone()))
            .execute(admin)
            .await
            .expect_err("staging guard");
        assert!(
            states.contains(&sqlstate(&error).as_str()),
            "{statement}: {error}"
        );
    }

    // Deliveries: no DELETE/TRUNCATE, immutable registration columns,
    // set-once receipt and quarantine.
    let refused = |statement: String| async move {
        let error = sqlx::query(sqlx::AssertSqlSafe(statement.clone()))
            .execute(admin)
            .await
            .expect_err("deliveries guard");
        let state = sqlstate(&error);
        assert!(state == "55000" || state == "0A000", "{statement}: {error}");
    };
    refused(format!(
        "UPDATE audit_relay.deliveries SET source_digest = '\\x{}' WHERE event_id = '{id}'",
        "00".repeat(32)
    ))
    .await;
    refused(format!(
        "UPDATE audit_relay.deliveries SET registered_at = now() - interval '1 day' \
         WHERE event_id = '{id}'"
    ))
    .await;
    refused(format!(
        "UPDATE audit_relay.deliveries SET registration_kind = 'repair' WHERE event_id = '{id}'"
    ))
    .await;
    refused(format!(
        "DELETE FROM audit_relay.deliveries WHERE event_id = '{id}'"
    ))
    .await;
    refused("TRUNCATE audit_relay.deliveries".to_owned()).await;
    // Set a receipt once (allowed from NULL), then try to change it.
    exec(
        admin,
        &format!(
            "UPDATE audit_relay.deliveries SET delivered_at = now(), store_seq = 7, \
             store_envelope_digest = '\\x{}', store_outcome = 'stored', \
             store_recovery_epoch = 1 WHERE event_id = '{id}'",
            "11".repeat(32)
        ),
    )
    .await;
    refused(format!(
        "UPDATE audit_relay.deliveries SET store_seq = 8 WHERE event_id = '{id}'"
    ))
    .await;
    let other = Staged::created();
    insert_staged(admin, &other).await;
    exec(
        admin,
        &format!(
            "UPDATE audit_relay.deliveries SET quarantined_at = now(), quarantine_code = 'conflict' \
             WHERE event_id = '{}'",
            other.event_id
        ),
    )
    .await;
    refused(format!(
        "UPDATE audit_relay.deliveries SET quarantined_at = NULL, quarantine_code = NULL \
         WHERE event_id = '{}'",
        other.event_id
    ))
    .await;
    // A registration always starts pending.
    exec(
        admin,
        "ALTER TABLE public.audit_outbox_events DISABLE TRIGGER audit_relay_register",
    )
    .await;
    let unregistered = Staged::created();
    insert_staged(admin, &unregistered).await;
    exec(
        admin,
        "ALTER TABLE public.audit_outbox_events ENABLE TRIGGER audit_relay_register",
    )
    .await;
    refused(format!(
        "INSERT INTO audit_relay.deliveries (event_id, registered_at, registration_kind, \
             source_digest, commitment_salt, available_at, attempt_count) \
         VALUES ('{}', now(), 'repair', '\\x{}', '\\x{}', now(), 3)",
        unregistered.event_id,
        "22".repeat(32),
        "33".repeat(32)
    ))
    .await;
    // Policy changes bump the revision; history is append-only.
    refused("UPDATE audit_relay.delivery_policy SET max_attempts = 20".to_owned()).await;
    refused("DELETE FROM audit_relay.delivery_policy".to_owned()).await;
    refused("UPDATE audit_relay.delivery_progress SET success_generation = -1".to_owned()).await;
    refused("TRUNCATE audit_relay.delivery_history".to_owned()).await;
}

#[tokio::test]
async fn role_matrix_posture_and_session_refusal() {
    let env = DocEnv::start().await;
    let admin = env.admin();
    insert_staged(admin, &Staged::created()).await;
    let worker = &env.worker.pool;
    let operator = &env.operator.pool;
    let nobody = doc_login(&env.cluster, "relay_nobody", &[]).await;

    for (pool, statement) in [
        (worker, "SELECT count(*) FROM audit_relay.deliveries"),
        (worker, "SELECT count(*) FROM public.audit_outbox_events"),
        (worker, "SELECT count(*) FROM audit_relay.delivery_history"),
        (
            worker,
            "SELECT audit_relay.replay('00000000-0000-7000-8000-000000000001', 1, 1)",
        ),
        (worker, "SELECT audit_relay.register_missing(10)"),
        (
            operator,
            "SELECT * FROM audit_relay.claim('00000000-0000-7000-8000-000000000001', 1, 6000)",
        ),
        (operator, "SELECT audit_relay.reap_exhausted(1)"),
        (operator, "SELECT count(*) FROM audit_relay.deliveries"),
        (&nobody.pool, "SELECT audit_relay.status()"),
        (
            &nobody.pool,
            "SELECT audit_relay.source_digest(o) FROM public.audit_outbox_events o",
        ),
        (
            worker,
            "SELECT audit_relay.commitment('\\x00'::bytea, '\\x00'::bytea)",
        ),
        // The forecast projection is staged content: the worker's only.
        (
            operator,
            "SELECT * FROM audit_relay.preview_pending(NULL, 10)",
        ),
    ] {
        let error = sqlx::query(sqlx::AssertSqlSafe(statement))
            .fetch_all(pool)
            .await
            .expect_err("outside the role matrix");
        assert_eq!(sqlstate(&error), "42501", "{statement}");
    }
    // Allowed calls work, and temp-table shadowing does not redirect them.
    exec(
        worker,
        "CREATE TEMP TABLE deliveries (event_id uuid); \
         CREATE TEMP TABLE audit_outbox_events (event_id uuid)",
    )
    .await;
    let status: Value = sqlx::query_scalar("SELECT audit_relay.status()")
        .fetch_one(worker)
        .await
        .expect("worker status");
    assert_eq!(status["registered"], json!(1));
    let status: Value = sqlx::query_scalar("SELECT audit_relay.status()")
        .fetch_one(operator)
        .await
        .expect("operator status");
    assert_eq!(status["staged"], json!(1));

    // Posture is clean after roles.sql; drift is reported.
    let posture = audit_relay::session::posture(worker)
        .await
        .expect("posture");
    assert_eq!(posture, vec![], "{posture:?}");
    exec(
        admin,
        "GRANT EXECUTE ON FUNCTION audit_relay.status() TO PUBLIC",
    )
    .await;
    exec(
        admin,
        "GRANT EXECUTE ON FUNCTION audit_relay.replay(uuid, bigint, bigint) TO audit_relay_worker",
    )
    .await;
    exec(
        admin,
        "ALTER TABLE public.audit_outbox_events DISABLE TRIGGER audit_relay_append_only",
    )
    .await;
    let codes: Vec<String> = audit_relay::session::posture(worker)
        .await
        .expect("posture")
        .into_iter()
        .map(|v| v.violation)
        .collect();
    for expected in ["public_execute", "acl_unexpected", "trigger_missing"] {
        assert!(codes.contains(&expected.to_owned()), "{codes:?}");
    }
    assert!(matches!(
        audit_relay::session::require_posture(worker).await,
        Err(StartupError::PostureInvalid { .. })
    ));
    exec(
        admin,
        "REVOKE EXECUTE ON FUNCTION audit_relay.status() FROM PUBLIC",
    )
    .await;
    exec(
        admin,
        "REVOKE EXECUTE ON FUNCTION audit_relay.replay(uuid, bigint, bigint) FROM audit_relay_worker",
    )
    .await;
    exec(
        admin,
        "ALTER TABLE public.audit_outbox_events ENABLE TRIGGER audit_relay_append_only",
    )
    .await;
    // Session refusal (design §10.1): superuser and owner members.
    let owner_member = doc_login(&env.cluster, "relay_owner_member", &["audit_relay_owner"]).await;
    assert_eq!(
        refuse_privileged_source(admin).await,
        Err(StartupError::Privileged(audit_relay::session::Side::Source))
    );
    assert_eq!(
        refuse_privileged_source(&owner_member.pool).await,
        Err(StartupError::Privileged(audit_relay::session::Side::Source))
    );
    refuse_privileged_source(worker)
        .await
        .expect("worker session");
    refuse_privileged_source(operator)
        .await
        .expect("operator session");

    // Any other role holding the owner's rights (e.g. the staging owner
    // kept as a member after a non-superuser migrate) bypasses every guard:
    // the posture reports it until the membership is revoked (design §5.3).
    let violations = audit_relay::session::posture(worker)
        .await
        .expect("posture");
    assert!(
        violations
            .iter()
            .any(|v| v.violation == "owner_member" && v.object == owner_member.role),
        "{violations:?}"
    );
    exec(
        admin,
        &format!("REVOKE audit_relay_owner FROM {}", owner_member.role),
    )
    .await;
    assert_eq!(posture_codes(worker).await, Vec::<String>::new());

    // Drift that would let a capability login read staged content or write
    // the relay tables, a zero timeout, row security hiding staged rows, and
    // registration/guard triggers replaced by other definitions.
    let worker_role = &env.worker.role;
    let operator_role = &env.operator.role;
    let scenarios: Vec<(String, String, &str)> = vec![
        (
            "GRANT SELECT ON public.audit_outbox_events TO audit_relay_worker".into(),
            "REVOKE SELECT ON public.audit_outbox_events FROM audit_relay_worker".into(),
            "staging_read",
        ),
        (
            format!("GRANT SELECT (data) ON public.audit_outbox_events TO {operator_role}"),
            format!("REVOKE SELECT (data) ON public.audit_outbox_events FROM {operator_role}"),
            "staging_read",
        ),
        (
            format!("GRANT pg_read_all_data TO {operator_role}"),
            format!("REVOKE pg_read_all_data FROM {operator_role}"),
            "staging_read",
        ),
        (
            format!("GRANT pg_write_all_data TO {worker_role}"),
            format!("REVOKE pg_write_all_data FROM {worker_role}"),
            "table_access",
        ),
        // Column grants are invisible to the table ACL and to
        // has_table_privilege: the commitment salts, or one receipt column.
        (
            format!("GRANT SELECT (commitment_salt) ON audit_relay.deliveries TO {operator_role}"),
            format!(
                "REVOKE SELECT (commitment_salt) ON audit_relay.deliveries FROM {operator_role}"
            ),
            "column_privilege",
        ),
        (
            "GRANT UPDATE (store_seq) ON audit_relay.deliveries TO PUBLIC".into(),
            "REVOKE UPDATE (store_seq) ON audit_relay.deliveries FROM PUBLIC".into(),
            "column_privilege",
        ),
        (
            format!("ALTER ROLE {worker_role} IN DATABASE {DOC_DB} SET statement_timeout = 0"),
            format!("ALTER ROLE {worker_role} IN DATABASE {DOC_DB} SET statement_timeout = '30s'"),
            "login_timeouts_missing",
        ),
        (
            "ALTER TABLE public.audit_outbox_events ENABLE ROW LEVEL SECURITY".into(),
            "ALTER TABLE public.audit_outbox_events DISABLE ROW LEVEL SECURITY".into(),
            "row_security",
        ),
        (
            "CREATE FUNCTION public.noop_trigger() RETURNS trigger LANGUAGE plpgsql \
             AS $$BEGIN RETURN NULL; END$$; \
             DROP TRIGGER audit_relay_register ON public.audit_outbox_events; \
             CREATE TRIGGER audit_relay_register AFTER INSERT ON public.audit_outbox_events \
             FOR EACH ROW EXECUTE FUNCTION public.noop_trigger()"
                .into(),
            "DROP TRIGGER audit_relay_register ON public.audit_outbox_events; \
             DROP FUNCTION public.noop_trigger(); \
             CREATE TRIGGER audit_relay_register AFTER INSERT ON public.audit_outbox_events \
             FOR EACH ROW EXECUTE FUNCTION audit_relay.register_staged()"
                .into(),
            "trigger_missing",
        ),
        (
            "DROP TRIGGER audit_relay_append_only ON public.audit_outbox_events; \
             CREATE TRIGGER audit_relay_append_only BEFORE DELETE ON public.audit_outbox_events \
             FOR EACH ROW EXECUTE FUNCTION audit_relay.guard_staging()"
                .into(),
            "DROP TRIGGER audit_relay_append_only ON public.audit_outbox_events; \
             CREATE TRIGGER audit_relay_append_only BEFORE UPDATE OR DELETE \
             ON public.audit_outbox_events \
             FOR EACH ROW EXECUTE FUNCTION audit_relay.guard_staging()"
                .into(),
            "trigger_missing",
        ),
        (
            "DROP TRIGGER deliveries_writer ON audit_relay.deliveries".into(),
            "CREATE TRIGGER deliveries_writer BEFORE INSERT OR UPDATE OR DELETE \
             ON audit_relay.deliveries \
             FOR EACH ROW EXECUTE FUNCTION audit_relay.guard_writer()"
                .into(),
            "trigger_missing",
        ),
    ];
    for (break_sql, repair_sql, violation) in &scenarios {
        exec(admin, break_sql).await;
        let codes = posture_codes(worker).await;
        assert!(
            codes.contains(&(*violation).to_owned()),
            "{break_sql}: {codes:?}"
        );
        exec(admin, repair_sql).await;
        assert_eq!(
            posture_codes(worker).await,
            Vec::<String>::new(),
            "{repair_sql}"
        );
    }
}

/// A Document login outside the relay's role matrix (no capability role,
/// e.g. a reporting or maintenance login) that holds the predefined data
/// roles can write the relay tables directly and forge a first delivery
/// receipt. The posture reports it, and the guards refuse a receipt that
/// does not come through the relay's definer functions, even with the
/// transition marker set. The data roles are reported for a login that can
/// connect to the Document database, file and program access for any login.
#[tokio::test]
async fn a_document_login_with_data_bypass_roles_cannot_forge_a_receipt() {
    let env = DocEnv::start().await;
    let admin = env.admin();
    let row = Staged::created();
    insert_staged(admin, &row).await;
    let worker = &env.worker.pool;
    assert_eq!(posture_codes(worker).await, Vec::<String>::new());

    let forger = doc_login(
        &env.cluster,
        "document_reporting",
        &["pg_read_all_data", "pg_write_all_data"],
    )
    .await;
    let found: BTreeSet<(String, String)> = audit_relay::session::posture(worker)
        .await
        .expect("posture")
        .into_iter()
        .map(|v| (v.violation, v.object))
        .collect();
    for expected in [
        ("table_access", forger.role.clone()),
        (
            "predefined_role_member",
            format!("{}:pg_read_all_data", forger.role),
        ),
        (
            "predefined_role_member",
            format!("{}:pg_write_all_data", forger.role),
        ),
    ] {
        assert!(
            found.contains(&(expected.0.to_owned(), expected.1.clone())),
            "{expected:?}: {found:?}"
        );
    }
    // The forged first receipt (and a forged history row) is refused, also
    // with the transition marker a definer path would set.
    for statement in [
        format!(
            "UPDATE audit_relay.deliveries SET delivered_at = now(), store_seq = 7, \
             store_envelope_digest = '\\x{}', store_outcome = 'stored', \
             store_recovery_epoch = 1 WHERE event_id = '{}'",
            "11".repeat(32),
            row.event_id
        ),
        format!(
            "SET audit_relay.transition = 'repair_reset_missing'; \
             UPDATE audit_relay.deliveries SET delivered_at = now(), store_seq = 7, \
             store_envelope_digest = '\\x{}', store_outcome = 'stored', \
             store_recovery_epoch = 1 WHERE event_id = '{}'",
            "11".repeat(32),
            row.event_id
        ),
        format!(
            "UPDATE audit_relay.deliveries SET quarantined_at = now(), \
             quarantine_code = 'conflict' WHERE event_id = '{}'",
            row.event_id
        ),
        "UPDATE audit_relay.delivery_progress SET success_generation = success_generation + 1"
            .to_owned(),
        "UPDATE audit_relay.delivery_policy SET max_attempts = 1, revision = revision + 1"
            .to_owned(),
    ] {
        let error = sqlx::raw_sql(sqlx::AssertSqlSafe(statement.clone()))
            .execute(&forger.pool)
            .await
            .expect_err("forged write");
        assert_eq!(sqlstate(&error), "42501", "{statement}: {error}");
    }
    let delivered: Option<i64> =
        sqlx::query_scalar("SELECT store_seq FROM audit_relay.deliveries WHERE event_id = $1")
            .bind(row.event_id)
            .fetch_one(admin)
            .await
            .expect("delivery");
    assert_eq!(delivered, None, "no receipt was forged");
    exec(
        admin,
        &format!(
            "REVOKE pg_read_all_data, pg_write_all_data FROM {}",
            forger.role
        ),
    )
    .await;
    assert_eq!(posture_codes(worker).await, Vec::<String>::new());

    // File and program access bypasses everything at the OS level.
    for role in [
        "pg_read_server_files",
        "pg_write_server_files",
        "pg_execute_server_program",
    ] {
        exec(admin, &format!("GRANT {role} TO {}", forger.role)).await;
        let found: Vec<(String, String)> = audit_relay::session::posture(worker)
            .await
            .expect("posture")
            .into_iter()
            .map(|v| (v.violation, v.object))
            .collect();
        assert_eq!(
            found,
            vec![(
                "predefined_role_member".to_owned(),
                format!("{}:{role}", forger.role)
            )]
        );
        exec(admin, &format!("REVOKE {role} FROM {}", forger.role)).await;
    }
    assert_eq!(posture_codes(worker).await, Vec::<String>::new());

    // A login that cannot connect to the Document database cannot use the
    // data bypass roles here: neither table_access nor predefined_role_member
    // (it is reported once it can connect). File and program access is
    // reported whether or not it can connect.
    exec(
        admin,
        &format!(
            "GRANT CONNECT ON DATABASE {DOC_DB} TO {}, {}, {}; \
             REVOKE CONNECT ON DATABASE {DOC_DB} FROM PUBLIC; \
             CREATE ROLE document_elsewhere LOGIN PASSWORD '{PASSWORD}' NOSUPERUSER; \
             GRANT pg_read_all_data, pg_write_all_data TO document_elsewhere",
            env.worker.role, env.operator.role, forger.role
        ),
    )
    .await;
    assert_eq!(posture_pairs(worker).await, vec![]);
    exec(admin, "GRANT pg_write_server_files TO document_elsewhere").await;
    assert_eq!(
        posture_pairs(worker).await,
        vec![(
            "predefined_role_member".to_owned(),
            "document_elsewhere:pg_write_server_files".to_owned()
        )]
    );
    // A REPLICATION login reads the whole cluster through the replication
    // protocol: reported whether or not it can connect.
    exec(
        admin,
        "REVOKE pg_write_server_files FROM document_elsewhere; \
         ALTER ROLE document_elsewhere REPLICATION",
    )
    .await;
    assert_eq!(
        posture_pairs(worker).await,
        vec![(
            "replication_login".to_owned(),
            "document_elsewhere".to_owned()
        )]
    );
    exec(
        admin,
        &format!(
            "ALTER ROLE document_elsewhere NOREPLICATION; \
             GRANT CONNECT ON DATABASE {DOC_DB} TO document_elsewhere"
        ),
    )
    .await;
    assert_eq!(
        posture_pairs(worker).await,
        vec![
            (
                "predefined_role_member".to_owned(),
                "document_elsewhere:pg_read_all_data".to_owned()
            ),
            (
                "predefined_role_member".to_owned(),
                "document_elsewhere:pg_write_all_data".to_owned()
            ),
            ("table_access".to_owned(), "document_elsewhere".to_owned()),
        ]
    );
    exec(
        admin,
        &format!(
            "REVOKE CONNECT ON DATABASE {DOC_DB} FROM document_elsewhere; \
             DROP ROLE document_elsewhere; GRANT CONNECT ON DATABASE {DOC_DB} TO PUBLIC"
        ),
    )
    .await;
    assert_eq!(posture_codes(worker).await, Vec::<String>::new());
}

async fn posture_pairs(pool: &PgPool) -> Vec<(String, String)> {
    audit_relay::session::posture(pool)
        .await
        .expect("posture")
        .into_iter()
        .map(|v| (v.violation, v.object))
        .collect()
}

async fn posture_codes(pool: &PgPool) -> Vec<String> {
    audit_relay::session::posture(pool)
        .await
        .expect("posture")
        .into_iter()
        .map(|v| v.violation)
        .collect()
}

#[tokio::test]
async fn claims_are_leased_and_stale_tokens_are_fenced() {
    let env = DocEnv::start().await;
    let admin = env.admin();
    let row = Staged::created();
    insert_staged(admin, &row).await;
    let id = row.event_id;
    let (store, ledger) = outbox(&env.worker.pool);
    store.verify_policy().await.expect("policy matches");

    let owner = Uuid::now_v7();
    let claims = store.claim(owner, 4, LEASE).await.expect("claim");
    assert_eq!(claims.len(), 1);
    let first = &claims[0];
    assert_eq!(first.envelope.event_id, id);
    assert_eq!(first.envelope.aggregate_type, "audit_outbox_events");
    assert_eq!(first.envelope.aggregate_id, id);
    assert_eq!((first.attempt, first.attempt_limit), (1, 16));
    assert!(
        store
            .claim(owner, 4, LEASE)
            .await
            .expect("claim")
            .is_empty()
    );
    let t1 = first.lease_token;
    assert_eq!(
        store.renew(id, Uuid::now_v7(), LEASE).await,
        Ok(FenceResult::Lost)
    );
    assert_eq!(store.renew(id, t1, LEASE).await, Ok(FenceResult::Updated));
    // No receipt in the ledger: the ack is refused, never invented.
    assert_eq!(
        store.settle_success(id, t1).await,
        Err(DeliveryError::StoreUnknown)
    );

    // FORCED_BY_TEST_SQL: expire the lease; a new claim gets a new token.
    exec(
        admin,
        &format!(
            "UPDATE audit_relay.deliveries SET lease_expires_at = clock_timestamp() - \
             interval '1 second' WHERE event_id = '{id}'"
        ),
    )
    .await;
    let second = store.claim(owner, 4, LEASE).await.expect("reclaim");
    let t2 = second[0].lease_token;
    assert_ne!(t1, t2);
    assert_eq!(second[0].attempt, 2);
    let receipt = IngestReceipt {
        seq: 1,
        envelope_digest: [9; 32],
        outcome: IngestOutcome::Stored,
        adapter_version: 1,
    };
    ledger.record(
        id,
        t1,
        Note::Receipt {
            receipt,
            store_epoch: 1,
        },
    );
    assert_eq!(store.renew(id, t1, LEASE).await, Ok(FenceResult::Lost));
    assert_eq!(store.settle_success(id, t1).await, Ok(FenceResult::Lost));
    assert_eq!(
        store
            .settle_failure(
                id,
                t1,
                ErrorCode::InvalidEnvelope,
                true,
                Duration::from_secs(1)
            )
            .await,
        Ok(FenceResult::Lost)
    );
    assert!(delivery(admin, id).await["quarantined_at"].is_null());

    // An outage on the current token returns the attempt and holds the row.
    ledger.record(
        id,
        t2,
        Note::Failure(FailureNote::outage("store_connection", false)),
    );
    assert_eq!(
        store
            .settle_failure(
                id,
                t2,
                ErrorCode::DeliveryUnknown,
                false,
                Duration::from_secs(1)
            )
            .await,
        Ok(FenceResult::Updated)
    );
    let held = delivery(admin, id).await;
    assert_eq!(held["attempt_count"], json!(1), "attempt returned");
    assert_eq!(held["last_outage_code"], json!("store_connection"));
    assert_eq!(
        held["outage_streak"],
        json!(0),
        "connection outages never count"
    );
    assert!(held["lease_token"].is_null() && held["quarantined_at"].is_null());

    // Policy pinning and argument bounds.
    let other = RelayOutboxStore::new(
        env.worker.pool.clone(),
        RelayPolicy {
            max_attempts: 8,
            ..RelayPolicy::default()
        },
        ledger.clone(),
    );
    assert_eq!(
        other.verify_policy().await,
        Err(DeliveryError::PolicyMismatch)
    );
    assert_eq!(
        store.claim(owner, 1, Duration::from_millis(500)).await,
        Err(DeliveryError::InvalidConfig)
    );
    let error = sqlx::query("SELECT * FROM audit_relay.claim($1, 1, 500)")
        .bind(owner)
        .fetch_all(&env.worker.pool)
        .await
        .expect_err("lease outside the policy");
    assert_eq!(sqlstate(&error), "22023");
}

#[tokio::test]
async fn claim_projection_is_total_and_never_carries_the_reason() {
    let env = DocEnv::start().await;
    let admin = env.admin();
    let reason = "日本語の理由文 secret-reason-text";
    let scalar = Staged::created().with_data(json!("just a string"));
    let array = Staged::created().with_data(json!([1, {"reason": "x"}]));
    let null = Staged::created().with_data(Value::Null);
    let mut long_subject = Staged::created();
    long_subject.subject = "s".repeat(2_000);
    let big_data = Staged::created().with_data(json!({"documentId": "x".repeat(17_000)}));
    let numeric_reason = Staged::withdrawn("x").with_data({
        let mut data = Staged::withdrawn("x").data;
        data["reason"] = json!(42);
        data
    });
    let with_reason = Staged::withdrawn(reason);
    let tampered_oversize = Staged::created().with_data(json!({"documentId": "y".repeat(17_000)}));
    for row in [
        &scalar,
        &array,
        &null,
        &long_subject,
        &big_data,
        &numeric_reason,
        &with_reason,
        &tampered_oversize,
    ] {
        insert_staged(admin, row).await;
    }
    // FORCED_BY_TEST_SQL: an owner bypasses the guard and edits a row.
    force(
        admin,
        &format!(
            "UPDATE public.audit_outbox_events SET result = 'failure' WHERE event_id = '{}'",
            tampered_oversize.event_id
        ),
    )
    .await;

    // Claim in a session with a non-UTC TimeZone.
    let mut tx = env.worker.pool.begin().await.expect("begin");
    sqlx::query("SET LOCAL TIME ZONE 'Asia/Tokyo'")
        .execute(&mut *tx)
        .await
        .expect("tz");
    let rows = sqlx::query("SELECT event_id, projection FROM audit_relay.claim($1, 32, 6000)")
        .bind(Uuid::now_v7())
        .fetch_all(&mut *tx)
        .await
        .expect("claim never raises on row content");
    tx.commit().await.expect("commit");
    assert_eq!(rows.len(), 8);
    let mut projections = std::collections::HashMap::new();
    for row in &rows {
        let id: Uuid = row.get("event_id");
        let projection: Value = row.get("projection");
        let text = projection.to_string();
        assert!(!text.contains("secret-reason-text"), "reason text leaked");
        assert_eq!(
            projection.as_object().expect("object").len(),
            20,
            "every key present"
        );
        let parsed: DocumentStagingProjection =
            serde_json::from_value(projection.clone()).expect("projection contract");
        projections.insert(id, (projection, parsed));
    }
    let get = |row: &Staged| &projections[&row.event_id];

    let (_, p) = get(&scalar);
    assert_eq!(
        (p.data.clone(), p.data_kind.as_str(), p.oversize),
        (None, "string", false)
    );
    assert!(p.source_intact);
    assert_eq!(audit_relay::health::forecast_code(p), "invalid_field");
    let (_, p) = get(&array);
    assert_eq!((p.data.clone(), p.data_kind.as_str()), (None, "array"));
    let (_, p) = get(&null);
    assert_eq!(p.data_kind, "null");
    let (_, p) = get(&long_subject);
    assert!(p.oversize && p.subject.is_empty() && p.data.is_none());
    assert_eq!(
        audit_relay::health::forecast_code(p),
        "source_row_too_large"
    );
    let (_, p) = get(&big_data);
    assert!(p.oversize);
    let (_, p) = get(&numeric_reason);
    assert_eq!(
        (p.reason_kind.as_deref(), p.reason_bytes),
        (Some("number"), None)
    );
    assert_eq!(audit_relay::health::forecast_code(p), "reason_not_string");
    let (raw, p) = get(&with_reason);
    assert_eq!(p.reason_kind.as_deref(), Some("string"));
    assert_eq!(p.reason_bytes, Some(reason.len() as i64));
    assert!(raw["data"].get("reason").is_none());
    assert_eq!(
        p.occurred_at, OCCURRED,
        "UTC rendering, session TimeZone ignored"
    );
    assert_eq!(p.registration_kind, "trigger");
    assert!(p.source_commitment.len() == 64 && p.source_intact);
    assert_eq!(audit_relay::health::forecast_code(p), "deliverable");
    let (_, p) = get(&tampered_oversize);
    assert!(
        p.oversize && !p.source_intact,
        "source_intact covers oversize rows"
    );
    assert_eq!(
        audit_relay::health::forecast_code(p),
        "source_digest_mismatch",
        "a source mismatch wins over oversize"
    );
}
