//! T5 (design §11, §14.3; operations guide §10): the Store is backed up,
//! more real Document events are delivered, and the Store is restored from
//! the backup into a new database, following the documented procedure:
//! relay stopped for the backup, the restored Store refuses until a
//! recovery epoch starts (health `store_recovery_required`),
//! `verify --recovery`, the recovery identity-chain export compared with
//! the out-of-band checkpoint, the `begin-recovery-epoch` preview appended
//! to the out-of-band recovery records, the epoch started with exactly
//! those expectations, `reconcile --repair`, redelivery, access and
//! retention re-applied, verify, a new checkpoint and the offline
//! assessment. The procedure keeps the relay stopped until the epoch; here
//! `audit-relay run` is pointed at the restored Store before any recovery
//! step (as an early restart would), and the gate holds: rows produced
//! meanwhile stay at attempt 0 and are not stored until the epoch starts,
//! then the same relay delivers them. The events lost by the restore are
//! delivered again (exactly once in the restored Store) and the declared
//! lost range keeps the assessment honest: `lost`, never `authentic`;
//! `unverified_recovery` without the out-of-band record or with a forged
//! one. A further epoch started without any bound (an operator-declared
//! incident, no checkpoint, no relay seq) records its loss as unknown and is
//! assessed `lost` too.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use audit_relay::reconcile::Reconciler;
use audit_store_postgres::admin::{AccessOperation, AuditAdmin};
use audit_store_postgres::assess::{AssessClass, AssessInputs, assess_dir};
use audit_store_postgres::files::{
    CheckpointFile, ExportRequest, RecoveryRecordLine, export_identity_chain_recovery,
    export_to_dir, read_recovery_records, write_checkpoint,
};
use audit_store_postgres::{PRIVILEGES_SQL, PostgresAuditStore};
use document_domain::FolderId;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::document::{Platform, editor_ctx, small_lifecycle};
use crate::support::*;

const RESTORED_DB: &str = "audit_store_restored";
const DUMP: &str = "/tmp/audit-acceptance-store.dump";

fn append_line(path: &Path, line: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("recovery records file");
    writeln!(file, "{line}").expect("append record");
}

/// The rows produced during the recovery are not attempted (attempt 0) nor
/// acknowledged, and the restored Store holds none of them.
async fn assert_still_waiting(env: &Env, restored_admin: &PgPool, waited: &[Uuid], when: &str) {
    let ledger = deliveries(env).await;
    for id in waited {
        let row = &ledger[id];
        assert!(!row.delivered, "{when}: {row:?}");
        assert_eq!(row.attempt_count, 0, "{when}: no attempt: {row:?}");
    }
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_store.events WHERE event_id = ANY($1)")
            .bind(waited)
            .fetch_one(restored_admin)
            .await
            .expect("restored rows");
    assert_eq!(stored, 0, "{when}: nothing of them is stored");
}

fn assess(
    dir: &Path,
    checkpoint: audit_core::Checkpoint,
    records: Vec<audit_core::RecoveryRecord>,
) -> audit_store_postgres::assess::AssessReport {
    assess_dir(
        dir,
        &AssessInputs {
            checkpoint,
            anchor: None,
            records,
        },
    )
    .expect("assess")
}

#[test]
fn a_store_restored_from_backup_redelivers_the_lost_range_and_assesses_it_as_lost() {
    run_scenario(store_restore);
}

async fn store_restore() {
    let env = Env::start().await;
    let platform = Platform::new(env.document.clone());
    let oob = scratch_dir("restore");

    // Before the backup.
    let relay = RunningRelay::start(&env);
    platform.bootstrap().await;
    small_lifecycle(&platform, "before the backup").await;
    wait_until("the relay drains", CONVERGE, || drained(&env)).await;
    relay.stop().await;
    let before_backup: Vec<Uuid> = staged_rows(&env).await.iter().map(|r| r.event_id).collect();

    // Backup (§10.1): relay stopped, checkpoint to the out-of-band store,
    // then pg_dump of the Store as the superuser.
    let report = env.health(false).await;
    assert_eq!(report["circuit"]["running"], json!(0));
    let verifier = env.verifier_admin().await;
    let record = verifier.checkpoint().await.expect("checkpoint");
    let checkpoint_file = oob.join("checkpoint-1.json");
    write_checkpoint(&record, &checkpoint_file).expect("out-of-band checkpoint");
    let (code, _, stderr) = env
        .docker_exec(&[
            "pg_dump", "-U", "postgres", "-Fc", "-d", STORE_DB, "-f", DUMP,
        ])
        .await;
    assert_eq!(code, 0, "pg_dump: {stderr}");

    // After the backup: more business, delivered and acknowledged in epoch 1.
    let relay = RunningRelay::start(&env);
    small_lifecycle(&platform, "after the backup").await;
    platform
        .create_folder(
            &editor_ctx(),
            FolderId::from_uuid(Uuid::now_v7()),
            Platform::root(),
            "Synthetic after backup",
            "synthetic reason after backup",
        )
        .await
        .expect("folder");
    wait_until("the relay drains", CONVERGE, || drained(&env)).await;
    relay.stop().await;
    let after_backup: Vec<Uuid> = staged_rows(&env)
        .await
        .iter()
        .map(|r| r.event_id)
        .filter(|id| !before_backup.contains(id))
        .collect();
    assert_eq!(after_backup.len(), 6);
    let relay_max = env.relay_max_seq(1).await;

    // Restore (§10.2, step 1–2) into a new database, privileges re-applied.
    let (code, _, stderr) = env
        .docker_exec(&["createdb", "-U", "postgres", RESTORED_DB])
        .await;
    assert_eq!(code, 0, "createdb: {stderr}");
    let (code, _, stderr) = env
        .docker_exec(&[
            "pg_restore",
            "-U",
            "postgres",
            "--exit-on-error",
            "--single-transaction",
            "-d",
            RESTORED_DB,
            DUMP,
        ])
        .await;
    assert_eq!(code, 0, "pg_restore: {stderr}");
    let restored_admin = connect_with_retry(&env.superuser_url(RESTORED_DB), 2).await;
    exec(&restored_admin, PRIVILEGES_SQL).await;
    let verifier = AuditAdmin::connect(env.pool_on(&env.verifier, RESTORED_DB).await)
        .await
        .expect("verifier on the restored Store");
    assert_eq!(verifier.posture().await.expect("posture"), Vec::new());
    let status = verifier.store_status().await.expect("status");
    assert!(status.recovery_mode, "the fingerprint detects the restore");
    assert_eq!(
        status.head_seq, record.seq,
        "the dump ends at the checkpoint"
    );

    // `audit-relay run` is pointed at the restored Store before any
    // recovery step. The Store requires recovery, so the running relay
    // admits nothing: a Document event produced now waits (no attempt,
    // nothing stored) until the new epoch starts.
    let restored_url = env.url(&env.relay_store.role, RESTORED_DB);
    let relay = RunningRelay::start_with(run_config(&env.worker.url, &restored_url));
    let waiting = platform
        .create_document(
            Platform::root(),
            "Synthetic during recovery",
            b"A waiting body\n",
        )
        .await
        .expect("business continues");
    let waited: Vec<Uuid> = staged_rows(&env)
        .await
        .iter()
        .filter(|row| row.resource_id == waiting.0.as_uuid())
        .map(|row| row.event_id)
        .collect();
    assert_eq!(waited.len(), 2, "created and version created");
    // The running relay has probed the restored Store and reports the gate
    // (sampled every second); health against the restored Store says the
    // same, and produced ≠ delivered: the waiting rows are pending.
    let report = health_when_against(&env, &restored_url, |report| {
        report["circuit"]["running"] == json!(1)
            && report["circuit"]["gate"] == json!("store_recovery_required")
    })
    .await;
    assert_eq!(
        report["stored"]["gate"],
        json!("store_recovery_required"),
        "{report}"
    );
    assert!(
        report["alarms"]
            .as_array()
            .expect("alarms")
            .contains(&json!("store_recovery_required"))
    );
    let staged_now = staged_rows(&env).await.len();
    assert_eq!(report["produced"]["staged"], json!(staged_now), "{report}");
    assert_eq!(
        report["delivered"]["delivered"],
        json!(staged_now - waited.len())
    );
    assert_eq!(report["delivered"]["pending"], json!(waited.len()));
    // Many poll and probe cycles later (poll 50 ms, breaker cooldown at
    // most 400 ms) the rows still wait.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(relay.is_running());
    assert_still_waiting(&env, &restored_admin, &waited, "while the Store is gated").await;
    // --relay-max-seq: the highest Store seq the relay references in the
    // restored epoch, as health reports it against the restored Store.
    assert_eq!(report["stored"]["relay_max_seq"], json!(relay_max));
    assert!(
        relay_max > record.seq,
        "rows past the backup were acknowledged"
    );

    // Step 3: internal verification and the identity chain compared with
    // the out-of-band checkpoint.
    let checkpoint = CheckpointFile::read(&checkpoint_file)
        .expect("read the checkpoint")
        .checkpoint()
        .expect("checkpoint");
    let recovered = verifier.verify_recovery().await.expect("verify --recovery");
    assert_eq!(recovered.outcome, "ok", "{recovered:?}");
    assert_eq!(recovered.head_seq, record.seq);
    let identity_dir = oob.join("identity-recovery");
    std::fs::create_dir(&identity_dir).expect("dir");
    let identity = export_identity_chain_recovery(&verifier, Some(checkpoint), &identity_dir)
        .await
        .expect("identity chain export");
    assert_eq!(
        identity.manifest.checkpoint.as_ref().map(|c| c.comparison),
        Some("match")
    );

    // Step 4–5: preview, append the record out of band, start the epoch
    // with exactly that record (a different record is refused).
    let maintainer = AuditAdmin::connect(env.pool_on(&env.maintainer, RESTORED_DB).await)
        .await
        .expect("maintainer on the restored Store");
    let preview = maintainer
        .preview_recovery_epoch(Some(&checkpoint), Some(relay_max))
        .await
        .expect("preview");
    assert_eq!(preview.restored_head_seq, record.seq);
    assert_eq!(preview.lost_upper_seq, relay_max);
    assert!(preview.lost_upper_known);
    let expected = preview.expectation().expect("expectation");
    let records_file = oob.join("recovery-records.jsonl");
    append_line(
        &records_file,
        &RecoveryRecordLine::from_record(&expected.record()).to_line(),
    );
    let records = read_recovery_records(&records_file).expect("records");
    let recorded = audit_store_postgres::admin::RecoveryExpectation::from(&records[0]);
    let mut wrong = recorded;
    wrong.lost_upper = Some(record.seq);
    let refused = maintainer
        .begin_recovery_epoch(Some(&checkpoint), Some(relay_max), &wrong)
        .await;
    assert!(refused.is_err(), "a record that hides the loss is refused");
    assert!(relay.is_running());
    assert_still_waiting(&env, &restored_admin, &waited, "before the epoch").await;
    let started = maintainer
        .begin_recovery_epoch(Some(&checkpoint), Some(relay_max), &recorded)
        .await
        .expect("epoch");
    assert_eq!((started.old_epoch, started.new_epoch), (1, 2));
    assert_eq!(started.classification, "restore");
    assert_eq!(started.checkpoint_classification.as_deref(), Some("match"));
    assert_eq!(
        (started.lost_from_seq, started.lost_upper_seq),
        (record.seq + 1, relay_max)
    );

    // Step 6: the relay running against the restored Store delivers the
    // waiting rows in the new epoch; the operator's repair resets what the
    // restore lost; the relay delivers it again.
    let operator_store = PostgresAuditStore::new(
        env.pool_on(&env.operator_store, RESTORED_DB).await,
        STORE_TIMEOUT,
    )
    .await
    .expect("operator Store client");
    let operator_store: Arc<dyn audit_relay::store::RelayStore> = Arc::new(operator_store);
    let repair = Reconciler::new(env.operator.pool.clone(), operator_store.clone())
        .run(true)
        .await
        .expect("reconcile --repair");
    assert_eq!(repair.store_epoch, 2);
    assert_eq!(
        repair.applied.delivered_missing,
        after_backup.len() as u64,
        "{:?}",
        repair.counts
    );
    wait_until("the relay redelivers the lost range", CONVERGE, || {
        drained(&env)
    })
    .await;
    relay.stop().await;

    // Exactly once in the restored Store; what the restore lost is in
    // epoch 2 now, what survived keeps its epoch-1 receipt.
    let restored_store = PostgresAuditStore::new(
        env.pool_on(&env.relay_store, RESTORED_DB).await,
        STORE_TIMEOUT,
    )
    .await
    .expect("relay Store client");
    let stored = assert_delivered_exactly_once(&env, &restored_admin, &restored_store).await;
    let ledger = deliveries(&env).await;
    for id in &before_backup {
        assert_eq!(ledger[id].store_recovery_epoch, Some(1));
        assert!(stored[id].seq <= record.seq);
    }
    for id in &after_backup {
        assert_eq!(ledger[id].store_recovery_epoch, Some(2), "{id}");
        assert!(stored[id].seq > started.seq, "redelivered after the epoch");
    }
    for id in &waited {
        assert_eq!(ledger[id].store_recovery_epoch, Some(2));
        assert!(stored[id].seq > started.seq, "stored after the epoch");
        assert_eq!(ledger[id].attempt_count, 1, "no attempt while gated");
    }
    let resets: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_relay.delivery_history \
         WHERE transition = 'repair_reset_missing' AND control_epoch = 2",
    )
    .fetch_one(&env.doc_admin)
    .await
    .expect("history");
    assert_eq!(
        resets,
        after_backup.len() as i64,
        "the reset is kept in history"
    );
    let classified = Reconciler::new(env.worker.pool.clone(), operator_store)
        .classify()
        .await
        .expect("classify");
    assert_eq!(classified.counts.delivered_missing, 0);
    assert_eq!(classified.counts.unaudited_replay, 0);
    assert_eq!(classified.counts.ok, stored.len() as u64);

    // Step 7 (§10.3): access and retention re-applied (no retention policy
    // exists), then verify, a new checkpoint and the offline assessment.
    let admin = AuditAdmin::connect(env.pool_on(&env.admin, RESTORED_DB).await)
        .await
        .expect("admin on the restored Store");
    admin
        .record_access_reapplied()
        .await
        .expect("access re-applied");
    maintainer
        .confirm_retention_reapplied()
        .await
        .expect("retention re-applied");
    let head = verifier.store_status().await.expect("status").head_seq;
    let verified = verifier.verify(Some(1), Some(head)).await.expect("verify");
    assert_eq!(verified.outcome, "ok", "{verified:?}");
    let latest = verifier.checkpoint().await.expect("checkpoint");
    let latest_file = oob.join("checkpoint-2.json");
    write_checkpoint(&latest, &latest_file).expect("out-of-band checkpoint");
    let latest = CheckpointFile::read(&latest_file)
        .expect("read")
        .checkpoint()
        .expect("checkpoint");
    let export_dir = oob.join("export");
    std::fs::create_dir(&export_dir).expect("dir");
    let exported = export_to_dir(
        &verifier,
        &ExportRequest {
            operation: AccessOperation::Verify,
            filter: json!({}),
            page_size: 1000,
            max_pages: 100,
            checkpoint: None,
        },
        &export_dir,
    )
    .await
    .expect("export after the recovery");
    assert_eq!(exported.manifest.watermark, Some(latest.seq));

    let with_records = assess(
        &export_dir,
        latest,
        read_recovery_records(&records_file).expect("records"),
    );
    assert_eq!(with_records.verdict, "lost", "{with_records:?}");
    assert_eq!(with_records.class(), AssessClass::Review);
    assert_eq!(with_records.epochs_total, 1);
    assert_eq!(with_records.epochs_unrecorded, 0);
    let without = assess(&export_dir, latest, Vec::new());
    assert_eq!(without.verdict, "unverified_recovery", "{without:?}");
    let mut forged = records[0];
    forged.lost_upper = forged.restored_head_seq;
    let forged = assess(&export_dir, latest, vec![forged]);
    assert_eq!(forged.verdict, "unverified_recovery", "{forged:?}");

    // A loss whose extent nobody knows: an operator-declared incident and an
    // epoch started without a checkpoint or the relay's seq. The Store
    // records the upper bound as unknown and the assessment says `lost`.
    maintainer
        .declare_recovery_pending("synthetic_incident")
        .await
        .expect("declare recovery pending");
    let preview = maintainer
        .preview_recovery_epoch(None, None)
        .await
        .expect("preview without bounds");
    assert!(!preview.lost_upper_known, "{preview:?}");
    let unknown = preview.expectation().expect("expectation");
    assert_eq!(unknown.lost_upper, None);
    let line = RecoveryRecordLine::from_record(&unknown.record()).to_line();
    assert!(line.contains("\"lost_upper\":null"), "{line}");
    append_line(&records_file, &line);
    let third = maintainer
        .begin_recovery_epoch(None, None, &unknown)
        .await
        .expect("epoch without bounds");
    assert_eq!((third.new_epoch, third.lost_upper_known), (3, false));
    admin
        .record_access_reapplied()
        .await
        .expect("access re-applied");
    maintainer
        .confirm_retention_reapplied()
        .await
        .expect("retention re-applied");
    let unbounded = verifier.checkpoint().await.expect("checkpoint");
    let unbounded = unbounded.checkpoint().expect("checkpoint");
    let unbounded_dir = oob.join("export-unbounded");
    std::fs::create_dir(&unbounded_dir).expect("dir");
    export_to_dir(
        &verifier,
        &ExportRequest {
            operation: AccessOperation::Verify,
            filter: json!({}),
            page_size: 1000,
            max_pages: 100,
            checkpoint: None,
        },
        &unbounded_dir,
    )
    .await
    .expect("export after the unbounded epoch");
    let unbounded = assess(
        &unbounded_dir,
        unbounded,
        read_recovery_records(&records_file).expect("records"),
    );
    assert_eq!(unbounded.verdict, "lost", "{unbounded:?}");
    assert_eq!(unbounded.epochs_total, 2);
    assert_eq!(unbounded.epochs_unrecorded, 0);
    eprintln!(
        "store_restore: backup_head={} relay_max_seq={relay_max} lost=({}, {}] redelivered={} \
         verdicts: records={} none={} forged={} unknown_bound={}",
        record.seq,
        started.lost_from_seq - 1,
        started.lost_upper_seq,
        after_backup.len(),
        with_records.verdict,
        without.verdict,
        forged.verdict,
        unbounded.verdict
    );
    let _ = std::fs::remove_dir_all(&oob);
}
