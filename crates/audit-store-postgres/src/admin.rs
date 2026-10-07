//! Client wrappers for the Store's administrative, investigation,
//! verification, retention and recovery functions (design §8–§11).
//!
//! Each call is one autocommit statement except [`AuditAdmin::read_page`],
//! which runs in its own READ ONLY transaction after the intent committed
//! (design §10.3). The server enforces the disclosure rules; the client only
//! follows the expected calling convention.

use std::fmt;

use audit_core::Checkpoint;
use serde::Serialize;
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::AdminError;
use crate::hex;
use crate::session::{SessionError, refuse_privileged, require_owner_member};

/// Operations of the two-phase disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessOperation {
    Investigate,
    Export,
    Verify,
    IdentityChain,
}

impl AccessOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Investigate => "investigate",
            Self::Export => "export",
            Self::Verify => "verify",
            Self::IdentityChain => "identity_chain",
        }
    }
}

/// Capability grant or revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessChange {
    Grant,
    Revoke,
}

impl AccessChange {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grant => "grant",
            Self::Revoke => "revoke",
        }
    }
}

/// A successful state change and the seq of its control event, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recorded {
    pub status: String,
    pub seq: Option<i64>,
}

/// An opened disclosure intent. The token is returned once and is a bearer
/// secret for the same session user: it is never printed by `Debug`.
#[derive(Clone)]
pub struct AccessToken {
    token: String,
    pub operation: AccessOperation,
    pub intent_seq: i64,
    pub watermark: i64,
    pub expires_at: String,
    /// Whether control events are in the visible range (fixed at intent).
    pub include_control: bool,
}

impl AccessToken {
    pub fn secret(&self) -> &str {
        &self.token
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessToken")
            .field("token", &"<redacted>")
            .field("operation", &self.operation)
            .field("intent_seq", &self.intent_seq)
            .field("watermark", &self.watermark)
            .field("expires_at", &self.expires_at)
            .field("include_control", &self.include_control)
            .finish()
    }
}

/// One export line exactly as the Store rendered it (design §10.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportLine {
    pub seq: i64,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifyReport {
    pub seq: i64,
    pub outcome: String,
    pub checked: i64,
    pub violations: Value,
    pub to_seq: i64,
    pub head_epoch: i64,
    pub head_chain: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckpointRecord {
    pub epoch: i64,
    pub seq: i64,
    pub chain: String,
    pub verified_seq: i64,
    pub outcome: String,
}

impl CheckpointRecord {
    pub fn checkpoint(&self) -> Option<Checkpoint> {
        Some(Checkpoint {
            epoch: self.epoch,
            seq: self.seq,
            chain: hex::decode32(&self.chain)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryVerify {
    pub outcome: String,
    pub checked: i64,
    pub head_seq: i64,
    pub head_epoch: i64,
    pub head_chain: String,
    pub recovery_mode: bool,
    pub violations: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyRevision {
    pub seq: i64,
    pub revision: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpireOutcome {
    pub status: String,
    pub seq: i64,
    pub expired_count: i64,
    pub effective_cutoff: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EpochStarted {
    pub seq: i64,
    pub new_epoch: i64,
    pub restored_head: i64,
    pub classification: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoreStatusRow {
    pub head_seq: i64,
    pub recovery_epoch: i64,
    pub recovery_mode: bool,
    pub posture_ok: bool,
    pub last_verified_seq: Option<i64>,
    pub last_verified_at: Option<String>,
    pub last_verified_outcome: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PostureViolation {
    pub violation: String,
    pub object: String,
}

/// Content-free receipt (design §10.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub event_id: Uuid,
    pub seq: i64,
    pub origin: String,
    pub event_type: String,
    pub envelope_digest: [u8; 32],
    pub source_commitment: Option<[u8; 32]>,
    pub adapter_version: i32,
    pub expired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlReceipt {
    pub seq: i64,
    pub event_id: Uuid,
    pub origin: String,
    pub event_type: String,
    pub envelope_digest: [u8; 32],
    pub target_event_id: Option<Uuid>,
}

/// Administrative client over one operator's own Store login.
#[derive(Clone)]
pub struct AuditAdmin {
    pool: PgPool,
}

impl fmt::Debug for AuditAdmin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditAdmin").finish_non_exhaustive()
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

/// Parses `YYYY-MM-DDTHH:MM:SS.ffffffZ`.
pub fn parse_utc_text(text: &str) -> Option<OffsetDateTime> {
    if !audit_core::kinds::is_utc_timestamp(text) {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<u32>().ok();
    let month = time::Month::try_from(u8::try_from(number(5..7)?).ok()?).ok()?;
    let date = time::Date::from_calendar_date(
        i32::try_from(number(0..4)?).ok()?,
        month,
        u8::try_from(number(8..10)?).ok()?,
    )
    .ok()?;
    let clock = time::Time::from_hms_micro(
        u8::try_from(number(11..13)?).ok()?,
        u8::try_from(number(14..16)?).ok()?,
        u8::try_from(number(17..19)?).ok()?,
        number(20..26)?,
    )
    .ok()?;
    Some(time::PrimitiveDateTime::new(date, clock).assume_utc())
}

fn protocol(_: sqlx::Error) -> AdminError {
    AdminError::Protocol("unexpected column shape")
}

fn get<'r, T>(row: &'r PgRow, name: &str) -> Result<T, AdminError>
where
    T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(name).map_err(protocol)
}

fn required<T>(value: Option<T>) -> Result<T, AdminError> {
    value.ok_or(AdminError::Protocol("missing value"))
}

fn digest(bytes: Vec<u8>) -> Result<[u8; 32], AdminError> {
    hex::digest32(&bytes).ok_or(AdminError::Protocol("digest length"))
}

/// Maps a `(status, seq, code)` row: `ok` statuses succeed, `denied` and
/// `refused` become [`AdminError::Denied`].
fn recorded(row: &PgRow, ok: &[&str]) -> Result<Recorded, AdminError> {
    let status: String = get(row, "status")?;
    let code: Option<String> = get(row, "code")?;
    if ok.contains(&status.as_str()) {
        return Ok(Recorded {
            status,
            seq: get(row, "seq")?,
        });
    }
    match status.as_str() {
        "denied" | "refused" => Err(AdminError::Denied {
            code: code.unwrap_or_else(|| status.clone()),
        }),
        _ => Err(AdminError::Protocol("unexpected status")),
    }
}

fn denied(row: &PgRow) -> Result<(), AdminError> {
    let status: String = get(row, "status")?;
    if status == "denied" || status == "refused" {
        let code: Option<String> = get(row, "code")?;
        return Err(AdminError::Denied {
            code: code.unwrap_or(status),
        });
    }
    Ok(())
}

impl AuditAdmin {
    /// Operator session: refuses superuser and owner-member logins.
    pub async fn connect(pool: PgPool) -> Result<Self, SessionError> {
        refuse_privileged(&pool).await?;
        Ok(Self { pool })
    }

    /// Owner-member session for bootstrap, bind and unbind.
    pub async fn connect_owner(pool: PgPool) -> Result<Self, SessionError> {
        require_owner_member(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn store_status(&self) -> Result<StoreStatusRow, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.store_status()")
            .fetch_one(&self.pool)
            .await?;
        let verified_at: Option<OffsetDateTime> = get(&row, "last_verified_at")?;
        Ok(StoreStatusRow {
            head_seq: get(&row, "head_seq")?,
            recovery_epoch: get(&row, "recovery_epoch")?,
            recovery_mode: get(&row, "recovery_mode")?,
            posture_ok: get(&row, "posture_ok")?,
            last_verified_seq: get(&row, "last_verified_seq")?,
            last_verified_at: verified_at.map(utc_text),
            last_verified_outcome: get(&row, "last_verified_outcome")?,
        })
    }

    pub async fn posture(&self) -> Result<Vec<PostureViolation>, AdminError> {
        let rows =
            sqlx::query("SELECT violation, object FROM audit_store.posture_check() ORDER BY 1, 2")
                .fetch_all(&self.pool)
                .await?;
        rows.iter()
            .map(|row| {
                Ok(PostureViolation {
                    violation: get(row, "violation")?,
                    object: get(row, "object")?,
                })
            })
            .collect()
    }

    pub async fn bootstrap_administrator(
        &self,
        db_role: &str,
        issuer: &str,
        principal_id: &str,
    ) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.bootstrap_administrator($1, $2, $3)")
            .bind(db_role)
            .bind(issuer)
            .bind(principal_id)
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["bootstrapped"])
    }

    pub async fn bind_principal(
        &self,
        db_role: &str,
        issuer: &str,
        principal_id: &str,
    ) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.bind_principal($1, $2, $3)")
            .bind(db_role)
            .bind(issuer)
            .bind(principal_id)
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["bound"])
    }

    pub async fn unbind_principal(&self, db_role: &str) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.unbind_principal($1)")
            .bind(db_role)
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["unbound"])
    }

    pub async fn change_access(
        &self,
        issuer: &str,
        principal_id: &str,
        capability: &str,
        change: AccessChange,
    ) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.change_access($1, $2, $3, $4)")
            .bind(issuer)
            .bind(principal_id)
            .bind(capability)
            .bind(change.as_str())
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["granted", "revoked", "unchanged"])
    }

    pub async fn set_retention_policy(
        &self,
        policy_id: &str,
        selector: &Value,
        retain_days: Option<i32>,
    ) -> Result<PolicyRevision, AdminError> {
        let row =
            sqlx::query("SELECT * FROM audit_store.set_retention_policy($1, $2::text::jsonb, $3)")
                .bind(policy_id)
                .bind(selector.to_string())
                .bind(retain_days)
                .fetch_one(&self.pool)
                .await?;
        denied(&row)?;
        Ok(PolicyRevision {
            seq: required(get(&row, "seq")?)?,
            revision: required(get(&row, "revision")?)?,
        })
    }

    pub async fn expire(
        &self,
        policy_id: &str,
        expected_revision: i32,
        cutoff: OffsetDateTime,
        limit: i32,
    ) -> Result<ExpireOutcome, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.expire($1, $2, $3, $4)")
            .bind(policy_id)
            .bind(expected_revision)
            .bind(cutoff)
            .bind(limit)
            .fetch_one(&self.pool)
            .await?;
        denied(&row)?;
        let effective: Option<OffsetDateTime> = get(&row, "effective_cutoff")?;
        Ok(ExpireOutcome {
            status: get(&row, "status")?,
            seq: required(get(&row, "seq")?)?,
            expired_count: required(get(&row, "expired_count")?)?,
            effective_cutoff: effective.map(utc_text),
        })
    }

    pub async fn purge_body(
        &self,
        event_id: Uuid,
        reason_code: &str,
    ) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.purge_body($1, $2)")
            .bind(event_id)
            .bind(reason_code)
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["purged"])
    }

    /// Records a relay control event and returns its seq (design §4.5).
    pub async fn record_relay_control(
        &self,
        event_type: &str,
        event_id: Uuid,
        code: &str,
        counts: Option<&Value>,
    ) -> Result<i64, AdminError> {
        let row = sqlx::query(
            "SELECT * FROM audit_store.record_relay_control($1, $2, $3, $4::text::jsonb)",
        )
        .bind(event_type)
        .bind(event_id)
        .bind(code)
        .bind(counts.map(Value::to_string))
        .fetch_one(&self.pool)
        .await?;
        required(recorded(&row, &["recorded"])?.seq)
    }

    pub async fn open_access(
        &self,
        operation: AccessOperation,
        filter: &Value,
        page_size: i32,
        max_pages: i32,
    ) -> Result<AccessToken, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.open_access($1, $2::text::jsonb, $3, $4)")
            .bind(operation.as_str())
            .bind(filter.to_string())
            .bind(page_size)
            .bind(max_pages)
            .fetch_one(&self.pool)
            .await?;
        denied(&row)?;
        let expires_at: Option<OffsetDateTime> = get(&row, "expires_at")?;
        Ok(AccessToken {
            token: required(get(&row, "token")?)?,
            operation,
            intent_seq: required(get(&row, "intent_seq")?)?,
            watermark: required(get(&row, "watermark")?)?,
            expires_at: utc_text(required(expires_at)?),
            include_control: required(get(&row, "include_control")?)?,
        })
    }

    /// One page in a fresh READ ONLY transaction (design §10.3).
    pub async fn read_page(
        &self,
        token: &str,
        after_seq: i64,
    ) -> Result<Vec<ExportLine>, AdminError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *tx)
            .await?;
        let rows = sqlx::query("SELECT seq, line FROM audit_store.read_page($1, $2)")
            .bind(token)
            .bind(after_seq)
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;
        lines(&rows)
    }

    /// All pages of an intent, each in its own READ ONLY transaction.
    pub async fn read_all(&self, token: &AccessToken) -> Result<Vec<Vec<ExportLine>>, AdminError> {
        let mut pages = Vec::new();
        let mut after = 0;
        loop {
            let page = self.read_page(token.secret(), after).await?;
            let Some(last) = page.last() else {
                break;
            };
            after = last.seq;
            pages.push(page);
        }
        Ok(pages)
    }

    pub async fn close_access(
        &self,
        token: &str,
        returned_count: i64,
        page_digests: &[String],
    ) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.close_access($1, $2, $3)")
            .bind(token)
            .bind(returned_count)
            .bind(page_digests)
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["closed"])
    }

    pub async fn verify(
        &self,
        from: Option<i64>,
        to: Option<i64>,
    ) -> Result<VerifyReport, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.verify($1, $2)")
            .bind(from)
            .bind(to)
            .fetch_one(&self.pool)
            .await?;
        denied(&row)?;
        Ok(VerifyReport {
            seq: required(get(&row, "seq")?)?,
            outcome: required(get(&row, "outcome")?)?,
            checked: required(get(&row, "checked")?)?,
            violations: required(get::<Option<Value>>(&row, "violations")?)?,
            to_seq: required(get(&row, "to_seq")?)?,
            head_epoch: required(get(&row, "head_epoch")?)?,
            head_chain: required(get(&row, "head_chain")?)?,
        })
    }

    pub async fn checkpoint(&self) -> Result<CheckpointRecord, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.checkpoint()")
            .fetch_one(&self.pool)
            .await?;
        denied(&row)?;
        Ok(CheckpointRecord {
            epoch: required(get(&row, "epoch")?)?,
            seq: required(get(&row, "seq")?)?,
            chain: required(get(&row, "chain")?)?,
            verified_seq: required(get(&row, "verified_seq")?)?,
            outcome: required(get(&row, "outcome")?)?,
        })
    }

    pub async fn verify_recovery(&self) -> Result<RecoveryVerify, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.verify_recovery()")
            .fetch_one(&self.pool)
            .await?;
        Ok(RecoveryVerify {
            outcome: get(&row, "outcome")?,
            checked: get(&row, "checked")?,
            head_seq: get(&row, "head_seq")?,
            head_epoch: get(&row, "head_epoch")?,
            head_chain: get(&row, "head_chain")?,
            recovery_mode: get(&row, "recovery_mode")?,
            violations: get(&row, "violations")?,
        })
    }

    pub async fn identity_chain_recovery_page(
        &self,
        after_seq: i64,
        limit: i32,
    ) -> Result<Vec<ExportLine>, AdminError> {
        let rows =
            sqlx::query("SELECT seq, line FROM audit_store.identity_chain_recovery_page($1, $2)")
                .bind(after_seq)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
        lines(&rows)
    }

    pub async fn begin_recovery_epoch(
        &self,
        checkpoint: &Checkpoint,
        relay_max_seq: i64,
    ) -> Result<EpochStarted, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.begin_recovery_epoch($1, $2, $3, $4)")
            .bind(checkpoint.epoch)
            .bind(checkpoint.seq)
            .bind(hex::encode(&checkpoint.chain))
            .bind(relay_max_seq)
            .fetch_one(&self.pool)
            .await?;
        denied(&row)?;
        Ok(EpochStarted {
            seq: required(get(&row, "seq")?)?,
            new_epoch: required(get(&row, "new_epoch")?)?,
            restored_head: required(get(&row, "restored_head")?)?,
            classification: required(get(&row, "classification")?)?,
        })
    }

    pub async fn rebind_fingerprint(&self) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.rebind_fingerprint()")
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["rebound", "unchanged"])
    }

    pub async fn lookup_receipts(&self, event_ids: &[Uuid]) -> Result<Vec<Receipt>, AdminError> {
        let rows = sqlx::query("SELECT * FROM audit_store.lookup_receipts($1)")
            .bind(event_ids)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(receipt).collect()
    }

    pub async fn list_source_receipts(
        &self,
        source: &str,
        after_seq: i64,
        limit: i32,
    ) -> Result<Vec<Receipt>, AdminError> {
        let rows = sqlx::query("SELECT * FROM audit_store.list_source_receipts($1, $2, $3)")
            .bind(source)
            .bind(after_seq)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(receipt).collect()
    }

    pub async fn lookup_control_receipts(
        &self,
        seqs: &[i64],
    ) -> Result<Vec<ControlReceipt>, AdminError> {
        let rows = sqlx::query("SELECT * FROM audit_store.lookup_control_receipts($1)")
            .bind(seqs)
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| {
                Ok(ControlReceipt {
                    seq: get(row, "seq")?,
                    event_id: get(row, "event_id")?,
                    origin: get(row, "origin")?,
                    event_type: get(row, "event_type")?,
                    envelope_digest: digest(get(row, "envelope_digest")?)?,
                    target_event_id: get(row, "target_event_id")?,
                })
            })
            .collect()
    }
}

fn lines(rows: &[PgRow]) -> Result<Vec<ExportLine>, AdminError> {
    rows.iter()
        .map(|row| {
            Ok(ExportLine {
                seq: get(row, "seq")?,
                line: get(row, "line")?,
            })
        })
        .collect()
}

fn receipt(row: &PgRow) -> Result<Receipt, AdminError> {
    let commitment: Option<Vec<u8>> = get(row, "source_commitment")?;
    Ok(Receipt {
        event_id: get(row, "event_id")?,
        seq: get(row, "seq")?,
        origin: get(row, "origin")?,
        event_type: get(row, "event_type")?,
        envelope_digest: digest(get(row, "envelope_digest")?)?,
        source_commitment: commitment.map(digest).transpose()?,
        adapter_version: get(row, "adapter_version")?,
        expired: get(row, "expired")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_text_round_trips() {
        let text = "2026-10-07T01:02:03.456789Z";
        let parsed = parse_utc_text(text).expect("valid");
        assert_eq!(utc_text(parsed), text);
        assert_eq!(parse_utc_text("2026-02-30T00:00:00.000000Z"), None);
        assert_eq!(parse_utc_text("2026-10-07T01:02:03Z"), None);
    }

    #[test]
    fn token_debug_is_redacted() {
        let token = AccessToken {
            token: "ab".repeat(32),
            operation: AccessOperation::Export,
            intent_seq: 3,
            watermark: 2,
            expires_at: "2026-10-07T01:02:03.000000Z".into(),
            include_control: false,
        };
        let rendered = format!("{token:?}");
        assert!(!rendered.contains("abab"));
        assert!(rendered.contains("<redacted>"));
    }
}
