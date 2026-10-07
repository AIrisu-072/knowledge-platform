//! Client wrappers for the Store's administrative, investigation,
//! verification, retention and recovery functions (design §8–§11).
//!
//! Each call is one autocommit statement except [`AuditAdmin::read_page`],
//! which runs in its own READ ONLY transaction after the intent committed
//! (design §10.3) and retries the transient `intent_not_durable` refusal a
//! few times. The server enforces the disclosure rules; the client only
//! follows the expected calling convention. The relay's content-free
//! receipts and control events go through [`crate::PostgresAuditStore`].

use std::fmt;
use std::time::Duration;

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
pub use crate::store::{StoreStatusRow, utc_text};

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

    /// Whether the operation reads the contiguous chain (all origins, only
    /// `seq_after` / `seq_through` filters).
    pub const fn is_chain(self) -> bool {
        matches!(self, Self::Verify | Self::IdentityChain)
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
    pub from_seq: i64,
    pub to_seq: i64,
    pub watermark: i64,
    pub head_epoch: i64,
    pub head_chain: String,
}

/// A checkpoint: the chain position (epoch, seq, chain) of the
/// `audit.integrity.verified` record that verified `1..=verified_through`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckpointRecord {
    pub epoch: i64,
    pub seq: i64,
    pub chain: String,
    pub verified_through: i64,
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

/// `expire` outcome: `status` is `expired` or the refusal
/// (`stale_revision` / `not_expirable` / `held`, recorded as
/// `audit.retention.expire_refused`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpireOutcome {
    pub status: String,
    pub seq: i64,
    pub expired_count: i64,
    pub effective_cutoff: Option<String>,
}

/// The result of `begin_recovery_epoch` (design §11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EpochStarted {
    pub seq: i64,
    pub new_epoch: i64,
    pub restored_head_seq: i64,
    /// `restore`, `planned_move` or `regression`.
    pub classification: String,
    /// `match` / `ahead` / `store_behind` / `mismatch`, or `None` without a
    /// checkpoint.
    pub checkpoint_classification: Option<String>,
    pub lost_from_seq: i64,
    pub lost_upper_seq: i64,
}

/// A `(status, code)` outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusCode {
    pub status: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PostureViolation {
    pub violation: String,
    pub object: String,
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

/// Attempts at reading one page while the Store answers `intent_not_durable`.
const DURABILITY_ATTEMPTS: u32 = 5;

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

    /// Owner-member session for bootstrap, bind, unbind and source services.
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
        crate::store::decode_store_status(&row).map_err(protocol)
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

    /// Registers a source-service principal for a relay source (owner
    /// members only; recorded).
    pub async fn register_source_service(
        &self,
        issuer: &str,
        principal_id: &str,
        source: &str,
    ) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.register_source_service($1, $2, $3)")
            .bind(issuer)
            .bind(principal_id)
            .bind(source)
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["registered", "unchanged"])
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

    /// Administrator: the out-of-band access state was re-applied after a
    /// recovery epoch (design §11).
    pub async fn record_access_reapplied(&self) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.record_access_reapplied()")
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["recorded"])
    }

    /// Maintainer: every active retention policy was re-run after the epoch
    /// (or there is none).
    pub async fn confirm_retention_reapplied(&self) -> Result<Recorded, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.confirm_retention_reapplied()")
            .fetch_one(&self.pool)
            .await?;
        recorded(&row, &["recorded"])
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

    /// One page in a fresh READ ONLY transaction (design §10.3), without
    /// retrying.
    pub async fn read_page_once(
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

    /// One page; retries while the Store reports `intent_not_durable`.
    pub async fn read_page(
        &self,
        token: &str,
        after_seq: i64,
    ) -> Result<Vec<ExportLine>, AdminError> {
        let mut attempt = 1;
        loop {
            match self.read_page_once(token, after_seq).await {
                Err(AdminError::Retryable { .. }) if attempt < DURABILITY_ATTEMPTS => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                other => return other,
            }
        }
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
            from_seq: required(get(&row, "from_seq")?)?,
            to_seq: required(get(&row, "to_seq")?)?,
            watermark: required(get(&row, "watermark")?)?,
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
            verified_through: required(get(&row, "verified_through")?)?,
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

    /// Starts a recovery epoch (design §11). `checkpoint` is the latest
    /// out-of-band checkpoint (for a planned move: the head before the move);
    /// `relay_max_seq` the relay's highest acknowledged store seq, if known.
    pub async fn begin_recovery_epoch(
        &self,
        checkpoint: Option<&Checkpoint>,
        relay_max_seq: Option<i64>,
    ) -> Result<EpochStarted, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.begin_recovery_epoch($1, $2, $3, $4)")
            .bind(checkpoint.map(|c| c.epoch))
            .bind(checkpoint.map(|c| c.seq))
            .bind(checkpoint.map(|c| hex::encode(&c.chain)))
            .bind(relay_max_seq)
            .fetch_one(&self.pool)
            .await?;
        denied(&row)?;
        Ok(EpochStarted {
            seq: required(get(&row, "seq")?)?,
            new_epoch: required(get(&row, "new_epoch")?)?,
            restored_head_seq: required(get(&row, "restored_head_seq")?)?,
            classification: required(get(&row, "classification")?)?,
            checkpoint_classification: get(&row, "checkpoint_classification")?,
            lost_from_seq: required(get(&row, "lost_from_seq")?)?,
            lost_upper_seq: required(get(&row, "lost_upper_seq")?)?,
        })
    }

    /// Maintainer: enter recovery mode for a known incident (design §11).
    pub async fn declare_recovery_pending(
        &self,
        incident_code: &str,
    ) -> Result<StatusCode, AdminError> {
        let row = sqlx::query("SELECT * FROM audit_store.declare_recovery_pending($1)")
            .bind(incident_code)
            .fetch_one(&self.pool)
            .await?;
        denied(&row)?;
        Ok(StatusCode {
            status: get(&row, "status")?,
            code: get(&row, "code")?,
        })
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
