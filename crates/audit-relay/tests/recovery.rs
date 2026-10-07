//! Crash, reconciliation, replay and restore (design §6.2, §6.4, §11, §12,
//! §14.3): SIGKILL after the Store commit (child process), final-attempt
//! crash and the fenced `quarantined_stored` repair, audited replay and
//! `unaudited_replay`, `store_only`, unregistered repair, and the Store
//! restore gates (`store_recovery_required`, `store_regressed`) followed by
//! a new epoch, `reconcile --repair` and redelivery.

mod support;

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use audit_core::port::BoxFuture;
use audit_core::{AuditEnvelope, AuditStore, IngestReceipt, StoreError, StoreStatus};
use audit_relay::breaker::{Gate, Mode};
use audit_relay::health::{HealthOptions, health};
use audit_relay::reconcile::Reconciler;
use audit_relay::relay::{Relay, RelayParts};
use audit_relay::replay::{ReplayError, replay};
use audit_relay::store_admin::{PgStoreAdmin, StoreAdmin};
use audit_store_postgres::admin::AuditAdmin;
use audit_store_postgres::{PRIVILEGES_SQL, PostgresAuditStore};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use support::*;
use uuid::Uuid;

const CONVERGE: Duration = Duration::from_secs(40);
const INGESTED: &str = "AUDIT_RELAY_CHILD_INGESTED";
const CHILD_LIFETIME: Duration = Duration::from_secs(90);

// ---------------------------------------------------------------------------
// Child process: ingest, then hang until SIGKILL
// ---------------------------------------------------------------------------

/// Ingests for real, reports it, then never returns (the parent kills).
struct PausingStore {
    inner: PostgresAuditStore,
}

impl AuditStore for PausingStore {
    fn ingest<'a>(
        &'a self,
        envelope: &'a AuditEnvelope,
    ) -> BoxFuture<'a, Result<IngestReceipt, StoreError>> {
        Box::pin(async move {
            let result = self.inner.ingest(envelope).await;
            if let Ok(receipt) = &result {
                println!("\n{INGESTED} {} {}", envelope.id(), receipt.seq);
                use std::io::Write;
                std::io::stdout().flush().expect("flush");
                std::future::pending::<()>().await;
            }
            result
        })
    }

    fn probe(&self) -> BoxFuture<'_, Result<StoreStatus, StoreError>> {
        self.inner.probe()
    }
}

#[tokio::test]
#[ignore = "独立した子プロセスからだけ実行する"]
async fn child_relay_fixture() {
    std::thread::spawn(|| {
        std::thread::sleep(CHILD_LIFETIME);
        eprintln!("audit-relay child lifetime exceeded");
        std::process::exit(124);
    });
    let source = connect_with_retry(
        &std::env::var("AUDIT_RELAY_CHILD_SOURCE").expect("source"),
        4,
    )
    .await;
    let store_pool =
        connect_with_retry(&std::env::var("AUDIT_RELAY_CHILD_STORE").expect("store"), 4).await;
    let inner = PostgresAuditStore::new(store_pool.clone(), Duration::from_secs(10))
        .await
        .expect("relay store session");
    let admin = PgStoreAdmin::new(AuditAdmin::connect(store_pool).await.expect("admin"));
    let mut handler = test_handler();
    // The parent kills long before any handler timeout could fire.
    handler.ingest_timeout = Duration::from_secs(60);
    let relay = Relay::assemble(RelayParts {
        source,
        store: Arc::new(PausingStore { inner }),
        admin: Arc::new(admin),
        delivery: test_delivery(),
        handler,
        breaker: test_breaker(),
        policy: Default::default(),
        projector: None,
    })
    .expect("relay");
    loop {
        let _ = relay.runner.run_cycle().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

struct ChildRelay {
    child: Child,
    lines: Receiver<String>,
}

impl ChildRelay {
    fn spawn(source: &str, store: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "child_relay_fixture", "--ignored", "--nocapture"])
            .env("AUDIT_RELAY_CHILD_SOURCE", source)
            .env("AUDIT_RELAY_CHILD_STORE", store)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn child relay");
        let stdout = child.stdout.take().expect("stdout");
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout.take(64 * 1024));
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }

    async fn wait_ingested(&mut self) -> (Uuid, i64) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            assert!(
                self.child.try_wait().expect("child state").is_none(),
                "child exited early"
            );
            while let Ok(line) = self.lines.try_recv() {
                if let Some(rest) = line.strip_prefix(INGESTED) {
                    let mut parts = rest.split_whitespace();
                    let id = Uuid::parse_str(parts.next().expect("id")).expect("uuid");
                    let seq = parts.next().expect("seq").parse().expect("seq");
                    return (id, seq);
                }
            }
            assert!(Instant::now() < deadline, "child did not ingest");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn kill9(&mut self) {
        self.child.kill().expect("SIGKILL owned child");
        let status = self.child.wait().expect("reap child");
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(status.signal(), Some(9));
        }
    }
}

impl Drop for ChildRelay {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// FORCED_BY_TEST_SQL: expire every live lease (the killed process held it).
async fn expire_leases(env: &Env) {
    exec(
        &env.doc_admin,
        "UPDATE audit_relay.deliveries SET lease_expires_at = clock_timestamp() - \
         interval '1 second' WHERE lease_token IS NOT NULL",
    )
    .await;
}

async fn is_delivered(env: &Env, id: Uuid) -> bool {
    !env.delivery(id).await["delivered_at"].is_null()
}

#[tokio::test]
async fn kill9_after_the_store_commit_redelivers_as_a_duplicate() {
    let env = Env::start().await;
    let row = Staged::withdrawn("crash window");
    env.insert(&row).await;
    let mut child = ChildRelay::spawn(&env.worker.url, &env.relay_store.url);
    let (id, seq) = child.wait_ingested().await;
    assert_eq!(id, row.event_id);
    child.kill9();
    let d = env.delivery(id).await;
    assert!(d["delivered_at"].is_null(), "killed before the ack");
    assert!(!d["lease_token"].is_null());
    assert_eq!(d["attempt_count"], json!(1));
    assert_eq!(env.store_rows(id).await.len(), 1, "the Store committed");

    expire_leases(&env).await;
    let relay = env.relay().await;
    drive(&relay, CONVERGE, || is_delivered(&env, id)).await;
    let d = env.delivery(id).await;
    assert_eq!(d["store_outcome"], json!("duplicate"));
    assert_eq!(d["store_seq"], json!(seq));
    assert_eq!(
        d["attempt_count"],
        json!(2),
        "restart does not reset attempts"
    );
    assert_eq!(env.store_rows(id).await.len(), 1, "exactly one Store event");
    assert_store_conforms(&env.store_admin).await;
}

#[tokio::test]
async fn final_attempt_crash_quarantines_and_only_the_fenced_repair_acks() {
    let env = Env::start().await;
    let target = Staged::created();
    env.insert(&target).await;
    // FORCED_BY_TEST_SQL: the next claim is the last of 16 attempts.
    exec(
        &env.doc_admin,
        &format!(
            "UPDATE audit_relay.deliveries SET attempt_count = 15, attempt_limit = 16 \
             WHERE event_id = '{}'",
            target.event_id
        ),
    )
    .await;
    let mut child = ChildRelay::spawn(&env.worker.url, &env.relay_store.url);
    let (id, seq) = child.wait_ingested().await;
    assert_eq!(id, target.event_id);
    child.kill9();
    expire_leases(&env).await;
    let relay = env.relay().await;
    relay.runner.run_cycle().await.expect("reaper cycle");
    let d = env.delivery(id).await;
    assert_eq!(d["quarantine_code"], json!("delivery_unknown_at_limit"));
    assert_eq!(d["attempt_count"], json!(16));

    // A conflict-coded quarantine and a commitment-mismatched one.
    let conflict = Staged::created();
    let mismatched = Staged::created();
    env.insert(&conflict).await;
    env.insert(&mismatched).await;
    drive(&relay, CONVERGE, || async {
        is_delivered(&env, conflict.event_id).await && is_delivered(&env, mismatched.event_id).await
    })
    .await;
    env.force(&format!(
        "UPDATE audit_relay.deliveries SET delivered_at = NULL, store_seq = NULL, \
             store_envelope_digest = NULL, store_outcome = NULL, store_recovery_epoch = NULL, \
             quarantined_at = now(), quarantine_code = 'conflict' WHERE event_id = '{}'; \
         UPDATE audit_relay.deliveries SET delivered_at = NULL, store_seq = NULL, \
             store_envelope_digest = NULL, store_outcome = NULL, store_recovery_epoch = NULL, \
             quarantined_at = now(), quarantine_code = 'delivery_unknown_at_limit', \
             commitment_salt = '\\x{}' WHERE event_id = '{}';",
        conflict.event_id,
        "77".repeat(32),
        mismatched.event_id
    ))
    .await;

    let observer = Reconciler::new(env.worker.pool.clone(), Arc::new(env.relay_admin().await));
    let report = observer.run(false).await.expect("read-only reconcile");
    assert_eq!(report.counts.quarantined_stored, 1);
    assert_eq!(report.counts.quarantined, 1);
    assert_eq!(report.counts.quarantined_conflict, 1);
    assert_eq!(report.counts.unaudited_replay, 0);
    assert_eq!(report.applied, Default::default());

    let operator_admin = Arc::new(env.operator_admin().await);
    let repair = Reconciler::new(env.operator.pool.clone(), operator_admin.clone())
        .run(true)
        .await
        .expect("repair reconcile");
    assert_eq!(repair.planned.quarantined_stored, 1);
    assert_eq!(repair.applied.quarantined_stored, 1);
    let d = env.delivery(id).await;
    assert_eq!(d["store_outcome"], json!("duplicate"));
    assert_eq!(d["store_seq"], json!(seq));
    assert!(d["quarantined_at"].is_null());
    let history = sqlx::query(
        "SELECT transition, reconcile_seq, control_seq, quarantine_code \
         FROM audit_relay.delivery_history WHERE event_id = $1",
    )
    .bind(id)
    .fetch_one(&env.doc_admin)
    .await
    .expect("history");
    assert_eq!(history.get::<String, _>("transition"), "repair_ack_stored");
    assert_eq!(
        history.get::<Option<i64>, _>("reconcile_seq"),
        repair.control_seq
    );
    assert_eq!(history.get::<Option<i64>, _>("control_seq"), None);
    assert_eq!(
        history
            .get::<Option<String>, _>("quarantine_code")
            .as_deref(),
        Some("delivery_unknown_at_limit")
    );
    for row in [&conflict, &mismatched] {
        assert!(
            !env.delivery(row.event_id).await["quarantined_at"].is_null(),
            "untouched"
        );
    }

    // The SQL fence refuses direct calls too.
    let receipts = env
        .relay_admin()
        .await
        .lookup_receipts(&[conflict.event_id, mismatched.event_id])
        .await
        .expect("receipts");
    for receipt in &receipts {
        let commitment: Vec<u8> = sqlx::query_scalar(
            "SELECT source_commitment FROM audit_relay.lookup_deliveries(ARRAY[$1]::uuid[])",
        )
        .bind(receipt.event_id)
        .fetch_one(&env.operator.pool)
        .await
        .expect("commitment");
        // The fence compares the offered (Store) commitment with the one
        // recomputed from the salt: the conflict-coded row refuses even its
        // own commitment; the mismatched row refuses the Store's.
        let store_commitment = receipt.source_commitment.expect("c").to_vec();
        let offers = if receipt.event_id == conflict.event_id {
            vec![commitment, store_commitment]
        } else {
            assert_ne!(commitment, store_commitment);
            vec![store_commitment]
        };
        for offered in offers {
            let acked: bool = sqlx::query_scalar(
                "SELECT audit_relay.repair_ack_stored($1, $2, $3, $4, 'duplicate', $5, 1)",
            )
            .bind(receipt.event_id)
            .bind(receipt.seq)
            .bind(receipt.envelope_digest.to_vec())
            .bind(offered)
            .bind(repair.control_seq.expect("seq"))
            .fetch_one(&env.operator.pool)
            .await
            .expect("fenced call");
            assert!(
                !acked,
                "conflict code or commitment mismatch is never acked"
            );
        }
    }
    let modes: Vec<String> = env
        .controls("audit.reconciliation.completed")
        .await
        .iter()
        .map(|(_, details)| details["mode"].as_str().expect("mode").to_owned())
        .collect();
    assert_eq!(modes, ["read_only", "repair"]);
    assert_store_conforms(&env.store_admin).await;
}

#[tokio::test]
async fn replay_is_audited_and_direct_sql_replay_is_detected() {
    let env = Env::start().await;
    let row = Staged::created().with_data(json!({"documentId": 123}));
    env.insert(&row).await;
    let id = row.event_id;
    let relay = env.relay().await;
    drive(&relay, CONVERGE, || async {
        !env.delivery(id).await["quarantined_at"].is_null()
    })
    .await;
    let operator_admin = env.operator_admin().await;
    let outcome = replay(&env.operator.pool, &operator_admin, id)
        .await
        .expect("replay");
    assert_eq!(outcome.previous_quarantine_code, "invalid_field");
    assert_eq!(outcome.control_epoch, 1);
    let d = env.delivery(id).await;
    assert_eq!(d["attempt_count"], json!(0));
    assert!(d["attempt_limit"].is_null() && d["quarantined_at"].is_null());
    assert_eq!(d["replay_count"], json!(1));
    let controls = env.controls("audit.delivery.replay_requested").await;
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].0, outcome.control_seq);
    assert_eq!(controls[0].1["event_id"], json!(id.to_string()));
    assert_eq!(controls[0].1["quarantine_code"], json!("invalid_field"));
    let history = sqlx::query(
        "SELECT transition, control_seq, control_epoch, attempt_count, quarantine_code \
         FROM audit_relay.delivery_history WHERE event_id = $1",
    )
    .bind(id)
    .fetch_one(&env.doc_admin)
    .await
    .expect("history");
    assert_eq!(history.get::<String, _>("transition"), "replay");
    assert_eq!(
        history.get::<Option<i64>, _>("control_seq"),
        Some(outcome.control_seq)
    );
    assert_eq!(history.get::<i64, _>("control_epoch"), 1);
    assert_eq!(history.get::<i32, _>("attempt_count"), 1);
    drive(&relay, CONVERGE, || async {
        !env.delivery(id).await["quarantined_at"].is_null()
    })
    .await;

    // Refusals.
    let pending = Staged::created();
    env.insert(&pending).await;
    assert_eq!(
        replay(&env.operator.pool, &operator_admin, pending.event_id).await,
        Err(ReplayError::NotQuarantined("pending".into()))
    );
    assert_eq!(
        replay(&env.operator.pool, &operator_admin, Uuid::now_v7()).await,
        Err(ReplayError::NotFound)
    );
    let error = sqlx::query("SELECT audit_relay.replay($1, NULL, 1)")
        .bind(id)
        .fetch_one(&env.operator.pool)
        .await
        .expect_err("a Store control seq is required");
    assert_eq!(sqlstate(&error), "22023");
    let error = sqlx::query("SELECT audit_relay.replay($1, $2, 1)")
        .bind(id)
        .bind(outcome.control_seq)
        .fetch_one(&env.operator.pool)
        .await
        .expect_err("a control seq maps to one history row");
    assert_eq!(sqlstate(&error), "23505");

    // A replay done directly in SQL, without the Store record.
    let done: bool = sqlx::query_scalar("SELECT audit_relay.replay($1, 999999, 1)")
        .bind(id)
        .fetch_one(&env.operator.pool)
        .await
        .expect("direct replay");
    assert!(done);
    let classification =
        Reconciler::new(env.worker.pool.clone(), Arc::new(env.relay_admin().await))
            .classify()
            .await
            .expect("classify");
    assert_eq!(classification.counts.unaudited_replay, 1);
    assert_eq!(classification.counts.replay_record_lost, 0);
    let report = health(
        &env.worker.pool,
        Arc::new(env.relay_admin().await),
        HealthOptions {
            forecast: false,
            reconcile: true,
        },
    )
    .await
    .expect("health");
    assert!(
        report["alarms"]
            .as_array()
            .expect("alarms")
            .contains(&json!("unaudited_replay"))
    );
    assert_store_conforms(&env.store_admin).await;
}

#[tokio::test]
async fn store_only_and_unregistered_rows_are_reported_and_repaired() {
    let env = Env::start().await;
    let kept = Staged::created();
    let lost = Staged::metadata_changed("lost from the source");
    env.insert(&kept).await;
    env.insert(&lost).await;
    let relay = env.relay().await;
    drive(&relay, CONVERGE, || async {
        delivered_count(&env).await == 2
    })
    .await;
    // FORCED_BY_TEST_SQL: an older source backup lacks `lost`.
    env.force(&format!(
        "DELETE FROM audit_relay.deliveries WHERE event_id = '{0}'; \
         DELETE FROM public.audit_outbox_events WHERE event_id = '{0}';",
        lost.event_id
    ))
    .await;
    // A lost registration trigger.
    exec(
        &env.doc_admin,
        "ALTER TABLE public.audit_outbox_events DISABLE TRIGGER audit_relay_register",
    )
    .await;
    let orphan = Staged::created();
    env.insert(&orphan).await;
    exec(
        &env.doc_admin,
        "ALTER TABLE public.audit_outbox_events ENABLE TRIGGER audit_relay_register",
    )
    .await;

    let report = Reconciler::new(env.worker.pool.clone(), Arc::new(env.relay_admin().await))
        .run(false)
        .await
        .expect("reconcile");
    assert_eq!(
        (
            report.counts.ok,
            report.counts.store_only,
            report.counts.unregistered
        ),
        (1, 1, 1)
    );
    let repair = Reconciler::new(
        env.operator.pool.clone(),
        Arc::new(env.operator_admin().await),
    )
    .run(true)
    .await
    .expect("repair");
    assert_eq!(repair.applied.unregistered, 1);
    assert_eq!(
        env.delivery(orphan.event_id).await["registration_kind"],
        json!("repair")
    );
    drive(&relay, CONVERGE, || is_delivered(&env, orphan.event_id)).await;
    let body: Value =
        serde_json::from_str(&env.store_body(orphan.event_id).await.expect("body")).expect("json");
    assert_eq!(body["data"]["provenance"]["registration"], json!("repair"));
    let report = health(
        &env.worker.pool,
        Arc::new(env.relay_admin().await),
        HealthOptions {
            forecast: false,
            reconcile: true,
        },
    )
    .await
    .expect("health");
    let alarms = report["alarms"].as_array().expect("alarms");
    assert!(alarms.contains(&json!("store_only")) && alarms.contains(&json!("repair_registered")));
    assert!(!alarms.contains(&json!("unregistered_rows")));
    assert_eq!(
        report["reconcile"]["counts"]["store_only"],
        json!(1),
        "never deleted"
    );
    assert_eq!(env.store_rows(lost.event_id).await.len(), 1);
}

// ---------------------------------------------------------------------------
// Store restore (design §11)
// ---------------------------------------------------------------------------

async fn pg_dump(env: &Env, file: &str) {
    let (code, output) = env
        .cluster
        .docker_exec(&[
            "pg_dump", "-U", "postgres", "-Fc", "-d", STORE_DB, "-f", file,
        ])
        .await;
    assert_eq!(code, 0, "pg_dump: {output}");
}

async fn checkpoint(env: &Env) -> audit_core::Checkpoint {
    AuditAdmin::connect(env.verifier.pool.clone())
        .await
        .expect("verifier")
        .checkpoint()
        .await
        .expect("checkpoint")
        .checkpoint()
        .expect("checkpoint value")
}

async fn relay_max_seq(env: &Env) -> i64 {
    env.status().await["max_acked_store_seq"]
        .as_i64()
        .expect("acked")
}

async fn store_pool(env: &Env, login: &Login, database: &str) -> PgPool {
    connect_with_retry(&env.cluster.url(&login.role, database), 4).await
}

async fn redeliver_after_repair(
    env: &Env,
    relay: &Relay,
    admin: Arc<dyn StoreAdmin>,
    missing: i64,
) {
    let report = Reconciler::new(env.worker.pool.clone(), admin.clone())
        .classify()
        .await
        .expect("classify");
    assert_eq!(report.counts.delivered_missing, missing);
    let repair = Reconciler::new(env.operator.pool.clone(), admin.clone())
        .run(true)
        .await
        .expect("repair");
    assert_eq!(repair.applied.delivered_missing, missing);
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_relay.deliveries")
        .fetch_one(&env.doc_admin)
        .await
        .expect("count");
    drive(relay, CONVERGE, || async {
        delivered_count(env).await == total
    })
    .await;
    let report = Reconciler::new(env.worker.pool.clone(), admin)
        .classify()
        .await
        .expect("classify");
    assert_eq!(report.counts.ok, total);
    assert_eq!(report.counts.delivered_missing, 0);
    assert_eq!(report.counts.unaudited_replay, 0);
    let resets: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_relay.delivery_history \
         WHERE transition = 'repair_reset_missing' AND control_epoch = 2",
    )
    .fetch_one(&env.doc_admin)
    .await
    .expect("history");
    assert_eq!(resets, missing);
}

#[tokio::test]
async fn store_restore_into_a_new_database_gates_until_a_new_epoch() {
    let env = Env::start().await;
    let relay = env.relay().await;
    for _ in 0..3 {
        env.insert(&Staged::created()).await;
    }
    drive(&relay, CONVERGE, || async {
        delivered_count(&env).await == 3
    })
    .await;
    let c1 = checkpoint(&env).await;
    pg_dump(&env, "/tmp/store.dump").await;
    for _ in 0..2 {
        env.insert(&Staged::created()).await;
    }
    drive(&relay, CONVERGE, || async {
        delivered_count(&env).await == 5
    })
    .await;

    let restored = "audit_store_restored";
    let (code, output) = env
        .cluster
        .docker_exec(&["createdb", "-U", "postgres", restored])
        .await;
    assert_eq!(code, 0, "createdb: {output}");
    let (code, output) = env
        .cluster
        .docker_exec(&[
            "pg_restore",
            "-U",
            "postgres",
            "--exit-on-error",
            "--single-transaction",
            "-d",
            restored,
            "/tmp/store.dump",
        ])
        .await;
    assert_eq!(code, 0, "pg_restore: {output}");
    let relay_pool = store_pool(&env, &env.relay_store, restored).await;
    let admin: Arc<dyn StoreAdmin> = Arc::new(PgStoreAdmin::new(
        AuditAdmin::connect(relay_pool.clone())
            .await
            .expect("admin"),
    ));
    let store = PostgresAuditStore::new(relay_pool, Duration::from_millis(1_500))
        .await
        .expect("store");
    let moved = env
        .relay_with(RelayOverrides {
            store: Some(Arc::new(store)),
            admin: Some(admin.clone()),
            ..RelayOverrides::default()
        })
        .await;
    let late = Staged::created();
    env.insert(&late).await;
    cycles(&moved, 6).await;
    assert_eq!(env.delivery(late.event_id).await["attempt_count"], json!(0));
    assert_eq!(
        moved.breaker.gate(),
        Gate::Outage(audit_core::OutageCode::RecoveryRequired)
    );
    let report = health(&env.worker.pool, admin.clone(), HealthOptions::default())
        .await
        .expect("health");
    assert!(
        report["alarms"]
            .as_array()
            .expect("alarms")
            .contains(&json!("store_recovery_required"))
    );

    let restored_admin = connect_with_retry(&env.cluster.superuser_url(restored), 2).await;
    exec(&restored_admin, PRIVILEGES_SQL).await;
    let maintainer = AuditAdmin::connect(store_pool(&env, &env.maintainer, restored).await)
        .await
        .expect("maintainer");
    let started = maintainer
        .begin_recovery_epoch(&c1, relay_max_seq(&env).await)
        .await
        .expect("epoch");
    assert_eq!(started.new_epoch, 2);
    drive(&moved, CONVERGE, || is_delivered(&env, late.event_id)).await;
    assert_eq!(
        env.delivery(late.event_id).await["store_recovery_epoch"],
        json!(2)
    );

    let operator_pool = store_pool(&env, &env.operator_store, restored).await;
    let operator: Arc<dyn StoreAdmin> = Arc::new(PgStoreAdmin::new(
        AuditAdmin::connect(operator_pool).await.expect("operator"),
    ));
    let _ = admin;
    redeliver_after_repair(&env, &moved, operator, 2).await;
    assert_store_conforms(&restored_admin).await;
}

#[tokio::test]
async fn an_in_place_restore_is_detected_as_store_regressed() {
    let env = Env::start().await;
    let wrapped = Arc::new(WrappedAdmin::new(Arc::new(env.relay_admin().await)));
    let relay = env
        .relay_with(RelayOverrides {
            admin: Some(wrapped.clone()),
            ..RelayOverrides::default()
        })
        .await;
    for _ in 0..2 {
        env.insert(&Staged::created()).await;
    }
    drive(&relay, CONVERGE, || async {
        delivered_count(&env).await == 2
    })
    .await;
    let c1 = checkpoint(&env).await;
    pg_dump(&env, "/tmp/store-inplace.dump").await;
    for _ in 0..2 {
        env.insert(&Staged::created()).await;
    }
    drive(&relay, CONVERGE, || async {
        delivered_count(&env).await == 4
    })
    .await;
    let acked = relay_max_seq(&env).await;

    // Same database, same oid and timeline: the fingerprint cannot see it.
    let (code, output) = env
        .cluster
        .docker_exec(&[
            "pg_restore",
            "-U",
            "postgres",
            "--clean",
            "--if-exists",
            "--exit-on-error",
            "--single-transaction",
            "-d",
            STORE_DB,
            "/tmp/store-inplace.dump",
        ])
        .await;
    assert_eq!(code, 0, "pg_restore: {output}");
    let late = Staged::created();
    env.insert(&late).await;
    cycles(&relay, 6).await;
    assert_eq!(relay.breaker.gate(), Gate::Regressed);
    assert_ne!(relay.breaker.mode(), Mode::Closed);
    assert_eq!(
        env.delivery(late.event_id).await["attempt_count"],
        json!(0),
        "no claims"
    );
    let reports = wrapped.reports.lock().unwrap().clone();
    assert!(
        !reports.is_empty() && reports.iter().all(|(seq, _)| *seq == acked),
        "{reports:?}"
    );
    let report = health(&env.worker.pool, wrapped.clone(), HealthOptions::default())
        .await
        .expect("health");
    assert_eq!(report["stored"]["gate"], json!("store_regressed"));
    assert!(
        report["alarms"]
            .as_array()
            .expect("alarms")
            .contains(&json!("store_regressed"))
    );
    // Sticky: still closed while the epoch is unchanged.
    cycles(&relay, 3).await;
    assert_eq!(env.delivery(late.event_id).await["attempt_count"], json!(0));

    exec(&env.store_admin, PRIVILEGES_SQL).await;
    let maintainer = AuditAdmin::connect(store_pool(&env, &env.maintainer, STORE_DB).await)
        .await
        .expect("maintainer");
    let started = maintainer
        .begin_recovery_epoch(&c1, acked)
        .await
        .expect("epoch after regression");
    assert_eq!(started.new_epoch, 2);
    drive(&relay, CONVERGE, || is_delivered(&env, late.event_id)).await;
    assert_eq!(relay.breaker.gate(), Gate::Ok);
    redeliver_after_repair(&env, &relay, Arc::new(env.operator_admin().await), 2).await;
    assert_store_conforms(&env.store_admin).await;
}
