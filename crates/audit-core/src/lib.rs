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

pub use catalog::{Catalog, EventClass, EventSpec, Origin, ResourceType};
pub use chain::{GENESIS, chain_next, envelope_digest};
pub use codes::{Rejection, RejectionCode};
pub use envelope::{AuditEnvelope, validate_envelope};
pub use export::{
    Anchor, Checkpoint, CheckpointComparison, ExportError, ExportReport, compare_checkpoint,
    verify_export, verify_export_subset, verify_identity_chain,
};
pub use json::parse_unique;
pub use legacy::{DocumentStagingProjection, project};
pub use port::{
    AuditStore, IngestOutcome, IngestReceipt, OutageCode, StoreError, StoreStatus,
    classify_sqlstate,
};
pub use schema::generate_json_schema;
