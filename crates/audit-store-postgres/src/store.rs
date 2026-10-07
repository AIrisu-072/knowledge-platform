//! [`audit_core::AuditStore`] over `audit_store.ingest` / `audit_store.probe`.
//!
//! Verdicts (`rejected:<code>`, `conflict`) arrive as structured result rows
//! and are the only terminal outcomes. Every other failure — transport,
//! timeout, any SQLSTATE, an unregistered type, recovery mode, an invalid
//! posture or an unexpected response — is an outage that holds delivery
//! (design §6.3).

use std::fmt;
use std::time::Duration;

use audit_core::port::{BoundedCode, BoxFuture};
use audit_core::{
    AuditEnvelope, AuditStore, IngestOutcome, IngestReceipt, Origin, OutageCode, StoreError,
    StoreStatus, classify_sqlstate, validate_envelope,
};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::session::{SessionError, refuse_privileged};

/// Default bound on one ingest call; keep it below a third of the relay lease.
pub const DEFAULT_INGEST_TIMEOUT: Duration = Duration::from_secs(8);

/// PostgreSQL Audit Store client for the relay.
#[derive(Clone)]
pub struct PostgresAuditStore {
    pool: PgPool,
    timeout: Duration,
}

impl fmt::Debug for PostgresAuditStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresAuditStore")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl PostgresAuditStore {
    /// Wraps a pool after refusing superuser and owner-member sessions.
    pub async fn new(pool: PgPool, timeout: Duration) -> Result<Self, SessionError> {
        refuse_privileged(&pool).await?;
        Ok(Self { pool, timeout })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn call_ingest(&self, envelope: &AuditEnvelope) -> Result<IngestReceipt, StoreError> {
        // Defence in depth: the relay already validated the envelope.
        validate_envelope(envelope.as_value(), Origin::Relay).map_err(|rejection| {
            StoreError::Rejected {
                code: BoundedCode::new(rejection.code.as_str())
                    .unwrap_or_else(|| unreachable!("rejection codes are bounded")),
            }
        })?;
        let text = envelope.to_json_string();
        let query = sqlx::query(
            "SELECT status, seq, envelope_digest, adapter_version, code \
             FROM audit_store.ingest($1::text::jsonb)",
        )
        .bind(text)
        .fetch_one(&self.pool);
        let row = tokio::time::timeout(self.timeout, query)
            .await
            .map_err(|_| StoreError::Outage {
                code: OutageCode::Timeout,
            })?
            .map_err(|error| classify_sqlx_error(&error))?;
        decode_ingest(&row)
    }

    async fn call_probe(&self) -> Result<StoreStatus, StoreError> {
        let query = sqlx::query(
            "SELECT status, head_seq, recovery_epoch, writable, code FROM audit_store.probe()",
        )
        .fetch_one(&self.pool);
        let row = tokio::time::timeout(self.timeout, query)
            .await
            .map_err(|_| StoreError::Outage {
                code: OutageCode::Timeout,
            })?
            .map_err(|error| classify_sqlx_error(&error))?;
        let status: String = row.try_get("status").map_err(|_| unclassified())?;
        match status.as_str() {
            "ok" => Ok(StoreStatus {
                head_seq: row.try_get("head_seq").map_err(|_| unclassified())?,
                recovery_epoch: row.try_get("recovery_epoch").map_err(|_| unclassified())?,
                writable: row.try_get("writable").map_err(|_| unclassified())?,
            }),
            "read_only" => Err(StoreError::Outage {
                code: OutageCode::ReadOnly,
            }),
            "recovery_required" => Err(StoreError::RecoveryRequired),
            "posture_invalid" => Err(StoreError::Outage {
                code: OutageCode::PostureInvalid,
            }),
            _ => Err(unclassified()),
        }
    }
}

impl AuditStore for PostgresAuditStore {
    fn ingest<'a>(
        &'a self,
        envelope: &'a AuditEnvelope,
    ) -> BoxFuture<'a, Result<IngestReceipt, StoreError>> {
        Box::pin(self.call_ingest(envelope))
    }

    fn probe(&self) -> BoxFuture<'_, Result<StoreStatus, StoreError>> {
        Box::pin(self.call_probe())
    }
}

fn unclassified() -> StoreError {
    StoreError::Outage {
        code: OutageCode::Unclassified,
    }
}

/// Maps the structured ingest row (design §7.2 step 7).
fn decode_ingest(row: &PgRow) -> Result<IngestReceipt, StoreError> {
    let status: String = row.try_get("status").map_err(|_| unclassified())?;
    let code: Option<String> = row.try_get("code").map_err(|_| unclassified())?;
    if let Some(outcome) = IngestOutcome::parse(&status) {
        let seq: Option<i64> = row.try_get("seq").map_err(|_| unclassified())?;
        let digest: Option<Vec<u8>> = row.try_get("envelope_digest").map_err(|_| unclassified())?;
        let adapter_version: Option<i32> =
            row.try_get("adapter_version").map_err(|_| unclassified())?;
        return match (
            seq,
            digest.as_deref().and_then(crate::hex::digest32),
            adapter_version,
        ) {
            (Some(seq), Some(envelope_digest), Some(adapter_version)) => Ok(IngestReceipt {
                seq,
                envelope_digest,
                outcome,
                adapter_version,
            }),
            _ => Err(unclassified()),
        };
    }
    match status.as_str() {
        "conflict" => Err(StoreError::Conflict),
        "rejected" => Err(StoreError::Rejected {
            code: code
                .as_deref()
                .and_then(BoundedCode::new)
                .ok_or_else(unclassified)?,
        }),
        "recovery_required" if code.as_deref() == Some(OutageCode::PostureInvalid.as_str()) => {
            Err(StoreError::Outage {
                code: OutageCode::PostureInvalid,
            })
        }
        "recovery_required" => Err(StoreError::RecoveryRequired),
        "outage" if code.as_deref() == Some("unregistered_type") => Err(StoreError::Outage {
            code: OutageCode::VersionSkew,
        }),
        _ => Err(unclassified()),
    }
}

/// Classifies a driver error as an outage (design §6.3): it is never a
/// verdict, because verdicts are result rows.
pub fn classify_sqlx_error(error: &sqlx::Error) -> StoreError {
    let code = match error {
        sqlx::Error::Database(db) => match db.code().as_deref() {
            Some("KA001") => return StoreError::RecoveryRequired,
            Some("KA002") => OutageCode::PostureInvalid,
            Some(state) => classify_sqlstate(state).unwrap_or(OutageCode::Unclassified),
            None => OutageCode::Unclassified,
        },
        sqlx::Error::PoolTimedOut => OutageCode::Timeout,
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::BeginFailed => OutageCode::Transport,
        _ => OutageCode::Unclassified,
    };
    StoreError::Outage { code }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_failures_are_outages() {
        for (error, code) in [
            (sqlx::Error::PoolTimedOut, OutageCode::Timeout),
            (sqlx::Error::PoolClosed, OutageCode::Transport),
            (sqlx::Error::WorkerCrashed, OutageCode::Transport),
            (
                sqlx::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)),
                OutageCode::Transport,
            ),
            (sqlx::Error::RowNotFound, OutageCode::Unclassified),
            (
                sqlx::Error::ColumnNotFound("status".into()),
                OutageCode::Unclassified,
            ),
        ] {
            let mapped = classify_sqlx_error(&error);
            assert_eq!(mapped, StoreError::Outage { code });
            assert!(mapped.is_outage() && !mapped.is_terminal());
        }
    }
}
