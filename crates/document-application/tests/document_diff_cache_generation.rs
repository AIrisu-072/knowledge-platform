use document_application::{
    VersionPurpose,
    document_diff::{
        DiffCacheKey, DiffPairSnapshot, DiffResult, SnapshotItem, VersionSnapshot,
        manifest_fingerprint,
    },
};
use document_diff_core::{
    ContentVerdict, DiffCoverage, DiffProfileVersion, ResourceProfileVersion,
};
use document_domain::{DocumentId, DocumentVersionId, FileId};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn pair(format: FormatId) -> DiffPairSnapshot {
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version = |id| VersionSnapshot {
        document_id,
        version_id: DocumentVersionId::from_uuid(Uuid::from_u128(id)),
        reference_purpose: VersionPurpose::History,
        document_revision: 5,
        title: "Official notice".to_owned(),
        items: vec![SnapshotItem {
            content_item_id: Uuid::from_u128(100),
            logical_path: "notice.pdf".to_owned(),
            ordinal: 0,
            authoritative_representation_id: Uuid::from_u128(200),
            file_id: FileId::from_uuid(Uuid::from_u128(300)),
            format: Some(format),
            inspection_profile: InspectionProfileVersion::DsiV0,
            semantic_fingerprint: Some([7; 32]),
            inspection_binding_digest: Some([8; 32]),
            raw_sha256: [9; 32],
            size_bytes: 100,
        }],
        manifest_fingerprint: [3; 32],
        version_metadata_digest: [4; 32],
    };
    DiffPairSnapshot {
        document_id,
        base: version(11),
        target: version(12),
    }
}

fn result(pair: &DiffPairSnapshot) -> DiffResult {
    DiffResult {
        document_id: pair.document_id,
        base_version_id: pair.base.version_id,
        target_version_id: pair.target.version_id,
        base_snapshot_digest: pair.base.snapshot_digest(),
        target_snapshot_digest: pair.target.snapshot_digest(),
        profile: DiffProfileVersion::V0,
        resource_profile: ResourceProfileVersion::V0,
        verdict: ContentVerdict::Same,
        coverage: DiffCoverage::Full,
        changes: vec![],
        unverified_regions: vec![],
        ancillary_changes: vec![],
    }
}

// Reconstruct the historical cache namespace independently of the production
// constant. Both Full and deterministic Partial entries used this same key.
fn legacy_cache_key(pair: &DiffPairSnapshot) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"document-diff-cache-v0\0");
    digest.update(pair.document_id.as_uuid().as_bytes());
    digest.update(pair.base.snapshot_digest());
    digest.update(pair.target.snapshot_digest());
    digest.update(DiffProfileVersion::V0.as_str().as_bytes());
    digest.update(ResourceProfileVersion::V0.as_str().as_bytes());
    digest.finalize().into()
}

fn digest_hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn derived_cache_coverage_generation_separates_legacy_keys_for_every_format() {
    for format in FormatId::REQUIRED_V0 {
        let pair = pair(format);
        let key =
            DiffCacheKey::from_pair(&pair, DiffProfileVersion::V0, ResourceProfileVersion::V0);
        assert_ne!(
            key.as_bytes(),
            &legacy_cache_key(&pair),
            "{format:?}: old coverage results must miss the new derived namespace"
        );
    }
}

#[test]
fn result_cache_key_also_separates_legacy_coverage_entries() {
    let pair = pair(FormatId::Pdf);
    let key = DiffCacheKey::from_result(&result(&pair));
    assert_ne!(key.as_bytes(), &legacy_cache_key(&pair));
}

#[test]
fn current_generation_pair_and_result_keys_agree_and_replay_deterministically() {
    let pair = pair(FormatId::Pdf);
    let result = result(&pair);
    assert!(result.validate().is_ok());
    let key = DiffCacheKey::from_pair(&pair, DiffProfileVersion::V0, ResourceProfileVersion::V0);
    assert_eq!(key, DiffCacheKey::from_result(&result));
    assert_eq!(
        key,
        DiffCacheKey::from_pair(
            &pair.clone(),
            DiffProfileVersion::V0,
            ResourceProfileVersion::V0,
        )
    );
    assert_eq!(key, DiffCacheKey::from_result(&result.clone()));
}

#[test]
fn coverage_generation_preserves_v0_profiles_and_existing_snapshot_identities() {
    let pair = pair(FormatId::Pdf);
    assert_eq!(InspectionProfileVersion::DsiV0.as_str(), "dsi-v0");
    assert_eq!(DiffProfileVersion::V0.as_str(), "document-diff-v0");
    assert_eq!(ResourceProfileVersion::V0.as_str(), "diff-resource-v0");
    assert_eq!(
        digest_hex(pair.base.snapshot_digest()),
        "eb6efe64bdb4b3948e85d76d9785278a6bc261f1892b12b577f33c3279bfe969"
    );
    assert_eq!(
        digest_hex(pair.target.snapshot_digest()),
        "cc4f6d7db518a8f5caa6859cf42c7fd09b8ae5843c0a72cdd1037dc753596cb7"
    );
    assert_eq!(
        digest_hex(pair.base.semantic_identity_digest()),
        "265db8850e3a8c8fae5843cefba140522aa7d7efa2510e19d4bde9e576418dca"
    );
    assert_eq!(
        digest_hex(pair.base.source_binding_digest()),
        "b1e70b7dc7c17834081a4072be947c789d7b5942ec3bff5ed0897dd6c3903bf4"
    );
    assert_eq!(
        digest_hex(manifest_fingerprint(&pair.base.title, &pair.base.items)),
        "e178195030ab2a04f93c316fcdfcd396b3e32fb9a294604893610b78002fae85"
    );
    assert_eq!(pair.base.items[0].semantic_fingerprint, Some([7; 32]));
    assert_eq!(pair.base.items[0].inspection_binding_digest, Some([8; 32]));
}
