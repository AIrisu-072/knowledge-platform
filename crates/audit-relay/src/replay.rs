//! `audit-relay replay --event-id` (design §6.4).
//!
//! 1. Record `audit.delivery.replay_requested` in the Store with the
//!    operator's own Store login and take its seq; abort if that fails.
//! 2. `audit_relay.replay(event_id, control_seq, control_epoch)` in one
//!    transaction: history row, attempt budget reset, back to pending.
//!
//! The Document function cannot verify the Store fact; reconcile reports a
//! history row whose control seq does not resolve as `unaudited_replay`.

use audit_store_postgres::AdminError;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::reconcile::{REPLAY_TYPE, lookup_deliveries};
use crate::store_admin::StoreAdmin;

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
    #[error("audit store refused or failed to record the replay: {0}")]
    Store(AdminError),
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
    admin: &dyn StoreAdmin,
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
    let control_seq = admin
        .record_relay_control(REPLAY_TYPE, event_id, &code, None)
        .await
        .map_err(ReplayError::Store)?;
    let control_epoch = admin
        .store_status()
        .await
        .map_err(ReplayError::Store)?
        .recovery_epoch;
    let done: bool = sqlx::query_scalar("SELECT audit_relay.replay($1, $2, $3)")
        .bind(event_id)
        .bind(control_seq)
        .bind(control_epoch)
        .fetch_one(source)
        .await
        .map_err(source_error)?;
    if !done {
        return Err(ReplayError::Refused { control_seq });
    }
    Ok(ReplayOutcome {
        event_id,
        previous_quarantine_code: code,
        control_seq,
        control_epoch,
    })
}
