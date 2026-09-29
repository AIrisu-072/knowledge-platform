use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId, RelocationKind,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest,
    WorkerProtocolVersion,
};
use document_diff_worker::PptxComparator;
use sha2::{Digest, Sha256};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const SLIDE_ADD: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/slide-add.pptx"
);
const SLIDE_REMOVE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/slide-remove.pptx"
);
const SLIDE_ORDER: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/slide-order-change.pptx"
);
const TEXT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/text-change.pptx"
);
const SHAPE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/shape-association-change.pptx"
);
const GROUP: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/group-change.pptx"
);
const TABLE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/table-change.pptx"
);
const CHART: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/chart-change.pptx"
);
const SMARTART: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/smartart-change.pptx"
);
const IMAGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/image-change.pptx"
);
const LINK: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/hyperlink-change.pptx"
);
const NOTES: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/speaker-note-change.pptx"
);
const COMMENT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/comment-only.pptx"
);
const THEME: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/theme-only.pptx"
);
const FONT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/font-only.pptx"
);
const BACKGROUND: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/background-only.pptx"
);
const ID_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/id-order-noise.pptx"
);
const UNKNOWN: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/unknown-semantic-part.pptx"
);

fn compare(base: &[u8], target: &[u8]) -> document_diff_core::WorkerDiffResponse {
    let request = WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Pptx,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    };
    let mut budget = ComparisonBudget::new(8_000_000, 100_000);
    let result = PptxComparator::compare(&request, base, target, &mut budget).unwrap();
    result.validate_against(&request).unwrap();
    result
}

#[test]
fn slide_add_remove_and_unique_order_changes_are_located() {
    let added = compare(BASE, SLIDE_ADD);
    assert!(
        added
            .changes
            .iter()
            .any(|change| change.facet == "pptx_slide"
                && change.operation == Some(ChangeOperation::Added))
    );
    let removed = compare(BASE, SLIDE_REMOVE);
    assert!(
        removed
            .changes
            .iter()
            .any(|change| change.facet == "pptx_slide"
                && change.operation == Some(ChangeOperation::Removed))
    );
    let reordered = compare(BASE, SLIDE_ORDER);
    assert!(
        reordered
            .changes
            .iter()
            .any(|change| change.facet == "pptx_slide"
                && change.relocation == Some(RelocationKind::Reordered))
    );
}

#[test]
fn text_shape_group_and_meaningful_layout_are_separate() {
    for (target, facet) in [
        (TEXT, "pptx_text"),
        (SHAPE, "pptx_shape"),
        (GROUP, "pptx_group"),
    ] {
        let result = compare(BASE, target);
        assert_ne!(result.coverage, DiffCoverage::None, "{facet}");
        assert!(
            result.changes.iter().any(|change| change.facet == facet),
            "{facet}"
        );
        assert!(
            result
                .changes
                .iter()
                .any(|change| matches!(change.base, Some(SourceLocator::SlideObject { .. })))
        );
    }
}

#[test]
fn table_chart_smartart_image_link_and_notes_have_distinct_facets() {
    for (target, facet) in [
        (TABLE, "pptx_table"),
        (CHART, "pptx_chart"),
        (SMARTART, "pptx_smartart"),
        (IMAGE, "pptx_image"),
        (LINK, "pptx_link"),
        (NOTES, "pptx_notes"),
    ] {
        let result = compare(BASE, target);
        assert_ne!(result.coverage, DiffCoverage::None, "{facet}");
        assert!(
            result.changes.iter().any(|change| change.facet == facet),
            "{facet}"
        );
    }
}

#[test]
fn decorative_noise_is_equal_and_comment_is_ancillary() {
    for target in [THEME, FONT, BACKGROUND, ID_NOISE] {
        let result = compare(BASE, target);
        assert_eq!(result.coverage, DiffCoverage::Full);
        assert!(result.changes.is_empty());
    }
    let comments = compare(BASE, COMMENT);
    assert_eq!(comments.coverage, DiffCoverage::Full);
    assert!(comments.changes.is_empty());
    assert!(!comments.ancillary_changes.is_empty());
}

#[test]
fn unknown_presentation_semantics_are_unverified() {
    let result = compare(BASE, UNKNOWN);
    assert_eq!(result.coverage, DiffCoverage::None);
    assert!(result.changes.is_empty());
    assert_eq!(
        result.unverified_regions[0].reason,
        UnverifiedReason::UnsupportedSemanticConstruct
    );
    assert_eq!(
        result.unverified_regions[0].base,
        Some(SourceLocator::ContentItem)
    );
}
