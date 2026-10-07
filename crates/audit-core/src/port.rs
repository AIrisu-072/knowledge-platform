//! The Audit Store port used by the relay (design §6.2, §6.3, §7.2, §10.4,
//! §11, §12). Runtime-neutral: no async runtime types appear here, only boxed
//! `Send` futures. Every type is content-free: identities, digests, counters
//! and bounded codes, never envelope bodies.
//!
//! Failure model (design §6.3) is two-way:
//!
//! - **Terminal** ([`StoreError::Conflict`], [`StoreError::Rejected`]): a
//!   definite Store verdict that quarantines the event. Verdicts are decoded
//!   only from the structured ingest result row ([`IngestRow::into_result`]).
//! - **Outage** ([`StoreError::Outage`]): everything else. The relay returns
//!   the attempt and holds delivery. Transport errors, timeouts, unknown
//!   outcomes, every SQLSTATE ([`classify_sqlstate`] is total), malformed
//!   result rows and the Store gates (recovery, regression, posture,
//!   unregistered type, denied ingest identity) are outages.
//!
//! Only [`OutageCode::counts_toward_outage_streak`] codes (residual,
//! unexpected failures) may advance `outage_streak`; Store-wide conditions
//! never do.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::catalog::{Catalog, Origin};
use crate::chain::to_hex;
use crate::codes::RejectionCode;
use crate::envelope::AuditEnvelope;

/// A boxed `Send` future borrowed for `'a`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Upper bound on ids or seqs per lookup call (design §10.4).
pub const MAX_RECEIPT_LOOKUP: usize = 1000;

/// An append-only, idempotent Audit Store.
///
/// All lookups are content-free and are not audited per call (design §10.4);
/// the Store restricts them by DB role. In recovery mode the Store still
/// answers `probe` and the lookups, reporting [`StoreState::RecoveryMode`],
/// while every publishing call returns an outage
/// ([`OutageCode::RecoveryRequired`]).
pub trait AuditStore: Send + Sync {
    /// Ingests one validated relay-origin envelope. Re-ingesting the same
    /// event id with the same source commitment converges to a `Duplicate*`
    /// outcome (design §7.2).
    fn ingest<'a>(
        &'a self,
        envelope: &'a AuditEnvelope,
    ) -> BoxFuture<'a, Result<IngestReceipt, StoreError>>;

    /// Runs the ingest gate without storing anything: head lock no-op,
    /// writability, fingerprint, recovery flag, posture, the ingest identity
    /// binding, the registration of `expected.types` for
    /// `(expected.source, expected.adapter_version)`, and the identity check
    /// of the relay's last acknowledged receipt (design §6.2, §11). A gate
    /// failure is reported in [`StoreStatus`], not as an error; `Err` means
    /// the probe itself could not run (an outage).
    fn probe<'a>(
        &'a self,
        expected: &'a ProbeExpectation,
    ) -> BoxFuture<'a, Result<StoreStatus, StoreError>>;

    /// Receipts of the given event ids (at most [`MAX_RECEIPT_LOOKUP`]).
    /// Unknown ids are simply absent from the result.
    fn lookup_receipts<'a>(
        &'a self,
        event_ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>>;

    /// Receipts of `source` with `seq > after_seq` in seq order, at most
    /// `limit` (≤ [`MAX_RECEIPT_LOOKUP`]) rows. Reconcile pages through it to
    /// find `store_only` events (design §12).
    fn list_source_receipts<'a>(
        &'a self,
        source: &'a str,
        after_seq: i64,
        limit: u32,
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>>;

    /// Control event receipts at the given seqs (at most
    /// [`MAX_RECEIPT_LOOKUP`]), used to resolve `delivery_history` control
    /// seqs for `unaudited_replay` (design §6.4).
    fn lookup_control_receipts<'a>(
        &'a self,
        seqs: &'a [i64],
    ) -> BoxFuture<'a, Result<Vec<ControlReceiptRow>, StoreError>>;

    /// Records a relay control event (origin `relay_control`) under the
    /// caller's bound principal and returns its position. The Store builds
    /// the envelope from [`RelayControl::details`]. `SourceMismatchDetected`
    /// is idempotent on `(event_id, code)`: a retry returns the original
    /// receipt (design §6.3).
    fn record_relay_control<'a>(
        &'a self,
        control: &'a RelayControl,
    ) -> BoxFuture<'a, Result<ControlReceipt, StoreError>>;

    /// Reports that the relay's last acknowledged receipt no longer resolves
    /// in the Store. The Store re-checks the identity under the head lock and
    /// only then sets `recovery_pending` (design §11).
    fn report_regression<'a>(
        &'a self,
        identity: &'a ReceiptIdentity,
    ) -> BoxFuture<'a, Result<(), StoreError>>;
}

/// How the Store settled an ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IngestOutcome {
    Stored,
    Duplicate,
    DuplicateExpired,
    DuplicateReprojected,
}

impl IngestOutcome {
    pub const ALL: [Self; 4] = [
        Self::Stored,
        Self::Duplicate,
        Self::DuplicateExpired,
        Self::DuplicateReprojected,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Duplicate => "duplicate",
            Self::DuplicateExpired => "duplicate_expired",
            Self::DuplicateReprojected => "duplicate_reprojected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|outcome| outcome.as_str() == value)
    }
}

/// Store receipt. For duplicates, `seq`, `envelope_digest` and
/// `adapter_version` describe the originally stored event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestReceipt {
    pub seq: i64,
    pub envelope_digest: [u8; 32],
    pub outcome: IngestOutcome,
    pub adapter_version: i32,
}

/// The structured row returned by `audit_store.ingest` (design §7.2 step 7):
/// `(status, seq, envelope_digest, adapter_version, code)`.
#[derive(Clone, PartialEq, Eq)]
pub struct IngestRow {
    pub status: String,
    pub seq: Option<i64>,
    pub envelope_digest: Option<Vec<u8>>,
    pub adapter_version: Option<i32>,
    pub code: Option<String>,
}

impl IngestRow {
    /// Decodes the row. This is the only place a terminal [`StoreError`] is
    /// produced.
    ///
    /// | status | result |
    /// |---|---|
    /// | `stored` / `duplicate` / `duplicate_expired` / `duplicate_reprojected` | receipt; a missing or malformed seq, 32-byte digest or adapter version is `Outage{OutcomeUnknown}` |
    /// | `conflict` | `Conflict` |
    /// | `rejected` | `Rejected{code}`; a missing or unbounded code is `Outage{Other}` |
    /// | `recovery_required` | `Outage{RecoveryRequired}` |
    /// | `outage` | `Outage{code}` where `code` names an [`OutageCode`] with or without the `store_` prefix (`unregistered_type` → `UnregisteredType`), otherwise `Outage{Other}` |
    /// | `denied` | `Outage{Denied}` (the ingest login is not the registered source service principal) |
    /// | anything else | `Outage{Other}` |
    pub fn into_result(self) -> Result<IngestReceipt, StoreError> {
        if let Some(outcome) = IngestOutcome::parse(&self.status) {
            let receipt = match (self.seq, self.envelope_digest, self.adapter_version) {
                (Some(seq), Some(digest), Some(adapter_version))
                    if seq >= 1 && adapter_version >= 1 =>
                {
                    <[u8; 32]>::try_from(digest.as_slice())
                        .ok()
                        .map(|envelope_digest| IngestReceipt {
                            seq,
                            envelope_digest,
                            outcome,
                            adapter_version,
                        })
                }
                _ => None,
            };
            return receipt.ok_or(StoreError::outage(OutageCode::OutcomeUnknown));
        }
        match self.status.as_str() {
            "conflict" => Err(StoreError::Conflict),
            "rejected" => Err(self
                .code
                .as_deref()
                .and_then(BoundedCode::new)
                .map_or(StoreError::outage(OutageCode::Other), |code| {
                    StoreError::Rejected { code }
                })),
            "recovery_required" => Err(StoreError::outage(OutageCode::RecoveryRequired)),
            "outage" => Err(StoreError::outage(
                self.code
                    .as_deref()
                    .and_then(OutageCode::parse_lenient)
                    .unwrap_or(OutageCode::Other),
            )),
            "denied" => Err(StoreError::outage(OutageCode::Denied)),
            _ => Err(StoreError::outage(OutageCode::Other)),
        }
    }
}

impl fmt::Debug for IngestRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bounded = |text: &str| BoundedCode::new(text).map_or("<invalid>".to_owned(), |c| c.0);
        f.debug_struct("IngestRow")
            .field("status", &bounded(&self.status))
            .field("seq", &self.seq)
            .field(
                "envelope_digest",
                &self.envelope_digest.as_deref().map(to_hex),
            )
            .field("adapter_version", &self.adapter_version)
            .field("code", &self.code.as_deref().map(bounded))
            .finish()
    }
}

/// The identity of one stored event, as acknowledged by the relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiptIdentity {
    pub seq: i64,
    pub event_id: Uuid,
    pub envelope_digest: [u8; 32],
}

impl ReceiptIdentity {
    /// Whether `rows` (from [`AuditStore::lookup_receipts`]) still contain
    /// this identity at the same seq with the same digest. `false` means the
    /// Store regressed behind an acknowledged receipt (design §11); seq
    /// comparisons alone are never used.
    pub fn is_confirmed_by(&self, rows: &[ReceiptRow]) -> bool {
        rows.iter().any(|row| row.identity() == *self)
    }
}

/// Content-free receipt of a stored event (design §10.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptRow {
    pub seq: i64,
    pub event_id: Uuid,
    pub origin: Origin,
    pub event_type: String,
    pub envelope_digest: [u8; 32],
    pub source_commitment: Option<[u8; 32]>,
    pub expired: bool,
    pub recovery_epoch: i64,
}

impl ReceiptRow {
    pub fn identity(&self) -> ReceiptIdentity {
        ReceiptIdentity {
            seq: self.seq,
            event_id: self.event_id,
            envelope_digest: self.envelope_digest,
        }
    }
}

/// Content-free receipt of a control event. `target_event_id` is the event
/// a relay control event is about; `code` its recorded machine code (for
/// `audit.delivery.replay_requested`, the previous quarantine code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlReceiptRow {
    pub seq: i64,
    pub recovery_epoch: i64,
    pub origin: Origin,
    pub event_type: String,
    pub target_event_id: Option<Uuid>,
    pub code: Option<String>,
}

/// Position of a recorded control event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlReceipt {
    pub seq: i64,
    pub recovery_epoch: i64,
}

/// What the relay expects the Store to accept (design §6.2 `probe(expected)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeExpectation {
    pub source: String,
    pub adapter_version: i32,
    pub types: Vec<String>,
    pub last_ack: Option<ReceiptIdentity>,
}

impl ProbeExpectation {
    /// The expectation for one relay source, from the catalog's
    /// [`Catalog::registered_types`]. `None` when the catalog has no relay
    /// types for `source`.
    pub fn from_catalog(
        catalog: &Catalog,
        source: &str,
        last_ack: Option<ReceiptIdentity>,
    ) -> Option<Self> {
        let registered: Vec<(&str, &str, i32)> = catalog
            .registered_types()
            .into_iter()
            .filter(|(s, _, _)| *s == source)
            .collect();
        let adapter_version = registered.first()?.2;
        Some(Self {
            source: source.to_owned(),
            adapter_version,
            types: registered.iter().map(|(_, t, _)| (*t).to_owned()).collect(),
            last_ack,
        })
    }
}

/// Why the Store is or is not writable. Mapping (design §6.2, §11):
///
/// | state | Store condition | outage code |
/// |---|---|---|
/// | `Operational` | gate passes | none |
/// | `RecoveryMode` | fingerprint mismatch or `recovery_pending` | `store_recovery_required` |
/// | `PostureInvalid` | `posture_check()` reports violations | `store_posture_invalid` |
/// | `ReadOnly` | `transaction_read_only` or `pg_is_in_recovery()` | `store_read_only` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreState {
    Operational,
    RecoveryMode,
    PostureInvalid,
    ReadOnly,
}

impl StoreState {
    pub const ALL: [Self; 4] = [
        Self::Operational,
        Self::RecoveryMode,
        Self::PostureInvalid,
        Self::ReadOnly,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Operational => "operational",
            Self::RecoveryMode => "recovery_mode",
            Self::PostureInvalid => "posture_invalid",
            Self::ReadOnly => "read_only",
        }
    }

    /// The outage a non-operational Store causes.
    pub const fn outage_code(self) -> Option<OutageCode> {
        match self {
            Self::Operational => None,
            Self::RecoveryMode => Some(OutageCode::RecoveryRequired),
            Self::PostureInvalid => Some(OutageCode::PostureInvalid),
            Self::ReadOnly => Some(OutageCode::ReadOnly),
        }
    }
}

/// Content-free Store status returned by [`AuditStore::probe`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreStatus {
    pub head_seq: i64,
    pub recovery_epoch: i64,
    pub state: StoreState,
    /// Expected types that are not registered for the expected source and
    /// adapter version (`store_catalog_skew` in health).
    pub missing_types: Vec<String>,
    /// The expected last acknowledged receipt does not resolve.
    pub regression_detected: bool,
    /// Seq of the last origin=store `audit.integrity.verified`.
    pub last_verified_seq: Option<i64>,
}

impl StoreStatus {
    /// Admission decision for the circuit breaker: `Ok` only when the Store
    /// is operational, no regression was detected and every expected type is
    /// registered. Checked in that order.
    pub fn admission(&self) -> Result<(), OutageCode> {
        if let Some(code) = self.state.outage_code() {
            return Err(code);
        }
        if self.regression_detected {
            return Err(OutageCode::Regressed);
        }
        if !self.missing_types.is_empty() {
            return Err(OutageCode::UnregisteredType);
        }
        Ok(())
    }
}

/// External-failure classes (design §6.3). An outage returns the attempt and
/// holds delivery instead of consuming the retry budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutageCode {
    /// The connection or protocol failed before a result row arrived.
    Transport,
    /// A client-side or statement timeout (SQLSTATE 57014).
    Timeout,
    /// SQLSTATE class 08.
    Connection,
    /// SQLSTATE class 53.
    Resources,
    /// SQLSTATE 25006, or a read-only Store.
    ReadOnly,
    /// SQLSTATE 57P (admin/crash shutdown, cannot connect now, ...).
    Shutdown,
    /// SQLSTATE 40001.
    Serialization,
    /// SQLSTATE 40P01.
    Deadlock,
    /// SQLSTATE 55P03 (`lock_timeout` on the head lock).
    LockUnavailable,
    /// SQLSTATE class 42: the Store schema or grants do not match the relay.
    DeployMismatch,
    /// SQLSTATE classes 58, XX and 54: residual internal failures.
    Internal,
    /// Any other SQLSTATE, a malformed or empty SQLSTATE, an unknown status
    /// or a malformed result row.
    Other,
    /// The commit outcome is unknown, or a receipt row lacked its identity.
    OutcomeUnknown,
    /// Fingerprint mismatch or `recovery_pending` (design §11).
    RecoveryRequired,
    /// The Store no longer holds an acknowledged receipt (design §11).
    Regressed,
    /// `posture_check()` reports violations (design §7.3).
    PostureInvalid,
    /// `(source, type, adapter_version)` is not registered in the Store:
    /// Store-side version skew (design §7.2 step 2).
    UnregisteredType,
    /// The ingest login is not bound to the registered source service
    /// principal (`denied:<code>`, design §7.2 step 1).
    Denied,
}

impl OutageCode {
    pub const ALL: [Self; 18] = [
        Self::Transport,
        Self::Timeout,
        Self::Connection,
        Self::Resources,
        Self::ReadOnly,
        Self::Shutdown,
        Self::Serialization,
        Self::Deadlock,
        Self::LockUnavailable,
        Self::DeployMismatch,
        Self::Internal,
        Self::Other,
        Self::OutcomeUnknown,
        Self::RecoveryRequired,
        Self::Regressed,
        Self::PostureInvalid,
        Self::UnregisteredType,
        Self::Denied,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Transport => "store_transport",
            Self::Timeout => "store_timeout",
            Self::Connection => "store_connection",
            Self::Resources => "store_resources",
            Self::ReadOnly => "store_read_only",
            Self::Shutdown => "store_shutdown",
            Self::Serialization => "store_serialization",
            Self::Deadlock => "store_deadlock",
            Self::LockUnavailable => "store_lock_unavailable",
            Self::DeployMismatch => "store_deploy_mismatch",
            Self::Internal => "store_internal",
            Self::Other => "store_other",
            Self::OutcomeUnknown => "store_outcome_unknown",
            Self::RecoveryRequired => "store_recovery_required",
            Self::Regressed => "store_regressed",
            Self::PostureInvalid => "store_posture_invalid",
            Self::UnregisteredType => "store_unregistered_type",
            Self::Denied => "store_denied",
        }
    }

    /// Parses a name produced by [`Self::as_str`].
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == value)
    }

    /// Parses a name with or without the `store_` prefix.
    fn parse_lenient(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| {
            let name = code.as_str();
            name == value || name.strip_prefix("store_") == Some(value)
        })
    }

    /// Whether this outage may advance a row's `outage_streak` (design §6.3).
    /// Only residual, unexpected failures count; gate results, known
    /// SQLSTATE classes, transport, timeouts and unknown outcomes are
    /// Store-wide conditions and never count.
    pub const fn counts_toward_outage_streak(self) -> bool {
        matches!(self, Self::Internal | Self::Other)
    }
}

/// Classifies any PostgreSQL SQLSTATE raised by a Store call. Total: every
/// input, including malformed or empty text, yields an outage class. Store
/// verdicts are never SQL errors; they arrive as result rows
/// ([`IngestRow::into_result`]).
///
/// | SQLSTATE | class |
/// |---|---|
/// | `25006` | `ReadOnly` |
/// | `57014` | `Timeout` |
/// | `57P..` | `Shutdown` |
/// | `40001` | `Serialization` |
/// | `40P01` | `Deadlock` |
/// | `55P03` | `LockUnavailable` |
/// | `08...` | `Connection` |
/// | `53...` | `Resources` |
/// | `42...` | `DeployMismatch` |
/// | `58...`, `XX...`, `54...` | `Internal` |
/// | anything else | `Other` |
pub fn classify_sqlstate(sqlstate: &str) -> OutageCode {
    let bytes = sqlstate.as_bytes();
    if bytes.len() != 5
        || !bytes
            .iter()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
    {
        return OutageCode::Other;
    }
    match sqlstate {
        "25006" => OutageCode::ReadOnly,
        "57014" => OutageCode::Timeout,
        "40001" => OutageCode::Serialization,
        "40P01" => OutageCode::Deadlock,
        "55P03" => OutageCode::LockUnavailable,
        _ => match &sqlstate[..2] {
            "57" if bytes[2] == b'P' => OutageCode::Shutdown,
            "08" => OutageCode::Connection,
            "53" => OutageCode::Resources,
            "42" => OutageCode::DeployMismatch,
            "58" | "XX" | "54" => OutageCode::Internal,
            _ => OutageCode::Other,
        },
    }
}

/// A bounded machine code (`[a-z0-9_]{1,64}`). It cannot carry payload text.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BoundedCode(String);

impl BoundedCode {
    pub fn new(code: &str) -> Option<Self> {
        crate::kinds::is_code(code).then(|| Self(code.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for BoundedCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for BoundedCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Store failures: terminal verdicts (quarantine) or outages (hold).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("audit store outage: {}", code.as_str())]
    Outage { code: OutageCode },
    #[error("audit store conflict")]
    Conflict,
    #[error("audit store rejected the envelope: {code}")]
    Rejected { code: BoundedCode },
}

impl StoreError {
    pub const fn outage(code: OutageCode) -> Self {
        Self::Outage { code }
    }

    /// Whether the failure is a definite verdict that quarantines the event.
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Conflict | Self::Rejected { .. })
    }

    /// Whether the relay should return the attempt and hold delivery. Always
    /// `!is_terminal()`.
    pub const fn is_outage(&self) -> bool {
        !self.is_terminal()
    }

    /// The outage class, if this error is an outage.
    pub const fn outage_code(&self) -> Option<OutageCode> {
        match self {
            Self::Outage { code } => Some(*code),
            Self::Conflict | Self::Rejected { .. } => None,
        }
    }

    /// Whether this failure may advance `outage_streak`.
    pub const fn counts_toward_outage_streak(&self) -> bool {
        match self {
            Self::Outage { code } => code.counts_toward_outage_streak(),
            Self::Conflict | Self::Rejected { .. } => false,
        }
    }
}

/// A relay control event submitted through
/// [`AuditStore::record_relay_control`] (catalog origin `relay_control`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayControl {
    pub kind: RelayControlKind,
}

/// What a [`RelayControl`] records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayControlKind {
    /// `audit.delivery.replay_requested`. `previous_code` is the quarantine
    /// code being cleared; it is recorded as `details.quarantine_code`.
    ReplayRequested {
        event_id: Uuid,
        previous_code: BoundedCode,
    },
    /// `audit.reconciliation.completed`, one per reconcile run.
    ReconciliationCompleted {
        run_id: Uuid,
        mode: ReconcileMode,
        watermark: i64,
        id_set_digest: [u8; 32],
        counts: ReconcileCounts,
    },
    /// `audit.integrity.source_mismatch_detected`, recorded before the row is
    /// quarantined.
    SourceMismatchDetected {
        event_id: Uuid,
        code: SourceMismatchCode,
    },
}

impl From<RelayControlKind> for RelayControl {
    fn from(kind: RelayControlKind) -> Self {
        Self { kind }
    }
}

impl RelayControl {
    pub const fn event_type(&self) -> &'static str {
        match self.kind {
            RelayControlKind::ReplayRequested { .. } => "audit.delivery.replay_requested",
            RelayControlKind::ReconciliationCompleted { .. } => "audit.reconciliation.completed",
            RelayControlKind::SourceMismatchDetected { .. } => {
                "audit.integrity.source_mismatch_detected"
            }
        }
    }

    /// The catalog `details` of the control event, except `session_role`,
    /// which the Store adds from `session_user`.
    pub fn details(&self) -> Map<String, Value> {
        let value = match &self.kind {
            RelayControlKind::ReplayRequested {
                event_id,
                previous_code,
            } => json!({"event_id": event_id, "quarantine_code": previous_code.as_str()}),
            RelayControlKind::ReconciliationCompleted {
                run_id,
                mode,
                watermark,
                id_set_digest,
                counts,
            } => {
                let mut details = counts.details();
                details.insert("run_id".to_owned(), json!(run_id));
                details.insert("mode".to_owned(), json!(mode.as_str()));
                details.insert("watermark".to_owned(), json!(watermark));
                details.insert("id_set_digest".to_owned(), json!(to_hex(id_set_digest)));
                Value::Object(details)
            }
            RelayControlKind::SourceMismatchDetected { event_id, code } => {
                json!({"event_id": event_id, "mismatch_code": code.as_str()})
            }
        };
        match value {
            Value::Object(map) => map,
            _ => unreachable!("details are built as objects"),
        }
    }
}

/// Whether a reconcile run only reported or also repaired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReconcileMode {
    ReadOnly,
    Repair,
}

impl ReconcileMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Repair => "repair",
        }
    }
}

/// The relay-side verdicts that are recorded as source mismatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceMismatchCode {
    SourceDigestMismatch,
    ActorMismatch,
}

impl SourceMismatchCode {
    pub const fn rejection_code(self) -> RejectionCode {
        match self {
            Self::SourceDigestMismatch => RejectionCode::SourceDigestMismatch,
            Self::ActorMismatch => RejectionCode::ActorMismatch,
        }
    }

    pub const fn as_str(self) -> &'static str {
        self.rejection_code().as_str()
    }
}

/// Per-class reconcile counts (design §6.4, §12), recorded as fixed counter
/// fields `count_<class>` and `repaired_<class>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ReconcileCounts {
    pub ok: u64,
    pub delivered_missing: u64,
    pub digest_mismatch: u64,
    pub quarantined_stored: u64,
    pub quarantined_conflict: u64,
    pub pending: u64,
    pub quarantined: u64,
    pub unregistered: u64,
    pub source_tampered: u64,
    pub store_only: u64,
    pub unaudited_replay: u64,
    pub replay_record_lost: u64,
    pub repaired_delivered_missing: u64,
    pub repaired_quarantined_stored: u64,
    pub repaired_unregistered: u64,
}

impl ReconcileCounts {
    fn details(&self) -> Map<String, Value> {
        [
            ("count_ok", self.ok),
            ("count_delivered_missing", self.delivered_missing),
            ("count_digest_mismatch", self.digest_mismatch),
            ("count_quarantined_stored", self.quarantined_stored),
            ("count_quarantined_conflict", self.quarantined_conflict),
            ("count_pending", self.pending),
            ("count_quarantined", self.quarantined),
            ("count_unregistered", self.unregistered),
            ("count_source_tampered", self.source_tampered),
            ("count_store_only", self.store_only),
            ("count_unaudited_replay", self.unaudited_replay),
            ("count_replay_record_lost", self.replay_record_lost),
            (
                "repaired_delivered_missing",
                self.repaired_delivered_missing,
            ),
            (
                "repaired_quarantined_stored",
                self.repaired_quarantined_stored,
            ),
            ("repaired_unregistered", self.repaired_unregistered),
        ]
        .into_iter()
        .map(|(name, count)| (name.to_owned(), json!(count)))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlstate_classification_is_total_and_specific() {
        for (state, code) in [
            ("08006", OutageCode::Connection),
            ("08001", OutageCode::Connection),
            ("08P01", OutageCode::Connection),
            ("53300", OutageCode::Resources),
            ("53100", OutageCode::Resources),
            ("25006", OutageCode::ReadOnly),
            ("57P01", OutageCode::Shutdown),
            ("57P02", OutageCode::Shutdown),
            ("57P03", OutageCode::Shutdown),
            ("57P04", OutageCode::Shutdown),
            ("57P05", OutageCode::Shutdown),
            ("57014", OutageCode::Timeout),
            ("40001", OutageCode::Serialization),
            ("40P01", OutageCode::Deadlock),
            ("55P03", OutageCode::LockUnavailable),
            ("42501", OutageCode::DeployMismatch),
            ("42883", OutageCode::DeployMismatch),
            ("42P01", OutageCode::DeployMismatch),
            ("XX000", OutageCode::Internal),
            ("XX001", OutageCode::Internal),
            ("58030", OutageCode::Internal),
            ("54000", OutageCode::Internal),
            ("54001", OutageCode::Internal),
            ("23505", OutageCode::Other),
            ("22P02", OutageCode::Other),
            ("22023", OutageCode::Other),
            ("P0001", OutageCode::Other),
            ("55000", OutageCode::Other),
            ("57000", OutageCode::Other),
            ("0A000", OutageCode::Other),
            ("40002", OutageCode::Other),
            ("", OutageCode::Other),
            ("0800", OutageCode::Other),
            ("080011", OutageCode::Other),
            ("08x01", OutageCode::Other),
            ("57p01", OutageCode::Other),
            ("xx000", OutageCode::Other),
            ("é8000", OutageCode::Other),
        ] {
            assert_eq!(classify_sqlstate(state), code, "{state:?}");
        }
    }

    #[test]
    fn no_sqlstate_ever_becomes_a_terminal_error() {
        const ALPHABET: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        // Every 5-character [0-9A-Z] SQLSTATE, split by first character.
        let checked: u64 = std::thread::scope(|scope| {
            let workers: Vec<_> = ALPHABET
                .chunks(6)
                .map(|firsts| {
                    scope.spawn(move || {
                        let mut checked = 0_u64;
                        let mut buffer = [0_u8; 5];
                        for a in firsts {
                            buffer[0] = *a;
                            for b in ALPHABET {
                                buffer[1] = *b;
                                for c in ALPHABET {
                                    buffer[2] = *c;
                                    for d in ALPHABET {
                                        buffer[3] = *d;
                                        for e in ALPHABET {
                                            buffer[4] = *e;
                                            let state =
                                                std::str::from_utf8(&buffer).expect("ascii");
                                            let error =
                                                StoreError::outage(classify_sqlstate(state));
                                            assert!(
                                                error.is_outage() && !error.is_terminal(),
                                                "{state}"
                                            );
                                            checked += 1;
                                        }
                                    }
                                }
                            }
                        }
                        checked
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().expect("worker"))
                .sum()
        });
        assert_eq!(checked, 36_u64.pow(5));
    }

    fn every_error() -> Vec<StoreError> {
        let mut errors: Vec<StoreError> = OutageCode::ALL
            .into_iter()
            .map(StoreError::outage)
            .collect();
        errors.push(StoreError::Conflict);
        errors.push(StoreError::Rejected {
            code: BoundedCode::new("invalid_envelope").expect("bounded"),
        });
        errors
    }

    #[test]
    fn failure_classification_is_two_way() {
        for error in every_error() {
            assert_eq!(error.is_outage(), !error.is_terminal(), "{error:?}");
            assert_eq!(
                error.outage_code().is_some(),
                error.is_outage(),
                "{error:?}"
            );
            let terminal = matches!(error, StoreError::Conflict | StoreError::Rejected { .. });
            assert_eq!(error.is_terminal(), terminal, "{error:?}");
        }
    }

    #[test]
    fn only_residual_unexpected_codes_count_toward_the_streak() {
        for code in OutageCode::ALL {
            let expected = matches!(code, OutageCode::Internal | OutageCode::Other);
            assert_eq!(code.counts_toward_outage_streak(), expected, "{code:?}");
            assert_eq!(
                StoreError::outage(code).counts_toward_outage_streak(),
                expected
            );
        }
        assert!(!StoreError::Conflict.counts_toward_outage_streak());
    }

    fn row(status: &str) -> IngestRow {
        IngestRow {
            status: status.to_owned(),
            seq: Some(7),
            envelope_digest: Some(vec![0xab; 32]),
            adapter_version: Some(1),
            code: None,
        }
    }

    #[test]
    fn ingest_rows_decode_to_receipts_verdicts_or_outages() {
        for outcome in IngestOutcome::ALL {
            assert_eq!(
                row(outcome.as_str()).into_result(),
                Ok(IngestReceipt {
                    seq: 7,
                    envelope_digest: [0xab; 32],
                    outcome,
                    adapter_version: 1,
                })
            );
        }
        let outage = |code| Err(StoreError::outage(code));
        let with = |status: &str, edit: fn(&mut IngestRow)| {
            let mut r = row(status);
            edit(&mut r);
            r.into_result()
        };
        assert_eq!(
            with("stored", |r| r.seq = None),
            outage(OutageCode::OutcomeUnknown)
        );
        assert_eq!(
            with("stored", |r| r.seq = Some(0)),
            outage(OutageCode::OutcomeUnknown)
        );
        assert_eq!(
            with("duplicate", |r| r.envelope_digest = Some(vec![1; 31])),
            outage(OutageCode::OutcomeUnknown)
        );
        assert_eq!(
            with("duplicate_expired", |r| r.envelope_digest = None),
            outage(OutageCode::OutcomeUnknown)
        );
        assert_eq!(
            with("duplicate_reprojected", |r| r.adapter_version = None),
            outage(OutageCode::OutcomeUnknown)
        );
        assert_eq!(row("conflict").into_result(), Err(StoreError::Conflict));
        assert_eq!(
            with("rejected", |r| r.code = Some("invalid_envelope".to_owned())),
            Err(StoreError::Rejected {
                code: BoundedCode::new("invalid_envelope").expect("bounded")
            })
        );
        assert_eq!(row("rejected").into_result(), outage(OutageCode::Other));
        assert_eq!(
            with("rejected", |r| r.code = Some("Free Text".to_owned())),
            outage(OutageCode::Other)
        );
        assert_eq!(
            row("recovery_required").into_result(),
            outage(OutageCode::RecoveryRequired)
        );
        assert_eq!(
            with("outage", |r| r.code = Some("unregistered_type".to_owned())),
            outage(OutageCode::UnregisteredType)
        );
        assert_eq!(
            with("outage", |r| r.code =
                Some("store_posture_invalid".to_owned())),
            outage(OutageCode::PostureInvalid)
        );
        assert_eq!(
            with("outage", |r| r.code = Some("regressed".to_owned())),
            outage(OutageCode::Regressed)
        );
        assert_eq!(
            with("outage", |r| r.code = Some("something_new".to_owned())),
            outage(OutageCode::Other)
        );
        assert_eq!(row("outage").into_result(), outage(OutageCode::Other));
        assert_eq!(
            with("denied", |r| r.code = Some("not_source_service".to_owned())),
            outage(OutageCode::Denied)
        );
        for status in ["", "STORED", "ok", "error", "stored "] {
            assert_eq!(
                row(status).into_result(),
                outage(OutageCode::Other),
                "{status:?}"
            );
        }
    }

    #[test]
    fn ingest_row_debug_prints_only_bounded_codes() {
        let mut r = row("rejected");
        r.code = Some("secret text".to_owned());
        let debug = format!("{r:?}");
        assert!(debug.contains("rejected"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn bounded_code_refuses_text() {
        assert!(BoundedCode::new("conflict_v1").is_some());
        assert!(BoundedCode::new("").is_none());
        assert!(BoundedCode::new("has space").is_none());
        assert!(BoundedCode::new("Upper").is_none());
        assert!(BoundedCode::new(&"a".repeat(65)).is_none());
    }

    #[test]
    fn names_are_unique_prefixed_and_round_trip() {
        for outcome in IngestOutcome::ALL {
            assert_eq!(IngestOutcome::parse(outcome.as_str()), Some(outcome));
        }
        let mut seen = std::collections::BTreeSet::new();
        for code in OutageCode::ALL {
            assert!(code.as_str().starts_with("store_"), "{code:?}");
            assert!(seen.insert(code.as_str()), "{code:?}");
            assert_eq!(OutageCode::parse(code.as_str()), Some(code));
        }
    }

    #[test]
    fn store_state_maps_to_outage_codes() {
        assert_eq!(StoreState::Operational.outage_code(), None);
        assert_eq!(
            StoreState::RecoveryMode.outage_code(),
            Some(OutageCode::RecoveryRequired)
        );
        assert_eq!(
            StoreState::PostureInvalid.outage_code(),
            Some(OutageCode::PostureInvalid)
        );
        assert_eq!(
            StoreState::ReadOnly.outage_code(),
            Some(OutageCode::ReadOnly)
        );
        for state in StoreState::ALL {
            assert!(!state.as_str().is_empty());
        }
    }

    #[test]
    fn admission_requires_operational_unregressed_and_registered() {
        let ok = StoreStatus {
            head_seq: 10,
            recovery_epoch: 1,
            state: StoreState::Operational,
            missing_types: Vec::new(),
            regression_detected: false,
            last_verified_seq: Some(9),
        };
        assert_eq!(ok.admission(), Ok(()));
        let skew = StoreStatus {
            missing_types: vec!["folder.moved".to_owned()],
            ..ok.clone()
        };
        assert_eq!(skew.admission(), Err(OutageCode::UnregisteredType));
        let regressed = StoreStatus {
            regression_detected: true,
            ..skew.clone()
        };
        assert_eq!(regressed.admission(), Err(OutageCode::Regressed));
        let recovery = StoreStatus {
            state: StoreState::RecoveryMode,
            ..regressed
        };
        assert_eq!(recovery.admission(), Err(OutageCode::RecoveryRequired));
    }

    #[test]
    fn receipt_identity_is_confirmed_by_identity_not_seq() {
        let identity = ReceiptIdentity {
            seq: 5,
            event_id: Uuid::from_u128(5),
            envelope_digest: [5; 32],
        };
        let stored = ReceiptRow {
            seq: 5,
            event_id: Uuid::from_u128(5),
            origin: Origin::Relay,
            event_type: "document.created".to_owned(),
            envelope_digest: [5; 32],
            source_commitment: Some([1; 32]),
            expired: false,
            recovery_epoch: 1,
        };
        assert!(identity.is_confirmed_by(std::slice::from_ref(&stored)));
        assert!(!identity.is_confirmed_by(&[]));
        let reused_seq = ReceiptRow {
            event_id: Uuid::from_u128(6),
            ..stored.clone()
        };
        assert!(!identity.is_confirmed_by(&[reused_seq]));
        let other_digest = ReceiptRow {
            envelope_digest: [6; 32],
            ..stored
        };
        assert!(!identity.is_confirmed_by(&[other_digest]));
    }

    #[test]
    fn the_port_is_object_safe() {
        fn accepts(_: Option<&dyn AuditStore>) {}
        accepts(None);
    }
}
