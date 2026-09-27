use std::io::Cursor;

use document_semantic_inspection_core::{
    FingerprintAlgorithm, FormatId, InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest,
    WorkerResponse,
};
use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, SemanticAdapter, run_worker_shell,
};

const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/docx/base.docx");
const TRACKED_REPLACEMENT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/tracked-replacement.docx"
);
const COMMENT_UNRESOLVED: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/comment-unresolved.docx"
);
const METADATA_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/metadata-noise.docx"
);

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

fn worker_response(bytes: &[u8]) -> WorkerResponse {
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: DOCX_MEDIA_TYPE.to_owned(),
        expected_raw_content_hash: sha256(bytes),
        expected_size_bytes: bytes.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).unwrap();
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

    let response: WorkerResponse = serde_json::from_slice(&stdout).unwrap();
    response.validate().unwrap();
    assert_eq!(response.detected_format, FormatId::Docx);
    assert_eq!(response.observed_raw_content_hash, sha256(bytes));
    assert_eq!(response.observed_size_bytes, bytes.len() as u64);

    let expected = DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("qualified fixture should inspect")
        .semantic_fingerprint();
    assert_eq!(response.semantic_fingerprint, expected);
    assert_eq!(
        response.semantic_fingerprint.algorithm(),
        FingerprintAlgorithm::Sha256
    );

    response
}

#[test]
fn worker_shell_preserves_docx_editorial_provenance_and_semantic_fingerprint() {
    let base_fingerprint = worker_response(BASE).semantic_fingerprint;

    let comments = worker_response(COMMENT_UNRESOLVED);
    assert_eq!(comments.semantic_fingerprint, base_fingerprint);
    assert_eq!(comments.editorial_provenance.comments.len(), 1);
    assert_eq!(
        comments.editorial_provenance.comments[0].source_locator,
        "word/comments.xml#comment:0"
    );
    assert_eq!(
        comments.editorial_provenance.comments[0].resolved_state,
        "unresolved"
    );

    let tracked = worker_response(TRACKED_REPLACEMENT);
    assert_eq!(tracked.semantic_fingerprint, base_fingerprint);
    assert!(
        tracked
            .editorial_provenance
            .tracked_changes
            .iter()
            .any(|change| {
                change.kind == "deletion" && change.source_locator == "word/document.xml#deletion:1"
            })
    );
    assert!(
        tracked
            .editorial_provenance
            .tracked_changes
            .iter()
            .any(|change| {
                change.kind == "insertion"
                    && change.source_locator == "word/document.xml#insertion:2"
            })
    );

    let metadata = worker_response(METADATA_NOISE);
    assert_eq!(metadata.semantic_fingerprint, base_fingerprint);
    assert_eq!(
        metadata.editorial_provenance.last_modified_by.as_deref(),
        Some("Other")
    );
}
