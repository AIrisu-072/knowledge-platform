//! Store admission: a circuit breaker with probe and regression gate
//! (design §6.2 step 1, §6.3, §11).
//!
//! - closed: claims up to the runner batch.
//! - open: no claims until an exponential cooldown elapses.
//! - half-open: one claim per permit (`max_claims_per_permit() == 1` unless
//!   closed). A permit released without an ingest outcome returns to
//!   half-open.
//!
//! The breaker opens on any Store outage. It closes on any structured ingest
//! verdict row (stored, duplicate*, conflict, rejected) — proof the Store
//! works — never on a probe alone. Before admitting, the probe
//! (`audit_store.probe()` via the ingest login) and the regression gate run;
//! failures are `Ok(None)` (no claim), never another `DeliveryError`.
//!
//! Regression gate: the highest acknowledged receipt of the current Store
//! recovery epoch must still exist in the Store with the same seq and
//! envelope digest, and the Store head must not be behind it. A mismatch is
//! `store_regressed`: the relay reports it to the Store and stays closed
//! (sticky) until the Store recovery epoch changes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use audit_core::{AuditStore, OutageCode, StoreError};
use outbox_delivery::runner::{ClaimAdmission, ClaimPermit};
use outbox_delivery::{DeliveryFuture, FenceResult};
use sqlx::{PgPool, Row};
use tokio::time::Instant;
use uuid::Uuid;

use crate::store_admin::StoreAdmin;

/// Breaker timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerConfig {
    pub initial_cooldown: Duration,
    pub max_cooldown: Duration,
    /// Bound on the probe and on each gate query.
    pub probe_timeout: Duration,
    /// Claims per permit while closed (the runner also caps by batch size).
    pub closed_claims: u32,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            initial_cooldown: Duration::from_secs(1),
            max_cooldown: Duration::from_secs(60),
            probe_timeout: Duration::from_secs(5),
            closed_claims: 32,
        }
    }
}

/// Externally visible breaker mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Closed,
    Open,
    HalfOpen,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Open => "open",
            Self::HalfOpen => "half_open",
        }
    }
}

/// The last admission gate result (fixed codes only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Unknown,
    Ok,
    /// The probe failed with this outage class.
    Outage(OutageCode),
    /// The source database could not answer the gate query.
    SourceUnavailable,
    /// Acknowledged receipts are missing or differ (sticky per epoch).
    Regressed,
}

impl Gate {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Ok => "ok",
            Self::Outage(code) => code.as_str(),
            Self::SourceUnavailable => "source_unavailable",
            Self::Regressed => OutageCode::Regressed.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum State {
    Closed,
    Open { until: Instant },
    HalfOpen { admitted: Option<u64> },
}

struct Inner {
    state: State,
    cooldown: Duration,
    regressed_epoch: Option<i64>,
    gate: Gate,
    outages: u64,
}

/// Shared breaker state (admission and handler).
pub struct Breaker {
    config: BreakerConfig,
    inner: Mutex<Inner>,
    permits: AtomicU64,
}

impl Breaker {
    pub fn new(config: BreakerConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(Inner {
                // Start half-open: the first claim must prove the Store works.
                state: State::HalfOpen { admitted: None },
                cooldown: config.initial_cooldown,
                regressed_epoch: None,
                gate: Gate::Unknown,
                outages: 0,
            }),
            permits: AtomicU64::new(1),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn config(&self) -> BreakerConfig {
        self.config
    }

    pub fn mode(&self) -> Mode {
        match self.lock().state {
            State::Closed => Mode::Closed,
            State::Open { .. } => Mode::Open,
            State::HalfOpen { .. } => Mode::HalfOpen,
        }
    }

    pub fn gate(&self) -> Gate {
        self.lock().gate
    }

    pub fn outages(&self) -> u64 {
        self.lock().outages
    }

    /// Store outage observed (ingest, probe, control call): open, and double
    /// the cooldown when it was already open or half-open.
    pub fn record_outage(&self) {
        let mut inner = self.lock();
        inner.outages += 1;
        let cooldown = inner.cooldown;
        inner.state = State::Open {
            until: Instant::now() + cooldown,
        };
        inner.cooldown = (cooldown * 2).min(self.config.max_cooldown);
    }

    /// A structured ingest verdict row: the Store works. Close.
    pub fn record_verdict(&self) {
        let mut inner = self.lock();
        inner.state = State::Closed;
        inner.cooldown = self.config.initial_cooldown;
    }

    fn release(&self, permit_id: u64) {
        let mut inner = self.lock();
        if let State::HalfOpen { admitted: Some(id) } = inner.state
            && id == permit_id
        {
            inner.state = State::HalfOpen { admitted: None };
        }
    }

    fn set_gate(&self, gate: Gate) {
        self.lock().gate = gate;
    }

    /// Admission decision after a successful probe and gate.
    fn admit(&self, epoch: i64) -> Option<BreakerPermit> {
        let mut inner = self.lock();
        let now = Instant::now();
        let half_open = match inner.state {
            State::Closed => false,
            State::Open { until } if now < until => return None,
            State::Open { .. } | State::HalfOpen { admitted: None } => true,
            State::HalfOpen { admitted: Some(_) } => return None,
        };
        let id = self.permits.fetch_add(1, Ordering::Relaxed);
        if half_open {
            inner.state = State::HalfOpen { admitted: Some(id) };
        }
        Some(BreakerPermit {
            breaker: None,
            id,
            epoch,
            half_open,
        })
    }

    fn cooling_down(&self) -> bool {
        matches!(self.lock().state, State::Open { until } if Instant::now() < until)
    }
}

/// A claim admission. `preflight`/`renew`/`release` always report `Updated`:
/// the breaker never revokes a claim already in progress.
#[derive(Clone)]
pub struct BreakerPermit {
    breaker: Option<Arc<Breaker>>,
    id: u64,
    /// The Store recovery epoch observed by the admission probe.
    pub epoch: i64,
    pub half_open: bool,
}

impl BreakerPermit {
    /// A permit outside any breaker (tests and one-shot tools).
    pub fn detached(epoch: i64) -> Self {
        Self {
            breaker: None,
            id: 0,
            epoch,
            half_open: false,
        }
    }
}

impl ClaimPermit for BreakerPermit {
    fn preflight(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async { Ok(FenceResult::Updated) })
    }

    fn renew(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async { Ok(FenceResult::Updated) })
    }

    fn release(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            if let Some(breaker) = &self.breaker {
                breaker.release(self.id);
            }
            Ok(FenceResult::Updated)
        })
    }
}

/// The highest acknowledged receipt in one Store epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckedHead {
    pub event_id: Uuid,
    pub store_seq: i64,
    pub envelope_digest: [u8; 32],
}

/// Reads `audit_relay.acked_head(epoch)`.
pub async fn acked_head(source: &PgPool, epoch: i64) -> Result<Option<AckedHead>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM audit_relay.acked_head($1)")
        .bind(epoch)
        .fetch_optional(source)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let digest: Vec<u8> = row.try_get("store_envelope_digest")?;
    let envelope_digest: [u8; 32] = digest
        .try_into()
        .map_err(|_| sqlx::Error::Protocol("acked digest length".into()))?;
    Ok(Some(AckedHead {
        event_id: row.try_get("event_id")?,
        store_seq: row.try_get("store_seq")?,
        envelope_digest,
    }))
}

/// Outcome of the identity check of one acknowledged receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regression {
    Intact,
    Regressed,
    /// The Store could not answer (an outage).
    Unavailable(OutageCode),
}

/// Compares the acknowledged head with the Store (design §11).
pub async fn check_regression(
    admin: &dyn StoreAdmin,
    head_seq: i64,
    acked: &AckedHead,
) -> Regression {
    if head_seq < acked.store_seq {
        return Regression::Regressed;
    }
    match admin
        .lookup_receipts(std::slice::from_ref(&acked.event_id))
        .await
    {
        Ok(receipts) => {
            let intact = receipts.iter().any(|r| {
                r.event_id == acked.event_id
                    && r.seq == acked.store_seq
                    && r.envelope_digest == acked.envelope_digest
            });
            if intact {
                Regression::Intact
            } else {
                Regression::Regressed
            }
        }
        Err(error) => Regression::Unavailable(
            OutageCode::parse(crate::store_admin::admin_outage_code(&error))
                .unwrap_or(OutageCode::Unclassified),
        ),
    }
}

/// [`ClaimAdmission`] over the breaker, the Store probe and the regression
/// gate.
pub struct BreakerAdmission {
    breaker: Arc<Breaker>,
    store: Arc<dyn AuditStore>,
    admin: Arc<dyn StoreAdmin>,
    source: PgPool,
}

impl BreakerAdmission {
    pub fn new(
        breaker: Arc<Breaker>,
        store: Arc<dyn AuditStore>,
        admin: Arc<dyn StoreAdmin>,
        source: PgPool,
    ) -> Self {
        Self {
            breaker,
            store,
            admin,
            source,
        }
    }

    async fn gate(&self) -> Option<i64> {
        let timeout = self.breaker.config.probe_timeout;
        let status = match tokio::time::timeout(timeout, self.store.probe()).await {
            Ok(Ok(status)) if status.writable => status,
            Ok(Ok(_)) => {
                self.breaker.set_gate(Gate::Outage(OutageCode::ReadOnly));
                self.breaker.record_outage();
                return None;
            }
            Ok(Err(error)) => {
                let code = probe_code(&error);
                self.breaker.set_gate(Gate::Outage(code));
                self.breaker.record_outage();
                return None;
            }
            Err(_) => {
                self.breaker.set_gate(Gate::Outage(OutageCode::Timeout));
                self.breaker.record_outage();
                return None;
            }
        };
        let epoch = status.recovery_epoch;
        {
            let mut inner = self.breaker.lock();
            match inner.regressed_epoch {
                Some(regressed) if regressed == epoch => {
                    inner.gate = Gate::Regressed;
                    return None;
                }
                Some(_) => inner.regressed_epoch = None,
                None => {}
            }
        }
        let acked = match tokio::time::timeout(timeout, acked_head(&self.source, epoch)).await {
            Ok(Ok(acked)) => acked,
            _ => {
                self.breaker.set_gate(Gate::SourceUnavailable);
                return None;
            }
        };
        if let Some(acked) = acked {
            let verdict = tokio::time::timeout(
                timeout,
                check_regression(self.admin.as_ref(), status.head_seq, &acked),
            )
            .await
            .unwrap_or(Regression::Unavailable(OutageCode::Timeout));
            match verdict {
                Regression::Intact => {}
                Regression::Unavailable(code) => {
                    self.breaker.set_gate(Gate::Outage(code));
                    self.breaker.record_outage();
                    return None;
                }
                Regression::Regressed => {
                    {
                        let mut inner = self.breaker.lock();
                        inner.regressed_epoch = Some(epoch);
                        inner.gate = Gate::Regressed;
                    }
                    self.breaker.record_outage();
                    // Best effort: the gate stays closed whatever the Store says.
                    let _ = tokio::time::timeout(
                        timeout,
                        self.admin.report_regression(
                            acked.store_seq,
                            acked.event_id,
                            &acked.envelope_digest,
                        ),
                    )
                    .await;
                    eprintln!(
                        "audit-relay: store_regressed (epoch {epoch}); claims stop until the \
                         Store recovery epoch changes"
                    );
                    return None;
                }
            }
        }
        self.breaker.set_gate(Gate::Ok);
        Some(epoch)
    }
}

fn probe_code(error: &StoreError) -> OutageCode {
    error.outage_code().unwrap_or(OutageCode::Unclassified)
}

impl ClaimAdmission for BreakerAdmission {
    type Permit = BreakerPermit;

    fn acquire(&self) -> DeliveryFuture<'_, Option<Self::Permit>> {
        Box::pin(async move {
            if self.breaker.cooling_down() {
                return Ok(None);
            }
            let Some(epoch) = self.gate().await else {
                return Ok(None);
            };
            Ok(self.breaker.admit(epoch).map(|mut permit| {
                permit.breaker = Some(self.breaker.clone());
                permit
            }))
        })
    }

    fn max_claims_per_permit(&self) -> u32 {
        match self.breaker.mode() {
            Mode::Closed => self.breaker.config.closed_claims.max(1),
            Mode::Open | Mode::HalfOpen => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BreakerConfig {
        BreakerConfig {
            initial_cooldown: Duration::from_millis(20),
            max_cooldown: Duration::from_millis(80),
            probe_timeout: Duration::from_secs(1),
            closed_claims: 8,
        }
    }

    #[tokio::test]
    async fn half_open_admits_one_and_a_bare_release_returns_to_half_open() {
        let breaker = Arc::new(Breaker::new(config()));
        assert_eq!(breaker.mode(), Mode::HalfOpen);
        let mut permit = breaker.admit(1).expect("first half-open permit");
        permit.breaker = Some(breaker.clone());
        assert!(permit.half_open);
        assert!(breaker.admit(1).is_none(), "one claim per half-open window");
        permit.release().await.expect("release");
        assert_eq!(breaker.mode(), Mode::HalfOpen);
        assert!(breaker.admit(1).is_some(), "released without outcome");
    }

    #[tokio::test]
    async fn verdict_closes_outage_opens_with_exponential_cooldown() {
        let breaker = Breaker::new(config());
        breaker.record_verdict();
        assert_eq!(breaker.mode(), Mode::Closed);
        assert!(!breaker.admit(1).expect("closed").half_open);
        breaker.record_outage();
        assert_eq!(breaker.mode(), Mode::Open);
        assert!(breaker.cooling_down());
        assert!(breaker.admit(1).is_none());
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!breaker.cooling_down());
        let permit = breaker.admit(1).expect("cooldown elapsed: half-open");
        assert!(permit.half_open);
        breaker.record_outage();
        breaker.record_outage();
        assert_eq!(breaker.lock().cooldown, Duration::from_millis(80), "capped");
        breaker.record_verdict();
        assert_eq!(breaker.lock().cooldown, Duration::from_millis(20), "reset");
    }
}
