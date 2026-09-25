use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    CapabilityEvidence, CoreError, Diagnostic, DigitalSignatureEvidence, EditorialProvenance,
    ExternalDependency, ExtractorProvenance, FormatId, InspectionProfileVersion,
    SemanticFingerprint,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerProtocolVersion {
    #[serde(rename = "dsi-worker-v0")]
    V0,
}

impl WorkerProtocolVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V0 => "dsi-worker-v0",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceContext {
    pub traceparent: String,
    pub tracestate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub protocol_version: WorkerProtocolVersion,
    pub inspection_profile_version: InspectionProfileVersion,
    pub declared_media_type: String,
    pub expected_raw_content_hash: [u8; 32],
    pub expected_size_bytes: u64,
    pub trace_context: Option<TraceContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerResponse {
    pub protocol_version: WorkerProtocolVersion,
    pub inspection_profile_version: InspectionProfileVersion,
    pub observed_raw_content_hash: [u8; 32],
    pub observed_size_bytes: u64,
    pub detected_format: FormatId,
    pub semantic_fingerprint: SemanticFingerprint,
    pub semantic_capabilities: Vec<CapabilityEvidence>,
    pub editorial_provenance: EditorialProvenance,
    pub external_dependencies: Vec<ExternalDependency>,
    pub digital_signature_evidence: Vec<DigitalSignatureEvidence>,
    pub extractor_provenance: ExtractorProvenance,
    pub diagnostics: Vec<Diagnostic>,
}

impl WorkerResponse {
    pub fn validate(&self) -> Result<(), CoreError> {
        let mut capability_ids = HashSet::with_capacity(self.semantic_capabilities.len());
        for capability in &self.semantic_capabilities {
            require_non_blank("capability_id", &capability.capability_id)?;
            if !capability_ids.insert(capability.capability_id.as_str()) {
                return Err(CoreError::InvalidWorkerResult(format!(
                    "duplicate capability_id: {}",
                    capability.capability_id
                )));
            }
        }

        require_non_blank(
            "extractor_provenance.worker_build_id",
            &self.extractor_provenance.worker_build_id,
        )?;
        require_non_blank(
            "extractor_provenance.adapter_id",
            &self.extractor_provenance.adapter_id,
        )?;
        require_non_blank(
            "extractor_provenance.adapter_version",
            &self.extractor_provenance.adapter_version,
        )?;

        for parser in &self.extractor_provenance.parser_libraries {
            require_non_blank("parser_library.name", &parser.name)?;
            require_non_blank("parser_library.version", &parser.version)?;
        }
        for native in &self.extractor_provenance.native_dependency_identity {
            require_non_blank("native_dependency.name", &native.name)?;
        }

        Ok(())
    }
}

pub fn decode_worker_response_bounded(
    bytes: &[u8],
    max_bytes: usize,
) -> Result<WorkerResponse, CoreError> {
    if bytes.len() > max_bytes {
        return Err(CoreError::ResultTooLarge {
            observed: bytes.len(),
            limit: max_bytes,
        });
    }

    let value: Value = serde_json::from_slice(bytes)?;
    let protocol_version = value
        .get("protocol_version")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CoreError::InvalidWorkerResult("missing string protocol_version".to_owned())
        })?;

    if protocol_version != WorkerProtocolVersion::V0.as_str() {
        return Err(CoreError::UnknownProtocolVersion(
            protocol_version.to_owned(),
        ));
    }

    let response: WorkerResponse = serde_json::from_value(value)?;
    response.validate()?;
    Ok(response)
}

fn require_non_blank(field: &str, value: &str) -> Result<(), CoreError> {
    if value.trim().is_empty() {
        Err(CoreError::InvalidWorkerResult(format!(
            "{field} must not be blank"
        )))
    } else {
        Ok(())
    }
}
