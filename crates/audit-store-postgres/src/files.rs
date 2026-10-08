//! Export, manifest and checkpoint files (design §8, §10.3, §10.4, §11).
//!
//! Files are created new with mode 0600 and never overwritten. Their content
//! is not logged. A chain export (`verify` with bodies, or `identity_chain`)
//! is read through as many intents as needed — each intent covers at most
//! `page_size * max_pages` rows of `(seq_after, seq_through]`, the first
//! intent fixes the target watermark and every intent is recorded in the
//! Store and listed in the manifest — and is verified offline with
//! audit-core (`verify_export_complete` / `verify_identity_chain_complete`)
//! from genesis or a trusted checkpoint up to that watermark before the
//! manifest is written. A filtered export is reported as an unanchored
//! subset. The manifest names the chain integrity verdict (`intact` /
//! `unanchored`; a broken chain writes no files).
//!
//! An expiry that commits after the first intent fixed the watermark W can
//! remove bodies of rows at or below W before they are read: those rows name
//! evidence past W, which the export does not contain. Such an export is
//! never presented as covering that evidence: it is written with
//! `complete: false` and `expired_after_watermark` > 0, its rows count as
//! unverified expiry evidence, and `assess_recovery` never calls it
//! `Authentic`. Export again (a new intent's W covers the evidence) to
//! verify those rows.
//!
//! The out-of-band recovery records (`kp-audit-recovery-records-v1`) are
//! JSON lines, one [`RecoveryRecordLine`] per recovery epoch, appended by the
//! operator outside the database (design §8, §11). `audit-admin
//! begin-recovery-epoch` prints the exact line; `audit-admin assess` reads
//! the file.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use audit_core::{
    Anchor, ChainIntegrity, Checkpoint, CheckpointComparison, ExportError, ExportReport, GENESIS,
    RecoveryRecord, compare_checkpoint, verify_export, verify_export_complete,
    verify_export_subset, verify_identity_chain, verify_identity_chain_complete,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::admin::{AccessOperation, AuditAdmin, CheckpointRecord, ExportLine};
use crate::error::AdminError;
use crate::hex;

pub const EXPORT_FILE: &str = "export.jsonl";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const MANIFEST_FORMAT: &str = "kp-audit-export-manifest-v1";
pub const CHECKPOINT_FORMAT: &str = "kp-audit-checkpoint-v1";
pub const RECOVERY_RECORDS_FORMAT: &str = "kp-audit-recovery-records-v1";

/// Upper bound on intents per chain export (a runaway guard, not a limit of
/// the Store: 1000 intents of 100k rows each).
const MAX_INTENTS: usize = 1000;

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error(transparent)]
    Store(#[from] AdminError),
    #[error("file error: {0}")]
    Io(#[from] io::Error),
    #[error("export verification failed: {0}")]
    Verification(#[from] audit_core::ExportError),
    #[error("export does not reach its watermark (head {head}, watermark {watermark})")]
    Incomplete { head: i64, watermark: i64 },
    #[error("invalid checkpoint file")]
    Checkpoint,
    #[error("a chain export needs genesis or a checkpoint at seq_after as its anchor")]
    Unanchored,
    #[error("refusing to write a checkpoint for a chain with violations")]
    Violations,
    #[error("recovery records line {line}: not a valid {RECOVERY_RECORDS_FORMAT} record")]
    RecoveryRecords { line: usize },
}

/// Creates `path` (it must not exist) with mode 0600 and writes `bytes`.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// SHA-256 of a page's lines, each followed by `\n`.
pub fn page_digest(page: &[ExportLine]) -> String {
    let mut hasher = Sha256::new();
    for line in page {
        hasher.update(line.line.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(&hasher.finalize())
}

/// The out-of-band checkpoint record written by `audit-admin checkpoint`:
/// the chain position of the `audit.integrity.verified` record that
/// verified `1..=verified_through`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointFile {
    pub format: String,
    pub epoch: i64,
    pub seq: i64,
    pub chain: String,
    pub verified_through: i64,
    pub genesis: String,
}

impl CheckpointFile {
    pub fn from_record(record: &CheckpointRecord) -> Self {
        Self {
            format: CHECKPOINT_FORMAT.to_owned(),
            epoch: record.epoch,
            seq: record.seq,
            chain: record.chain.clone(),
            verified_through: record.verified_through,
            genesis: hex::encode(&GENESIS),
        }
    }

    pub fn checkpoint(&self) -> Option<Checkpoint> {
        (self.format == CHECKPOINT_FORMAT && self.epoch >= 1 && self.seq >= 0).then_some(())?;
        Some(Checkpoint {
            epoch: self.epoch,
            seq: self.seq,
            chain: hex::decode32(&self.chain)?,
        })
    }

    pub fn read(path: &Path) -> Result<Self, FileError> {
        let text = std::fs::read_to_string(path)?;
        let file: Self = serde_json::from_str(&text).map_err(|_| FileError::Checkpoint)?;
        file.checkpoint().ok_or(FileError::Checkpoint)?;
        Ok(file)
    }

    pub fn write(&self, path: &Path) -> Result<(), FileError> {
        let mut text = serde_json::to_string_pretty(self).map_err(|_| FileError::Checkpoint)?;
        text.push('\n');
        write_private(path, text.as_bytes())?;
        Ok(())
    }
}

/// One line of the out-of-band recovery records file
/// (`kp-audit-recovery-records-v1`): the operator's record of one recovery
/// epoch transition (`audit_core::RecoveryRecord`). The lost range is
/// `(restored_head_seq, lost_upper]`; `lost_upper = restored_head_seq` loses
/// nothing (a planned move). `lost_upper` is `null` when the Store could not
/// bound the lost range (`lost_upper_known: false`: a restore recorded
/// without a checkpoint, a relay seq or a regression report); such a record
/// is never authentic. The file is JSON lines, appended in epoch order; blank
/// lines are ignored and every other line must be a valid record (the key set
/// is closed and every key is required, keys are unique,
/// `new_epoch = old_epoch + 1`, `0 <= restored_head_seq <= lost_upper` when
/// it is a number, the chain is 64 lowercase hex digits).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRecordLine {
    pub format: String,
    pub old_epoch: i64,
    pub new_epoch: i64,
    pub restored_head_seq: i64,
    pub restored_head_chain: String,
    /// Required; `null` is the unknown bound (never a missing key).
    #[serde(deserialize_with = "required_nullable")]
    pub lost_upper: Option<i64>,
}

/// A required member whose value may be `null` (`Option` members are
/// otherwise optional in serde).
fn required_nullable<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<i64>::deserialize(deserializer)
}

impl RecoveryRecordLine {
    pub fn from_record(record: &RecoveryRecord) -> Self {
        Self {
            format: RECOVERY_RECORDS_FORMAT.to_owned(),
            old_epoch: record.old_epoch,
            new_epoch: record.new_epoch,
            restored_head_seq: record.restored_head_seq,
            restored_head_chain: hex::encode(&record.restored_head_chain),
            lost_upper: record.lost_upper_known.then_some(record.lost_upper),
        }
    }

    /// The record, when the line is well formed. An unknown bound
    /// (`lost_upper: null`) is the record's `lost_upper_known: false` with
    /// `lost_upper = restored_head_seq`.
    pub fn record(&self) -> Option<RecoveryRecord> {
        let valid = self.format == RECOVERY_RECORDS_FORMAT
            && self.old_epoch >= 1
            && self.old_epoch.checked_add(1) == Some(self.new_epoch)
            && self.restored_head_seq >= 0
            && self
                .lost_upper
                .is_none_or(|upper| upper >= self.restored_head_seq);
        valid.then_some(())?;
        Some(RecoveryRecord {
            old_epoch: self.old_epoch,
            new_epoch: self.new_epoch,
            restored_head_seq: self.restored_head_seq,
            restored_head_chain: hex::decode32(&self.restored_head_chain)?,
            lost_upper: self.lost_upper.unwrap_or(self.restored_head_seq),
            lost_upper_known: self.lost_upper.is_some(),
        })
    }

    /// The exact line to append (compact JSON, without the newline).
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Parses a recovery records file (`kp-audit-recovery-records-v1`).
pub fn parse_recovery_records(text: &str) -> Result<Vec<RecoveryRecord>, FileError> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            audit_core::parse_unique(line)
                .ok()
                .and_then(|value| serde_json::from_value::<RecoveryRecordLine>(value).ok())
                .and_then(|line| line.record())
                .ok_or(FileError::RecoveryRecords { line: index + 1 })
        })
        .collect()
}

/// Reads a recovery records file.
pub fn read_recovery_records(path: &Path) -> Result<Vec<RecoveryRecord>, FileError> {
    parse_recovery_records(&std::fs::read_to_string(path)?)
}

/// Writes the checkpoint file for a clean `checkpoint()` result.
pub fn write_checkpoint(
    record: &CheckpointRecord,
    path: &Path,
) -> Result<CheckpointFile, FileError> {
    if record.outcome != "ok" {
        return Err(FileError::Violations);
    }
    let file = CheckpointFile::from_record(record);
    file.write(path)?;
    Ok(file)
}

/// One disclosure intent of an export (each is chained in the Store).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestIntent {
    pub intent_seq: i64,
    pub watermark: i64,
    pub seq_after: i64,
    pub seq_through: Option<i64>,
    pub rows: u64,
    pub page_digests: Vec<String>,
}

/// Export manifest (design §10.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    pub format: &'static str,
    pub operation: AccessOperation,
    /// Every intent opened for this export, in order.
    pub intents: Vec<ManifestIntent>,
    /// The watermark the export covers (fixed by the first intent).
    pub watermark: Option<i64>,
    pub rows: u64,
    pub first_seq: Option<i64>,
    pub last_seq: Option<i64>,
    pub genesis: String,
    pub anchored: bool,
    /// Anchored, verified contiguously up to `watermark`, and every expiry
    /// evidence it names lies at or below `watermark`.
    pub complete: bool,
    /// `intact` (anchored, contiguous) or `unanchored` (filtered subset):
    /// what the verification established about the chain itself
    /// (`audit_core::ChainIntegrity`). Never an authenticity claim.
    pub chain_integrity: &'static str,
    /// Rows at or below the watermark whose body was removed by an expiry
    /// that committed after the watermark (a concurrent `expire` /
    /// `purge_body`): their evidence is not in this export, so it is not
    /// complete.
    pub expired_after_watermark: u64,
    pub head: Option<ManifestHead>,
    pub checkpoint: Option<ManifestCheckpoint>,
    pub unverified_expiry_evidence: u64,
    pub epochs_authenticated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestHead {
    pub epoch: i64,
    pub seq: i64,
    pub chain: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestCheckpoint {
    pub epoch: i64,
    pub seq: i64,
    pub chain: String,
    pub comparison: &'static str,
}

/// How an export is requested.
#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub operation: AccessOperation,
    pub filter: Value,
    pub page_size: i32,
    pub max_pages: i32,
    /// Trusted out-of-band checkpoint to anchor (when the export starts at
    /// its seq) and to compare with.
    pub checkpoint: Option<Checkpoint>,
}

/// Result of writing an export directory.
#[derive(Debug)]
pub struct ExportOutcome {
    pub manifest: Manifest,
    pub report: ExportReport,
    pub export_path: PathBuf,
    pub manifest_path: PathBuf,
}

/// The manifest name of a checkpoint comparison.
pub const fn comparison_name(comparison: CheckpointComparison) -> &'static str {
    match comparison {
        CheckpointComparison::Match => "match",
        CheckpointComparison::Mismatch => "mismatch",
        CheckpointComparison::EpochMismatch => "epoch_mismatch",
        CheckpointComparison::StoreBehind => "store_behind",
        CheckpointComparison::Ahead => "ahead",
        CheckpointComparison::BeforeAnchor => "before_anchor",
        CheckpointComparison::Unanchored => "unanchored",
    }
}

/// The result of verifying an anchored chain export against its watermark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainCheck {
    pub report: ExportReport,
    /// Verified by `verify_export_complete` /
    /// `verify_identity_chain_complete`: the head is the watermark and no row
    /// names expiry evidence past it.
    pub complete: bool,
    /// Rows whose expiry evidence lies past the watermark (an expiry that
    /// committed after the intent): counted unverified, never covered.
    pub expired_after_watermark: u64,
}

/// Offline verification of an anchored chain export that must reach
/// `watermark` exactly: audit-core's `verify_export_complete` (bodies) or
/// `verify_identity_chain_complete`. The single place that decides
/// completeness.
///
/// The only failure that is not an error is evidence past the watermark: an
/// `expire` / `purge_body` that committed after the intent fixed W removed
/// the bodies of rows at or below W. The export is then verified as an
/// ordinary anchored export (those rows count as unverified expiry
/// evidence), `complete` is false and the rows are counted in
/// `expired_after_watermark`. Every other failure, including missing
/// evidence at or below W, is an error.
pub fn verify_chain_export(
    text: &str,
    anchor: Anchor,
    watermark: i64,
    bodies: bool,
) -> Result<ChainCheck, FileError> {
    let complete = if bodies {
        verify_export_complete(text, anchor, watermark)
    } else {
        verify_identity_chain_complete(text, anchor, watermark)
    };
    let error = match complete {
        Ok(report) => {
            return Ok(ChainCheck {
                report,
                complete: true,
                expired_after_watermark: 0,
            });
        }
        Err(ExportError::WatermarkMismatch { watermark, head }) => {
            return Err(FileError::Incomplete { head, watermark });
        }
        Err(error @ ExportError::ExpiryEvidenceMissing { .. }) => error,
        Err(error) => return Err(error.into()),
    };
    let report = if bodies {
        verify_export(text, anchor)?
    } else {
        verify_identity_chain(text, anchor)?
    };
    if report.head.seq != watermark {
        return Err(FileError::Incomplete {
            head: report.head.seq,
            watermark,
        });
    }
    let expired_after_watermark = report
        .expired_rows()
        .iter()
        .filter(|row| row.evidence_seq > watermark)
        .count() as u64;
    if expired_after_watermark == 0 {
        return Err(error.into());
    }
    Ok(ChainCheck {
        report,
        complete: false,
        expired_after_watermark,
    })
}

fn seq_member(filter: &Map<String, Value>, key: &str) -> Option<i64> {
    filter.get(key).and_then(Value::as_i64)
}

/// Opens one intent, reads every page in READ ONLY transactions and records
/// the close.
async fn read_intent(
    admin: &AuditAdmin,
    request: &ExportRequest,
    filter: &Value,
) -> Result<(ManifestIntent, Vec<ExportLine>, bool), FileError> {
    let token = admin
        .open_access(
            request.operation,
            filter,
            request.page_size,
            request.max_pages,
        )
        .await?;
    let pages = admin.read_all(&token).await?;
    let page_digests: Vec<String> = pages.iter().map(|page| page_digest(page)).collect();
    let rows: Vec<ExportLine> = pages.into_iter().flatten().collect();
    let returned = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    admin
        .close_access(token.secret(), returned, &page_digests)
        .await?;
    let object = filter.as_object().cloned().unwrap_or_default();
    Ok((
        ManifestIntent {
            intent_seq: token.intent_seq,
            watermark: token.watermark,
            seq_after: seq_member(&object, "seq_after").unwrap_or(0),
            seq_through: seq_member(&object, "seq_through"),
            rows: rows.len() as u64,
            page_digests,
        },
        rows,
        token.include_control,
    ))
}

/// Opens the intents, reads every page, records the closes, verifies offline
/// and writes `export.jsonl` and `manifest.json`.
pub async fn export_to_dir(
    admin: &AuditAdmin,
    request: &ExportRequest,
    dir: &Path,
) -> Result<ExportOutcome, FileError> {
    let filter = request.filter.as_object().cloned().unwrap_or_default();
    let range_only = filter
        .keys()
        .all(|k| k == "seq_after" || k == "seq_through");
    if request.operation.is_chain() || range_only {
        return export_range(admin, request, &filter, dir).await;
    }
    let (intent, rows, _) = read_intent(admin, request, &request.filter).await?;
    let watermark = intent.watermark;
    let report = verify_export_subset(&joined(&rows))?;
    write_export(
        request,
        &rows,
        vec![intent],
        Some(watermark),
        unanchored(report),
        dir,
    )
}

fn unanchored(report: ExportReport) -> ChainCheck {
    ChainCheck {
        report,
        complete: false,
        expired_after_watermark: 0,
    }
}

/// A range export (`seq_after` / `seq_through` only), looping over intents
/// until the first intent's watermark (or the requested `seq_through`).
async fn export_range(
    admin: &AuditAdmin,
    request: &ExportRequest,
    filter: &Map<String, Value>,
    dir: &Path,
) -> Result<ExportOutcome, FileError> {
    let start = seq_member(filter, "seq_after").unwrap_or(0);
    let mut target = seq_member(filter, "seq_through");
    let mut after = start;
    let mut intents = Vec::new();
    let mut rows: Vec<ExportLine> = Vec::new();
    let mut full_visibility = true;
    for _ in 0..MAX_INTENTS {
        let mut next = json!({"seq_after": after});
        if let Some(target) = target {
            if after >= target {
                break;
            }
            next["seq_through"] = json!(target);
        }
        let (intent, page, visible) = read_intent(admin, request, &next).await?;
        full_visibility &= visible;
        let fixed = *target.get_or_insert(intent.watermark);
        intents.push(intent);
        let Some(last) = page.last() else {
            break;
        };
        after = last.seq;
        rows.extend(page);
        if after >= fixed || !visible {
            break;
        }
    }
    let watermark = target.unwrap_or(start);
    let text = joined(&rows);
    let anchor = match request.checkpoint {
        _ if start == 0 => Some(Anchor::Genesis),
        Some(checkpoint) if checkpoint.seq == start => Some(Anchor::Checkpoint(checkpoint)),
        _ => None,
    };
    let bodies = request.operation != AccessOperation::IdentityChain;
    let check = match anchor {
        Some(anchor) if full_visibility && request.operation != AccessOperation::Investigate => {
            verify_chain_export(&text, anchor, watermark, bodies)?
        }
        _ if request.operation == AccessOperation::IdentityChain => {
            return Err(FileError::Unanchored);
        }
        _ => unanchored(verify_export_subset(&text)?),
    };
    write_export(request, &rows, intents, Some(watermark), check, dir)
}

/// Writes an identity-chain export from the recovery path (no intent; the
/// Store records nothing in recovery mode, design §11), verified from
/// genesis.
pub async fn export_identity_chain_recovery(
    admin: &AuditAdmin,
    checkpoint: Option<Checkpoint>,
    dir: &Path,
) -> Result<ExportOutcome, FileError> {
    const PAGE: i32 = 1000;
    let mut rows: Vec<ExportLine> = Vec::new();
    let mut after = 0;
    loop {
        let page = admin.identity_chain_recovery_page(after, PAGE).await?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.seq;
        rows.extend(page);
    }
    let request = ExportRequest {
        operation: AccessOperation::IdentityChain,
        filter: Value::Object(Map::new()),
        page_size: PAGE,
        max_pages: 1,
        checkpoint,
    };
    let check = verify_chain_export(&joined(&rows), Anchor::Genesis, after, false)?;
    write_export(&request, &rows, Vec::new(), Some(after), check, dir)
}

fn joined(rows: &[ExportLine]) -> String {
    let mut text = String::new();
    for row in rows {
        text.push_str(&row.line);
        text.push('\n');
    }
    text
}

fn write_export(
    request: &ExportRequest,
    rows: &[ExportLine],
    intents: Vec<ManifestIntent>,
    watermark: Option<i64>,
    check: ChainCheck,
    dir: &Path,
) -> Result<ExportOutcome, FileError> {
    let ChainCheck {
        report,
        complete,
        expired_after_watermark,
    } = check;
    let text = joined(rows);
    let checkpoint = request.checkpoint.map(|c| ManifestCheckpoint {
        epoch: c.epoch,
        seq: c.seq,
        chain: hex::encode(&c.chain),
        comparison: comparison_name(compare_checkpoint(&report, &c)),
    });
    let manifest = Manifest {
        format: MANIFEST_FORMAT,
        operation: request.operation,
        intents,
        watermark,
        rows: report.rows,
        first_seq: rows.first().map(|r| r.seq),
        last_seq: rows.last().map(|r| r.seq),
        genesis: hex::encode(&GENESIS),
        anchored: report.anchored,
        complete,
        chain_integrity: ChainIntegrity::of_report(&report).as_str(),
        expired_after_watermark,
        head: report.anchored.then(|| ManifestHead {
            epoch: report.head.epoch,
            seq: report.head.seq,
            chain: hex::encode(&report.head.chain),
        }),
        checkpoint,
        unverified_expiry_evidence: report.unverified_expiry_evidence,
        epochs_authenticated: report.epochs_authenticated,
    };
    let export_path = dir.join(EXPORT_FILE);
    let manifest_path = dir.join(MANIFEST_FILE);
    write_private(&export_path, text.as_bytes())?;
    let mut manifest_text = serde_json::to_string_pretty(&manifest)
        .map_err(|_| FileError::Store(AdminError::Protocol("manifest serialization")))?;
    manifest_text.push('\n');
    write_private(&manifest_path, manifest_text.as_bytes())?;
    Ok(ExportOutcome {
        manifest,
        report,
        export_path,
        manifest_path,
    })
}
