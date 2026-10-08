//! [`outbox_delivery::OutboxStore`] over the `audit_relay` definer functions
//! (design §6.1–§6.3). The worker login holds EXECUTE on those functions and
//! no table privilege.
//!
//! Every driver error is `StoreUnknown` (trait contract); only a confirmed
//! zero-row fence is `Lost`.

use std::sync::Arc;
use std::time::Duration;

use outbox_delivery::observe::QueueSnapshot;
use outbox_delivery::{
    ClaimedEvent, DeliveryEnvelope, DeliveryError, DeliveryFuture, ErrorCode, FenceResult,
    OutboxStore,
};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ledger::{DeliveryLedger, FailureNote, Note};

/// `aggregate_type` of every relay envelope.
pub const AGGREGATE_TYPE: &str = "audit_outbox_events";

/// The pinned `audit_relay.delivery_policy` (design §6.3 defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayPolicy {
    pub revision: i64,
    pub max_attempts: i32,
    pub lease_min_ms: i64,
    pub lease_max_ms: i64,
    pub backoff_min_ms: i64,
    pub backoff_max_ms: i64,
    pub outage_streak_limit: i32,
}

impl Default for RelayPolicy {
    fn default() -> Self {
        Self {
            revision: 1,
            max_attempts: 16,
            lease_min_ms: 1_000,
            lease_max_ms: 120_000,
            backoff_min_ms: 1_000,
            backoff_max_ms: 300_000,
            outage_streak_limit: 64,
        }
    }
}

impl RelayPolicy {
    fn validate(&self) -> Result<(), DeliveryError> {
        let valid = self.revision > 0
            && (1..=32).contains(&self.max_attempts)
            && (1_000..=120_000).contains(&self.lease_min_ms)
            && (self.lease_min_ms..=120_000).contains(&self.lease_max_ms)
            && (1_000..=300_000).contains(&self.backoff_min_ms)
            && (self.backoff_min_ms..=300_000).contains(&self.backoff_max_ms)
            && (1..=1024).contains(&self.outage_streak_limit);
        if valid {
            Ok(())
        } else {
            Err(DeliveryError::InvalidConfig)
        }
    }

    fn lease_ms(&self, lease: Duration) -> Result<i64, DeliveryError> {
        let lease_ms =
            i64::try_from(lease.as_millis()).map_err(|_| DeliveryError::InvalidConfig)?;
        if !(self.lease_min_ms..=self.lease_max_ms).contains(&lease_ms)
            || lease != Duration::from_millis(lease_ms as u64)
        {
            return Err(DeliveryError::InvalidConfig);
        }
        Ok(lease_ms)
    }

    /// Reads the database policy.
    pub async fn load(pool: &PgPool) -> Result<Self, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM audit_relay.policy()")
            .fetch_one(pool)
            .await?;
        Ok(Self {
            revision: row.try_get("revision")?,
            max_attempts: row.try_get("max_attempts")?,
            lease_min_ms: row.try_get("lease_min_ms")?,
            lease_max_ms: row.try_get("lease_max_ms")?,
            backoff_min_ms: row.try_get("backoff_min_ms")?,
            backoff_max_ms: row.try_get("backoff_max_ms")?,
            outage_streak_limit: row.try_get("outage_streak_limit")?,
        })
    }
}

/// The relay's delivery state on the Document database.
pub struct RelayOutboxStore {
    pool: PgPool,
    expected: RelayPolicy,
    ledger: Arc<DeliveryLedger>,
}

impl RelayOutboxStore {
    pub fn new(pool: PgPool, expected: RelayPolicy, ledger: Arc<DeliveryLedger>) -> Self {
        Self {
            pool,
            expected,
            ledger,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn ledger(&self) -> &Arc<DeliveryLedger> {
        &self.ledger
    }
}

fn unknown<E>(_: E) -> DeliveryError {
    DeliveryError::StoreUnknown
}

fn fence(updated: bool) -> FenceResult {
    if updated {
        FenceResult::Updated
    } else {
        FenceResult::Lost
    }
}

fn map_claimed(row: &PgRow) -> Result<ClaimedEvent, DeliveryError> {
    let event_id: Uuid = row.try_get("event_id").map_err(unknown)?;
    let payload: Value = row.try_get("projection").map_err(unknown)?;
    let occurred_at: OffsetDateTime = row.try_get("occurred_at").map_err(unknown)?;
    Ok(ClaimedEvent {
        envelope: DeliveryEnvelope {
            event_id,
            event_type: row.try_get("event_type").map_err(unknown)?,
            aggregate_type: AGGREGATE_TYPE.to_owned(),
            aggregate_id: event_id,
            payload,
            occurred_at,
        },
        attempt: row.try_get("attempt_count").map_err(unknown)?,
        attempt_limit: row.try_get("attempt_limit").map_err(unknown)?,
        lease_token: row.try_get("lease_token").map_err(unknown)?,
        lease_owner: row.try_get("lease_owner").map_err(unknown)?,
        lease_expires_at: row.try_get("lease_expires_at").map_err(unknown)?,
    })
}

impl OutboxStore for RelayOutboxStore {
    fn queue_snapshot(&self) -> DeliveryFuture<'_, Option<QueueSnapshot>> {
        Box::pin(async move {
            let status: Value = sqlx::query_scalar("SELECT audit_relay.status()")
                .fetch_one(&self.pool)
                .await
                .map_err(unknown)?;
            let count = |key: &str| status[key].as_u64().ok_or(DeliveryError::StoreUnknown);
            let oldest = status["oldest_pending_age_seconds"]
                .as_f64()
                .map(|s| Duration::try_from_secs_f64(s.max(0.0)).map_err(unknown))
                .transpose()?;
            Ok(Some(QueueSnapshot {
                pending: count("pending")?,
                in_flight: count("leased")?,
                dead_letter: count("quarantined_total")?,
                expired: 0,
                exhausted: 0,
                oldest_age: oldest,
            }))
        })
    }

    fn verify_policy(&self) -> DeliveryFuture<'_, ()> {
        Box::pin(async move {
            self.expected.validate()?;
            let db = RelayPolicy::load(&self.pool).await.map_err(unknown)?;
            if db != self.expected {
                return Err(DeliveryError::PolicyMismatch);
            }
            Ok(())
        })
    }

    fn backoff_bounds(&self) -> (Duration, Duration) {
        (
            Duration::from_millis(u64::try_from(self.expected.backoff_min_ms).unwrap_or(1_000)),
            Duration::from_millis(u64::try_from(self.expected.backoff_max_ms).unwrap_or(300_000)),
        )
    }

    fn claim(
        &self,
        owner: Uuid,
        limit: u32,
        lease: Duration,
    ) -> DeliveryFuture<'_, Vec<ClaimedEvent>> {
        Box::pin(async move {
            self.expected.validate()?;
            if !(1..=32).contains(&limit) {
                return Err(DeliveryError::InvalidConfig);
            }
            let lease_ms = self.expected.lease_ms(lease)?;
            let rows = sqlx::query("SELECT * FROM audit_relay.claim($1, $2, $3)")
                .bind(owner)
                .bind(i32::try_from(limit).map_err(|_| DeliveryError::InvalidConfig)?)
                .bind(lease_ms)
                .fetch_all(&self.pool)
                .await
                .map_err(unknown)?;
            rows.iter().map(map_claimed).collect()
        })
    }

    fn renew(
        &self,
        event_id: Uuid,
        lease_token: Uuid,
        lease: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            let lease_ms = self.expected.lease_ms(lease)?;
            let updated: bool = sqlx::query_scalar("SELECT audit_relay.renew($1, $2, $3)")
                .bind(event_id)
                .bind(lease_token)
                .bind(lease_ms)
                .fetch_one(&self.pool)
                .await
                .map_err(unknown)?;
            Ok(fence(updated))
        })
    }

    /// Acks with the Store receipt the handler left in the ledger. Without a
    /// receipt there is nothing to ack: refuse instead of inventing one.
    fn settle_success(&self, event_id: Uuid, lease_token: Uuid) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            let Some(Note::Receipt {
                receipt,
                store_epoch,
            }) = self.ledger.take(event_id, lease_token)
            else {
                return Err(DeliveryError::StoreUnknown);
            };
            let updated: bool =
                sqlx::query_scalar("SELECT audit_relay.settle_success($1, $2, $3, $4, $5, $6)")
                    .bind(event_id)
                    .bind(lease_token)
                    .bind(receipt.seq)
                    .bind(receipt.envelope_digest.to_vec())
                    .bind(receipt.outcome.as_str())
                    .bind(store_epoch)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(unknown)?;
            Ok(fence(updated))
        })
    }

    /// Uses the ledger's detail code and outage mark. Nothing about an outage
    /// is derived from the runner's attempt-based backoff: the SQL function
    /// computes the outage backoff from the row's streak.
    fn settle_failure(
        &self,
        event_id: Uuid,
        lease_token: Uuid,
        code: ErrorCode,
        terminal: bool,
        backoff: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            let backoff_ms =
                i64::try_from(backoff.as_millis()).map_err(|_| DeliveryError::InvalidConfig)?;
            if !(self.expected.backoff_min_ms..=self.expected.backoff_max_ms).contains(&backoff_ms)
            {
                return Err(DeliveryError::InvalidConfig);
            }
            let note = match self.ledger.take(event_id, lease_token) {
                Some(Note::Failure(note)) => note,
                // Lost or mismatched note: the generic, attempt-consuming path.
                _ => FailureNote::verdict(code.as_str()),
            };
            let terminal = terminal && !note.outage;
            let updated: bool = sqlx::query_scalar(
                "SELECT audit_relay.settle_failure($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(event_id)
            .bind(lease_token)
            .bind(&note.code)
            .bind(terminal)
            .bind(backoff_ms)
            .bind(note.outage)
            .bind(note.streak_countable)
            .bind(note.relay_hold)
            .fetch_one(&self.pool)
            .await
            .map_err(unknown)?;
            Ok(fence(updated))
        })
    }

    fn reap_exhausted(&self, limit: u32) -> DeliveryFuture<'_, u64> {
        Box::pin(async move {
            if !(1..=32).contains(&limit) {
                return Err(DeliveryError::InvalidConfig);
            }
            let reaped: i64 = sqlx::query_scalar("SELECT audit_relay.reap_exhausted($1)")
                .bind(i32::try_from(limit).map_err(|_| DeliveryError::InvalidConfig)?)
                .fetch_one(&self.pool)
                .await
                .map_err(unknown)?;
            u64::try_from(reaped).map_err(unknown)
        })
    }
}
