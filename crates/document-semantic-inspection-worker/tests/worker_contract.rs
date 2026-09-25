use std::{io::Cursor, os::fd::AsRawFd};

use document_semantic_inspection_core::{
    FormatId, InspectionProfileVersion, TraceContext, WorkerProtocolVersion, WorkerRequest,
};
use document_semantic_inspection_worker::{
    PreparedInput, WorkerFailureCode, decode_request_bounded, detect_format,
    guard_worker_execution, open_inherited_input, prepare_input_bounded,
};

fn request(media_type: &str, hash: [u8; 32], size: u64) -> WorkerRequest {
    WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: media_type.to_owned(),
        expected_raw_content_hash: hash,
        expected_size_bytes: size,
        trace_context: Some(TraceContext {
            traceparent: "00-00000000000000000000000000000001-0000000000000001-01".into(),
            tracestate: None,
        }),
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

fn zip_central_directory(names: &[&str]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for name in names {
        bytes.extend_from_slice(b"PK\x01\x02");
        bytes.extend_from_slice(&[0_u8; 24]);
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&[0_u8; 12]);
        bytes.extend_from_slice(name.as_bytes());
    }
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&[0_u8; 18]);
    bytes
}

#[test]
fn worker_recomputes_raw_hash_and_size_before_semantic_work() {
    let bytes = b"alpha\nbeta\n";
    let req = request("text/plain", sha256(bytes), bytes.len() as u64);
    let mut reader = Cursor::new(bytes.as_slice());

    let prepared = prepare_input_bounded(&req, &mut reader, 1024).unwrap();

    assert_eq!(prepared.observed_raw_content_hash(), &sha256(bytes));
    assert_eq!(prepared.observed_size_bytes(), bytes.len() as u64);
    assert_eq!(prepared.detected_format(), FormatId::Txt);
    assert_eq!(prepared.bytes(), bytes);
}

#[test]
fn raw_hash_or_size_mismatch_fails_closed() {
    let bytes = b"a,b\n1,2\n";

    let bad_hash = request("text/csv", [7; 32], bytes.len() as u64);
    let mut reader = Cursor::new(bytes.as_slice());
    let error = prepare_input_bounded(&bad_hash, &mut reader, 1024).unwrap_err();
    assert_eq!(error.code(), WorkerFailureCode::RawBindingMismatch);

    let bad_size = request("text/csv", sha256(bytes), 999);
    let mut reader = Cursor::new(bytes.as_slice());
    let error = prepare_input_bounded(&bad_size, &mut reader, 1024).unwrap_err();
    assert_eq!(error.code(), WorkerFailureCode::RawBindingMismatch);
}

#[test]
fn input_bound_is_enforced_before_format_specific_parsing() {
    let bytes = b"abcdef";
    let req = request("text/plain", sha256(bytes), bytes.len() as u64);
    let mut reader = Cursor::new(bytes.as_slice());

    let error = prepare_input_bounded(&req, &mut reader, 5).unwrap_err();
    assert_eq!(
        error.code(),
        WorkerFailureCode::InspectionResourceLimitExceeded
    );
}

#[test]
fn format_detection_uses_content_structure_not_filename() {
    assert_eq!(
        detect_format(b"%PDF-1.7\n", "application/pdf").unwrap(),
        FormatId::Pdf
    );

    let docx = zip_central_directory(&["[Content_Types].xml", "word/document.xml"]);
    assert_eq!(
        detect_format(
            &docx,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        )
        .unwrap(),
        FormatId::Docx
    );

    let xlsx = zip_central_directory(&["[Content_Types].xml", "xl/workbook.xml"]);
    assert_eq!(
        detect_format(
            &xlsx,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        )
        .unwrap(),
        FormatId::Xlsx
    );

    let xlsm = zip_central_directory(&[
        "[Content_Types].xml",
        "xl/workbook.xml",
        "xl/vbaProject.bin",
    ]);
    assert_eq!(
        detect_format(&xlsm, "application/vnd.ms-excel.sheet.macroEnabled.12").unwrap(),
        FormatId::Xlsm
    );

    let pptx = zip_central_directory(&["[Content_Types].xml", "ppt/presentation.xml"]);
    assert_eq!(
        detect_format(
            &pptx,
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        )
        .unwrap(),
        FormatId::Pptx
    );
}

#[test]
fn declared_and_detected_format_mismatch_fails_closed() {
    let error = detect_format(
        b"%PDF-1.7\n",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    )
    .unwrap_err();
    assert_eq!(error.code(), WorkerFailureCode::FormatMismatch);
}

#[test]
fn unknown_binary_and_scriptless_text_classification_are_controlled() {
    let unknown = detect_format(&[0, 1, 2, 3, 4, 5], "application/octet-stream").unwrap_err();
    assert_eq!(unknown.code(), WorkerFailureCode::UnsupportedDocumentFormat);

    assert_eq!(
        detect_format(b"<html><body>x</body></html>", "text/html").unwrap(),
        FormatId::Html
    );
    assert_eq!(
        detect_format(b"a,b\n1,2\n", "text/csv").unwrap(),
        FormatId::Csv
    );
    assert_eq!(
        detect_format(b"plain text\n", "text/plain").unwrap(),
        FormatId::Txt
    );
}

#[test]
fn malformed_or_oversized_request_is_a_controlled_failure() {
    let malformed = decode_request_bounded(b"{not-json", 1024).unwrap_err();
    assert_eq!(malformed.code(), WorkerFailureCode::MalformedRequest);

    let oversized =
        decode_request_bounded(br#"{"protocol_version":"dsi-worker-v0"}"#, 4).unwrap_err();
    assert_eq!(
        oversized.code(),
        WorkerFailureCode::InspectionResourceLimitExceeded
    );
}

#[test]
fn inherited_input_is_opened_by_fd_without_storage_path_in_protocol() {
    let file = tempfile::tempfile().unwrap();
    let inherited = open_inherited_input(file.as_raw_fd()).unwrap();
    drop(inherited);

    let req = request("text/plain", [0; 32], 0);
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("storage_key"));
    assert!(!json.contains("path"));
    assert!(!json.contains("database"));
    assert!(!json.contains("credential"));
}

#[test]
fn panic_guard_cannot_return_partial_success() {
    let outcome = guard_worker_execution(|| -> Result<PreparedInput, _> {
        panic!("synthetic parser panic");
    });

    let error = outcome.unwrap_err();
    assert_eq!(error.code(), WorkerFailureCode::WorkerPanicked);
}
