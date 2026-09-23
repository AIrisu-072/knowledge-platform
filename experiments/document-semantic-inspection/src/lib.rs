//! Isolated Document Semantic Inspection v0 qualification harness.
//!
//! This crate is PoC-only. It intentionally exposes common evidence metadata
//! while keeping each adapter's semantic projection opaque bytes.

pub mod adapters;
mod canonical;
mod error;
mod manifest;
mod model;
mod report;
mod runner;
mod vba_language;

pub use adapters::{AlwaysSuccessAdapter, CsvAdapter, DocxAdapter, HtmlAdapter, PptxAdapter, SpreadsheetAdapter, TextAdapter, VbaAdapter};
pub use canonical::{canonical_json_bytes, fingerprint};
pub use error::{ErrorCode, PocError};
pub use manifest::{FixtureCase, FixtureManifest, fixture_case};
pub use model::{
    AdapterOutput, CapabilityEvidence, CommentEvidence, Diagnostic, EditorialEvidence,
    ExternalDependency, FormatId, InspectionAdapter, InspectionProfile, InspectionResult,
    SignatureEvidence, SignatureValidity, TrackedChangeEvidence,
};
pub use report::{CaseReport, CaseVerdict, VerificationReport, write_reports};
pub use runner::{AdapterRegistry, run_case, run_case_at, verify_manifest};
