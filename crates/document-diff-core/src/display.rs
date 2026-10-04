use serde::{Deserialize, Serialize};

use crate::{DiffCoreError, FormatId, SourceLocator, WorkerProtocolVersion};

pub const MAX_DISPLAY_FRAGMENT_BYTES_V0: usize = 16 * 1024;
pub const MAX_DISPLAY_PAGE_BYTES_V0: usize = 1024 * 1024;
pub const MAX_DISPLAY_SOURCE_BYTES_V0: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayCell {
    pub row: Option<u32>,
    pub column: Option<u32>,
    pub label: Option<String>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DisplayFragment {
    Text {
        text: String,
        truncated: bool,
        locator: SourceLocator,
    },
    Table {
        cells: Vec<DisplayCell>,
        truncated: bool,
        locator: SourceLocator,
    },
    Structural {
        summary: String,
        locator: SourceLocator,
    },
    Unavailable {
        reason: DisplayUnavailableReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayUnavailableReason {
    NonTextual,
    Unverified,
    ResourceLimit,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerDisplayRequest {
    pub protocol_version: WorkerProtocolVersion,
    pub format: FormatId,
    pub raw_sha256: [u8; 32],
    pub size_bytes: u64,
    pub locator: SourceLocator,
    pub max_fragment_bytes: u32,
}

impl WorkerDisplayRequest {
    pub fn validate(&self) -> Result<(), DiffCoreError> {
        if self.size_bytes > MAX_DISPLAY_SOURCE_BYTES_V0
            || !(32..=MAX_DISPLAY_FRAGMENT_BYTES_V0 as u32).contains(&self.max_fragment_bytes)
        {
            return Err(DiffCoreError::ResourceLimit("display request bounds"));
        }
        self.locator.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerDisplayResponse {
    pub protocol_version: WorkerProtocolVersion,
    pub format: FormatId,
    pub raw_sha256: [u8; 32],
    pub size_bytes: u64,
    pub fragment: DisplayFragment,
}

impl WorkerDisplayResponse {
    pub fn validate_against(&self, request: &WorkerDisplayRequest) -> Result<(), DiffCoreError> {
        request.validate()?;
        if self.protocol_version != request.protocol_version
            || self.format != request.format
            || self.raw_sha256 != request.raw_sha256
            || self.size_bytes != request.size_bytes
            || serialized_fragment_bytes(&self.fragment) > request.max_fragment_bytes as usize
        {
            return Err(DiffCoreError::InvalidResponse(
                "display response binding or size mismatch".into(),
            ));
        }
        match &self.fragment {
            DisplayFragment::Text { locator, .. }
            | DisplayFragment::Table { locator, .. }
            | DisplayFragment::Structural { locator, .. }
                if locator != &request.locator =>
            {
                Err(DiffCoreError::InvalidResponse(
                    "display locator mismatch".into(),
                ))
            }
            _ => Ok(()),
        }
    }
}

pub fn serialized_fragment_bytes(fragment: &DisplayFragment) -> usize {
    serde_json::to_vec(fragment)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}
