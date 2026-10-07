//! Out-of-database verification of Store exports (design §8, §9, §10.4, §11).
//!
//! An export line is
//! `{"seq","event_id","origin","envelope_digest","prev_chain","chain","recovery_epoch","expired","expired_by_seq","envelope"}`
//! where `envelope` is the Store's `jsonb::text` verbatim (or `null` when the
//! body expired or for identity-chain exports) and `expired_by_seq` is the
//! seq of the control event that removed the body (`null` exactly when
//! `expired` is false). The digest is recomputed over the exact body bytes;
//! nothing is re-serialized.
//!
//! Authenticity is only claimed for contiguous verification from a trusted
//! anchor (genesis or an out-of-band checkpoint) whose head then matches an
//! out-of-band checkpoint. Filtered subsets are reported as unanchored.
//! `origin`, `recovery_epoch`, `expired` and `expired_by_seq` are not part of
//! the chain; in anchored body exports they are attested by chained bodies
//! (origin against the body's catalog type and source, expiry against
//! `audit.retention.expired` / `audit.body.purged`, epoch changes against
//! `audit.recovery.epoch_started`). Identity-chain exports cannot attest them.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde_json::Value;
use serde_json::value::RawValue;
use uuid::Uuid;

use crate::catalog::{
    AUDIT_RELAY_SOURCE, AUDIT_STORE_SOURCE, CONTROL_TYPE_PREFIX, Catalog, Origin,
};
use crate::chain::{GENESIS, chain_next, envelope_digest, expired_set_digest, parse_hex32};
use crate::json::parse_unique;
use crate::kinds::is_uuid;

const RETENTION_EXPIRED: &str = "audit.retention.expired";
const BODY_PURGED: &str = "audit.body.purged";
const EPOCH_STARTED: &str = "audit.recovery.epoch_started";

/// A trusted chain position (stored out of band). `epoch` is the
/// `recovery_epoch` of the row at `seq`.
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
    /// Expired rows whose expiry evidence could not be verified offline: the
    /// evidence seq lies past the exported range, the evidence's referenced
    /// set reaches before the anchor, or the export carries no bodies
    /// (identity chain, unanchored subset). Never silently zero.
    pub unverified_expiry_evidence: u64,
    /// Whether `recovery_epoch` values are attested: true only for anchored
    /// body exports, where every epoch change landed on a verified
    /// `audit.recovery.epoch_started` body. Identity-chain exports and
    /// subsets report epochs as unauthenticated.
    pub epochs_authenticated: bool,
    /// Chain value of every verified row, in seq order (anchored only).
    chains: Vec<[u8; 32]>,
    /// Recovery epoch changes along the verified path (anchored only).
    transitions: Vec<EpochTransition>,
}

impl ExportReport {
    /// Recovery epoch changes observed along the verified path, as
    /// `(first seq, old epoch, new epoch)`. Every one of them is listed for
    /// human review (design §8).
    pub fn epoch_transitions(&self) -> &[EpochTransition] {
        &self.transitions
    }

    /// The recovery epoch of the row at `seq` (the anchor's epoch at the
    /// anchor seq), or `None` outside the anchored, verified range.
    pub fn epoch_at(&self, seq: i64) -> Option<i64> {
        let anchor = self.anchor.filter(|_| self.anchored)?;
        if seq < anchor.seq || seq > self.head.seq {
            return None;
        }
        Some(
            self.transitions
                .iter()
                .rev()
                .find(|t| t.seq <= seq)
                .map_or(anchor.epoch, |t| t.new_epoch),
        )
    }

    /// The chain value at `seq` (the anchor's chain at the anchor seq), or
    /// `None` outside the anchored, verified range.
    pub fn chain_at(&self, seq: i64) -> Option<[u8; 32]> {
        let anchor = self.anchor.filter(|_| self.anchored)?;
        if seq == anchor.seq {
            return Some(anchor.chain);
        }
        let index = seq.checked_sub(anchor.seq)?.checked_sub(1)?;
        usize::try_from(index)
            .ok()
            .and_then(|index| self.chains.get(index).copied())
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
            .field(
                "unverified_expiry_evidence",
                &self.unverified_expiry_evidence,
            )
            .field("epochs_authenticated", &self.epochs_authenticated)
            .field("transitions", &self.transitions)
            .finish_non_exhaustive()
    }
}

/// A recovery epoch change observed in an export: the row at `seq` is the
/// first row of `new_epoch` (in body exports, the Store's
/// `audit.recovery.epoch_started`). `new_epoch` is always `old_epoch + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochTransition {
    pub seq: i64,
    pub old_epoch: i64,
    pub new_epoch: i64,
}

/// The operator's out-of-band record of an epoch transition (design §8,
/// §11), appended to the checkpoint log kept outside the database.
/// `restored_head_seq` / `restored_head_chain` name the last surviving row;
/// `lost_upper` is the claimed upper bound of the lost range
/// `(restored_head_seq, lost_upper]` (equal to `restored_head_seq` for a
/// planned move, which loses nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryRecord {
    pub old_epoch: i64,
    pub new_epoch: i64,
    pub restored_head_seq: i64,
    pub restored_head_chain: [u8; 32],
    pub lost_upper: i64,
}

impl RecoveryRecord {
    /// Whether this record describes `transition` on the verified path. The
    /// restored head must precede the transition, and when it lies on the
    /// verified path its chain must match.
    fn matches(&self, transition: &EpochTransition, report: &ExportReport) -> bool {
        self.old_epoch == transition.old_epoch
            && self.new_epoch == transition.new_epoch
            && self.restored_head_seq < transition.seq
            && self.lost_upper >= self.restored_head_seq
            && report
                .chain_at(self.restored_head_seq)
                .is_none_or(|chain| chain == self.restored_head_chain)
    }

    fn loses(&self) -> bool {
        self.lost_upper > self.restored_head_seq
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
    /// matching out-of-band record, the records disagree, or a checkpoint's
    /// epoch disagrees with the export: suspected tampering disguised as
    /// recovery.
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
///   a record that matches no epoch, or a checkpoint whose chain matches but
///   whose epoch differs is `UnverifiedRecovery`.
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
            record: records
                .iter()
                .find(|r| r.matches(transition, report))
                .copied(),
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
                CheckpointComparison::EpochMismatch => (ChainVerdict::UnverifiedRecovery, None),
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
            Some(record) if checkpoint.seq <= record.restored_head_seq => {
                return (ChainVerdict::Tampered, None);
            }
            Some(record) if checkpoint.seq <= record.lost_upper => {
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
    /// The head equals the checkpoint (seq, chain and epoch).
    Match,
    /// The chain at the checkpoint's seq differs: rewritten history.
    Mismatch,
    /// The chain at the checkpoint's seq matches but the export's recovery
    /// epoch at that seq differs from the checkpoint's epoch: the epoch
    /// metadata or the checkpoint log was altered.
    EpochMismatch,
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
    #[error("export line {line}: origin does not match the body's type and source")]
    OriginMismatch { line: usize },
    #[error("export line {line}: only relay-origin events can expire")]
    ExpiryOnControlEvent { line: usize },
    #[error("export line {line}: expired_by_seq names no valid later evidence row")]
    ExpiryEvidenceMissing { line: usize },
    #[error("export line {line}: expiry evidence does not match the rows it expired")]
    ExpiryEvidenceMismatch { line: usize },
    #[error("export line {line}: recovery epoch regressed")]
    EpochRegressed { line: usize },
    #[error("export line {line}: recovery epoch increased by more than one")]
    EpochSkipped { line: usize },
    #[error("export line {line}: epoch change without a matching epoch_started record")]
    UnattestedEpochChange { line: usize },
}

/// Verifies a full (body-carrying) export contiguously from `start`.
pub fn verify_export(text: &str, start: Anchor) -> Result<ExportReport, ExportError> {
    verify(text, Some(start), Mode::Bodies)
}

/// Verifies an identity-chain export (`envelope` always null) from `start`.
/// Epochs and expiry are reported as unauthenticated.
pub fn verify_identity_chain(text: &str, start: Anchor) -> Result<ExportReport, ExportError> {
    verify(text, Some(start), Mode::IdentityChain)
}

/// Checks a filtered export row by row (digest, id, origin, self-consistent
/// chain step, increasing seq). The result is never anchored.
pub fn verify_export_subset(text: &str) -> Result<ExportReport, ExportError> {
    verify(text, None, Mode::Bodies)
}

/// Compares a verified report with an out-of-band checkpoint, including its
/// epoch.
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
    match report.chain_at(checkpoint.seq) {
        Some(chain) if chain == checkpoint.chain => {
            if report.epoch_at(checkpoint.seq) != Some(checkpoint.epoch) {
                CheckpointComparison::EpochMismatch
            } else if checkpoint.seq == report.head.seq {
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

const LINE_KEYS: [&str; 10] = [
    "chain",
    "envelope",
    "envelope_digest",
    "event_id",
    "expired",
    "expired_by_seq",
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
    expired_by_seq: Option<i64>,
    #[serde(borrow)]
    envelope: Option<&'a RawValue>,
}

/// Details of the control bodies the verifier interprets. Members that are
/// missing or mistyped are `None` and fail the corresponding check.
enum Evidence {
    Retention {
        count: Option<i64>,
        first_seq: Option<Option<i64>>,
        last_seq: Option<Option<i64>>,
        digest: Option<[u8; 32]>,
    },
    Purge {
        target_seq: Option<i64>,
        target_event_id: Option<Uuid>,
    },
    EpochStarted {
        old_epoch: Option<i64>,
        new_epoch: Option<i64>,
        restored_head_seq: Option<i64>,
        restored_head_chain: Option<[u8; 32]>,
    },
}

struct Body<'a> {
    text: &'a str,
    id: Option<String>,
    event_type: Option<String>,
    source: Option<String>,
    evidence: Option<Evidence>,
}

struct Line<'a> {
    seq: i64,
    event_id: Uuid,
    origin: Origin,
    digest: [u8; 32],
    prev_chain: [u8; 32],
    chain: [u8; 32],
    epoch: i64,
    expired_by_seq: Option<i64>,
    body: Option<Body<'a>>,
}

fn nullable_int(value: Option<&Value>) -> Option<Option<i64>> {
    match value? {
        Value::Null => Some(None),
        other => other.as_i64().map(Some),
    }
}

fn hex(value: Option<&Value>) -> Option<[u8; 32]> {
    value.and_then(Value::as_str).and_then(parse_hex32)
}

fn evidence(event_type: &str, details: &Value) -> Option<Evidence> {
    let int = |name: &str| details.get(name).and_then(Value::as_i64);
    match event_type {
        RETENTION_EXPIRED => Some(Evidence::Retention {
            count: int("count"),
            first_seq: nullable_int(details.get("first_seq")),
            last_seq: nullable_int(details.get("last_seq")),
            digest: hex(details.get("expired_set_digest")),
        }),
        BODY_PURGED => Some(Evidence::Purge {
            target_seq: int("target_seq"),
            target_event_id: details
                .get("target_event_id")
                .and_then(Value::as_str)
                .filter(|id| is_uuid(id))
                .and_then(|id| Uuid::parse_str(id).ok()),
        }),
        EPOCH_STARTED => Some(Evidence::EpochStarted {
            old_epoch: int("old_epoch"),
            new_epoch: int("new_epoch"),
            restored_head_seq: int("restored_head_seq"),
            restored_head_chain: hex(details.get("restored_head_chain")),
        }),
        _ => None,
    }
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
    let origin = Origin::parse(&raw.origin)?;
    if raw.seq < 1
        || raw.recovery_epoch < 1
        || !is_uuid(&raw.event_id)
        || raw.expired != raw.expired_by_seq.is_some()
    {
        return None;
    }
    let body = match (&object["envelope"], raw.envelope) {
        (Value::Null, _) => None,
        (Value::Object(envelope), Some(raw_body)) => {
            let text_of = |name: &str| {
                envelope
                    .get(name)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            };
            let event_type = text_of("type");
            Some(Body {
                text: raw_body.get(),
                id: text_of("id"),
                source: text_of("source"),
                evidence: event_type.as_deref().and_then(|t| {
                    let details = envelope.get("data").and_then(|d| d.get("details"))?;
                    evidence(t, details)
                }),
                event_type,
            })
        }
        _ => return None,
    };
    Some(Line {
        seq: raw.seq,
        event_id: Uuid::parse_str(&raw.event_id).ok()?,
        origin,
        digest: parse_hex32(&raw.envelope_digest)?,
        prev_chain: parse_hex32(&raw.prev_chain)?,
        chain: parse_hex32(&raw.chain)?,
        epoch: raw.recovery_epoch,
        expired_by_seq: raw.expired_by_seq,
        body,
    })
}

/// Whether the body's type and source belong to the line's origin: the
/// catalog entry when the type is known, the structural rule otherwise.
fn origin_matches(origin: Origin, body: &Body<'_>) -> bool {
    let (Some(event_type), Some(source)) = (body.event_type.as_deref(), body.source.as_deref())
    else {
        return false;
    };
    match Catalog::embedded().get(event_type) {
        Some(spec) => spec.origin == origin && spec.source == source,
        None => {
            let control = event_type.starts_with(CONTROL_TYPE_PREFIX);
            match origin {
                Origin::Store => control && source == AUDIT_STORE_SOURCE,
                Origin::RelayControl => control && source == AUDIT_RELAY_SOURCE,
                Origin::Relay => {
                    !control && source != AUDIT_STORE_SOURCE && source != AUDIT_RELAY_SOURCE
                }
            }
        }
    }
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

/// An expired row, by the seq of the evidence it names.
struct ExpiredRow {
    seq: i64,
    event_id: Uuid,
    line: usize,
}

/// Expiry bookkeeping for anchored body exports.
#[derive(Default)]
struct ExpiryLedger {
    /// Expired rows grouped by `expired_by_seq`.
    by_evidence: BTreeMap<i64, Vec<ExpiredRow>>,
    /// Retention and purge bodies by seq, with their line.
    evidence: BTreeMap<i64, (usize, Evidence)>,
}

impl ExpiryLedger {
    /// Checks every expiry against its evidence and returns the number of
    /// expired rows whose evidence could not be verified.
    fn settle(self, anchor_seq: i64, head_seq: i64) -> Result<u64, ExportError> {
        let mut unverified = 0_u64;
        for (evidence_seq, rows) in &self.by_evidence {
            if *evidence_seq > head_seq {
                unverified += rows.len() as u64;
                continue;
            }
            let first_line = rows.first().map_or(0, |row| row.line);
            if !self.evidence.contains_key(evidence_seq) {
                return Err(ExportError::ExpiryEvidenceMissing { line: first_line });
            }
        }
        let none: Vec<ExpiredRow> = Vec::new();
        for (evidence_seq, (line, evidence)) in &self.evidence {
            let rows = self.by_evidence.get(evidence_seq).unwrap_or(&none);
            let mismatch = ExportError::ExpiryEvidenceMismatch { line: *line };
            match evidence {
                Evidence::Retention {
                    count,
                    first_seq,
                    last_seq,
                    digest,
                } => {
                    let (Some(count), Some(first), Some(last), Some(digest)) =
                        (count, first_seq, last_seq, digest)
                    else {
                        return Err(mismatch);
                    };
                    let seqs: Vec<i64> = rows.iter().map(|row| row.seq).collect();
                    let starts_before_anchor = first.is_some_and(|first| first <= anchor_seq);
                    if starts_before_anchor {
                        // Part of the set precedes the export: only check
                        // that what is visible fits, and report it.
                        let fits = usize::try_from(*count).is_ok_and(|c| seqs.len() <= c)
                            && seqs.iter().all(|seq| {
                                first.is_some_and(|f| f <= *seq) && last.is_some_and(|l| *seq <= l)
                            });
                        if !fits {
                            return Err(mismatch);
                        }
                        unverified += seqs.len() as u64;
                        continue;
                    }
                    let exact = usize::try_from(*count).is_ok_and(|c| seqs.len() == c)
                        && *first == seqs.first().copied()
                        && *last == seqs.last().copied()
                        && expired_set_digest(&seqs) == *digest;
                    if !exact {
                        return Err(mismatch);
                    }
                }
                Evidence::Purge {
                    target_seq,
                    target_event_id,
                } => {
                    let Some(target_seq) = target_seq else {
                        return Err(mismatch);
                    };
                    let ok = if *target_seq <= anchor_seq {
                        rows.is_empty()
                    } else {
                        matches!(rows.as_slice(), [row] if row.seq == *target_seq
                            && Some(row.event_id) == *target_event_id)
                    };
                    if !ok {
                        return Err(mismatch);
                    }
                }
                Evidence::EpochStarted { .. } => {}
            }
        }
        Ok(unverified)
    }
}

fn attests_transition(line: &Line<'_>, old_epoch: i64, report: &ExportReport) -> bool {
    let Some(body) = &line.body else {
        return false;
    };
    let Some(Evidence::EpochStarted {
        old_epoch: Some(recorded_old),
        new_epoch: Some(recorded_new),
        restored_head_seq: Some(restored_seq),
        restored_head_chain: Some(restored_chain),
    }) = &body.evidence
    else {
        return false;
    };
    line.origin == Origin::Store
        && body.event_type.as_deref() == Some(EPOCH_STARTED)
        && *recorded_old == old_epoch
        && *recorded_new == line.epoch
        && *restored_seq < line.seq
        && report
            .chain_at(*restored_seq)
            .is_none_or(|chain| chain == *restored_chain)
}

fn verify(text: &str, start: Option<Anchor>, mode: Mode) -> Result<ExportReport, ExportError> {
    let anchor = start.map(Anchor::checkpoint);
    let anchored = anchor.is_some();
    let mut head = anchor.unwrap_or(Checkpoint::GENESIS);
    let mut report = ExportReport {
        anchored,
        anchor,
        head,
        rows: 0,
        bodies: 0,
        expired: 0,
        unverified_expiry_evidence: 0,
        epochs_authenticated: anchored && mode == Mode::Bodies,
        chains: Vec::new(),
        transitions: Vec::new(),
    };
    let mut ledger = ExpiryLedger::default();
    for (index, text) in split_lines(text).into_iter().enumerate() {
        let number = index + 1;
        let line = parse_line(text).ok_or(ExportError::Malformed { line: number })?;
        let next_seq = head
            .seq
            .checked_add(1)
            .ok_or(ExportError::Malformed { line: number })?;
        let mut transition = false;
        if anchored {
            if line.seq != next_seq {
                return Err(ExportError::SeqGap {
                    line: number,
                    expected: next_seq,
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
                if head.epoch.checked_add(1) != Some(line.epoch) {
                    return Err(ExportError::EpochSkipped { line: number });
                }
                transition = true;
            }
        } else if report.rows > 0 && line.seq < next_seq {
            return Err(ExportError::SeqGap {
                line: number,
                expected: next_seq,
                found: line.seq,
            });
        }
        if chain_next(&line.prev_chain, line.seq, line.event_id, &line.digest) != line.chain {
            return Err(ExportError::ChainMismatch { line: number });
        }
        if let Some(evidence_seq) = line.expired_by_seq {
            if line.origin != Origin::Relay {
                return Err(ExportError::ExpiryOnControlEvent { line: number });
            }
            if evidence_seq <= line.seq {
                return Err(ExportError::ExpiryEvidenceMissing { line: number });
            }
        }
        match (mode, line.expired_by_seq, &line.body) {
            (Mode::Bodies, None, Some(body)) => {
                if envelope_digest(body.text) != line.digest {
                    return Err(ExportError::DigestMismatch { line: number });
                }
                if body.id.as_deref() != Some(line.event_id.to_string().as_str()) {
                    return Err(ExportError::EventIdMismatch { line: number });
                }
                if !origin_matches(line.origin, body) {
                    return Err(ExportError::OriginMismatch { line: number });
                }
                report.bodies += 1;
            }
            (Mode::Bodies, Some(_), None) | (Mode::IdentityChain, _, None) => {}
            _ => return Err(ExportError::BodyState { line: number }),
        }
        if transition {
            if mode == Mode::Bodies && !attests_transition(&line, head.epoch, &report) {
                return Err(ExportError::UnattestedEpochChange { line: number });
            }
            report.transitions.push(EpochTransition {
                seq: line.seq,
                old_epoch: head.epoch,
                new_epoch: line.epoch,
            });
        }
        if let Some(evidence_seq) = line.expired_by_seq {
            report.expired += 1;
            ledger
                .by_evidence
                .entry(evidence_seq)
                .or_default()
                .push(ExpiredRow {
                    seq: line.seq,
                    event_id: line.event_id,
                    line: number,
                });
        }
        if line.origin == Origin::Store
            && let Some(body) = line.body
            && let Some(evidence) = body.evidence
            && matches!(
                evidence,
                Evidence::Retention { .. } | Evidence::Purge { .. }
            )
        {
            ledger.evidence.insert(line.seq, (number, evidence));
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
    report.unverified_expiry_evidence = match (anchor, mode) {
        (Some(anchor), Mode::Bodies) => ledger.settle(anchor.seq, head.seq)?,
        _ => report.expired,
    };
    Ok(report)
}
