use document_application::document_diff::{
    DiffCacheKey, DiffPairSnapshot, DiffRequest, DiffResult, LocatorGranularity, SnapshotItem,
    SourceEvidence, UnverifiedRegion, VersionSnapshot,
};
use document_diff_core::{
    ContentVerdict, DiffCoverage, DiffProfileVersion, ResourceProfileVersion, SourceLocator,
    UnverifiedReason,
};
use document_domain::{DocumentId, DocumentVersionId, FileId};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};
use uuid::Uuid;

fn document_id() -> DocumentId {
    DocumentId::from_uuid(Uuid::from_u128(10))
}

fn item(path: &str, ordinal: u32) -> SnapshotItem {
    SnapshotItem {
        content_item_id: Uuid::from_u128(100 + u128::from(ordinal)),
        logical_path: path.to_owned(),
        ordinal,
        authoritative_representation_id: Uuid::from_u128(200 + u128::from(ordinal)),
        file_id: FileId::from_uuid(Uuid::from_u128(300 + u128::from(ordinal))),
        format: Some(FormatId::Txt),
        inspection_profile: InspectionProfileVersion::DsiV0,
        semantic_fingerprint: Some([7; 32]),
        inspection_binding_digest: Some([8; 32]),
        raw_sha256: [9; 32],
        size_bytes: 100,
    }
}

fn version(version: u128, title: &str) -> VersionSnapshot {
    VersionSnapshot {
        document_id: document_id(),
        version_id: DocumentVersionId::from_uuid(Uuid::from_u128(version)),
        reference_purpose: document_application::VersionPurpose::History,
        document_revision: 5,
        title: title.to_owned(),
        items: vec![item("part/a.txt", 0), item("part/b.txt", 1)],
        manifest_fingerprint: [3; 32],
        version_metadata_digest: [4; 32],
    }
}

fn pair() -> DiffPairSnapshot {
    DiffPairSnapshot {
        document_id: document_id(),
        base: version(11, "Cafe\u{301}\r\nReview"),
        target: version(12, "Target"),
    }
}

#[test]
fn normalized_title_is_stable_but_content_change_is_not() {
    let base = version(11, "Cafe\u{301}\r\nReview");
    let equivalent = version(11, "Café\nReview");
    assert_eq!(
        base.semantic_identity_digest(),
        equivalent.semantic_identity_digest()
    );
    assert_eq!(base.snapshot_digest(), equivalent.snapshot_digest());

    let changed = version(11, "Café\nApproved");
    assert_ne!(
        base.semantic_identity_digest(),
        changed.semantic_identity_digest()
    );
    assert_ne!(base.snapshot_digest(), changed.snapshot_digest());
}

#[test]
fn snapshot_binds_manifest_and_raw_but_revision_is_only_a_concurrency_token() {
    let base = version(11, "Title");
    let mut changed = base.clone();
    changed.items.reverse();
    assert_ne!(base.snapshot_digest(), changed.snapshot_digest());
    assert_ne!(
        base.semantic_identity_digest(),
        changed.semantic_identity_digest()
    );

    let mut changed = base.clone();
    changed.items[0].raw_sha256 = [42; 32];
    assert_ne!(base.snapshot_digest(), changed.snapshot_digest());
    assert_eq!(
        base.semantic_identity_digest(),
        changed.semantic_identity_digest()
    );

    let mut changed = base.clone();
    changed.items[0].format = Some(FormatId::Csv);
    assert_ne!(base.snapshot_digest(), changed.snapshot_digest());
    assert_ne!(
        base.semantic_identity_digest(),
        changed.semantic_identity_digest()
    );

    let mut changed = base.clone();
    changed.items[0].semantic_fingerprint = Some([44; 32]);
    assert_ne!(base.snapshot_digest(), changed.snapshot_digest());
    assert_ne!(
        base.semantic_identity_digest(),
        changed.semantic_identity_digest()
    );

    let mut changed = base.clone();
    changed.items[0].inspection_binding_digest = None;
    assert_ne!(base.snapshot_digest(), changed.snapshot_digest());

    let mut changed = base.clone();
    changed.version_metadata_digest = [45; 32];
    assert_ne!(base.snapshot_digest(), changed.snapshot_digest());
    assert_eq!(
        base.semantic_identity_digest(),
        changed.semantic_identity_digest()
    );

    let mut changed = base.clone();
    changed.document_revision += 1;
    assert_eq!(base.snapshot_digest(), changed.snapshot_digest());
}

#[test]
fn cache_key_is_directional_and_profile_bound() {
    let pair = pair();
    assert!(pair.validate().is_ok());
    let base = DiffCacheKey::from_pair(&pair, DiffProfileVersion::V0, ResourceProfileVersion::V0);
    let reversed = DiffPairSnapshot {
        document_id: pair.document_id,
        base: pair.target.clone(),
        target: pair.base.clone(),
    };
    assert_ne!(
        base,
        DiffCacheKey::from_pair(
            &reversed,
            DiffProfileVersion::V0,
            ResourceProfileVersion::V0
        )
    );

    let mut changed = pair.clone();
    changed.target.items[0].raw_sha256 = [55; 32];
    assert_ne!(
        base,
        DiffCacheKey::from_pair(&changed, DiffProfileVersion::V0, ResourceProfileVersion::V0)
    );
    assert_eq!(
        pair.target.semantic_identity_digest(),
        changed.target.semantic_identity_digest()
    );

    let same_version = DiffPairSnapshot {
        target: pair.base.clone(),
        ..pair.clone()
    };
    assert!(same_version.validate().is_err());
    let mut wrong_document = pair;
    wrong_document.target.document_id = DocumentId::from_uuid(Uuid::from_u128(999));
    assert!(wrong_document.validate().is_err());
}

#[test]
fn request_carries_only_two_distinct_versions_of_one_document() {
    let request = DiffRequest {
        document_id: document_id(),
        base_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(11)),
        target_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(12)),
        profile: DiffProfileVersion::V0,
    };
    assert!(request.validate().is_ok());
    assert!(
        DiffRequest {
            target_version_id: request.base_version_id,
            ..request
        }
        .validate()
        .is_err()
    );
}

#[test]
fn canonical_result_digest_changes_with_verdict_and_not_with_transient_audit() {
    let pair = pair();
    let result = DiffResult {
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
    };
    assert_eq!(result.canonical_digest(), result.clone().canonical_digest());
    assert!(result.validate().is_ok());
    let mut incomplete = result.clone();
    incomplete.coverage = DiffCoverage::Partial;
    assert!(incomplete.validate().is_err());

    let mut incorrectly_different = result.clone();
    incorrectly_different.verdict = ContentVerdict::Different;
    incorrectly_different.coverage = DiffCoverage::Partial;
    incorrectly_different
        .unverified_regions
        .push(UnverifiedRegion {
            base: Some(SourceEvidence {
                document_id: pair.document_id,
                version_id: pair.base.version_id,
                content_item_id: pair.base.items[0].content_item_id,
                authoritative_representation_id: pair.base.items[0].authoritative_representation_id,
                file_id: pair.base.items[0].file_id,
                raw_sha256: pair.base.items[0].raw_sha256,
                inspection_profile: InspectionProfileVersion::DsiV0,
                locator: SourceLocator::ContentItem,
                granularity: LocatorGranularity::ContentItem,
                parser_provenance: "identity-v0".to_owned(),
            }),
            target: None,
            reason: UnverifiedReason::ResourceLimit,
            navigation_hint: None,
        });
    assert!(incorrectly_different.validate().is_err());
    let mut changed = result.clone();
    changed.verdict = ContentVerdict::Different;
    assert_ne!(changed.canonical_digest(), result.canonical_digest());
}
