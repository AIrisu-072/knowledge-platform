use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::SemanticFingerprint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Present,
    Absent,
    NotRepresentable,
    NotVerifiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEvidence {
    pub capability_id: String,
    pub presence: CapabilityState,
    pub version_significant: bool,
    pub equivalence_fingerprint: Option<SemanticFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedChangeEvidence {
    pub kind: String,
    pub author_label: Option<String>,
    pub timestamp: Option<String>,
    pub source_locator: String,
    pub unresolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentEvidence {
    pub author_label: Option<String>,
    pub timestamp: Option<String>,
    pub resolved_state: String,
    pub source_locator: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EditorialProvenance {
    pub tracked_changes: Vec<TrackedChangeEvidence>,
    pub comments: Vec<CommentEvidence>,
    pub document_author_labels: Vec<String>,
    pub last_modified_by: Option<String>,
    pub modification_metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalDependency {
    pub dependency_kind: String,
    pub normalized_reference: String,
    pub source_locator: String,
    pub version_significant: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureValidity {
    Valid,
    Invalid,
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigitalSignatureEvidence {
    pub signature_type: String,
    pub signer_claim: Option<String>,
    pub certificate_subject: Option<String>,
    pub certificate_issuer: Option<String>,
    pub certificate_fingerprint: Option<String>,
    pub signed_at: Option<String>,
    pub cryptographic_validity: SignatureValidity,
    pub covered_content: Vec<String>,
    pub validation_diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParserLibraryIdentity {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeDependencyIdentity {
    pub name: String,
    pub version: Option<String>,
    pub sha256: Option<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractorProvenance {
    pub worker_build_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub parser_libraries: Vec<ParserLibraryIdentity>,
    pub native_dependency_identity: Vec<NativeDependencyIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
}
