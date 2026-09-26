//! Production contract types for Document Semantic Inspection v0.
//!
//! This crate is infrastructure-free. It owns the stable semantic-inspection
//! value types and worker wire contract, but no parser, storage, SQL, or OS
//! sandbox implementation.

mod canonical;
mod cross_format;
mod error;
mod evidence;
mod fingerprint;
mod format;
mod profile;
mod protocol;

pub use canonical::canonical_worker_response_bytes;
pub use cross_format::{AuthorityMigrationDecision, assess_authority_migration};
pub use error::CoreError;
pub use evidence::{
    CapabilityEvidence, CapabilityState, CommentEvidence, Diagnostic, DigitalSignatureEvidence,
    EditorialProvenance, ExternalDependency, ExtractorProvenance, NativeDependencyIdentity,
    ParserLibraryIdentity, SignatureValidity, TrackedChangeEvidence,
};
pub use fingerprint::{FingerprintAlgorithm, SemanticFingerprint};
pub use format::FormatId;
pub use profile::InspectionProfileVersion;
pub use protocol::{
    TraceContext, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
    decode_worker_response_bounded,
};
