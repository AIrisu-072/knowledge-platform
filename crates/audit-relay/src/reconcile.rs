//! Reconciliation of relay deliveries against Store receipts (design §12).
//!
//! Reads are content-free on both sides: `audit_relay.reconcile_page`,
//! `lookup_deliveries` and `reconcile_history_page` on the Document
//! database; `lookup_receipts`, `list_source_receipts`,
//! `lookup_control_receipts`, `lookup_lost_ranges` and `store_status` on the
//! Store (reconciler role). Every run records exactly one
//! `audit.reconciliation.completed` (`audit_core::ReconcileCounts`).
//! `--repair` records it first and then applies only the allowed
//! transitions:
//! - delivered_missing → pending (history kept),
//! - quarantined_stored → acked with the Store receipt (SQL fence:
//!   `delivery_unknown_at_limit` only, commitment recomputed server-side),
//! - unregistered → registered (`registration_kind = 'repair'`).
//!
//! Nothing else is touched: conflict, source/actor mismatch and validation
//! quarantines, store_only, digest_mismatch and quarantined_conflict stay as
//! they are.
//!
//! History (replay and repair transitions) must resolve to its Store control
//! event: a replay row to an `audit.delivery.replay_requested` of the same
//! event, recovery epoch and quarantine code, claimed by no other row (one
//! to one); a repair row to an `audit.reconciliation.completed` run of mode
//! `repair` in the same epoch. A row that does not resolve is
//! `replay_record_lost` when its control seq lies in a lost range the Store
//! declared for that epoch (`lookup_lost_ranges`), otherwise
//! `unaudited_replay`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use audit_core::catalog::DOCUMENT_SOURCE;
use audit_core::{
    BoundedCode, ControlReceiptRow, Origin, ReceiptRow, ReconcileCounts, ReconcileMode,
    RelayControl, RelayControlKind, StoreError,
};
use serde::{Serialize, Serializer};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::handler::CATALOG_SKEW;
use crate::store::{LostRange, RelayStore};

pub const RECONCILIATION_TYPE: &str = "audit.reconciliation.completed";
pub const REPLAY_TYPE: &str = "audit.delivery.replay_requested";
pub const UNKNOWN_AT_LIMIT: &str = "delivery_unknown_at_limit";

const PAGE: i32 = 500;
const STORE_PAGE: u32 = 1000;

/// One content-free delivery row (`audit_relay.delivery_view`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryRow {
    pub event_id: Uuid,
    pub state: String,
    pub registration_kind: Option<String>,
    pub delivered_at: Option<OffsetDateTime>,
    pub store_seq: Option<i64>,
    pub store_envelope_digest: Option<Vec<u8>>,
    pub store_outcome: Option<String>,
    pub store_recovery_epoch: Option<i64>,
    pub quarantine_code: Option<String>,
    pub source_commitment: Option<Vec<u8>>,
    pub source_intact: Option<bool>,
    /// The bounded code of the last failed attempt (`relay_catalog_skew`
    /// marks a row held because the relay's catalog cannot project it).
    pub last_error_code: Option<String>,
}

fn delivery_row(row: &PgRow) -> Result<DeliveryRow, sqlx::Error> {
    Ok(DeliveryRow {
        event_id: row.try_get("event_id")?,
        state: row.try_get("state")?,
        registration_kind: row.try_get("registration_kind")?,
        delivered_at: row.try_get("delivered_at")?,
        store_seq: row.try_get("store_seq")?,
        store_envelope_digest: row.try_get("store_envelope_digest")?,
        store_outcome: row.try_get("store_outcome")?,
        store_recovery_epoch: row.try_get("store_recovery_epoch")?,
        quarantine_code: row.try_get("quarantine_code")?,
        source_commitment: row.try_get("source_commitment")?,
        source_intact: row.try_get("source_intact")?,
        last_error_code: row.try_get("last_error_code")?,
    })
}

pub async fn reconcile_page(
    source: &PgPool,
    after: Option<Uuid>,
    limit: i32,
) -> Result<Vec<DeliveryRow>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM audit_relay.reconcile_page($1, $2)")
        .bind(after)
        .bind(limit)
        .fetch_all(source)
        .await?;
    rows.iter().map(delivery_row).collect()
}

pub async fn lookup_deliveries(
    source: &PgPool,
    event_ids: &[Uuid],
) -> Result<Vec<DeliveryRow>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM audit_relay.lookup_deliveries($1)")
        .bind(event_ids)
        .fetch_all(source)
        .await?;
    rows.iter().map(delivery_row).collect()
}

/// One `audit_relay.delivery_history` row (content-free).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub history_id: i64,
    pub event_id: Uuid,
    pub transition: String,
    pub control_seq: Option<i64>,
    pub control_epoch: i64,
    pub reconcile_seq: Option<i64>,
    /// The quarantine code before the transition (for a replay: the code the
    /// replay cleared).
    pub quarantine_code: Option<String>,
}

pub async fn history_page(
    source: &PgPool,
    after: i64,
    limit: i32,
) -> Result<Vec<HistoryRow>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM audit_relay.reconcile_history_page($1, $2)")
        .bind(after)
        .bind(limit)
        .fetch_all(source)
        .await?;
    rows.iter()
        .map(|row| {
            Ok(HistoryRow {
                history_id: row.try_get("history_id")?,
                event_id: row.try_get("event_id")?,
                transition: row.try_get("transition")?,
                control_seq: row.try_get("control_seq")?,
                control_epoch: row.try_get("control_epoch")?,
                reconcile_seq: row.try_get("reconcile_seq")?,
                quarantine_code: row.try_get("quarantine_code")?,
            })
        })
        .collect()
}

/// Reconciliation classes of one delivery row (design §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Ok,
    DeliveredMissing,
    DigestMismatch,
    QuarantinedStored,
    QuarantinedConflict,
    Pending,
    Quarantined,
    Unregistered,
    SourceTampered,
    /// Undelivered and held because the relay's catalog cannot project it
    /// (a newer producer type or key, design §6.3).
    RelayCatalogSkew,
}

/// Classifies one delivery row against its Store receipt (origin relay).
pub fn classify(row: &DeliveryRow, receipt: Option<&ReceiptRow>) -> Class {
    if row.state == "unregistered" {
        return Class::Unregistered;
    }
    if row.source_intact == Some(false) {
        return Class::SourceTampered;
    }
    let commitment_matches = |r: &ReceiptRow| {
        r.source_commitment.map(|c| c.to_vec()) == row.source_commitment
            && r.source_commitment.is_some()
    };
    match row.state.as_str() {
        "delivered" => match receipt {
            None => Class::DeliveredMissing,
            Some(r)
                if Some(r.seq) == row.store_seq
                    && row.store_envelope_digest.as_deref() == Some(&r.envelope_digest[..])
                    && commitment_matches(r) =>
            {
                Class::Ok
            }
            Some(_) => Class::DigestMismatch,
        },
        "quarantined" => match receipt {
            Some(r) if !commitment_matches(r) => Class::QuarantinedConflict,
            Some(_) if row.quarantine_code.as_deref() == Some(UNKNOWN_AT_LIMIT) => {
                Class::QuarantinedStored
            }
            _ => Class::Quarantined,
        },
        _ if row.last_error_code.as_deref() == Some(CATALOG_SKEW) => Class::RelayCatalogSkew,
        _ => Class::Pending,
    }
}

fn add(counts: &mut ReconcileCounts, class: Class) {
    let counter = match class {
        Class::Ok => &mut counts.ok,
        Class::DeliveredMissing => &mut counts.delivered_missing,
        Class::DigestMismatch => &mut counts.digest_mismatch,
        Class::QuarantinedStored => &mut counts.quarantined_stored,
        Class::QuarantinedConflict => &mut counts.quarantined_conflict,
        Class::Pending => &mut counts.pending,
        Class::Quarantined => &mut counts.quarantined,
        Class::Unregistered => &mut counts.unregistered,
        Class::SourceTampered => &mut counts.source_tampered,
        Class::RelayCatalogSkew => &mut counts.relay_catalog_skew,
    };
    *counter += 1;
}

/// Alarm codes of a classification (design §12): fixed labels only.
pub fn alarms(counts: &ReconcileCounts) -> Vec<&'static str> {
    [
        (counts.unaudited_replay, "unaudited_replay"),
        (counts.delivered_missing, "delivered_missing"),
        (counts.digest_mismatch, "digest_mismatch"),
        (counts.store_only, "store_only"),
        (counts.source_tampered, "source_tampered"),
        (counts.quarantined_conflict, "quarantined_conflict"),
        (counts.replay_record_lost, "replay_record_lost"),
        (counts.unregistered, "unregistered"),
        (counts.relay_catalog_skew, "relay_catalog_skew"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(_, code)| code)
    .collect()
}

/// The counts as JSON (report output; the same field names as the struct).
pub fn counts_json(counts: &ReconcileCounts) -> serde_json::Value {
    json!({
        "ok": counts.ok,
        "delivered_missing": counts.delivered_missing,
        "digest_mismatch": counts.digest_mismatch,
        "quarantined_stored": counts.quarantined_stored,
        "quarantined_conflict": counts.quarantined_conflict,
        "pending": counts.pending,
        "quarantined": counts.quarantined,
        "unregistered": counts.unregistered,
        "source_tampered": counts.source_tampered,
        "store_only": counts.store_only,
        "unaudited_replay": counts.unaudited_replay,
        "replay_record_lost": counts.replay_record_lost,
        "relay_catalog_skew": counts.relay_catalog_skew,
        "repaired_delivered_missing": counts.repaired_delivered_missing,
        "repaired_quarantined_stored": counts.repaired_quarantined_stored,
        "repaired_unregistered": counts.repaired_unregistered,
    })
}

fn serialize_counts<S: Serializer>(counts: &ReconcileCounts, out: S) -> Result<S::Ok, S::Error> {
    counts_json(counts).serialize(out)
}

/// How one history row relates to the Store's control events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryVerdict {
    Resolved,
    /// The control event was in a range the Store declared lost.
    RecordLost,
    Unaudited,
}

/// Resolves one history row (pure; `claimed` holds the replay control seqs
/// already claimed by earlier rows of this run).
pub fn resolve_history(
    row: &HistoryRow,
    controls: &HashMap<i64, ControlReceiptRow>,
    lost: &[LostRange],
    claimed: &mut HashSet<i64>,
) -> HistoryVerdict {
    let (seq, expected) = match row.transition.as_str() {
        "replay" => (row.control_seq, REPLAY_TYPE),
        _ => (row.reconcile_seq, RECONCILIATION_TYPE),
    };
    let Some(seq) = seq else {
        return HistoryVerdict::Unaudited;
    };
    let resolved = controls.get(&seq).is_some_and(|control| {
        let code = control.code.as_ref().map(BoundedCode::as_str);
        control.origin == Origin::RelayControl
            && control.event_type.as_str() == expected
            && control.recovery_epoch == row.control_epoch
            && if expected == REPLAY_TYPE {
                control.target_event_id == Some(row.event_id)
                    && code.is_some()
                    && code == row.quarantine_code.as_deref()
            } else {
                code == Some(ReconcileMode::Repair.as_str())
            }
    });
    if resolved && (expected != REPLAY_TYPE || claimed.insert(seq)) {
        return HistoryVerdict::Resolved;
    }
    if lost
        .iter()
        .any(|range| range.contains(row.control_epoch, seq))
    {
        HistoryVerdict::RecordLost
    } else {
        HistoryVerdict::Unaudited
    }
}

/// Repairs planned (recorded in the control event) and applied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Repairs {
    pub delivered_missing: u64,
    pub quarantined_stored: u64,
    pub unregistered: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconcileReport {
    pub run_id: Uuid,
    pub mode: &'static str,
    pub watermark: i64,
    pub store_epoch: i64,
    #[serde(serialize_with = "serialize_counts")]
    pub counts: ReconcileCounts,
    pub id_set_digest: String,
    pub planned: Repairs,
    pub applied: Repairs,
    pub control_seq: Option<i64>,
    pub control_epoch: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReconcileError {
    #[error("source database error ({0})")]
    Source(String),
    #[error("audit store call failed: {0}")]
    Store(StoreError),
}

fn source_error(error: sqlx::Error) -> ReconcileError {
    ReconcileError::Source(match &error {
        sqlx::Error::Database(db) => db.code().map(|c| c.into_owned()).unwrap_or_default(),
        _ => "unavailable".to_owned(),
    })
}

enum Repair {
    ResetMissing {
        event_id: Uuid,
        store_seq: i64,
    },
    AckStored {
        event_id: Uuid,
        receipt: ReceiptRow,
        commitment: Vec<u8>,
    },
}

/// The classification of one pass, before anything is recorded.
pub struct Classification {
    pub watermark: i64,
    pub store_epoch: i64,
    pub counts: ReconcileCounts,
    pub id_set_digest: [u8; 32],
    repairs: Vec<Repair>,
}

/// Reconciles one Document source against the Store.
pub struct Reconciler {
    source: PgPool,
    store: Arc<dyn RelayStore>,
}

impl Reconciler {
    pub fn new(source: PgPool, store: Arc<dyn RelayStore>) -> Self {
        Self { source, store }
    }

    /// Classifies without recording anything (health uses this).
    pub async fn classify(&self) -> Result<Classification, ReconcileError> {
        let status = self
            .store
            .store_status()
            .await
            .map_err(ReconcileError::Store)?;
        let watermark = status.head_seq;
        let store_epoch = status.recovery_epoch;
        let mut counts = ReconcileCounts::default();
        let mut hasher = Sha256::new();
        let mut repairs = Vec::new();

        let mut after = None;
        loop {
            let rows = reconcile_page(&self.source, after, PAGE)
                .await
                .map_err(source_error)?;
            let Some(last) = rows.last() else {
                break;
            };
            after = Some(last.event_id);
            let ids: Vec<Uuid> = rows
                .iter()
                .filter(|row| row.state == "delivered" || row.state == "quarantined")
                .map(|row| row.event_id)
                .collect();
            let receipts: HashMap<Uuid, ReceiptRow> = if ids.is_empty() {
                HashMap::new()
            } else {
                self.store
                    .lookup_receipts(&ids)
                    .await
                    .map_err(ReconcileError::Store)?
                    .into_iter()
                    .filter(|r| r.origin == Origin::Relay)
                    .map(|r| (r.event_id, r))
                    .collect()
            };
            for row in &rows {
                hasher.update(row.event_id.as_bytes());
                let receipt = receipts.get(&row.event_id);
                let class = classify(row, receipt);
                add(&mut counts, class);
                match (class, receipt) {
                    (Class::DeliveredMissing, _) => {
                        if let Some(store_seq) = row.store_seq {
                            repairs.push(Repair::ResetMissing {
                                event_id: row.event_id,
                                store_seq,
                            });
                        }
                    }
                    (Class::QuarantinedStored, Some(receipt)) => {
                        if let Some(commitment) = &row.source_commitment {
                            repairs.push(Repair::AckStored {
                                event_id: row.event_id,
                                receipt: receipt.clone(),
                                commitment: commitment.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        // store_only: Store receipts of this source up to the watermark that
        // the source does not hold at all.
        let mut after_seq = 0;
        loop {
            let page = self
                .store
                .list_source_receipts(DOCUMENT_SOURCE, after_seq, STORE_PAGE)
                .await
                .map_err(ReconcileError::Store)?;
            let Some(last) = page.last() else {
                break;
            };
            after_seq = last.seq;
            let ids: Vec<Uuid> = page
                .iter()
                .filter(|r| r.seq <= watermark)
                .map(|r| r.event_id)
                .collect();
            if !ids.is_empty() {
                let known: HashSet<Uuid> = lookup_deliveries(&self.source, &ids)
                    .await
                    .map_err(source_error)?
                    .into_iter()
                    .map(|row| row.event_id)
                    .collect();
                counts.store_only += ids.iter().filter(|id| !known.contains(id)).count() as u64;
            }
            if after_seq >= watermark {
                break;
            }
        }

        // Replay/repair history must resolve to the matching Store control
        // event; unresolved rows inside a declared lost range are losses.
        let lost = self
            .store
            .lookup_lost_ranges()
            .await
            .map_err(ReconcileError::Store)?;
        let mut claimed = HashSet::new();
        let mut after_history = 0;
        loop {
            let history = history_page(&self.source, after_history, PAGE)
                .await
                .map_err(source_error)?;
            let Some(last) = history.last() else {
                break;
            };
            after_history = last.history_id;
            let seqs: Vec<i64> = history
                .iter()
                .filter_map(|h| h.control_seq.or(h.reconcile_seq))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let controls: HashMap<i64, ControlReceiptRow> = if seqs.is_empty() {
                HashMap::new()
            } else {
                self.store
                    .lookup_control_receipts(&seqs)
                    .await
                    .map_err(ReconcileError::Store)?
                    .into_iter()
                    .map(|c| (c.seq, c))
                    .collect()
            };
            for h in &history {
                match resolve_history(h, &controls, &lost, &mut claimed) {
                    HistoryVerdict::Resolved => {}
                    HistoryVerdict::RecordLost => counts.replay_record_lost += 1,
                    HistoryVerdict::Unaudited => counts.unaudited_replay += 1,
                }
            }
        }

        Ok(Classification {
            watermark,
            store_epoch,
            counts,
            id_set_digest: hasher.finalize().into(),
            repairs,
        })
    }

    /// One reconciliation run; records `audit.reconciliation.completed`.
    pub async fn run(&self, repair: bool) -> Result<ReconcileReport, ReconcileError> {
        let classification = self.classify().await?;
        let mut counts = classification.counts;
        let mut planned = Repairs::default();
        if repair {
            planned.delivered_missing = classification
                .repairs
                .iter()
                .filter(|r| matches!(r, Repair::ResetMissing { .. }))
                .count() as u64;
            planned.quarantined_stored = classification
                .repairs
                .iter()
                .filter(|r| matches!(r, Repair::AckStored { .. }))
                .count() as u64;
            planned.unregistered = counts.unregistered;
        }
        counts.repaired_delivered_missing = planned.delivered_missing;
        counts.repaired_quarantined_stored = planned.quarantined_stored;
        counts.repaired_unregistered = planned.unregistered;
        let run_id = Uuid::now_v7();
        let mode = if repair {
            ReconcileMode::Repair
        } else {
            ReconcileMode::ReadOnly
        };
        let receipt = self
            .store
            .record_relay_control(&RelayControl::from(
                RelayControlKind::ReconciliationCompleted {
                    run_id,
                    mode,
                    watermark: classification.watermark,
                    id_set_digest: classification.id_set_digest,
                    counts,
                },
            ))
            .await
            .map_err(ReconcileError::Store)?;
        let (seq, epoch) = (receipt.seq, receipt.recovery_epoch);
        let mut applied = Repairs::default();
        if repair {
            for item in &classification.repairs {
                match item {
                    Repair::ResetMissing {
                        event_id,
                        store_seq,
                    } => {
                        let done: bool = sqlx::query_scalar(
                            "SELECT audit_relay.repair_reset_missing($1, $2, $3, $4)",
                        )
                        .bind(event_id)
                        .bind(store_seq)
                        .bind(seq)
                        .bind(epoch)
                        .fetch_one(&self.source)
                        .await
                        .map_err(source_error)?;
                        applied.delivered_missing += u64::from(done);
                    }
                    Repair::AckStored {
                        event_id,
                        receipt,
                        commitment,
                    } => {
                        let outcome = if receipt.expired {
                            "duplicate_expired"
                        } else {
                            "duplicate"
                        };
                        let done: bool = sqlx::query_scalar(
                            "SELECT audit_relay.repair_ack_stored($1, $2, $3, $4, $5, $6, $7)",
                        )
                        .bind(event_id)
                        .bind(receipt.seq)
                        .bind(receipt.envelope_digest.to_vec())
                        .bind(commitment)
                        .bind(outcome)
                        .bind(seq)
                        // The epoch of the run's control event: the history
                        // reference and the ack's observation epoch.
                        .bind(epoch)
                        .fetch_one(&self.source)
                        .await
                        .map_err(source_error)?;
                        applied.quarantined_stored += u64::from(done);
                    }
                }
            }
            let mut remaining = planned.unregistered;
            while remaining > 0 {
                let registered: i64 = sqlx::query_scalar("SELECT audit_relay.register_missing($1)")
                    .bind(i32::try_from(remaining.min(1000)).unwrap_or(1000))
                    .fetch_one(&self.source)
                    .await
                    .map_err(source_error)?;
                let registered = u64::try_from(registered).unwrap_or(0);
                if registered == 0 {
                    break;
                }
                applied.unregistered += registered;
                remaining = remaining.saturating_sub(registered);
            }
        }
        Ok(ReconcileReport {
            run_id,
            mode: mode.as_str(),
            watermark: classification.watermark,
            store_epoch: classification.store_epoch,
            counts,
            id_set_digest: audit_core::chain::to_hex(&classification.id_set_digest),
            planned,
            applied,
            control_seq: Some(seq),
            control_epoch: Some(epoch),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audit_core::EventTypeName;

    fn row(state: &str) -> DeliveryRow {
        DeliveryRow {
            event_id: Uuid::from_u128(1),
            state: state.to_owned(),
            registration_kind: Some("trigger".into()),
            delivered_at: None,
            store_seq: Some(5),
            store_envelope_digest: Some(vec![1; 32]),
            store_outcome: Some("stored".into()),
            store_recovery_epoch: Some(1),
            quarantine_code: None,
            source_commitment: Some(vec![2; 32]),
            source_intact: Some(true),
            last_error_code: None,
        }
    }

    fn receipt(seq: i64, digest: u8, commitment: u8) -> ReceiptRow {
        ReceiptRow {
            event_id: Uuid::from_u128(1),
            seq,
            origin: Origin::Relay,
            event_type: EventTypeName::new("document.created").expect("type"),
            envelope_digest: [digest; 32],
            source_commitment: Some([commitment; 32]),
            expired: false,
            recovery_epoch: 1,
        }
    }

    #[test]
    fn classes_follow_design_12() {
        let delivered = row("delivered");
        assert_eq!(classify(&delivered, Some(&receipt(5, 1, 2))), Class::Ok);
        assert_eq!(classify(&delivered, None), Class::DeliveredMissing);
        assert_eq!(
            classify(&delivered, Some(&receipt(6, 1, 2))),
            Class::DigestMismatch
        );
        assert_eq!(
            classify(&delivered, Some(&receipt(5, 1, 3))),
            Class::DigestMismatch,
            "commitment is compared too"
        );
        let mut quarantined = row("quarantined");
        quarantined.quarantine_code = Some(UNKNOWN_AT_LIMIT.into());
        assert_eq!(
            classify(&quarantined, Some(&receipt(5, 1, 2))),
            Class::QuarantinedStored
        );
        assert_eq!(
            classify(&quarantined, Some(&receipt(5, 1, 9))),
            Class::QuarantinedConflict
        );
        quarantined.quarantine_code = Some("conflict".into());
        assert_eq!(
            classify(&quarantined, Some(&receipt(5, 1, 2))),
            Class::Quarantined,
            "only delivery_unknown_at_limit is repairable"
        );
        assert_eq!(classify(&row("pending"), None), Class::Pending);
        let mut held = row("pending");
        held.last_error_code = Some(CATALOG_SKEW.into());
        assert_eq!(classify(&held, None), Class::RelayCatalogSkew);
        let mut leased = row("leased");
        leased.last_error_code = Some(CATALOG_SKEW.into());
        assert_eq!(classify(&leased, None), Class::RelayCatalogSkew);
        let mut outage = row("pending");
        outage.last_error_code = Some("store_connection".into());
        assert_eq!(classify(&outage, None), Class::Pending);
        assert_eq!(classify(&row("unregistered"), None), Class::Unregistered);
        let mut tampered = row("delivered");
        tampered.source_intact = Some(false);
        assert_eq!(
            classify(&tampered, Some(&receipt(5, 1, 2))),
            Class::SourceTampered
        );
    }

    fn replay_row(history_id: i64, event: u128, seq: i64, epoch: i64, code: &str) -> HistoryRow {
        HistoryRow {
            history_id,
            event_id: Uuid::from_u128(event),
            transition: "replay".into(),
            control_seq: Some(seq),
            control_epoch: epoch,
            reconcile_seq: None,
            quarantine_code: Some(code.into()),
        }
    }

    fn replay_control(seq: i64, epoch: i64, event: u128, code: &str) -> ControlReceiptRow {
        ControlReceiptRow {
            seq,
            recovery_epoch: epoch,
            origin: Origin::RelayControl,
            event_type: EventTypeName::new(REPLAY_TYPE).expect("type"),
            target_event_id: Some(Uuid::from_u128(event)),
            code: BoundedCode::new(code),
        }
    }

    fn lost_range(old_epoch: i64, from: i64, upper: i64) -> LostRange {
        LostRange {
            epoch_seq: upper + 1,
            old_epoch,
            new_epoch: old_epoch + 1,
            classification: "restore".into(),
            restored_head_seq: from - 1,
            lost_from_seq: from,
            lost_upper_seq: upper,
            lost_upper_known: true,
        }
    }

    #[test]
    fn replay_history_resolves_one_to_one_with_the_same_code_and_epoch() {
        let controls: HashMap<i64, ControlReceiptRow> = [
            replay_control(10, 1, 1, "conflict"),
            replay_control(11, 1, 2, "rejected_invalid_provenance"),
            replay_control(12, 2, 3, "conflict"),
        ]
        .into_iter()
        .map(|c| (c.seq, c))
        .collect();
        let lost = [lost_range(1, 20, 30)];
        let mut claimed = HashSet::new();
        let verdict = |row: &HistoryRow, claimed: &mut HashSet<i64>| {
            resolve_history(row, &controls, &lost, claimed)
        };
        assert_eq!(
            verdict(&replay_row(1, 1, 10, 1, "conflict"), &mut claimed),
            HistoryVerdict::Resolved
        );
        // The same control event claimed by a second row is unaudited.
        assert_eq!(
            verdict(&replay_row(2, 1, 10, 1, "conflict"), &mut claimed),
            HistoryVerdict::Unaudited
        );
        // Another event's control event, a different code, another epoch.
        assert_eq!(
            verdict(
                &replay_row(3, 9, 11, 1, "rejected_invalid_provenance"),
                &mut claimed
            ),
            HistoryVerdict::Unaudited
        );
        assert_eq!(
            verdict(&replay_row(4, 2, 11, 1, "conflict"), &mut claimed),
            HistoryVerdict::Unaudited
        );
        assert_eq!(
            verdict(&replay_row(5, 3, 12, 1, "conflict"), &mut claimed),
            HistoryVerdict::Unaudited
        );
        assert_eq!(
            verdict(&replay_row(6, 3, 12, 2, "conflict"), &mut claimed),
            HistoryVerdict::Resolved
        );
        // A missing control event inside a declared lost range of its epoch
        // is a recorded loss; outside it, or in another epoch, unaudited.
        assert_eq!(
            verdict(&replay_row(7, 4, 25, 1, "conflict"), &mut claimed),
            HistoryVerdict::RecordLost
        );
        assert_eq!(
            verdict(&replay_row(8, 4, 31, 1, "conflict"), &mut claimed),
            HistoryVerdict::Unaudited
        );
        assert_eq!(
            verdict(&replay_row(9, 4, 25, 2, "conflict"), &mut claimed),
            HistoryVerdict::Unaudited
        );
    }

    #[test]
    fn repair_history_resolves_to_a_repair_run_of_its_epoch() {
        let run = ControlReceiptRow {
            seq: 40,
            recovery_epoch: 1,
            origin: Origin::RelayControl,
            event_type: EventTypeName::new(RECONCILIATION_TYPE).expect("type"),
            target_event_id: None,
            code: BoundedCode::new("repair"),
        };
        let read_only = ControlReceiptRow {
            seq: 41,
            code: BoundedCode::new("read_only"),
            ..run.clone()
        };
        let controls: HashMap<i64, ControlReceiptRow> =
            [(40, run), (41, read_only)].into_iter().collect();
        let repair = |history_id, seq| HistoryRow {
            history_id,
            event_id: Uuid::from_u128(history_id as u128),
            transition: "repair_reset_missing".into(),
            control_seq: None,
            control_epoch: 1,
            reconcile_seq: Some(seq),
            quarantine_code: None,
        };
        let mut claimed = HashSet::new();
        for (row, expected) in [
            (repair(1, 40), HistoryVerdict::Resolved),
            (repair(2, 40), HistoryVerdict::Resolved),
            (repair(3, 41), HistoryVerdict::Unaudited),
            (repair(4, 42), HistoryVerdict::Unaudited),
        ] {
            assert_eq!(
                resolve_history(&row, &controls, &[], &mut claimed),
                expected,
                "{row:?}"
            );
        }
    }

    #[test]
    fn alarms_and_report_counts_use_fixed_labels() {
        let counts = ReconcileCounts {
            unaudited_replay: 1,
            replay_record_lost: 2,
            relay_catalog_skew: 3,
            ok: 5,
            ..ReconcileCounts::default()
        };
        assert_eq!(
            alarms(&counts),
            [
                "unaudited_replay",
                "replay_record_lost",
                "relay_catalog_skew"
            ]
        );
        let value = counts_json(&counts);
        assert_eq!(value["replay_record_lost"], 2);
        assert_eq!(value["relay_catalog_skew"], 3);
        // One report key per ReconcileCounts field, and the recorded
        // control event carries every count_<class> / repaired_<class>
        // field of the catalog entry.
        assert_eq!(value.as_object().expect("object").len(), 16);
        let details = RelayControl::from(RelayControlKind::ReconciliationCompleted {
            run_id: Uuid::from_u128(1),
            mode: ReconcileMode::ReadOnly,
            watermark: 0,
            id_set_digest: [0; 32],
            counts,
        })
        .details();
        for (key, count) in value.as_object().expect("object") {
            let field = if key.starts_with("repaired_") {
                key.clone()
            } else {
                format!("count_{key}")
            };
            assert_eq!(details.get(&field), Some(count), "{field}");
        }
    }
}
