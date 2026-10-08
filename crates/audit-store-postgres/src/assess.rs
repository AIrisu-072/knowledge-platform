//! The overall offline verdict of an export directory (design §8,
//! `audit-admin assess`). No database connection.
//!
//! The export written by `audit-admin export` (`export.jsonl` and
//! `manifest.json`) is verified again with audit-core: the manifest only
//! says how it was produced (operation, first `seq_after`, watermark,
//! anchored or filtered); nothing it claims about the result is trusted. The
//! anchor is genesis, or an out-of-band checkpoint at the export's first
//! `seq_after` (never the manifest's own checkpoint). The verified report is
//! then judged by `audit_core::assess_recovery` against the out-of-band
//! checkpoint (`kp-audit-checkpoint-v1`) and the out-of-band recovery
//! records (`kp-audit-recovery-records-v1`).
//!
//! The report is bounded: verdict codes, seqs, epochs, chain values and
//! counts, never an envelope. A chain that fails verification is reported as
//! `broken` with the first offending line.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use audit_core::{
    Anchor, ChainIntegrity, ChainVerdict, Checkpoint, ExportReport, RecoveryAssessment,
    RecoveryRecord, assess_recovery, verify_export_subset,
};
use serde::{Deserialize, Serialize};

use crate::files::{
    EXPORT_FILE, FileError, MANIFEST_FILE, MANIFEST_FORMAT, ManifestHead, comparison_name,
    verify_chain_export,
};
use crate::hex;

/// Bound on the epoch transitions listed in the report (all are counted).
pub const MAX_LISTED_EPOCHS: usize = 64;

/// The trusted inputs kept outside the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssessInputs {
    /// The out-of-band checkpoint the export is judged against.
    pub checkpoint: Checkpoint,
    /// A trusted out-of-band checkpoint at the export's first `seq_after`,
    /// for an export that starts after genesis.
    pub anchor: Option<Checkpoint>,
    /// The out-of-band recovery records.
    pub records: Vec<RecoveryRecord>,
}

#[derive(Debug, thiserror::Error)]
pub enum AssessError {
    #[error("file error: {0}")]
    Io(#[from] io::Error),
    /// The export starts after genesis and no out-of-band checkpoint sits
    /// at its first `seq_after`.
    #[error("the export starts after seq {seq}: pass the out-of-band checkpoint at that seq")]
    AnchorRequired { seq: i64 },
}

/// The overall verdict: audit-core's, a chain that failed verification, or
/// a checkpoint the export cannot be judged against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overall {
    Chain(ChainVerdict),
    Broken,
    /// The checkpoint lies past the export's last seq in the same (or a
    /// later) recovery epoch: the export is older or cut short
    /// (`--seq-through`), or the Store lost rows. The export can neither
    /// confirm nor refute it: assess with a checkpoint at or before the
    /// export's last seq, or export again through the checkpoint (a fresh
    /// full export that still ends before it means the Store lost rows).
    StoreBehind,
}

/// How a verdict is acted on (the `audit-admin assess` exit status).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssessClass {
    /// `Authentic`: exit 0.
    Authentic,
    /// Not authenticated, nothing contradicts the out-of-band records
    /// beyond what a human must review (`AuthenticThrough`,
    /// `UnverifiedExpiry`, `NoCheckpoint`, `Lost`, `UnverifiedRecovery`):
    /// exit 4.
    Review,
    /// `Tampered`, `Unanchored` or a broken chain: exit 5.
    Rejected,
    /// The inputs do not fit together (`store_behind`): exit 2, like a usage
    /// error. Never authentic.
    Usage,
}

impl Overall {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chain(ChainVerdict::Authentic) => "authentic",
            Self::Chain(ChainVerdict::AuthenticThrough { .. }) => "authentic_through",
            Self::Chain(ChainVerdict::UnverifiedExpiry) => "unverified_expiry",
            Self::Chain(ChainVerdict::NoCheckpoint) => "no_checkpoint",
            Self::Chain(ChainVerdict::Lost) => "lost",
            Self::Chain(ChainVerdict::UnverifiedRecovery) => "unverified_recovery",
            Self::Chain(ChainVerdict::Tampered) => "tampered",
            Self::Chain(ChainVerdict::Unanchored) => "unanchored",
            Self::Broken => "broken",
            Self::StoreBehind => "store_behind",
        }
    }

    pub const fn class(self) -> AssessClass {
        match self {
            Self::Chain(ChainVerdict::Authentic) => AssessClass::Authentic,
            Self::Chain(
                ChainVerdict::AuthenticThrough { .. }
                | ChainVerdict::UnverifiedExpiry
                | ChainVerdict::NoCheckpoint
                | ChainVerdict::Lost
                | ChainVerdict::UnverifiedRecovery,
            ) => AssessClass::Review,
            Self::Chain(ChainVerdict::Tampered | ChainVerdict::Unanchored) | Self::Broken => {
                AssessClass::Rejected
            }
            Self::StoreBehind => AssessClass::Usage,
        }
    }
}

/// One recovery epoch along the verified path, for human review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EpochSummary {
    /// The first row of `new_epoch`.
    pub seq: i64,
    pub old_epoch: i64,
    pub new_epoch: i64,
    /// Whether an out-of-band recovery record explains the transition.
    pub recorded: bool,
    pub restored_head_verified: bool,
    /// The lost range `(restored_head_seq, lost_upper]` of the record (else
    /// of the attesting `audit.recovery.epoch_started` body).
    pub restored_head_seq: Option<i64>,
    pub lost_upper: Option<i64>,
    /// False when the bound is unknown (`lost_upper` is then the restored
    /// head and the epoch is never authentic).
    pub lost_upper_known: Option<bool>,
    /// `restore` / `planned_move` / `regression` (body exports only).
    pub classification: Option<&'static str>,
}

/// The bounded verdict printed by `audit-admin assess`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssessReport {
    #[serde(skip)]
    pub overall: Overall,
    pub verdict: &'static str,
    /// The highest seq an out-of-band checkpoint confirms (`Match` or
    /// `Ahead`).
    pub authenticated_through: Option<i64>,
    /// `intact` / `unanchored` / `broken` (`audit_core::ChainIntegrity`).
    pub chain_integrity: &'static str,
    /// The first offending line of a broken chain (positions only).
    pub error: Option<String>,
    pub operation: Option<String>,
    pub anchor_seq: Option<i64>,
    /// The verified head (anchored exports).
    pub head: Option<ManifestHead>,
    pub rows: u64,
    /// Verified up to the watermark with every expiry evidence inside it.
    pub complete: bool,
    /// Checkpoint findings by comparison (`match`, `ahead`, `mismatch`,
    /// `epoch_mismatch`, `store_behind`, `before_anchor`).
    pub findings: BTreeMap<&'static str, u64>,
    /// Findings that confirm nothing (a checkpoint before or at the anchor).
    pub findings_neutral: u64,
    pub epochs_total: u64,
    pub epochs_unrecorded: u64,
    /// At most [`MAX_LISTED_EPOCHS`] transitions, in seq order.
    pub epochs: Vec<EpochSummary>,
    pub unmatched_records: u64,
    pub records_before_anchor: u64,
    pub records_after_head: u64,
    pub unconfirmed_expiries: u64,
    pub unverified_expiry_evidence: u64,
    pub expired_after_watermark: u64,
}

impl AssessReport {
    pub fn class(&self) -> AssessClass {
        self.overall.class()
    }

    fn empty(overall: Overall, integrity: &'static str, operation: Option<String>) -> Self {
        Self {
            overall,
            verdict: overall.as_str(),
            authenticated_through: None,
            chain_integrity: integrity,
            error: None,
            operation,
            anchor_seq: None,
            head: None,
            rows: 0,
            complete: false,
            findings: BTreeMap::new(),
            findings_neutral: 0,
            epochs_total: 0,
            epochs_unrecorded: 0,
            epochs: Vec::new(),
            unmatched_records: 0,
            records_before_anchor: 0,
            records_after_head: 0,
            unconfirmed_expiries: 0,
            unverified_expiry_evidence: 0,
            expired_after_watermark: 0,
        }
    }

    fn broken(error: impl Into<String>, operation: Option<String>) -> Self {
        let mut report = Self::empty(Overall::Broken, "broken", operation);
        report.error = Some(error.into());
        report
    }

    fn unanchored(report: &ExportReport, operation: Option<String>) -> Self {
        let mut out = Self::empty(
            Overall::Chain(ChainVerdict::Unanchored),
            ChainIntegrity::of_report(report).as_str(),
            operation,
        );
        out.rows = report.rows;
        out.unverified_expiry_evidence = report.unverified_expiry_evidence;
        out
    }
}

/// The manifest members that say how the export was produced.
#[derive(Deserialize)]
struct ManifestInput {
    format: String,
    operation: String,
    intents: Vec<IntentInput>,
    watermark: Option<i64>,
    rows: u64,
    anchored: bool,
    head: Option<HeadInput>,
}

#[derive(Deserialize)]
struct IntentInput {
    seq_after: i64,
}

#[derive(Deserialize)]
struct HeadInput {
    epoch: i64,
    seq: i64,
    chain: String,
}

/// Verifies the export directory `dir` again and assesses it against the
/// out-of-band inputs. I/O errors and a missing anchor are errors; anything
/// wrong with the files' content is a `broken` report.
pub fn assess_dir(dir: &Path, inputs: &AssessInputs) -> Result<AssessReport, AssessError> {
    let manifest_text = std::fs::read_to_string(dir.join(MANIFEST_FILE))?;
    let export_text = std::fs::read_to_string(dir.join(EXPORT_FILE))?;
    let Some(manifest) = serde_json::from_str::<ManifestInput>(&manifest_text)
        .ok()
        .filter(|manifest| manifest.format == MANIFEST_FORMAT)
    else {
        return Ok(AssessReport::broken("manifest is malformed", None));
    };
    let operation = Some(manifest.operation.clone());
    let bodies = match manifest.operation.as_str() {
        "export" | "verify" => true,
        "identity_chain" => false,
        _ => {
            return Ok(AssessReport::broken(
                "manifest names an unknown operation",
                None,
            ));
        }
    };
    if !manifest.anchored {
        // A filtered subset proves nothing about the chain.
        return Ok(match verify_export_subset(&export_text) {
            Ok(report) if bodies => AssessReport::unanchored(&report, operation),
            Ok(_) => AssessReport::broken("an identity chain is never a subset", operation),
            Err(error) => AssessReport::broken(error.to_string(), operation),
        });
    }
    let start = manifest
        .intents
        .first()
        .map_or(0, |intent| intent.seq_after);
    let anchor = if start == 0 {
        Anchor::Genesis
    } else {
        match [inputs.anchor, Some(inputs.checkpoint)]
            .into_iter()
            .flatten()
            .find(|checkpoint| checkpoint.seq == start)
        {
            Some(checkpoint) => Anchor::Checkpoint(checkpoint),
            None => return Err(AssessError::AnchorRequired { seq: start }),
        }
    };
    let Some(watermark) = manifest.watermark else {
        return Ok(AssessReport::broken("manifest has no watermark", operation));
    };
    let check = match verify_chain_export(&export_text, anchor, watermark, bodies) {
        Ok(check) => check,
        Err(FileError::Verification(error)) => {
            return Ok(AssessReport::broken(error.to_string(), operation));
        }
        Err(other) => return Ok(AssessReport::broken(other.to_string(), operation)),
    };
    let report = &check.report;
    let head = ManifestHead {
        epoch: report.head.epoch,
        seq: report.head.seq,
        chain: hex::encode(&report.head.chain),
    };
    let claimed_head = manifest
        .head
        .as_ref()
        .map(|h| (h.epoch, h.seq, h.chain.as_str()));
    if manifest.rows != report.rows
        || claimed_head
            .is_some_and(|claimed| claimed != (head.epoch, head.seq, head.chain.as_str()))
    {
        return Ok(AssessReport::broken(
            "manifest does not match the export",
            operation,
        ));
    }
    let assessment = assess_recovery(report, &[inputs.checkpoint], &inputs.records);
    let mut out = summarize(&assessment, report, operation);
    // A checkpoint past the head with no recovery after it on this path
    // cannot be judged from this export (audit-core would call it
    // tampering): fail closed as a usage outcome instead. A checkpoint of an
    // earlier epoch stays audit-core's (lost, unverified recovery or
    // tampered).
    if inputs.checkpoint.seq > report.head.seq && inputs.checkpoint.epoch >= report.head.epoch {
        out.overall = Overall::StoreBehind;
        out.verdict = Overall::StoreBehind.as_str();
        out.authenticated_through = None;
    }
    out.anchor_seq = report.anchor.map(|anchor| anchor.seq);
    out.head = Some(head);
    out.complete = check.complete;
    out.expired_after_watermark = check.expired_after_watermark;
    Ok(out)
}

fn summarize(
    assessment: &RecoveryAssessment,
    report: &ExportReport,
    operation: Option<String>,
) -> AssessReport {
    let overall = Overall::Chain(assessment.verdict);
    let mut out = AssessReport::empty(
        overall,
        ChainIntegrity::of_report(report).as_str(),
        operation,
    );
    out.authenticated_through = assessment.authenticated_through;
    out.rows = report.rows;
    for finding in &assessment.findings {
        *out.findings
            .entry(comparison_name(finding.comparison))
            .or_insert(0) += 1;
        if finding.verdict == ChainVerdict::NoCheckpoint {
            out.findings_neutral += 1;
        }
    }
    out.epochs_total = assessment.epochs.len() as u64;
    out.epochs_unrecorded = assessment
        .epochs
        .iter()
        .filter(|epoch| epoch.record.is_none())
        .count() as u64;
    out.epochs = assessment
        .epochs
        .iter()
        .take(MAX_LISTED_EPOCHS)
        .map(|epoch| {
            let attestation = epoch.transition.attestation;
            EpochSummary {
                seq: epoch.transition.seq,
                old_epoch: epoch.transition.old_epoch,
                new_epoch: epoch.transition.new_epoch,
                recorded: epoch.record.is_some(),
                restored_head_verified: epoch.restored_head_verified,
                restored_head_seq: epoch
                    .record
                    .map(|record| record.restored_head_seq)
                    .or(attestation.map(|body| body.restored_head_seq)),
                lost_upper: epoch
                    .record
                    .map(|record| record.lost_upper)
                    .or(attestation.map(|body| body.lost_upper_seq)),
                lost_upper_known: epoch
                    .record
                    .map(|record| record.lost_upper_known)
                    .or(attestation.map(|body| body.lost_upper_known)),
                classification: attestation.map(|body| body.classification.as_str()),
            }
        })
        .collect();
    out.unmatched_records = assessment.unmatched_records.len() as u64;
    out.records_before_anchor = assessment.records_before_anchor.len() as u64;
    out.records_after_head = assessment.records_after_head.len() as u64;
    out.unconfirmed_expiries = assessment.unconfirmed_expiries;
    out.unverified_expiry_evidence = report.unverified_expiry_evidence;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{RecoveryRecordLine, parse_recovery_records, write_private};
    use audit_core::{GENESIS, chain_next, envelope_digest};
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn verdict_names_and_classes() {
        let cases = [
            (ChainVerdict::Authentic, "authentic", AssessClass::Authentic),
            (
                ChainVerdict::AuthenticThrough { seq: 3 },
                "authentic_through",
                AssessClass::Review,
            ),
            (
                ChainVerdict::UnverifiedExpiry,
                "unverified_expiry",
                AssessClass::Review,
            ),
            (
                ChainVerdict::NoCheckpoint,
                "no_checkpoint",
                AssessClass::Review,
            ),
            (ChainVerdict::Lost, "lost", AssessClass::Review),
            (
                ChainVerdict::UnverifiedRecovery,
                "unverified_recovery",
                AssessClass::Review,
            ),
            (ChainVerdict::Tampered, "tampered", AssessClass::Rejected),
            (
                ChainVerdict::Unanchored,
                "unanchored",
                AssessClass::Rejected,
            ),
        ];
        for (verdict, name, class) in cases {
            assert_eq!(Overall::Chain(verdict).as_str(), name);
            assert_eq!(Overall::Chain(verdict).class(), class, "{name}");
        }
        assert_eq!(Overall::Broken.as_str(), "broken");
        assert_eq!(Overall::Broken.class(), AssessClass::Rejected);
        assert_eq!(Overall::StoreBehind.as_str(), "store_behind");
        assert_eq!(Overall::StoreBehind.class(), AssessClass::Usage);
    }

    #[test]
    fn recovery_records_round_trip_and_refuse_malformed_lines() {
        let record = RecoveryRecord {
            old_epoch: 1,
            new_epoch: 2,
            restored_head_seq: 5,
            restored_head_chain: [0xab; 32],
            lost_upper: 7,
            lost_upper_known: true,
        };
        let line = RecoveryRecordLine::from_record(&record).to_line();
        assert_eq!(
            line,
            format!(
                "{{\"format\":\"kp-audit-recovery-records-v1\",\"old_epoch\":1,\"new_epoch\":2,\
                 \"restored_head_seq\":5,\"restored_head_chain\":\"{}\",\"lost_upper\":7}}",
                "ab".repeat(32)
            )
        );
        let text = format!("{line}\n\n{line}\n");
        assert_eq!(
            parse_recovery_records(&text).expect("parses"),
            vec![record, record]
        );
        assert_eq!(parse_recovery_records("").expect("empty"), vec![]);
        let base = json!({
            "format": "kp-audit-recovery-records-v1", "old_epoch": 1, "new_epoch": 2,
            "restored_head_seq": 5, "restored_head_chain": "ab".repeat(32), "lost_upper": 7,
        });
        let variants = [
            ("format", json!("kp-audit-recovery-records-v2")),
            ("new_epoch", json!(3)),
            ("old_epoch", json!(0)),
            ("restored_head_seq", json!(-1)),
            ("lost_upper", json!(4)),
            ("restored_head_chain", json!("AB".repeat(32))),
            ("restored_head_chain", json!("ab")),
            ("incident", json!("x")),
        ];
        for (key, value) in variants {
            let mut bad = base.clone();
            bad[key] = value;
            let text = format!("{line}\n{bad}\n");
            assert!(
                matches!(
                    parse_recovery_records(&text),
                    Err(FileError::RecoveryRecords { line: 2 })
                ),
                "{key}"
            );
        }
        let duplicate = line.replacen("\"lost_upper\":7", "\"lost_upper\":7,\"lost_upper\":7", 1);
        assert!(parse_recovery_records(&duplicate).is_err(), "duplicate key");

        // An unknown bound is an explicit null, never a missing key.
        let unknown = RecoveryRecord {
            lost_upper: 5,
            lost_upper_known: false,
            ..record
        };
        let unknown_line = RecoveryRecordLine::from_record(&unknown).to_line();
        assert!(
            unknown_line.ends_with(",\"lost_upper\":null}"),
            "{unknown_line}"
        );
        assert_eq!(
            parse_recovery_records(&unknown_line).expect("parses"),
            vec![unknown]
        );
        let mut missing = base.clone();
        missing
            .as_object_mut()
            .expect("object")
            .remove("lost_upper");
        assert!(
            matches!(
                parse_recovery_records(&missing.to_string()),
                Err(FileError::RecoveryRecords { line: 1 })
            ),
            "a missing lost_upper is not an unknown bound"
        );
        for value in [json!("unknown"), json!(false), json!(5.5)] {
            let mut bad = base.clone();
            bad["lost_upper"] = value;
            assert!(parse_recovery_records(&bad.to_string()).is_err(), "{bad}");
        }
    }

    /// A synthetic identity-chain export of `rows` relay rows from genesis
    /// (no bodies): `(export text, chain of each seq)`.
    fn identity_chain(rows: i64) -> (String, Vec<[u8; 32]>) {
        let mut prev = GENESIS;
        let mut text = String::new();
        let mut chains = Vec::new();
        for seq in 1..=rows {
            let event_id = Uuid::from_u128(seq as u128);
            let digest = envelope_digest(&format!("{{\"n\":{seq}}}"));
            let chain = chain_next(&prev, seq, event_id, &digest);
            text.push_str(
                &json!({
                    "seq": seq, "event_id": event_id.to_string(), "origin": "relay",
                    "envelope_digest": hex::encode(&digest), "prev_chain": hex::encode(&prev),
                    "chain": hex::encode(&chain), "recovery_epoch": 1, "expired": false,
                    "expired_by_seq": null, "envelope": null,
                })
                .to_string(),
            );
            text.push('\n');
            chains.push(chain);
            prev = chain;
        }
        (text, chains)
    }

    fn write_dir(text: &str, manifest: &serde_json::Value) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("audit-assess-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(&dir).expect("dir");
        write_private(&dir.join(EXPORT_FILE), text.as_bytes()).expect("export");
        write_private(&dir.join(MANIFEST_FILE), manifest.to_string().as_bytes()).expect("manifest");
        dir
    }

    fn manifest(rows: i64, seq_after: i64, anchored: bool) -> serde_json::Value {
        json!({
            "format": MANIFEST_FORMAT, "operation": "identity_chain",
            "intents": [{"intent_seq": 99, "watermark": rows, "seq_after": seq_after,
                         "seq_through": null, "rows": rows, "page_digests": []}],
            "watermark": rows, "rows": rows - seq_after, "anchored": anchored,
        })
    }

    #[test]
    fn identity_chain_directories_are_assessed_offline() {
        let (text, chains) = identity_chain(4);
        let dir = write_dir(&text, &manifest(4, 0, true));
        let at = |seq: i64| Checkpoint {
            epoch: 1,
            seq,
            chain: chains[usize::try_from(seq - 1).expect("seq")],
        };
        let inputs = |checkpoint| AssessInputs {
            checkpoint,
            anchor: None,
            records: Vec::new(),
        };
        let report = assess_dir(&dir, &inputs(at(4))).expect("assess");
        assert_eq!(report.verdict, "authentic");
        assert_eq!(report.authenticated_through, Some(4));
        assert_eq!(report.findings, BTreeMap::from([("match", 1)]));
        assert_eq!(report.head.as_ref().map(|h| h.seq), Some(4));
        assert!(report.complete);
        let through = assess_dir(&dir, &inputs(at(2))).expect("assess");
        assert_eq!(
            (through.verdict, through.authenticated_through),
            ("authentic_through", Some(2))
        );
        // A checkpoint past the export's last seq (same epoch) cannot be
        // judged from it: never authentic, never called tampering.
        let (prefix, _) = identity_chain(3);
        let prefix_dir = write_dir(&prefix, &manifest(3, 0, true));
        let behind = assess_dir(&prefix_dir, &inputs(at(4))).expect("assess");
        assert_eq!(
            (behind.verdict, behind.class(), behind.authenticated_through),
            ("store_behind", AssessClass::Usage, None)
        );
        assert_eq!(behind.findings, BTreeMap::from([("store_behind", 1)]));
        let mut forged = at(4);
        forged.chain = [0; 32];
        let tampered = assess_dir(&dir, &inputs(forged)).expect("assess");
        assert_eq!(tampered.verdict, "tampered");
        assert_eq!(tampered.class(), AssessClass::Rejected);
        assert_eq!(tampered.findings, BTreeMap::from([("mismatch", 1)]));

        // Edit one line: the chain no longer recomputes.
        let edited = text.replacen("\"seq\":2", "\"seq\":3", 1);
        let dir = write_dir(&edited, &manifest(4, 0, true));
        let broken = assess_dir(&dir, &inputs(at(4))).expect("assess");
        assert_eq!(
            (broken.verdict, broken.chain_integrity),
            ("broken", "broken")
        );
        assert!(broken.error.is_some_and(|e| e.starts_with("export line 2")));

        // A manifest that does not match the export is broken input.
        let dir = write_dir(&text, &json!({"format": MANIFEST_FORMAT}));
        assert_eq!(
            assess_dir(&dir, &inputs(at(4))).expect("assess").verdict,
            "broken"
        );
        let mut lying = manifest(4, 0, true);
        lying["rows"] = json!(3);
        let dir = write_dir(&text, &lying);
        assert_eq!(
            assess_dir(&dir, &inputs(at(4)))
                .expect("assess")
                .error
                .as_deref(),
            Some("manifest does not match the export")
        );

        // An export after genesis needs a trusted anchor at its seq_after.
        let tail: String = text.lines().skip(2).map(|l| format!("{l}\n")).collect();
        let dir = write_dir(&tail, &manifest(4, 2, true));
        assert!(matches!(
            assess_dir(&dir, &inputs(at(4))),
            Err(AssessError::AnchorRequired { seq: 2 })
        ));
        let anchored = AssessInputs {
            checkpoint: at(4),
            anchor: Some(at(2)),
            records: Vec::new(),
        };
        let report = assess_dir(&dir, &anchored).expect("assess");
        assert_eq!((report.verdict, report.anchor_seq), ("authentic", Some(2)));
        let neutral = assess_dir(&dir, &inputs(at(2))).expect("assess");
        assert_eq!(neutral.verdict, "no_checkpoint");
        assert_eq!(neutral.findings_neutral, 1);
    }
}
