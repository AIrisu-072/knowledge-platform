use std::io::Cursor;

use document_semantic_inspection_core::{
    FormatId, InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
};
use document_semantic_inspection_worker::{
    AdapterProfile, PdfAdapter, SemanticAdapter, SemanticAdapterOutput, WorkerFailureCode,
    run_worker_shell,
};
use sha2::{Digest, Sha256};

const PDF_MEDIA_TYPE: &str = "application/pdf";

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf");
const TEXT_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/text-change.pdf"
);
const PAGE_ORDER_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/page-order-change.pdf"
);
const LINK_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/link-change.pdf"
);
const FORM_VALUE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/form-value-change.pdf"
);
const IMAGE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/image-change.pdf"
);
const ANNOTATION_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/annotation-change.pdf"
);
const PRODUCER_AND_OBJECT_ID_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/object-id-producer-noise.pdf"
);
const SCAN_ONLY: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/scan-only.pdf");
const ENCRYPTED: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/encrypted.pdf");
const BROKEN_XREF: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/broken-xref.pdf"
);
const PARSER_DISAGREEMENT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/parser-disagreement.pdf"
);
const AMBIGUOUS_READ_ORDER: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/ambiguous-read-order.pdf"
);

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn inspect(bytes: &[u8]) -> SemanticAdapterOutput {
    PdfAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("qualified PDF fixture should inspect")
}

fn fingerprint(bytes: &[u8]) -> document_semantic_inspection_core::SemanticFingerprint {
    inspect(bytes).semantic_fingerprint()
}

fn assert_same_as_base(bytes: &[u8]) {
    assert_eq!(fingerprint(BASE), fingerprint(bytes));
}

fn assert_different_from_base(bytes: &[u8]) {
    assert_ne!(fingerprint(BASE), fingerprint(bytes));
}

fn assert_failure(bytes: &[u8], expected: WorkerFailureCode) {
    let failure = PdfAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect_err("PDF fixture should fail closed");
    assert_eq!(failure.code(), expected, "{failure}");
}

fn worker_response(bytes: &[u8]) -> WorkerResponse {
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: PDF_MEDIA_TYPE.to_owned(),
        expected_raw_content_hash: sha256(bytes),
        expected_size_bytes: bytes.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).expect("valid worker request");
    let mut input = Cursor::new(bytes);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit = run_worker_shell(
        &request_bytes,
        &mut input,
        &mut stdout,
        &mut stderr,
        64 * 1024,
        8 * 1024 * 1024,
    );
    assert_eq!(
        exit,
        0,
        "worker shell failed: {}",
        String::from_utf8_lossy(&stderr)
    );
    assert!(stderr.is_empty());

    let response: WorkerResponse = serde_json::from_slice(&stdout).expect("worker response JSON");
    response.validate().expect("valid worker response");
    assert_eq!(response.detected_format, FormatId::Pdf);
    assert_eq!(response.observed_raw_content_hash, sha256(bytes));
    assert_eq!(response.observed_size_bytes, bytes.len() as u64);
    assert_eq!(
        response.semantic_fingerprint,
        inspect(bytes).semantic_fingerprint()
    );
    response
}

#[test]
fn pdf_reader_visible_semantics_are_version_significant() {
    for fixture in [
        TEXT_CHANGE,
        PAGE_ORDER_CHANGE,
        LINK_CHANGE,
        FORM_VALUE_CHANGE,
        IMAGE_CHANGE,
    ] {
        assert_different_from_base(fixture);
    }
}

#[test]
fn pdf_producer_and_object_id_noise_is_invariant() {
    assert_same_as_base(PRODUCER_AND_OBJECT_ID_NOISE);
}

#[test]
fn pdf_annotations_are_editorial_not_version_identity() {
    let inspection = inspect(ANNOTATION_CHANGE);
    assert_eq!(fingerprint(BASE), inspection.semantic_fingerprint());

    let comments = &inspection.editorial_provenance().comments;
    assert!(
        comments
            .iter()
            .any(|comment| comment.content.contains("Note B"))
    );
}

#[test]
fn pdf_scan_only_encrypted_and_broken_xref_inputs_fail_with_specific_codes() {
    assert_failure(SCAN_ONLY, WorkerFailureCode::RequiresOcr);
    assert_failure(ENCRYPTED, WorkerFailureCode::EncryptedContentUnsupported);
    assert_failure(BROKEN_XREF, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn pdfium_and_lopdf_semantic_disagreement_fails_closed() {
    assert_failure(PARSER_DISAGREEMENT, WorkerFailureCode::ParserDisagreement);
}

#[test]
fn ambiguous_pdf_read_order_fails_closed() {
    assert_failure(
        AMBIGUOUS_READ_ORDER,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn pdf_adapter_reports_the_frozen_format() {
    assert_eq!(PdfAdapter.format(), FormatId::Pdf);
}

#[test]
fn pdf_worker_response_records_pdfium_build_identity_and_binary_hash() {
    let response = worker_response(BASE);
    let pdfium = response
        .extractor_provenance
        .native_dependency_identity
        .iter()
        .find(|dependency| dependency.name == "pdfium")
        .expect("PDFium native dependency identity");

    assert_eq!(pdfium.version.as_deref(), Some("151.0.7881.0"));
    let binary_hash = pdfium
        .sha256
        .as_ref()
        .expect("PDFium binary hash must be recorded");
    let observed_hash = binary_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let expected_hash = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "4eaad6c3e8d786cf6f66a45d7d014edf5c65f372f98c3070e66595ebb50e43d9"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7"
    } else {
        panic!("no qualified PDFium binary for this test platform")
    };
    assert_eq!(observed_hash, expected_hash);
}
