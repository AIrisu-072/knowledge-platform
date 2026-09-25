use std::io::Cursor;

use document_semantic_inspection_core::{
    FormatId, InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
};
use document_semantic_inspection_worker::run_worker_shell;
use sha2::{Digest, Sha256};

const XLSX_MEDIA_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/xlsx/base.xlsx");
const EXTERNAL_WORKBOOK: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/external-reference-add.xlsx"
);
const ODBC_CONNECTION: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/odbc-connection-add.xlsx"
);

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn worker_response(bytes: &[u8]) -> WorkerResponse {
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: XLSX_MEDIA_TYPE.to_owned(),
        expected_raw_content_hash: sha256(bytes),
        expected_size_bytes: bytes.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).expect("valid request");
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
        exit, 0,
        "qualified XLSX fixture failed with exit code {exit}"
    );
    assert!(stderr.is_empty());

    let response: WorkerResponse = serde_json::from_slice(&stdout).expect("structured response");
    response.validate().expect("valid response");
    assert_eq!(response.detected_format, FormatId::Xlsx);
    assert_eq!(response.observed_raw_content_hash, sha256(bytes));
    assert_eq!(response.observed_size_bytes, bytes.len() as u64);
    response
}

#[test]
fn external_workbook_and_odbc_definitions_reach_the_worker_response() {
    let baseline = worker_response(BASE);
    assert_eq!(baseline.external_dependencies.len(), 1);
    assert_eq!(
        baseline.external_dependencies[0].dependency_kind,
        "hyperlink"
    );
    assert!(
        baseline.external_dependencies[0]
            .normalized_reference
            .contains("https://example.com/a")
    );

    let external = worker_response(EXTERNAL_WORKBOOK);
    assert_eq!(external.external_dependencies.len(), 2);
    assert!(external.external_dependencies.iter().any(|dependency| {
        dependency.dependency_kind == "external_workbook"
            && dependency
                .normalized_reference
                .contains("external-book.xlsx")
            && !dependency.source_locator.is_empty()
            && dependency.version_significant
    }));

    let odbc = worker_response(ODBC_CONNECTION);
    assert_eq!(odbc.external_dependencies.len(), 2);
    assert!(odbc.external_dependencies.iter().any(|dependency| {
        dependency.dependency_kind == "odbc"
            && dependency
                .normalized_reference
                .contains("SERVER=db.internal")
            && dependency
                .normalized_reference
                .contains("SELECT account_id,balance FROM ledger")
            && !dependency.source_locator.is_empty()
            && dependency.version_significant
    }));
}
