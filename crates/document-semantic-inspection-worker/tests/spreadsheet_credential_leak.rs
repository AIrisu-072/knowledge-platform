use std::io::{Cursor, Read, Write};

use document_semantic_inspection_core::{
    InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
};
use document_semantic_inspection_worker::{WorkerFailure, WorkerFailureCode, run_worker_shell};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const ODBC_CONNECTION: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/odbc-connection-add.xlsx"
);
const XLSX_BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/xlsx/base.xlsx");
const XLSX_EXTERNAL_REFERENCE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/external-reference-add.xlsx"
);
const XLSX_MEDIA_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const CONNECTIONS_PATH: &str = "xl/connections.xml";
const WORKSHEET_RELS_PATH: &str = "xl/worksheets/_rels/sheet1.xml.rels";
const EXTERNAL_LINK_RELS_PATH: &str = "xl/externalLinks/_rels/externalLink1.xml.rels";
const PASSWORD_SENTINEL: &str = "DSI_ODBC_PWD_LEAK_SENTINEL";
const URI_USERINFO_SENTINEL: &str = "DSI_URI_USERINFO_LEAK_SENTINEL";
const EXTERNAL_WORKBOOK_USERINFO_SENTINEL: &str = "DSI_EXTERNAL_WORKBOOK_USERINFO_LEAK_SENTINEL";
const REVIEWED_BYPASS_SENTINEL: &str = "DSI_REVIEWED_CREDENTIAL_BYPASS_SENTINEL";

#[test]
fn embedded_odbc_password_fails_closed_without_serializing_credential_bytes() {
    let (baseline_exit, baseline_stdout, baseline_stderr) = run_worker(ODBC_CONNECTION);
    assert_eq!(
        baseline_exit, 0,
        "qualified noncredential ODBC fixture remains supported"
    );
    assert!(baseline_stderr.is_empty());
    let baseline: WorkerResponse =
        serde_json::from_slice(&baseline_stdout).expect("qualified worker response");
    assert!(baseline.external_dependencies.iter().any(|dependency| {
        dependency.dependency_kind == "odbc"
            && dependency
                .normalized_reference
                .contains("SERVER=db.internal")
            && dependency
                .normalized_reference
                .contains("SELECT account_id,balance FROM ledger")
    }));

    let credential_bearing = xlsx_with_odbc_password(ODBC_CONNECTION);
    let (exit, stdout, stderr) = run_worker(&credential_bearing);
    let stdout_contains_password = String::from_utf8_lossy(&stdout).contains(PASSWORD_SENTINEL);
    let stderr_contains_password = String::from_utf8_lossy(&stderr).contains(PASSWORD_SENTINEL);
    assert_ne!(
        exit, 0,
        "credential-bearing ODBC definition was accepted (stdout_contains_password={stdout_contains_password}, stderr_contains_password={stderr_contains_password})"
    );

    let stdout_text = String::from_utf8_lossy(&stdout);
    let stderr_text = String::from_utf8_lossy(&stderr);
    assert!(!stdout_text.contains(PASSWORD_SENTINEL));
    assert!(!stderr_text.contains(PASSWORD_SENTINEL));
    assert!(
        stdout.is_empty(),
        "failed inspection must not emit a result"
    );

    let failure: WorkerFailure =
        serde_json::from_slice(&stderr).expect("stderr contains one structured failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
    assert!(!failure.message().contains(PASSWORD_SENTINEL));
    let serialized_failure = serde_json::to_vec(&failure).expect("failure serializes");
    assert!(!String::from_utf8_lossy(&serialized_failure).contains(PASSWORD_SENTINEL));
}

#[test]
fn unknown_odbc_credential_parameter_fails_closed_without_serializing_value() {
    let xml = read_connections_xml(ODBC_CONNECTION);
    let changed = xml.replace(
        "UID=readonly",
        &format!("UID=readonly;CREDENTIAL={PASSWORD_SENTINEL}"),
    );
    assert_ne!(
        changed, xml,
        "qualified fixture must have an ODBC connection"
    );
    let xlsx = xlsx_with_connections_xml(ODBC_CONNECTION, &changed);
    let (exit, stdout, stderr) = run_worker(&xlsx);
    let stdout_contains_value = String::from_utf8_lossy(&stdout).contains(PASSWORD_SENTINEL);

    assert_ne!(
        exit, 0,
        "unknown ODBC credential parameter was accepted (stdout_contains_value={stdout_contains_value})"
    );
    assert!(
        stdout.is_empty(),
        "failed inspection must not emit a result"
    );
    let failure: WorkerFailure =
        serde_json::from_slice(&stderr).expect("stderr contains one structured failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
    assert!(!String::from_utf8_lossy(&stderr).contains(PASSWORD_SENTINEL));
}

#[test]
fn web_query_connection_is_not_silently_accepted_without_semantic_projection() {
    let connections_xml = r#"<connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1"><connection id="1" name="WebQuery" type="5" refreshedVersion="0" background="0"><webPr url="https://example.invalid/finance.csv"/></connection></connections>"#;
    let xlsx = xlsx_with_connections_xml(ODBC_CONNECTION, connections_xml);
    let (exit, stdout, stderr) = run_worker(&xlsx);

    assert_ne!(
        exit, 0,
        "a webPr connection definition outside the supported database subset must fail closed"
    );
    assert!(
        stdout.is_empty(),
        "failed inspection must not emit a result"
    );
    let failure: WorkerFailure =
        serde_json::from_slice(&stderr).expect("stderr contains one structured failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

#[test]
fn connection_refresh_on_load_attribute_fails_closed() {
    let xml = read_connections_xml(ODBC_CONNECTION);
    let changed = xml.replace("background=\"0\">", "background=\"0\" refreshOnLoad=\"1\">");
    assert_ne!(changed, xml, "fixture must have a connection element");
    assert_worker_fails_closed(&xlsx_with_connections_xml(ODBC_CONNECTION, &changed));
}

#[test]
fn foreign_namespace_dbpr_fails_closed() {
    let connections_xml = r#"<connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1"><connection id="1" name="ForeignDatabase" type="1" refreshedVersion="0" background="0"><ext:dbPr xmlns:ext="urn:untrusted" connection="ODBC;SERVER=db.internal"/></connection></connections>"#;
    let xlsx = xlsx_with_connections_xml(ODBC_CONNECTION, connections_xml);
    assert_worker_fails_closed(&xlsx);
}

#[test]
fn duplicate_dbpr_definitions_fail_closed() {
    let connections_xml = r#"<connections xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1"><connection id="1" name="DuplicateDatabase" type="1" refreshedVersion="0" background="0"><dbPr connection="ODBC;SERVER=first.internal"/><dbPr connection="ODBC;SERVER=second.internal"/></connection></connections>"#;
    let xlsx = xlsx_with_connections_xml(ODBC_CONNECTION, connections_xml);
    assert_worker_fails_closed(&xlsx);
}

#[test]
fn external_hyperlink_userinfo_fails_closed_without_serializing_credential_bytes() {
    let relationship = String::from_utf8(read_package_part(XLSX_BASE, WORKSHEET_RELS_PATH))
        .expect("qualified relationships XML is UTF-8");
    let changed = relationship.replace(
        "https://example.com/a",
        &format!("https://service:{URI_USERINFO_SENTINEL}@example.com/a"),
    );
    assert_ne!(
        changed, relationship,
        "qualified fixture must contain the target URL"
    );
    let xlsx = xlsx_with_package_part(XLSX_BASE, WORKSHEET_RELS_PATH, changed.as_bytes());
    let (exit, stdout, stderr) = run_worker(&xlsx);
    let stdout_contains_userinfo = String::from_utf8_lossy(&stdout).contains(URI_USERINFO_SENTINEL);
    let stderr_contains_userinfo = String::from_utf8_lossy(&stderr).contains(URI_USERINFO_SENTINEL);

    assert_ne!(
        exit, 0,
        "external URL userinfo was accepted (stdout_contains_userinfo={stdout_contains_userinfo}, stderr_contains_userinfo={stderr_contains_userinfo})"
    );
    assert!(
        stdout.is_empty(),
        "failed inspection must not emit a result"
    );
    let stderr_text = String::from_utf8_lossy(&stderr);
    assert!(!stderr_text.contains(URI_USERINFO_SENTINEL));
    let failure: WorkerFailure =
        serde_json::from_slice(&stderr).expect("stderr contains one structured failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
    let serialized_failure = serde_json::to_vec(&failure).expect("failure serializes");
    assert!(!String::from_utf8_lossy(&serialized_failure).contains(URI_USERINFO_SENTINEL));
}

#[test]
fn external_workbook_userinfo_fails_closed_without_serializing_credential_bytes() {
    let relationship = String::from_utf8(read_package_part(
        XLSX_EXTERNAL_REFERENCE,
        EXTERNAL_LINK_RELS_PATH,
    ))
    .expect("qualified external workbook relationships XML is UTF-8");
    let changed = relationship.replace(
        "external-book.xlsx",
        &format!(
            "https://service:{EXTERNAL_WORKBOOK_USERINFO_SENTINEL}@example.invalid/external-book.xlsx"
        ),
    );
    assert_ne!(
        changed, relationship,
        "qualified fixture must contain the external workbook target"
    );
    let xlsx = xlsx_with_package_part(
        XLSX_EXTERNAL_REFERENCE,
        EXTERNAL_LINK_RELS_PATH,
        changed.as_bytes(),
    );
    let (exit, stdout, stderr) = run_worker(&xlsx);
    let stdout_contains_userinfo =
        String::from_utf8_lossy(&stdout).contains(EXTERNAL_WORKBOOK_USERINFO_SENTINEL);
    let stderr_contains_userinfo =
        String::from_utf8_lossy(&stderr).contains(EXTERNAL_WORKBOOK_USERINFO_SENTINEL);

    assert_ne!(
        exit, 0,
        "external workbook URI userinfo was accepted (stdout_contains_userinfo={stdout_contains_userinfo}, stderr_contains_userinfo={stderr_contains_userinfo})"
    );
    assert!(
        stdout.is_empty(),
        "failed inspection must not emit a result"
    );
    let stderr_text = String::from_utf8_lossy(&stderr);
    assert!(!stderr_text.contains(EXTERNAL_WORKBOOK_USERINFO_SENTINEL));
    let failure: WorkerFailure =
        serde_json::from_slice(&stderr).expect("stderr contains one structured failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
    let serialized_failure = serde_json::to_vec(&failure).expect("failure serializes");
    assert!(
        !String::from_utf8_lossy(&serialized_failure).contains(EXTERNAL_WORKBOOK_USERINFO_SENTINEL)
    );
}

#[test]
fn odbc_server_userinfo_fails_closed_without_serializing_credential_bytes() {
    let xml = read_connections_xml(ODBC_CONNECTION);
    let changed = xml.replace(
        "SERVER=db.internal",
        &format!("SERVER=user:{REVIEWED_BYPASS_SENTINEL}@db.internal"),
    );
    assert_ne!(
        changed, xml,
        "qualified ODBC fixture must contain the server"
    );
    let xlsx = xlsx_with_connections_xml(ODBC_CONNECTION, &changed);
    assert_rejected_without_marker(&xlsx, REVIEWED_BYPASS_SENTINEL);
}

#[test]
fn schemeless_external_workbook_userinfo_fails_closed_without_serializing_credential_bytes() {
    let relationship = String::from_utf8(read_package_part(
        XLSX_EXTERNAL_REFERENCE,
        EXTERNAL_LINK_RELS_PATH,
    ))
    .expect("qualified external workbook relationships XML is UTF-8");
    let changed = relationship.replace(
        "external-book.xlsx",
        &format!("user:{REVIEWED_BYPASS_SENTINEL}@example.invalid/external-book.xlsx"),
    );
    assert_ne!(changed, relationship, "qualified target must be present");
    let xlsx = xlsx_with_package_part(
        XLSX_EXTERNAL_REFERENCE,
        EXTERNAL_LINK_RELS_PATH,
        changed.as_bytes(),
    );
    assert_rejected_without_marker(&xlsx, REVIEWED_BYPASS_SENTINEL);
}

#[test]
fn external_hyperlink_query_credential_alias_fails_closed_without_serializing_value() {
    let relationship = String::from_utf8(read_package_part(XLSX_BASE, WORKSHEET_RELS_PATH))
        .expect("qualified relationships XML is UTF-8");
    let changed = relationship.replace(
        "https://example.com/a",
        &format!("https://example.com/a?credential={REVIEWED_BYPASS_SENTINEL}"),
    );
    assert_ne!(changed, relationship, "qualified target must be present");
    let xlsx = xlsx_with_package_part(XLSX_BASE, WORKSHEET_RELS_PATH, changed.as_bytes());
    assert_rejected_without_marker(&xlsx, REVIEWED_BYPASS_SENTINEL);
}

#[test]
fn all_supported_odbc_fields_reject_userinfo_syntax_without_leak() {
    let baseline = read_connections_xml(ODBC_CONNECTION);
    let marker = REVIEWED_BYPASS_SENTINEL;
    let cases = [
        (
            "id",
            "id=\"1\"",
            format!("id=\"user:{marker}@db.internal\""),
        ),
        (
            "name",
            "name=\"FinanceDB\"",
            format!("name=\"user:{marker}@db.internal\""),
        ),
        (
            "refreshedVersion",
            "refreshedVersion=\"0\"",
            format!("refreshedVersion=\"user:{marker}@db.internal\""),
        ),
        (
            "background",
            "background=\"0\"",
            format!("background=\"user:{marker}@db.internal\""),
        ),
        (
            "driver",
            "DRIVER={PostgreSQL Unicode}",
            format!("DRIVER={{user:{marker}@db.internal}}"),
        ),
        (
            "port",
            "PORT=5432",
            format!("PORT=user:{marker}@db.internal"),
        ),
        (
            "database",
            "DATABASE=finance",
            format!("DATABASE=user:{marker}@db.internal"),
        ),
        (
            "uid",
            "UID=readonly",
            format!("UID=user:{marker}@db.internal"),
        ),
        (
            "command",
            "command=\"SELECT account_id,balance FROM ledger\"",
            format!("command=\"SELECT 'user:{marker}@db.internal' FROM ledger\""),
        ),
    ];

    let mut accepted_or_echoed = Vec::new();
    for (case_name, original, replacement) in cases {
        let changed = baseline.replace(original, &replacement);
        assert_ne!(changed, baseline, "qualified fixture missing {case_name}");
        let xlsx = xlsx_with_connections_xml(ODBC_CONNECTION, &changed);
        let (exit, stdout, stderr) = run_worker(&xlsx);
        if exit == 0
            || String::from_utf8_lossy(&stdout).contains(marker)
            || String::from_utf8_lossy(&stderr).contains(marker)
        {
            accepted_or_echoed.push(case_name);
        }
    }
    assert!(
        accepted_or_echoed.is_empty(),
        "ODBC fields accepted or echoed credential syntax: {accepted_or_echoed:?}"
    );
}

#[test]
fn unqualified_sql_command_with_string_literal_fails_closed_without_leak() {
    let xml = read_connections_xml(ODBC_CONNECTION);
    let changed = xml.replace(
        "SELECT account_id,balance FROM ledger",
        &format!("SELECT account_id FROM ledger WHERE access='user:{REVIEWED_BYPASS_SENTINEL}@db.internal'"),
    );
    assert_ne!(changed, xml, "qualified command must be present");
    let xlsx = xlsx_with_connections_xml(ODBC_CONNECTION, &changed);
    assert_rejected_without_marker(&xlsx, REVIEWED_BYPASS_SENTINEL);
}

#[test]
fn odbc_connection_over_derived_byte_bound_fails_before_segment_allocation() {
    let xml = read_connections_xml(ODBC_CONNECTION);
    let oversized_value = "a".repeat(1_025);
    let changed = xml.replace("UID=readonly", &format!("UID={oversized_value}"));
    assert_ne!(changed, xml, "qualified UID field must be present");
    let xlsx = xlsx_with_connections_xml(ODBC_CONNECTION, &changed);
    let (exit, stdout, stderr) = run_worker(&xlsx);
    assert_ne!(exit, 0, "oversized connection string was accepted");
    assert!(
        stdout.is_empty(),
        "failed inspection must not emit a result"
    );
    let failure: WorkerFailure =
        serde_json::from_slice(&stderr).expect("stderr contains one structured failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::InspectionResourceLimitExceeded
    );
    assert!(!String::from_utf8_lossy(&stderr).contains(&oversized_value));
}

fn run_worker(bytes: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: XLSX_MEDIA_TYPE.to_owned(),
        expected_raw_content_hash: Sha256::digest(bytes).into(),
        expected_size_bytes: bytes.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).expect("serialize request");
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
    (exit, stdout, stderr)
}

fn assert_worker_fails_closed(bytes: &[u8]) {
    let (exit, stdout, stderr) = run_worker(bytes);
    assert_ne!(exit, 0, "unsupported connection semantics were accepted");
    assert!(
        stdout.is_empty(),
        "failed inspection must not emit a result"
    );
    let failure: WorkerFailure =
        serde_json::from_slice(&stderr).expect("stderr contains one structured failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

fn assert_rejected_without_marker(bytes: &[u8], marker: &str) {
    let (exit, stdout, stderr) = run_worker(bytes);
    let stdout_contains_marker = String::from_utf8_lossy(&stdout).contains(marker);
    let stderr_contains_marker = String::from_utf8_lossy(&stderr).contains(marker);
    assert_ne!(
        exit, 0,
        "credential-bearing definition was accepted (stdout_contains_marker={stdout_contains_marker}, stderr_contains_marker={stderr_contains_marker})"
    );
    assert!(
        stdout.is_empty(),
        "failed inspection must not emit a result"
    );
    assert!(!stderr_contains_marker, "failure must not echo the marker");
    let failure: WorkerFailure =
        serde_json::from_slice(&stderr).expect("stderr contains one structured failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

fn xlsx_with_odbc_password(input: &[u8]) -> Vec<u8> {
    let xml = read_connections_xml(input);
    let changed = xml.replace(
        "UID=readonly",
        &format!("UID=readonly;PWD={PASSWORD_SENTINEL}"),
    );
    assert_ne!(
        changed, xml,
        "fixture must contain a noncredential ODBC connection"
    );
    xlsx_with_connections_xml(input, &changed)
}

fn read_connections_xml(input: &[u8]) -> String {
    let contents = read_package_part(input, CONNECTIONS_PATH);
    String::from_utf8(contents).expect("connections XML is UTF-8")
}

fn xlsx_with_connections_xml(input: &[u8], connections_xml: &str) -> Vec<u8> {
    xlsx_with_package_part(input, CONNECTIONS_PATH, connections_xml.as_bytes())
}

fn read_package_part(input: &[u8], part_path: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("qualified XLSX fixture");
    let mut entry = archive.by_name(part_path).expect("qualified package part");
    let mut contents = Vec::new();
    entry.read_to_end(&mut contents).expect("read package part");
    contents
}

fn xlsx_with_package_part(input: &[u8], part_path: &str, replacement: &[u8]) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("qualified XLSX fixture");
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .expect("read qualified package entry");
        let name = entry.name().to_owned();
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .expect("read qualified package part");
        if name == part_path {
            contents = replacement.to_vec();
        }
        entries.push((name, contents));
    }
    drop(archive);

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries {
        writer
            .start_file(name, options)
            .expect("write synthetic XLSX part");
        writer
            .write_all(&contents)
            .expect("write synthetic XLSX bytes");
    }
    writer
        .finish()
        .expect("finish synthetic XLSX package")
        .into_inner()
}
