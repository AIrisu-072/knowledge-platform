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
//! out-of-band checkpoint (design §8). A checkpoint behind the head
//! authenticates the path only up to its own seq ([`ChainVerdict::AuthenticThrough`]):
//! the chain is public sha256, so anyone holding an export can extend it.
//! Filtered subsets are reported as unanchored.
//!
//! `origin`, `recovery_epoch`, `expired` and `expired_by_seq` are not part of
//! the chain; in anchored body exports they are attested by chained bodies
//! (origin against the body's catalog type and source, expiry against
//! `audit.retention.expired` / `audit.body.purged`, epoch changes against
//! `audit.recovery.epoch_started`). Identity-chain exports cannot attest them.
//! Offline expiry verification proves set consistency (count, seq range,
//! `expired_set_digest`, purge target), never retention eligibility: the
//! selector, the cutoff and the identity columns are neither exported nor
//! chained.

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
    /// Attested epoch transitions whose restored head lies before the
    /// anchor, so its chain could not be compared on this path (anchored
    /// body exports). Verify from an earlier anchor to check them.
    pub unverified_restored_heads: u64,
    /// Whether `recovery_epoch` values are attested: true only for anchored
    /// body exports, where every epoch change landed on a verified
    /// `audit.recovery.epoch_started` body. Identity-chain exports and
    /// subsets report epochs as unauthenticated.
    pub epochs_authenticated: bool,
    /// Chain value of every verified row, in seq order (anchored only).
    chains: Vec<[u8; 32]>,
    /// Recovery epoch changes along the verified path (anchored only).
    transitions: Vec<EpochTransition>,
    /// Every expired row and its evidence, in seq order.
    expiries: Vec<ExpiredRowEvidence>,
}

impl ExportReport {
    /// Recovery epoch changes observed along the verified path. Every one
    /// of them is listed for human review (design §8).
    pub fn epoch_transitions(&self) -> &[EpochTransition] {
        &self.transitions
    }

    /// Every expired row of the export with the seq of the evidence that
    /// removed its body and whether that evidence was verified here, in seq
    /// order. [`assess_recovery`] compares the evidence seqs with the
    /// checkpoints.
    pub fn expired_rows(&self) -> &[ExpiredRowEvidence] {
        &self.expiries
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
            .field("unverified_restored_heads", &self.unverified_restored_heads)
            .field("epochs_authenticated", &self.epochs_authenticated)
            .field("transitions", &self.transitions)
            .field("expiries", &self.expiries)
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
    /// What the chained `audit.recovery.epoch_started` body at `seq` claims
    /// (anchored body exports only; `None` for identity-chain exports).
    pub attestation: Option<EpochAttestation>,
}

/// The details of a verified `audit.recovery.epoch_started` body. The lost
/// range is `(restored_head_seq, lost_upper_seq]`, so `lost_from_seq` is
/// always `restored_head_seq + 1` and `lost_upper_seq >= restored_head_seq`.
/// `lost_upper_known` is false when the Store could not bound the lost range
/// (no checkpoint, no relay seq, no regression report): `lost_upper_seq` is
/// then only a lower bound and the epoch is never authentic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochAttestation {
    pub restored_head_seq: i64,
    pub restored_head_chain: [u8; 32],
    pub lost_from_seq: i64,
    pub lost_upper_seq: i64,
    pub lost_upper_known: bool,
    pub classification: RecoveryClassification,
}

/// `audit.recovery.epoch_started` `classification`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryClassification {
    Restore,
    PlannedMove,
    Regression,
}

impl RecoveryClassification {
    pub const ALL: [Self; 3] = [Self::Restore, Self::PlannedMove, Self::Regression];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Restore => "restore",
            Self::PlannedMove => "planned_move",
            Self::Regression => "regression",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == value)
    }
}

/// An expired row and the seq of the control event that removed its body.
/// `verified` is true only when that evidence row lies inside the export and
/// its count, range and `expired_set_digest` (or purge target) match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpiredRowEvidence {
    pub seq: i64,
    pub evidence_seq: i64,
    pub verified: bool,
}

/// The operator's out-of-band record of an epoch transition (design §8,
/// §11), appended to the checkpoint log kept outside the database.
/// `restored_head_seq` / `restored_head_chain` name the last surviving row;
/// `lost_upper` is the claimed upper bound of the lost range
/// `(restored_head_seq, lost_upper]` (equal to `restored_head_seq` for a
/// planned move, which loses nothing). `lost_upper_known` is false when the
/// bound is unknown (the Store's `lost_upper_known: false`): `lost_upper` is
/// then `restored_head_seq`, everything after the restored head may be lost
/// and the epoch is reported as lost, never as authentic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryRecord {
    pub old_epoch: i64,
    pub new_epoch: i64,
    pub restored_head_seq: i64,
    pub restored_head_chain: [u8; 32],
    pub lost_upper: i64,
    pub lost_upper_known: bool,
}

impl RecoveryRecord {
    /// Whether this record describes `transition` on the verified path:
    ///
    /// - the epochs are equal, `0 <= restored_head_seq < transition.seq`;
    /// - every row between the restored head and the transition lies inside
    ///   the lost range (`transition.seq - 1 <= lost_upper`; for a planned
    ///   move, which loses nothing, this forces
    ///   `transition.seq == restored_head_seq + 1`);
    /// - in body exports, the restored head, its chain, the lost upper bound
    ///   and whether it is known equal the chained `epoch_started` body, and
    ///   a body classified `planned_move` has an empty lost range;
    /// - when the restored head lies on the verified path, its chain matches.
    fn matches(&self, transition: &EpochTransition, report: &ExportReport) -> bool {
        let Some(last_before) = transition.seq.checked_sub(1) else {
            return false;
        };
        let shape = self.old_epoch == transition.old_epoch
            && self.new_epoch == transition.new_epoch
            && self.restored_head_seq >= 0
            && self.restored_head_seq < transition.seq
            && self.lost_upper >= last_before;
        let attested = transition.attestation.is_none_or(|body| {
            body.restored_head_seq == self.restored_head_seq
                && body.restored_head_chain == self.restored_head_chain
                && body.lost_upper_seq == self.lost_upper
                && body.lost_upper_known == self.lost_upper_known
                && (body.classification != RecoveryClassification::PlannedMove || !self.loses())
        });
        shape
            && attested
            && report
                .chain_at(self.restored_head_seq)
                .is_none_or(|chain| chain == self.restored_head_chain)
    }

    /// Whether the recovery lost (or may have lost) rows: a non-empty lost
    /// range, or an unknown bound.
    fn loses(&self) -> bool {
        !self.lost_upper_known || self.lost_upper > self.restored_head_seq
    }

    /// Whether `seq` lies in the lost range (an unknown bound covers every
    /// seq after the restored head).
    fn covers(&self, seq: i64) -> bool {
        seq > self.restored_head_seq && (!self.lost_upper_known || seq <= self.lost_upper)
    }
}

/// What verifying an export established about the chain itself, before any
/// out-of-band checkpoint is compared (design §8). It is never an
/// authenticity claim: [`assess_recovery`] decides that from checkpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainIntegrity {
    /// Contiguous from a trusted anchor: every seq follows the previous one,
    /// every `prev_chain` links to the previous chain and every chain value
    /// recomputes. For identity chains (no bodies), epoch changes and
    /// expiries are intact but not attested
    /// ([`ExportReport::epochs_authenticated`],
    /// [`ExportReport::unverified_expiry_evidence`]).
    Intact,
    /// Verification failed: rows are missing, reordered or rewritten, a link
    /// or recomputation fails, an epoch rule is violated or a line is
    /// malformed. The error names the first offending line.
    Broken(ExportError),
    /// A filtered subset without an anchor: rows were checked one by one;
    /// nothing is established about the chain.
    Unanchored,
}

impl ChainIntegrity {
    /// The verdict of a verification result ([`verify_export`],
    /// [`verify_export_complete`], [`verify_identity_chain`],
    /// [`verify_identity_chain_complete`] or [`verify_export_subset`]).
    pub fn of(result: &Result<ExportReport, ExportError>) -> Self {
        match result {
            Ok(report) => Self::of_report(report),
            Err(error) => Self::Broken(error.clone()),
        }
    }

    /// The verdict of a successful verification.
    pub fn of_report(report: &ExportReport) -> Self {
        if report.anchored {
            Self::Intact
        } else {
            Self::Unanchored
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Intact => "intact",
            Self::Broken(_) => "broken",
            Self::Unanchored => "unanchored",
        }
    }
}

/// Overall offline verdict (design §8). Ordered from best to worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChainVerdict {
    /// Contiguous from a trusted anchor, the head equals an out-of-band
    /// checkpoint (seq, chain and epoch), no recovery lost rows, and every
    /// expired row's evidence was verified inside the authenticated range.
    Authentic,
    /// The path is confirmed by an out-of-band checkpoint only up to `seq`,
    /// which lies before the head. Rows after `seq` are not authenticated
    /// (anyone can extend the public chain). Not an authenticity claim for
    /// the export.
    AuthenticThrough { seq: i64 },
    /// The path is confirmed, but the removal of some authenticated row's
    /// body is not: its evidence lies past the authenticated range or
    /// outside the export, its set reaches before the anchor, or the export
    /// has no bodies. See [`RecoveryAssessment::unconfirmed_expiries`].
    UnverifiedExpiry,
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
    /// Whether the restored head (of the record, else of the attesting body)
    /// lies on the verified path with a matching chain. False when it lies
    /// before the anchor: the epoch is then unverified on this path.
    pub restored_head_verified: bool,
}

/// How one out-of-band checkpoint relates to the verified path.
/// Findings for checkpoints before the anchor (`BeforeAnchor`) or at the
/// anchor's own seq (any comparison) are neutral: their verdict
/// (`NoCheckpoint`, they confirm nothing) does not enter the overall verdict.
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
    /// The highest seq at which an out-of-band checkpoint confirms the
    /// verified path (`Match` or `Ahead`), or `None` when nothing confirms
    /// it. Equal to the head seq only for a `Match` at the head.
    pub authenticated_through: Option<i64>,
    pub epochs: Vec<EpochReview>,
    pub findings: Vec<CheckpointFinding>,
    /// Out-of-band records that match no epoch on the verified path although
    /// their epoch comes after the anchor's.
    pub unmatched_records: Vec<RecoveryRecord>,
    /// Out-of-band records of recoveries that precede the anchor
    /// (`new_epoch <= anchor.epoch`): neutral, listed for review only.
    pub records_before_anchor: Vec<RecoveryRecord>,
    /// Out-of-band records of recoveries that come after the verified head
    /// (`old_epoch >= head epoch`: the transition leaves the head's epoch, so
    /// the path ends before it, for example an export taken before a later
    /// restore): neutral, listed for review only. A rollback that removes a
    /// recorded transition is caught by a checkpoint taken after it.
    pub records_after_head: Vec<RecoveryRecord>,
    /// Expired rows at or below `authenticated_through` whose removal is not
    /// confirmed: their evidence lies past `authenticated_through` or could
    /// not be verified in the export. Rows past `authenticated_through` are
    /// covered by `AuthenticThrough` instead.
    pub unconfirmed_expiries: u64,
}

/// Assesses a verified, anchored export against the out-of-band checkpoints
/// and recovery records (design §8 "recovery epochの扱い").
///
/// - Authenticity: `Authentic` needs a checkpoint that `Match`es the head; a
///   checkpoint behind the head (`Ahead`) gives at most
///   `AuthenticThrough { seq }`; with neither, `NoCheckpoint`. Checkpoints
///   before the anchor, and checkpoints at the anchor's own seq (the trusted
///   starting point confirms nothing new and is not evidence about the
///   exported rows, whatever it holds), are neutral.
/// - Records of recoveries before the anchor or after the verified head are
///   neutral (listed for review).
/// - Expiry: an expired row at or below `authenticated_through` whose
///   evidence is unverified or lies past it makes the verdict at least
///   `UnverifiedExpiry`. `Authentic` is never returned while
///   `report.unverified_expiry_evidence > 0`.
/// - A difference at or below a documented restored head is tampering.
/// - A difference inside a documented lost range is `Lost`; a record whose
///   bound is unknown covers everything after its restored head, and its
///   epoch is `Lost` even without any difference.
/// - A difference past an undocumented recovery epoch, an undocumented epoch,
///   a record that matches no epoch (and does not precede the anchor), a
///   restored head that cannot be checked on this path, or a checkpoint
///   whose chain matches but whose epoch differs is `UnverifiedRecovery`.
/// - Any other difference is `Tampered`.
pub fn assess_recovery(
    report: &ExportReport,
    checkpoints: &[Checkpoint],
    records: &[RecoveryRecord],
) -> RecoveryAssessment {
    let Some(anchor) = report.anchor.filter(|_| report.anchored) else {
        return RecoveryAssessment {
            verdict: ChainVerdict::Unanchored,
            authenticated_through: None,
            epochs: Vec::new(),
            findings: Vec::new(),
            unmatched_records: records.to_vec(),
            records_before_anchor: Vec::new(),
            records_after_head: Vec::new(),
            unconfirmed_expiries: 0,
        };
    };
    let epochs: Vec<EpochReview> = report
        .transitions
        .iter()
        .map(|transition| {
            let record = records
                .iter()
                .find(|r| r.matches(transition, report))
                .copied();
            let restored_head = record
                .map(|r| (r.restored_head_seq, r.restored_head_chain))
                .or_else(|| {
                    transition
                        .attestation
                        .map(|a| (a.restored_head_seq, a.restored_head_chain))
                });
            EpochReview {
                transition: *transition,
                record,
                restored_head_verified: restored_head
                    .is_some_and(|(seq, chain)| report.chain_at(seq) == Some(chain)),
            }
        })
        .collect();
    let (records_before_anchor, rest): (Vec<RecoveryRecord>, Vec<RecoveryRecord>) = records
        .iter()
        .filter(|record| !epochs.iter().any(|e| e.record == Some(**record)))
        .copied()
        .partition(|record| record.new_epoch <= anchor.epoch);
    let (records_after_head, unmatched_records): (Vec<RecoveryRecord>, Vec<RecoveryRecord>) = rest
        .into_iter()
        .partition(|record| record.old_epoch >= report.head.epoch);
    let mut worst = ChainVerdict::Authentic;
    for epoch in &epochs {
        let verdict = match epoch.record {
            None => ChainVerdict::UnverifiedRecovery,
            Some(_) if !epoch.restored_head_verified => ChainVerdict::UnverifiedRecovery,
            Some(record) if record.loses() => ChainVerdict::Lost,
            Some(_) => ChainVerdict::Authentic,
        };
        worst = worst.max(verdict);
    }
    if !unmatched_records.is_empty() {
        worst = worst.max(ChainVerdict::UnverifiedRecovery);
    }
    // A checkpoint at the anchor's own seq names the trusted starting point:
    // it confirms nothing about the exported rows, and a disagreement with
    // the anchor is between the out-of-band inputs, not evidence against the
    // export. Its finding keeps the factual comparison.
    let at_anchor = |checkpoint: &Checkpoint| checkpoint.seq == anchor.seq;
    let mut authenticated_through: Option<i64> = None;
    let mut findings = Vec::with_capacity(checkpoints.len());
    for checkpoint in checkpoints {
        let comparison = compare_checkpoint(report, checkpoint);
        let (verdict, explained_by) = match comparison {
            _ if at_anchor(checkpoint) => (ChainVerdict::NoCheckpoint, None),
            CheckpointComparison::Match => (ChainVerdict::Authentic, None),
            CheckpointComparison::Ahead => (
                ChainVerdict::AuthenticThrough {
                    seq: checkpoint.seq,
                },
                None,
            ),
            CheckpointComparison::BeforeAnchor => (ChainVerdict::NoCheckpoint, None),
            CheckpointComparison::Unanchored => (ChainVerdict::Unanchored, None),
            CheckpointComparison::EpochMismatch => (ChainVerdict::UnverifiedRecovery, None),
            CheckpointComparison::Mismatch | CheckpointComparison::StoreBehind => {
                classify_difference(checkpoint, &epochs)
            }
        };
        match comparison {
            _ if at_anchor(checkpoint) => {}
            CheckpointComparison::Match | CheckpointComparison::Ahead => {
                authenticated_through = authenticated_through.max(Some(checkpoint.seq));
            }
            CheckpointComparison::BeforeAnchor => {}
            _ => worst = worst.max(verdict),
        }
        findings.push(CheckpointFinding {
            checkpoint: *checkpoint,
            comparison,
            verdict,
            explained_by,
        });
    }
    let confirmed = match authenticated_through {
        None => ChainVerdict::NoCheckpoint,
        Some(seq) if seq == report.head.seq => ChainVerdict::Authentic,
        Some(seq) => ChainVerdict::AuthenticThrough { seq },
    };
    worst = worst.max(confirmed);
    let unconfirmed_expiries = authenticated_through.map_or(0, |through| {
        report
            .expiries
            .iter()
            .filter(|row| row.seq <= through && (!row.verified || row.evidence_seq > through))
            .count() as u64
    });
    if unconfirmed_expiries > 0
        || (worst == ChainVerdict::Authentic && report.unverified_expiry_evidence > 0)
    {
        worst = worst.max(ChainVerdict::UnverifiedExpiry);
    }
    RecoveryAssessment {
        verdict: worst,
        authenticated_through,
        epochs,
        findings,
        unmatched_records,
        records_before_anchor,
        records_after_head,
        unconfirmed_expiries,
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
            Some(record) if record.covers(checkpoint.seq) => {
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
    /// Store was restored to an older state. Pass only checkpoints up to the
    /// export head when verifying a prefix (`seq_through`).
    StoreBehind,
    /// The verified path passes through the checkpoint and continues past it.
    /// It authenticates the path only up to the checkpoint.
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
    #[error("export line {line}: epoch_started record without an epoch change")]
    EpochStartedWithoutTransition { line: usize },
    #[error("export ends at seq {head}, not at the manifest watermark {watermark}")]
    WatermarkMismatch { watermark: i64, head: i64 },
}

/// Verifies a full (body-carrying) export contiguously from `start`.
/// Expiry evidence past the export's head is counted in
/// `unverified_expiry_evidence`.
pub fn verify_export(text: &str, start: Anchor) -> Result<ExportReport, ExportError> {
    verify(text, Some(start), Mode::Bodies, None)
}

/// Verifies a complete body export up to the manifest `watermark`
/// (design §10.3 W): like [`verify_export`], but the head must be exactly
/// `watermark` ([`ExportError::WatermarkMismatch`]) and no row may name
/// expiry evidence past it ([`ExportError::ExpiryEvidenceMissing`]). A
/// complete export therefore carries every evidence row it references, which
/// closes origin relabelling of expired control rows (their evidence must be
/// a retention or purge body, and those only expire relay rows).
pub fn verify_export_complete(
    text: &str,
    start: Anchor,
    watermark: i64,
) -> Result<ExportReport, ExportError> {
    verify(text, Some(start), Mode::Bodies, Some(watermark))
}

/// Verifies an identity-chain export (`envelope` always null) from `start`.
/// Epochs and expiry are reported as unauthenticated.
pub fn verify_identity_chain(text: &str, start: Anchor) -> Result<ExportReport, ExportError> {
    verify(text, Some(start), Mode::IdentityChain, None)
}

/// Verifies a complete identity-chain export up to the manifest `watermark`:
/// like [`verify_identity_chain`], but the head must be exactly `watermark`
/// ([`ExportError::WatermarkMismatch`]) and no row may name expiry evidence
/// past it ([`ExportError::ExpiryEvidenceMissing`]).
pub fn verify_identity_chain_complete(
    text: &str,
    start: Anchor,
    watermark: i64,
) -> Result<ExportReport, ExportError> {
    verify(text, Some(start), Mode::IdentityChain, Some(watermark))
}

/// Checks a filtered export row by row (digest, id, origin, self-consistent
/// chain step, increasing seq). The result is never anchored.
pub fn verify_export_subset(text: &str) -> Result<ExportReport, ExportError> {
    verify(text, None, Mode::Bodies, None)
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
        lost_from_seq: Option<i64>,
        lost_upper_seq: Option<i64>,
        lost_upper_known: Option<bool>,
        classification: Option<RecoveryClassification>,
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
            lost_from_seq: int("lost_from_seq"),
            lost_upper_seq: int("lost_upper_seq"),
            lost_upper_known: details.get("lost_upper_known").and_then(Value::as_bool),
            classification: details
                .get("classification")
                .and_then(Value::as_str)
                .and_then(RecoveryClassification::parse),
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
struct PendingExpiry {
    seq: i64,
    event_id: Uuid,
    line: usize,
}

/// Expiry bookkeeping.
#[derive(Default)]
struct ExpiryLedger {
    /// Expired rows grouped by `expired_by_seq`.
    by_evidence: BTreeMap<i64, Vec<PendingExpiry>>,
    /// Retention and purge bodies by seq, with their line.
    evidence: BTreeMap<i64, (usize, Evidence)>,
}

impl ExpiryLedger {
    /// Every expired row, unverified (exports without bodies, subsets).
    fn unverified(self) -> Vec<ExpiredRowEvidence> {
        let mut rows: Vec<ExpiredRowEvidence> = self
            .by_evidence
            .into_iter()
            .flat_map(|(evidence_seq, rows)| {
                rows.into_iter().map(move |row| ExpiredRowEvidence {
                    seq: row.seq,
                    evidence_seq,
                    verified: false,
                })
            })
            .collect();
        rows.sort_by_key(|row| row.seq);
        rows
    }

    /// Checks every expiry of an anchored body export against its evidence
    /// and returns each expired row with whether its evidence was verified.
    fn settle(
        self,
        anchor_seq: i64,
        head_seq: i64,
    ) -> Result<Vec<ExpiredRowEvidence>, ExportError> {
        let mut settled = Vec::new();
        let mut record = |rows: &[PendingExpiry], evidence_seq: i64, verified: bool| {
            settled.extend(rows.iter().map(|row| ExpiredRowEvidence {
                seq: row.seq,
                evidence_seq,
                verified,
            }));
        };
        for (evidence_seq, rows) in &self.by_evidence {
            if *evidence_seq > head_seq {
                record(rows, *evidence_seq, false);
                continue;
            }
            let first_line = rows.first().map_or(0, |row| row.line);
            if !self.evidence.contains_key(evidence_seq) {
                return Err(ExportError::ExpiryEvidenceMissing { line: first_line });
            }
        }
        let none: Vec<PendingExpiry> = Vec::new();
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
                        record(rows, *evidence_seq, false);
                        continue;
                    }
                    let exact = usize::try_from(*count).is_ok_and(|c| seqs.len() == c)
                        && *first == seqs.first().copied()
                        && *last == seqs.last().copied()
                        && expired_set_digest(&seqs) == *digest;
                    if !exact {
                        return Err(mismatch);
                    }
                    record(rows, *evidence_seq, true);
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
                    record(rows, *evidence_seq, true);
                }
                Evidence::EpochStarted { .. } => {}
            }
        }
        settled.sort_by_key(|row| row.seq);
        Ok(settled)
    }
}

/// The attestation of an epoch change by the line's body: an origin=store
/// `audit.recovery.epoch_started` whose epochs equal the change, whose
/// restored head precedes the line (`0 <= restored_head_seq < seq`), whose
/// lost range `(restored_head_seq, lost_upper_seq]` is well formed (with a
/// boolean `lost_upper_known`), and whose restored head chain matches the
/// verified path when the head lies on it.
fn attest_transition(
    line: &Line<'_>,
    old_epoch: i64,
    report: &ExportReport,
) -> Option<EpochAttestation> {
    let body = line.body.as_ref()?;
    let Some(Evidence::EpochStarted {
        old_epoch: Some(recorded_old),
        new_epoch: Some(recorded_new),
        restored_head_seq: Some(restored_seq),
        restored_head_chain: Some(restored_chain),
        lost_from_seq: Some(lost_from),
        lost_upper_seq: Some(lost_upper),
        lost_upper_known: Some(lost_upper_known),
        classification: Some(classification),
    }) = body.evidence
    else {
        return None;
    };
    let attested = line.origin == Origin::Store
        && body.event_type.as_deref() == Some(EPOCH_STARTED)
        && recorded_old == old_epoch
        && recorded_new == line.epoch
        && restored_seq >= 0
        && restored_seq < line.seq
        && restored_seq.checked_add(1) == Some(lost_from)
        && lost_upper >= restored_seq
        && report
            .chain_at(restored_seq)
            .is_none_or(|chain| chain == restored_chain);
    attested.then_some(EpochAttestation {
        restored_head_seq: restored_seq,
        restored_head_chain: restored_chain,
        lost_from_seq: lost_from,
        lost_upper_seq: lost_upper,
        lost_upper_known,
        classification,
    })
}

/// Whether the line carries an origin=store `audit.recovery.epoch_started`.
fn is_epoch_started(line: &Line<'_>) -> bool {
    line.origin == Origin::Store
        && line
            .body
            .as_ref()
            .is_some_and(|body| body.event_type.as_deref() == Some(EPOCH_STARTED))
}

fn verify(
    text: &str,
    start: Option<Anchor>,
    mode: Mode,
    watermark: Option<i64>,
) -> Result<ExportReport, ExportError> {
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
        unverified_restored_heads: 0,
        epochs_authenticated: anchored && mode == Mode::Bodies,
        chains: Vec::new(),
        transitions: Vec::new(),
        expiries: Vec::new(),
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
            if evidence_seq <= line.seq || watermark.is_some_and(|w| evidence_seq > w) {
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
        let attests = anchored && mode == Mode::Bodies;
        if transition {
            let attestation = if attests {
                let attestation = attest_transition(&line, head.epoch, &report)
                    .ok_or(ExportError::UnattestedEpochChange { line: number })?;
                if anchor.is_some_and(|a| attestation.restored_head_seq < a.seq) {
                    report.unverified_restored_heads += 1;
                }
                Some(attestation)
            } else {
                None
            };
            report.transitions.push(EpochTransition {
                seq: line.seq,
                old_epoch: head.epoch,
                new_epoch: line.epoch,
                attestation,
            });
        } else if attests && is_epoch_started(&line) {
            return Err(ExportError::EpochStartedWithoutTransition { line: number });
        }
        if let Some(evidence_seq) = line.expired_by_seq {
            report.expired += 1;
            ledger
                .by_evidence
                .entry(evidence_seq)
                .or_default()
                .push(PendingExpiry {
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
    if let Some(watermark) = watermark
        && head.seq != watermark
    {
        return Err(ExportError::WatermarkMismatch {
            watermark,
            head: head.seq,
        });
    }
    report.expiries = match (anchor, mode) {
        (Some(anchor), Mode::Bodies) => ledger.settle(anchor.seq, head.seq)?,
        _ => ledger.unverified(),
    };
    report.unverified_expiry_evidence =
        report.expiries.iter().filter(|row| !row.verified).count() as u64;
    Ok(report)
}
