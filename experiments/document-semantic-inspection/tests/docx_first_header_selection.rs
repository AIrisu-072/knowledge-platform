mod support;

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    AdapterOutput, DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile, PocError,
    fingerprint,
};
use support::ooxml::docx_fixture;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const HEADER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const HEADER_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";

#[test]
fn title_page_header_selection_changes_identity_or_fails_closed() {
    let default_header_on_first_page = docx_with_title_page_header(false);
    let first_header_on_first_page = docx_with_title_page_header(true);
    assert_only_title_page_selection_differs(
        &default_header_on_first_page,
        &first_header_on_first_page,
    );

    let baseline = inspect_docx(&default_header_on_first_page)
        .expect("valid DOCX with default first-page header selection is accepted");

    match inspect_docx(&first_header_on_first_page) {
        Ok(first_page) => assert_ne!(
            fingerprint(&baseline.semantic_projection),
            fingerprint(&first_page.semantic_projection),
            "enabling w:titlePg selects different first-page header content and must change semantic identity"
        ),
        Err(error) if error.code() == ErrorCode::UnsupportedSemanticConstruct => {}
        Err(error) => panic!(
            "valid first-page header selection must be fingerprinted or explicitly rejected; got {error:?}"
        ),
    }
}

fn inspect_docx(bytes: &[u8]) -> Result<AdapterOutput, PocError> {
    DocxAdapter.inspect(bytes, &InspectionProfile::default())
}

fn docx_with_title_page_header(title_page_header: bool) -> Vec<u8> {
    let mut parts = read_parts(&docx_fixture("same body text"));

    let document = String::from_utf8(parts["word/document.xml"].clone()).expect("document XML");
    let document = document.replacen(
        &format!("xmlns:w=\"{W_NS}\""),
        &format!("xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\""),
        1,
    );
    assert!(document.contains(&format!("xmlns:r=\"{R_NS}\"")));

    let title_page = if title_page_header {
        "<w:titlePg/>"
    } else {
        ""
    };
    let section_properties = format!(
        "<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdDefaultHeader\"/><w:headerReference w:type=\"first\" r:id=\"rIdFirstHeader\"/>{title_page}</w:sectPr>"
    );
    let document = document.replace("</w:body>", &format!("{section_properties}</w:body>"));
    assert!(document.contains("w:type=\"default\" r:id=\"rIdDefaultHeader\""));
    assert!(document.contains("w:type=\"first\" r:id=\"rIdFirstHeader\""));
    assert_eq!(document.contains("<w:titlePg/>"), title_page_header);
    parts.insert("word/document.xml".into(), document.into_bytes());

    let relationships = format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdDefaultHeader\" Type=\"{HEADER_REL}\" Target=\"header1.xml\"/><Relationship Id=\"rIdFirstHeader\" Type=\"{HEADER_REL}\" Target=\"header2.xml\"/></Relationships>"
    );
    parts.insert(
        "word/_rels/document.xml.rels".into(),
        relationships.into_bytes(),
    );

    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone())
        .expect("content types XML")
        .replace(
            "</Types>",
            &format!(
                "<Override PartName=\"/word/header1.xml\" ContentType=\"{HEADER_CONTENT_TYPE}\"/><Override PartName=\"/word/header2.xml\" ContentType=\"{HEADER_CONTENT_TYPE}\"/></Types>"
            ),
        );
    parts.insert("[Content_Types].xml".into(), content_types.into_bytes());
    parts.insert(
        "word/header1.xml".into(),
        header_xml("Default page header").into_bytes(),
    );
    parts.insert(
        "word/header2.xml".into(),
        header_xml("First page header").into_bytes(),
    );

    write_parts(parts)
}

fn header_xml(text: &str) -> String {
    format!("<w:hdr xmlns:w=\"{W_NS}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:hdr>")
}

fn assert_only_title_page_selection_differs(before: &[u8], after: &[u8]) {
    let before_parts = read_parts(before);
    let after_parts = read_parts(after);
    assert_eq!(
        before_parts.keys().collect::<Vec<_>>(),
        after_parts.keys().collect::<Vec<_>>(),
        "title-page selection must not add or remove package parts"
    );

    for (name, before_data) in &before_parts {
        let after_data = &after_parts[name];
        if name == "word/document.xml" {
            let before_xml = String::from_utf8(before_data.clone()).expect("before document XML");
            let after_xml = String::from_utf8(after_data.clone()).expect("after document XML");
            assert!(before_xml.contains("w:type=\"default\" r:id=\"rIdDefaultHeader\""));
            assert!(before_xml.contains("w:type=\"first\" r:id=\"rIdFirstHeader\""));
            assert!(!before_xml.contains("<w:titlePg/>"));
            assert!(after_xml.contains("w:type=\"default\" r:id=\"rIdDefaultHeader\""));
            assert!(after_xml.contains("w:type=\"first\" r:id=\"rIdFirstHeader\""));
            assert!(after_xml.contains("<w:titlePg/>"));
            assert_eq!(
                before_xml,
                after_xml.replace("<w:titlePg/>", ""),
                "w:titlePg must be the only document XML change"
            );
        } else {
            assert_eq!(
                before_data, after_data,
                "package part must remain fixed: {name}"
            );
        }
    }
}

fn read_parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("valid DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("ZIP entry");
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read ZIP entry");
        assert!(
            parts.insert(name.clone(), data).is_none(),
            "duplicate part {name}"
        );
    }
    parts
}

fn write_parts(parts: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, data) in parts {
        writer.start_file(name, options).expect("start ZIP entry");
        writer.write_all(&data).expect("write ZIP entry");
    }
    writer.finish().expect("finish DOCX ZIP").into_inner()
}
