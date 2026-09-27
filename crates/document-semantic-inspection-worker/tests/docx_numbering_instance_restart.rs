use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
    WorkerFailure, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const OFFICE_REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const MAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const NUMBERING_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const ROOT_RELATIONSHIPS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

#[test]
fn restarting_a_second_numbering_instance_changes_identity_or_fails_closed() {
    let continuing_instance = numbered_docx("1");
    let restarted_instance = numbered_docx("2");
    assert_only_second_paragraph_num_id_changes(&continuing_instance, &restarted_instance);

    let continuing_output = inspect(&continuing_instance);
    let restarted_output = inspect(&restarted_instance);

    match (continuing_output, restarted_output) {
        (Ok(continuing), Ok(restarted)) => assert_ne!(
            continuing.semantic_fingerprint(),
            restarted.semantic_fingerprint(),
            "a new w:num instance restarts the list and changes semantic identity",
        ),
        (Err(continuing), Err(restarted)) => {
            assert_eq!(
                continuing.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the continuing-instance DOCX must be explicitly unsupported",
            );
            assert_eq!(
                restarted.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the restarted-instance DOCX must be explicitly unsupported",
            );
        }
        (continuing, restarted) => panic!(
            "both valid numbering-instance DOCX files must be distinguished or explicitly unsupported: {continuing:?}, {restarted:?}"
        ),
    }
}

fn inspect(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic numbering-instance DOCX passes the OOXML coverage sentinel");
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn numbered_docx(second_num_id: &str) -> Vec<u8> {
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="{MAIN_CONTENT_TYPE}"/><Override PartName="/word/numbering.xml" ContentType="{NUMBERING_CONTENT_TYPE}"/></Types>"#
    );
    let document = format!(
        r#"<w:document xmlns:w="{WORD_NS}"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First list item</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="{second_num_id}"/></w:numPr></w:pPr><w:r><w:t>Second list item</w:t></w:r></w:p></w:body></w:document>"#
    );
    let document_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdNumbering" Type="{OFFICE_REL_NS}/numbering" Target="numbering.xml"/></Relationships>"#
    );
    let numbering = format!(
        r#"<w:numbering xmlns:w="{WORD_NS}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );

    stored_docx(vec![
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", ROOT_RELATIONSHIPS.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        (
            "word/_rels/document.xml.rels",
            document_relationships.into_bytes(),
        ),
        ("word/numbering.xml", numbering.into_bytes()),
    ])
}

fn assert_only_second_paragraph_num_id_changes(continuing: &[u8], restarted: &[u8]) {
    let continuing_parts = package_parts(continuing);
    let restarted_parts = package_parts(restarted);
    assert_eq!(
        continuing_parts.keys().collect::<Vec<_>>(),
        restarted_parts.keys().collect::<Vec<_>>(),
        "the DOCX package parts must be identical",
    );

    for (name, continuing_contents) in &continuing_parts {
        let restarted_contents = &restarted_parts[name];
        if name == "word/document.xml" {
            let continuing_xml = String::from_utf8(continuing_contents.clone())
                .expect("synthetic document XML is UTF-8");
            let expected = continuing_xml.replacen(
                r#"<w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Second list item"#,
                r#"<w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>Second list item"#,
                1,
            );
            assert_ne!(expected, continuing_xml);
            assert_eq!(expected.as_bytes(), restarted_contents.as_slice());
        } else {
            assert_eq!(
                continuing_contents, restarted_contents,
                "fixture changed unrelated package part {name}",
            );
        }
    }
}

fn package_parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("synthetic DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("synthetic DOCX member");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("read synthetic DOCX member");
        assert!(
            parts.insert(name.clone(), contents).is_none(),
            "duplicate part {name}"
        );
    }
    parts
}

fn stored_docx(parts: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);

    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("start synthetic DOCX part");
        writer
            .write_all(&contents)
            .expect("write synthetic DOCX part");
    }

    writer
        .finish()
        .expect("finish synthetic DOCX ZIP")
        .into_inner()
}
