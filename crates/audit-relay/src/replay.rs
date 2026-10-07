//! `audit-relay replay --event-id` (design §6.4).
//!
//! 1. Record `audit.delivery.replay_requested` in the Store with the
//!    operator's own Store login (`quarantine_code`: the code being cleared)
//!    and take its position (seq, recovery epoch); abort if that fails.
//! 2. `audit_relay.replay(event_id, control_seq, control_epoch)` in one
//!    transaction: history row, attempt budget reset, back to pending.
//!
//! The Document function cannot verify the Store fact; reconcile reports a
//! history row that does not resolve to that control event (same event,
//! epoch and code, one to one) as `unaudited_replay`.

use audit_core::{AuditStore, BoundedCode, RelayControl, RelayControlKind, StoreError};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::reconcile::lookup_deliveries;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayOutcome {
    pub event_id: Uuid,
    pub previous_quarantine_code: String,
    pub control_seq: i64,
    pub control_epoch: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    #[error("the event is not registered")]
    NotFound,
    #[error("the event is not quarantined (state {0})")]
    NotQuarantined(String),
    #[error("the quarantine code is not a Store code ([a-z0-9_]{{1,64}})")]
    InvalidCode,
    #[error("audit store refused or failed to record the replay: {0}")]
    Store(StoreError),
    #[error(
        "the replay was recorded in the Store (seq {control_seq}) but the delivery changed \
         before it could be reset"
    )]
    Refused { control_seq: i64 },
    #[error("source database error ({0})")]
    Source(String),
}

fn source_error(error: sqlx::Error) -> ReplayError {
    ReplayError::Source(match &error {
        sqlx::Error::Database(db) => db.code().map(|c| c.into_owned()).unwrap_or_default(),
        _ => "unavailable".to_owned(),
    })
}

/// Replays one quarantined delivery.
pub async fn replay(
    source: &PgPool,
    store: &dyn AuditStore,
    event_id: Uuid,
) -> Result<ReplayOutcome, ReplayError> {
    let rows = lookup_deliveries(source, &[event_id])
        .await
        .map_err(source_error)?;
    let row = rows
        .into_iter()
        .find(|row| row.event_id == event_id)
        .ok_or(ReplayError::NotFound)?;
    if row.state != "quarantined" {
        return Err(ReplayError::NotQuarantined(row.state));
    }
    let code = row
        .quarantine_code
        .ok_or_else(|| ReplayError::NotQuarantined("quarantined".into()))?;
    let previous_code = BoundedCode::new(&code).ok_or(ReplayError::InvalidCode)?;
    let receipt = store
        .record_relay_control(&RelayControl::from(RelayControlKind::ReplayRequested {
            event_id,
            previous_code,
        }))
        .await
        .map_err(ReplayError::Store)?;
    let done: bool = sqlx::query_scalar("SELECT audit_relay.replay($1, $2, $3)")
        .bind(event_id)
        .bind(receipt.seq)
        .bind(receipt.recovery_epoch)
        .fetch_one(source)
        .await
        .map_err(source_error)?;
    if !done {
        return Err(ReplayError::Refused {
            control_seq: receipt.seq,
        });
    }
    Ok(ReplayOutcome {
        event_id,
        previous_quarantine_code: code,
        control_seq: receipt.seq,
        control_epoch: receipt.recovery_epoch,
    })
}
