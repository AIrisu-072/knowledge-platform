mod support;

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile,
    fingerprint as semantic_fingerprint,
};
use support::ooxml::docx_fixture;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

#[test]
fn self_closing_and_expanded_empty_paragraphs_are_equivalent() {
    let short = projection(&docx_with_empty_paragraph("<w:p/>"));
    let expanded = projection(&docx_with_empty_paragraph("<w:p></w:p>"));
    assert_eq!(
        semantic_fingerprint(&short),
        semantic_fingerprint(&expanded)
    );
}

#[test]
fn inserting_empty_paragraph_changes_structure_or_fails_closed() {
    let baseline = projection(&docx_with_empty_paragraph(""));
    let inserted = DocxAdapter.inspect(
        &docx_with_empty_paragraph("<w:p/>"),
        &InspectionProfile::default(),
    );
    match inserted {
        Ok(output) => assert_ne!(
            semantic_fingerprint(&baseline),
            semantic_fingerprint(&output.semantic_projection),
            "empty paragraph insertion changes paragraph structure",
        ),
        Err(error) => assert_eq!(error.code(), ErrorCode::UnsupportedSemanticConstruct),
    }
}

fn projection(bytes: &[u8]) -> Vec<u8> {
    DocxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .expect("valid DOCX baseline")
        .semantic_projection
}

fn docx_with_empty_paragraph(empty: &str) -> Vec<u8> {
    let base = docx_fixture("Stable body text");
    let mut archive = ZipArchive::new(Cursor::new(base)).expect("DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("DOCX part");
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).expect("read DOCX part");
        parts.insert(file.name().to_owned(), contents);
    }

    let document = String::from_utf8(parts["word/document.xml"].clone()).expect("document XML");
    assert!(document.contains("</w:body>"));
    let changed = document.replacen("</w:body>", &format!("{empty}</w:body>"), 1);
    if !empty.is_empty() {
        assert_ne!(changed, document);
    }
    parts.insert("word/document.xml".into(), changed.into_bytes());

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, contents) in parts {
        writer.start_file(name, options).expect("start DOCX part");
        writer.write_all(&contents).expect("write DOCX part");
    }
    writer.finish().expect("finish DOCX").into_inner()
}
