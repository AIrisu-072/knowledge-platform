//! Generic, Search-independent delivery contracts.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
pub struct DeliveryEnvelope {
    pub event_id: Uuid,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub payload: Value,
    pub occurred_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClaimedEvent {
    pub envelope: DeliveryEnvelope,
    pub attempt: i32,
    pub attempt_limit: i32,
    pub lease_token: Uuid,
    pub lease_owner: Uuid,
    pub lease_expires_at: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FenceResult {
    Updated,
    Lost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryDecision {
    Applied,
    KnownNoop,
    Retryable(ErrorCode),
    Terminal(ErrorCode),
}

/// The only codes accepted by the outbox schema and emitted by the worker.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ErrorCode {
    UnsupportedEvent,
    InvalidEnvelope,
    SourceUnavailable,
    IndexingFailed,
    HandlerTimeout,
    DeliveryUnknown,
    DeliveryUnknownAtLimit,
}

impl ErrorCode {
    pub const ALL: [Self; 7] = [
        Self::UnsupportedEvent,
        Self::InvalidEnvelope,
        Self::SourceUnavailable,
        Self::IndexingFailed,
        Self::HandlerTimeout,
        Self::DeliveryUnknown,
        Self::DeliveryUnknownAtLimit,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedEvent => "unsupported_event",
            Self::InvalidEnvelope => "invalid_envelope",
            Self::SourceUnavailable => "source_unavailable",
            Self::IndexingFailed => "indexing_failed",
            Self::HandlerTimeout => "handler_timeout",
            Self::DeliveryUnknown => "delivery_unknown",
            Self::DeliveryUnknownAtLimit => "delivery_unknown_at_limit",
        }
    }
}

impl std::str::FromStr for ErrorCode {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unsupported_event" => Ok(Self::UnsupportedEvent),
            "invalid_envelope" => Ok(Self::InvalidEnvelope),
            "source_unavailable" => Ok(Self::SourceUnavailable),
            "indexing_failed" => Ok(Self::IndexingFailed),
            "handler_timeout" => Ok(Self::HandlerTimeout),
            "delivery_unknown" => Ok(Self::DeliveryUnknown),
            "delivery_unknown_at_limit" => Ok(Self::DeliveryUnknownAtLimit),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeliveryError {
    #[error("outbox delivery policy does not match the database policy")]
    PolicyMismatch,
    #[error("legacy outbox rows have exhausted the current attempt limit")]
    LegacyExhausted { count: i64, first_ids: Vec<Uuid> },
    #[error("invalid outbox delivery configuration")]
    InvalidConfig,
    #[error("outbox store result is unknown")]
    StoreUnknown,
}

pub type DeliveryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, DeliveryError>> + Send + 'a>>;
pub type HandlerFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Store operations return `Lost` only for a confirmed zero-row fence. Any
/// database failure, including an unknown commit result, returns `StoreUnknown`.
pub trait OutboxStore: Send + Sync {
    /// 実測値がない実装は None とし、ゼロとして公開しない。
    fn queue_snapshot(&self) -> DeliveryFuture<'_, Option<crate::observe::QueueSnapshot>> {
        Box::pin(async { Ok(None) })
    }
    /// 保存された親文脈だけを返す。旧行や未対応ストアは None。
    fn trace_context(
        &self,
        _event_id: Uuid,
        _lease_token: Uuid,
    ) -> DeliveryFuture<'_, Option<crate::observe::ValidatedTrace>> {
        Box::pin(async { Ok(None) })
    }

    fn verify_policy(&self) -> DeliveryFuture<'_, ()>;
    fn claim(
        &self,
        owner: Uuid,
        limit: u32,
        lease: Duration,
    ) -> DeliveryFuture<'_, Vec<ClaimedEvent>>;
    fn renew(
        &self,
        event_id: Uuid,
        lease_token: Uuid,
        lease: Duration,
    ) -> DeliveryFuture<'_, FenceResult>;
    fn settle_success(&self, event_id: Uuid, lease_token: Uuid) -> DeliveryFuture<'_, FenceResult>;
    fn settle_failure(
        &self,
        event_id: Uuid,
        lease_token: Uuid,
        code: ErrorCode,
        terminal: bool,
        backoff: Duration,
    ) -> DeliveryFuture<'_, FenceResult>;
    fn reap_exhausted(&self, limit: u32) -> DeliveryFuture<'_, u64>;
}
