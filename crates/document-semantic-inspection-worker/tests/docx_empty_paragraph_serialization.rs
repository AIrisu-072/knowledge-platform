use std::io::{Cursor, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
    WorkerFailure, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const WORD_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

#[test]
fn self_closing_and_expanded_empty_paragraphs_have_the_same_fingerprint() {
    let self_closing = inspect_valid(&docx_with_empty_paragraph("<w:p/>"));
    let expanded = inspect_valid(&docx_with_empty_paragraph("<w:p></w:p>"));

    assert_eq!(
        self_closing.semantic_fingerprint(),
        expanded.semantic_fingerprint(),
        "self-closing and expanded empty paragraph serialization must be equivalent",
    );
}

#[test]
fn inserting_a_self_closing_empty_paragraph_changes_identity_or_fails_closed() {
    let without_empty_paragraph = inspect_valid(&docx_with_empty_paragraph(""));
    let with_empty_paragraph = inspect(&docx_with_empty_paragraph("<w:p/>"));

    match with_empty_paragraph {
        Ok(with_empty_paragraph) => assert_ne!(
            without_empty_paragraph.semantic_fingerprint(),
            with_empty_paragraph.semantic_fingerprint(),
            "an inserted empty paragraph changes paragraph structure",
        ),
        Err(error) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "an empty paragraph that cannot be projected must fail closed as unsupported",
        ),
    }
}

fn inspect_valid(bytes: &[u8]) -> SemanticAdapterOutput {
    inspect(bytes).expect("synthetic DOCX passes the coverage sentinel and adapter")
}

fn inspect(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)?;
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn docx_with_empty_paragraph(empty_paragraph: &str) -> Vec<u8> {
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="{WORD_MAIN}"/></Types>"#
    );
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body><w:p><w:r><w:t>Stable body text</w:t></w:r></w:p>{empty_paragraph}</w:body></w:document>"#
    );
    let document_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_REL_NS}"/>"#
    );

    stored_docx(vec![
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", ROOT_RELATIONSHIPS.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        (
            "word/_rels/document.xml.rels",
            document_relationships.into_bytes(),
        ),
    ])
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
