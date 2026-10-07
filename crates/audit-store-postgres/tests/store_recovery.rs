//! Backup/restore contract and recovery mode (design §11): pg_dump and
//! pg_restore of the Store database into a new database in the same
//! cluster, the fingerprint gate, the exhaustive recovery-mode allowlist,
//! not_in_recovery, the posture gate, begin_recovery_epoch with the
//! restore / planned_move / regression classifications, report_regression
//! and declare_recovery_pending, and access_reapply_pending.

mod support;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use audit_core::{
    AuditStore, ChainVerdict, Checkpoint, IngestOutcome, OutageCode, ReceiptIdentity,
    RecoveryRecord, RelayControl, RelayControlKind, SourceMismatchCode, StoreError, StoreState,
    assess_recovery,
};
use audit_store_postgres::admin::{
    AccessChange, AccessOperation, AuditAdmin, RecoveryExpectation, parse_utc_text,
};
use audit_store_postgres::files::{ExportRequest, export_identity_chain_recovery, export_to_dir};
use audit_store_postgres::{AdminError, PRIVILEGES_SQL, PostgresAuditStore};
use serde_json::{Value, json};
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
    reader: AuditAdmin,
    verifier: AuditAdmin,
    administrator: AuditAdmin,
    maintainer: AuditAdmin,
    dba: AuditAdmin,
}

async fn connect_restored(db: &TestDb, cast: &Cast, target: &str) -> Restored {
    let url = |login: &Login| db.url(&login.role, target);
    Restored {
        admin: pool(&db.superuser_url(target)).await,
        relay: PostgresAuditStore::new(pool(&url(&cast.relay)).await, TIMEOUT)
            .await
            .expect("relay"),
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

fn rejected(code: &str) -> AdminError {
    AdminError::Rejected { code: code.into() }
}

fn outage(code: OutageCode) -> StoreError {
    StoreError::outage(code)
}

/// Re-applies access and retention after an epoch (no active retention
/// policy) and checks that investigate/export reopen only at the end.
async fn reapply(administrator: &AuditAdmin, maintainer: &AuditAdmin, reader: &AuditAdmin) {
    assert_eq!(
        reader
            .open_access(AccessOperation::Export, &json!({}), 10, 1)
            .await
            .expect_err("closed after the epoch"),
        rejected("access_reapply_pending")
    );
    administrator
        .record_access_reapplied()
        .await
        .expect("access re-applied");
    assert_eq!(
        reader
            .open_access(AccessOperation::Investigate, &json!({}), 10, 1)
            .await
            .expect_err("retention not re-run yet"),
        rejected("access_reapply_pending")
    );
    maintainer
        .confirm_retention_reapplied()
        .await
        .expect("no active policy: confirmed");
    reader
        .open_access(AccessOperation::Export, &json!({}), 10, 1)
        .await
        .expect("open again");
    assert_eq!(
        administrator.record_access_reapplied().await,
        Err(AdminError::Denied {
            code: "not_pending".into()
        })
    );
}

#[tokio::test]
async fn restore_enters_recovery_mode_and_a_new_epoch_continues_the_chain() {
    let db = TestDb::start().await;
    if db.container.is_none() {
        eprintln!("skipped: the restore test runs pg_dump inside the container");
        return;
    }
    let cast = Cast::new(&db).await;
    let store = cast.relay.store().await;
    ingest(&store, 4).await;
    let verifier = cast.verifier.admin().await;
    let c1 = verifier
        .checkpoint()
        .await
        .expect("checkpoint")
        .checkpoint()
        .expect("c1");

    dump_and_restore(&db, "/tmp/store.dump", None).await;
    let dump_head = head(&db.admin).await.0;
    assert_eq!(dump_head, c1.seq, "the checkpoint is the dumped head");
    // Delivered after the backup: lost by the restore.
    let lost = ingest(&store, 2).await;
    let relay_max_seq = lost.last().expect("lost").1;
    let c2 = verifier
        .checkpoint()
        .await
        .expect("checkpoint")
        .checkpoint()
        .expect("c2");

    restore(&db, "/tmp/store.dump", "audit_store_restored").await;
    let r = connect_restored(&db, &cast, "audit_store_restored").await;
    let restored_head = head(&r.admin).await;
    assert_eq!(restored_head.0, dump_head);

    // Recovery mode: publication refuses, content-free reads answer.
    let late = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 9);
    assert_eq!(
        r.relay.ingest(&late).await,
        Err(outage(OutageCode::RecoveryRequired))
    );
    let status = r.relay.probe(&expectation(None)).await.expect("probe");
    assert_eq!(status.state, StoreState::RecoveryMode);
    assert_eq!(status.admission(), Err(OutageCode::RecoveryRequired));
    assert_eq!(
        r.relay
            .record_relay_control(&RelayControl::from(
                RelayControlKind::SourceMismatchDetected {
                    event_id: lost[0].0,
                    code: SourceMismatchCode::ActorMismatch,
                }
            ))
            .await,
        Err(outage(OutageCode::RecoveryRequired))
    );
    assert_eq!(
        r.reader
            .open_access(AccessOperation::Export, &json!({}), 10, 1)
            .await
            .expect_err("open_access"),
        AdminError::RecoveryRequired
    );
    assert_eq!(
        r.dba.bind_principal(&cast.reader.role, ISSUER, "x").await,
        Err(AdminError::RecoveryRequired)
    );
    let receipts = r
        .relay
        .lookup_receipts(&[lost[0].0])
        .await
        .expect("lookups work in recovery mode");
    assert!(
        receipts.is_empty(),
        "the lost event is not in the restored Store"
    );
    for admin in [&r.verifier, &r.maintainer] {
        let check = admin.verify_recovery().await.expect("verify_recovery");
        assert_eq!(check.outcome, "ok", "{}", check.violations);
        assert!(check.recovery_mode);
        assert_eq!(check.head_seq, dump_head);
    }
    assert!(
        r.verifier
            .store_status()
            .await
            .expect("status")
            .recovery_mode
    );
    let dir = scratch_dir("recovery");
    let recovered = export_identity_chain_recovery(&r.maintainer, Some(c1), &dir)
        .await
        .expect("identity chain");
    assert!(recovered.manifest.anchored && recovered.manifest.complete);
    assert!(
        recovered.manifest.intents.is_empty(),
        "no intent in recovery mode"
    );
    assert_eq!(
        recovered
            .manifest
            .checkpoint
            .as_ref()
            .expect("c1")
            .comparison,
        "match"
    );
    assert_eq!(
        head(&r.admin).await,
        restored_head,
        "recovery reads change nothing"
    );

    // privileges.sql is not part of the dump: the posture gate refuses.
    assert_eq!(
        r.maintainer
            .preview_recovery_epoch(Some(&c2), Some(relay_max_seq))
            .await,
        Err(AdminError::PostureInvalid)
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(PRIVILEGES_SQL))
        .execute(&r.admin)
        .await
        .expect("privileges on the restored database");
    assert_eq!(r.maintainer.posture().await.expect("posture"), vec![]);

    // The latest out-of-band checkpoint (c2) lies beyond the restored head;
    // the lost range reaches the higher of it and the relay's ack.
    assert!(c2.seq > relay_max_seq);
    // The operator's out-of-band record must equal the Store's actual
    // restored state: every mismatched expectation is refused and changes
    // nothing (design §8, §11).
    let preview = r
        .maintainer
        .preview_recovery_epoch(Some(&c2), Some(relay_max_seq))
        .await
        .expect("preview");
    assert_eq!(
        (
            preview.old_epoch,
            preview.restored_head_seq,
            preview.restored_head_chain.as_str(),
            preview.lost_upper_seq,
            preview.classification.as_str(),
            preview.checkpoint_classification.as_deref()
        ),
        (
            1,
            dump_head,
            hex(&c1.chain).as_str(),
            c2.seq,
            "restore",
            Some("store_behind")
        )
    );
    let honest = preview.expectation().expect("expectation");
    for wrong in [
        RecoveryExpectation {
            old_epoch: 2,
            ..honest
        },
        RecoveryExpectation {
            restored_head_seq: dump_head - 1,
            ..honest
        },
        RecoveryExpectation {
            restored_head_chain: [0xee; 32],
            ..honest
        },
        RecoveryExpectation {
            lost_upper: relay_max_seq,
            ..honest
        },
    ] {
        assert_eq!(
            r.maintainer
                .begin_recovery_epoch(Some(&c2), Some(relay_max_seq), &wrong)
                .await,
            Err(AdminError::Denied {
                code: "expectation_mismatch".into()
            }),
            "{wrong:?}"
        );
    }
    // A lost upper bound below the restored head is not a record at all.
    assert_eq!(
        r.maintainer
            .begin_recovery_epoch(
                Some(&c2),
                Some(relay_max_seq),
                &RecoveryExpectation {
                    lost_upper: dump_head - 1,
                    ..honest
                }
            )
            .await,
        Err(AdminError::Denied {
            code: "invalid_input".into()
        })
    );
    assert_eq!(
        head(&r.admin).await,
        restored_head,
        "refusals change nothing"
    );
    assert!(
        r.verifier
            .store_status()
            .await
            .expect("status")
            .recovery_mode
    );
    let (started, epoch_record) =
        start_recovery_epoch(&r.maintainer, Some(&c2), Some(relay_max_seq))
            .await
            .expect("epoch started");
    assert_eq!(
        started.seq,
        dump_head + 1,
        "epoch_started at restored_head + 1"
    );
    assert_eq!(
        (
            started.new_epoch,
            started.restored_head_seq,
            started.classification.as_str(),
            started.checkpoint_classification.as_deref(),
            started.lost_from_seq,
            started.lost_upper_seq
        ),
        (
            2,
            dump_head,
            "restore",
            Some("store_behind"),
            dump_head + 1,
            c2.seq
        )
    );
    let (seq, _, epoch) = head(&r.admin).await;
    assert_eq!((seq, epoch), (dump_head + 1, 2));
    let details = control_events(&r.admin, "audit.recovery.epoch_started").await;
    let details = &details[0].1;
    assert_eq!(details["classification"], json!("restore"));
    assert_eq!(details["restored_head_seq"], json!(dump_head));
    assert_eq!(details["restored_head_chain"], json!(hex(&c1.chain)));
    assert_eq!(details["lost_from_seq"], json!(dump_head + 1));
    assert_eq!(details["lost_upper_seq"], json!(c2.seq));
    assert_eq!(details["relay_max_seq"], json!(relay_max_seq));
    assert_eq!(details["lost_upper_known"], json!(true));
    assert_eq!(details["checkpoint_seq"], json!(c2.seq));
    assert_eq!(details["checkpoint_classification"], json!("store_behind"));
    assert_eq!(details["regression_reported_by"], Value::Null);
    assert_ne!(details["old_database_oid"], details["new_database_oid"]);
    // Not in recovery any more.
    assert_eq!(
        r.maintainer
            .begin_recovery_epoch(Some(&c1), None, &honest)
            .await,
        Err(rejected("not_in_recovery"))
    );
    assert_eq!(
        r.verifier.verify_recovery().await,
        Err(rejected("not_in_recovery"))
    );

    // Content disclosure stays closed until access and retention are
    // re-applied; verification is open.
    assert_eq!(
        r.verifier.verify(None, None).await.expect("verify").outcome,
        "ok"
    );
    reapply(&r.administrator, &r.maintainer, &r.reader).await;

    // Publication resumes and the chain continues from the restored head.
    let resumed = r.relay.ingest(&late).await.expect("stored");
    assert!(resumed.seq > dump_head + 1);
    let redelivered = r
        .relay
        .ingest(&document_created(lost[0].0, Uuid::now_v7(), OCCURRED, 0))
        .await
        .expect("re-delivery of a lost event");
    assert_eq!(redelivered.outcome, IngestOutcome::Stored);
    let ranges = r.relay.lookup_lost_ranges().await.expect("lost ranges");
    assert_eq!(ranges.len(), 1);
    assert!(ranges[0].contains(1, relay_max_seq) && ranges[0].contains(1, c2.seq));
    assert!(!ranges[0].contains(1, dump_head) && !ranges[0].contains(2, relay_max_seq));
    assert_store_conforms(&r.admin).await;

    // Offline: the recovery epoch needs an out-of-band record.
    let export_dir = scratch_dir("after-epoch");
    let export = export_to_dir(
        &r.verifier,
        &ExportRequest {
            operation: AccessOperation::Verify,
            filter: json!({}),
            page_size: 1000,
            max_pages: 1,
            checkpoint: None,
        },
        &export_dir,
    )
    .await
    .expect("body export");
    assert!(export.manifest.complete);
    assert!(
        export.report.epochs_authenticated,
        "epoch_started attests the change"
    );
    let transitions = export.report.epoch_transitions();
    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0].seq, dump_head + 1);
    let checkpoints: [Checkpoint; 2] = [c1, c2];
    assert_eq!(
        assess_recovery(&export.report, &checkpoints, &[]).verdict,
        ChainVerdict::UnverifiedRecovery
    );
    // A record that does not cover c2 does not explain it.
    let short = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head_seq: dump_head,
        restored_head_chain: c1.chain,
        lost_upper: relay_max_seq,
    };
    assert_eq!(
        assess_recovery(&export.report, &checkpoints, &[short]).verdict,
        ChainVerdict::UnverifiedRecovery
    );
    // The record of epoch_started (lost range up to c2) explains it as lost.
    let record = RecoveryRecord {
        lost_upper: started.lost_upper_seq,
        ..short
    };
    assert_eq!(record, epoch_record, "the record the Store confirmed");
    let assessment = assess_recovery(&export.report, &checkpoints, &[record]);
    assert_eq!(assessment.verdict, ChainVerdict::Lost);
    assert_eq!(assessment.epochs[0].record, Some(record));
}

#[tokio::test]
async fn planned_move_is_an_epoch_with_an_empty_lost_range() {
    let db = TestDb::start().await;
    if db.container.is_none() {
        eprintln!("skipped: the restore test runs pg_dump inside the container");
        return;
    }
    let cast = Cast::new(&db).await;
    let store = cast.relay.store().await;
    ingest(&store, 2).await;
    // The relay is stopped; the checkpoint is the last row before the move.
    let c = cast
        .verifier
        .admin()
        .await
        .checkpoint()
        .await
        .expect("checkpoint")
        .checkpoint()
        .expect("c");
    dump_and_restore(&db, "/tmp/move.dump", Some("audit_store_moved")).await;
    let r = connect_restored(&db, &cast, "audit_store_moved").await;
    let moved_head = head(&r.admin).await.0;
    assert_eq!(moved_head, c.seq);
    sqlx::raw_sql(sqlx::AssertSqlSafe(PRIVILEGES_SQL))
        .execute(&r.admin)
        .await
        .expect("privileges");
    let (started, _) = start_recovery_epoch(&r.maintainer, Some(&c), None)
        .await
        .expect("planned move");
    assert_eq!(
        (
            started.classification.as_str(),
            started.checkpoint_classification.as_deref(),
            started.new_epoch,
            started.lost_from_seq,
            started.lost_upper_seq
        ),
        ("planned_move", Some("match"), 2, moved_head + 1, moved_head)
    );
    let details = &control_events(&r.admin, "audit.recovery.epoch_started").await[0].1;
    assert_eq!(details["classification"], json!("planned_move"));
    assert_eq!(details["lost_upper_seq"], json!(moved_head));
    assert_ne!(details["old_database_oid"], details["new_database_oid"]);
    reapply(&r.administrator, &r.maintainer, &r.reader).await;
    r.relay
        .ingest(&document_created(
            Uuid::now_v7(),
            Uuid::now_v7(),
            OCCURRED,
            5,
        ))
        .await
        .expect("publication resumes");
    // Offline: the documented move loses nothing and the final checkpoint
    // matches the exported head exactly.
    let c_final = r
        .verifier
        .checkpoint()
        .await
        .expect("checkpoint")
        .checkpoint()
        .expect("final");
    let export = export_to_dir(
        &r.verifier,
        &ExportRequest {
            operation: AccessOperation::IdentityChain,
            filter: json!({}),
            page_size: 1000,
            max_pages: 1,
            checkpoint: Some(c_final),
        },
        &scratch_dir("moved"),
    )
    .await
    .expect("identity chain");
    assert_eq!(
        export.manifest.checkpoint.as_ref().expect("c").comparison,
        "match"
    );
    let record = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head_seq: moved_head,
        restored_head_chain: c.chain,
        lost_upper: moved_head,
    };
    let assessment = assess_recovery(&export.report, &[c, c_final], &[record]);
    assert_eq!(assessment.epochs[0].record, Some(record));
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    assert_store_conforms(&r.admin).await;
}

#[tokio::test]
async fn a_regression_report_enters_recovery_until_a_regression_epoch() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = cast.relay.store().await;
    let ids = ingest(&store, 3).await;
    let receipts = store.lookup_receipts(&[ids[2].0]).await.expect("receipts");
    let intact = receipts[0].identity();
    // An identity that still resolves changes nothing.
    store.report_regression(&intact).await.expect("intact");
    assert!(
        !cast
            .verifier
            .admin()
            .await
            .store_status()
            .await
            .expect("status")
            .recovery_pending
    );
    let (head_before, _, _) = head(&db.admin).await;
    let missing = ReceiptIdentity {
        seq: ids[1].1,
        event_id: Uuid::now_v7(),
        envelope_digest: [7; 32],
    };
    store.report_regression(&missing).await.expect("reported");
    let status = cast
        .verifier
        .admin()
        .await
        .store_status()
        .await
        .expect("status");
    assert!(status.recovery_pending && status.recovery_mode);
    assert_eq!(
        status.recovery_pending_reason.as_deref(),
        Some("regression")
    );
    // Sticky: publication is closed, the probe reports recovery mode.
    let next = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 9);
    assert_eq!(
        store.ingest(&next).await,
        Err(outage(OutageCode::RecoveryRequired))
    );
    assert_eq!(
        store
            .probe(&expectation(Some(intact)))
            .await
            .expect("probe")
            .state,
        StoreState::RecoveryMode
    );
    store
        .report_regression(&missing)
        .await
        .expect("already pending is not an error");
    assert_eq!(head(&db.admin).await.0, head_before, "nothing published");
    // Only begin_recovery_epoch clears it; the regression is classified and
    // its evidence recorded.
    let maintainer = cast.maintainer.admin().await;
    let (started, _) = start_recovery_epoch(&maintainer, None, None)
        .await
        .expect("regression epoch");
    assert_eq!(started.classification, "regression");
    assert_eq!(started.checkpoint_classification, None);
    assert_eq!(started.seq, head_before + 1);
    let details = &control_events(&db.admin, "audit.recovery.epoch_started").await[0].1;
    assert_eq!(details["regression_reported_seq"], json!(ids[1].1));
    assert_eq!(
        details["regression_reported_event_id"],
        json!(missing.event_id.to_string())
    );
    assert_eq!(details["regression_reported_digest"], json!(hex(&[7; 32])));
    assert_eq!(details["regression_reported_by"], json!(cast.relay.role));
    assert_eq!(details["regression_head_seq"], json!(head_before));
    assert_eq!(details["lost_upper_known"], json!(true));
    assert_eq!(details["old_database_oid"], details["new_database_oid"]);
    let status = store
        .probe(&expectation(Some(intact)))
        .await
        .expect("probe");
    assert_eq!(status.state, StoreState::Operational);
    assert_eq!(status.recovery_epoch, 2);
    assert!(
        !status.regression_detected,
        "rows below the restored head survive"
    );
    assert_eq!(
        store.ingest(&next).await.expect("resumes").outcome,
        IngestOutcome::Stored
    );
    db.assert_store_conforms().await;
}

/// Every EXECUTE-granted function, called with harmless arguments by a login
/// holding its role: (login, call, allowed in recovery mode).
fn recovery_calls(cast: &Cast, intact: &ReceiptIdentity) -> Vec<(String, String, bool)> {
    let doc = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 1);
    let relay = &cast.relay.role;
    let calls: Vec<(&str, String, bool)> = vec![
        (relay, format!("SELECT * FROM audit_store.ingest('{}')", doc.to_json_string()), false),
        (
            relay,
            "SELECT * FROM audit_store.record_relay_control('audit.integrity.source_mismatch_detected', \
             '{\"event_id\": \"0199a1b2-0000-7000-8000-000000000001\", \"mismatch_code\": \"actor_mismatch\"}')"
                .into(),
            false,
        ),
        (
            "reader",
            "SELECT * FROM audit_store.open_access('export', '{}', 10, 1)".into(),
            false,
        ),
        (
            "reader",
            "SELECT * FROM audit_store.read_page(repeat('0', 64), 0)".into(),
            false,
        ),
        (
            "reader",
            "SELECT * FROM audit_store.close_access(repeat('0', 64), 0, ARRAY[]::text[])".into(),
            false,
        ),
        ("verifier", "SELECT * FROM audit_store.verify(NULL, NULL)".into(), false),
        ("verifier", "SELECT * FROM audit_store.checkpoint()".into(), false),
        (
            "admin",
            format!("SELECT * FROM audit_store.change_access('{ISSUER}', 'reader-1', 'verify', 'grant')"),
            false,
        ),
        (
            "admin",
            "SELECT * FROM audit_store.set_retention_policy('p', '{\"event_classes\": [\"SECURITY\"]}', NULL)"
                .into(),
            false,
        ),
        ("admin", "SELECT * FROM audit_store.record_access_reapplied()".into(), false),
        (
            "maintainer",
            "SELECT * FROM audit_store.expire('p', 1, now(), 1)".into(),
            false,
        ),
        (
            "maintainer",
            "SELECT * FROM audit_store.purge_body(gen_random_uuid(), 'adapter_defect')".into(),
            false,
        ),
        (
            "maintainer",
            "SELECT * FROM audit_store.confirm_retention_reapplied()".into(),
            false,
        ),
        (
            "dba",
            format!("SELECT * FROM audit_store.bind_principal('{}', 'i', 'p')", cast.admin2.role),
            false,
        ),
        (
            "dba",
            format!("SELECT * FROM audit_store.unbind_principal('{}')", cast.admin2.role),
            false,
        ),
        (
            "dba",
            format!("SELECT * FROM audit_store.bootstrap_administrator('{}', 'i', 'p')", cast.admin2.role),
            false,
        ),
        (
            "dba",
            "SELECT * FROM audit_store.register_source_service('i', 'p', \
             'urn:knowledge-platform:document-platform')"
                .into(),
            false,
        ),
        // The allowlist (design §11).
        (
            relay,
            "SELECT * FROM audit_store.probe('urn:knowledge-platform:document-platform', 1, \
             ARRAY['document.created'], NULL, NULL, NULL)"
                .into(),
            true,
        ),
        (relay, "SELECT * FROM audit_store.store_status()".into(), true),
        ("verifier", "SELECT * FROM audit_store.posture_check()".into(), true),
        (
            relay,
            "SELECT * FROM audit_store.lookup_receipts(ARRAY[gen_random_uuid()])".into(),
            true,
        ),
        (
            relay,
            "SELECT * FROM audit_store.list_source_receipts('urn:knowledge-platform:document-platform', 0, 10)"
                .into(),
            true,
        ),
        (
            relay,
            "SELECT * FROM audit_store.lookup_control_receipts(ARRAY[1, 2]::bigint[])".into(),
            true,
        ),
        (relay, "SELECT * FROM audit_store.lookup_lost_ranges()".into(), true),
        ("verifier", "SELECT * FROM audit_store.verify_recovery()".into(), true),
        ("maintainer", "SELECT * FROM audit_store.verify_recovery()".into(), true),
        (
            "verifier",
            "SELECT * FROM audit_store.identity_chain_recovery_page(0, 10)".into(),
            true,
        ),
        (
            "maintainer",
            "SELECT * FROM audit_store.identity_chain_recovery_page(0, 10)".into(),
            true,
        ),
        (
            relay,
            format!(
                "SELECT * FROM audit_store.report_regression({}, '{}', decode('{}', 'hex'))",
                intact.seq,
                intact.event_id,
                hex(&intact.envelope_digest)
            ),
            true,
        ),
        (
            "maintainer",
            "SELECT * FROM audit_store.declare_recovery_pending('incident_2')".into(),
            true,
        ),
    ];
    calls
        .into_iter()
        .map(|(login, sql, allowed)| (login.to_owned(), sql, allowed))
        .collect()
}

#[tokio::test]
async fn declared_recovery_mode_allows_exactly_the_recovery_allowlist() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = cast.relay.store().await;
    let ids = ingest(&store, 2).await;
    let intact = store.lookup_receipts(&[ids[1].0]).await.expect("receipt")[0].identity();
    let maintainer = cast.maintainer.admin().await;
    let admin = cast.admin.admin().await;
    // An active retention policy (re-applied after the epoch below).
    let policy = admin
        .set_retention_policy(
            "old_reads",
            &json!({"event_classes": ["DATA_ACCESS"]}),
            Some(30),
        )
        .await
        .expect("policy");
    // Outside recovery mode the recovery functions refuse.
    let verifier = cast.verifier.admin().await;
    assert_eq!(
        verifier.verify_recovery().await,
        Err(rejected("not_in_recovery"))
    );
    assert_eq!(
        verifier.identity_chain_recovery_page(0, 10).await,
        Err(rejected("not_in_recovery"))
    );
    assert_eq!(
        maintainer.preview_recovery_epoch(None, None).await,
        Err(rejected("not_in_recovery"))
    );
    // Incident codes follow the code grammar (recorded denial outside
    // recovery mode).
    assert_eq!(
        maintainer.declare_recovery_pending("Not A Code").await,
        Err(AdminError::Denied {
            code: "invalid_input".into()
        })
    );
    let declared = maintainer
        .declare_recovery_pending("incident_1")
        .await
        .expect("declared");
    assert_eq!(
        (declared.status.as_str(), declared.code.as_deref()),
        ("recovery_pending", Some("declared"))
    );
    let frozen = head(&db.admin).await;

    // Exhaustive: every function granted to a capability role is either in
    // the allowlist or refused with store_recovery_required.
    let granted: BTreeSet<String> = sqlx::query_scalar(
        "SELECT DISTINCT p.proname::text FROM pg_proc AS p \
         JOIN pg_namespace AS n ON n.oid = p.pronamespace, \
         LATERAL aclexplode(p.proacl) AS a \
         WHERE n.nspname = 'audit_store' AND a.grantee <> p.proowner",
    )
    .fetch_all(&db.admin)
    .await
    .expect("granted")
    .into_iter()
    .collect();
    let calls = recovery_calls(&cast, &intact);
    let owner_only: BTreeSet<String> = [
        "bind_principal",
        "unbind_principal",
        "bootstrap_administrator",
        "register_source_service",
    ]
    .map(String::from)
    .into();
    // begin_recovery_epoch (allowed) ends recovery mode: exercised below.
    let mut covered: BTreeSet<String> = calls
        .iter()
        .map(|(_, sql, _)| {
            sql.split("audit_store.")
                .nth(1)
                .and_then(|rest| rest.split('(').next())
                .expect("function name")
                .to_owned()
        })
        .collect();
    covered.insert("begin_recovery_epoch".to_owned());
    assert_eq!(
        covered,
        granted.union(&owner_only).cloned().collect::<BTreeSet<_>>(),
        "the call list covers every granted and owner-only function"
    );
    let logins = |name: &str| -> &PgPool {
        match name {
            "reader" => &cast.reader.pool,
            "verifier" => &cast.verifier.pool,
            "admin" => &cast.admin.pool,
            "maintainer" => &cast.maintainer.pool,
            "dba" => &cast.dba.pool,
            _ => &cast.relay.pool,
        }
    };
    for (login, sql, allowed) in &calls {
        let result = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.clone()))
            .fetch_all(logins(login))
            .await;
        let refused = match &result {
            Err(sqlx::Error::Database(e)) => e.code().as_deref() == Some("KA001"),
            Ok(rows) => rows.first().is_some_and(|row| {
                row.try_get::<String, _>("status")
                    .is_ok_and(|s| s == "recovery_required")
            }),
            Err(_) => false,
        };
        if *allowed {
            assert!(
                result.is_ok() && !refused,
                "{sql} must be allowed: {result:?}"
            );
        } else {
            assert!(
                refused,
                "{sql} must refuse with store_recovery_required: {result:?}"
            );
        }
    }
    assert_eq!(head(&db.admin).await, frozen, "nothing was appended");

    // begin_recovery_epoch: a declared restore without claims.
    let (started, _) = start_recovery_epoch(&maintainer, None, None)
        .await
        .expect("epoch");
    assert_eq!(started.classification, "restore");
    assert_eq!(started.lost_upper_seq, frozen.0);
    let details = &control_events(&db.admin, "audit.recovery.epoch_started").await[0].1;
    assert_eq!(details["lost_upper_known"], json!(false));
    assert_eq!(
        details["regression_reported_by"],
        json!(cast.maintainer.role)
    );
    assert_eq!(details["regression_reported_seq"], Value::Null);

    // Access re-application with an active retention policy: the policy must
    // be re-run before the maintainer can confirm.
    let reader = cast.reader.admin().await;
    assert_eq!(
        maintainer.confirm_retention_reapplied().await,
        Err(AdminError::Denied {
            code: "retention_not_reapplied".into()
        })
    );
    admin.record_access_reapplied().await.expect("access");
    assert_eq!(
        reader
            .open_access(AccessOperation::Investigate, &json!({}), 10, 1)
            .await
            .expect_err("still closed"),
        rejected("access_reapply_pending")
    );
    // A stale revision does not count; the current one settles it.
    let far = parse_utc_text("2100-01-01T00:00:00.000000Z").expect("far");
    let stale = maintainer
        .expire("old_reads", policy.revision + 1, far, 10)
        .await
        .expect("stale attempt is recorded");
    assert_eq!(stale.status, "stale_revision");
    assert!(
        cast.verifier
            .admin()
            .await
            .store_status()
            .await
            .expect("s")
            .access_reapply_pending
    );
    let rerun = maintainer
        .expire("old_reads", policy.revision, far, 10)
        .await
        .expect("re-run");
    assert_eq!(rerun.status, "expired");
    assert!(
        !cast
            .verifier
            .admin()
            .await
            .store_status()
            .await
            .expect("s")
            .access_reapply_pending
    );
    reader
        .open_access(AccessOperation::Investigate, &json!({}), 10, 1)
        .await
        .expect("open again");
    let reapplied: Vec<Value> = control_events(&db.admin, "audit.access_policy.changed")
        .await
        .into_iter()
        .map(|(_, d)| d)
        .filter(|d| d["change"] == "reapplied")
        .collect();
    assert_eq!(reapplied.len(), 1);
    assert_eq!(reapplied[0]["capability"], json!("administer"));
    // Grants still work after the epoch.
    admin
        .change_access(ISSUER, "reader-1", "verify", AccessChange::Grant)
        .await
        .expect("grant");
    db.assert_store_conforms().await;
}
