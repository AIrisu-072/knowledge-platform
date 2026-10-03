//! Pure, bounded contract between the trusted Source host and extraction worker.
//! No file access, parser, or sandbox implementation belongs in this crate.

#![forbid(unsafe_code)]

pub mod budget;
pub mod coverage;
pub mod protocol;
pub mod validation;

pub use budget::{BudgetMeter, ExtractionBudgets};
pub use coverage::{
    BodyCoverage, CoverageReason, ItemOperationState, PermanentFailureCode, RetryableFailureCode,
};
pub use protocol::{
    NativeOmission, ReaderFailure, WorkerFragment, WorkerOperation, WorkerReport, WorkerRequest,
    WorkerResponse, decode_request, decode_response, encode_request, encode_response,
};
pub use validation::{
    ExtractionError, RegisteredProfile, checked_completed_report, resource_limit_failure,
    validate_worker_report, validate_worker_request,
};
