//! Reconciliation of relay deliveries against Store receipts (design §12).
//!
//! Reads are content-free on both sides: `audit_relay.reconcile_page`,
//! `lookup_deliveries` and `reconcile_history_page` on the Document
//! database; `lookup_receipts`, `list_source_receipts` and
//! `lookup_control_receipts` on the Store. Every run records exactly one
//! `audit.reconciliation.completed`. `--repair` records it first and then
//! applies only the allowed transitions:
//! - delivered_missing → pending (history kept),
//! - quarantined_stored → acked with the Store receipt (SQL fence:
//!   `delivery_unknown_at_limit` only, commitment recomputed server-side),
//! - unregistered → registered (`registration_kind = 'repair'`).
//!
//! Nothing else is touched: conflict, source/actor mismatch and validation
//! quarantines, store_only, digest_mismatch and quarantined_conflict stay as
//! they are.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use audit_core::catalog::DOCUMENT_SOURCE;
use audit_store_postgres::AdminError;
use audit_store_postgres::admin::{ControlReceipt, Receipt};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::store_admin::StoreAdmin;

pub const RECONCILIATION_TYPE: &str = "audit.reconciliation.completed";
pub const REPLAY_TYPE: &str = "audit.delivery.replay_requested";
pub const UNKNOWN_AT_LIMIT: &str = "delivery_unknown_at_limit";

const PAGE: i32 = 500;
const STORE_PAGE: i32 = 1000;

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

/// Reconciliation classes (design §12), plus `replay_record_lost`: a
/// history record whose Store control event belongs to an earlier recovery
/// epoch and is gone (a declared loss, not an unaudited transition).
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
}

/// Classifies one delivery row against its Store receipt (origin relay).
pub fn classify(row: &DeliveryRow, receipt: Option<&Receipt>) -> Class {
    if row.state == "unregistered" {
        return Class::Unregistered;
    }
    if row.source_intact == Some(false) {
        return Class::SourceTampered;
    }
    let commitment_matches = |r: &Receipt| {
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
        _ => Class::Pending,
    }
}

/// Class counts of one run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub ok: i64,
    pub delivered_missing: i64,
    pub digest_mismatch: i64,
    pub quarantined_stored: i64,
    pub quarantined_conflict: i64,
    pub pending: i64,
    pub quarantined: i64,
    pub unregistered: i64,
    pub source_tampered: i64,
    pub store_only: i64,
    pub unaudited_replay: i64,
    pub replay_record_lost: i64,
}

impl Counts {
    fn add(&mut self, class: Class) {
        match class {
            Class::Ok => self.ok += 1,
            Class::DeliveredMissing => self.delivered_missing += 1,
            Class::DigestMismatch => self.digest_mismatch += 1,
            Class::QuarantinedStored => self.quarantined_stored += 1,
            Class::QuarantinedConflict => self.quarantined_conflict += 1,
            Class::Pending => self.pending += 1,
            Class::Quarantined => self.quarantined += 1,
            Class::Unregistered => self.unregistered += 1,
            Class::SourceTampered => self.source_tampered += 1,
        }
    }

    /// Alarm codes (design §12): fixed labels only.
    pub fn alarms(&self) -> Vec<&'static str> {
        let mut alarms = Vec::new();
        for (count, code) in [
            (self.unaudited_replay, "unaudited_replay"),
            (self.delivered_missing, "delivered_missing"),
            (self.digest_mismatch, "digest_mismatch"),
            (self.store_only, "store_only"),
            (self.source_tampered, "source_tampered"),
            (self.quarantined_conflict, "quarantined_conflict"),
            (self.replay_record_lost, "replay_record_lost"),
            (self.unregistered, "unregistered"),
        ] {
            if count > 0 {
                alarms.push(code);
            }
        }
        alarms
    }
}

/// Repairs planned (recorded in the control event) and applied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Repairs {
    pub delivered_missing: i64,
    pub quarantined_stored: i64,
    pub unregistered: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconcileReport {
    pub run_id: Uuid,
    pub mode: &'static str,
    pub watermark: i64,
    pub store_epoch: i64,
    pub counts: Counts,
    pub id_set_digest: String,
    pub planned: Repairs,
    pub applied: Repairs,
    pub control_seq: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReconcileError {
    #[error("source database error ({0})")]
    Source(String),
    #[error("audit store call failed: {0}")]
    Store(AdminError),
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
        receipt: Receipt,
        commitment: Vec<u8>,
    },
}

/// The classification of one pass, before anything is recorded.
pub struct Classification {
    pub watermark: i64,
    pub store_epoch: i64,
    pub counts: Counts,
    pub id_set_digest: String,
    repairs: Vec<Repair>,
}

/// Reconciles one Document source against the Store.
pub struct Reconciler {
    source: PgPool,
    admin: Arc<dyn StoreAdmin>,
}

impl Reconciler {
    pub fn new(source: PgPool, admin: Arc<dyn StoreAdmin>) -> Self {
        Self { source, admin }
    }

    /// Classifies without recording anything (health uses this).
    pub async fn classify(&self) -> Result<Classification, ReconcileError> {
        let status = self
            .admin
            .store_status()
            .await
            .map_err(ReconcileError::Store)?;
        let watermark = status.head_seq;
        let store_epoch = status.recovery_epoch;
        let mut counts = Counts::default();
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
            let receipts: HashMap<Uuid, Receipt> = if ids.is_empty() {
                HashMap::new()
            } else {
                self.admin
                    .lookup_receipts(&ids)
                    .await
                    .map_err(ReconcileError::Store)?
                    .into_iter()
                    .filter(|r| r.origin == "relay")
                    .map(|r| (r.event_id, r))
                    .collect()
            };
            for row in &rows {
                hasher.update(row.event_id.as_bytes());
                let receipt = receipts.get(&row.event_id);
                let class = classify(row, receipt);
                counts.add(class);
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
        'store: loop {
            let page = self
                .admin
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
                counts.store_only += ids.iter().filter(|id| !known.contains(id)).count() as i64;
            }
            if after_seq >= watermark {
                break 'store;
            }
        }

        // Replay/repair history must resolve to the matching Store control
        // event (one to one: control_seq is unique in the history).
        let mut after_history = 0;
        loop {
            let history = history_page(&self.source, after_history, STORE_PAGE)
                .await
                .map_err(source_error)?;
            let Some(last) = history.last() else {
                break;
            };
            after_history = last.history_id;
            let seqs: Vec<i64> = history
                .iter()
                .filter_map(|h| h.control_seq.or(h.reconcile_seq))
                .collect();
            let controls: HashMap<i64, ControlReceipt> = self
                .admin
                .lookup_control_receipts(&seqs)
                .await
                .map_err(ReconcileError::Store)?
                .into_iter()
                .map(|c| (c.seq, c))
                .collect();
            for h in &history {
                let (seq, expected) = match h.transition.as_str() {
                    "replay" => (h.control_seq, REPLAY_TYPE),
                    _ => (h.reconcile_seq, RECONCILIATION_TYPE),
                };
                let resolved = seq.and_then(|seq| controls.get(&seq)).is_some_and(|c| {
                    c.origin == "relay_control"
                        && c.event_type == expected
                        && (expected != REPLAY_TYPE || c.target_event_id == Some(h.event_id))
                });
                if resolved {
                    continue;
                }
                if h.control_epoch < store_epoch {
                    counts.replay_record_lost += 1;
                } else {
                    counts.unaudited_replay += 1;
                }
            }
        }

        Ok(Classification {
            watermark,
            store_epoch,
            counts,
            id_set_digest: audit_core::chain::to_hex(&hasher.finalize()),
            repairs,
        })
    }

    /// One reconciliation run; records `audit.reconciliation.completed`.
    pub async fn run(&self, repair: bool) -> Result<ReconcileReport, ReconcileError> {
        let classification = self.classify().await?;
        let counts = &classification.counts;
        let mut planned = Repairs::default();
        if repair {
            planned.delivered_missing = classification
                .repairs
                .iter()
                .filter(|r| matches!(r, Repair::ResetMissing { .. }))
                .count() as i64;
            planned.quarantined_stored = classification
                .repairs
                .iter()
                .filter(|r| matches!(r, Repair::AckStored { .. }))
                .count() as i64;
            planned.unregistered = counts.unregistered;
        }
        let run_id = Uuid::now_v7();
        let mode = if repair { "repair" } else { "read_only" };
        let payload = control_counts(
            counts,
            &planned,
            classification.watermark,
            &classification.id_set_digest,
        );
        let seq = self
            .admin
            .record_relay_control(RECONCILIATION_TYPE, run_id, mode, Some(&payload))
            .await
            .map_err(ReconcileError::Store)?;
        let mut applied = Repairs::default();
        if repair {
            let epoch = classification.store_epoch;
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
                        applied.delivered_missing += i64::from(done);
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
                        .bind(epoch)
                        .fetch_one(&self.source)
                        .await
                        .map_err(source_error)?;
                        applied.quarantined_stored += i64::from(done);
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
                if registered == 0 {
                    break;
                }
                applied.unregistered += registered;
                remaining -= registered;
            }
        }
        Ok(ReconcileReport {
            run_id,
            mode,
            watermark: classification.watermark,
            store_epoch: classification.store_epoch,
            counts: classification.counts,
            id_set_digest: classification.id_set_digest,
            planned,
            applied,
            control_seq: Some(seq),
        })
    }
}

/// The counts object accepted by `audit_store.record_relay_control` for
/// `audit.reconciliation.completed` (exactly these keys).
pub fn control_counts(counts: &Counts, planned: &Repairs, watermark: i64, digest: &str) -> Value {
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
        "repaired_delivered_missing": planned.delivered_missing,
        "repaired_quarantined_stored": planned.quarantined_stored,
        "repaired_unregistered": planned.unregistered,
        "watermark": watermark,
        "id_set_digest": digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
    }

    fn receipt(seq: i64, digest: u8, commitment: u8) -> Receipt {
        Receipt {
            event_id: Uuid::from_u128(1),
            seq,
            origin: "relay".into(),
            event_type: "document.created".into(),
            envelope_digest: [digest; 32],
            source_commitment: Some([commitment; 32]),
            adapter_version: 1,
            expired: false,
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
        assert_eq!(classify(&row("unregistered"), None), Class::Unregistered);
        let mut tampered = row("delivered");
        tampered.source_intact = Some(false);
        assert_eq!(
            classify(&tampered, Some(&receipt(5, 1, 2))),
            Class::SourceTampered
        );
    }

    #[test]
    fn control_counts_have_exactly_the_store_keys() {
        let value = control_counts(&Counts::default(), &Repairs::default(), 3, &"0".repeat(64));
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "delivered_missing",
                "digest_mismatch",
                "id_set_digest",
                "ok",
                "pending",
                "quarantined",
                "quarantined_conflict",
                "quarantined_stored",
                "repaired_delivered_missing",
                "repaired_quarantined_stored",
                "repaired_unregistered",
                "source_tampered",
                "store_only",
                "unaudited_replay",
                "unregistered",
                "watermark"
            ]
        );
    }
}
