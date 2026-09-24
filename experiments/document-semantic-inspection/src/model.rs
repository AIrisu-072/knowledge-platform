use crate::PocError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum FormatId {
    Txt,
    Csv,
    Html,
    Docx,
    Xlsx,
    Xlsm,
    Pptx,
    Pdf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionProfile {
    pub id: String,
    pub csv_delimiter: Option<u8>,
    pub text_encoding: Option<String>,
    pub html_script_required: bool,
}

impl Default for InspectionProfile {
    fn default() -> Self {
        Self {
            id: "dsi-v0".to_owned(),
            csv_delimiter: None,
            text_encoding: None,
            html_script_required: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEvidence {
    pub capability: String,
    pub present: bool,
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
pub struct EditorialEvidence {
    pub tracked_changes_present: bool,
    pub comments_present: bool,
    pub tracked_changes: Vec<TrackedChangeEvidence>,
    pub comments: Vec<CommentEvidence>,
    pub document_author_labels: Vec<String>,
    pub last_modified_by: Option<String>,
    pub modification_metadata: BTreeMap<String, String>,
}

impl EditorialEvidence {
    pub fn has_unresolved_changes(&self) -> bool {
        self.tracked_changes.iter().any(|change| change.unresolved)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalDependency {
    pub kind: String,
    pub definition: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureValidity {
    Valid,
    Invalid,
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureEvidence {
    pub kind: String,
    pub signer_claim: Option<String>,
    pub certificate_subject: Option<String>,
    pub certificate_issuer: Option<String>,
    pub certificate_fingerprint: Option<String>,
    pub signed_at: Option<String>,
    pub validity: SignatureValidity,
    pub covered_content: Option<String>,
    pub validation_diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterOutput {
    pub semantic_projection: Vec<u8>,
    pub capabilities: Vec<CapabilityEvidence>,
    pub editorial: EditorialEvidence,
    pub external_dependencies: Vec<ExternalDependency>,
    pub signatures: Vec<SignatureEvidence>,
    pub diagnostics: Vec<Diagnostic>,
}

impl AdapterOutput {
    pub fn projection_only(semantic_projection: Vec<u8>) -> Self {
        Self {
            semantic_projection,
            capabilities: Vec::new(),
            editorial: EditorialEvidence::default(),
            external_dependencies: Vec::new(),
            signatures: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionResult {
    pub semantic_fingerprint: [u8; 32],
    pub output: AdapterOutput,
}

pub trait InspectionAdapter {
    fn format(&self) -> FormatId;

    fn inspect(
        &self,
        input: &[u8],
        profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError>;
}
