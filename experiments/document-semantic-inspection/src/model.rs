use crate::PocError;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EditorialEvidence {
    pub tracked_changes_present: bool,
    pub comments_present: bool,
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
    pub validity: SignatureValidity,
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
