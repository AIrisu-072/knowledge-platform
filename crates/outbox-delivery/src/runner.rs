//! Bounded, generic dispatch of fenced outbox claims.

use std::sync::{
    Arc,
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
    HandlerFuture, OutboxStore, retry_delay,
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
        })
    }

    pub async fn run_cycle(&self) -> Result<CycleSummary, DeliveryError> {
        self.run_cycle_inner(None).await
    }

    async fn run_cycle_inner(
        &self,
        mut shutdown: Option<&mut watch::Receiver<bool>>,
    ) -> Result<CycleSummary, DeliveryError> {
        self.store.verify_policy().await?;
        let reaped = self.store.reap_exhausted(self.config.reap_batch).await?;
        let mut summary = CycleSummary {
            reaped,
            ..CycleSummary::default()
        };
        if shutdown.as_ref().is_some_and(|receiver| *receiver.borrow()) {
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
        let Some(permit) = self.admission.acquire().await? else {
            return Ok(summary);
        };
        if shutdown.as_ref().is_some_and(|receiver| *receiver.borrow()) {
            let _ = permit.release().await;
            return Ok(summary);
        }
        let claimed = match self
            .store
            .claim(self.owner, slots.len() as u32, self.config.lease_duration)
            .await
        {
            Ok(claimed) => claimed,
            Err(err) => {
                let _ = permit.release().await;
                return Err(err);
            }
        };
        if claimed.len() > slots.len() {
            let _ = permit.release().await;
            return Err(DeliveryError::StoreUnknown);
        }
        summary.claimed = claimed.len() as u32;
        let mut tasks = JoinSet::new();
        let mut cancellations = Vec::new();
        for (event, slot) in claimed.into_iter().zip(slots) {
            let cancel = Arc::new(AtomicBool::new(false));
            cancellations.push(cancel.clone());
            tasks.spawn(process_claim(
                self.store.clone(),
                self.handler.clone(),
                permit.clone(),
                event,
                self.config,
                cancel,
                slot,
            ));
        }
        let mut first_error = None;
        let mut drain_deadline = None;
        while !tasks.is_empty() {
            let joined = if let Some(deadline) = drain_deadline {
                match tokio::time::timeout_at(deadline, tasks.join_next()).await {
                    Ok(joined) => joined,
                    Err(_) => {
                        // The handler may ignore cooperative cancellation. Its
                        // outbox token remains unacknowledged for expiry/reap.
                        for cancel in &cancellations {
                            cancel.store(true, Ordering::SeqCst);
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
                            drain_deadline = Some(Instant::now() + self.config.drain_timeout);
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
        if let Err(err) = permit.release().await {
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
        let mut summary = RunSummary::default();
        let mut backoff = self.config.poll_interval.max(Duration::from_millis(100));
        while !*shutdown.borrow() {
            match self.run_cycle_inner(Some(&mut shutdown)).await {
                Ok(cycle) => {
                    summary.cycles += 1;
                    summary.claimed += u64::from(cycle.claimed);
                    summary.settled += u64::from(cycle.settled);
                    summary.lost += u64::from(cycle.lost);
                    summary.reaped += cycle.reaped;
                    backoff = self.config.poll_interval.max(Duration::from_millis(50));
                }
                Err(DeliveryError::StoreUnknown) => {
                    summary.outages += 1;
                    backoff = backoff.saturating_mul(2).min(Duration::from_secs(5));
                }
                Err(err) => return Err(err),
            }
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {},
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
        // Reaper is one last independent, policy-fenced DB operation. Never
        // claim it succeeded if the DB is still unavailable at shutdown.
        summary.reaped += self.store.reap_exhausted(self.config.reap_batch).await?;
        Ok(summary)
    }
}

fn fold_join(
    summary: &mut CycleSummary,
    first_error: &mut Option<DeliveryError>,
    joined: Result<Result<ClaimOutcome, DeliveryError>, tokio::task::JoinError>,
) {
    match joined {
        Ok(Ok(ClaimOutcome::Settled)) => summary.settled += 1,
        Ok(Ok(ClaimOutcome::Lost)) => summary.lost += 1,
        Ok(Err(err)) => {
            first_error.get_or_insert(err);
        }
        Err(_) => {
            first_error.get_or_insert(DeliveryError::StoreUnknown);
        }
    }
}

enum ClaimOutcome {
    Settled,
    Lost,
}

async fn check_preflight<S: OutboxStore, P: ClaimPermit>(
    store: &S,
    event_id: Uuid,
    outbox_token: Uuid,
    permit: &P,
    lease: std::time::Duration,
) -> Result<bool, DeliveryError> {
    if store.renew(event_id, outbox_token, lease).await? != FenceResult::Updated {
        return Ok(false);
    }
    // A read-only preflight can succeed just before Source expiry. Renewal is
    // explicit; adapters must not need to smuggle it into `preflight`.
    if permit.renew().await? != FenceResult::Updated {
        return Ok(false);
    }
    Ok(permit.preflight().await? == FenceResult::Updated)
}

async fn process_claim<S, H, P>(
    store: Arc<S>,
    handler: Arc<H>,
    permit: P,
    event: crate::ClaimedEvent,
    config: DeliveryConfig,
    cancel: Arc<AtomicBool>,
    _slot: OwnedSemaphorePermit,
) -> Result<ClaimOutcome, DeliveryError>
where
    S: OutboxStore,
    P: ClaimPermit,
    H: DeliveryHandler<P>,
{
    let id = event.envelope.event_id;
    let token = event.lease_token;
    // This first conditional renewal verifies the committed claim before any
    // handler code runs. DB uncertainty never becomes permission to dispatch.
    if !check_preflight(store.as_ref(), id, token, &permit, config.lease_duration).await? {
        return Ok(ClaimOutcome::Lost);
    }
    let context = DeliveryContext {
        attempt: event.attempt,
        outbox_token: token,
        outbox_deadline: event.lease_expires_at,
        cancel: cancel.clone(),
    };
    let handler_future = handler.deliver(event.envelope, context, permit.clone());
    tokio::pin!(handler_future);
    let mut heartbeat = tokio::time::interval_at(
        Instant::now() + config.renew_interval,
        config.renew_interval,
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let deadline = tokio::time::sleep(config.max_processing);
    tokio::pin!(deadline);
    let decision = loop {
        tokio::select! {
            decision = &mut handler_future => break decision,
            _ = heartbeat.tick() => {
                let outbox = store.renew(id, token, config.lease_duration).await;
                let source = match outbox {
                    Ok(FenceResult::Updated) => permit.renew().await,
                    Ok(FenceResult::Lost) => Ok(FenceResult::Lost),
                    Err(err) => Err(err),
                };
                if source != Ok(FenceResult::Updated) {
                    cancel.store(true, Ordering::SeqCst);
                    return match source {
                        Ok(FenceResult::Lost) => Ok(ClaimOutcome::Lost),
                        Err(err) => Err(err),
                        _ => unreachable!(),
                    };
                }
            }
            _ = &mut deadline => {
                cancel.store(true, Ordering::SeqCst);
                return Ok(ClaimOutcome::Lost);
            }
        }
    };
    if cancel.load(Ordering::SeqCst)
        || !check_preflight(store.as_ref(), id, token, &permit, config.lease_duration).await?
    {
        return Ok(ClaimOutcome::Lost);
    }
    let settled = match decision {
        DeliveryDecision::Applied | DeliveryDecision::KnownNoop => {
            store.settle_success(id, token).await?
        }
        DeliveryDecision::Retryable(code) => {
            store
                .settle_failure(id, token, code, false, retry_delay(id, event.attempt))
                .await?
        }
        DeliveryDecision::Terminal(code) => {
            store
                .settle_failure(id, token, code, true, retry_delay(id, event.attempt))
                .await?
        }
    };
    Ok(match settled {
        FenceResult::Updated => ClaimOutcome::Settled,
        FenceResult::Lost => ClaimOutcome::Lost,
    })
}
