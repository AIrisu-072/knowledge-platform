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

// These probes exercise the real runner with in-process unresolved dependency
// futures. An entered operation is not a confirmed response: only `returned`
// records that response, and dropping a pending future records `abandoned`.
// In particular, cancellation is not evidence that a remote commit rolled back.
#[derive(Default)]
struct OperationProbe {
    entered: AtomicUsize,
    returned: AtomicUsize,
    abandoned: AtomicUsize,
    pending_on: Option<usize>,
    delay_first: Option<Duration>,
    cancel_on_return: Option<(usize, Arc<PendingHandler>)>,
    unknown_on_return: Option<usize>,
}

impl OperationProbe {
    fn pending_on(call: usize) -> Self {
        Self {
            pending_on: Some(call),
            ..Self::default()
        }
    }

    fn cancelling_on_return(call: usize, handler: Arc<PendingHandler>) -> Self {
        Self {
            cancel_on_return: Some((call, handler)),
            ..Self::default()
        }
    }

    async fn run<T, F: future::Future<Output = Result<T, DeliveryError>>>(
        &self,
        operation: F,
    ) -> Result<T, DeliveryError> {
        let call = self.entered.fetch_add(1, Ordering::SeqCst) + 1;
        let mut guard = OperationGuard {
            probe: self,
            returned: false,
        };
        if call == 1
            && let Some(delay) = self.delay_first
        {
            tokio::time::sleep(delay).await;
        }
        if self.pending_on == Some(call) {
            future::pending::<()>().await;
        }
        let result = operation.await;
        guard.returned = true;
        self.returned.fetch_add(1, Ordering::SeqCst);
        // Inject cancellation at an exact fake response boundary using only
        // the real public signal captured after the handler was dispatched.
        if let Some((cancel_call, handler)) = &self.cancel_on_return
            && call == *cancel_call
        {
            handler
                .cancel
                .lock()
                .unwrap()
                .as_ref()
                .expect("response-boundary cancellation requires a started handler")
                .store(true, Ordering::SeqCst);
        }
        if self.unknown_on_return == Some(call) {
            Err(DeliveryError::StoreUnknown)
        } else {
            result
        }
    }

    fn assert_pending_was_abandoned(&self, call: usize) {
        let entered = self.entered.load(Ordering::SeqCst);
        assert!(entered >= call);
        assert_eq!(self.returned.load(Ordering::SeqCst), entered - 1);
        assert_eq!(self.abandoned.load(Ordering::SeqCst), 1);
    }
}

struct OperationGuard<'a> {
    probe: &'a OperationProbe,
    returned: bool,
}

impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        if !self.returned {
            self.probe.abandoned.fetch_add(1, Ordering::SeqCst);
        }
    }
}

struct PendingStore {
    inner: LifecycleStore,
    verify: OperationProbe,
    claim: OperationProbe,
    renew: OperationProbe,
    success: OperationProbe,
    failure: OperationProbe,
    reap: OperationProbe,
}

impl PendingStore {
    fn new(count: usize) -> Self {
        Self {
            inner: LifecycleStore::new(count),
            verify: OperationProbe::default(),
            claim: OperationProbe::default(),
            renew: OperationProbe::default(),
            success: OperationProbe::default(),
            failure: OperationProbe::default(),
            reap: OperationProbe::default(),
        }
    }

    fn assert_no_settlement(&self) {
        assert!(self.inner.acked.lock().unwrap().is_empty());
        assert_eq!(self.inner.fail_calls.load(Ordering::SeqCst), 0);
        assert_eq!(self.success.returned.load(Ordering::SeqCst), 0);
        assert_eq!(self.failure.returned.load(Ordering::SeqCst), 0);
    }
}

impl OutboxStore for PendingStore {
    fn verify_policy(&self) -> DeliveryFuture<'_, ()> {
        Box::pin(self.verify.run(self.inner.verify_policy()))
    }

    fn claim(
        &self,
        owner: Uuid,
        limit: u32,
        lease: Duration,
    ) -> DeliveryFuture<'_, Vec<ClaimedEvent>> {
        Box::pin(self.claim.run(self.inner.claim(owner, limit, lease)))
    }

    fn renew(&self, event: Uuid, token: Uuid, lease: Duration) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(self.renew.run(self.inner.renew(event, token, lease)))
    }

    fn settle_success(&self, event: Uuid, token: Uuid) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(self.success.run(self.inner.settle_success(event, token)))
    }

    fn settle_failure(
        &self,
        event: Uuid,
        token: Uuid,
        code: ErrorCode,
        terminal: bool,
        backoff: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(
            self.failure.run(
                self.inner
                    .settle_failure(event, token, code, terminal, backoff),
            ),
        )
    }

    fn reap_exhausted(&self, limit: u32) -> DeliveryFuture<'_, u64> {
        Box::pin(self.reap.run(self.inner.reap_exhausted(limit)))
    }
}

#[derive(Default)]
struct PendingPermitState {
    preflight: OperationProbe,
    renew: OperationProbe,
    release: OperationProbe,
    released: AtomicBool,
}

#[derive(Clone, Default)]
struct PendingPermit(Arc<PendingPermitState>);

impl ClaimPermit for PendingPermit {
    fn preflight(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(self.0.preflight.run(async { Ok(FenceResult::Updated) }))
    }

    fn renew(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(self.0.renew.run(async { Ok(FenceResult::Updated) }))
    }

    fn release(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(self.0.release.run(async {
            self.0.released.store(true, Ordering::SeqCst);
            Ok(FenceResult::Updated)
        }))
    }
}

struct PendingAdmission {
    acquire: OperationProbe,
    permit: PendingPermit,
}

impl ClaimAdmission for PendingAdmission {
    type Permit = PendingPermit;

    fn acquire(&self) -> DeliveryFuture<'_, Option<Self::Permit>> {
        Box::pin(self.acquire.run(async { Ok(Some(self.permit.clone())) }))
    }

    fn max_claims_per_permit(&self) -> u32 {
        1
    }
}

struct PendingHandler {
    started: AtomicUsize,
    completed: AtomicUsize,
    cancel: Mutex<Option<Arc<AtomicBool>>>,
    decision: Option<DeliveryDecision>,
    delay: Duration,
    resume_when_cancelled: bool,
}

impl PendingHandler {
    fn new(decision: Option<DeliveryDecision>) -> Self {
        Self {
            started: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
            cancel: Mutex::new(None),
            decision,
            delay: Duration::ZERO,
            resume_when_cancelled: false,
        }
    }

    fn assert_cancelled(&self) {
        assert!(
            self.cancel
                .lock()
                .unwrap()
                .as_ref()
                .expect("handler started")
                .load(Ordering::SeqCst)
        );
    }
}

impl DeliveryHandler<PendingPermit> for PendingHandler {
    fn deliver(
        &self,
        _event: DeliveryEnvelope,
        context: DeliveryContext,
        _permit: PendingPermit,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        Box::pin(async move {
            *self.cancel.lock().unwrap() = Some(context.cancel);
            self.started.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            if self.resume_when_cancelled {
                // This deliberately uncooperative handler makes an extra poll
                // after cancellation observable. The heartbeat drives polling;
                // no background task or timing race changes the flag here.
                return future::poll_fn(|_| {
                    if self
                        .cancel
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .load(Ordering::SeqCst)
                    {
                        self.completed.fetch_add(1, Ordering::SeqCst);
                        std::task::Poll::Ready(DeliveryDecision::Applied)
                    } else {
                        std::task::Poll::Pending
                    }
                })
                .await;
            }
            let Some(decision) = self.decision else {
                return future::pending().await;
            };
            self.completed.fetch_add(1, Ordering::SeqCst);
            decision
        })
    }
}

fn pending_config() -> DeliveryConfig {
    DeliveryConfig {
        batch_size: 1,
        max_in_flight: 1,
        renew_interval: Duration::from_millis(50),
        max_processing: Duration::from_millis(250),
        drain_timeout: Duration::from_millis(100),
        ..config()
    }
}

// This is a test watchdog, not a replacement runner budget. Abort and join on
// failure so an intentional RED cannot leave a detached runner task behind.
async fn bounded_join<T>(mut task: tokio::task::JoinHandle<T>, budget: Duration, label: &str) -> T {
    match tokio::time::timeout(budget + Duration::from_millis(500), &mut task).await {
        Ok(result) => result.expect("runner task panicked"),
        Err(_) => {
            task.abort();
            let _ = task.await;
            panic!("{label}: runner exceeded its configured budget and watchdog slack");
        }
    }
}

async fn assert_processing_unknown(
    store: Arc<PendingStore>,
    permit: PendingPermit,
    handler: Arc<PendingHandler>,
    blocked: &OperationProbe,
    call: usize,
    expected_started: usize,
) {
    let cfg = pending_config();
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        Arc::new(PendingAdmission {
            acquire: OperationProbe::default(),
            permit,
        }),
        cfg,
    )
    .unwrap();
    let task = tokio::spawn(async move { runner.run_cycle().await });
    wait_for(&blocked.entered, call).await;
    let result = bounded_join(task, cfg.max_processing, "max_processing").await;
    assert_eq!(
        result,
        Err(DeliveryError::StoreUnknown),
        "an unresolved operation is unknown, not a confirmed zero-row fence"
    );
    blocked.assert_pending_was_abandoned(call);
    assert_eq!(handler.started.load(Ordering::SeqCst), expected_started);
    if expected_started > 0 {
        handler.assert_cancelled();
    }
    store.assert_no_settlement();
}

#[tokio::test]
async fn total_deadline_bounds_pending_outbox_heartbeat() {
    let store = Arc::new(PendingStore {
        renew: OperationProbe::pending_on(2),
        ..PendingStore::new(1)
    });
    assert_processing_unknown(
        store.clone(),
        PendingPermit::default(),
        Arc::new(PendingHandler::new(None)),
        &store.renew,
        2,
        1,
    )
    .await;
}

#[tokio::test]
async fn total_deadline_bounds_pending_source_heartbeat() {
    let permit = PendingPermit(Arc::new(PendingPermitState {
        renew: OperationProbe::pending_on(2),
        ..PendingPermitState::default()
    }));
    assert_processing_unknown(
        Arc::new(PendingStore::new(1)),
        permit.clone(),
        Arc::new(PendingHandler::new(None)),
        &permit.0.renew,
        2,
        1,
    )
    .await;
}

#[tokio::test]
async fn total_deadline_bounds_pending_initial_outbox_preflight() {
    let store = Arc::new(PendingStore {
        renew: OperationProbe::pending_on(1),
        ..PendingStore::new(1)
    });
    assert_processing_unknown(
        store.clone(),
        PendingPermit::default(),
        Arc::new(PendingHandler::new(None)),
        &store.renew,
        1,
        0,
    )
    .await;
}

#[tokio::test]
async fn total_deadline_bounds_pending_initial_source_renewal() {
    let permit = PendingPermit(Arc::new(PendingPermitState {
        renew: OperationProbe::pending_on(1),
        ..PendingPermitState::default()
    }));
    assert_processing_unknown(
        Arc::new(PendingStore::new(1)),
        permit.clone(),
        Arc::new(PendingHandler::new(None)),
        &permit.0.renew,
        1,
        0,
    )
    .await;
}

#[tokio::test]
async fn total_deadline_bounds_pending_initial_source_preflight() {
    let permit = PendingPermit(Arc::new(PendingPermitState {
        preflight: OperationProbe::pending_on(1),
        ..PendingPermitState::default()
    }));
    assert_processing_unknown(
        Arc::new(PendingStore::new(1)),
        permit.clone(),
        Arc::new(PendingHandler::new(None)),
        &permit.0.preflight,
        1,
        0,
    )
    .await;
}

#[tokio::test]
async fn total_deadline_bounds_pending_post_handler_outbox_preflight() {
    let store = Arc::new(PendingStore {
        renew: OperationProbe::pending_on(2),
        ..PendingStore::new(1)
    });
    let handler = Arc::new(PendingHandler::new(Some(DeliveryDecision::Applied)));
    assert_processing_unknown(
        store.clone(),
        PendingPermit::default(),
        handler.clone(),
        &store.renew,
        2,
        1,
    )
    .await;
    assert_eq!(handler.completed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn total_deadline_bounds_pending_post_handler_source_renewal() {
    let permit = PendingPermit(Arc::new(PendingPermitState {
        renew: OperationProbe::pending_on(2),
        ..PendingPermitState::default()
    }));
    let handler = Arc::new(PendingHandler::new(Some(DeliveryDecision::Applied)));
    assert_processing_unknown(
        Arc::new(PendingStore::new(1)),
        permit.clone(),
        handler.clone(),
        &permit.0.renew,
        2,
        1,
    )
    .await;
    assert_eq!(handler.completed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn total_deadline_bounds_pending_post_handler_source_preflight() {
    let permit = PendingPermit(Arc::new(PendingPermitState {
        preflight: OperationProbe::pending_on(2),
        ..PendingPermitState::default()
    }));
    let handler = Arc::new(PendingHandler::new(Some(DeliveryDecision::Applied)));
    assert_processing_unknown(
        Arc::new(PendingStore::new(1)),
        permit.clone(),
        handler.clone(),
        &permit.0.preflight,
        2,
        1,
    )
    .await;
    assert_eq!(handler.completed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn total_deadline_bounds_pending_success_settlement_without_confirming_ack() {
    let store = Arc::new(PendingStore {
        success: OperationProbe::pending_on(1),
        ..PendingStore::new(1)
    });
    assert_processing_unknown(
        store.clone(),
        PendingPermit::default(),
        Arc::new(PendingHandler::new(Some(DeliveryDecision::Applied))),
        &store.success,
        1,
        1,
    )
    .await;
}

#[tokio::test]
async fn total_deadline_bounds_pending_failure_settlement_without_confirming_retry() {
    let store = Arc::new(PendingStore {
        failure: OperationProbe::pending_on(1),
        ..PendingStore::new(1)
    });
    assert_processing_unknown(
        store.clone(),
        PendingPermit::default(),
        Arc::new(PendingHandler::new(Some(DeliveryDecision::Retryable(
            ErrorCode::IndexingFailed,
        )))),
        &store.failure,
        1,
        1,
    )
    .await;
}

#[tokio::test]
async fn initial_preflight_time_counts_toward_total_processing_deadline() {
    let cfg = pending_config();
    // Both stages individually fit 250 ms; together they exceed it. Restarting
    // the processing timer after preflight would incorrectly acknowledge this.
    let store = Arc::new(PendingStore {
        renew: OperationProbe {
            delay_first: Some(Duration::from_millis(180)),
            ..OperationProbe::default()
        },
        ..PendingStore::new(1)
    });
    let handler = Arc::new(PendingHandler {
        delay: Duration::from_millis(180),
        ..PendingHandler::new(Some(DeliveryDecision::Applied))
    });
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        Arc::new(PendingAdmission {
            acquire: OperationProbe::default(),
            permit: PendingPermit::default(),
        }),
        cfg,
    )
    .unwrap();
    let task = tokio::spawn(async move { runner.run_cycle().await });
    let summary = bounded_join(task, cfg.max_processing, "total processing budget")
        .await
        .unwrap();
    assert_eq!(summary.claimed, 1);
    assert_eq!(
        summary.settled, 0,
        "preflight must not reset the claim's processing deadline"
    );
    assert_eq!(summary.lost, 1);
    assert_eq!(handler.started.load(Ordering::SeqCst), 1);
    assert_eq!(handler.completed.load(Ordering::SeqCst), 0);
    handler.assert_cancelled();
    store.assert_no_settlement();
}

async fn assert_shutdown_unknown(
    store: Arc<PendingStore>,
    admission: Arc<PendingAdmission>,
    handler: Arc<PendingHandler>,
    blocked: &OperationProbe,
    call: usize,
    shutdown_after_handler: bool,
) {
    let cfg = pending_config();
    let runner = DeliveryRunner::new(store.clone(), handler.clone(), admission, cfg).unwrap();
    let (shutdown, receiver) = watch::channel(false);
    let task = tokio::spawn(async move { runner.run_until_shutdown(receiver).await });
    if shutdown_after_handler {
        wait_for(&handler.started, 1).await;
    } else {
        wait_for(&blocked.entered, call).await;
    }
    shutdown.send(true).unwrap();
    let result = bounded_join(task, cfg.drain_timeout, "shutdown drain_timeout").await;
    match result {
        Err(err) => assert_eq!(err, DeliveryError::StoreUnknown),
        Ok(summary) => {
            assert_eq!(summary.settled, 0);
            assert_eq!(summary.reaped, 0);
            assert!(
                summary.outages > 0,
                "unresolved shutdown work must remain observable as unknown"
            );
        }
    }
    blocked.assert_pending_was_abandoned(call);
    if shutdown_after_handler {
        handler.assert_cancelled();
    } else {
        assert_eq!(handler.started.load(Ordering::SeqCst), 0);
    }
    store.assert_no_settlement();
}

fn immediate_admission(permit: PendingPermit) -> Arc<PendingAdmission> {
    Arc::new(PendingAdmission {
        acquire: OperationProbe::default(),
        permit,
    })
}

#[tokio::test]
async fn shutdown_interrupts_pending_policy_verification() {
    let store = Arc::new(PendingStore {
        verify: OperationProbe::pending_on(1),
        ..PendingStore::new(1)
    });
    let admission = immediate_admission(PendingPermit::default());
    assert_shutdown_unknown(
        store.clone(),
        admission.clone(),
        Arc::new(PendingHandler::new(None)),
        &store.verify,
        1,
        false,
    )
    .await;
    assert_eq!(admission.acquire.entered.load(Ordering::SeqCst), 0);
    assert_eq!(store.claim.entered.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn shutdown_interrupts_pending_cycle_reaper() {
    let store = Arc::new(PendingStore {
        reap: OperationProbe::pending_on(1),
        ..PendingStore::new(1)
    });
    let admission = immediate_admission(PendingPermit::default());
    assert_shutdown_unknown(
        store.clone(),
        admission.clone(),
        Arc::new(PendingHandler::new(None)),
        &store.reap,
        1,
        false,
    )
    .await;
    assert_eq!(admission.acquire.entered.load(Ordering::SeqCst), 0);
    assert_eq!(store.claim.entered.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn shutdown_interrupts_pending_admission_without_claiming() {
    let store = Arc::new(PendingStore::new(1));
    let admission = Arc::new(PendingAdmission {
        acquire: OperationProbe::pending_on(1),
        permit: PendingPermit::default(),
    });
    assert_shutdown_unknown(
        store.clone(),
        admission.clone(),
        Arc::new(PendingHandler::new(None)),
        &admission.acquire,
        1,
        false,
    )
    .await;
    assert_eq!(store.claim.entered.load(Ordering::SeqCst), 0);
    assert_eq!(admission.permit.0.release.entered.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn shutdown_interrupts_pending_claim_without_dispatching() {
    let store = Arc::new(PendingStore {
        claim: OperationProbe::pending_on(1),
        ..PendingStore::new(1)
    });
    let permit = PendingPermit::default();
    assert_shutdown_unknown(
        store.clone(),
        immediate_admission(permit.clone()),
        Arc::new(PendingHandler::new(None)),
        &store.claim,
        1,
        false,
    )
    .await;
    assert_eq!(store.renew.entered.load(Ordering::SeqCst), 0);
    assert_eq!(permit.0.release.returned.load(Ordering::SeqCst), 1);
    assert!(permit.0.released.load(Ordering::SeqCst));
}

#[tokio::test]
async fn shutdown_bounds_pending_source_release_after_drain() {
    let store = Arc::new(PendingStore::new(1));
    let permit = PendingPermit(Arc::new(PendingPermitState {
        release: OperationProbe::pending_on(1),
        ..PendingPermitState::default()
    }));
    assert_shutdown_unknown(
        store,
        immediate_admission(permit.clone()),
        Arc::new(PendingHandler::new(None)),
        &permit.0.release,
        1,
        true,
    )
    .await;
    assert!(
        !permit.0.released.load(Ordering::SeqCst),
        "an unresolved release is not a confirmed release"
    );
}

#[tokio::test]
async fn shutdown_bounds_pending_final_reaper_without_confirming_reap() {
    let store = Arc::new(PendingStore {
        reap: OperationProbe::pending_on(2),
        ..PendingStore::new(1)
    });
    assert_shutdown_unknown(
        store.clone(),
        immediate_admission(PendingPermit::default()),
        Arc::new(PendingHandler::new(None)),
        &store.reap,
        2,
        true,
    )
    .await;
    assert_eq!(
        store.inner.reap_calls.load(Ordering::SeqCst),
        1,
        "only the initial reaper returned"
    );
}

// Wait for the exact task-phase I/O before signaling shutdown. The shorter
// drain budget must preserve uncertainty when it aborts an unresolved response;
// a plain hanging handler and a confirmed zero-row fence are different cases.
async fn assert_shutdown_task_io_unknown(
    store: Arc<PendingStore>,
    permit: PendingPermit,
    handler: Arc<PendingHandler>,
    blocked: &OperationProbe,
    call: usize,
    expected_started: usize,
) {
    let cfg = pending_config();
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        immediate_admission(permit),
        cfg,
    )
    .unwrap();
    let (shutdown, receiver) = watch::channel(false);
    let task = tokio::spawn(async move { runner.run_until_shutdown(receiver).await });
    wait_for(&blocked.entered, call).await;
    assert_eq!(handler.started.load(Ordering::SeqCst), expected_started);
    assert_eq!(blocked.returned.load(Ordering::SeqCst), call - 1);
    shutdown.send(true).unwrap();
    let result = bounded_join(task, cfg.drain_timeout, "shutdown with unresolved task I/O").await;
    assert_eq!(
        result,
        Err(DeliveryError::StoreUnknown),
        "drain cancellation cannot turn an unresolved dependency response into confirmed Lost"
    );
    // Aborted child tasks can observe their cancellation on the next scheduler
    // turn, after the runner has returned; require their future to be dropped.
    wait_for(&blocked.abandoned, 1).await;
    blocked.assert_pending_was_abandoned(call);
    assert_eq!(handler.started.load(Ordering::SeqCst), expected_started);
    if expected_started > 0 {
        handler.assert_cancelled();
    }
    store.assert_no_settlement();
}

#[tokio::test]
async fn shutdown_pending_outbox_heartbeat_remains_unknown() {
    let store = Arc::new(PendingStore {
        renew: OperationProbe::pending_on(2),
        ..PendingStore::new(1)
    });
    assert_shutdown_task_io_unknown(
        store.clone(),
        PendingPermit::default(),
        Arc::new(PendingHandler::new(None)),
        &store.renew,
        2,
        1,
    )
    .await;
}

#[tokio::test]
async fn shutdown_pending_source_heartbeat_remains_unknown() {
    let permit = PendingPermit(Arc::new(PendingPermitState {
        renew: OperationProbe::pending_on(2),
        ..PendingPermitState::default()
    }));
    assert_shutdown_task_io_unknown(
        Arc::new(PendingStore::new(1)),
        permit.clone(),
        Arc::new(PendingHandler::new(None)),
        &permit.0.renew,
        2,
        1,
    )
    .await;
}

#[tokio::test]
async fn shutdown_pending_initial_outbox_preflight_remains_unknown() {
    let store = Arc::new(PendingStore {
        renew: OperationProbe::pending_on(1),
        ..PendingStore::new(1)
    });
    assert_shutdown_task_io_unknown(
        store.clone(),
        PendingPermit::default(),
        Arc::new(PendingHandler::new(None)),
        &store.renew,
        1,
        0,
    )
    .await;
}

#[tokio::test]
async fn shutdown_pending_initial_source_preflight_remains_unknown() {
    let permit = PendingPermit(Arc::new(PendingPermitState {
        preflight: OperationProbe::pending_on(1),
        ..PendingPermitState::default()
    }));
    assert_shutdown_task_io_unknown(
        Arc::new(PendingStore::new(1)),
        permit.clone(),
        Arc::new(PendingHandler::new(None)),
        &permit.0.preflight,
        1,
        0,
    )
    .await;
}

#[tokio::test]
async fn shutdown_pending_success_settlement_remains_unknown() {
    let store = Arc::new(PendingStore {
        success: OperationProbe::pending_on(1),
        ..PendingStore::new(1)
    });
    assert_shutdown_task_io_unknown(
        store.clone(),
        PendingPermit::default(),
        Arc::new(PendingHandler::new(Some(DeliveryDecision::Applied))),
        &store.success,
        1,
        1,
    )
    .await;
}

#[tokio::test]
async fn shutdown_pending_failure_settlement_remains_unknown() {
    let store = Arc::new(PendingStore {
        failure: OperationProbe::pending_on(1),
        ..PendingStore::new(1)
    });
    assert_shutdown_task_io_unknown(
        store.clone(),
        PendingPermit::default(),
        Arc::new(PendingHandler::new(Some(DeliveryDecision::Retryable(
            ErrorCode::IndexingFailed,
        )))),
        &store.failure,
        1,
        1,
    )
    .await;
}

#[tokio::test]
async fn pending_heartbeat_becomes_unknown_before_lease_expiry() {
    let cfg = DeliveryConfig {
        max_processing: Duration::from_secs(2),
        ..pending_config()
    };
    assert!(cfg.max_processing > cfg.lease_duration);
    let store = Arc::new(PendingStore {
        renew: OperationProbe::pending_on(2),
        ..PendingStore::new(1)
    });
    let handler = Arc::new(PendingHandler::new(None));
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        immediate_admission(PendingPermit::default()),
        cfg,
    )
    .unwrap();
    let started = tokio::time::Instant::now();
    let task = tokio::spawn(async move { runner.run_cycle().await });
    wait_for(&store.renew.entered, 2).await;
    let result = bounded_join(task, cfg.lease_duration, "unresolved heartbeat lease bound").await;
    assert!(
        started.elapsed() < cfg.lease_duration,
        "waiting for max_processing would keep unresolved ownership past lease expiry"
    );
    assert_eq!(result, Err(DeliveryError::StoreUnknown));
    store.renew.assert_pending_was_abandoned(2);
    handler.assert_cancelled();
    store.assert_no_settlement();
}

#[tokio::test]
async fn cancellation_before_heartbeat_prevents_new_renewal_attempts() {
    let cfg = DeliveryConfig {
        renew_interval: Duration::from_millis(200),
        max_processing: Duration::from_secs(1),
        ..pending_config()
    };
    let store = Arc::new(PendingStore::new(1));
    let permit = PendingPermit::default();
    let handler = Arc::new(PendingHandler::new(None));
    let runner = DeliveryRunner::new(
        store.clone(),
        handler.clone(),
        immediate_admission(permit.clone()),
        cfg,
    )
    .unwrap();
    let task = tokio::spawn(async move { runner.run_cycle().await });
    wait_for(&handler.started, 1).await;
    assert_eq!(store.renew.entered.load(Ordering::SeqCst), 1);
    assert_eq!(permit.0.renew.entered.load(Ordering::SeqCst), 1);
    // The handler has yielded pending on this current-thread runtime. Set the
    // real public cancellation signal before its first heartbeat is eligible.
    // This witnesses the missing cancellation check, not every multicore race.
    handler
        .cancel
        .lock()
        .unwrap()
        .as_ref()
        .expect("handler has captured its cancellation signal")
        .store(true, Ordering::SeqCst);
    let result = bounded_join(task, cfg.max_processing, "cancelled claim lifecycle").await;
    assert_eq!(
        (
            store.renew.entered.load(Ordering::SeqCst),
            permit.0.renew.entered.load(Ordering::SeqCst),
        ),
        (1, 1),
        "cancellation must prevent heartbeat I/O after the confirmed initial preflight"
    );
    let summary = result.expect("cancellation before I/O has no unresolved dependency response");
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.settled, 0);
    assert_eq!(summary.lost, 1);
    assert_eq!(handler.completed.load(Ordering::SeqCst), 0);
    handler.assert_cancelled();
    store.assert_no_settlement();
}

async fn run_response_boundary_cycle(
    store: Arc<PendingStore>,
    permit: PendingPermit,
    handler: Arc<PendingHandler>,
) -> Result<outbox_delivery::runner::CycleSummary, DeliveryError> {
    let cfg = pending_config();
    let runner = DeliveryRunner::new(store, handler, immediate_admission(permit), cfg).unwrap();
    let task = tokio::spawn(async move { runner.run_cycle().await });
    bounded_join(task, cfg.max_processing, "response-boundary cancellation").await
}

fn assert_response_boundary_cancelled(
    result: Result<outbox_delivery::runner::CycleSummary, DeliveryError>,
    store: &PendingStore,
    handler: &PendingHandler,
) {
    let summary = result.expect("all started fence operations returned Updated");
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.settled, 0);
    assert_eq!(summary.lost, 1);
    handler.assert_cancelled();
    store.assert_no_settlement();
    assert_eq!(store.success.entered.load(Ordering::SeqCst), 0);
    assert_eq!(store.failure.entered.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancellation_during_final_outbox_response_stops_source_fences() {
    let handler = Arc::new(PendingHandler::new(Some(DeliveryDecision::Applied)));
    let store = Arc::new(PendingStore {
        renew: OperationProbe::cancelling_on_return(2, handler.clone()),
        ..PendingStore::new(1)
    });
    let permit = PendingPermit::default();
    let result = run_response_boundary_cycle(store.clone(), permit.clone(), handler.clone()).await;
    assert_eq!(store.renew.returned.load(Ordering::SeqCst), 2);
    assert_eq!(
        (
            permit.0.renew.entered.load(Ordering::SeqCst),
            permit.0.preflight.entered.load(Ordering::SeqCst),
        ),
        (1, 1),
        "cancellation during final outbox renewal must stop both following Source fences"
    );
    assert_response_boundary_cancelled(result, &store, &handler);
}

#[tokio::test]
async fn cancellation_during_final_source_renewal_stops_source_preflight() {
    let handler = Arc::new(PendingHandler::new(Some(DeliveryDecision::Applied)));
    let store = Arc::new(PendingStore::new(1));
    let permit = PendingPermit(Arc::new(PendingPermitState {
        renew: OperationProbe::cancelling_on_return(2, handler.clone()),
        ..PendingPermitState::default()
    }));
    let result = run_response_boundary_cycle(store.clone(), permit.clone(), handler.clone()).await;
    assert_eq!(permit.0.renew.returned.load(Ordering::SeqCst), 2);
    assert_eq!(
        permit.0.preflight.entered.load(Ordering::SeqCst),
        1,
        "cancellation during final Source renewal must stop the following preflight"
    );
    assert_response_boundary_cancelled(result, &store, &handler);
}

#[tokio::test]
async fn cancellation_during_final_source_preflight_stops_settlement() {
    let handler = Arc::new(PendingHandler::new(Some(DeliveryDecision::Applied)));
    let store = Arc::new(PendingStore::new(1));
    let permit = PendingPermit(Arc::new(PendingPermitState {
        preflight: OperationProbe::cancelling_on_return(2, handler.clone()),
        ..PendingPermitState::default()
    }));
    let result = run_response_boundary_cycle(store.clone(), permit.clone(), handler.clone()).await;
    assert_eq!(permit.0.preflight.returned.load(Ordering::SeqCst), 2);
    assert_eq!(
        store.success.entered.load(Ordering::SeqCst),
        0,
        "cancellation during the final Source preflight must stop settlement"
    );
    assert_response_boundary_cancelled(result, &store, &handler);
}

#[tokio::test]
async fn cancellation_during_heartbeat_outbox_response_stops_source_renewal() {
    let handler = Arc::new(PendingHandler::new(None));
    let store = Arc::new(PendingStore {
        renew: OperationProbe::cancelling_on_return(2, handler.clone()),
        ..PendingStore::new(1)
    });
    let permit = PendingPermit::default();
    let result = run_response_boundary_cycle(store.clone(), permit.clone(), handler.clone()).await;
    assert_eq!(store.renew.returned.load(Ordering::SeqCst), 2);
    assert_eq!(
        permit.0.renew.entered.load(Ordering::SeqCst),
        1,
        "cancellation during an outbox heartbeat must stop the following Source renewal"
    );
    assert_eq!(handler.completed.load(Ordering::SeqCst), 0);
    assert_response_boundary_cancelled(result, &store, &handler);
}

#[tokio::test]
async fn cancellation_during_heartbeat_source_response_stops_handler_polling() {
    let handler = Arc::new(PendingHandler {
        resume_when_cancelled: true,
        ..PendingHandler::new(None)
    });
    let store = Arc::new(PendingStore::new(1));
    let permit = PendingPermit(Arc::new(PendingPermitState {
        renew: OperationProbe::cancelling_on_return(2, handler.clone()),
        ..PendingPermitState::default()
    }));
    let result = run_response_boundary_cycle(store.clone(), permit.clone(), handler.clone()).await;
    assert_eq!(permit.0.renew.returned.load(Ordering::SeqCst), 2);
    assert_eq!(
        handler.completed.load(Ordering::SeqCst),
        0,
        "a cancelled Source heartbeat must not resume handler work"
    );
    assert_response_boundary_cancelled(result, &store, &handler);
}

#[tokio::test]
async fn returned_store_unknown_is_preserved_when_final_preflight_also_cancels() {
    let handler = Arc::new(PendingHandler::new(Some(DeliveryDecision::Applied)));
    let store = Arc::new(PendingStore::new(1));
    let permit = PendingPermit(Arc::new(PendingPermitState {
        preflight: OperationProbe {
            unknown_on_return: Some(2),
            ..OperationProbe::cancelling_on_return(2, handler.clone())
        },
        ..PendingPermitState::default()
    }));
    let result = run_response_boundary_cycle(store.clone(), permit.clone(), handler.clone()).await;
    assert_eq!(result, Err(DeliveryError::StoreUnknown));
    assert_eq!(permit.0.preflight.returned.load(Ordering::SeqCst), 2);
    handler.assert_cancelled();
    store.assert_no_settlement();
    assert_eq!(store.success.entered.load(Ordering::SeqCst), 0);
    assert_eq!(store.failure.entered.load(Ordering::SeqCst), 0);
}
