//! Pure audit event contract: catalog, CloudEvents envelope and payload v1
//! validation, Document legacy projection, hash chain, export verification and
//! the Audit Store port. No database, async runtime or filesystem access.

pub mod catalog;
pub mod chain;
pub mod codes;
pub mod envelope;
pub mod export;
pub mod json;
pub mod kinds;
pub mod legacy;
pub mod port;
pub mod schema;

pub use catalog::{AdapterSpec, Catalog, EventClass, EventSpec, Origin, Requirement, ResourceType};
pub use chain::{GENESIS, chain_next, envelope_digest, expired_set_digest};
pub use codes::{Rejection, RejectionCode};
pub use envelope::{AuditEnvelope, JSONB_TEXT_LIMIT, validate_envelope};
pub use export::{
    Anchor, ChainVerdict, Checkpoint, CheckpointComparison, CheckpointFinding, EpochReview,
    EpochTransition, ExportError, ExportReport, RecoveryAssessment, RecoveryRecord,
    assess_recovery, compare_checkpoint, verify_export, verify_export_subset,
    verify_identity_chain,
};
pub use json::{jsonb_text_len, parse_unique};
pub use legacy::{DocumentStagingProjection, LEGACY_ADAPTER_VERSION, project};
pub use port::{
    AuditStore, BoundedCode, ControlReceipt, ControlReceiptRow, IngestOutcome, IngestReceipt,
    IngestRow, OutageCode, ProbeExpectation, ReceiptIdentity, ReceiptRow, ReconcileCounts,
    ReconcileMode, RelayControl, RelayControlKind, SourceMismatchCode, StoreError, StoreState,
    StoreStatus, classify_sqlstate,
};
pub use schema::generate_json_schema;
