use document_diff_core::{
    ChangeOperation, ContentVerdict, DiffCoverage, DiffProfileVersion, RelocationKind,
    ResourceProfileVersion, SourceLocator, UnverifiedReason,
};
use document_domain::{DocumentId, DocumentVersionId, FileId};
use document_semantic_inspection_core::InspectionProfileVersion;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const RESULT_DOMAIN: &[u8] = b"document-diff-result-v0\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffRequest {
    pub document_id: DocumentId,
    pub base_version_id: DocumentVersionId,
    pub target_version_id: DocumentVersionId,
    pub profile: DiffProfileVersion,
}

impl DiffRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.base_version_id == self.target_version_id {
            return Err("base and target versions must differ");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocatorGranularity {
    Exact,
    Parent,
    ContentItem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEvidence {
    pub document_id: DocumentId,
    pub version_id: DocumentVersionId,
    pub content_item_id: Uuid,
    pub authoritative_representation_id: Uuid,
    pub file_id: FileId,
    pub raw_sha256: [u8; 32],
    pub inspection_profile: InspectionProfileVersion,
    pub locator: SourceLocator,
    pub granularity: LocatorGranularity,
    pub parser_provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub operation: Option<ChangeOperation>,
    pub relocation: Option<RelocationKind>,
    pub facet: String,
    pub base: Option<SourceEvidence>,
    pub target: Option<SourceEvidence>,
    pub reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnverifiedRegion {
    pub base: Option<SourceEvidence>,
    pub target: Option<SourceEvidence>,
    pub reason: UnverifiedReason,
    pub navigation_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AncillaryChange {
    pub kind: String,
    pub base_digest: Option<[u8; 32]>,
    pub target_digest: Option<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffResult {
    pub document_id: DocumentId,
    pub base_version_id: DocumentVersionId,
    pub target_version_id: DocumentVersionId,
    pub base_snapshot_digest: [u8; 32],
    pub target_snapshot_digest: [u8; 32],
    pub profile: DiffProfileVersion,
    pub resource_profile: ResourceProfileVersion,
    pub verdict: ContentVerdict,
    pub coverage: DiffCoverage,
    pub changes: Vec<Change>,
    pub unverified_regions: Vec<UnverifiedRegion>,
    pub ancillary_changes: Vec<AncillaryChange>,
}

impl DiffResult {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.base_version_id == self.target_version_id {
            return Err("base and target versions must differ");
        }
        match (self.verdict, self.coverage) {
            (ContentVerdict::Same, DiffCoverage::Full)
                if self.changes.is_empty() && self.unverified_regions.is_empty() => {}
            (ContentVerdict::Different, DiffCoverage::Full)
                if !self.changes.is_empty() && self.unverified_regions.is_empty() => {}
            (ContentVerdict::Different, DiffCoverage::Partial)
                if !self.changes.is_empty() && !self.unverified_regions.is_empty() => {}
            (ContentVerdict::Unknown, DiffCoverage::Partial)
                if self.changes.is_empty() && !self.unverified_regions.is_empty() => {}
            (ContentVerdict::Unknown, DiffCoverage::None)
                if self.changes.is_empty() && !self.unverified_regions.is_empty() => {}
            _ => return Err("inconsistent verdict and coverage"),
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("typed result contains serializable fields")
    }

    pub fn canonical_digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(RESULT_DOMAIN);
        digest.update(self.canonical_bytes());
        digest.finalize().into()
    }
}
