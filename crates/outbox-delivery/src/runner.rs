//! Bounded, generic dispatch of fenced outbox claims.

use std::future::Future;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use time::OffsetDateTime;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, watch},
    task::JoinSet,
    time::{Instant, MissedTickBehavior},
};
use uuid::Uuid;

use crate::{
    DeliveryConfig, DeliveryDecision, DeliveryEnvelope, DeliveryError, DeliveryFuture, FenceResult,
    HandlerFuture, OutboxStore,
    observe::{
        DeliveryMetricKind as Metric, DeliveryObserver, DeliveryOutcome as Outcome, DeliveryRoute,
        DeliverySpan, HandlerTiming, Observation, SpanTiming, ValidatedTrace,
    },
    retry_delay_within,
};

/// A separate admission fence, such as a distributed Source lease.
pub trait ClaimPermit: Clone + Send + Sync {
    /// Check the current owner/fence without assuming this extends its TTL.
    fn preflight(&self) -> DeliveryFuture<'_, FenceResult>;
    /// Extend the current owner/fence lease before handler work or on heartbeat.
    fn renew(&self) -> DeliveryFuture<'_, FenceResult>;
    fn release(&self) -> DeliveryFuture<'_, FenceResult>;
}

/// Acquire admission before claiming an outbox row. `None` is contention.
pub trait ClaimAdmission: Send + Sync {
    type Permit: ClaimPermit;

    fn acquire(&self) -> DeliveryFuture<'_, Option<Self::Permit>>;

    /// A single admission permit can cover a generic batch. Exclusive Source
    /// admissions override this with one, regardless of runner batch size.
    fn max_claims_per_permit(&self) -> u32 {
        32
    }
}

#[derive(Clone, Copy, Default)]
pub struct NoopPermit;

impl ClaimPermit for NoopPermit {
    fn preflight(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async { Ok(FenceResult::Updated) })
    }

    fn renew(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async { Ok(FenceResult::Updated) })
    }

    fn release(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async { Ok(FenceResult::Updated) })
    }
}

#[derive(Clone, Copy, Default)]
pub struct NoopAdmission;

impl ClaimAdmission for NoopAdmission {
    type Permit = NoopPermit;

    fn acquire(&self) -> DeliveryFuture<'_, Option<Self::Permit>> {
        Box::pin(async { Ok(Some(NoopPermit)) })
    }
}

#[derive(Clone)]
pub struct DeliveryContext {
    pub attempt: i32,
    pub outbox_token: Uuid,
    pub outbox_deadline: OffsetDateTime,
    pub cancel: Arc<AtomicBool>,
    /// 観測を有効にした配送の新しいスパン文脈。旧行も新規ルートを持つ。
    pub trace: Option<ValidatedTrace>,
}

pub trait DeliveryHandler<P: ClaimPermit>: Send + Sync {
    fn deliver(
        &self,
        envelope: DeliveryEnvelope,
        context: DeliveryContext,
        permit: P,
    ) -> HandlerFuture<'_, DeliveryDecision>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CycleSummary {
    pub claimed: u32,
    pub settled: u32,
    pub lost: u32,
    pub reaped: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RunSummary {
    pub cycles: u64,
    pub claimed: u64,
    pub settled: u64,
    pub lost: u64,
    pub reaped: u64,
    pub outages: u64,
}

pub struct DeliveryRunner<S, H, A>
where
    S: OutboxStore,
    A: ClaimAdmission,
    H: DeliveryHandler<A::Permit>,
{
    store: Arc<S>,
    handler: Arc<H>,
    admission: Arc<A>,
    config: DeliveryConfig,
    owner: Uuid,
    slots: Arc<Semaphore>,
    observation: Observation,
}

impl<S, H, A> DeliveryRunner<S, H, A>
where
    S: OutboxStore + 'static,
    A: ClaimAdmission,
    A::Permit: 'static,
    H: DeliveryHandler<A::Permit> + 'static,
{
    pub fn new(
        store: Arc<S>,
        handler: Arc<H>,
        admission: Arc<A>,
        config: DeliveryConfig,
    ) -> Result<Self, DeliveryError> {
        config.validate()?;
        Ok(Self {
            store,
            handler,
            admission,
            config,
            owner: Uuid::now_v7(),
            slots: Arc::new(Semaphore::new(config.max_in_flight as usize)),
            observation: Observation {
                observer: None,
                route: DeliveryRoute::Generic,
            },
        })
    }

    pub fn with_observer(
        mut self,
        observer: Arc<dyn DeliveryObserver>,
        route: DeliveryRoute,
    ) -> Self {
        self.observation = Observation {
            observer: Some(observer),
            route,
        };
        self
    }

    pub async fn run_cycle(&self) -> Result<CycleSummary, DeliveryError> {
        self.run_cycle_inner(None, &OnceLock::new()).await
    }

    async fn run_cycle_inner(
        &self,
        shutdown: Option<&mut watch::Receiver<bool>>,
        drain: &OnceLock<Instant>,
    ) -> Result<CycleSummary, DeliveryError> {
        let result = self.run_cycle_work(shutdown, drain).await;
        // 失敗したサイクルを一度だけ数える。Unknown を成功や stale に置換しない。
        if let Err(error) = &result {
            self.observation.error(error);
        }
        result
    }

    async fn run_cycle_work(
        &self,
        mut shutdown: Option<&mut watch::Receiver<bool>>,
        drain: &OnceLock<Instant>,
    ) -> Result<CycleSummary, DeliveryError> {
        if shutdown_requested(shutdown.as_deref(), drain, self.config) {
            return Ok(CycleSummary::default());
        }
        prepare_operation(
            self.store.verify_policy(),
            &mut shutdown,
            drain,
            self.config,
        )
        .await?;
        if self.observation.observer.is_some() {
            match prepare_operation(
                self.store.queue_snapshot(),
                &mut shutdown,
                drain,
                self.config,
            )
            .await
            {
                Ok(Some(snapshot)) => self.observation.snapshot(snapshot),
                Ok(None) => {}
                Err(error) => self.observation.error(&error),
            }
        }
        let reaped = prepare_operation(
            self.store.reap_exhausted(self.config.reap_batch),
            &mut shutdown,
            drain,
            self.config,
        )
        .await?;
        self.observation.record(
            Metric::Reaped,
            reaped as f64,
            Outcome::Exhausted,
            Some(crate::ErrorCode::DeliveryUnknownAtLimit),
        );
        let mut summary = CycleSummary {
            reaped,
            ..CycleSummary::default()
        };
        if shutdown_requested(shutdown.as_deref(), drain, self.config) {
            return Ok(summary);
        }

        let maximum = self
            .config
            .batch_size
            .min(self.admission.max_claims_per_permit());
        if maximum == 0 {
            return Err(DeliveryError::InvalidConfig);
        }
        // Own all capacity before admission/claim. An awaiting semaphore
        // permit here would mean a claimed DB row whose handler has not run.
        let mut slots = Vec::new();
        for _ in 0..maximum {
            match self.slots.clone().try_acquire_owned() {
                Ok(slot) => slots.push(slot),
                Err(_) => break,
            }
        }
        if slots.is_empty() {
            return Ok(summary);
        }
        let Some(permit) =
            prepare_operation(self.admission.acquire(), &mut shutdown, drain, self.config).await?
        else {
            return Ok(summary);
        };
        if shutdown_requested(shutdown.as_deref(), drain, self.config) {
            cleanup_operation(permit.release(), drain, self.config).await?;
            return Ok(summary);
        }
        let claimed = match prepare_operation(
            self.store
                .claim(self.owner, slots.len() as u32, self.config.lease_duration),
            &mut shutdown,
            drain,
            self.config,
        )
        .await
        {
            Ok(claimed) => claimed,
            Err(err) => {
                // An abandoned claim can already have committed. Release only
                // the admission token we actually obtained, never infer rollback.
                let _ = cleanup_operation(permit.release(), drain, self.config).await;
                return Err(err);
            }
        };
        if claimed.len() > slots.len() {
            let _ = cleanup_operation(permit.release(), drain, self.config).await;
            return Err(DeliveryError::StoreUnknown);
        }
        summary.claimed = claimed.len() as u32;
        self.observation.record(
            Metric::Claimed,
            f64::from(summary.claimed),
            Outcome::Observed,
            None,
        );
        if shutdown_requested(shutdown.as_deref(), drain, self.config) {
            cleanup_operation(permit.release(), drain, self.config).await?;
            // Returned claims are known, but none has completed. Do not dispatch
            // them after shutdown or imply that cancellation refunded attempts.
            return if claimed.is_empty() {
                Ok(summary)
            } else {
                Err(DeliveryError::StoreUnknown)
            };
        }
        let mut tasks = JoinSet::new();
        let mut cancellations = Vec::new();
        let processing_deadline = Instant::now() + self.config.max_processing;
        for (event, slot) in claimed.into_iter().zip(slots) {
            let cancel = Arc::new(AtomicBool::new(false));
            let incomplete_io = Arc::new(AtomicBool::new(false));
            cancellations.push((
                CancellationGuard::new(cancel.clone()),
                incomplete_io.clone(),
            ));
            let work = process_claim(
                self.store.clone(),
                self.handler.clone(),
                permit.clone(),
                event,
                ClaimExecution {
                    config: self.config,
                    cancel,
                    deadline: processing_deadline,
                    incomplete_io: incomplete_io.clone(),
                    observation: self.observation.clone(),
                },
                slot,
            );
            tasks.spawn(async move { (incomplete_io, work.await) });
        }
        let mut first_error = None;
        while !tasks.is_empty() {
            let joined = if let Some(deadline) = drain.get().copied() {
                match tokio::time::timeout_at(deadline, tasks.join_next()).await {
                    Ok(joined) => joined,
                    Err(_) => {
                        // The handler may ignore cooperative cancellation. Its
                        // outbox token remains unacknowledged for expiry/reap.
                        for (cancel, incomplete_io) in &cancellations {
                            cancel.signal.store(true, Ordering::SeqCst);
                            if incomplete_io.load(Ordering::SeqCst) {
                                // A cancelled renewal or settlement can already
                                // have committed. Do not erase that uncertainty
                                // by classifying every aborted task as Lost.
                                first_error.get_or_insert(DeliveryError::StoreUnknown);
                            }
                        }
                        summary.lost += tasks.len() as u32;
                        tasks.abort_all();
                        break;
                    }
                }
            } else if let Some(receiver) = shutdown.as_deref_mut() {
                tokio::select! {
                    joined = tasks.join_next() => joined,
                    changed = receiver.changed() => {
                        if changed.is_err() || *receiver.borrow() {
                            start_drain(drain, self.config);
                        }
                        continue;
                    }
                }
            } else {
                tasks.join_next().await
            };
            if let Some(joined) = joined {
                fold_join(&mut summary, &mut first_error, joined);
            }
        }
        drop(tasks); // Abort stragglers before releasing the admission fence.
        for (cancel, _) in &mut cancellations {
            cancel.disarm();
        }
        if let Err(err) = cleanup_operation(permit.release(), drain, self.config).await {
            first_error.get_or_insert(err);
        }
        match first_error {
            Some(err) => Err(err),
            None => Ok(summary),
        }
    }

    pub async fn run_until_shutdown(
        &self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<RunSummary, DeliveryError> {
        let drain = OnceLock::new();
        let worker = self.run_until_shutdown_inner(shutdown.clone(), &drain);
        tokio::pin!(worker);
        // This observer remains live while any nested operation is pending,
        // including cleanup. All stages share its one absolute drain budget.
        tokio::select! {
            biased;
            _ = wait_for_shutdown(&mut shutdown) => {
                let deadline = start_drain(&drain, self.config);
                tokio::time::timeout_at(deadline, &mut worker)
                    .await
                    .map_err(|_| DeliveryError::StoreUnknown)?
            }
            result = &mut worker => result,
        }
    }

    async fn run_until_shutdown_inner(
        &self,
        mut shutdown: watch::Receiver<bool>,
        drain: &OnceLock<Instant>,
    ) -> Result<RunSummary, DeliveryError> {
        let mut summary = RunSummary::default();
        let mut backoff = self.config.poll_interval.max(Duration::from_millis(100));
        let mut shutdown_error = None;
        while !shutdown_requested(Some(&shutdown), drain, self.config) {
            // A cycle that claimed work polls again at once: with one claim
            // per cycle a backlog would otherwise wait one interval per row.
            let mut busy = false;
            match self.run_cycle_inner(Some(&mut shutdown), drain).await {
                Ok(cycle) => {
                    summary.cycles += 1;
                    summary.claimed += u64::from(cycle.claimed);
                    summary.settled += u64::from(cycle.settled);
                    summary.lost += u64::from(cycle.lost);
                    summary.reaped += cycle.reaped;
                    backoff = self.config.poll_interval.max(Duration::from_millis(50));
                    busy = cycle.claimed > 0;
                }
                Err(DeliveryError::StoreUnknown) => {
                    summary.outages += 1;
                    backoff = backoff.saturating_mul(2).min(Duration::from_secs(5));
                    if shutdown_requested(Some(&shutdown), drain, self.config) {
                        shutdown_error = Some(DeliveryError::StoreUnknown);
                    }
                }
                Err(err) => return Err(err),
            }
            if shutdown_requested(Some(&shutdown), drain, self.config) {
                break;
            }
            if busy {
                continue;
            }
            tokio::select! {
                biased;
                _ = wait_for_shutdown(&mut shutdown) => {
                    start_drain(drain, self.config);
                    break;
                }
                _ = tokio::time::sleep(backoff) => {},
            }
        }
        // Reaper is one last independent, policy-fenced DB operation. Never
        // claim it succeeded if the DB is still unavailable at shutdown.
        let reaped = cleanup_operation(
            self.store.reap_exhausted(self.config.reap_batch),
            drain,
            self.config,
        )
        .await
        .inspect_err(|error| self.observation.error(error))?;
        summary.reaped += reaped;
        self.observation.record(
            Metric::Reaped,
            reaped as f64,
            Outcome::Exhausted,
            Some(crate::ErrorCode::DeliveryUnknownAtLimit),
        );
        match shutdown_error {
            Some(err) => Err(err),
            None => Ok(summary),
        }
    }
}

fn start_drain(drain: &OnceLock<Instant>, config: DeliveryConfig) -> Instant {
    *drain.get_or_init(|| Instant::now() + config.drain_timeout)
}

fn shutdown_requested(
    shutdown: Option<&watch::Receiver<bool>>,
    drain: &OnceLock<Instant>,
    config: DeliveryConfig,
) -> bool {
    let requested = drain.get().is_some()
        || shutdown.is_some_and(|receiver| *receiver.borrow() || receiver.has_changed().is_err());
    if requested {
        start_drain(drain, config);
    }
    requested
}

async fn wait_for_shutdown(receiver: &mut watch::Receiver<bool>) {
    loop {
        let requested = *receiver.borrow_and_update();
        if requested || receiver.changed().await.is_err() {
            return;
        }
    }
}

// A renewal interval is strictly below one third of the lease. Bounding a
// dependency round by one third leaves room for the other fence and avoids
// relying on a lease throughout an unresolved 15-minute processing budget.
fn dependency_deadline(config: DeliveryConfig) -> Instant {
    Instant::now() + config.lease_duration / 3
}

async fn bounded_operation<T>(
    future: impl Future<Output = Result<T, DeliveryError>>,
    deadline: Instant,
) -> Result<T, DeliveryError> {
    tokio::time::timeout_at(deadline, future)
        .await
        .map_err(|_| DeliveryError::StoreUnknown)?
}

async fn prepare_operation<T>(
    future: impl Future<Output = Result<T, DeliveryError>>,
    shutdown: &mut Option<&mut watch::Receiver<bool>>,
    drain: &OnceLock<Instant>,
    config: DeliveryConfig,
) -> Result<T, DeliveryError> {
    if shutdown_requested(shutdown.as_deref(), drain, config) {
        return Err(DeliveryError::StoreUnknown);
    }
    let bounded = bounded_operation(future, dependency_deadline(config));
    if let Some(receiver) = shutdown.as_deref_mut() {
        tokio::select! {
            // Capture an actually returned permit/claim if both become ready.
            biased;
            result = bounded => result,
            _ = wait_for_shutdown(receiver) => {
                start_drain(drain, config);
                Err(DeliveryError::StoreUnknown)
            }
        }
    } else {
        bounded.await
    }
}

async fn cleanup_operation<T>(
    future: impl Future<Output = Result<T, DeliveryError>>,
    drain: &OnceLock<Instant>,
    config: DeliveryConfig,
) -> Result<T, DeliveryError> {
    let deadline = drain
        .get()
        .copied()
        .unwrap_or_else(|| Instant::now() + config.drain_timeout)
        .min(dependency_deadline(config));
    // An immediately completed conditional cleanup can still be observed at
    // the bound. A pending cleanup gets no fresh drain window and is Unknown.
    bounded_operation(future, deadline).await
}

struct CancellationGuard {
    signal: Arc<AtomicBool>,
    armed: bool,
}

impl CancellationGuard {
    fn new(signal: Arc<AtomicBool>) -> Self {
        Self {
            signal,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if self.armed {
            self.signal.store(true, Ordering::SeqCst);
        }
    }
}

fn fold_join(
    summary: &mut CycleSummary,
    first_error: &mut Option<DeliveryError>,
    joined: Result<ClaimResult, tokio::task::JoinError>,
) {
    let outcome = match joined {
        Ok((incomplete_io, outcome)) => {
            // Clear only once the parent has observed the task's result; a
            // completed but unobserved settlement is still conservatively
            // unknown if the parent abandons it at the shutdown deadline.
            incomplete_io.store(false, Ordering::SeqCst);
            outcome
        }
        Err(_) => Err(DeliveryError::StoreUnknown),
    };
    match outcome {
        Ok(ClaimOutcome::Settled) => summary.settled += 1,
        Ok(ClaimOutcome::Lost) => summary.lost += 1,
        Err(err) => {
            first_error.get_or_insert(err);
        }
    }
}

type ClaimResult = (Arc<AtomicBool>, Result<ClaimOutcome, DeliveryError>);

struct ClaimExecution {
    config: DeliveryConfig,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    incomplete_io: Arc<AtomicBool>,
    observation: Observation,
}

enum ClaimOutcome {
    Settled,
    Lost,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FenceCheck {
    Current,
    Lost,
    Cancelled,
}

async fn check_preflight<S: OutboxStore, P: ClaimPermit>(
    store: &S,
    event_id: Uuid,
    outbox_token: Uuid,
    permit: &P,
    lease: std::time::Duration,
    cancel: &AtomicBool,
    observation: &Observation,
) -> Result<FenceCheck, DeliveryError> {
    if cancel.load(Ordering::SeqCst) {
        return Ok(FenceCheck::Cancelled);
    }
    let outbox = store.renew(event_id, outbox_token, lease).await?;
    if outbox == FenceResult::Lost {
        observation.stale();
        return Ok(FenceCheck::Lost);
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(FenceCheck::Cancelled);
    }
    // 読取だけの preflight に依存せず、Source の期限も明示的に延長する。
    let source = permit.renew().await?;
    if source == FenceResult::Lost {
        observation.stale();
        return Ok(FenceCheck::Lost);
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(FenceCheck::Cancelled);
    }
    let current = permit.preflight().await?;
    if current == FenceResult::Lost {
        observation.stale();
        return Ok(FenceCheck::Lost);
    }
    Ok(if cancel.load(Ordering::SeqCst) {
        FenceCheck::Cancelled
    } else {
        FenceCheck::Current
    })
}

async fn process_claim<S, H, P>(
    store: Arc<S>,
    handler: Arc<H>,
    permit: P,
    event: crate::ClaimedEvent,
    execution: ClaimExecution,
    _slot: OwnedSemaphorePermit,
) -> Result<ClaimOutcome, DeliveryError>
where
    S: OutboxStore,
    P: ClaimPermit,
    H: DeliveryHandler<P>,
{
    let mut cancellation = CancellationGuard::new(execution.cancel.clone());
    let work = process_claim_inner(store, handler, permit, event, &execution);
    let result = tokio::time::timeout_at(execution.deadline, work)
        .await
        .map_err(|_| DeliveryError::StoreUnknown)?;
    if matches!(result, Ok(ClaimOutcome::Settled)) {
        cancellation.disarm();
    }
    result
}

async fn process_claim_inner<S, H, P>(
    store: Arc<S>,
    handler: Arc<H>,
    permit: P,
    event: crate::ClaimedEvent,
    execution: &ClaimExecution,
) -> Result<ClaimOutcome, DeliveryError>
where
    S: OutboxStore,
    P: ClaimPermit,
    H: DeliveryHandler<P>,
{
    let config = execution.config;
    let cancel = &execution.cancel;
    let processing_deadline = execution.deadline;
    if Instant::now() >= processing_deadline {
        return Err(DeliveryError::StoreUnknown);
    }
    let id = event.envelope.event_id;
    let token = event.lease_token;
    let observation = &execution.observation;
    if cancel.load(Ordering::SeqCst) {
        return Ok(ClaimOutcome::Lost);
    }
    let parent = if observation.observer.is_some() {
        match bounded_operation(
            store.trace_context(id, token),
            dependency_deadline(config).min(processing_deadline),
        )
        .await
        {
            Ok(parent) => parent,
            Err(error) => {
                observation.error(&error);
                None
            }
        }
    } else {
        None
    };
    execution.incomplete_io.store(true, Ordering::SeqCst);
    // Publish the I/O phase before reading cancellation. With the parent's
    // cancel-then-read ordering, it either observes uncertain I/O or this
    // task observes cancellation and starts no dependency operation.
    if cancel.load(Ordering::SeqCst) {
        return Ok(ClaimOutcome::Lost);
    }
    // This first conditional renewal verifies the committed claim before any
    // handler code runs. DB uncertainty never becomes permission to dispatch.
    if bounded_operation(
        check_preflight(
            store.as_ref(),
            id,
            token,
            &permit,
            config.lease_duration,
            cancel,
            observation,
        ),
        dependency_deadline(config).min(processing_deadline),
    )
    .await?
        != FenceCheck::Current
    {
        return Ok(ClaimOutcome::Lost);
    }
    if Instant::now() >= processing_deadline {
        return Err(DeliveryError::StoreUnknown);
    }
    let mut span = observation.observer.as_ref().map(|_| {
        SpanTiming::new(
            observation.clone(),
            DeliverySpan::new(id, event.attempt, parent),
        )
    });
    let context = DeliveryContext {
        attempt: event.attempt,
        outbox_token: token,
        outbox_deadline: event.lease_expires_at,
        cancel: cancel.clone(),
        trace: span.as_ref().map(|span| span.span.context().clone()),
    };
    execution.incomplete_io.store(false, Ordering::SeqCst);
    if cancel.load(Ordering::SeqCst) {
        return Ok(ClaimOutcome::Lost);
    }
    let handler_timing = HandlerTiming::new(observation.clone());
    let handler_future = handler.deliver(event.envelope, context, permit.clone());
    tokio::pin!(handler_future);
    let mut heartbeat = tokio::time::interval_at(
        Instant::now() + config.renew_interval,
        config.renew_interval,
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let deadline = tokio::time::sleep_until(processing_deadline);
    tokio::pin!(deadline);
    let decision = loop {
        tokio::select! {
            biased;
            _ = &mut deadline => {
                cancel.store(true, Ordering::SeqCst);
                return Ok(ClaimOutcome::Lost);
            }
            decision = &mut handler_future => break decision,
            _ = heartbeat.tick() => {
                execution.incomplete_io.store(true, Ordering::SeqCst);
                if cancel.load(Ordering::SeqCst) {
                    return Ok(ClaimOutcome::Lost);
                }
                let renewal = async {
                    let outbox = store.renew(id, token, config.lease_duration).await?;
                    if outbox == FenceResult::Lost { observation.stale(); return Ok(FenceCheck::Lost); }
                    if cancel.load(Ordering::SeqCst) { return Ok(FenceCheck::Cancelled); }
                    let source = permit.renew().await?;
                    if source == FenceResult::Lost { observation.stale(); return Ok(FenceCheck::Lost); }
                    Ok(if cancel.load(Ordering::SeqCst) { FenceCheck::Cancelled } else { FenceCheck::Current })
                };
                let source = bounded_operation(
                    renewal,
                    dependency_deadline(config).min(processing_deadline),
                ).await;
                if source != Ok(FenceCheck::Current) {
                    cancel.store(true, Ordering::SeqCst);
                    return match source {
                        Ok(check) => {
                            if check == FenceCheck::Lost && let Some(span) = &mut span {
                                span.outcome = Outcome::Lost;
                            }
                            Ok(ClaimOutcome::Lost)
                        },
                        Err(err) => Err(err),
                    };
                }
                execution.incomplete_io.store(false, Ordering::SeqCst);
                if cancel.load(Ordering::SeqCst) {
                    return Ok(ClaimOutcome::Lost);
                }
            }
        }
    };
    handler_timing.complete(decision);
    if Instant::now() >= processing_deadline {
        return Err(DeliveryError::StoreUnknown);
    }
    execution.incomplete_io.store(true, Ordering::SeqCst);
    if cancel.load(Ordering::SeqCst) {
        return Ok(ClaimOutcome::Lost);
    }
    let final_check = bounded_operation(
        check_preflight(
            store.as_ref(),
            id,
            token,
            &permit,
            config.lease_duration,
            cancel,
            observation,
        ),
        dependency_deadline(config).min(processing_deadline),
    )
    .await?;
    if final_check != FenceCheck::Current {
        if final_check == FenceCheck::Lost
            && let Some(span) = &mut span
        {
            span.outcome = Outcome::Lost;
        }
        return Ok(ClaimOutcome::Lost);
    }
    if Instant::now() >= processing_deadline {
        return Err(DeliveryError::StoreUnknown);
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(ClaimOutcome::Lost);
    }
    let settlement = async {
        match decision {
            DeliveryDecision::Applied | DeliveryDecision::KnownNoop => {
                store.settle_success(id, token).await
            }
            DeliveryDecision::Retryable(code) => {
                let backoff = retry_delay_within(id, event.attempt, store.backoff_bounds());
                store.settle_failure(id, token, code, false, backoff).await
            }
            DeliveryDecision::Terminal(code) => {
                let backoff = retry_delay_within(id, event.attempt, store.backoff_bounds());
                store.settle_failure(id, token, code, true, backoff).await
            }
        }
    };
    let settled = bounded_operation(
        settlement,
        dependency_deadline(config).min(processing_deadline),
    )
    .await?;
    Ok(match settled {
        FenceResult::Updated => {
            let outcome = match decision {
                DeliveryDecision::Applied => Outcome::Applied,
                DeliveryDecision::KnownNoop => Outcome::KnownNoop,
                DeliveryDecision::Retryable(_) if event.attempt < event.attempt_limit => {
                    Outcome::Retryable
                }
                DeliveryDecision::Retryable(_) => Outcome::Exhausted,
                DeliveryDecision::Terminal(_) => Outcome::Terminal,
            };
            match decision {
                DeliveryDecision::Applied | DeliveryDecision::KnownNoop => {
                    observation.record(Metric::Acknowledged, 1.0, outcome, None)
                }
                DeliveryDecision::Retryable(code) if event.attempt < event.attempt_limit => {
                    observation.record(Metric::Retried, 1.0, outcome, Some(code))
                }
                _ => {}
            }
            if let Some(span) = &mut span {
                span.outcome = outcome;
            }
            ClaimOutcome::Settled
        }
        FenceResult::Lost => {
            observation.stale();
            if let Some(span) = &mut span {
                span.outcome = Outcome::Lost;
            }
            ClaimOutcome::Lost
        }
    })
}
