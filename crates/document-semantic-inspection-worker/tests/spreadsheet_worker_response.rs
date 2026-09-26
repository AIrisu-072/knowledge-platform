use std::io::{Cursor, Read, Write};

use document_semantic_inspection_core::{
    FormatId, InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
};
use document_semantic_inspection_worker::{WorkerFailure, WorkerFailureCode, run_worker_shell};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const XLSX_MEDIA_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/xlsx/base.xlsx");
const EXTERNAL_WORKBOOK: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/external-reference-add.xlsx"
);
const ODBC_CONNECTION: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/odbc-connection-add.xlsx"
);
const CONNECTIONS_PATH: &str = "xl/connections.xml";
const MAX_STRUCTURED_RESULT_BYTES: usize = 16 * 1024 * 1024;

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

#[test]
fn oversized_connection_evidence_fails_before_any_stdout_is_written() {
    let columns = std::iter::repeat_n("x".repeat(128), 64)
        .collect::<Vec<_>>()
        .join(",");
    let mut definitions = String::new();
    for id in 1..=2_200 {
        definitions.push_str(&format!(
            "<connection id=\"{id}\" name=\"Conn{id}\" type=\"1\" refreshedVersion=\"0\" background=\"0\"><dbPr connection=\"DRIVER={{PostgreSQL Unicode}};SERVER=db.internal;PORT=5432;DATABASE=finance;UID=readonly\" command=\"SELECT {columns} FROM ledger\" commandType=\"2\"/></connection>"
        ));
    }
    let replacement = format!(
        "<connections xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" count=\"2200\">{definitions}</connections>"
    );
    let mut source = ZipArchive::new(Cursor::new(ODBC_CONNECTION)).expect("qualified ZIP");
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut output);
        for index in 0..source.len() {
            let mut entry = source.by_index(index).expect("qualified entry");
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read qualified entry");
            writer
                .start_file(
                    &name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .expect("write entry");
            if name == CONNECTIONS_PATH {
                writer
                    .write_all(replacement.as_bytes())
                    .expect("write connections");
            } else {
                writer.write_all(&bytes).expect("write original part");
            }
        }
        writer.finish().expect("finish ZIP");
    }
    let bytes = output.into_inner();
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: XLSX_MEDIA_TYPE.to_owned(),
        expected_raw_content_hash: sha256(&bytes),
        expected_size_bytes: bytes.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).expect("valid request");
    let mut input = Cursor::new(&bytes);
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
        65,
        "oversized structured result must fail closed (stdout_bytes={})",
        stdout.len()
    );
    assert!(stdout.is_empty(), "failure must not write partial success");
    assert!(stderr.len() < 1_024);
    let failure: WorkerFailure = serde_json::from_slice(&stderr).expect("bounded failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::InspectionResourceLimitExceeded
    );
    assert!(
        replacement.len() > MAX_STRUCTURED_RESULT_BYTES,
        "test input must force an oversized response"
    );
}
