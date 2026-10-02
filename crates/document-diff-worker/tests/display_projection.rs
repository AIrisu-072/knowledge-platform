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
