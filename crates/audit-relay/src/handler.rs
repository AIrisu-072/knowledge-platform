//! The delivery handler (design §6.2 step 3–4, §6.3).
//!
//! Classification is two-way. Terminal (quarantine) only for:
//! - a Store structured verdict row: `conflict`, `rejected:<code>` (recorded
//!   as `conflict` / `rejected_<code>`);
//! - a relay verdict: `source_digest_mismatch`, `actor_mismatch`, or another
//!   catalog rejection of the projection.
//!
//! Everything else holds the row as an outage that returns the attempt:
//! transport, timeouts, any SQLSTATE, unknown outcomes, recovery mode,
//! regression, posture, unregistered types, identity refusals. Relay catalog
//! skew (`unknown_event_type` / `unknown_field`: a Document deploy ahead of
//! the relay) is held as `relay_catalog_skew`, exempt from the streak.
//!
//! A source mismatch records `audit.integrity.source_mismatch_detected`
//! first (once per event and code); when the Store cannot record it the row
//! is held as an outage instead of being quarantined.
//!
//! Quarantine codes use the Store code format `[a-z0-9_]{1,64}` (so that a
//! replay can record them as `quarantine_code`): relay rejection codes as
//! they are, `conflict`, and `rejected_<store code>` (truncated to 64 bytes).
//! Only `OutageCode::counts_toward_outage_streak` outages (residual
//! `store_internal` / `store_other`) may advance `outage_streak`.

use std::sync::Arc;
use std::time::Duration;

use audit_core::codes::{Rejection, RejectionCode};
use audit_core::{
    AuditEnvelope, AuditStore, DocumentStagingProjection, IngestOutcome, OutageCode, RelayControl,
    RelayControlKind, SourceMismatchCode, StoreError,
};
use outbox_delivery::runner::{DeliveryContext, DeliveryHandler};
use outbox_delivery::{DeliveryDecision, DeliveryEnvelope, ErrorCode, HandlerFuture};
use sqlx::PgPool;
use uuid::Uuid;

use crate::breaker::{Breaker, BreakerPermit};
use crate::ledger::{DeliveryLedger, FailureNote, Note};
use crate::store::outage_of;

/// The projection from a claim row to an envelope (`audit_core::project`
/// in production; tests may substitute a re-projecting adapter).
pub type Projector =
    Arc<dyn Fn(&DocumentStagingProjection) -> Result<AuditEnvelope, Rejection> + Send + Sync>;

/// Held while the relay's catalog is behind the Document producer.
pub const CATALOG_SKEW: &str = "relay_catalog_skew";
/// Held when the claim projection does not parse (SQL/relay version skew).
pub const PROJECTION_INVALID: &str = "relay_projection_invalid";
/// Held when the source database fails during the mismatch bookkeeping.
pub const SOURCE_UNAVAILABLE: &str = "relay_source_unavailable";
/// Held when the Store returns neither a receipt nor a verdict nor an outage
/// class (an unknown outcome).
pub const OUTCOME_UNKNOWN: &str = "store_outcome_unknown";
/// Quarantine code of a Store `conflict` verdict.
pub const STORE_CONFLICT: &str = "conflict";
/// Prefix of the quarantine code of a Store `rejected:<code>` verdict. Relay
/// codes use the Store code format `[a-z0-9_]{1,64}` so that a replay can
/// record them.
pub const STORE_REJECTED_PREFIX: &str = "rejected_";
/// Whether a held code may count toward `outage_streak`: only Store outage
/// classes that audit-core marks as residual (design §6.3). Relay hold codes
/// (catalog skew, projection, source) never count.
pub fn streak_countable(code: &str) -> bool {
    OutageCode::parse(code).is_some_and(OutageCode::counts_toward_outage_streak)
}

/// The quarantine code of a Store structured verdict.
pub fn verdict_code(error: &StoreError) -> String {
    match error {
        StoreError::Conflict => STORE_CONFLICT.to_owned(),
        StoreError::Rejected { code } => {
            let mut text = format!("{STORE_REJECTED_PREFIX}{}", code.as_str());
            text.truncate(64);
            text
        }
        _ => format!("{STORE_REJECTED_PREFIX}unclassified"),
    }
}

/// The outage code of a Store failure that is not a verdict.
pub fn outage_code(error: &StoreError) -> &'static str {
    match error.outage_code() {
        Some(code) => code.as_str(),
        None => OUTCOME_UNKNOWN,
    }
}

/// Handler timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandlerConfig {
    /// Bound on one ingest (below a third of the lease).
    pub ingest_timeout: Duration,
    /// Bound on one control-event call.
    pub control_timeout: Duration,
    /// Attempts at recording a mismatch control event per delivery.
    pub control_attempts: u32,
}

impl Default for HandlerConfig {
    fn default() -> Self {
        Self {
            ingest_timeout: Duration::from_secs(8),
            control_timeout: Duration::from_secs(3),
            control_attempts: 3,
        }
    }
}

/// Delivers one claimed staging row into the Audit Store.
pub struct AuditDeliveryHandler {
    store: Arc<dyn AuditStore>,
    source: PgPool,
    ledger: Arc<DeliveryLedger>,
    breaker: Arc<Breaker>,
    projector: Projector,
    config: HandlerConfig,
}

impl AuditDeliveryHandler {
    pub fn new(
        store: Arc<dyn AuditStore>,
        source: PgPool,
        ledger: Arc<DeliveryLedger>,
        breaker: Arc<Breaker>,
        config: HandlerConfig,
    ) -> Self {
        Self {
            store,
            source,
            ledger,
            breaker,
            projector: Arc::new(audit_core::project),
            config,
        }
    }

    /// Replaces the projection (tests: adapter re-projection).
    pub fn with_projector(mut self, projector: Projector) -> Self {
        self.projector = projector;
        self
    }

    fn hold(&self, id: Uuid, token: Uuid, code: &str, countable: bool) -> DeliveryDecision {
        self.ledger.record(
            id,
            token,
            Note::Failure(FailureNote::outage(code, countable)),
        );
        DeliveryDecision::Retryable(ErrorCode::DeliveryUnknown)
    }

    fn quarantine(
        &self,
        id: Uuid,
        token: Uuid,
        code: &str,
        runner_code: ErrorCode,
    ) -> DeliveryDecision {
        self.ledger
            .record(id, token, Note::Failure(FailureNote::verdict(code)));
        DeliveryDecision::Terminal(runner_code)
    }

    async fn deliver_claim(
        &self,
        envelope: DeliveryEnvelope,
        context: DeliveryContext,
        permit: BreakerPermit,
    ) -> DeliveryDecision {
        let id = envelope.event_id;
        let token = context.outbox_token;
        let row: DocumentStagingProjection = match serde_json::from_value(envelope.payload) {
            Ok(row) => row,
            Err(_) => return self.hold(id, token, PROJECTION_INVALID, false),
        };
        // A changed source row wins over every other finding, oversize included.
        if !row.source_intact {
            return self
                .source_mismatch(id, token, SourceMismatchCode::SourceDigestMismatch)
                .await;
        }
        let audit = match (self.projector)(&row) {
            Ok(audit) => audit,
            Err(rejection) => {
                return match rejection.code {
                    RejectionCode::SourceDigestMismatch => {
                        self.source_mismatch(id, token, SourceMismatchCode::SourceDigestMismatch)
                            .await
                    }
                    RejectionCode::ActorMismatch => {
                        self.source_mismatch(id, token, SourceMismatchCode::ActorMismatch)
                            .await
                    }
                    RejectionCode::UnknownEventType | RejectionCode::UnknownField => {
                        self.hold(id, token, CATALOG_SKEW, false)
                    }
                    code => self.quarantine(id, token, code.as_str(), ErrorCode::InvalidEnvelope),
                };
            }
        };
        if audit.id() != id {
            return self.quarantine(
                id,
                token,
                RejectionCode::InvalidEnvelope.as_str(),
                ErrorCode::InvalidEnvelope,
            );
        }
        let result =
            tokio::time::timeout(self.config.ingest_timeout, self.store.ingest(&audit)).await;
        match result {
            Ok(Ok(receipt)) => {
                self.breaker.record_verdict();
                self.ledger.record(
                    id,
                    token,
                    Note::Receipt {
                        receipt,
                        store_epoch: permit.epoch,
                    },
                );
                if receipt.outcome == IngestOutcome::Stored {
                    DeliveryDecision::Applied
                } else {
                    DeliveryDecision::KnownNoop
                }
            }
            Ok(Err(error)) if error.is_terminal() => {
                self.breaker.record_verdict();
                self.quarantine(id, token, &verdict_code(&error), ErrorCode::InvalidEnvelope)
            }
            Ok(Err(error)) => {
                self.breaker.record_outage();
                self.hold(
                    id,
                    token,
                    outage_code(&error),
                    error.counts_toward_outage_streak(),
                )
            }
            Err(_) => {
                self.breaker.record_outage();
                self.hold(id, token, OutageCode::Timeout.as_str(), false)
            }
        }
    }

    /// Records the source mismatch control event (once per event and code),
    /// then quarantines; holds as an outage when it cannot be recorded.
    async fn source_mismatch(
        &self,
        id: Uuid,
        token: Uuid,
        mismatch: SourceMismatchCode,
    ) -> DeliveryDecision {
        let code = mismatch.as_str();
        let recorded: Result<Option<i64>, sqlx::Error> =
            sqlx::query_scalar("SELECT audit_relay.mismatch_seq($1, $2, $3)")
                .bind(id)
                .bind(token)
                .bind(code)
                .fetch_one(&self.source)
                .await;
        let recorded = match recorded {
            Ok(recorded) => recorded,
            Err(_) => return self.hold(id, token, SOURCE_UNAVAILABLE, false),
        };
        if recorded.is_none() {
            let control = RelayControl::from(RelayControlKind::SourceMismatchDetected {
                event_id: id,
                code: mismatch,
            });
            let mut last_error = OutageCode::Other;
            let mut seq = None;
            for attempt in 0..self.config.control_attempts.max(1) {
                if attempt > 0 {
                    tokio::time::sleep(Duration::from_millis(200 * u64::from(attempt))).await;
                }
                match tokio::time::timeout(
                    self.config.control_timeout,
                    self.store.record_relay_control(&control),
                )
                .await
                {
                    Ok(Ok(receipt)) => {
                        seq = Some(receipt.seq);
                        break;
                    }
                    Ok(Err(error)) => last_error = outage_of(&error),
                    Err(_) => last_error = OutageCode::Timeout,
                }
            }
            let Some(seq) = seq else {
                self.breaker.record_outage();
                return self.hold(id, token, last_error.as_str(), false);
            };
            let noted: Result<bool, sqlx::Error> =
                sqlx::query_scalar("SELECT audit_relay.note_mismatch($1, $2, $3, $4)")
                    .bind(id)
                    .bind(token)
                    .bind(code)
                    .bind(seq)
                    .fetch_one(&self.source)
                    .await;
            if noted.is_err() {
                return self.hold(id, token, SOURCE_UNAVAILABLE, false);
            }
        }
        self.quarantine(id, token, code, ErrorCode::InvalidEnvelope)
    }
}

impl DeliveryHandler<BreakerPermit> for AuditDeliveryHandler {
    fn deliver(
        &self,
        envelope: DeliveryEnvelope,
        context: DeliveryContext,
        permit: BreakerPermit,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        Box::pin(self.deliver_claim(envelope, context, permit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audit_core::IngestRow;

    /// A Store verdict, built the only way the port allows.
    fn verdict(status: &str, code: Option<&str>) -> StoreError {
        IngestRow {
            status: status.to_owned(),
            seq: None,
            envelope_digest: None,
            adapter_version: None,
            code: code.map(str::to_owned),
        }
        .into_result()
        .expect_err("a verdict")
    }

    #[test]
    fn verdicts_and_outages_are_two_way() {
        let conflict = verdict("conflict", None);
        assert_eq!(verdict_code(&conflict), "conflict");
        let rejected = verdict("rejected", Some("invalid_provenance"));
        assert_eq!(verdict_code(&rejected), "rejected_invalid_provenance");
        let long = verdict("rejected", Some(&"x".repeat(64)));
        let code = verdict_code(&long);
        assert_eq!(code.len(), 64);
        assert!(
            audit_core::kinds::is_code(&code),
            "quarantine codes are Store codes"
        );
        assert!(rejected.is_terminal() && conflict.is_terminal());
        for code in OutageCode::ALL {
            let error = StoreError::outage(code);
            assert!(!error.is_terminal());
            assert_eq!(outage_code(&error), code.as_str());
            assert_eq!(
                streak_countable(outage_code(&error)),
                matches!(code, OutageCode::Internal | OutageCode::Other),
                "{code:?}"
            );
            assert_eq!(
                streak_countable(outage_code(&error)),
                error.counts_toward_outage_streak()
            );
        }
        for held in [
            CATALOG_SKEW,
            OUTCOME_UNKNOWN,
            PROJECTION_INVALID,
            SOURCE_UNAVAILABLE,
        ] {
            assert!(!streak_countable(held), "{held}");
        }
        for mismatch in [
            SourceMismatchCode::SourceDigestMismatch,
            SourceMismatchCode::ActorMismatch,
        ] {
            assert!(audit_core::kinds::is_code(mismatch.as_str()));
        }
    }
}
