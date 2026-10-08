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
//! works — never on a probe alone. Its outage streak counts the Store
//! outages since the last verdict ([`BreakerSnapshot`], reported to `health`
//! by [`crate::monitor`]). Before admitting, the Store probe runs
//! with the expectation of the catalog (source, adapter_version, types) and
//! the relay's last acknowledged receipt of the Store's current recovery
//! epoch; failures are `Ok(None)` (no claim), never another `DeliveryError`.
//!
//! The last acknowledged receipt is per epoch, so the gate keeps an epoch
//! hint: when the probe reports another epoch than the hint, the gate reads
//! the acknowledged head of that epoch and probes again before admitting.
//!
//! Order: a sticky regression of the current epoch; a non-operational Store
//! state (recovery mode, posture, read-only: an outage of that class); a
//! regression: when an operational Store no longer resolves the
//! acknowledged receipt (`regression_detected`, e.g. an in-place restore
//! the fingerprint cannot see), the relay reports it (`report_regression`,
//! which re-checks it and sets `recovery_pending`) and stays closed (sticky)
//! until the Store recovery epoch changes, whatever the Store answers;
//! finally `StoreStatus::admission` (missing registered types:
//! `store_unregistered_type`, the catalog skew health reports).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use audit_core::catalog::DOCUMENT_SOURCE;
use audit_core::{
    AuditStore, Catalog, LEGACY_ADAPTER_VERSION, OutageCode, ProbeExpectation, ReceiptIdentity,
    StoreStatus,
};
use outbox_delivery::runner::{ClaimAdmission, ClaimPermit};
use outbox_delivery::{DeliveryFuture, FenceResult};
use sqlx::{PgPool, Row};
use tokio::time::Instant;
use uuid::Uuid;

use crate::store::outage_of;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// The Store recovery epoch of the last probe (selects the acked head).
    epoch_hint: Option<i64>,
    regressed_epoch: Option<i64>,
    gate: Gate,
    outages: u64,
    /// Outages since the last structured verdict.
    outage_streak: u64,
}

/// What the breaker shows outside the process (fixed codes and counts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerSnapshot {
    pub mode: Mode,
    pub gate: Gate,
    /// Store outages since the last structured ingest verdict.
    pub outage_streak: u64,
    /// Store outages since the process started.
    pub outages: u64,
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
                epoch_hint: None,
                regressed_epoch: None,
                gate: Gate::Unknown,
                outages: 0,
                outage_streak: 0,
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
        self.snapshot().mode
    }

    pub fn gate(&self) -> Gate {
        self.lock().gate
    }

    pub fn outages(&self) -> u64 {
        self.lock().outages
    }

    pub fn outage_streak(&self) -> u64 {
        self.lock().outage_streak
    }

    /// Mode, gate and outage counts, read together.
    pub fn snapshot(&self) -> BreakerSnapshot {
        let inner = self.lock();
        BreakerSnapshot {
            mode: match inner.state {
                State::Closed => Mode::Closed,
                State::Open { .. } => Mode::Open,
                State::HalfOpen { .. } => Mode::HalfOpen,
            },
            gate: inner.gate,
            outage_streak: inner.outage_streak,
            outages: inner.outages,
        }
    }

    /// Store outage observed (ingest, probe, control call): open, and double
    /// the cooldown when it was already open or half-open.
    pub fn record_outage(&self) {
        let mut inner = self.lock();
        inner.outages += 1;
        inner.outage_streak += 1;
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
        inner.outage_streak = 0;
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

impl AckedHead {
    pub fn identity(&self) -> ReceiptIdentity {
        ReceiptIdentity {
            seq: self.store_seq,
            event_id: self.event_id,
            envelope_digest: self.envelope_digest,
        }
    }
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

/// What the relay expects the Store to accept for the Document source: the
/// catalog's registered types at the catalog adapter version.
pub fn expectation(last_ack: Option<ReceiptIdentity>) -> ProbeExpectation {
    ProbeExpectation::from_catalog(Catalog::embedded(), DOCUMENT_SOURCE, last_ack).unwrap_or(
        // The embedded catalog always has Document types; an empty list
        // would only lose the skew check, never admit more.
        ProbeExpectation {
            source: DOCUMENT_SOURCE.to_owned(),
            adapter_version: LEGACY_ADAPTER_VERSION,
            types: Vec::new(),
            last_ack,
        },
    )
}

/// [`ClaimAdmission`] over the breaker, the Store probe and the regression
/// gate.
pub struct BreakerAdmission {
    breaker: Arc<Breaker>,
    store: Arc<dyn AuditStore>,
    source: PgPool,
}

impl BreakerAdmission {
    pub fn new(breaker: Arc<Breaker>, store: Arc<dyn AuditStore>, source: PgPool) -> Self {
        Self {
            breaker,
            store,
            source,
        }
    }

    fn refuse(&self, gate: Gate) -> Option<i64> {
        self.breaker.set_gate(gate);
        if gate != Gate::SourceUnavailable {
            self.breaker.record_outage();
        }
        None
    }

    /// Probes with the acknowledged head of `epoch` (none before the first
    /// probe).
    async fn probe_at(
        &self,
        epoch: Option<i64>,
    ) -> Result<(StoreStatus, Option<ReceiptIdentity>), Gate> {
        let timeout = self.breaker.config.probe_timeout;
        let last_ack = match epoch {
            None => None,
            Some(epoch) => {
                match tokio::time::timeout(timeout, acked_head(&self.source, epoch)).await {
                    Ok(Ok(acked)) => acked.map(|acked| acked.identity()),
                    _ => return Err(Gate::SourceUnavailable),
                }
            }
        };
        let expected = expectation(last_ack);
        match tokio::time::timeout(timeout, self.store.probe(&expected)).await {
            Ok(Ok(status)) => Ok((status, last_ack)),
            Ok(Err(error)) => Err(Gate::Outage(outage_of(&error))),
            Err(_) => Err(Gate::Outage(OutageCode::Timeout)),
        }
    }

    async fn gate(&self) -> Option<i64> {
        let hint = self.breaker.lock().epoch_hint;
        let (mut status, mut last_ack) = match self.probe_at(hint).await {
            Ok(probed) => probed,
            Err(gate) => return self.refuse(gate),
        };
        if hint != Some(status.recovery_epoch) {
            let epoch = status.recovery_epoch;
            self.breaker.lock().epoch_hint = Some(epoch);
            (status, last_ack) = match self.probe_at(Some(epoch)).await {
                Ok(probed) => probed,
                Err(gate) => return self.refuse(gate),
            };
            if status.recovery_epoch != epoch {
                // The epoch moved between the two probes: try again later.
                self.breaker.lock().epoch_hint = Some(status.recovery_epoch);
                self.breaker.set_gate(Gate::Unknown);
                return None;
            }
        }
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
        // A Store that is not operational (recovery mode after a restore,
        // posture, read-only) gates first: a restore detected by the
        // fingerprint needs no regression report, the operator records the
        // relay's maximum seq with the epoch.
        if let Some(code) = status.state.outage_code() {
            return self.refuse(Gate::Outage(code));
        }
        if status.regression_detected {
            {
                let mut inner = self.breaker.lock();
                inner.regressed_epoch = Some(epoch);
                inner.gate = Gate::Regressed;
            }
            self.breaker.record_outage();
            if let Some(identity) = last_ack {
                // Best effort: the gate stays closed whatever the Store says.
                let _ = tokio::time::timeout(
                    self.breaker.config.probe_timeout,
                    self.store.report_regression(&identity),
                )
                .await;
            }
            eprintln!(
                "audit-relay: store_regressed (epoch {epoch}); claims stop until the Store \
                 recovery epoch changes"
            );
            return None;
        }
        if let Err(code) = status.admission() {
            return self.refuse(Gate::Outage(code));
        }
        self.breaker.set_gate(Gate::Ok);
        Some(epoch)
    }
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

    #[test]
    fn the_probe_expects_the_catalog_registration_and_the_last_ack() {
        let ack = ReceiptIdentity {
            seq: 7,
            event_id: Uuid::from_u128(7),
            envelope_digest: [7; 32],
        };
        let expected = expectation(Some(ack));
        assert_eq!(expected.source, DOCUMENT_SOURCE);
        assert_eq!(expected.adapter_version, LEGACY_ADAPTER_VERSION);
        assert_eq!(expected.last_ack, Some(ack));
        let catalog: Vec<String> = Catalog::embedded()
            .registered_types()
            .into_iter()
            .filter(|(source, _, _)| *source == DOCUMENT_SOURCE)
            .map(|(_, event_type, _)| event_type.to_owned())
            .collect();
        assert!(!catalog.is_empty());
        assert_eq!(expected.types, catalog);
        assert_eq!(expectation(None).last_ack, None);
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
        assert_eq!(
            breaker.snapshot(),
            BreakerSnapshot {
                mode: Mode::Open,
                gate: Gate::Unknown,
                outage_streak: 3,
                outages: 3,
            }
        );
        breaker.record_verdict();
        assert_eq!(breaker.lock().cooldown, Duration::from_millis(20), "reset");
        let snapshot = breaker.snapshot();
        assert_eq!((snapshot.mode, snapshot.outage_streak), (Mode::Closed, 0));
        assert_eq!(snapshot.outages, 3, "the total is kept");
    }
}
