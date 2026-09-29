use document_application::document_diff::{
    Change, ComparisonRowState, DiffResult, LocatorGranularity, SourceEvidence, UnverifiedRegion,
    project_comparison_table,
};
use document_diff_core::{
    ChangeOperation, ContentVerdict, DiffCoverage, DiffProfileVersion, ResourceProfileVersion,
    SourceLocator, UnverifiedReason,
};
use document_domain::{DocumentId, DocumentVersionId, FileId};
use document_semantic_inspection_core::InspectionProfileVersion;
use uuid::Uuid;

fn evidence(version: u128, locator: SourceLocator) -> SourceEvidence {
    SourceEvidence {
        document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
        version_id: DocumentVersionId::from_uuid(Uuid::from_u128(version)),
        content_item_id: Uuid::from_u128(version + 10),
        authoritative_representation_id: Uuid::from_u128(version + 20),
        file_id: FileId::from_uuid(Uuid::from_u128(version + 30)),
        raw_sha256: [7; 32],
        inspection_profile: InspectionProfileVersion::DsiV0,
        granularity: if matches!(locator, SourceLocator::ContentItem) {
            LocatorGranularity::ContentItem
        } else {
            LocatorGranularity::Exact
        },
        locator,
        parser_provenance: "qualified".into(),
    }
}

#[test]
fn comparison_table_preserves_old_new_location_reason_and_unverified_navigation() {
    let result = DiffResult {
        document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
        base_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(2)),
        target_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(3)),
        base_snapshot_digest: [1; 32],
        target_snapshot_digest: [2; 32],
        profile: DiffProfileVersion::V0,
        resource_profile: ResourceProfileVersion::V0,
        verdict: ContentVerdict::Different,
        coverage: DiffCoverage::Partial,
        changes: vec![Change {
            operation: Some(ChangeOperation::Modified),
            relocation: None,
            facet: "formula".into(),
            base: Some(evidence(
                2,
                SourceLocator::SheetCell {
                    sheet: "Visible".into(),
                    cell: "A1".into(),
                },
            )),
            target: Some(evidence(
                3,
                SourceLocator::SheetCell {
                    sheet: "Visible".into(),
                    cell: "A1".into(),
                },
            )),
            reason_code: "formula_changed".into(),
        }],
        unverified_regions: vec![
            UnverifiedRegion {
                base: Some(evidence(
                    2,
                    SourceLocator::SheetCell {
                        sheet: "Hidden".into(),
                        cell: "A1".into(),
                    },
                )),
                target: Some(evidence(3, SourceLocator::ContentItem)),
                reason: UnverifiedReason::UnsupportedSemanticConstruct,
                navigation_hint: Some("非表示シート全体を原本で確認".into()),
            },
            UnverifiedRegion {
                base: Some(evidence(
                    2,
                    SourceLocator::VbaModule {
                        module: "Module1".into(),
                        procedure: None,
                    },
                )),
                target: Some(evidence(
                    3,
                    SourceLocator::VbaModule {
                        module: "Module1".into(),
                        procedure: None,
                    },
                )),
                reason: UnverifiedReason::AmbiguousAlignment,
                navigation_hint: Some("VBA手順を原本で確認".into()),
            },
        ],
        ancillary_changes: vec![],
    };
    let before = result.canonical_digest();
    let rows = project_comparison_table(&result);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].state, ComparisonRowState::Confirmed);
    assert_eq!(rows[0].reason, "formula_changed");
    assert_eq!(rows[1].state, ComparisonRowState::Unverified);
    assert!(
        rows[1]
            .navigation_hint
            .as_deref()
            .unwrap()
            .contains("非表示シート")
    );
    assert_eq!(
        rows[1].target.as_ref().unwrap().granularity,
        LocatorGranularity::ContentItem
    );
    assert!(rows[2].navigation_hint.as_deref().unwrap().contains("VBA"));
    assert_eq!(result.canonical_digest(), before);
}
