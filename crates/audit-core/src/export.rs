//! Out-of-database verification of Store exports (design §8, §10.4).
//!
//! An export line is
//! `{"seq","event_id","origin","envelope_digest","prev_chain","chain","recovery_epoch","expired","envelope"}`
//! where `envelope` is the Store's `jsonb::text` verbatim (or `null` when the
//! body expired or for identity-chain exports). The digest is recomputed over
//! those exact bytes; nothing is re-serialized.
//!
//! Authenticity is only claimed for contiguous verification from a trusted
//! anchor (genesis or an out-of-band checkpoint) whose head then matches an
//! out-of-band checkpoint. Filtered subsets are reported as unanchored.

use std::fmt;

use serde::Deserialize;
use serde_json::value::RawValue;
use uuid::Uuid;

use crate::catalog::Origin;
use crate::chain::{GENESIS, chain_next, envelope_digest, parse_hex32};
use crate::json::parse_unique;
use crate::kinds::is_uuid;

/// A trusted chain position (stored out of band).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub epoch: i64,
    pub seq: i64,
    pub chain: [u8; 32],
}

impl Checkpoint {
    /// The position before seq 1 of a fresh Store (epoch 1).
    pub const GENESIS: Self = Self {
        epoch: 1,
        seq: 0,
        chain: GENESIS,
    };
}

/// Where contiguous verification starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Genesis,
    Checkpoint(Checkpoint),
}

impl Anchor {
    fn checkpoint(self) -> Checkpoint {
        match self {
            Self::Genesis => Checkpoint::GENESIS,
            Self::Checkpoint(checkpoint) => checkpoint,
        }
    }
}

/// Result of verifying an export.
#[derive(Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// True only for contiguous verification from a trusted anchor.
    pub anchored: bool,
    /// The trusted starting point (`None` for unanchored subsets).
    pub anchor: Option<Checkpoint>,
    /// The last verified row, or the anchor when the export is empty.
    pub head: Checkpoint,
    pub rows: u64,
    pub bodies: u64,
    pub expired: u64,
    /// Chain value of every verified row, in seq order (anchored only).
    chains: Vec<[u8; 32]>,
    /// Recovery epoch changes along the verified path (anchored only).
    transitions: Vec<EpochTransition>,
}

impl ExportReport {
    /// Recovery epoch changes observed along the verified path. Every one of
    /// them is listed for human review (design §8).
    pub fn epoch_transitions(&self) -> &[EpochTransition] {
        &self.transitions
    }
}

impl fmt::Debug for ExportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExportReport")
            .field("anchored", &self.anchored)
            .field("anchor", &self.anchor)
            .field("head", &self.head)
            .field("rows", &self.rows)
            .field("bodies", &self.bodies)
            .field("expired", &self.expired)
            .field("transitions", &self.transitions)
            .finish_non_exhaustive()
    }
}

/// A recovery epoch change observed in an export: the row at `seq` is the
/// first row of `new_epoch` (the Store's `audit.recovery.epoch_started`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochTransition {
    pub seq: i64,
    pub old_epoch: i64,
    pub new_epoch: i64,
}

/// The operator's out-of-band record of a restore (design §8, §11): appended
/// at restore time to the checkpoint log kept outside the database.
/// `restored_head` is the last surviving seq; `lost_to` is the claimed upper
/// bound of the lost range `(restored_head, lost_to]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryRecord {
    pub old_epoch: i64,
    pub new_epoch: i64,
    pub restored_head: i64,
    pub lost_to: i64,
}

impl RecoveryRecord {
    fn matches(&self, transition: &EpochTransition) -> bool {
        self.old_epoch == transition.old_epoch
            && self.new_epoch == transition.new_epoch
            && self.restored_head.checked_add(1) == Some(transition.seq)
            && self.lost_to >= self.restored_head
    }

    fn loses(&self) -> bool {
        self.lost_to > self.restored_head
    }
}

/// Overall offline verdict (design §8). Ordered from best to worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChainVerdict {
    /// Contiguous from a trusted anchor, every out-of-band checkpoint lies on
    /// the verified path, and no recovery lost rows.
    Authentic,
    /// Verified from a trusted anchor, but no out-of-band checkpoint confirms
    /// the path. Authenticity is not claimed.
    NoCheckpoint,
    /// Differences are explained only by recoveries documented out of band:
    /// reported as "lost (recovery epoch k)", never as authentic.
    Lost,
    /// A recovery epoch (or a checkpoint mismatch past a recovery) has no
    /// matching out-of-band record, or the records disagree: suspected
    /// tampering disguised as recovery.
    UnverifiedRecovery,
    /// History at or below a restored head, or without any recovery, differs
    /// from an out-of-band checkpoint.
    Tampered,
    /// A filtered subset: it proves nothing about the chain.
    Unanchored,
}

/// One recovery epoch along the verified path and its out-of-band record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochReview {
    pub transition: EpochTransition,
    pub record: Option<RecoveryRecord>,
}

/// How one out-of-band checkpoint relates to the verified path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointFinding {
    pub checkpoint: Checkpoint,
    pub comparison: CheckpointComparison,
    pub verdict: ChainVerdict,
    /// The documented recovery that explains a difference, if any.
    pub explained_by: Option<RecoveryRecord>,
}

/// The offline assessment: the verdict plus everything a human must review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryAssessment {
    pub verdict: ChainVerdict,
    pub epochs: Vec<EpochReview>,
    pub findings: Vec<CheckpointFinding>,
    /// Out-of-band records that match no epoch on the verified path.
    pub unmatched_records: Vec<RecoveryRecord>,
}

/// Assesses a verified, anchored export against the out-of-band checkpoints
/// and recovery records (design §8 "recovery epochの扱い").
///
/// - A difference at or below a documented restored head is tampering.
/// - A difference inside a documented lost range is `Lost`.
/// - A difference past an undocumented recovery epoch, an undocumented epoch,
///   or a record that matches no epoch is `UnverifiedRecovery`.
/// - Any other difference is `Tampered`.
pub fn assess_recovery(
    report: &ExportReport,
    checkpoints: &[Checkpoint],
    records: &[RecoveryRecord],
) -> RecoveryAssessment {
    if !report.anchored {
        return RecoveryAssessment {
            verdict: ChainVerdict::Unanchored,
            epochs: Vec::new(),
            findings: Vec::new(),
            unmatched_records: records.to_vec(),
        };
    }
    let epochs: Vec<EpochReview> = report
        .transitions
        .iter()
        .map(|transition| EpochReview {
            transition: *transition,
            record: records.iter().find(|r| r.matches(transition)).copied(),
        })
        .collect();
    let unmatched_records: Vec<RecoveryRecord> = records
        .iter()
        .filter(|record| !epochs.iter().any(|e| e.record == Some(**record)))
        .copied()
        .collect();
    let mut worst = ChainVerdict::Authentic;
    for epoch in &epochs {
        let verdict = match epoch.record {
            None => ChainVerdict::UnverifiedRecovery,
            Some(record) if record.loses() => ChainVerdict::Lost,
            Some(_) => ChainVerdict::Authentic,
        };
        worst = worst.max(verdict);
    }
    if !unmatched_records.is_empty() {
        worst = worst.max(ChainVerdict::UnverifiedRecovery);
    }
    let mut confirmed = false;
    let findings: Vec<CheckpointFinding> = checkpoints
        .iter()
        .map(|checkpoint| {
            let comparison = compare_checkpoint(report, checkpoint);
            let (verdict, explained_by) = match comparison {
                CheckpointComparison::Match | CheckpointComparison::Ahead => {
                    confirmed = true;
                    (ChainVerdict::Authentic, None)
                }
                CheckpointComparison::BeforeAnchor => (ChainVerdict::NoCheckpoint, None),
                CheckpointComparison::Unanchored => (ChainVerdict::Unanchored, None),
                CheckpointComparison::Mismatch | CheckpointComparison::StoreBehind => {
                    classify_difference(checkpoint, &epochs)
                }
            };
            worst = worst.max(verdict);
            CheckpointFinding {
                checkpoint: *checkpoint,
                comparison,
                verdict,
                explained_by,
            }
        })
        .collect();
    if !confirmed {
        worst = worst.max(ChainVerdict::NoCheckpoint);
    }
    RecoveryAssessment {
        verdict: worst,
        epochs,
        findings,
        unmatched_records,
    }
}

fn classify_difference(
    checkpoint: &Checkpoint,
    epochs: &[EpochReview],
) -> (ChainVerdict, Option<RecoveryRecord>) {
    // Recoveries after the checkpoint was taken could have replaced it.
    let later = || {
        epochs
            .iter()
            .filter(|e| e.transition.old_epoch >= checkpoint.epoch)
    };
    let mut disagrees = false;
    for epoch in later() {
        match epoch.record {
            // History that survived a documented restore must still match.
            Some(record) if checkpoint.seq <= record.restored_head => {
                return (ChainVerdict::Tampered, None);
            }
            Some(record) if checkpoint.seq <= record.lost_to => {
                return (ChainVerdict::Lost, Some(record));
            }
            // The record's lost range does not cover the checkpoint.
            Some(_) => disagrees = true,
            None if checkpoint.seq >= epoch.transition.seq => disagrees = true,
            None => {}
        }
    }
    if disagrees {
        (ChainVerdict::UnverifiedRecovery, None)
    } else {
        (ChainVerdict::Tampered, None)
    }
}

/// Comparison of a verified export with an out-of-band checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointComparison {
    /// The head equals the checkpoint.
    Match,
    /// The chain at the checkpoint's seq differs: rewritten history.
    Mismatch,
    /// The checkpoint is beyond the verified head: rows were lost or the
    /// Store was restored to an older state.
    StoreBehind,
    /// The verified path passes through the checkpoint and continues past it.
    Ahead,
    /// The checkpoint precedes the verification anchor; verify from an
    /// earlier anchor to compare.
    BeforeAnchor,
    /// The report is a filtered subset; it proves nothing about the chain.
    Unanchored,
}

/// Why an export failed verification. Only positions are reported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    #[error("export line {line}: malformed")]
    Malformed { line: usize },
    #[error("export line {line}: expected seq {expected}, found {found}")]
    SeqGap {
        line: usize,
        expected: i64,
        found: i64,
    },
    #[error("export line {line}: prev_chain does not link to the previous chain")]
    BrokenLink { line: usize },
    #[error("export line {line}: chain recomputation mismatch")]
    ChainMismatch { line: usize },
    #[error("export line {line}: envelope digest mismatch")]
    DigestMismatch { line: usize },
    #[error("export line {line}: envelope id differs from event_id")]
    EventIdMismatch { line: usize },
    #[error("export line {line}: body presence contradicts the expiry flag")]
    BodyState { line: usize },
    #[error("export line {line}: recovery epoch regressed")]
    EpochRegressed { line: usize },
}

/// Verifies a full (body-carrying) export contiguously from `start`.
pub fn verify_export(text: &str, start: Anchor) -> Result<ExportReport, ExportError> {
    verify(text, Some(start), Mode::Bodies)
}

/// Verifies an identity-chain export (`envelope` always null) from `start`.
pub fn verify_identity_chain(text: &str, start: Anchor) -> Result<ExportReport, ExportError> {
    verify(text, Some(start), Mode::IdentityChain)
}

/// Checks a filtered export row by row (digest, id, self-consistent chain
/// step, increasing seq). The result is never anchored.
pub fn verify_export_subset(text: &str) -> Result<ExportReport, ExportError> {
    verify(text, None, Mode::Bodies)
}

/// Compares a verified report with an out-of-band checkpoint.
pub fn compare_checkpoint(report: &ExportReport, checkpoint: &Checkpoint) -> CheckpointComparison {
    let Some(anchor) = report.anchor.filter(|_| report.anchored) else {
        return CheckpointComparison::Unanchored;
    };
    if checkpoint.seq > report.head.seq {
        return CheckpointComparison::StoreBehind;
    }
    if checkpoint.seq < anchor.seq {
        return CheckpointComparison::BeforeAnchor;
    }
    let chain_at = if checkpoint.seq == anchor.seq {
        Some(anchor.chain)
    } else {
        usize::try_from(checkpoint.seq - anchor.seq - 1)
            .ok()
            .and_then(|index| report.chains.get(index).copied())
    };
    match chain_at {
        Some(chain) if chain == checkpoint.chain => {
            if checkpoint.seq == report.head.seq {
                CheckpointComparison::Match
            } else {
                CheckpointComparison::Ahead
            }
        }
        _ => CheckpointComparison::Mismatch,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Bodies,
    IdentityChain,
}

const LINE_KEYS: [&str; 9] = [
    "chain",
    "envelope",
    "envelope_digest",
    "event_id",
    "expired",
    "origin",
    "prev_chain",
    "recovery_epoch",
    "seq",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLine<'a> {
    seq: i64,
    event_id: String,
    origin: String,
    envelope_digest: String,
    prev_chain: String,
    chain: String,
    recovery_epoch: i64,
    expired: bool,
    #[serde(borrow)]
    envelope: Option<&'a RawValue>,
}

struct Line<'a> {
    seq: i64,
    event_id: Uuid,
    digest: [u8; 32],
    prev_chain: [u8; 32],
    chain: [u8; 32],
    epoch: i64,
    expired: bool,
    body: Option<&'a str>,
    body_id: Option<String>,
}

fn parse_line(text: &str) -> Option<Line<'_>> {
    // First pass: closed key set and duplicate keys at any depth.
    let value = parse_unique(text).ok()?;
    let object = value.as_object()?;
    if object.len() != LINE_KEYS.len() || !LINE_KEYS.iter().all(|key| object.contains_key(*key)) {
        return None;
    }
    // Second pass: borrow the envelope text verbatim.
    let raw: RawLine<'_> = serde_json::from_str(text).ok()?;
    Origin::parse(&raw.origin)?;
    if raw.seq < 1 || raw.recovery_epoch < 1 || !is_uuid(&raw.event_id) {
        return None;
    }
    let body = match (&object["envelope"], raw.envelope) {
        (serde_json::Value::Null, _) => None,
        (serde_json::Value::Object(_), Some(raw)) => Some(raw.get()),
        _ => return None,
    };
    Some(Line {
        seq: raw.seq,
        event_id: Uuid::parse_str(&raw.event_id).ok()?,
        digest: parse_hex32(&raw.envelope_digest)?,
        prev_chain: parse_hex32(&raw.prev_chain)?,
        chain: parse_hex32(&raw.chain)?,
        epoch: raw.recovery_epoch,
        expired: raw.expired,
        body,
        body_id: object["envelope"]
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}

fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .collect()
}

fn verify(text: &str, start: Option<Anchor>, mode: Mode) -> Result<ExportReport, ExportError> {
    let anchor = start.map(Anchor::checkpoint);
    let anchored = anchor.is_some();
    let mut head = anchor.unwrap_or(Checkpoint {
        epoch: 1,
        seq: 0,
        chain: GENESIS,
    });
    let mut report = ExportReport {
        anchored,
        anchor,
        head,
        rows: 0,
        bodies: 0,
        expired: 0,
        chains: Vec::new(),
        transitions: Vec::new(),
    };
    for (index, text) in split_lines(text).into_iter().enumerate() {
        let number = index + 1;
        let line = parse_line(text).ok_or(ExportError::Malformed { line: number })?;
        if anchored {
            let expected = head
                .seq
                .checked_add(1)
                .ok_or(ExportError::Malformed { line: number })?;
            if line.seq != expected {
                return Err(ExportError::SeqGap {
                    line: number,
                    expected,
                    found: line.seq,
                });
            }
            if line.prev_chain != head.chain {
                return Err(ExportError::BrokenLink { line: number });
            }
            if line.epoch < head.epoch {
                return Err(ExportError::EpochRegressed { line: number });
            }
            if line.epoch > head.epoch {
                report.transitions.push(EpochTransition {
                    seq: line.seq,
                    old_epoch: head.epoch,
                    new_epoch: line.epoch,
                });
            }
        } else if report.rows > 0 && line.seq <= head.seq {
            return Err(ExportError::SeqGap {
                line: number,
                expected: head.seq + 1,
                found: line.seq,
            });
        }
        if chain_next(&line.prev_chain, line.seq, line.event_id, &line.digest) != line.chain {
            return Err(ExportError::ChainMismatch { line: number });
        }
        match (mode, line.expired, line.body) {
            (Mode::Bodies, false, Some(body)) => {
                if envelope_digest(body) != line.digest {
                    return Err(ExportError::DigestMismatch { line: number });
                }
                if line.body_id.as_deref() != Some(line.event_id.to_string().as_str()) {
                    return Err(ExportError::EventIdMismatch { line: number });
                }
                report.bodies += 1;
            }
            (Mode::Bodies, true, None) | (Mode::IdentityChain, _, None) => {}
            _ => return Err(ExportError::BodyState { line: number }),
        }
        if line.expired {
            report.expired += 1;
        }
        report.rows += 1;
        head = Checkpoint {
            epoch: line.epoch,
            seq: line.seq,
            chain: line.chain,
        };
        if anchored {
            report.chains.push(line.chain);
        }
    }
    report.head = head;
    Ok(report)
}
