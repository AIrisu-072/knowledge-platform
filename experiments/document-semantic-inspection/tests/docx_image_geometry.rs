use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile,
    fingerprint as semantic_fingerprint,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

#[test]
fn image_preset_geometry_changes_identity_or_fails_closed() {
    let base = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/docx/base.docx"),
    )
    .expect("valid body-image DOCX fixture");
    let base_parts = package_parts(&base);
    let base_document =
        String::from_utf8(base_parts["word/document.xml"].clone()).expect("document XML");
    assert_eq!(base_document.matches("</pic:pic>").count(), 1);

    let rect_document = base_document.replace(
        "</pic:pic>",
        r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>"#,
    );
    assert_eq!(rect_document.matches("prst=\"rect\"").count(), 1);

    let ellipse_document = rect_document.replace("prst=\"rect\"", "prst=\"ellipse\"");
    assert_eq!(
        ellipse_document.replace("prst=\"ellipse\"", "prst=\"rect\""),
        rect_document,
        "the only document XML change is a:prstGeom/@prst",
    );

    let rect_docx = docx_with_document(&base_parts, &rect_document);
    let ellipse_docx = docx_with_document(&base_parts, &ellipse_document);
    let rect_parts = package_parts(&rect_docx);
    let ellipse_parts = package_parts(&ellipse_docx);
    assert_eq!(
        rect_parts.keys().collect::<Vec<_>>(),
        ellipse_parts.keys().collect::<Vec<_>>()
    );
    for (name, bytes) in &rect_parts {
        if name != "word/document.xml" {
            assert_eq!(bytes, &ellipse_parts[name], "package part {name}");
        }
    }

    let profile = InspectionProfile::default();
    let rect_output = DocxAdapter
        .inspect(&rect_docx, &profile)
        .expect("rect image geometry baseline should inspect successfully");
    let ellipse_result = DocxAdapter.inspect(&ellipse_docx, &profile);
    match ellipse_result {
        Ok(ellipse_output) => assert_ne!(
            semantic_fingerprint(&rect_output.semantic_projection),
            semantic_fingerprint(&ellipse_output.semantic_projection),
            "ellipse geometry changes the visible shape while the PNG pixels remain identical",
        ),
        Err(error) => assert_eq!(
            error.code(),
            ErrorCode::UnsupportedSemanticConstruct,
            "unsupported image geometry must fail closed with an explicit semantic classification",
        ),
    }
}

fn package_parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("DOCX part");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).expect("read DOCX part");
        parts.insert(name, contents);
    }
    parts
}

fn docx_with_document(base_parts: &BTreeMap<String, Vec<u8>>, document: &str) -> Vec<u8> {
    let mut parts = base_parts.clone();
    parts.insert("word/document.xml".to_owned(), document.as_bytes().to_vec());

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
