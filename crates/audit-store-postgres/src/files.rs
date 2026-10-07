//! Export, manifest and checkpoint files (design §10.3, §10.4, §11).
//!
//! Files are created new with mode 0600 and never overwritten. Their content
//! is not logged. A full export from genesis (or from a trusted checkpoint)
//! is verified with audit-core before the manifest is written; a filtered
//! export is reported as an unanchored subset.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use audit_core::{
    Anchor, Checkpoint, CheckpointComparison, ExportReport, GENESIS, compare_checkpoint,
    verify_export, verify_export_subset, verify_identity_chain,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::admin::{AccessOperation, AuditAdmin, CheckpointRecord, ExportLine};
use crate::error::AdminError;
use crate::hex;

pub const EXPORT_FILE: &str = "export.jsonl";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const MANIFEST_FORMAT: &str = "kp-audit-export-manifest-v1";
pub const CHECKPOINT_FORMAT: &str = "kp-audit-checkpoint-v1";

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error(transparent)]
    Store(#[from] AdminError),
    #[error("file error: {0}")]
    Io(#[from] io::Error),
    #[error("export verification failed: {0}")]
    Verification(#[from] audit_core::ExportError),
    #[error("invalid checkpoint file")]
    Checkpoint,
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

/// The out-of-band checkpoint record written by `audit-admin checkpoint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointFile {
    pub format: String,
    pub epoch: i64,
    pub seq: i64,
    pub chain: String,
    pub verified_seq: i64,
    pub genesis: String,
}

impl CheckpointFile {
    pub fn from_record(record: &CheckpointRecord) -> Self {
        Self {
            format: CHECKPOINT_FORMAT.to_owned(),
            epoch: record.epoch,
            seq: record.seq,
            chain: record.chain.clone(),
            verified_seq: record.verified_seq,
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

/// Export manifest (design §10.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    pub format: &'static str,
    pub operation: AccessOperation,
    pub intent_seq: Option<i64>,
    pub watermark: Option<i64>,
    pub rows: u64,
    pub first_seq: Option<i64>,
    pub last_seq: Option<i64>,
    pub page_digests: Vec<String>,
    pub genesis: String,
    pub anchored: bool,
    pub head: Option<ManifestHead>,
    pub checkpoint: Option<ManifestCheckpoint>,
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
    /// Trusted out-of-band checkpoint to anchor (when the filter starts at
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
        CheckpointComparison::StoreBehind => "store_behind",
        CheckpointComparison::Ahead => "ahead",
        CheckpointComparison::BeforeAnchor => "before_anchor",
        CheckpointComparison::Unanchored => "unanchored",
    }
}

/// Opens an intent, reads every page in READ ONLY transactions, records the
/// close, verifies offline and writes `export.jsonl` and `manifest.json`.
pub async fn export_to_dir(
    admin: &AuditAdmin,
    request: &ExportRequest,
    dir: &Path,
) -> Result<ExportOutcome, FileError> {
    let token = admin
        .open_access(
            request.operation,
            &request.filter,
            request.page_size,
            request.max_pages,
        )
        .await?;
    let pages = admin.read_all(&token).await?;
    let page_digests: Vec<String> = pages.iter().map(|page| page_digest(page)).collect();
    let rows: Vec<&ExportLine> = pages.iter().flatten().collect();
    let returned = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    admin
        .close_access(token.secret(), returned, &page_digests)
        .await?;
    write_export(
        request,
        &rows,
        page_digests,
        Some(token.intent_seq),
        Some(token.watermark),
        token.include_control,
        dir,
    )
}

/// Writes an identity-chain export from the recovery path (no intent; the
/// Store records nothing in recovery mode, design §11).
pub async fn export_identity_chain_recovery(
    admin: &AuditAdmin,
    checkpoint: Option<Checkpoint>,
    dir: &Path,
) -> Result<ExportOutcome, FileError> {
    const PAGE: i32 = 1000;
    let mut pages = Vec::new();
    let mut after = 0;
    loop {
        let page = admin.identity_chain_recovery_page(after, PAGE).await?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.seq;
        pages.push(page);
    }
    let page_digests: Vec<String> = pages.iter().map(|page| page_digest(page)).collect();
    let rows: Vec<&ExportLine> = pages.iter().flatten().collect();
    let request = ExportRequest {
        operation: AccessOperation::IdentityChain,
        filter: Value::Object(serde_json::Map::new()),
        page_size: PAGE,
        max_pages: i32::try_from(pages.len()).unwrap_or(i32::MAX),
        checkpoint,
    };
    write_export(&request, &rows, page_digests, None, None, true, dir)
}

fn write_export(
    request: &ExportRequest,
    rows: &[&ExportLine],
    page_digests: Vec<String>,
    intent_seq: Option<i64>,
    watermark: Option<i64>,
    full_visibility: bool,
    dir: &Path,
) -> Result<ExportOutcome, FileError> {
    let mut text = String::new();
    for row in rows {
        text.push_str(&row.line);
        text.push('\n');
    }
    let filter = request.filter.as_object();
    let seq_after = filter
        .and_then(|f| f.get("seq_after"))
        .and_then(Value::as_i64);
    // Contiguous only when nothing but a seq_after bound narrows the range and
    // control events are visible.
    let only_seq_after =
        full_visibility && filter.is_none_or(|f| f.keys().all(|k| k == "seq_after"));
    let anchor = match (only_seq_after, seq_after, request.checkpoint) {
        (true, None | Some(0), _) => Some(Anchor::Genesis),
        (true, Some(after), Some(checkpoint)) if checkpoint.seq == after => {
            Some(Anchor::Checkpoint(checkpoint))
        }
        _ => None,
    };
    let report = match (request.operation, anchor) {
        (AccessOperation::IdentityChain, Some(anchor)) => verify_identity_chain(&text, anchor)?,
        // An identity chain is only meaningful from genesis or a trusted checkpoint.
        (AccessOperation::IdentityChain, None) => return Err(FileError::Checkpoint),
        (_, Some(anchor)) if request.operation != AccessOperation::Investigate => {
            verify_export(&text, anchor)?
        }
        _ => verify_export_subset(&text)?,
    };
    let checkpoint = request.checkpoint.map(|c| ManifestCheckpoint {
        epoch: c.epoch,
        seq: c.seq,
        chain: hex::encode(&c.chain),
        comparison: comparison_name(compare_checkpoint(&report, &c)),
    });
    let manifest = Manifest {
        format: MANIFEST_FORMAT,
        operation: request.operation,
        intent_seq,
        watermark,
        rows: report.rows,
        first_seq: rows.first().map(|r| r.seq),
        last_seq: rows.last().map(|r| r.seq),
        page_digests,
        genesis: hex::encode(&GENESIS),
        anchored: report.anchored,
        head: report.anchored.then(|| ManifestHead {
            epoch: report.head.epoch,
            seq: report.head.seq,
            chain: hex::encode(&report.head.chain),
        }),
        checkpoint,
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
