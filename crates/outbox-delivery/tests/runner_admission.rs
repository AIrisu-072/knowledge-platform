//! P6-G05: dispatch capacity and both fences are checked before invoking a handler.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use outbox_delivery::{
    ClaimedEvent, DeliveryConfig, DeliveryDecision, DeliveryEnvelope, DeliveryError,
    DeliveryFuture, ErrorCode, FenceResult, HandlerFuture, OutboxStore,
    runner::{
        ClaimAdmission, ClaimPermit, DeliveryContext, DeliveryHandler, DeliveryRunner,
        NoopAdmission,
    },
};
use serde_json::json;
use time::OffsetDateTime;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Clone, Copy)]
enum Preflight {
    Updated,
    Lost,
    Unknown,
}

fn fenced(mode: Preflight) -> Result<FenceResult, DeliveryError> {
    match mode {
        Preflight::Updated => Ok(FenceResult::Updated),
        Preflight::Lost => Ok(FenceResult::Lost),
        Preflight::Unknown => Err(DeliveryError::StoreUnknown),
    }
}

struct FakeStore {
    pending: Mutex<VecDeque<ClaimedEvent>>,
    claims: AtomicUsize,
    renewals: AtomicUsize,
    settlements: AtomicUsize,
    renew_mode: Preflight,
}

impl FakeStore {
    fn with_events(count: usize, renew_mode: Preflight) -> Self {
        let now = OffsetDateTime::now_utc();
        let pending = (1..=count)
            .map(|n| ClaimedEvent {
                envelope: DeliveryEnvelope {
                    event_id: Uuid::from_u128(n as u128),
                    event_type: "DocumentRegistered".into(),
                    aggregate_type: "Document".into(),
                    aggregate_id: Uuid::from_u128(100),
                    payload: json!({"test": n}),
                    occurred_at: now,
                },
                attempt: 1,
                attempt_limit: 8,
                lease_token: Uuid::from_u128(1_000 + n as u128),
                lease_owner: Uuid::from_u128(2_000),
                lease_expires_at: now + time::Duration::seconds(120),
            })
            .collect();
        Self {
            pending: Mutex::new(pending),
            claims: AtomicUsize::new(0),
            renewals: AtomicUsize::new(0),
            settlements: AtomicUsize::new(0),
            renew_mode,
        }
    }
}

impl OutboxStore for FakeStore {
    fn verify_policy(&self) -> DeliveryFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn claim(
        &self,
        _owner: Uuid,
        limit: u32,
        _lease: Duration,
    ) -> DeliveryFuture<'_, Vec<ClaimedEvent>> {
        Box::pin(async move {
            let mut pending = self.pending.lock().unwrap();
            let claimed: Vec<_> = (0..limit).filter_map(|_| pending.pop_front()).collect();
            self.claims.fetch_add(claimed.len(), Ordering::SeqCst);
            Ok(claimed)
        })
    }

    fn renew(
        &self,
        _event_id: Uuid,
        _lease_token: Uuid,
        _lease: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.renewals.fetch_add(1, Ordering::SeqCst);
            fenced(self.renew_mode)
        })
    }

    fn settle_success(
        &self,
        _event_id: Uuid,
        _lease_token: Uuid,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.settlements.fetch_add(1, Ordering::SeqCst);
            Ok(FenceResult::Updated)
        })
    }

    fn settle_failure(
        &self,
        _event_id: Uuid,
        _lease_token: Uuid,
        _code: ErrorCode,
        _terminal: bool,
        _backoff: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.settlements.fetch_add(1, Ordering::SeqCst);
            Ok(FenceResult::Updated)
        })
    }

    fn reap_exhausted(&self, _limit: u32) -> DeliveryFuture<'_, u64> {
        Box::pin(async { Ok(0) })
    }
}

#[derive(Clone)]
struct FakePermit {
    preflight_mode: Preflight,
    renew_mode: Preflight,
    preflights: Arc<AtomicUsize>,
    renewals: Arc<AtomicUsize>,
}

impl ClaimPermit for FakePermit {
    fn preflight(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.preflights.fetch_add(1, Ordering::SeqCst);
            fenced(self.preflight_mode)
        })
    }

    fn renew(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.renewals.fetch_add(1, Ordering::SeqCst);
            fenced(self.renew_mode)
        })
    }

    fn release(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async { Ok(FenceResult::Updated) })
    }
}

struct FakeAdmission {
    permit: Option<FakePermit>,
    max_claims_per_permit: u32,
    acquires: Arc<AtomicUsize>,
}

impl ClaimAdmission for FakeAdmission {
    type Permit = FakePermit;

    fn max_claims_per_permit(&self) -> u32 {
        self.max_claims_per_permit
    }

    fn acquire(&self) -> DeliveryFuture<'_, Option<Self::Permit>> {
        Box::pin(async {
            self.acquires.fetch_add(1, Ordering::SeqCst);
            Ok(self.permit.clone())
        })
    }
}

struct TestHandler {
    started: AtomicUsize,
    gate: Option<watch::Receiver<bool>>,
}

impl<P: ClaimPermit> DeliveryHandler<P> for TestHandler {
    fn deliver(
        &self,
        _envelope: DeliveryEnvelope,
        _context: DeliveryContext,
        _permit: P,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        Box::pin(async move {
            self.started.fetch_add(1, Ordering::SeqCst);
            if let Some(mut gate) = self.gate.clone() {
                while !*gate.borrow_and_update() {
                    gate.changed().await.expect("gate sender remains alive");
                }
            }
            DeliveryDecision::Applied
        })
    }
}

fn config(batch_size: u32, max_in_flight: u32) -> DeliveryConfig {
    DeliveryConfig {
        batch_size,
        max_in_flight,
        ..DeliveryConfig::default()
    }
}

async fn wait_for_count(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("expected dispatch did not start");
}

#[tokio::test]
async fn batch_32_inflight_8_never_claims_queued_work() {
    // A regression that claims batch_size rows before reserving slots leaves
    // 24 committed attempts waiting behind eight blocked handlers.
    let store = Arc::new(FakeStore::with_events(32, Preflight::Updated));
    let (release, gate) = watch::channel(false);
    let handler = Arc::new(TestHandler {
        started: AtomicUsize::new(0),
        gate: Some(gate),
    });
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        Arc::new(NoopAdmission),
        DeliveryConfig {
            lease_duration: Duration::from_secs(1),
            renew_interval: Duration::from_millis(200),
            ..config(32, 8)
        },
    )
    .unwrap();
    let task = tokio::spawn(async move { runner.run_cycle().await });
    wait_for_count(&handler.started, 8).await;
    assert_eq!(store.claims.load(Ordering::SeqCst), 8);
    assert_eq!(store.pending.lock().unwrap().len(), 24);
    assert_eq!(store.settlements.load(Ordering::SeqCst), 0);
    // Renewal must already be active for all eight claims while the handler
    // remains blocked. A queued row must not silently burn its lease.
    tokio::time::sleep(Duration::from_millis(450)).await;
    assert!(store.renewals.load(Ordering::SeqCst) > 8);
    release.send(true).unwrap();
    task.await.unwrap().unwrap();
    assert_eq!(store.settlements.load(Ordering::SeqCst), 8);
}

#[tokio::test]
async fn pre_dispatch_lost_never_calls_handler() {
    for (outbox, permit) in [
        (Preflight::Lost, Preflight::Updated),
        (Preflight::Unknown, Preflight::Updated),
        (Preflight::Updated, Preflight::Lost),
        (Preflight::Updated, Preflight::Unknown),
    ] {
        let store = Arc::new(FakeStore::with_events(1, outbox));
        let preflights = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(TestHandler {
            started: AtomicUsize::new(0),
            gate: None,
        });
        let admission = Arc::new(FakeAdmission {
            permit: Some(FakePermit {
                preflight_mode: permit,
                renew_mode: Preflight::Updated,
                preflights: preflights.clone(),
                renewals: Arc::new(AtomicUsize::new(0)),
            }),
            max_claims_per_permit: 1,
            acquires: Arc::new(AtomicUsize::new(0)),
        });
        let runner =
            DeliveryRunner::new(store.clone(), handler.clone(), admission, config(1, 1)).unwrap();
        let _ = runner.run_cycle().await;
        assert_eq!(store.claims.load(Ordering::SeqCst), 1);
        assert_eq!(handler.started.load(Ordering::SeqCst), 0);
        assert_eq!(store.settlements.load(Ordering::SeqCst), 0);
        assert!(store.renewals.load(Ordering::SeqCst) > 0);
        if matches!(outbox, Preflight::Updated) {
            assert!(preflights.load(Ordering::SeqCst) > 0);
        }
    }
}

#[tokio::test]
async fn search_admission_denied_claims_zero() {
    let store = Arc::new(FakeStore::with_events(1, Preflight::Updated));
    let handler = Arc::new(TestHandler {
        started: AtomicUsize::new(0),
        gate: None,
    });
    let acquires = Arc::new(AtomicUsize::new(0));
    let admission = Arc::new(FakeAdmission {
        permit: None,
        max_claims_per_permit: 1,
        acquires: acquires.clone(),
    });
    let runner =
        DeliveryRunner::new(store.clone(), handler.clone(), admission, config(1, 1)).unwrap();
    runner.run_cycle().await.unwrap();
    assert_eq!(acquires.load(Ordering::SeqCst), 1);
    assert_eq!(store.claims.load(Ordering::SeqCst), 0);
    assert_eq!(store.pending.lock().unwrap().len(), 1);
    assert_eq!(handler.started.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn source_style_admission_caps_a_generic_batch_to_one() {
    // A Source lease is exclusive across processes, so one permit may not
    // turn batch_size=32 into concurrent reconciliation of that Source.
    let store = Arc::new(FakeStore::with_events(4, Preflight::Updated));
    let handler = Arc::new(TestHandler {
        started: AtomicUsize::new(0),
        gate: None,
    });
    let admission = Arc::new(FakeAdmission {
        permit: Some(FakePermit {
            preflight_mode: Preflight::Updated,
            renew_mode: Preflight::Updated,
            preflights: Arc::new(AtomicUsize::new(0)),
            renewals: Arc::new(AtomicUsize::new(0)),
        }),
        max_claims_per_permit: 1,
        acquires: Arc::new(AtomicUsize::new(0)),
    });
    let runner =
        DeliveryRunner::new(store.clone(), handler.clone(), admission, config(32, 8)).unwrap();
    runner.run_cycle().await.unwrap();
    assert_eq!(store.claims.load(Ordering::SeqCst), 1);
    assert_eq!(store.pending.lock().unwrap().len(), 3);
    assert_eq!(handler.started.load(Ordering::SeqCst), 1);
    assert_eq!(store.settlements.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn short_remaining_source_permit_renew_lost_never_dispatches() {
    // A read-only preflight can observe a still-owned permit whose remaining
    // TTL is too short for dispatch. The runner must renew it, and a failed
    // renewal is a lost fence rather than handler authorization.
    let store = Arc::new(FakeStore::with_events(1, Preflight::Updated));
    let handler = Arc::new(TestHandler {
        started: AtomicUsize::new(0),
        gate: None,
    });
    let permit_renewals = Arc::new(AtomicUsize::new(0));
    let admission = Arc::new(FakeAdmission {
        permit: Some(FakePermit {
            preflight_mode: Preflight::Updated,
            renew_mode: Preflight::Lost,
            preflights: Arc::new(AtomicUsize::new(0)),
            renewals: permit_renewals.clone(),
        }),
        max_claims_per_permit: 1,
        acquires: Arc::new(AtomicUsize::new(0)),
    });
    let runner =
        DeliveryRunner::new(store.clone(), handler.clone(), admission, config(1, 1)).unwrap();
    let _ = runner.run_cycle().await;
    assert_eq!(store.claims.load(Ordering::SeqCst), 1);
    assert_eq!(permit_renewals.load(Ordering::SeqCst), 1);
    assert_eq!(handler.started.load(Ordering::SeqCst), 0);
    assert_eq!(store.settlements.load(Ordering::SeqCst), 0);
}
