#![forbid(unsafe_code)]
//! Document Diff worker contract and bounded comparison primitives.

mod alignment;
mod change;
mod protocol;

pub use alignment::{
    AlignedPair, AlignmentBudget, AlignmentKind, AlignmentOutcome, ItemAnchor, UnresolvedCluster,
    align_items,
};
pub use change::{
    ChangeOperation, ComparisonBudget, ContentVerdict, DiffCoverage, RelocationKind, SourceLocator,
    UnverifiedReason, WorkerChange, WorkerUnverifiedRegion,
};
pub use document_semantic_inspection_core::FormatId;
pub use protocol::{
    DiffProfileVersion, ResourceProfileVersion, WorkerDiffRequest, WorkerDiffResponse,
    WorkerProtocolVersion, decode_worker_response_bounded,
};

#[derive(Debug, thiserror::Error)]
pub enum DiffCoreError {
    #[error("invalid worker response: {0}")]
    InvalidResponse(String),
    #[error("unknown worker protocol version: {0}")]
    UnknownProtocolVersion(String),
    #[error("resource limit: {0}")]
    ResourceLimit(&'static str),
    #[error("invalid worker JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Worker-facing failure class; infrastructure errors are mapped by the runner.
pub type WorkerDiffFailure = DiffCoreError;
