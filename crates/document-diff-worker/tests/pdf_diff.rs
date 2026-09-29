use document_diff_core::{
    ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId, RelocationKind,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest,
    WorkerProtocolVersion,
};
use document_diff_worker::PdfComparator;
use sha2::{Digest, Sha256};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf");
const TEXT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/text-change.pdf"
);
const PAGE_ORDER: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/page-order-change.pdf"
);
const IMAGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/image-change.pdf"
);
const LINK: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/link-change.pdf"
);
const FORM: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/form-value-change.pdf"
);
const NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/object-id-producer-noise.pdf"
);
const SCAN: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/scan-only.pdf");
const AMBIGUOUS: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/ambiguous-read-order.pdf"
);
const BROKEN: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/broken-xref.pdf"
);

fn compare(base: &[u8], target: &[u8]) -> document_diff_core::WorkerDiffResponse {
    let request = WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Pdf,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    };
    let mut budget = ComparisonBudget::new(8_000_000, 100_000);
    let result = PdfComparator::compare(&request, base, target, &mut budget).unwrap();
    result.validate_against(&request).unwrap();
    result
}

#[test]
fn native_text_change_and_page_reordering_keep_page_locations() {
    let text = compare(BASE, TEXT);
    assert_eq!(text.coverage, DiffCoverage::Full);
    assert!(text.changes.iter().any(|change| {
        change.facet == "pdf_text"
            && matches!(change.base, Some(SourceLocator::PdfPage { page: 1, .. }))
            && matches!(change.target, Some(SourceLocator::PdfPage { page: 1, .. }))
    }));

    let order = compare(BASE, PAGE_ORDER);
    assert_eq!(order.coverage, DiffCoverage::Full);
    assert!(order.changes.iter().any(|change| {
        change.facet == "pdf_page" && change.relocation == Some(RelocationKind::Reordered)
    }));
}

#[test]
fn visual_link_and_form_changes_have_separate_facets() {
    for (target, facet) in [
        (IMAGE, "pdf_visual"),
        (LINK, "pdf_link"),
        (FORM, "pdf_form"),
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
fn producer_and_object_id_noise_does_not_change_content() {
    let result = compare(BASE, NOISE);
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert!(result.changes.is_empty());
    assert!(result.unverified_regions.is_empty());
}

#[test]
fn scan_ambiguous_read_order_and_corruption_remain_unverified() {
    for (target, reason) in [
        (SCAN, UnverifiedReason::UnsupportedSemanticConstruct),
        (AMBIGUOUS, UnverifiedReason::UnsupportedSemanticConstruct),
        (BROKEN, UnverifiedReason::CorruptedSource),
    ] {
        let result = compare(BASE, target);
        assert_eq!(result.coverage, DiffCoverage::None);
        assert!(result.changes.is_empty());
        assert_eq!(result.unverified_regions[0].reason, reason);
        assert_eq!(
            result.unverified_regions[0].base,
            Some(SourceLocator::ContentItem)
        );
        assert_eq!(
            result.unverified_regions[0].target,
            Some(SourceLocator::ContentItem)
        );
    }
}
