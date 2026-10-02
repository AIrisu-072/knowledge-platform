use std::io::Cursor;

use document_diff_core::{
    DisplayFragment, FormatId, MAX_DISPLAY_FRAGMENT_BYTES_V0, SourceLocator, WorkerDisplayRequest,
    WorkerDisplayResponse, WorkerProtocolVersion, serialized_fragment_bytes,
};
use document_diff_worker::{WorkerError, run_display_worker_shell};
use sha2::{Digest, Sha256};

fn request(
    format: FormatId,
    source: &[u8],
    locator: SourceLocator,
    limit: usize,
) -> WorkerDisplayRequest {
    WorkerDisplayRequest {
        protocol_version: WorkerProtocolVersion::V0,
        format,
        raw_sha256: Sha256::digest(source).into(),
        size_bytes: source.len() as u64,
        locator,
        max_fragment_bytes: limit as u32,
    }
}

fn extract(source: &[u8], request: WorkerDisplayRequest) -> Result<DisplayFragment, WorkerError> {
    let encoded = serde_json::to_vec(&request).unwrap();
    let response = run_display_worker_shell(&encoded, Cursor::new(source))?;
    Ok(response.fragment)
}

#[test]
fn text_span_reads_only_the_authoritative_line_and_preserves_its_locator() {
    let source = b"before\r\nchanged line\r\nafter\r\n";
    let fragment = extract(
        source,
        request(
            FormatId::Txt,
            source,
            SourceLocator::TextSpan {
                line: 2,
                byte_start: 8,
                byte_end: 22,
            },
            16 * 1024,
        ),
    )
    .unwrap();
    assert_eq!(
        fragment,
        DisplayFragment::Text {
            text: "changed line".into(),
            truncated: false,
            locator: SourceLocator::TextSpan {
                line: 2,
                byte_start: 8,
                byte_end: 22,
            },
        }
    );
}

#[test]
fn csv_cell_and_html_node_return_format_specific_structured_fragments() {
    let csv = b"name,value\nitem,changed\n";
    let cell = extract(
        csv,
        request(
            FormatId::Csv,
            csv,
            SourceLocator::CsvCell { row: 2, column: 2 },
            16 * 1024,
        ),
    )
    .unwrap();
    assert!(matches!(cell, DisplayFragment::Table { cells, .. } if cells[0].value == "changed"));

    let html = b"<p><a href=\"/new\">Manual</a></p>";
    let node = extract(
        html,
        request(
            FormatId::Html,
            html,
            SourceLocator::HtmlNode {
                path: "/html[1]/body[1]/p[1]/a[1]".into(),
            },
            16 * 1024,
        ),
    )
    .unwrap();
    assert!(
        matches!(node, DisplayFragment::Structural { summary, .. } if summary.contains("Manual") && summary.contains("/new"))
    );
}

#[test]
fn display_output_is_bounded_and_raw_binding_mismatch_fails_closed() {
    let source = format!("{}\n", "x".repeat(1024));
    let fragment = extract(
        source.as_bytes(),
        request(
            FormatId::Txt,
            source.as_bytes(),
            SourceLocator::TextSpan {
                line: 1,
                byte_start: 0,
                byte_end: source.len() as u32,
            },
            160,
        ),
    )
    .unwrap();
    assert!(matches!(
        fragment,
        DisplayFragment::Text {
            truncated: true,
            ..
        }
    ));
    assert!(serde_json::to_vec(&fragment).unwrap().len() <= 160);

    let mut invalid = request(FormatId::Txt, b"real", SourceLocator::ContentItem, 1000);
    invalid.raw_sha256 = [0; 32];
    assert_eq!(
        extract(b"real", invalid),
        Err(WorkerError::RawBindingMismatch)
    );
}

#[test]
fn one_side_fragment_accepts_exact_limit_and_rejects_one_byte_over() {
    let locator = SourceLocator::TextSpan {
        line: 1,
        byte_start: 0,
        byte_end: 1,
    };
    let request = request(
        FormatId::Txt,
        b"x",
        locator.clone(),
        MAX_DISPLAY_FRAGMENT_BYTES_V0,
    );
    let empty = DisplayFragment::Text {
        text: String::new(),
        truncated: false,
        locator: locator.clone(),
    };
    let empty_size = serialized_fragment_bytes(&empty);
    let exact_fragment = DisplayFragment::Text {
        text: "x".repeat(MAX_DISPLAY_FRAGMENT_BYTES_V0 - empty_size),
        truncated: false,
        locator: locator.clone(),
    };
    assert_eq!(
        serialized_fragment_bytes(&exact_fragment),
        MAX_DISPLAY_FRAGMENT_BYTES_V0
    );
    WorkerDisplayResponse {
        protocol_version: request.protocol_version,
        format: request.format,
        raw_sha256: request.raw_sha256,
        size_bytes: request.size_bytes,
        fragment: exact_fragment,
    }
    .validate_against(&request)
    .unwrap();

    let over_fragment = DisplayFragment::Text {
        text: "x".repeat(MAX_DISPLAY_FRAGMENT_BYTES_V0 - empty_size + 1),
        truncated: false,
        locator,
    };
    assert!(matches!(
        WorkerDisplayResponse {
            protocol_version: request.protocol_version,
            format: request.format,
            raw_sha256: request.raw_sha256,
            size_bytes: request.size_bytes,
            fragment: over_fragment,
        }
        .validate_against(&request),
        Err(document_diff_core::DiffCoreError::InvalidResponse(_))
    ));

    let mut invalid_request = request;
    invalid_request.max_fragment_bytes = (MAX_DISPLAY_FRAGMENT_BYTES_V0 + 1) as u32;
    assert!(matches!(
        invalid_request.validate(),
        Err(document_diff_core::DiffCoreError::ResourceLimit(_))
    ));
}

#[test]
fn native_pdf_display_reads_the_requested_authoritative_page_text() {
    for (source, expected) in [
        (
            include_bytes!("../../../apps/document-web/e2e-runtime/fixtures/pdf/base.pdf")
                .as_slice(),
            "Page A",
        ),
        (
            include_bytes!("../../../apps/document-web/e2e-runtime/fixtures/pdf/text-change.pdf")
                .as_slice(),
            "Page X",
        ),
    ] {
        let locator = SourceLocator::PdfPage {
            page: 1,
            region: None,
        };
        let fragment = extract(
            source,
            request(FormatId::Pdf, source, locator.clone(), 16 * 1024),
        )
        .unwrap();
        assert!(
            matches!(fragment, DisplayFragment::Text { ref text, truncated: false, locator: ref actual } if text.trim() == expected && actual == &locator)
        );
    }
    let source =
        include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf");
    let fragment = extract(
        source,
        request(
            FormatId::Pdf,
            source,
            SourceLocator::PdfPage {
                page: 2,
                region: None,
            },
            16 * 1024,
        ),
    )
    .unwrap();
    assert!(
        matches!(fragment, DisplayFragment::Text { ref text, .. } if text.contains("Page B") && !text.contains("Page A"))
    );
}

#[test]
fn pdf_display_preserves_raw_binding_limits_and_unavailable_boundaries() {
    let source = include_bytes!("../../../apps/document-web/e2e-runtime/fixtures/pdf/base.pdf");
    let locator = SourceLocator::PdfPage {
        page: 1,
        region: None,
    };
    let full = extract(
        source,
        request(FormatId::Pdf, source, locator.clone(), 16 * 1024),
    )
    .unwrap();
    let limit = serialized_fragment_bytes(&full) - 1;
    let bounded = extract(
        source,
        request(FormatId::Pdf, source, locator.clone(), limit),
    )
    .unwrap();
    assert!(matches!(
        bounded,
        DisplayFragment::Text {
            truncated: true,
            ..
        }
    ));
    assert!(serialized_fragment_bytes(&bounded) <= limit);
    let mut invalid = request(FormatId::Pdf, source, locator, 16 * 1024);
    invalid.raw_sha256 = [0; 32];
    assert_eq!(
        extract(source, invalid),
        Err(WorkerError::RawBindingMismatch)
    );
    for locator in [
        SourceLocator::PdfPage {
            page: 2,
            region: None,
        },
        SourceLocator::PdfPage {
            page: 1,
            region: Some([0, 0, 1, 1]),
        },
    ] {
        assert!(matches!(
            extract(source, request(FormatId::Pdf, source, locator, 16 * 1024)).unwrap(),
            DisplayFragment::Unavailable { .. }
        ));
    }
    let corrupt = b"%PDF-1.7\nnot a valid PDF";
    assert!(matches!(
        extract(
            corrupt,
            request(
                FormatId::Pdf,
                corrupt,
                SourceLocator::PdfPage {
                    page: 1,
                    region: None
                },
                16 * 1024
            )
        )
        .unwrap(),
        DisplayFragment::Unavailable { .. }
    ));
}

#[test]
fn image_only_pdf_display_remains_unavailable_without_ocr() {
    let source = include_bytes!(
        "../../../experiments/document-semantic-inspection/fixtures/pdf/scan-only.pdf"
    );
    let result = extract(
        source,
        request(
            FormatId::Pdf,
            source,
            SourceLocator::PdfPage {
                page: 1,
                region: None,
            },
            16 * 1024,
        ),
    )
    .unwrap();
    assert!(matches!(result, DisplayFragment::Unavailable { .. }));
    assert!(
        extract(
            source,
            request(
                FormatId::Pdf,
                source,
                SourceLocator::PdfPage {
                    page: 0,
                    region: None
                },
                16 * 1024
            )
        )
        .is_err()
    );
}
