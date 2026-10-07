//! Export, manifest and checkpoint files (design §8, §10.3, §10.4, §11).
//!
//! Files are created new with mode 0600 and never overwritten. Their content
//! is not logged. A chain export (`verify` with bodies, or `identity_chain`)
//! is read through as many intents as needed — each intent covers at most
//! `page_size * max_pages` rows of `(seq_after, seq_through]`, the first
//! intent fixes the target watermark and every intent is recorded in the
//! Store and listed in the manifest — and is verified offline with
//! audit-core from genesis or a trusted checkpoint up to that watermark
//! before the manifest is written. A filtered export is reported as an
//! unanchored subset.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use audit_core::{
    Anchor, Checkpoint, CheckpointComparison, ExportReport, GENESIS, compare_checkpoint,
    verify_export, verify_export_subset, verify_identity_chain,
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
    /// Anchored and verified contiguously up to `watermark`.
    pub complete: bool,
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

const fn comparison_name(comparison: CheckpointComparison) -> &'static str {
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

/// Offline verification of an anchored chain export that must reach
/// `watermark` exactly. The single place that decides completeness.
// TODO(audit-core amendment): replace with audit_core::verify_export_complete
// (and its identity-chain counterpart) once it exists.
pub fn verify_complete(
    text: &str,
    anchor: Anchor,
    watermark: i64,
    bodies: bool,
) -> Result<ExportReport, FileError> {
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
    Ok(report)
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
        report,
        false,
        dir,
    )
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
    let (report, complete) = match anchor {
        Some(anchor) if full_visibility && request.operation != AccessOperation::Investigate => {
            (verify_complete(&text, anchor, watermark, bodies)?, true)
        }
        _ if request.operation == AccessOperation::IdentityChain => {
            return Err(FileError::Unanchored);
        }
        _ => (verify_export_subset(&text)?, false),
    };
    write_export(
        request,
        &rows,
        intents,
        Some(watermark),
        report,
        complete,
        dir,
    )
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
    let report = verify_complete(&joined(&rows), Anchor::Genesis, after, false)?;
    write_export(&request, &rows, Vec::new(), Some(after), report, true, dir)
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
    report: ExportReport,
    complete: bool,
    dir: &Path,
) -> Result<ExportOutcome, FileError> {
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
