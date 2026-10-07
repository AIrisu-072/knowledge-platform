//! Backup/restore contract and recovery mode (design §11): pg_dump and
//! pg_restore of the Store database into a new database in the same
//! cluster, the fingerprint gate, the three allowed recovery functions, the
//! posture gate, begin_recovery_epoch and rebind_fingerprint.

mod support;

use std::path::PathBuf;
use std::time::Duration;

use audit_core::{
    AuditStore, ChainVerdict, Checkpoint, IngestOutcome, RecoveryRecord, StoreError,
    assess_recovery,
};
use audit_store_postgres::admin::{AccessChange, AccessOperation, AuditAdmin, parse_utc_text};
use audit_store_postgres::files::{ExportRequest, export_identity_chain_recovery, export_to_dir};
use audit_store_postgres::{AdminError, PRIVILEGES_SQL, PostgresAuditStore};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use support::*;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(10);

fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("audit-store-{name}-{}", Uuid::now_v7().simple()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn pool(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(4)
        .connect(url)
        .await
        .expect("connect to restored database")
}

/// pg_dump of the store database and pg_restore into `target`, inside the
/// container (design §11: -Fc, --exit-on-error --single-transaction, no
/// --no-owner/--no-privileges).
async fn dump_and_restore(db: &TestDb, dump: &str, target: Option<&str>) {
    let (code, output) = db
        .docker_exec(&[
            "pg_dump", "-U", "postgres", "-Fc", "-d", STORE_DB, "-f", dump,
        ])
        .await;
    assert_eq!(code, 0, "pg_dump: {output}");
    if let Some(target) = target {
        restore(db, dump, target).await;
    }
}

async fn restore(db: &TestDb, dump: &str, target: &str) {
    let (code, output) = db
        .docker_exec(&["createdb", "-U", "postgres", target])
        .await;
    assert_eq!(code, 0, "createdb: {output}");
    let (code, output) = db
        .docker_exec(&[
            "pg_restore",
            "-U",
            "postgres",
            "--exit-on-error",
            "--single-transaction",
            "-d",
            target,
            dump,
        ])
        .await;
    assert_eq!(code, 0, "pg_restore: {output}");
}

struct Restored {
    admin: PgPool,
    relay: PostgresAuditStore,
    relay_admin: AuditAdmin,
    reader: AuditAdmin,
    verifier: AuditAdmin,
    administrator: AuditAdmin,
    maintainer: AuditAdmin,
    dba: AuditAdmin,
}

async fn connect_restored(db: &TestDb, cast: &Cast, target: &str) -> Restored {
    let url = |login: &Login| db.url(&login.role, target);
    let relay_pool = pool(&url(&cast.relay)).await;
    Restored {
        admin: pool(&db.superuser_url(target)).await,
        relay: PostgresAuditStore::new(relay_pool.clone(), TIMEOUT)
            .await
            .expect("relay"),
        relay_admin: AuditAdmin::connect(relay_pool).await.expect("relay admin"),
        reader: AuditAdmin::connect(pool(&url(&cast.reader)).await)
            .await
            .expect("reader"),
        verifier: AuditAdmin::connect(pool(&url(&cast.verifier)).await)
            .await
            .expect("verifier"),
        administrator: AuditAdmin::connect(pool(&url(&cast.admin)).await)
            .await
            .expect("admin"),
        maintainer: AuditAdmin::connect(pool(&url(&cast.maintainer)).await)
            .await
            .expect("maintainer"),
        dba: AuditAdmin::connect_owner(pool(&url(&cast.dba)).await)
            .await
            .expect("dba"),
    }
}

async fn ingest(store: &PostgresAuditStore, count: u8) -> Vec<(Uuid, i64)> {
    let mut out = Vec::new();
    for n in 0..count {
        let id = Uuid::now_v7();
        let receipt = store
            .ingest(&document_created(id, Uuid::now_v7(), OCCURRED, n))
            .await
            .expect("stored");
        out.push((id, receipt.seq));
    }
    out
}

#[tokio::test]
async fn restore_enters_recovery_mode_and_a_new_epoch_continues_the_chain() {
    let db = TestDb::start().await;
    if db.container.is_none() {
        eprintln!("skipped: the restore test runs pg_dump inside the container");
        return;
    }
    let cast = Cast::new(&db).await;
    let store = PostgresAuditStore::new(cast.relay.pool.clone(), TIMEOUT)
        .await
        .expect("relay");
    ingest(&store, 4).await;
    let verifier = cast.verifier.admin().await;
    let before_backup = verifier.checkpoint().await.expect("checkpoint");
    let c1 = before_backup.checkpoint().expect("c1");

    dump_and_restore(&db, "/tmp/store.dump", None).await;
    let dump_head = head(&db.admin).await.0;
    assert_eq!(dump_head, before_backup.verified_seq);
    // Delivered after the backup: lost by the restore.
    let lost = ingest(&store, 2).await;
    let relay_max_seq = lost.last().expect("lost").1;
    let after_backup = verifier.checkpoint().await.expect("checkpoint");
    let c2 = after_backup.checkpoint().expect("c2");
    assert_eq!(c2.seq, relay_max_seq);

    restore(&db, "/tmp/store.dump", "audit_store_restored").await;
    let r = connect_restored(&db, &cast, "audit_store_restored").await;
    let restored_head = head(&r.admin).await;
    assert_eq!(restored_head.0, dump_head);

    // Every publication function refuses with store_recovery_required.
    let late = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 9);
    assert_eq!(
        r.relay.ingest(&late).await,
        Err(StoreError::RecoveryRequired)
    );
    assert_eq!(r.relay.probe().await, Err(StoreError::RecoveryRequired));
    let recovery = AdminError::RecoveryRequired;
    assert_eq!(
        r.reader
            .open_access(AccessOperation::Export, &json!({}), 10, 1)
            .await
            .expect_err("open_access"),
        recovery
    );
    assert_eq!(
        r.reader.read_page(&"0".repeat(64), 0).await,
        Err(recovery.clone())
    );
    assert_eq!(
        r.reader.close_access(&"0".repeat(64), 0, &[]).await,
        Err(recovery.clone())
    );
    assert_eq!(r.verifier.verify(None, None).await, Err(recovery.clone()));
    assert_eq!(r.verifier.checkpoint().await, Err(recovery.clone()));
    assert_eq!(
        r.administrator
            .change_access(ISSUER, "reader-1", "verify", AccessChange::Grant)
            .await,
        Err(recovery.clone())
    );
    assert_eq!(
        r.administrator
            .set_retention_policy("p", &json!({"sources": ["x"]}), None)
            .await,
        Err(recovery.clone())
    );
    let far = parse_utc_text("2100-01-01T00:00:00.000000Z").expect("far");
    assert_eq!(
        r.maintainer.expire("p", 1, far, 1).await,
        Err(recovery.clone())
    );
    assert_eq!(
        r.maintainer
            .purge_body(Uuid::now_v7(), "adapter_defect")
            .await,
        Err(recovery.clone())
    );
    assert_eq!(
        r.relay_admin
            .record_relay_control(
                "audit.delivery.replay_requested",
                lost[0].0,
                "delivery_unknown_at_limit",
                None
            )
            .await,
        Err(recovery.clone())
    );
    assert_eq!(
        r.dba.bind_principal(&cast.reader.role, ISSUER, "x").await,
        Err(recovery.clone())
    );
    assert_eq!(
        r.dba.unbind_principal(&cast.reader.role).await,
        Err(recovery.clone())
    );
    assert_eq!(
        r.dba
            .bootstrap_administrator(&cast.reader.role, ISSUER, "x")
            .await,
        Err(recovery)
    );

    // The three allowed paths work and change nothing.
    for admin in [&r.verifier, &r.maintainer] {
        let check = admin.verify_recovery().await.expect("verify_recovery");
        assert_eq!(check.outcome, "ok", "{}", check.violations);
        assert!(check.recovery_mode);
        assert_eq!(check.head_seq, dump_head);
    }
    assert_eq!(
        r.maintainer
            .identity_chain_recovery_page(0, 1000)
            .await
            .expect("page")
            .len(),
        usize::try_from(dump_head).expect("usize")
    );
    let status = r.verifier.store_status().await.expect("status");
    assert!(status.recovery_mode);
    let dir = scratch_dir("recovery");
    let recovered = export_identity_chain_recovery(&r.maintainer, Some(c1), &dir)
        .await
        .expect("identity chain");
    assert!(recovered.manifest.anchored);
    assert_eq!(
        recovered
            .manifest
            .checkpoint
            .as_ref()
            .expect("c1")
            .comparison,
        "ahead"
    );
    assert_eq!(
        head(&r.admin).await,
        restored_head,
        "recovery reads change nothing"
    );

    // privileges.sql is not part of the dump: the posture gate refuses.
    assert_eq!(
        r.maintainer.begin_recovery_epoch(&c1, relay_max_seq).await,
        Err(AdminError::PostureInvalid)
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(PRIVILEGES_SQL))
        .execute(&r.admin)
        .await
        .expect("privileges on the restored database");
    assert_eq!(r.maintainer.posture().await.expect("posture"), vec![]);

    let started = r
        .maintainer
        .begin_recovery_epoch(&c1, relay_max_seq)
        .await
        .expect("epoch started");
    assert_eq!(
        started.seq,
        dump_head + 1,
        "epoch_started at restored_head + 1"
    );
    assert_eq!((started.new_epoch, started.restored_head), (2, dump_head));
    assert_eq!(started.classification, "ahead");
    let (seq, _, epoch) = head(&r.admin).await;
    assert_eq!((seq, epoch), (dump_head + 1, 2));
    let details = control_events(&r.admin, "audit.recovery.epoch_started").await;
    let details = &details[0].1;
    assert_eq!(details["reason_kind"], json!("fingerprint_mismatch"));
    assert_eq!(details["lost_after_seq"], json!(dump_head));
    assert_eq!(details["lost_to_seq_claimed"], json!(relay_max_seq));
    assert_eq!(details["checkpoint_seq"], json!(c1.seq));
    assert_ne!(details["old_database_oid"], details["new_database_oid"]);
    // Not in recovery any more.
    assert!(matches!(
        r.maintainer.begin_recovery_epoch(&c1, 0).await,
        Err(AdminError::Database { ref sqlstate, .. }) if sqlstate == "55000"
    ));

    // Publication resumes and the chain continues from the restored head.
    let resumed = r.relay.ingest(&late).await.expect("stored");
    assert_eq!(resumed.seq, dump_head + 2);
    let redelivered = r
        .relay
        .ingest(&document_created(lost[0].0, Uuid::now_v7(), OCCURRED, 0))
        .await
        .expect("re-delivery of a lost event");
    assert_eq!(redelivered.outcome, IngestOutcome::Stored);
    assert_eq!(
        r.verifier.verify(None, None).await.expect("verify").outcome,
        "ok"
    );
    assert_store_conforms(&r.admin).await;

    // Offline: the recovery epoch needs an out-of-band record.
    let export_dir = scratch_dir("after-epoch");
    let export = export_to_dir(
        &r.verifier,
        &ExportRequest {
            operation: AccessOperation::IdentityChain,
            filter: json!({}),
            page_size: 1000,
            max_pages: 1,
            checkpoint: None,
        },
        &export_dir,
    )
    .await
    .expect("identity chain");
    let transitions = export.report.epoch_transitions();
    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0].seq, dump_head + 1);
    let checkpoints: [Checkpoint; 2] = [c1, c2];
    assert_eq!(
        assess_recovery(&export.report, &checkpoints, &[]).verdict,
        ChainVerdict::UnverifiedRecovery
    );
    let record = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head: dump_head,
        lost_to: relay_max_seq,
    };
    let assessment = assess_recovery(&export.report, &checkpoints, &[record]);
    assert_eq!(assessment.verdict, ChainVerdict::Lost);
    assert_eq!(assessment.epochs[0].record, Some(record));
}

#[tokio::test]
async fn planned_move_rebinds_the_fingerprint_without_a_new_epoch() {
    let db = TestDb::start().await;
    if db.container.is_none() {
        eprintln!("skipped: the restore test runs pg_dump inside the container");
        return;
    }
    let cast = Cast::new(&db).await;
    let store = PostgresAuditStore::new(cast.relay.pool.clone(), TIMEOUT)
        .await
        .expect("relay");
    ingest(&store, 2).await;
    assert_eq!(
        cast.maintainer
            .admin()
            .await
            .rebind_fingerprint()
            .await
            .expect("unchanged")
            .status,
        "unchanged"
    );
    dump_and_restore(&db, "/tmp/move.dump", Some("audit_store_moved")).await;
    let r = connect_restored(&db, &cast, "audit_store_moved").await;
    let moved_head = head(&r.admin).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(PRIVILEGES_SQL))
        .execute(&r.admin)
        .await
        .expect("privileges");
    let rebound = r.maintainer.rebind_fingerprint().await.expect("rebound");
    assert_eq!(rebound.status, "rebound");
    let (seq, _, epoch) = head(&r.admin).await;
    assert_eq!((seq, epoch), (moved_head.0 + 1, 1), "no new epoch");
    let recorded = control_events(&r.admin, "audit.recovery.fingerprint_rebound").await;
    assert_eq!(recorded.len(), 1);
    assert_ne!(
        recorded[0].1["old_database_oid"],
        recorded[0].1["new_database_oid"]
    );
    assert_eq!(
        r.maintainer
            .rebind_fingerprint()
            .await
            .expect("again")
            .status,
        "unchanged"
    );
    r.relay
        .ingest(&document_created(
            Uuid::now_v7(),
            Uuid::now_v7(),
            OCCURRED,
            5,
        ))
        .await
        .expect("publication resumes");
    assert_eq!(
        r.verifier.verify(None, None).await.expect("verify").outcome,
        "ok"
    );
    assert_store_conforms(&r.admin).await;
}

#[tokio::test]
async fn a_regression_claim_starts_an_epoch_without_a_fingerprint_change() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = PostgresAuditStore::new(cast.relay.pool.clone(), TIMEOUT)
        .await
        .expect("relay");
    ingest(&store, 3).await;
    let checkpoint = cast.verifier.admin().await.checkpoint().await.expect("cp");
    let c = checkpoint.checkpoint().expect("checkpoint");
    let maintainer = cast.maintainer.admin().await;
    let (head_seq, _, _) = head(&db.admin).await;
    // A claim not beyond the head is not a regression.
    assert!(matches!(
        maintainer.begin_recovery_epoch(&c, head_seq).await,
        Err(AdminError::Database { ref sqlstate, .. }) if sqlstate == "55000"
    ));
    // The relay acknowledged seqs the Store no longer has (store_regressed).
    let started = maintainer
        .begin_recovery_epoch(&c, head_seq + 5)
        .await
        .expect("epoch");
    assert_eq!((started.seq, started.new_epoch), (head_seq + 1, 2));
    let details = control_events(&db.admin, "audit.recovery.epoch_started").await;
    assert_eq!(details[0].1["reason_kind"], json!("regressed"));
    assert_eq!(details[0].1["lost_to_seq_claimed"], json!(head_seq + 5));
    let row = sqlx::query("SELECT recovery_epoch FROM audit_store.events WHERE seq = $1")
        .bind(started.seq)
        .fetch_one(&db.admin)
        .await
        .expect("row");
    assert_eq!(row.get::<i64, _>("recovery_epoch"), 2);
    db.assert_store_conforms().await;
}
