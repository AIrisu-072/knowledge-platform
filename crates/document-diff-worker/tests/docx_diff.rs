use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest,
    WorkerProtocolVersion,
};
use document_diff_worker::DocxComparator;
use sha2::{Digest, Sha256};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/docx/base.docx");
const BODY: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/body-text-change.docx"
);
const HEADING_LIST: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/heading-list-change.docx"
);
const TABLE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/table-merge-change.docx"
);
const HEADER: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/header-change.docx"
);
const FOOTER: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/footer-change.docx"
);
const FOOTNOTE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/footnote-change.docx"
);
const ENDNOTE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/endnote-change.docx"
);
const LINK: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/hyperlink-target-change.docx"
);
const IMAGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/image-content-change.docx"
);
const SECTION: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/section-change.docx"
);
const TRACKED: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/tracked-replacement.docx"
);
const COMMENT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/comment-resolved.docx"
);
const METADATA: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/metadata-noise.docx"
);
const ORDER_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/package-order-noise.docx"
);
const REL_ID_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/relationship-id-noise.docx"
);
const UNKNOWN: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/unknown-semantic-part.docx"
);

fn request(base: &[u8], target: &[u8]) -> WorkerDiffRequest {
    WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Docx,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    }
}

fn compare(base: &[u8], target: &[u8]) -> document_diff_core::WorkerDiffResponse {
    let request = request(base, target);
    let mut budget = ComparisonBudget::new(100_000, 100_000);
    let result = DocxComparator::compare(&request, base, target, &mut budget).unwrap();
    result.validate_against(&request).unwrap();
    result
}

fn has_facet_at_office_path(result: &document_diff_core::WorkerDiffResponse, facet: &str) -> bool {
    result.changes.iter().any(|change| {
        change.facet == facet
            && matches!(change.base, Some(SourceLocator::OfficePath { .. }))
            && matches!(change.target, Some(SourceLocator::OfficePath { .. }))
    })
}

#[test]
fn paragraph_heading_list_and_table_changes_are_located() {
    let body = compare(BASE, BODY);
    assert_eq!(body.coverage, DiffCoverage::Full);
    assert!(has_facet_at_office_path(&body, "docx_paragraph"));
    assert!(
        body.changes
            .iter()
            .any(|change| change.operation == Some(ChangeOperation::Modified))
    );

    let heading_list = compare(BASE, HEADING_LIST);
    assert!(
        heading_list
            .changes
            .iter()
            .any(|change| { matches!(change.facet.as_str(), "docx_heading" | "docx_list") })
    );

    let table = compare(BASE, TABLE);
    assert!(has_facet_at_office_path(&table, "docx_table"));
}

#[test]
fn headers_footers_notes_links_images_and_sections_are_distinct_facets() {
    for (target, facet) in [
        (HEADER, "docx_header"),
        (FOOTER, "docx_footer"),
        (FOOTNOTE, "docx_footnote"),
        (ENDNOTE, "docx_endnote"),
        (LINK, "docx_link"),
        (IMAGE, "docx_image"),
        (SECTION, "docx_section"),
    ] {
        let result = compare(BASE, target);
        assert_ne!(result.coverage, DiffCoverage::None, "{facet}");
        assert!(has_facet_at_office_path(&result, facet), "{facet}");
    }
}

#[test]
fn package_noise_is_equal_and_editorial_only_change_is_ancillary() {
    for target in [METADATA, ORDER_NOISE, REL_ID_NOISE] {
        let result = compare(BASE, target);
        assert_eq!(result.coverage, DiffCoverage::Full);
        assert!(result.changes.is_empty());
    }
    for target in [TRACKED, COMMENT] {
        let result = compare(BASE, target);
        assert_eq!(result.coverage, DiffCoverage::Full);
        assert!(result.changes.is_empty());
        assert!(!result.ancillary_changes.is_empty());
    }
}

#[test]
fn unknown_ooxml_semantics_are_unverified_with_original_navigation() {
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
    assert_eq!(
        result.unverified_regions[0].target,
        Some(SourceLocator::ContentItem)
    );
}
