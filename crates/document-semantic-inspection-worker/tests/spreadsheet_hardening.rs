use std::io::{Cursor, Read, Write};

use document_semantic_inspection_core::{
    InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest,
};
use document_semantic_inspection_worker::{
    AdapterProfile, SemanticAdapter, SemanticAdapterOutput, SpreadsheetAdapter, WorkerFailure,
    WorkerFailureCode, run_worker_shell,
};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const XLSX_BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/xlsx/base.xlsx");
const XLSM_BASE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsm/calamine-vba.xlsm"
);

const XLSX_MEDIA_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const XLSM_MEDIA_TYPE: &str = "application/vnd.ms-excel.sheet.macroEnabled.12";

const CELL_BODY: &str = "DSI_CELL_BODY_LEAK_SENTINEL";
const FORMULA_BODY: &str = "DSI_FORMULA_BODY_LEAK_SENTINEL";
const LINK_BODY: &str = "DSI_LINK_BODY_LEAK_SENTINEL";
const VBA_BODY: &str = "DSI_VBA_BODY_LEAK_SENTINEL";
const COMMENT_BODY: &str = "DSI_EDITORIAL_COMMENT_SENTINEL";

const CONTENT_TYPES_PATH: &str = "[Content_Types].xml";
const WORKSHEET_PATH: &str = "xl/worksheets/sheet1.xml";
const WORKSHEET_RELS_PATH: &str = "xl/worksheets/_rels/sheet1.xml.rels";
const VBA_PROJECT_PATH: &str = "xl/vbaProject.bin";

fn inspect_xlsx(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    SpreadsheetAdapter::XLSX.inspect(bytes, &AdapterProfile::default())
}

fn inspect_xlsm(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    SpreadsheetAdapter::XLSM.inspect(bytes, &AdapterProfile::default())
}

#[test]
fn xlsx_comments_are_editorial_evidence_without_changing_semantic_identity() {
    let base = inspect_xlsx(XLSX_BASE).expect("PoC-qualified base XLSX");
    let commented_bytes = xlsx_with_comment(XLSX_BASE, COMMENT_BODY);
    let commented = inspect_xlsx(&commented_bytes).expect("synthetic comment-only XLSX");

    assert_eq!(
        commented.semantic_fingerprint(),
        base.semantic_fingerprint(),
        "an editorial comment must not alter workbook identity"
    );
    assert!(base.editorial_provenance().comments.is_empty());
    let comments = &commented.editorial_provenance().comments;
    assert_eq!(comments.len(), 1, "the synthetic comment must be retained");
    assert!(
        comments[0].content.contains(COMMENT_BODY),
        "the comment body belongs in editorial evidence"
    );
    assert!(
        comments[0].source_locator.contains("R1C1"),
        "the comment evidence must identify the A1 cell that carries it"
    );
}

#[test]
fn qualified_content_types_prefix_noise_preserves_semantic_identity() {
    let base = inspect_xlsx(XLSX_BASE).expect("PoC-qualified base XLSX");
    assert_prefix_noise_preserves_identity(
        &base,
        CONTENT_TYPES_PATH,
        "Types",
        &["Default", "Override"],
        "http://schemas.openxmlformats.org/package/2006/content-types",
        "ct",
    );
}

#[test]
fn qualified_worksheet_relationships_prefix_noise_preserves_semantic_identity() {
    let base = inspect_xlsx(XLSX_BASE).expect("PoC-qualified base XLSX");
    assert_prefix_noise_preserves_identity(
        &base,
        WORKSHEET_RELS_PATH,
        "Relationships",
        &["Relationship"],
        "http://schemas.openxmlformats.org/package/2006/relationships",
        "rel",
    );
}

#[test]
fn xlsx_parser_disagreement_does_not_leak_cell_formula_or_link_bodies() {
    let conflicting = xlsx_with_conflicting_internal_link(XLSX_BASE);
    let direct_failure = inspect_xlsx(&conflicting)
        .expect_err("conflicting parser interpretations must fail closed");

    assert_eq!(direct_failure.code(), WorkerFailureCode::ParserDisagreement);
    assert_no_representative_body(
        direct_failure.message(),
        &[CELL_BODY, FORMULA_BODY, LINK_BODY],
    );
    assert_shell_failure_is_bounded(
        &conflicting,
        XLSX_MEDIA_TYPE,
        Some(WorkerFailureCode::ParserDisagreement),
        &[CELL_BODY, FORMULA_BODY, LINK_BODY],
    );
}

#[test]
fn malformed_xlsx_and_xlsm_fail_without_leaking_bodies_or_partial_stdout() {
    let malformed_xlsx = xlsx_with_malformed_cell(XLSX_BASE);
    let xlsx_failure =
        inspect_xlsx(&malformed_xlsx).expect_err("malformed worksheet XML must fail closed");
    assert_no_representative_body(
        xlsx_failure.message(),
        &[CELL_BODY, FORMULA_BODY, LINK_BODY],
    );
    assert_shell_failure_is_bounded(
        &malformed_xlsx,
        XLSX_MEDIA_TYPE,
        None,
        &[CELL_BODY, FORMULA_BODY, LINK_BODY],
    );

    let malformed_xlsm = xlsm_with_malformed_vba(XLSM_BASE);
    let xlsm_failure =
        inspect_xlsm(&malformed_xlsm).expect_err("incomplete VBA syntax must fail closed");
    assert_eq!(
        xlsm_failure.code(),
        WorkerFailureCode::SemanticExtractionFailed
    );
    assert_no_representative_body(xlsm_failure.message(), &[VBA_BODY]);
    assert_shell_failure_is_bounded(
        &malformed_xlsm,
        XLSM_MEDIA_TYPE,
        Some(WorkerFailureCode::SemanticExtractionFailed),
        &[VBA_BODY],
    );
}

#[test]
fn rxls_text_truncation_must_not_be_returned_as_success() {
    let truncated = xlsx_that_exhausts_rxls_xml_reference_budget(XLSX_BASE);
    let workbook = rxls::Workbook::open(&truncated)
        .expect("synthetic XLSX remains a readable package to the qualified parser");
    assert!(
        workbook.text_truncated,
        "fixture must exercise rxls's partial-text signal"
    );

    let _failure = inspect_xlsx(&truncated)
        .expect_err("a truncated parser projection cannot be a successful inspection");
    assert_shell_failure_is_bounded(&truncated, XLSX_MEDIA_TYPE, None, &[]);
}

fn xlsx_with_comment(input: &[u8], comment_body: &str) -> Vec<u8> {
    let content_types = String::from_utf8(read_package_entry(input, CONTENT_TYPES_PATH))
        .expect("content-types XML is UTF-8");
    let original_content_types = content_types.clone();
    let content_types = content_types.replace(
        "</Types>",
        r#"<Override PartName="/xl/comments1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml"/></Types>"#,
    );
    assert_ne!(content_types, original_content_types);

    let worksheet_rels = String::from_utf8(read_package_entry(input, WORKSHEET_RELS_PATH))
        .expect("worksheet relationships XML is UTF-8");
    let original_worksheet_rels = worksheet_rels.clone();
    let worksheet_rels = worksheet_rels.replace(
        "</Relationships>",
        r#"<Relationship Id="rIdComments" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../comments1.xml"/></Relationships>"#,
    );
    assert_ne!(worksheet_rels, original_worksheet_rels);

    let comments_xml = format!(
        r#"<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>Synthetic reviewer</author></authors><commentList><comment ref="A1" authorId="0"><text><t>{comment_body}</t></text></comment></commentList></comments>"#
    );
    rewrite_package(
        input,
        &[
            (CONTENT_TYPES_PATH, content_types.as_bytes()),
            (WORKSHEET_RELS_PATH, worksheet_rels.as_bytes()),
        ],
        &[("xl/comments1.xml", comments_xml.as_bytes())],
    )
}

fn xlsx_with_prefixed_namespace(
    input: &[u8],
    part_path: &str,
    root: &str,
    children: &[&str],
    namespace: &str,
    prefix: &str,
) -> Vec<u8> {
    let original = String::from_utf8(read_package_entry(input, part_path))
        .expect("qualified OOXML part is UTF-8");
    let mut prefixed = original.replace(
        &format!(r#"<{root} xmlns="{namespace}">"#),
        &format!(r#"<{prefix}:{root} xmlns:{prefix}="{namespace}">"#),
    );
    prefixed = prefixed.replace(&format!("</{root}>"), &format!("</{prefix}:{root}>"));
    for child in children {
        prefixed = prefixed
            .replace(&format!("<{child} "), &format!("<{prefix}:{child} "))
            .replace(&format!("</{child}>"), &format!("</{prefix}:{child}>"));
    }
    assert_ne!(
        prefixed, original,
        "prefix fixture must alter exactly one part"
    );

    rewrite_package(input, &[(part_path, prefixed.as_bytes())], &[])
}

fn assert_prefix_noise_preserves_identity(
    base: &SemanticAdapterOutput,
    part_path: &str,
    root: &str,
    children: &[&str],
    namespace: &str,
    prefix: &str,
) {
    let serialized =
        xlsx_with_prefixed_namespace(XLSX_BASE, part_path, root, children, namespace, prefix);
    let variant = inspect_xlsx(&serialized)
        .expect("prefix-only namespace serialization of a qualified base XLSX");

    assert_eq!(
        variant.semantic_fingerprint(),
        base.semantic_fingerprint(),
        "prefix-only XML serialization noise in {part_path} must not alter workbook identity"
    );
}

fn xlsx_with_conflicting_internal_link(input: &[u8]) -> Vec<u8> {
    let worksheet = String::from_utf8(read_package_entry(input, WORKSHEET_PATH))
        .expect("worksheet XML is UTF-8");
    let worksheet = worksheet.replace("<t>Hello</t>", &format!("<t>{CELL_BODY}</t>"));
    let worksheet = worksheet.replace(
        "<f>SUM(1,2)</f>",
        &format!(r#"<f>SUM(1,2)+N("{FORMULA_BODY}")</f>"#),
    );
    let worksheet = worksheet.replace(
        r#"<hyperlink ref="A1" r:id="rIdHyper"/>"#,
        &format!(r#"<hyperlink ref="A1" location="'Sheet1'!B1" display="{LINK_BODY}"/>"#),
    );
    assert!(worksheet.contains(CELL_BODY));
    assert!(worksheet.contains(FORMULA_BODY));
    assert!(worksheet.contains(LINK_BODY));

    let empty_relationships =
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;
    rewrite_package(
        input,
        &[
            (WORKSHEET_PATH, worksheet.as_bytes()),
            (WORKSHEET_RELS_PATH, empty_relationships),
        ],
        &[],
    )
}

fn xlsx_with_malformed_cell(input: &[u8]) -> Vec<u8> {
    let worksheet = String::from_utf8(read_package_entry(input, WORKSHEET_PATH))
        .expect("worksheet XML is UTF-8");
    let worksheet = worksheet
        .replace("<t>Hello</t>", &format!("<t>{CELL_BODY}</wrong>"))
        .replace(
            "<f>SUM(1,2)</f>",
            &format!(r#"<f>SUM(1,2)+N("{FORMULA_BODY}")</f>"#),
        );
    assert!(worksheet.contains(CELL_BODY));
    assert!(worksheet.contains(FORMULA_BODY));

    let relationships = String::from_utf8(read_package_entry(input, WORKSHEET_RELS_PATH))
        .expect("worksheet relationships XML is UTF-8");
    let relationships = relationships.replace(
        "https://example.com/a",
        &format!("https://example.invalid/{LINK_BODY}"),
    );
    assert!(relationships.contains(LINK_BODY));

    rewrite_package(
        input,
        &[
            (WORKSHEET_PATH, worksheet.as_bytes()),
            (WORKSHEET_RELS_PATH, relationships.as_bytes()),
        ],
        &[],
    )
}

fn xlsx_that_exhausts_rxls_xml_reference_budget(input: &[u8]) -> Vec<u8> {
    // rxls 0.1.3 caps XML general-reference work at 1 << 20 per part. Repeated
    // valid ampersand entities reach its partial-text signal with a small XLSX.
    let worksheet = String::from_utf8(read_package_entry(input, WORKSHEET_PATH))
        .expect("worksheet XML is UTF-8");
    let expanded_cell_text = "&amp;".repeat((1 << 20) + 1);
    let worksheet = worksheet.replace("<t>Hello</t>", &format!("<t>{expanded_cell_text}</t>"));
    assert!(worksheet.len() > 5_000_000);
    rewrite_package(input, &[(WORKSHEET_PATH, worksheet.as_bytes())], &[])
}

fn xlsm_with_malformed_vba(input: &[u8]) -> Vec<u8> {
    let vba_project = read_package_entry(input, VBA_PROJECT_PATH);
    let project = ovba::open_project(vba_project.clone()).expect("PoC-qualified VBA project");
    let module = project
        .modules
        .iter()
        .find(|module| module.name == "testVBA")
        .expect("synthetic seed has the testVBA module");
    let stream_path = format!("/VBA/{}", module.stream_name);
    let text_offset = module.text_offset;
    drop(project);

    let mut compound =
        cfb::CompoundFile::open(Cursor::new(vba_project)).expect("open synthetic VBA CFB");
    let mut stream_bytes = Vec::new();
    compound
        .open_stream(&stream_path)
        .expect("open testVBA module stream")
        .read_to_end(&mut stream_bytes)
        .expect("read testVBA module stream");
    assert!(text_offset <= stream_bytes.len());

    let source = format!(
        "Attribute VB_Name = \"testVBA\"\nPublic Sub test()\n    MsgBox \"{VBA_BODY}\"\nEnd Sub\nPublic Sub broken(\n"
    );
    let mut replacement = stream_bytes[..text_offset].to_vec();
    replacement.extend_from_slice(&compress_vba_literals(source.as_bytes()));
    compound
        .create_stream(&stream_path)
        .expect("replace testVBA module stream")
        .write_all(&replacement)
        .expect("write synthetic malformed VBA source");
    compound.flush().expect("flush synthetic VBA project");
    let changed_vba_project = compound.into_inner().into_inner();

    rewrite_package(input, &[(VBA_PROJECT_PATH, &changed_vba_project)], &[])
}

fn compress_vba_literals(source: &[u8]) -> Vec<u8> {
    assert!(!source.is_empty());
    let mut chunk = Vec::with_capacity(source.len() + source.len().div_ceil(8));
    for group in source.chunks(8) {
        chunk.push(0);
        chunk.extend_from_slice(group);
    }
    assert!(
        chunk.len() <= 4096,
        "synthetic module must fit one VBA chunk"
    );

    let header = 0xb000u16 | u16::try_from(chunk.len() - 1).expect("VBA chunk size");
    let mut compressed = Vec::with_capacity(3 + chunk.len());
    compressed.push(0x01);
    compressed.extend_from_slice(&header.to_le_bytes());
    compressed.extend_from_slice(&chunk);
    compressed
}

fn read_package_entry(input: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("valid synthetic OOXML package");
    let mut entry = archive.by_name(name).expect("required package entry");
    let mut contents = Vec::new();
    entry
        .read_to_end(&mut contents)
        .expect("read synthetic package entry");
    contents
}

fn rewrite_package(
    input: &[u8],
    replacements: &[(&str, &[u8])],
    additions: &[(&str, &[u8])],
) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("valid synthetic OOXML package");
    let mut entries = Vec::with_capacity(archive.len() + additions.len());
    let mut replacements_seen = vec![false; replacements.len()];
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("read package entry");
        let name = entry.name().to_owned();
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .expect("read package entry bytes");
        if let Some(replacement_index) = replacements
            .iter()
            .position(|(path, _)| *path == name.as_str())
        {
            contents = replacements[replacement_index].1.to_vec();
            replacements_seen[replacement_index] = true;
        }
        entries.push((name, contents));
    }

    assert!(
        replacements_seen.iter().all(|seen| *seen),
        "every synthetic package replacement must name an existing part"
    );
    for (name, _) in additions {
        assert!(
            !entries
                .iter()
                .any(|(existing, _)| existing.as_str() == *name),
            "synthetic package addition must not duplicate a part"
        );
    }
    drop(archive);

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries.into_iter().chain(
        additions
            .iter()
            .map(|(name, contents)| ((*name).to_owned(), contents.to_vec())),
    ) {
        writer
            .start_file(name, options)
            .expect("write synthetic package entry");
        writer
            .write_all(&contents)
            .expect("write synthetic package bytes");
    }
    writer
        .finish()
        .expect("finish synthetic OOXML package")
        .into_inner()
}

fn assert_shell_failure_is_bounded(
    input: &[u8],
    media_type: &str,
    expected_code: Option<WorkerFailureCode>,
    bodies: &[&str],
) {
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: media_type.to_owned(),
        expected_raw_content_hash: Sha256::digest(input).into(),
        expected_size_bytes: input.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).expect("serialize synthetic request");
    let mut source = Cursor::new(input);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_worker_shell(
        &request_bytes,
        &mut source,
        &mut stdout,
        &mut stderr,
        64 * 1024,
        8 * 1024 * 1024,
    );

    assert_ne!(
        exit_code, 0,
        "failed inspection must return a non-zero status"
    );
    assert!(
        stdout.is_empty(),
        "failed inspection must not emit a partial structured response"
    );
    let stderr_text = String::from_utf8_lossy(&stderr);
    assert_no_representative_body(&stderr_text, bodies);
    let failure: WorkerFailure =
        serde_json::from_slice(&stderr).expect("stderr contains one structured failure");
    if let Some(expected_code) = expected_code {
        assert_eq!(failure.code(), expected_code);
    }
    assert_no_representative_body(failure.message(), bodies);
}

fn assert_no_representative_body(message: &str, bodies: &[&str]) {
    for body in bodies {
        assert!(
            !message.contains(body),
            "failure diagnostics must not contain representative document bodies"
        );
    }
}
