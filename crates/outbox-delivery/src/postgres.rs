//! PostgreSQL delivery state: bounded claims and fenced lease settlement.

use std::time::Duration;

use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::model::{
    ClaimedEvent, DeliveryEnvelope, DeliveryError, DeliveryFuture, ErrorCode, FenceResult,
    OutboxStore,
};
use crate::observe::{QueueSnapshot, ValidatedTrace, validate_trace_context};
use crate::policy::DeliveryPolicy;

pub struct PostgresOutboxStore {
    pool: PgPool,
    expected: DeliveryPolicy,
}

impl PostgresOutboxStore {
    pub fn new(pool: PgPool, expected: DeliveryPolicy) -> Self {
        Self { pool, expected }
    }

    fn validate_expected(&self) -> Result<(), DeliveryError> {
        let p = self.expected;
        let valid = p.revision > 0
            && (1..=32).contains(&p.max_attempts)
            && (1_000..=120_000).contains(&p.lease_min_ms)
            && (p.lease_min_ms..=120_000).contains(&p.lease_max_ms)
            && (1_000..=300_000).contains(&p.backoff_min_ms)
            && (p.backoff_min_ms..=300_000).contains(&p.backoff_max_ms);
        if valid {
            Ok(())
        } else {
            Err(DeliveryError::InvalidConfig)
        }
    }

    fn validate_claim(&self, limit: u32, lease: Duration) -> Result<i64, DeliveryError> {
        if !(1..=32).contains(&limit) {
            return Err(DeliveryError::InvalidConfig);
        }
        let lease_ms =
            i64::try_from(lease.as_millis()).map_err(|_| DeliveryError::InvalidConfig)?;
        if !(self.expected.lease_min_ms..=self.expected.lease_max_ms).contains(&lease_ms)
            || lease != Duration::from_millis(lease_ms as u64)
        {
            return Err(DeliveryError::InvalidConfig);
        }
        Ok(lease_ms)
    }

    /// Keep the policy row locked through the caller's transaction. A rollout
    /// cannot change its revision or any bound between this check and claim.
    async fn guard_policy(&self, tx: &mut Transaction<'_, Postgres>) -> Result<i32, DeliveryError> {
        let row = sqlx::query(
            "SELECT revision,max_attempts,lease_min_ms,lease_max_ms,backoff_min_ms,backoff_max_ms \
             FROM outbox_delivery_policy WHERE policy_id=1 FOR SHARE",
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| DeliveryError::StoreUnknown)?
        .ok_or(DeliveryError::PolicyMismatch)?;
        let db_policy = DeliveryPolicy {
            revision: row
                .try_get("revision")
                .map_err(|_| DeliveryError::StoreUnknown)?,
            max_attempts: row
                .try_get("max_attempts")
                .map_err(|_| DeliveryError::StoreUnknown)?,
            lease_min_ms: row
                .try_get("lease_min_ms")
                .map_err(|_| DeliveryError::StoreUnknown)?,
            lease_max_ms: row
                .try_get("lease_max_ms")
                .map_err(|_| DeliveryError::StoreUnknown)?,
            backoff_min_ms: row
                .try_get("backoff_min_ms")
                .map_err(|_| DeliveryError::StoreUnknown)?,
            backoff_max_ms: row
                .try_get("backoff_max_ms")
                .map_err(|_| DeliveryError::StoreUnknown)?,
        };
        if db_policy != self.expected {
            return Err(DeliveryError::PolicyMismatch);
        }

        // Historic unpinned attempts are an operator decision, not an
        // automatic reset or terminal transition. Bound diagnostic IDs to 32.
        let (count, first_ids): (i64, Vec<Uuid>) = sqlx::query_as(
            "SELECT \
             (SELECT count(*) FROM outbox_events WHERE delivered_at IS NULL \
                AND dead_lettered_at IS NULL AND attempt_limit IS NULL \
                AND attempt_count >= $1), \
             ARRAY(SELECT event_id FROM outbox_events WHERE delivered_at IS NULL \
                AND dead_lettered_at IS NULL AND attempt_limit IS NULL \
                AND attempt_count >= $1 ORDER BY event_id LIMIT 32)",
        )
        .bind(db_policy.max_attempts)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| DeliveryError::StoreUnknown)?;
        if count > 0 {
            return Err(DeliveryError::LegacyExhausted { count, first_ids });
        }
        Ok(db_policy.max_attempts)
    }
}

fn map_claimed(row: PgRow) -> Result<ClaimedEvent, DeliveryError> {
    let envelope = DeliveryEnvelope {
        event_id: row
            .try_get("event_id")
            .map_err(|_| DeliveryError::StoreUnknown)?,
        event_type: row
            .try_get("event_type")
            .map_err(|_| DeliveryError::StoreUnknown)?,
        aggregate_type: row
            .try_get("aggregate_type")
            .map_err(|_| DeliveryError::StoreUnknown)?,
        aggregate_id: row
            .try_get("aggregate_id")
            .map_err(|_| DeliveryError::StoreUnknown)?,
        payload: row
            .try_get("payload")
            .map_err(|_| DeliveryError::StoreUnknown)?,
        occurred_at: row
            .try_get("occurred_at")
            .map_err(|_| DeliveryError::StoreUnknown)?,
    };
    let lease_expires_at: OffsetDateTime = row
        .try_get("lease_expires_at")
        .map_err(|_| DeliveryError::StoreUnknown)?;
    Ok(ClaimedEvent {
        envelope,
        attempt: row
            .try_get("attempt_count")
            .map_err(|_| DeliveryError::StoreUnknown)?,
        attempt_limit: row
            .try_get("attempt_limit")
            .map_err(|_| DeliveryError::StoreUnknown)?,
        lease_token: row
            .try_get("lease_token")
            .map_err(|_| DeliveryError::StoreUnknown)?,
        lease_owner: row
            .try_get("lease_owner")
            .map_err(|_| DeliveryError::StoreUnknown)?,
        lease_expires_at,
    })
}

impl OutboxStore for PostgresOutboxStore {
    fn queue_snapshot(&self) -> DeliveryFuture<'_, Option<QueueSnapshot>> {
        Box::pin(async move {
            // 同じ DB 時計と MVCC スナップショットで状態を導出する。
            let row = sqlx::query(
                "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t) \
                 SELECT count(*) FILTER (WHERE delivered_at IS NULL AND dead_lettered_at IS NULL \
                     AND (lease_expires_at IS NULL OR lease_expires_at <= tick.t)) AS pending, \
                   count(*) FILTER (WHERE delivered_at IS NULL AND dead_lettered_at IS NULL \
                     AND lease_expires_at > tick.t) AS in_flight, \
                   count(*) FILTER (WHERE dead_lettered_at IS NOT NULL) AS dead_letter, \
                   count(*) FILTER (WHERE delivered_at IS NULL AND dead_lettered_at IS NULL \
                     AND lease_expires_at <= tick.t) AS expired, \
                   count(*) FILTER (WHERE delivered_at IS NULL AND dead_lettered_at IS NULL \
                     AND (lease_expires_at IS NULL OR lease_expires_at <= tick.t) \
                     AND attempt_limit IS NOT NULL AND attempt_count >= attempt_limit) AS exhausted, \
                   extract(epoch FROM (max(tick.t) - min(occurred_at) FILTER \
                     (WHERE delivered_at IS NULL AND dead_lettered_at IS NULL)))::double precision AS oldest_age \
                 FROM outbox_events CROSS JOIN tick",
            ).fetch_one(&self.pool).await.map_err(|_| DeliveryError::StoreUnknown)?;
            let count = |column| -> Result<u64, DeliveryError> {
                let value: i64 = row
                    .try_get(column)
                    .map_err(|_| DeliveryError::StoreUnknown)?;
                u64::try_from(value).map_err(|_| DeliveryError::StoreUnknown)
            };
            let oldest: Option<f64> = row
                .try_get("oldest_age")
                .map_err(|_| DeliveryError::StoreUnknown)?;
            let oldest_age = oldest
                .map(|seconds| {
                    Duration::try_from_secs_f64(seconds.max(0.0))
                        .map_err(|_| DeliveryError::StoreUnknown)
                })
                .transpose()?;
            Ok(Some(QueueSnapshot {
                pending: count("pending")?,
                in_flight: count("in_flight")?,
                dead_letter: count("dead_letter")?,
                expired: count("expired")?,
                exhausted: count("exhausted")?,
                oldest_age,
            }))
        })
    }

    fn trace_context(
        &self,
        event_id: Uuid,
        lease_token: Uuid,
    ) -> DeliveryFuture<'_, Option<ValidatedTrace>> {
        Box::pin(async move {
            // 巨大な保存値もアプリへ転送する前に落とす。所有権の判定は既存 renew が担う。
            let row = sqlx::query(
                "SELECT CASE WHEN octet_length(traceparent)<=512 THEN traceparent END AS traceparent, \
                        CASE WHEN octet_length(tracestate)<=512 THEN tracestate END AS tracestate \
                 FROM outbox_events WHERE event_id=$1 AND lease_token=$2",
            ).bind(event_id).bind(lease_token).fetch_optional(&self.pool).await.map_err(|_| DeliveryError::StoreUnknown)?;
            let Some(row) = row else {
                return Ok(None);
            };
            let parent: Option<String> = row
                .try_get("traceparent")
                .map_err(|_| DeliveryError::StoreUnknown)?;
            let state: Option<String> = row
                .try_get("tracestate")
                .map_err(|_| DeliveryError::StoreUnknown)?;
            Ok(validate_trace_context(parent.as_deref(), state.as_deref()))
        })
    }

    fn verify_policy(&self) -> DeliveryFuture<'_, ()> {
        Box::pin(async move {
            self.validate_expected()?;
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|_| DeliveryError::StoreUnknown)?;
            self.guard_policy(&mut tx).await?;
            tx.commit().await.map_err(|_| DeliveryError::StoreUnknown)
        })
    }

    fn claim(
        &self,
        owner: Uuid,
        limit: u32,
        lease: Duration,
    ) -> DeliveryFuture<'_, Vec<ClaimedEvent>> {
        Box::pin(async move {
            self.validate_expected()?;
            let lease_ms = self.validate_claim(limit, lease)?;
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|_| DeliveryError::StoreUnknown)?;
            let db_max = self.guard_policy(&mut tx).await?;
            // The caller has already reserved `limit` free dispatch permits.
            // The row locks end at commit; the lease token fences later work.
            let rows = sqlx::query(
                "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t), \
                 picked AS ( \
                   SELECT o.event_id FROM outbox_events AS o CROSS JOIN tick \
                   WHERE o.delivered_at IS NULL AND o.dead_lettered_at IS NULL \
                     AND o.available_at <= tick.t \
                     AND (o.lease_expires_at IS NULL OR o.lease_expires_at <= tick.t) \
                     AND o.attempt_count < COALESCE(o.attempt_limit, $2) \
                   ORDER BY o.available_at,o.occurred_at,o.event_id \
                   LIMIT $1 FOR UPDATE OF o SKIP LOCKED \
                 ) \
                 UPDATE outbox_events AS o \
                 SET attempt_count=o.attempt_count+1, \
                     attempt_limit=COALESCE(o.attempt_limit,$2), \
                     lease_token=gen_random_uuid(),lease_owner=$3, \
                     lease_expires_at=tick.t+($4::bigint * interval '1 millisecond'), \
                     last_attempt_at=tick.t \
                 FROM picked,tick WHERE o.event_id=picked.event_id \
                 RETURNING o.event_id,o.event_type,o.aggregate_type,o.aggregate_id, \
                           o.payload,o.occurred_at,o.attempt_count,o.attempt_limit, \
                           o.lease_token,o.lease_owner,o.lease_expires_at",
            )
            .bind(i64::from(limit))
            .bind(db_max)
            .bind(owner)
            .bind(lease_ms)
            .fetch_all(&mut *tx)
            .await
            .map_err(|_| DeliveryError::StoreUnknown)?;
            let claimed = rows
                .into_iter()
                .map(map_claimed)
                .collect::<Result<Vec<_>, _>>()?;
            tx.commit().await.map_err(|_| DeliveryError::StoreUnknown)?;
            Ok(claimed)
        })
    }

    // One DB clock sample per statement is used for both the expiry fence and
    // the new deadline. A zero-row update is the only confirmed `Lost` case.
    fn renew(
        &self,
        event_id: Uuid,
        lease_token: Uuid,
        lease: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.validate_expected()?;
            let lease_ms = self.validate_claim(1, lease)?;
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|_| DeliveryError::StoreUnknown)?;
            let changed: Option<Uuid> = sqlx::query_scalar(
                "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t) \
                 UPDATE outbox_events AS o \
                 SET lease_expires_at=tick.t+($3::bigint * interval '1 millisecond') \
                 FROM tick \
                 WHERE o.event_id=$1 AND o.lease_token=$2 \
                   AND o.delivered_at IS NULL AND o.dead_lettered_at IS NULL \
                   AND o.lease_expires_at > tick.t \
                 RETURNING o.event_id",
            )
            .bind(event_id)
            .bind(lease_token)
            .bind(lease_ms)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| DeliveryError::StoreUnknown)?;
            tx.commit().await.map_err(|_| DeliveryError::StoreUnknown)?;
            Ok(if changed.is_some() {
                FenceResult::Updated
            } else {
                FenceResult::Lost
            })
        })
    }

    fn settle_success(&self, event_id: Uuid, lease_token: Uuid) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|_| DeliveryError::StoreUnknown)?;
            let changed: Option<Uuid> = sqlx::query_scalar(
                "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t) \
                 UPDATE outbox_events AS o \
                 SET delivered_at=tick.t,lease_token=NULL,lease_owner=NULL,lease_expires_at=NULL \
                 FROM tick \
                 WHERE o.event_id=$1 AND o.lease_token=$2 \
                   AND o.delivered_at IS NULL AND o.dead_lettered_at IS NULL \
                   AND o.lease_expires_at > tick.t \
                 RETURNING o.event_id",
            )
            .bind(event_id)
            .bind(lease_token)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| DeliveryError::StoreUnknown)?;
            tx.commit().await.map_err(|_| DeliveryError::StoreUnknown)?;
            Ok(if changed.is_some() {
                FenceResult::Updated
            } else {
                FenceResult::Lost
            })
        })
    }

    fn settle_failure(
        &self,
        event_id: Uuid,
        lease_token: Uuid,
        code: ErrorCode,
        terminal: bool,
        backoff: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async move {
            self.validate_expected()?;
            let backoff_ms =
                i64::try_from(backoff.as_millis()).map_err(|_| DeliveryError::InvalidConfig)?;
            if !(self.expected.backoff_min_ms..=self.expected.backoff_max_ms).contains(&backoff_ms)
                || backoff != Duration::from_millis(backoff_ms as u64)
            {
                return Err(DeliveryError::InvalidConfig);
            }
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|_| DeliveryError::StoreUnknown)?;
            let changed: Option<Uuid> = sqlx::query_scalar(
                "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t) \
                 UPDATE outbox_events AS o \
                 SET available_at=CASE WHEN $4::boolean OR o.attempt_count >= o.attempt_limit \
                                       THEN o.available_at \
                                       ELSE tick.t+($5::bigint * interval '1 millisecond') END, \
                     dead_lettered_at=CASE WHEN $4::boolean OR o.attempt_count >= o.attempt_limit \
                                           THEN tick.t ELSE NULL END, \
                     last_error_code=$3, \
                     lease_token=NULL,lease_owner=NULL,lease_expires_at=NULL \
                 FROM tick \
                 WHERE o.event_id=$1 AND o.lease_token=$2 \
                   AND o.delivered_at IS NULL AND o.dead_lettered_at IS NULL \
                   AND o.lease_expires_at > tick.t AND o.attempt_limit IS NOT NULL \
                 RETURNING o.event_id",
            )
            .bind(event_id)
            .bind(lease_token)
            .bind(code.as_str())
            .bind(terminal)
            .bind(backoff_ms)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| DeliveryError::StoreUnknown)?;
            tx.commit().await.map_err(|_| DeliveryError::StoreUnknown)?;
            Ok(if changed.is_some() {
                FenceResult::Updated
            } else {
                FenceResult::Lost
            })
        })
    }

    fn reap_exhausted(&self, limit: u32) -> DeliveryFuture<'_, u64> {
        Box::pin(async move {
            self.validate_expected()?;
            if !(1..=32).contains(&limit) {
                return Err(DeliveryError::InvalidConfig);
            }
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|_| DeliveryError::StoreUnknown)?;
            self.guard_policy(&mut tx).await?;
            let reaped = sqlx::query(
                "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t), \
                 exhausted AS ( \
                   SELECT o.event_id FROM outbox_events AS o CROSS JOIN tick \
                   WHERE o.delivered_at IS NULL AND o.dead_lettered_at IS NULL \
                     AND o.attempt_limit IS NOT NULL \
                     AND o.attempt_count >= o.attempt_limit \
                     AND (o.lease_token IS NULL OR o.lease_expires_at <= tick.t) \
                   ORDER BY o.last_attempt_at NULLS FIRST,o.event_id \
                   LIMIT $1 FOR UPDATE OF o SKIP LOCKED \
                 ) \
                 UPDATE outbox_events AS o \
                 SET dead_lettered_at=tick.t, \
                     lease_token=NULL,lease_owner=NULL,lease_expires_at=NULL, \
                     last_error_code='delivery_unknown_at_limit' \
                 FROM exhausted,tick WHERE o.event_id=exhausted.event_id \
                 RETURNING o.event_id",
            )
            .bind(i64::from(limit))
            .fetch_all(&mut *tx)
            .await
            .map_err(|_| DeliveryError::StoreUnknown)?;
            tx.commit().await.map_err(|_| DeliveryError::StoreUnknown)?;
            Ok(reaped.len() as u64)
        })
    }
}
