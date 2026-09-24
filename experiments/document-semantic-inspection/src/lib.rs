//! Isolated Document Semantic Inspection v0 qualification harness.
//!
//! This crate is PoC-only. It intentionally exposes common evidence metadata
//! while keeping each adapter's semantic projection opaque bytes.

pub mod adapters;
mod canonical;
mod cross_format;
mod error;
mod manifest;
mod model;
mod report;
mod runner;
mod vba_language;

pub use adapters::{AlwaysSuccessAdapter, CsvAdapter, DocxAdapter, HtmlAdapter, PdfAdapter, PptxAdapter, SignatureInspector, SignatureTrustContext, SpreadsheetAdapter, TextAdapter, VbaAdapter};
pub use canonical::{canonical_json_bytes, fingerprint};
pub use cross_format::{assess_authority_migration, AuthorityMigrationDecision};
pub use error::{ErrorCode, PocError};
pub use manifest::{FixtureCase, FixtureClass, FixtureManifest, fixture_case};
pub use model::{
    AdapterOutput, CapabilityEvidence, CapabilityState, CommentEvidence, Diagnostic, EditorialEvidence,
    ExternalDependency, FormatId, InspectionAdapter, InspectionProfile, InspectionResult,
    SignatureEvidence, SignatureValidity, TrackedChangeEvidence,
};
pub use report::{aggregate_promotion_gates, CaseReport, CaseVerdict, ExternalGateEvidence, FormatPromotionGate, GateCount, VerificationReport, write_reports};
pub use runner::{AdapterRegistry, run_case, run_case_at, verify_manifest};
