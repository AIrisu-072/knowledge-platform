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
const ANNOTATION: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/annotation-change.pdf"
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
    let visual = compare(BASE, IMAGE);
    assert_eq!(visual.coverage, DiffCoverage::Partial);
    assert!(visual.unverified_regions.iter().any(|region| {
        matches!(
            region.base,
            Some(SourceLocator::PdfPage {
                page: 1,
                region: None
            })
        ) && region.reason == UnverifiedReason::UnsupportedSemanticConstruct
    }));
}

#[test]
fn producer_and_object_id_noise_does_not_change_content() {
    let result = compare(BASE, NOISE);
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert!(result.changes.is_empty());
    assert!(result.unverified_regions.is_empty());
}

#[test]
fn annotation_edits_are_ancillary_without_false_content_change() {
    let result = compare(BASE, ANNOTATION);
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert!(result.changes.is_empty());
    assert!(!result.ancillary_changes.is_empty());
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

#[test]
fn overlapping_text_and_image_paint_order_is_a_visual_change() {
    let text_then_image = paint_order_pdf(true);
    let image_then_text = paint_order_pdf(false);
    let result = compare(&text_then_image, &image_then_text);
    assert_eq!(result.coverage, DiffCoverage::Partial);
    assert!(result.unverified_regions.iter().any(|region| {
        matches!(
            region.base,
            Some(SourceLocator::PdfPage {
                page: 1,
                region: None
            })
        )
    }));
    assert!(result.changes.iter().any(|change| {
        change.facet == "pdf_visual"
            && change.reason_code == "paint_order_changed"
            && matches!(change.base, Some(SourceLocator::PdfPage { page: 1, .. }))
    }));
}

fn paint_order_pdf(text_before_image: bool) -> Vec<u8> {
    let text = "BT /F1 12 Tf 12 180 Td (SAME NATIVE TEXT) Tj ET";
    let image = "q 200 0 0 200 0 0 cm /Im0 Do Q";
    let content = if text_before_image {
        format!("{text}\n{image}")
    } else {
        format!("{image}\n{text}")
    };
    serialize_pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> >> /Contents 4 0 R >>".to_vec()),
        (4, pdf_stream(b"", content.as_bytes())),
        (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
        (6, pdf_stream(b"/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8", &[255, 0, 0])),
    ])
}

#[test]
fn bounded_vector_difference_is_partial_visual_at_the_affected_page() {
    let base = bounded_vector_pdf(1, "Stable final page");
    let target = bounded_vector_pdf(2, "Stable final page");
    assert_supported_vector_fixture(&base);
    assert_supported_vector_fixture(&target);
    let result = compare(&base, &target);

    assert_eq!(result.coverage, DiffCoverage::Partial);
    assert!(result.changes.iter().any(|change| {
        change.facet == "pdf_visual"
            && change.operation == Some(document_diff_core::ChangeOperation::Modified)
            && change.base
                == Some(SourceLocator::PdfPage {
                    page: 1,
                    region: None,
                })
            && change.target
                == Some(SourceLocator::PdfPage {
                    page: 1,
                    region: None,
                })
    }));
    assert!(result.unverified_regions.iter().any(|region| {
        region.base
            == Some(SourceLocator::PdfPage {
                page: 1,
                region: None,
            })
            && region.target
                == Some(SourceLocator::PdfPage {
                    page: 1,
                    region: None,
                })
            && region.reason == UnverifiedReason::UnsupportedSemanticConstruct
    }));
    assert!(
        !result
            .changes
            .iter()
            .any(|change| change.facet == "pdf_text")
    );
}

#[test]
fn bounded_vector_difference_with_another_pages_text_change_never_becomes_full() {
    let base = bounded_vector_pdf(1, "Old final page");
    let target = bounded_vector_pdf(2, "New final page");
    assert_supported_vector_fixture(&base);
    assert_supported_vector_fixture(&target);
    let result = compare(&base, &target);

    assert_eq!(result.coverage, DiffCoverage::Partial);
    for (facet, page) in [("pdf_visual", 1), ("pdf_text", 3)] {
        assert!(
            result.changes.iter().any(|change| {
                change.facet == facet
                    && change.operation == Some(document_diff_core::ChangeOperation::Modified)
                    && change.base == Some(SourceLocator::PdfPage { page, region: None })
                    && change.target == Some(SourceLocator::PdfPage { page, region: None })
            }),
            "missing {facet} change on page {page}: {:?}",
            result.changes
        );
    }
    assert!(result.unverified_regions.iter().any(|region| {
        region.base
            == Some(SourceLocator::PdfPage {
                page: 1,
                region: None,
            })
            && region.target
                == Some(SourceLocator::PdfPage {
                    page: 1,
                    region: None,
                })
            && region.reason == UnverifiedReason::UnsupportedSemanticConstruct
    }));
}

fn bounded_vector_pdf(line_width: u8, final_text: &str) -> Vec<u8> {
    bounded_vector_pdf_with_start(line_width, final_text, 40)
}

fn assert_supported_vector_fixture(bytes: &[u8]) {
    document_semantic_inspection_worker::PdfAdapter.inspect_with_projection(
        bytes, &document_semantic_inspection_worker::AdapterProfile::default(),
    ).expect("vector fixture must pass real PDF inspection before testing Diff");
}

#[test]
fn vector_outside_conservative_page_proof_remains_unverified() {
    let base = bounded_vector_pdf_with_start(1, "Stable final page", 12);
    let target = bounded_vector_pdf_with_start(2, "Stable final page", 12);
    assert_supported_vector_fixture(&base);
    let error = document_semantic_inspection_worker::PdfAdapter.inspect_with_projection(
        &target, &document_semantic_inspection_worker::AdapterProfile::default(),
    ).expect_err("original edge fixture is outside the bounded crop proof");
    assert_eq!(error.message(), "pdf_clip_does_not_enclose_paint");
    let result = compare(&base, &target);
    assert_eq!(result.coverage, DiffCoverage::None);
    assert!(result.changes.is_empty());
    assert!(result.unverified_regions.iter().any(|region| {
        region.reason == UnverifiedReason::UnsupportedSemanticConstruct
    }));
}

fn bounded_vector_pdf_with_start(line_width: u8, final_text: &str, start_x: u8) -> Vec<u8> {
    let first = format!(
        "q {line_width} w {start_x} 80 m 100 80 l S Q \
         BT /F1 12 Tf 12 180 Td (Stable vector page) Tj ET"
    );
    let anchor = "BT /F1 12 Tf 12 180 Td (Unique unchanged anchor) Tj ET";
    let last = format!("BT /F1 12 Tf 12 180 Td ({final_text}) Tj ET");
    serialize_pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [4 0 R 6 0 R 8 0 R] /Count 3 >>".to_vec()),
        (3, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
        (4, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_vec()),
        (5, pdf_stream(b"", first.as_bytes())),
        (6, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 3 0 R >> >> /Contents 7 0 R >>".to_vec()),
        (7, pdf_stream(b"", anchor.as_bytes())),
        (8, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 3 0 R >> >> /Contents 9 0 R >>".to_vec()),
        (9, pdf_stream(b"", last.as_bytes())),
    ])
}

fn pdf_stream(attributes: &[u8], data: &[u8]) -> Vec<u8> {
    let mut body = format!(
        "<< {} /Length {} >>\nstream\n",
        String::from_utf8_lossy(attributes),
        data.len()
    )
    .into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

fn serialize_pdf(objects: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0usize];
    for (id, body) in objects {
        assert_eq!(id as usize, offsets.len());
        offsets.push(bytes.len());
        writeln!(bytes, "{id} 0 obj").unwrap();
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = bytes.len();
    write!(bytes, "xref\n0 {}\n0000000000 65535 f \n", offsets.len()).unwrap();
    for offset in offsets.iter().skip(1) {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
        offsets.len()
    )
    .unwrap();
    bytes
}
use std::io::Write as _;

#[test]
fn runtime_publication_fixtures_produce_full_native_text_diff() {
    let result = compare(
        include_bytes!("../../../apps/document-web/e2e-runtime/fixtures/pdf/base.pdf"),
        include_bytes!("../../../apps/document-web/e2e-runtime/fixtures/pdf/text-change.pdf"),
    );
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert!(
        result
            .changes
            .iter()
            .any(|change| change.facet == "pdf_text")
    );
}
