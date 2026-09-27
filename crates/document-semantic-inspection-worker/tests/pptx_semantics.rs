use std::io::Cursor;

use document_semantic_inspection_core::{
    FingerprintAlgorithm, FormatId, InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest,
    WorkerResponse,
};
use document_semantic_inspection_worker::{
    AdapterProfile, PptxAdapter, SemanticAdapter, SemanticAdapterOutput, WorkerFailure,
    WorkerFailureCode, run_worker_shell,
};
use sha2::{Digest, Sha256};

const PPTX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const TEXT_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/text-change.pptx"
);
const SLIDE_ADD: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/slide-add.pptx"
);
const SLIDE_REMOVE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/slide-remove.pptx"
);
const SLIDE_ORDER_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/slide-order-change.pptx"
);
const SHAPE_ASSOCIATION_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/shape-association-change.pptx"
);
const GROUP_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/group-change.pptx"
);
const TABLE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/table-change.pptx"
);
const CHART_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/chart-change.pptx"
);
const SMARTART_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/smartart-change.pptx"
);
const IMAGE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/image-change.pptx"
);
const HYPERLINK_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/hyperlink-change.pptx"
);
const SPEAKER_NOTE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/speaker-note-change.pptx"
);
const THEME_ONLY: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/theme-only.pptx"
);
const FONT_ONLY: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/font-only.pptx"
);
const BACKGROUND_ONLY: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/background-only.pptx"
);
const ID_ORDER_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/id-order-noise.pptx"
);
const COMMENT_ONLY: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/comment-only.pptx"
);
const UNKNOWN_SEMANTIC_PART: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/unknown-semantic-part.pptx"
);

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn inspect(bytes: &[u8]) -> SemanticAdapterOutput {
    PptxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("qualified PPTX fixture should inspect")
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
    let failure = PptxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect_err("PPTX fixture should fail closed");
    assert_eq!(failure.code(), expected, "{failure}");
}

fn worker_response(bytes: &[u8]) -> WorkerResponse {
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: PPTX_MEDIA_TYPE.to_owned(),
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
    assert_eq!(response.detected_format, FormatId::Pptx);
    assert_eq!(response.observed_raw_content_hash, sha256(bytes));
    assert_eq!(response.observed_size_bytes, bytes.len() as u64);
    assert_eq!(
        response.semantic_fingerprint,
        inspect(bytes).semantic_fingerprint()
    );
    assert_eq!(
        response.semantic_fingerprint.algorithm(),
        FingerprintAlgorithm::Sha256
    );

    response
}

#[test]
fn pptx_slides_text_shape_association_and_grouping_are_version_significant() {
    for fixture in [
        TEXT_CHANGE,
        SLIDE_ADD,
        SLIDE_REMOVE,
        SLIDE_ORDER_CHANGE,
        SHAPE_ASSOCIATION_CHANGE,
        GROUP_CHANGE,
    ] {
        assert_different_from_base(fixture);
    }
}

#[test]
fn pptx_tables_charts_smartart_images_hyperlinks_and_speaker_notes_are_version_significant() {
    for fixture in [
        TABLE_CHANGE,
        CHART_CHANGE,
        SMARTART_CHANGE,
        IMAGE_CHANGE,
        HYPERLINK_CHANGE,
        SPEAKER_NOTE_CHANGE,
    ] {
        assert_different_from_base(fixture);
    }
}

#[test]
fn pptx_theme_font_background_internal_ids_and_package_order_are_noise() {
    for fixture in [THEME_ONLY, FONT_ONLY, BACKGROUND_ONLY, ID_ORDER_NOISE] {
        assert_same_as_base(fixture);
    }
}

#[test]
fn pptx_comments_preserve_identity_and_remain_editorial_evidence() {
    assert_same_as_base(COMMENT_ONLY);

    let inspection = inspect(COMMENT_ONLY);
    let comments = &inspection.editorial_provenance().comments;
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].content, "Review note");
}

#[test]
fn unknown_semantic_pptx_content_fails_closed() {
    assert_failure(
        UNKNOWN_SEMANTIC_PART,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn pptx_adapter_reports_the_frozen_format() {
    assert_eq!(PptxAdapter.format(), FormatId::Pptx);
}

#[test]
fn worker_shell_binds_raw_pptx_bytes_and_returns_a_valid_semantic_response() {
    let response = worker_response(BASE);
    assert_eq!(response.observed_raw_content_hash, sha256(BASE));
    assert_eq!(response.observed_size_bytes, BASE.len() as u64);
}

#[test]
fn worker_shell_rejects_a_mismatched_pptx_raw_hash_without_partial_success() {
    let mut wrong_hash = sha256(BASE);
    wrong_hash[0] ^= 0xff;
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: PPTX_MEDIA_TYPE.to_owned(),
        expected_raw_content_hash: wrong_hash,
        expected_size_bytes: BASE.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).expect("valid worker request");
    let mut input = Cursor::new(BASE);
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
    assert_ne!(exit, 0);
    assert!(
        stdout.is_empty(),
        "raw-binding failure must not emit success"
    );

    let failure: WorkerFailure = serde_json::from_slice(&stderr).expect("structured failure");
    assert_eq!(failure.code(), WorkerFailureCode::RawBindingMismatch);
}
