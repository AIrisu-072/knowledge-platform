use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    DiffCoreError, DiffCoverage, WorkerAncillaryChange, WorkerChange, WorkerUnverifiedRegion,
};
use document_semantic_inspection_core::FormatId;

pub const MAX_SOURCE_BYTES_V0: u64 = 256 * 1024 * 1024;
pub const MAX_RESULT_BYTES_V0: usize = 16 * 1024 * 1024;
pub const MAX_CHANGES_V0: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkerProtocolVersion {
    #[serde(rename = "diff-worker-v0")]
    V0,
}

impl WorkerProtocolVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V0 => "diff-worker-v0",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiffProfileVersion {
    #[serde(rename = "document-diff-v0")]
    V0,
}

impl DiffProfileVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V0 => "document-diff-v0",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceProfileVersion {
    #[serde(rename = "diff-resource-v0")]
    V0,
}

impl ResourceProfileVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V0 => "diff-resource-v0",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerDiffRequest {
    pub protocol_version: WorkerProtocolVersion,
    pub diff_profile_version: DiffProfileVersion,
    pub resource_profile_version: ResourceProfileVersion,
    pub format: FormatId,
    pub base_raw_sha256: [u8; 32],
    pub base_size_bytes: u64,
    pub target_raw_sha256: [u8; 32],
    pub target_size_bytes: u64,
}

impl WorkerDiffRequest {
    pub fn validate(&self) -> Result<(), DiffCoreError> {
        if self.base_size_bytes > MAX_SOURCE_BYTES_V0
            || self.target_size_bytes > MAX_SOURCE_BYTES_V0
        {
            return Err(DiffCoreError::ResourceLimit("source bytes"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerDiffResponse {
    pub protocol_version: WorkerProtocolVersion,
    pub diff_profile_version: DiffProfileVersion,
    pub resource_profile_version: ResourceProfileVersion,
    pub base_raw_sha256: [u8; 32],
    pub base_size_bytes: u64,
    pub target_raw_sha256: [u8; 32],
    pub target_size_bytes: u64,
    pub format: FormatId,
    pub coverage: DiffCoverage,
    pub changes: Vec<WorkerChange>,
    pub unverified_regions: Vec<WorkerUnverifiedRegion>,
    #[serde(default)]
    pub ancillary_changes: Vec<WorkerAncillaryChange>,
    pub parser_provenance: String,
}

impl WorkerDiffResponse {
    pub fn validate(&self) -> Result<(), DiffCoreError> {
        if self.parser_provenance.trim().is_empty() || self.parser_provenance.len() > 4096 {
            return Err(DiffCoreError::InvalidResponse(
                "missing parser provenance".to_owned(),
            ));
        }
        if self.changes.len() > MAX_CHANGES_V0
            || self.unverified_regions.len() > MAX_CHANGES_V0
            || self.ancillary_changes.len() > MAX_CHANGES_V0
        {
            return Err(DiffCoreError::ResourceLimit("result entries"));
        }
        if self.coverage == DiffCoverage::Full && !self.unverified_regions.is_empty() {
            return Err(DiffCoreError::InvalidResponse(
                "full result has unverified regions".to_owned(),
            ));
        }
        if self.coverage == DiffCoverage::Partial && self.unverified_regions.is_empty() {
            return Err(DiffCoreError::InvalidResponse(
                "partial result lacks unverified region".to_owned(),
            ));
        }
        if self.coverage == DiffCoverage::None
            && (self.unverified_regions.is_empty() || !self.changes.is_empty())
        {
            return Err(DiffCoreError::InvalidResponse(
                "none coverage has invalid result".to_owned(),
            ));
        }
        for change in &self.changes {
            change.validate()?;
        }
        for region in &self.unverified_regions {
            region.validate()?;
        }
        for ancillary in &self.ancillary_changes {
            ancillary.validate()?;
        }
        Ok(())
    }

    pub fn validate_against(&self, request: &WorkerDiffRequest) -> Result<(), DiffCoreError> {
        request.validate()?;
        self.validate()?;
        if self.protocol_version != request.protocol_version
            || self.diff_profile_version != request.diff_profile_version
            || self.resource_profile_version != request.resource_profile_version
            || self.format != request.format
            || self.base_raw_sha256 != request.base_raw_sha256
            || self.base_size_bytes != request.base_size_bytes
            || self.target_raw_sha256 != request.target_raw_sha256
            || self.target_size_bytes != request.target_size_bytes
        {
            return Err(DiffCoreError::InvalidResponse(
                "worker response binding mismatch".to_owned(),
            ));
        }
        Ok(())
    }
}

pub fn decode_worker_response_bounded(
    bytes: &[u8],
    max_bytes: usize,
) -> Result<WorkerDiffResponse, DiffCoreError> {
    if bytes.len() > max_bytes.min(MAX_RESULT_BYTES_V0) {
        return Err(DiffCoreError::ResourceLimit("response bytes"));
    }
    let value: Value = serde_json::from_slice(bytes)?;
    let version = value
        .get("protocol_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    if version != WorkerProtocolVersion::V0.as_str() {
        return Err(DiffCoreError::UnknownProtocolVersion(version.to_owned()));
    }
    let response: WorkerDiffResponse = serde_json::from_value(value)?;
    response.validate()?;
    Ok(response)
}
