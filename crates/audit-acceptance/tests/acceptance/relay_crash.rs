//! T4 (design §6.2, §12, §14.3): a relay process is killed (SIGKILL) after
//! the Store committed a delivery and before the relay acknowledged it.
//! After a restart (`audit-relay run`), the lease expires, the row is
//! claimed again, the Store answers `duplicate` with the original seq, the
//! attempt count is not reset, nothing is lost and nothing is stored twice;
//! a read-only reconcile run finds every row `ok`.
//!
//! The killed relay is a child process: this test binary itself, running the
//! ignored `child_relay_fixture` with a Store client that ingests for real,
//! reports the receipt on stdout and then hangs until it is killed.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use audit_core::port::BoxFuture;
use audit_core::{
    AuditEnvelope, AuditStore, ControlReceipt, ControlReceiptRow, IngestReceipt, ProbeExpectation,
    ReceiptIdentity, ReceiptRow, RelayControl, StoreError, StoreStatus,
};
use audit_relay::breaker::BreakerConfig;
use audit_relay::handler::HandlerConfig;
use audit_relay::reconcile::Reconciler;
use audit_relay::relay::{Relay, RelayParts};
use audit_store_postgres::PostgresAuditStore;
use document_domain::FolderId;
use outbox_delivery::DeliveryConfig;
use serde_json::json;
use uuid::Uuid;

use crate::document::{Platform, editor_ctx, small_lifecycle};
use crate::support::*;

const INGESTED: &str = "AUDIT_ACCEPTANCE_CHILD_INGESTED";
const CHILD_SOURCE: &str = "AUDIT_ACCEPTANCE_CHILD_SOURCE";
const CHILD_STORE: &str = "AUDIT_ACCEPTANCE_CHILD_STORE";
const CHILD_LIFETIME: Duration = Duration::from_secs(90);
/// The child's `application_name` on both databases: the parent waits until
/// every backend of the killed process has ended before it reads what the
/// child committed.
const CHILD_APP: &str = "audit-acceptance-child-relay";
/// The killed relay's lease: the restarted relay claims its rows again
/// once it has expired (no forced SQL).
const CHILD_LEASE: Duration = Duration::from_secs(3);

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

    fn probe<'a>(
        &'a self,
        expected: &'a ProbeExpectation,
    ) -> BoxFuture<'a, Result<StoreStatus, StoreError>> {
        self.inner.probe(expected)
    }

    fn lookup_receipts<'a>(
        &'a self,
        event_ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>> {
        self.inner.lookup_receipts(event_ids)
    }

    fn list_source_receipts<'a>(
        &'a self,
        source: &'a str,
        after_seq: i64,
        limit: u32,
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>> {
        self.inner.list_source_receipts(source, after_seq, limit)
    }

    fn lookup_control_receipts<'a>(
        &'a self,
        seqs: &'a [i64],
    ) -> BoxFuture<'a, Result<Vec<ControlReceiptRow>, StoreError>> {
        self.inner.lookup_control_receipts(seqs)
    }

    fn record_relay_control<'a>(
        &'a self,
        control: &'a RelayControl,
    ) -> BoxFuture<'a, Result<ControlReceipt, StoreError>> {
        self.inner.record_relay_control(control)
    }

    fn report_regression<'a>(
        &'a self,
        identity: &'a ReceiptIdentity,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        self.inner.report_regression(identity)
    }
}

#[test]
#[ignore = "独立した子プロセスからだけ実行する"]
fn child_relay_fixture() {
    std::thread::spawn(|| {
        std::thread::sleep(CHILD_LIFETIME);
        eprintln!("audit-acceptance child relay lifetime exceeded");
        std::process::exit(124);
    });
    run_scenario(child_relay);
}

async fn child_relay() {
    let source = connect_with_retry(&std::env::var(CHILD_SOURCE).expect("source"), 4).await;
    let store_pool = connect_with_retry(&std::env::var(CHILD_STORE).expect("store"), 4).await;
    let inner = PostgresAuditStore::new(store_pool, Duration::from_secs(10))
        .await
        .expect("relay store session");
    let relay = Relay::assemble(RelayParts {
        source,
        store: Arc::new(PausingStore { inner }),
        delivery: DeliveryConfig {
            batch_size: 8,
            max_in_flight: 4,
            lease_duration: CHILD_LEASE,
            renew_interval: Duration::from_millis(500),
            max_processing: Duration::from_secs(60),
            drain_timeout: Duration::from_secs(5),
            poll_interval: Duration::from_millis(50),
            reap_batch: 32,
        },
        // The parent kills long before any handler timeout could fire.
        handler: HandlerConfig {
            ingest_timeout: Duration::from_secs(60),
            control_timeout: Duration::from_secs(1),
            control_attempts: 2,
        },
        breaker: BreakerConfig {
            initial_cooldown: Duration::from_millis(20),
            max_cooldown: Duration::from_millis(200),
            probe_timeout: STORE_TIMEOUT,
            closed_claims: 8,
        },
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
        let named = |url: &str| format!("{url}?application_name={CHILD_APP}");
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "relay_crash::child_relay_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env(CHILD_SOURCE, named(source))
            .env(CHILD_STORE, named(store))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn the child relay");
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
                "the child relay exited early"
            );
            while let Ok(line) = self.lines.try_recv() {
                if let Some(rest) = line.strip_prefix(INGESTED) {
                    let mut parts = rest.split_whitespace();
                    let id = Uuid::parse_str(parts.next().expect("id")).expect("uuid");
                    let seq = parts.next().expect("seq").parse().expect("seq");
                    return (id, seq);
                }
            }
            assert!(Instant::now() < deadline, "the child relay did not ingest");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn kill9(&mut self) {
        self.child.kill().expect("SIGKILL the owned child");
        let status = self.child.wait().expect("reap the child");
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

#[test]
fn a_relay_killed_after_the_store_commit_redelivers_as_a_duplicate_once() {
    run_scenario(relay_crash);
}

async fn relay_crash() {
    let env = Env::start().await;
    let platform = Platform::new(env.document.clone());
    platform.bootstrap().await;
    small_lifecycle(&platform, "first crash document").await;
    small_lifecycle(&platform, "second crash document").await;
    platform
        .create_folder(
            &editor_ctx(),
            FolderId::from_uuid(Uuid::now_v7()),
            Platform::root(),
            "Synthetic crash folder",
            "synthetic crash reason",
        )
        .await
        .expect("folder");
    let staged = staged_rows(&env).await;
    assert_eq!(staged.len(), 12);

    // The relay commits one delivery in the Store, then dies before the ack.
    let mut child = ChildRelay::spawn(&env.worker.url, &env.relay_store.url);
    let (killed, seq) = child.wait_ingested().await;
    child.kill9();
    // A statement the server had already received when the process died
    // still runs to its end (another in-flight ingest may commit after the
    // kill): read the ledger and the Store once every backend of the
    // killed process is gone.
    wait_until("the killed relay's sessions end", CONVERGE, || async {
        let sessions: i64 =
            sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE application_name = $1")
                .bind(CHILD_APP)
                .fetch_one(&env.doc_admin)
                .await
                .expect("sessions");
        sessions == 0
    })
    .await;
    let ledger = deliveries(&env).await;
    let row = &ledger[&killed];
    assert!(!row.delivered, "killed before the ack");
    assert_eq!(row.attempt_count, 1);
    let lease: Option<Uuid> =
        sqlx::query_scalar("SELECT lease_token FROM audit_relay.deliveries WHERE event_id = $1")
            .bind(killed)
            .fetch_one(&env.doc_admin)
            .await
            .expect("lease");
    assert!(lease.is_some(), "the killed process still holds the lease");
    let committed: Vec<i64> =
        sqlx::query_scalar("SELECT seq FROM audit_store.events WHERE event_id = $1")
            .bind(killed)
            .fetch_all(&env.store_admin)
            .await
            .expect("store rows");
    assert_eq!(committed, [seq], "the Store committed the delivery");
    // Other rows the child had in flight may have been stored before the
    // kill as well; none of them was acknowledged.
    let in_flight: Vec<Uuid> = sqlx::query_scalar(
        "SELECT event_id FROM audit_relay.deliveries WHERE lease_token IS NOT NULL",
    )
    .fetch_all(&env.doc_admin)
    .await
    .expect("leased rows");
    assert!(in_flight.contains(&killed));
    let committed_by_child: BTreeMap<Uuid, i64> = sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT event_id, seq FROM audit_store.events WHERE event_id = ANY($1)",
    )
    .bind(&in_flight)
    .fetch_all(&env.store_admin)
    .await
    .expect("rows the child stored")
    .into_iter()
    .collect();
    assert_eq!(committed_by_child.get(&killed), Some(&seq));
    let stored_relay_rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_store.events WHERE origin = 'relay'")
            .fetch_one(&env.store_admin)
            .await
            .expect("count");
    assert_eq!(
        stored_relay_rows,
        committed_by_child.len() as i64,
        "the child stored only rows it had leased"
    );

    // Restart: `audit-relay run`. The lease expires, the row is claimed
    // again and the Store recognizes the delivery.
    let relay = RunningRelay::start(&env);
    wait_until("the restarted relay drains", CONVERGE, || drained(&env)).await;
    relay.stop().await;

    let store = env.store_client().await;
    let stored = assert_delivered_exactly_once(&env, &env.store_admin, &store).await;
    assert_eq!(stored.len(), staged.len(), "no loss");
    let ledger = deliveries(&env).await;
    let row = &ledger[&killed];
    assert_eq!(row.store_outcome.as_deref(), Some("duplicate"));
    assert_eq!(
        row.store_seq,
        Some(seq),
        "the original receipt is acknowledged"
    );
    assert_eq!(row.attempt_count, 2, "a restart does not reset attempts");
    for id in &in_flight {
        assert_eq!(ledger[id].attempt_count, 2, "{id}: claimed by both relays");
    }
    // Every row the killed relay stored is acknowledged by the restarted
    // one as a duplicate of the child's receipt. Every other row is stored
    // by the restarted relay, or acknowledged as a duplicate only after its
    // own timed-out attempt (a late Store answer on a loaded host).
    let mut late_duplicates = 0;
    for (id, row) in &ledger {
        match committed_by_child.get(id) {
            Some(child_seq) => {
                assert_eq!(row.store_outcome.as_deref(), Some("duplicate"), "{row:?}");
                assert_eq!(row.store_seq, Some(*child_seq), "the child's receipt");
            }
            None => late_duplicates += usize::from(assert_stored_or_late_duplicate(row)),
        }
    }
    let duplicates = ledger
        .values()
        .filter(|row| row.store_outcome.as_deref() == Some("duplicate"))
        .count();
    assert_eq!(duplicates, committed_by_child.len() + late_duplicates);
    eprintln!(
        "relay_crash: staged={} killed_seq={seq} in_flight_at_kill={} stored_by_child={} \
         duplicates={duplicates} late_duplicates={late_duplicates}",
        staged.len(),
        in_flight.len(),
        committed_by_child.len()
    );

    // A read-only reconcile run (worker login, operator's Store login):
    // every row ok, recorded once in the Store.
    let report = Reconciler::new(
        env.worker.pool.clone(),
        Arc::new(env.operator_client().await),
    )
    .run(false)
    .await
    .expect("read-only reconcile");
    assert_eq!(report.counts.ok, staged.len() as u64, "{:?}", report.counts);
    assert_eq!(report.counts.delivered_missing, 0);
    assert_eq!(report.counts.digest_mismatch, 0);
    assert_eq!(report.counts.store_only, 0);
    assert_eq!(report.counts.pending, 0);
    assert_eq!(report.counts.quarantined, 0);
    assert_eq!(report.counts.unaudited_replay, 0);
    assert_eq!(report.applied, Default::default(), "read-only");
    let recorded = control_events(&env.store_admin, "audit.reconciliation.completed").await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].1["mode"], json!("read_only"));
    let health = env.health(true).await;
    assert_eq!(health["alarms"], json!([]), "{health}");
    assert_eq!(
        assert_store_chain(&env.store_admin).await,
        staged.len(),
        "one relay event per staged row"
    );
}
