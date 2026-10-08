//! T3 (design §6.3, §12, §14.3; operations guide §5.3): the Store goes down
//! while a running relay delivers and the Document platform keeps working.
//! Business transactions commit; their audit rows stay staged and pending;
//! the row the relay had claimed when the Store went away is held as an
//! outage with its attempt returned (never quarantined); health reports a
//! Store outage (not a staging failure) and the open circuit of the running
//! relay. Once the Store is back, the same relay drains everything exactly
//! once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use audit_core::port::BoxFuture;
use audit_core::{
    AuditEnvelope, AuditStore, ControlReceipt, ControlReceiptRow, IngestReceipt, ProbeExpectation,
    ReceiptIdentity, ReceiptRow, RelayControl, StoreError, StoreStatus,
};
use audit_store_postgres::PostgresAuditStore;
use document_domain::FolderId;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::document::{Platform, editor_ctx, reader_ctx, small_lifecycle};
use crate::support::*;

/// The real Store client; when armed, the Store database goes down right
/// before the next ingest reaches it (after the relay's probe admitted the
/// claim): a Store crash in the middle of a delivery.
struct StoreFailsMidDelivery {
    inner: PostgresAuditStore,
    cluster_admin: PgPool,
    armed: AtomicBool,
    hit: Mutex<Option<Uuid>>,
}

impl AuditStore for StoreFailsMidDelivery {
    fn ingest<'a>(
        &'a self,
        envelope: &'a AuditEnvelope,
    ) -> BoxFuture<'a, Result<IngestReceipt, StoreError>> {
        Box::pin(async move {
            if self.armed.swap(false, Ordering::SeqCst) {
                store_down(&self.cluster_admin).await;
                *self.hit.lock().expect("hit") = Some(envelope.id());
            }
            self.inner.ingest(envelope).await
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
fn a_store_outage_holds_deliveries_while_business_commits_then_drains_once() {
    run_scenario(store_outage);
}

async fn store_outage() {
    let env = Env::start().await;
    assert_relay_posture_clean(&env).await;
    let platform = Platform::new(env.document.clone());
    let store = Arc::new(StoreFailsMidDelivery {
        inner: env.store_client().await,
        cluster_admin: env.doc_admin.clone(),
        armed: AtomicBool::new(false),
        hit: Mutex::new(None),
    });
    let relay = RunningRelay::start_over(&env, store.clone()).await;

    // Normal operation.
    platform.bootstrap().await;
    small_lifecycle(&platform, "before the outage").await;
    wait_until("the relay drains", CONVERGE, || drained(&env)).await;
    let delivered_before = staged_rows(&env).await.len();

    // The Store goes down while the relay delivers a folder creation.
    store.armed.store(true, Ordering::SeqCst);
    let folder = FolderId::from_uuid(Uuid::now_v7());
    platform
        .create_folder(
            &editor_ctx(),
            folder,
            Platform::root(),
            "Synthetic during outage",
            "synthetic outage reason",
        )
        .await
        .expect("the business commits");
    wait_until("the claimed row is held as an outage", CONVERGE, || async {
        let Some(hit) = *store.hit.lock().expect("hit") else {
            return false;
        };
        let ledger = deliveries(&env).await;
        ledger[&hit].last_outage_code.is_some()
    })
    .await;
    let hit = store.hit.lock().expect("hit").expect("hit");

    // The Document platform keeps working while the Store is down.
    let document = small_lifecycle(&platform, "during the outage").await;
    platform
        .rename_folder(folder, "Synthetic renamed in outage", "synthetic rename")
        .await
        .expect("rename commits");
    platform
        .set_folder_policy(folder, 0, "synthetic policy in outage")
        .await
        .expect("policy commits");
    let refused = platform
        .create_folder(
            &reader_ctx(),
            FolderId::from_uuid(Uuid::now_v7()),
            Platform::root(),
            "Refused",
            "synthetic refused",
        )
        .await;
    assert!(refused.is_err(), "the denial is audited, not committed");
    assert_eq!(platform.document_revision(document).await, 2);

    // Health: a Store outage, held deliveries, the running relay's open
    // circuit; produced rows are all registered (no staging failure).
    let staged = staged_rows(&env).await;
    let held = staged.len() - delivered_before;
    assert_eq!(
        held,
        1 + 5 + 2 + 1,
        "folder, lifecycle, rename, policy, denial"
    );
    // The breaker alternates between open and half-open probes while the
    // Store stays down; the running relay reports it every second.
    let report = health_when(&env, |report| report["circuit"]["state"] == json!("open")).await;
    assert_eq!(report["stored"]["available"], json!(false), "{report}");
    let gate = report["stored"]["gate"].as_str().expect("gate");
    assert!(gate.starts_with("store_"), "{report}");
    let alarms = report["alarms"].as_array().expect("alarms");
    for alarm in ["store_unavailable", "circuit_open", "outage_held"] {
        assert!(alarms.contains(&json!(alarm)), "{alarm}: {report}");
    }
    assert!(!alarms.contains(&json!("quarantined")), "{report}");
    assert_eq!(report["produced"]["staged"], json!(staged.len()));
    assert_eq!(report["produced"]["registered"], json!(staged.len()));
    assert_eq!(report["produced"]["unregistered"], json!(0));
    assert_eq!(report["delivered"]["delivered"], json!(delivered_before));
    assert_eq!(report["delivered"]["pending"], json!(held));
    assert_eq!(report["delivered"]["outage_held"], json!(1));
    assert_eq!(report["delivered"]["quarantined_total"], json!(0));
    assert_eq!(report["circuit"]["running"], json!(1));
    assert!(
        report["circuit"]["gate"]
            .as_str()
            .expect("gate")
            .starts_with("store_")
    );
    let ledger = deliveries(&env).await;
    for row in &staged[delivered_before..] {
        let delivery = &ledger[&row.event_id];
        assert!(!delivery.delivered, "{}", row.event_type);
        assert_eq!(
            delivery.attempt_count, 0,
            "no attempt consumed: {}",
            row.event_type
        );
        assert_eq!(delivery.quarantine_code, None, "{}", row.event_type);
    }
    let claimed = &ledger[&hit];
    assert!(
        claimed
            .last_outage_code
            .as_deref()
            .is_some_and(|code| code.starts_with("store_")),
        "{claimed:?}"
    );
    assert!(
        relay.is_running(),
        "the relay keeps running through the outage"
    );

    // The Store returns: the same relay drains everything exactly once.
    env.store_up().await;
    wait_until("the relay drains after the outage", CONVERGE, || {
        drained(&env)
    })
    .await;
    let report = env.health(true).await;
    assert_eq!(report["stored"]["available"], json!(true), "{report}");
    assert_eq!(report["delivered"]["outage_held"], json!(0));
    assert_eq!(report["reconcile"]["counts"]["ok"], json!(staged.len()));
    relay.stop().await;
    let client = env.store_client().await;
    assert_delivered_exactly_once(&env, &env.store_admin, &client).await;
    for delivery in deliveries(&env).await.values() {
        assert_eq!(delivery.store_outcome.as_deref(), Some("stored"));
        assert_eq!(delivery.attempt_count, 1, "the outage attempt was returned");
    }
    assert_eq!(assert_store_chain(&env.store_admin).await, staged.len());
}
