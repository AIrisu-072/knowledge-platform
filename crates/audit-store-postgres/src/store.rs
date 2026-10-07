//! [`audit_core::AuditStore`] over the Store's SQL functions (design §6.2,
//! §6.3, §7.2, §10.4, §11).
//!
//! Failure model (two-way, design §6.3): verdicts (`rejected:<code>`,
//! `conflict`) arrive as the structured ingest row and are decoded only by
//! [`audit_core::IngestRow::into_result`]. Every other failure — transport,
//! timeout, any SQLSTATE, an unregistered type, recovery mode, an invalid
//! posture, a denied ingest identity or an unexpected/malformed response — is
//! an outage that holds delivery.
//!
//! The row decoders ([`decode_receipt`], [`decode_control_receipt`]) are the
//! only places that build audit-core receipt rows; a malformed row is
//! `Outage{Other}`.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use audit_core::codes::RejectionCode;
use audit_core::port::{BoxFuture, MAX_RECEIPT_LOOKUP};
use audit_core::{
    AuditEnvelope, AuditStore, ControlReceipt, ControlReceiptRow, IngestReceipt, IngestRow, Origin,
    OutageCode, ProbeExpectation, ReceiptIdentity, ReceiptRow, RelayControl, StoreError,
    StoreState, StoreStatus, classify_sqlstate,
};
use serde::Serialize;
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::session::{SessionError, refuse_privileged};

/// Default bound on one Store call; keep it below a third of the relay lease.
pub const DEFAULT_INGEST_TIMEOUT: Duration = Duration::from_secs(8);

/// PostgreSQL Audit Store client (the relay's `AuditStore` port plus the
/// content-free status calls).
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

/// Content-free Store status (`audit_store.store_status()`, design §12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoreStatusRow {
    pub head_seq: i64,
    pub recovery_epoch: i64,
    pub recovery_mode: bool,
    pub posture_ok: bool,
    pub recovery_pending: bool,
    pub recovery_pending_reason: Option<String>,
    pub access_reapply_pending: bool,
    pub last_verified_seq: Option<i64>,
    pub last_verified_at: Option<String>,
    pub last_verified_outcome: Option<String>,
}

/// One declared lost range (`audit_store.lookup_lost_ranges()`): the rows
/// after `restored_head_seq` of `old_epoch` up to `lost_upper_seq` (or
/// unbounded when the upper bound is unknown) did not survive the recovery
/// that started `new_epoch` at `epoch_seq` (design §11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LostRange {
    pub epoch_seq: i64,
    pub old_epoch: i64,
    pub new_epoch: i64,
    pub classification: String,
    pub restored_head_seq: i64,
    pub lost_from_seq: i64,
    pub lost_upper_seq: i64,
    pub lost_upper_known: bool,
}

impl LostRange {
    /// Whether a seq recorded in `epoch` falls inside this declared loss.
    pub fn contains(&self, epoch: i64, seq: i64) -> bool {
        epoch == self.old_epoch
            && seq >= self.lost_from_seq
            && (!self.lost_upper_known || seq <= self.lost_upper_seq)
    }
}

/// `YYYY-MM-DDTHH:MM:SS.ffffffZ` in UTC.
pub fn utc_text(at: OffsetDateTime) -> String {
    let at = at.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        at.microsecond()
    )
}

fn other() -> StoreError {
    StoreError::outage(OutageCode::Other)
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

    async fn bounded<T, F>(&self, call: F) -> Result<T, StoreError>
    where
        F: Future<Output = Result<T, sqlx::Error>>,
    {
        tokio::time::timeout(self.timeout, call)
            .await
            .map_err(|_| StoreError::outage(OutageCode::Timeout))?
            .map_err(|error| classify_sqlx_error(&error))
    }

    async fn call_ingest(&self, envelope: &AuditEnvelope) -> Result<IngestReceipt, StoreError> {
        // A control envelope never reaches the Store: refuse it locally with
        // the Store's own verdict shape (the only constructor of verdicts is
        // IngestRow::into_result).
        if envelope.origin() != Origin::Relay {
            return IngestRow {
                status: "rejected".to_owned(),
                seq: None,
                envelope_digest: None,
                adapter_version: None,
                code: Some(RejectionCode::ControlTypeForbidden.as_str().to_owned()),
            }
            .into_result();
        }
        let text = envelope.to_json_string();
        let row = self
            .bounded(
                sqlx::query(
                    "SELECT status, seq, envelope_digest, adapter_version, code \
                     FROM audit_store.ingest($1::text::jsonb)",
                )
                .bind(text)
                .fetch_one(&self.pool),
            )
            .await?;
        decode_ingest_row(&row)?.into_result()
    }

    async fn call_probe(&self, expected: &ProbeExpectation) -> Result<StoreStatus, StoreError> {
        let last = expected.last_ack;
        let row = self
            .bounded(
                sqlx::query(
                    "SELECT status, state, head_seq, recovery_epoch, missing_types, \
                            regression_detected, last_verified_seq, code \
                     FROM audit_store.probe($1, $2, $3, $4, $5, $6)",
                )
                .bind(&expected.source)
                .bind(expected.adapter_version)
                .bind(&expected.types)
                .bind(last.map(|l| l.seq))
                .bind(last.map(|l| l.event_id))
                .bind(last.map(|l| l.envelope_digest.to_vec()))
                .fetch_one(&self.pool),
            )
            .await?;
        decode_probe_row(&row)
    }

    /// Content-free status (`store_status()`; reconciler and operator roles).
    pub async fn store_status(&self) -> Result<StoreStatusRow, StoreError> {
        let row = self
            .bounded(sqlx::query("SELECT * FROM audit_store.store_status()").fetch_one(&self.pool))
            .await?;
        decode_store_status(&row).map_err(|_| other())
    }

    /// Declared lost ranges of every recovery epoch (reconciler role).
    pub async fn lookup_lost_ranges(&self) -> Result<Vec<LostRange>, StoreError> {
        let rows = self
            .bounded(
                sqlx::query("SELECT * FROM audit_store.lookup_lost_ranges()").fetch_all(&self.pool),
            )
            .await?;
        rows.iter().map(decode_lost_range).collect()
    }

    async fn call_lookup(&self, event_ids: &[Uuid]) -> Result<Vec<ReceiptRow>, StoreError> {
        if event_ids.len() > MAX_RECEIPT_LOOKUP {
            return Err(other());
        }
        let rows = self
            .bounded(
                sqlx::query("SELECT * FROM audit_store.lookup_receipts($1)")
                    .bind(event_ids)
                    .fetch_all(&self.pool),
            )
            .await?;
        rows.iter().map(decode_receipt).collect()
    }

    async fn call_list(
        &self,
        source: &str,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<ReceiptRow>, StoreError> {
        let limit = i32::try_from(limit)
            .ok()
            .filter(|l| (1..=MAX_RECEIPT_LOOKUP as i32).contains(l))
            .ok_or_else(other)?;
        let rows = self
            .bounded(
                sqlx::query("SELECT * FROM audit_store.list_source_receipts($1, $2, $3)")
                    .bind(source)
                    .bind(after_seq)
                    .bind(limit)
                    .fetch_all(&self.pool),
            )
            .await?;
        rows.iter().map(decode_receipt).collect()
    }

    async fn call_control(&self, seqs: &[i64]) -> Result<Vec<ControlReceiptRow>, StoreError> {
        if seqs.len() > MAX_RECEIPT_LOOKUP {
            return Err(other());
        }
        let rows = self
            .bounded(
                sqlx::query("SELECT * FROM audit_store.lookup_control_receipts($1)")
                    .bind(seqs)
                    .fetch_all(&self.pool),
            )
            .await?;
        rows.iter().map(decode_control_receipt).collect()
    }

    async fn call_record(&self, control: &RelayControl) -> Result<ControlReceipt, StoreError> {
        let details = Value::Object(control.details()).to_string();
        let row = self
            .bounded(
                sqlx::query(
                    "SELECT status, seq, recovery_epoch, code \
                     FROM audit_store.record_relay_control($1, $2::text::jsonb)",
                )
                .bind(control.event_type())
                .bind(details)
                .fetch_one(&self.pool),
            )
            .await?;
        let status: String = row.try_get("status").map_err(|_| other())?;
        match status.as_str() {
            "recorded" => Ok(ControlReceipt {
                seq: row.try_get("seq").map_err(|_| other())?,
                recovery_epoch: row.try_get("recovery_epoch").map_err(|_| other())?,
            }),
            "denied" => Err(StoreError::outage(OutageCode::Denied)),
            _ => Err(other()),
        }
    }

    async fn call_report(&self, identity: &ReceiptIdentity) -> Result<(), StoreError> {
        let row = self
            .bounded(
                sqlx::query("SELECT status, code FROM audit_store.report_regression($1, $2, $3)")
                    .bind(identity.seq)
                    .bind(identity.event_id)
                    .bind(identity.envelope_digest.to_vec())
                    .fetch_one(&self.pool),
            )
            .await?;
        let status: String = row.try_get("status").map_err(|_| other())?;
        match status.as_str() {
            "recovery_pending" | "already_pending" | "intact" => Ok(()),
            "denied" => Err(StoreError::outage(OutageCode::Denied)),
            _ => Err(other()),
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

    fn probe<'a>(
        &'a self,
        expected: &'a ProbeExpectation,
    ) -> BoxFuture<'a, Result<StoreStatus, StoreError>> {
        Box::pin(self.call_probe(expected))
    }

    fn lookup_receipts<'a>(
        &'a self,
        event_ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>> {
        Box::pin(self.call_lookup(event_ids))
    }

    fn list_source_receipts<'a>(
        &'a self,
        source: &'a str,
        after_seq: i64,
        limit: u32,
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>> {
        Box::pin(self.call_list(source, after_seq, limit))
    }

    fn lookup_control_receipts<'a>(
        &'a self,
        seqs: &'a [i64],
    ) -> BoxFuture<'a, Result<Vec<ControlReceiptRow>, StoreError>> {
        Box::pin(self.call_control(seqs))
    }

    fn record_relay_control<'a>(
        &'a self,
        control: &'a RelayControl,
    ) -> BoxFuture<'a, Result<ControlReceipt, StoreError>> {
        Box::pin(self.call_record(control))
    }

    fn report_regression<'a>(
        &'a self,
        identity: &'a ReceiptIdentity,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        Box::pin(self.call_report(identity))
    }
}

/// The structured ingest row (design §7.2 step 7). A row that cannot be read
/// at all is an outage; its meaning is decided by `IngestRow::into_result`.
fn decode_ingest_row(row: &PgRow) -> Result<IngestRow, StoreError> {
    Ok(IngestRow {
        status: row.try_get("status").map_err(|_| other())?,
        seq: row.try_get("seq").map_err(|_| other())?,
        envelope_digest: row.try_get("envelope_digest").map_err(|_| other())?,
        adapter_version: row.try_get("adapter_version").map_err(|_| other())?,
        code: row.try_get("code").map_err(|_| other())?,
    })
}

fn decode_probe_row(row: &PgRow) -> Result<StoreStatus, StoreError> {
    let status: String = row.try_get("status").map_err(|_| other())?;
    match status.as_str() {
        "ok" => {}
        "denied" => return Err(StoreError::outage(OutageCode::Denied)),
        _ => return Err(other()),
    }
    let state: String = row.try_get("state").map_err(|_| other())?;
    let state = StoreState::ALL
        .into_iter()
        .find(|s| s.as_str() == state)
        .ok_or_else(other)?;
    Ok(StoreStatus {
        head_seq: row.try_get("head_seq").map_err(|_| other())?,
        recovery_epoch: row.try_get("recovery_epoch").map_err(|_| other())?,
        state,
        missing_types: row.try_get("missing_types").map_err(|_| other())?,
        regression_detected: row.try_get("regression_detected").map_err(|_| other())?,
        last_verified_seq: row.try_get("last_verified_seq").map_err(|_| other())?,
    })
}

fn digest32(bytes: Vec<u8>) -> Result<[u8; 32], StoreError> {
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| other())
}

/// Decodes one `lookup_receipts` / `list_source_receipts` row. The only
/// builder of [`ReceiptRow`] in this crate.
pub fn decode_receipt(row: &PgRow) -> Result<ReceiptRow, StoreError> {
    let origin: String = row.try_get("origin").map_err(|_| other())?;
    let commitment: Option<Vec<u8>> = row.try_get("source_commitment").map_err(|_| other())?;
    Ok(ReceiptRow {
        seq: row.try_get("seq").map_err(|_| other())?,
        event_id: row.try_get("event_id").map_err(|_| other())?,
        origin: Origin::parse(&origin).ok_or_else(other)?,
        event_type: row.try_get("event_type").map_err(|_| other())?,
        envelope_digest: digest32(row.try_get("envelope_digest").map_err(|_| other())?)?,
        source_commitment: commitment.map(digest32).transpose()?,
        expired: row.try_get("expired").map_err(|_| other())?,
        recovery_epoch: row.try_get("recovery_epoch").map_err(|_| other())?,
    })
}

/// Decodes one `lookup_control_receipts` row. The only builder of
/// [`ControlReceiptRow`] in this crate; a code that is not a bounded code is
/// malformed.
pub fn decode_control_receipt(row: &PgRow) -> Result<ControlReceiptRow, StoreError> {
    let origin: String = row.try_get("origin").map_err(|_| other())?;
    let code: Option<String> = row.try_get("code").map_err(|_| other())?;
    if code
        .as_deref()
        .is_some_and(|c| audit_core::port::BoundedCode::new(c).is_none())
    {
        return Err(other());
    }
    Ok(ControlReceiptRow {
        seq: row.try_get("seq").map_err(|_| other())?,
        recovery_epoch: row.try_get("recovery_epoch").map_err(|_| other())?,
        origin: Origin::parse(&origin).ok_or_else(other)?,
        event_type: row.try_get("event_type").map_err(|_| other())?,
        target_event_id: row.try_get("target_event_id").map_err(|_| other())?,
        code,
    })
}

fn decode_lost_range(row: &PgRow) -> Result<LostRange, StoreError> {
    let required = |name: &str| -> Result<i64, StoreError> {
        row.try_get::<Option<i64>, _>(name)
            .map_err(|_| other())?
            .ok_or_else(other)
    };
    Ok(LostRange {
        epoch_seq: required("epoch_seq")?,
        old_epoch: required("old_epoch")?,
        new_epoch: required("new_epoch")?,
        classification: row
            .try_get::<Option<String>, _>("classification")
            .map_err(|_| other())?
            .ok_or_else(other)?,
        restored_head_seq: required("restored_head_seq")?,
        lost_from_seq: required("lost_from_seq")?,
        lost_upper_seq: required("lost_upper_seq")?,
        lost_upper_known: row.try_get("lost_upper_known").map_err(|_| other())?,
    })
}

pub(crate) fn decode_store_status(row: &PgRow) -> Result<StoreStatusRow, sqlx::Error> {
    let verified_at: Option<OffsetDateTime> = row.try_get("last_verified_at")?;
    Ok(StoreStatusRow {
        head_seq: row.try_get("head_seq")?,
        recovery_epoch: row.try_get("recovery_epoch")?,
        recovery_mode: row.try_get("recovery_mode")?,
        posture_ok: row.try_get("posture_ok")?,
        recovery_pending: row.try_get("recovery_pending")?,
        recovery_pending_reason: row.try_get("recovery_pending_reason")?,
        access_reapply_pending: row.try_get("access_reapply_pending")?,
        last_verified_seq: row.try_get("last_verified_seq")?,
        last_verified_at: verified_at.map(utc_text),
        last_verified_outcome: row.try_get("last_verified_outcome")?,
    })
}

/// Classifies a driver error as an outage (design §6.3): it is never a
/// verdict, because verdicts are result rows. The Store's own gate
/// SQLSTATEs (`KA001` recovery required, `KA002` posture invalid) keep their
/// meaning; every other SQLSTATE goes through [`classify_sqlstate`].
pub fn classify_sqlx_error(error: &sqlx::Error) -> StoreError {
    let code = match error {
        sqlx::Error::Database(db) => match db.code().as_deref() {
            Some("KA001") => OutageCode::RecoveryRequired,
            Some("KA002") => OutageCode::PostureInvalid,
            Some(state) => classify_sqlstate(state),
            None => OutageCode::Other,
        },
        sqlx::Error::PoolTimedOut => OutageCode::Timeout,
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Configuration(_)
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::BeginFailed => OutageCode::Transport,
        _ => OutageCode::OutcomeUnknown,
    };
    StoreError::outage(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_failures_are_outages() {
        for (error, code) in [
            (sqlx::Error::PoolTimedOut, OutageCode::Timeout),
            (sqlx::Error::PoolClosed, OutageCode::Transport),
            (sqlx::Error::WorkerCrashed, OutageCode::Transport),
            (sqlx::Error::BeginFailed, OutageCode::Transport),
            (
                sqlx::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)),
                OutageCode::Transport,
            ),
            (sqlx::Error::RowNotFound, OutageCode::OutcomeUnknown),
            (
                sqlx::Error::Protocol("cut".into()),
                OutageCode::OutcomeUnknown,
            ),
            (
                sqlx::Error::ColumnNotFound("status".into()),
                OutageCode::OutcomeUnknown,
            ),
        ] {
            let mapped = classify_sqlx_error(&error);
            assert_eq!(mapped, StoreError::outage(code));
            assert!(mapped.is_outage() && !mapped.is_terminal());
        }
    }

    #[test]
    fn lost_ranges_contain_only_their_epoch() {
        let range = LostRange {
            epoch_seq: 9,
            old_epoch: 1,
            new_epoch: 2,
            classification: "restore".into(),
            restored_head_seq: 8,
            lost_from_seq: 9,
            lost_upper_seq: 12,
            lost_upper_known: true,
        };
        assert!(range.contains(1, 9) && range.contains(1, 12));
        assert!(!range.contains(1, 8) && !range.contains(1, 13) && !range.contains(2, 10));
        let unknown = LostRange {
            lost_upper_known: false,
            lost_upper_seq: 8,
            ..range
        };
        assert!(unknown.contains(1, 1000));
    }

    #[test]
    fn utc_text_is_microsecond_z() {
        let at =
            OffsetDateTime::from_unix_timestamp_nanos(1_791_334_923_456_789_000).expect("time");
        assert_eq!(utc_text(at), "2026-10-07T01:02:03.456789Z");
    }
}
