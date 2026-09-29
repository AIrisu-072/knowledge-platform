use document_diff_core::{DiffProfileVersion, ResourceProfileVersion};
use document_domain::{DocumentId, DocumentVersionId, FileId};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};
use serde::Serialize;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use super::model::DiffResult;
use crate::VersionPurpose;

const SNAPSHOT_DOMAIN: &[u8] = b"document-diff-snapshot-v0\0";
const SEMANTIC_DOMAIN: &[u8] = b"document-diff-semantic-v0\0";
const CACHE_DOMAIN: &[u8] = b"document-diff-cache-v0\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotItem {
    pub content_item_id: Uuid,
    pub logical_path: String,
    pub ordinal: u32,
    pub authoritative_representation_id: Uuid,
    pub file_id: FileId,
    pub format: Option<FormatId>,
    pub inspection_profile: InspectionProfileVersion,
    pub semantic_fingerprint: Option<[u8; 32]>,
    pub inspection_binding_digest: Option<[u8; 32]>,
    pub raw_sha256: [u8; 32],
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSnapshot {
    pub document_id: DocumentId,
    pub version_id: DocumentVersionId,
    pub reference_purpose: VersionPurpose,
    pub document_revision: u64,
    pub title: String,
    pub items: Vec<SnapshotItem>,
    pub manifest_fingerprint: [u8; 32],
    pub version_metadata_digest: [u8; 32],
}

impl VersionSnapshot {
    pub fn snapshot_digest(&self) -> [u8; 32] {
        #[derive(Serialize)]
        struct SnapshotView<'a> {
            document_id: DocumentId,
            version_id: DocumentVersionId,
            title: String,
            items: &'a [SnapshotItem],
            manifest_fingerprint: [u8; 32],
            version_metadata_digest: [u8; 32],
        }
        digest_json(
            SNAPSHOT_DOMAIN,
            &SnapshotView {
                document_id: self.document_id,
                version_id: self.version_id,
                title: normalize_title(&self.title),
                items: &self.items,
                manifest_fingerprint: self.manifest_fingerprint,
                version_metadata_digest: self.version_metadata_digest,
            },
        )
    }

    pub fn source_binding_digest(&self) -> [u8; 32] {
        #[derive(Serialize)]
        struct SourceItem<'a> {
            content_item_id: Uuid,
            logical_path: &'a str,
            ordinal: u32,
            authoritative_representation_id: Uuid,
            file_id: FileId,
            raw_sha256: [u8; 32],
            size_bytes: u64,
        }
        let items: Vec<_> = self
            .items
            .iter()
            .map(|item| SourceItem {
                content_item_id: item.content_item_id,
                logical_path: &item.logical_path,
                ordinal: item.ordinal,
                authoritative_representation_id: item.authoritative_representation_id,
                file_id: item.file_id,
                raw_sha256: item.raw_sha256,
                size_bytes: item.size_bytes,
            })
            .collect();
        digest_json(
            b"document-diff-source-binding-v0\0",
            &(
                self.document_id,
                self.version_id,
                normalize_title(&self.title),
                &items,
                self.version_metadata_digest,
            ),
        )
    }

    pub fn semantic_identity_digest(&self) -> [u8; 32] {
        #[derive(Serialize)]
        struct SemanticItem<'a> {
            logical_path: &'a str,
            ordinal: u32,
            format: Option<FormatId>,
            inspection_profile: InspectionProfileVersion,
            semantic_fingerprint: Option<[u8; 32]>,
        }
        #[derive(Serialize)]
        struct SemanticView<'a> {
            title: String,
            manifest_fingerprint: [u8; 32],
            items: Vec<SemanticItem<'a>>,
        }
        let items = self
            .items
            .iter()
            .map(|item| SemanticItem {
                logical_path: &item.logical_path,
                ordinal: item.ordinal,
                format: item.format,
                inspection_profile: item.inspection_profile,
                semantic_fingerprint: item.semantic_fingerprint,
            })
            .collect();
        digest_json(
            SEMANTIC_DOMAIN,
            &SemanticView {
                title: normalize_title(&self.title),
                manifest_fingerprint: self.manifest_fingerprint,
                items,
            },
        )
    }
}

pub(super) fn normalize_title(value: &str) -> String {
    let line_normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    line_normalized.nfc().collect::<String>().trim().to_owned()
}

fn digest_json(domain: &[u8], value: &impl Serialize) -> [u8; 32] {
    let bytes = serde_json::to_vec(value).expect("typed snapshot contains serializable fields");
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(bytes);
    digest.finalize().into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffPairSnapshot {
    pub document_id: DocumentId,
    pub base: VersionSnapshot,
    pub target: VersionSnapshot,
}

impl DiffPairSnapshot {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.base.version_id == self.target.version_id {
            return Err("base and target versions must differ");
        }
        if self.base.items.is_empty() || self.target.items.is_empty() {
            return Err("both versions require authoritative content items");
        }
        if self.base.document_id != self.document_id || self.target.document_id != self.document_id
        {
            return Err("both versions must belong to the requested document");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiffCacheKey([u8; 32]);

impl DiffCacheKey {
    pub fn from_pair(
        pair: &DiffPairSnapshot,
        profile: DiffProfileVersion,
        resource: ResourceProfileVersion,
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(CACHE_DOMAIN);
        digest.update(pair.document_id.as_uuid().as_bytes());
        digest.update(pair.base.snapshot_digest());
        digest.update(pair.target.snapshot_digest());
        digest.update(profile.as_str().as_bytes());
        digest.update(resource.as_str().as_bytes());
        Self(digest.finalize().into())
    }

    pub fn from_result(result: &DiffResult) -> Self {
        let mut digest = Sha256::new();
        digest.update(CACHE_DOMAIN);
        digest.update(result.document_id.as_uuid().as_bytes());
        digest.update(result.base_snapshot_digest);
        digest.update(result.target_snapshot_digest);
        digest.update(result.profile.as_str().as_bytes());
        digest.update(result.resource_profile.as_str().as_bytes());
        Self(digest.finalize().into())
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Stable version manifest identity; missing inspection evidence remains a visible marker.
pub fn manifest_fingerprint(title: &str, items: &[SnapshotItem]) -> [u8; 32] {
    #[derive(Serialize)]
    struct ManifestItem<'a> {
        logical_path: &'a str,
        ordinal: u32,
        format: Option<FormatId>,
        profile: InspectionProfileVersion,
        fingerprint: Option<[u8; 32]>,
    }
    let items: Vec<_> = items
        .iter()
        .map(|item| ManifestItem {
            logical_path: &item.logical_path,
            ordinal: item.ordinal,
            format: item.format,
            profile: item.inspection_profile,
            fingerprint: item.semantic_fingerprint,
        })
        .collect();
    digest_json(
        b"document-diff-manifest-v0\0",
        &(normalize_title(title), items),
    )
}
