#[path = "support/management.rs"]
mod support;

use document_application::{
    RepositoryError,
    document_diff::{
        AncillaryChange, DiffCache, DiffCacheKey, DiffResult, LocatorGranularity, SourceEvidence,
        UnverifiedRegion,
    },
};
use document_diff_core::{
    ContentVerdict, DiffCoverage, DiffProfileVersion, ResourceProfileVersion, SourceLocator,
    UnverifiedReason,
};
use document_domain::{DocumentId, DocumentVersionId, FileId};
use document_semantic_inspection_core::InspectionProfileVersion;
use support::fixture;
use uuid::Uuid;

fn result(seed: u8) -> DiffResult {
    DiffResult {
        document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
        base_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(2)),
        target_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(3)),
        base_snapshot_digest: [seed; 32],
        target_snapshot_digest: [4; 32],
        profile: DiffProfileVersion::V0,
        resource_profile: ResourceProfileVersion::V0,
        verdict: ContentVerdict::Same,
        coverage: DiffCoverage::Full,
        changes: vec![],
        unverified_regions: vec![],
        ancillary_changes: vec![],
    }
}

#[tokio::test]
async fn exact_snapshot_key_reuses_result_and_working_change_misses() {
    let f = fixture().await;
    let first = result(1);
    let key = DiffCacheKey::from_result(&first);
    f.repository
        .put(key, first.clone(), first.canonical_digest())
        .await
        .unwrap();
    assert_eq!(f.repository.get(&key).await.unwrap(), Some(first.clone()));
    let changed = result(2);
    assert_ne!(key, DiffCacheKey::from_result(&changed));
    assert_eq!(
        f.repository
            .get(&DiffCacheKey::from_result(&changed))
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        f.repository
            .put(key, first.clone(), [0; 32])
            .await
            .unwrap_err(),
        RepositoryError::IntegrityViolation
    );
}

#[tokio::test]
async fn entry_and_count_bounds_hold_and_partial_reason_round_trips() {
    let f = fixture().await;
    let mut large = result(1);
    large.ancillary_changes.push(AncillaryChange {
        kind: "x".repeat(16 * 1024 * 1024 + 1),
        base_digest: None,
        target_digest: None,
    });
    let large_key = DiffCacheKey::from_result(&large);
    f.repository
        .put(large_key, large.clone(), large.canonical_digest())
        .await
        .unwrap();
    assert_eq!(f.repository.get(&large_key).await.unwrap(), None);

    for seed in 0..65_u8 {
        let value = result(seed);
        f.repository
            .put(
                DiffCacheKey::from_result(&value),
                value.clone(),
                value.canonical_digest(),
            )
            .await
            .unwrap();
    }
    assert_eq!(
        f.repository
            .get(&DiffCacheKey::from_result(&result(0)))
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        f.repository
            .get(&DiffCacheKey::from_result(&result(64)))
            .await
            .unwrap(),
        Some(result(64))
    );

    let mut partial = result(88);
    partial.verdict = ContentVerdict::Unknown;
    partial.coverage = DiffCoverage::None;
    partial.unverified_regions.push(UnverifiedRegion {
        base: Some(SourceEvidence {
            document_id: partial.document_id,
            version_id: partial.base_version_id,
            content_item_id: Uuid::from_u128(20),
            authoritative_representation_id: Uuid::from_u128(21),
            file_id: FileId::from_uuid(Uuid::from_u128(22)),
            raw_sha256: [7; 32],
            inspection_profile: InspectionProfileVersion::DsiV0,
            locator: SourceLocator::ContentItem,
            granularity: LocatorGranularity::ContentItem,
            parser_provenance: "test".into(),
        }),
        target: None,
        reason: UnverifiedReason::AmbiguousAlignment,
        navigation_hint: Some("旧版原本を確認".into()),
    });
    let key = DiffCacheKey::from_result(&partial);
    f.repository
        .put(key, partial.clone(), partial.canonical_digest())
        .await
        .unwrap();
    assert_eq!(f.repository.get(&key).await.unwrap(), Some(partial));
}
