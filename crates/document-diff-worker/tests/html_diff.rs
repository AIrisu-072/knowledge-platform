use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest,
    WorkerProtocolVersion,
};
use document_diff_worker::HtmlComparator;
use sha2::{Digest, Sha256};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/html/base.html");
const NOISE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/html/noise.html");
const TEXT_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/html/text-change.html"
);
const LINK_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/html/link-change.html"
);
const IMAGE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/html/image-change.html"
);
const JS_ONLY: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/html/js-only.html");

fn request(base: &[u8], target: &[u8]) -> WorkerDiffRequest {
    WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Html,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    }
}

fn compare(base: &[u8], target: &[u8]) -> document_diff_core::WorkerDiffResponse {
    let request = request(base, target);
    let mut budget = ComparisonBudget::new(100_000, 100_000);
    let result = HtmlComparator::compare(&request, base, target, &mut budget).unwrap();
    result.validate_against(&request).unwrap();
    result
}

#[test]
fn whitespace_and_decorative_noise_are_equal() {
    let result = compare(BASE, NOISE);
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert!(result.changes.is_empty());
    assert!(result.unverified_regions.is_empty());
}

#[test]
fn visible_text_link_and_image_changes_have_original_dom_locations() {
    for (target, facet) in [
        (TEXT_CHANGE, "html_text"),
        (LINK_CHANGE, "html_link"),
        (IMAGE_CHANGE, "html_image"),
    ] {
        let result = compare(BASE, target);
        assert_eq!(result.coverage, DiffCoverage::Full);
        assert!(result.changes.iter().any(|change| {
            change.operation == Some(ChangeOperation::Modified)
                && change.facet == facet
                && matches!(change.base, Some(SourceLocator::HtmlNode { .. }))
                && matches!(change.target, Some(SourceLocator::HtmlNode { .. }))
        }));
    }
}

#[test]
fn heading_and_table_structure_changes_are_visible() {
    let heading = compare(b"<h1>Policy</h1>", b"<h2>Policy</h2>");
    assert_eq!(heading.coverage, DiffCoverage::Full);
    assert!(!heading.changes.is_empty());

    let table = compare(
        b"<table><tr><td>A</td></tr></table>",
        b"<table><tr><td>A</td><td>B</td></tr></table>",
    );
    assert_eq!(table.coverage, DiffCoverage::Full);
    assert!(
        table
            .changes
            .iter()
            .any(|change| change.facet == "html_structure")
    );
}

#[test]
fn scripts_are_not_executed_and_script_only_semantics_are_unverified() {
    let base = b"<p>Visible</p><script>throw new Error('must not run')</script>";
    let target = b"<p>Visible</p><script>throw new Error('changed')</script>";
    let harmless = compare(base, target);
    assert_eq!(harmless.coverage, DiffCoverage::Full);
    assert!(harmless.changes.is_empty());

    let script_required = compare(BASE, JS_ONLY);
    assert_eq!(script_required.coverage, DiffCoverage::None);
    assert_eq!(script_required.unverified_regions.len(), 1);
    assert_eq!(
        script_required.unverified_regions[0].reason,
        UnverifiedReason::UnsupportedSemanticConstruct
    );
}
