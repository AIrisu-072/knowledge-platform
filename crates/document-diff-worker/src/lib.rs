#![forbid(unsafe_code)]
//! Untrusted two-source Document Diff worker.

mod adapters;
mod shell;

pub use shell::{
    MAX_REQUEST_BYTES, MAX_RESULT_BYTES, MAX_SOURCE_BYTES, decode_request_bounded,
    guard_worker_execution, run_worker_shell,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WorkerError {
    #[error("invalid worker request")]
    InvalidRequest,
    #[error("authoritative raw binding mismatch")]
    RawBindingMismatch,
    #[error("worker input is unreadable")]
    UnreadableInput,
    #[error("worker resource limit: {0}")]
    ResourceLimit(&'static str),
    #[error("invalid worker result")]
    InvalidResult,
    #[error("worker panicked")]
    WorkerPanic,
}

impl WorkerError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "malformed_request",
            Self::RawBindingMismatch => "raw_binding_mismatch",
            Self::UnreadableInput => "extractor_unavailable",
            Self::ResourceLimit(_) => "inspection_resource_limit_exceeded",
            Self::InvalidResult => "invalid_worker_result",
            Self::WorkerPanic => "extractor_unavailable",
        }
    }
}
