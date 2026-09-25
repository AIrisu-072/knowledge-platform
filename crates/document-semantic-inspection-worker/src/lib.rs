//! Production worker shell for Document Semantic Inspection v0.
//!
//! This crate owns the untrusted parser-process boundary. Task 3 implements
//! request decoding, raw-binding verification, content-based format detection,
//! inherited read-only input handling, panic containment, and provenance only.
//! Format-specific semantic adapters are promoted in later tasks.

mod detect;
mod error;
mod input;
mod provenance;

pub use detect::detect_format;
pub use error::{WorkerFailure, WorkerFailureCode};
pub use input::{
    PreparedInput, decode_request_bounded, guard_worker_execution, open_inherited_input,
    prepare_input_bounded,
};
pub use provenance::{extractor_provenance, worker_build_id};
