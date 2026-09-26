mod csv;
mod docx;
mod html;
mod pptx;
mod pptx_package;
mod spreadsheet;
mod spreadsheet_package;
mod text;
mod vba;
mod vba_guard;

#[cfg(test)]
mod capability_state_behavior;

use serde::Serialize;
use serde_json::{Map, Value};

pub use csv::CsvAdapter;
pub use docx::{DocxAdapter, OoxmlCoverageSentinel};
pub use html::HtmlAdapter;
pub use pptx::PptxAdapter;
pub use spreadsheet::SpreadsheetAdapter;
pub use text::TextAdapter;

use document_semantic_inspection_core::{
    CapabilityEvidence, CapabilityState, EditorialProvenance, ExternalDependency,
    ExtractorProvenance, FormatId, ParserLibraryIdentity, SemanticFingerprint,
};

use crate::{WorkerFailure, WorkerFailureCode, extractor_provenance};

pub(super) fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let value = serde_json::to_value(value)?;
    serde_json::to_vec(&canonicalize(value))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));

            let mut sorted = Map::new();
            for (key, value) in entries {
                sorted.insert(key, canonicalize(value));
            }
            Value::Object(sorted)
        }
        scalar => scalar,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdapterProfile {
    csv_delimiter: Option<u8>,
    text_encoding: Option<String>,
    html_script_required: bool,
}

impl AdapterProfile {
    pub fn with_csv_delimiter(mut self, delimiter: u8) -> Self {
        self.csv_delimiter = Some(delimiter);
        self
    }

    pub fn with_text_encoding(mut self, encoding: impl Into<String>) -> Self {
        self.text_encoding = Some(encoding.into());
        self
    }

    pub fn with_html_script_required(mut self, required: bool) -> Self {
        self.html_script_required = required;
        self
    }

    pub(crate) const fn csv_delimiter(&self) -> Option<u8> {
        self.csv_delimiter
    }

    pub(crate) fn text_encoding(&self) -> Option<&str> {
        self.text_encoding.as_deref()
    }

    pub(crate) const fn html_script_required(&self) -> bool {
        self.html_script_required
    }
}

pub trait SemanticAdapter {
    fn format(&self) -> FormatId;

    fn inspect(
        &self,
        input: &[u8],
        profile: &AdapterProfile,
    ) -> Result<SemanticAdapterOutput, WorkerFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticAdapterOutput {
    semantic_fingerprint: SemanticFingerprint,
    semantic_capabilities: Vec<CapabilityEvidence>,
    editorial_provenance: EditorialProvenance,
    external_dependencies: Vec<ExternalDependency>,
    extractor_provenance: ExtractorProvenance,
}

impl SemanticAdapterOutput {
    pub const fn semantic_fingerprint(&self) -> SemanticFingerprint {
        self.semantic_fingerprint
    }

    pub fn editorial_provenance(&self) -> &EditorialProvenance {
        &self.editorial_provenance
    }

    pub fn with_editorial_provenance(mut self, editorial: EditorialProvenance) -> Self {
        self.editorial_provenance = editorial;
        self
    }

    pub fn with_external_dependencies(mut self, dependencies: Vec<ExternalDependency>) -> Self {
        self.external_dependencies = dependencies;
        self
    }

    pub fn external_dependencies(&self) -> &[ExternalDependency] {
        &self.external_dependencies
    }

    pub(crate) fn with_capability_state(
        mut self,
        capability_id: &str,
        state: CapabilityState,
    ) -> Result<Self, WorkerFailure> {
        let Some(capability) = self
            .semantic_capabilities
            .iter_mut()
            .find(|capability| capability.capability_id == capability_id)
        else {
            return Err(WorkerFailure::new(
                WorkerFailureCode::InvalidWorkerResult,
                format!("unknown semantic capability ID: {capability_id}"),
            ));
        };

        capability.presence = state;
        capability.equivalence_fingerprint = if state == CapabilityState::Present {
            Some(self.semantic_fingerprint)
        } else {
            None
        };

        Ok(self)
    }

    pub(crate) fn semantic_capabilities(&self) -> &[CapabilityEvidence] {
        &self.semantic_capabilities
    }

    pub(crate) fn extractor_provenance(&self) -> &ExtractorProvenance {
        &self.extractor_provenance
    }

    pub(crate) fn from_projection(
        semantic_projection: &[u8],
        capability_ids: &[&str],
        adapter_id: &'static str,
        parser_libraries: &[(&'static str, &'static str)],
    ) -> Self {
        let semantic_fingerprint = SemanticFingerprint::sha256(semantic_projection);
        let semantic_capabilities = capability_ids
            .iter()
            .map(|capability_id| CapabilityEvidence {
                capability_id: (*capability_id).to_owned(),
                presence: CapabilityState::Present,
                version_significant: true,
                equivalence_fingerprint: Some(semantic_fingerprint),
            })
            .collect();
        let parser_libraries = parser_libraries
            .iter()
            .map(|(name, version)| ParserLibraryIdentity {
                name: (*name).to_owned(),
                version: (*version).to_owned(),
            })
            .collect();

        Self {
            semantic_fingerprint,
            semantic_capabilities,
            editorial_provenance: EditorialProvenance::default(),
            external_dependencies: Vec::new(),
            extractor_provenance: extractor_provenance(
                adapter_id,
                "dsi-v0",
                parser_libraries,
                Vec::new(),
            ),
        }
    }
}
