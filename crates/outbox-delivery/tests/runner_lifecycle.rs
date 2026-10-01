//! P6-G06: lost leases, shutdown drain and store outages cannot produce an ack.

use std::{
    collections::{HashMap, VecDeque},
    future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use outbox_delivery::{
    ClaimedEvent, DeliveryConfig, DeliveryDecision, DeliveryEnvelope, DeliveryError,
    DeliveryFuture, ErrorCode, FenceResult, HandlerFuture, OutboxStore,
    runner::{
        ClaimAdmission, ClaimPermit, DeliveryContext, DeliveryHandler, DeliveryRunner,
        NoopAdmission, NoopPermit,
    },
};
use serde_json::json;
use time::OffsetDateTime;
use tokio::sync::watch;
use uuid::Uuid;

struct LifecycleStore {
    pending: Mutex<VecDeque<ClaimedEvent>>,
    acked: Mutex<Vec<Uuid>>,
    claim_calls: AtomicUsize,
    renew_calls: AtomicUsize,
    fail_calls: AtomicUsize,
    reap_calls: AtomicUsize,
    verify_calls: AtomicUsize,
    lose_on_renew: Option<usize>,
    outage: bool,
}

impl LifecycleStore {
    fn new(count: usize) -> Self {
        let now = OffsetDateTime::now_utc();
        let pending = (1..=count)
            .map(|n| ClaimedEvent {
                envelope: DeliveryEnvelope {
                    event_id: Uuid::from_u128(n as u128),
                    event_type: "DocumentRegistered".into(),
                    aggregate_type: "Document".into(),
                    aggregate_id: Uuid::from_u128(100),
                    payload: json!({"n": n}),
                    occurred_at: now,
                },
                attempt: 1,
                attempt_limit: 8,
                lease_token: Uuid::from_u128(1_000 + n as u128),
                lease_owner: Uuid::from_u128(2_000),
                lease_expires_at: now + time::Duration::seconds(1),
            })
            .collect();
        Self {
            pending: Mutex::new(pending),
            acked: Mutex::new(Vec::new()),
            claim_calls: AtomicUsize::new(0),
            renew_calls: AtomicUsize::new(0),
            fail_calls: AtomicUsize::new(0),
            reap_calls: AtomicUsize::new(0),
            verify_calls: AtomicUsize::new(0),
            lose_on_renew: None,
            outage: false,
        }
    }
}

impl OutboxStore for LifecycleStore {
    fn verify_policy(&self) -> DeliveryFuture<'_, ()> {
        Box::pin(async move {
            self.verify_calls.fetch_add(1, Ordering::SeqCst);
            if self.outage {
                Err(DeliveryError::StoreUnknown)
            } else {
                Ok(())
            }
        })
    }

    fn claim(
        &self,
        _owner: Uuid,
        limit: u32,
        _lease: Duration,
    ) -> DeliveryFuture<'_, Vec<ClaimedEvent>> {
        Box::pin(async move {
            self.claim_calls.fetch_add(1, Ordering::SeqCst);
            Ok((0..limit)
                .filter_map(|_| self.pending.lock().unwrap().pop_front())
                .collect())
        })
    }

    fn renew(
        &self,
        _event_id: Uuid,
        _lease_token: Uuid,
        _lease: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            let number = self.renew_calls.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(if self.lose_on_renew == Some(number) {
                FenceResult::Lost
            } else {
                FenceResult::Updated
            })
        })
    }

    fn settle_success(&self, event_id: Uuid, _token: Uuid) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.acked.lock().unwrap().push(event_id);
            Ok(FenceResult::Updated)
        })
    }

    fn settle_failure(
        &self,
        _event_id: Uuid,
        _token: Uuid,
        _code: ErrorCode,
        _terminal: bool,
        _backoff: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.fail_calls.fetch_add(1, Ordering::SeqCst);
            Ok(FenceResult::Updated)
        })
    }

    fn reap_exhausted(&self, _limit: u32) -> DeliveryFuture<'_, u64> {
        Box::pin(async move {
            self.reap_calls.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        })
    }
}

struct HangingHandler {
    started: AtomicUsize,
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

impl DeliveryHandler<NoopPermit> for HangingHandler {
    fn deliver(
        &self,
        _event: DeliveryEnvelope,
        context: DeliveryContext,
        _permit: NoopPermit,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        Box::pin(async move {
            *self.cancel.lock().unwrap() = Some(context.cancel);
            self.started.fetch_add(1, Ordering::SeqCst);
            future::pending().await
        })
    }
}

struct DrainHandler {
    started: AtomicUsize,
    cancels: Mutex<HashMap<Uuid, Arc<AtomicBool>>>,
    complete_first: watch::Receiver<bool>,
}

impl DeliveryHandler<NoopPermit> for DrainHandler {
    fn deliver(
        &self,
        event: DeliveryEnvelope,
        context: DeliveryContext,
        _permit: NoopPermit,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        Box::pin(async move {
            self.cancels
                .lock()
                .unwrap()
                .insert(event.event_id, context.cancel);
            self.started.fetch_add(1, Ordering::SeqCst);
            if event.event_id == Uuid::from_u128(1) {
                let mut gate = self.complete_first.clone();
                while !*gate.borrow_and_update() {
                    gate.changed().await.expect("completion gate stays open");
                }
                DeliveryDecision::Applied
            } else {
                future::pending().await
            }
        })
    }
}

fn config() -> DeliveryConfig {
    DeliveryConfig {
        batch_size: 2,
        max_in_flight: 2,
        lease_duration: Duration::from_secs(1),
        renew_interval: Duration::from_millis(200),
        max_processing: Duration::from_secs(3),
        drain_timeout: Duration::from_millis(350),
        poll_interval: Duration::from_millis(50),
        reap_batch: 2,
    }
}

async fn wait_for(counter: &AtomicUsize, at_least: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) < at_least {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("expected task did not start");
}

#[tokio::test]
async fn renew_lost_cancels_and_never_settles() {
    let mut raw = LifecycleStore::new(1);
    raw.lose_on_renew = Some(2); // first preflight succeeds; heartbeat loses.
    let store = Arc::new(raw);
    let handler = Arc::new(HangingHandler {
        started: AtomicUsize::new(0),
        cancel: Mutex::new(None),
    });
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        Arc::new(NoopAdmission),
        config(),
    )
    .unwrap();
    let (shutdown, receiver) = watch::channel(false);
    let task = tokio::spawn(async move { runner.run_until_shutdown(receiver).await });
    wait_for(&handler.started, 1).await;
    shutdown.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("lost fence did not stop drain")
        .unwrap()
        .unwrap();
    assert_eq!(handler.started.load(Ordering::SeqCst), 1);
    assert!(
        handler
            .cancel
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .load(Ordering::SeqCst)
    );
    assert!(store.renew_calls.load(Ordering::SeqCst) >= 2);
    assert!(store.acked.lock().unwrap().is_empty());
    assert_eq!(store.fail_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn shutdown_drains_completed_and_leaves_unfinished() {
    let store = Arc::new(LifecycleStore::new(3));
    let (first_gate, first_ready) = watch::channel(false);
    let handler = Arc::new(DrainHandler {
        started: AtomicUsize::new(0),
        cancels: Mutex::new(HashMap::new()),
        complete_first: first_ready,
    });
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        Arc::new(NoopAdmission),
        config(),
    )
    .unwrap();
    let (shutdown, receiver) = watch::channel(false);
    let task = tokio::spawn(async move { runner.run_until_shutdown(receiver).await });
    wait_for(&handler.started, 2).await;
    shutdown.send(true).unwrap();
    first_gate.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("bounded drain exceeded deadline")
        .unwrap()
        .unwrap();
    assert_eq!(*store.acked.lock().unwrap(), vec![Uuid::from_u128(1)]);
    assert_eq!(store.fail_calls.load(Ordering::SeqCst), 0);
    assert_eq!(store.pending.lock().unwrap().len(), 1);
    assert!(handler.cancels.lock().unwrap()[&Uuid::from_u128(2)].load(Ordering::SeqCst));
    assert!(store.reap_calls.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn outage_does_not_spin_or_ack_unknown() {
    let mut raw = LifecycleStore::new(1);
    raw.outage = true;
    let store = Arc::new(raw);
    let handler = Arc::new(HangingHandler {
        started: AtomicUsize::new(0),
        cancel: Mutex::new(None),
    });
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        Arc::new(NoopAdmission),
        config(),
    )
    .unwrap();
    let (shutdown, receiver) = watch::channel(false);
    let task = tokio::spawn(async move { runner.run_until_shutdown(receiver).await });
    tokio::time::sleep(Duration::from_millis(380)).await;
    shutdown.send(true).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("worker did not respond to shutdown")
        .unwrap();
    let polls = store.verify_calls.load(Ordering::SeqCst);
    assert!(
        (2..=8).contains(&polls),
        "unexpected outage poll count {polls}"
    );
    assert_eq!(store.claim_calls.load(Ordering::SeqCst), 0);
    assert!(store.acked.lock().unwrap().is_empty());
    assert_eq!(handler.started.load(Ordering::SeqCst), 0);
}

#[derive(Clone)]
struct SourcePermit {
    owned: Arc<AtomicBool>,
    releases: Arc<AtomicUsize>,
}

impl ClaimPermit for SourcePermit {
    fn preflight(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            Ok(if self.owned.load(Ordering::SeqCst) {
                FenceResult::Updated
            } else {
                FenceResult::Lost
            })
        })
    }

    fn renew(&self) -> DeliveryFuture<'_, FenceResult> {
        self.preflight()
    }

    fn release(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.releases.fetch_add(1, Ordering::SeqCst);
            Ok(if self.owned.swap(false, Ordering::SeqCst) {
                FenceResult::Updated
            } else {
                FenceResult::Lost
            })
        })
    }
}

struct SourceAdmission(SourcePermit);

impl ClaimAdmission for SourceAdmission {
    type Permit = SourcePermit;

    fn acquire(&self) -> DeliveryFuture<'_, Option<Self::Permit>> {
        Box::pin(async { Ok(Some(self.0.clone())) })
    }

    fn max_claims_per_permit(&self) -> u32 {
        1
    }
}

struct SourceHangingHandler {
    started: AtomicUsize,
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

impl DeliveryHandler<SourcePermit> for SourceHangingHandler {
    fn deliver(
        &self,
        _event: DeliveryEnvelope,
        context: DeliveryContext,
        _permit: SourcePermit,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        Box::pin(async move {
            *self.cancel.lock().unwrap() = Some(context.cancel);
            self.started.fetch_add(1, Ordering::SeqCst);
            future::pending().await
        })
    }
}

#[tokio::test]
async fn shutdown_releases_still_owned_source_permit_after_timeout() {
    let store = Arc::new(LifecycleStore::new(1));
    let releases = Arc::new(AtomicUsize::new(0));
    let permit = SourcePermit {
        owned: Arc::new(AtomicBool::new(true)),
        releases: releases.clone(),
    };
    let handler = Arc::new(SourceHangingHandler {
        started: AtomicUsize::new(0),
        cancel: Mutex::new(None),
    });
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        Arc::new(SourceAdmission(permit.clone())),
        config(),
    )
    .unwrap();
    let (shutdown, receiver) = watch::channel(false);
    let task = tokio::spawn(async move { runner.run_until_shutdown(receiver).await });
    wait_for(&handler.started, 1).await;
    shutdown.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("source drain exceeded deadline")
        .unwrap()
        .unwrap();
    assert_eq!(releases.load(Ordering::SeqCst), 1);
    assert!(!permit.owned.load(Ordering::SeqCst));
    assert!(
        handler
            .cancel
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .load(Ordering::SeqCst)
    );
    assert!(store.acked.lock().unwrap().is_empty());
    assert_eq!(store.fail_calls.load(Ordering::SeqCst), 0);
}
