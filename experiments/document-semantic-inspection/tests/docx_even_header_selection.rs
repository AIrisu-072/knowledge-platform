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
fn default_only_header_is_accepted_and_its_text_is_semantic() {
    let before = docx_with_headers("Default header before", None);
    let after = docx_with_headers("Default header after", None);
    assert_settings_are_absent(&before);
    assert_settings_are_absent(&after);

    let before = inspect_docx(&before).expect("default-only header package is supported");
    let after = inspect_docx(&after).expect("default-only header package is supported");
    assert_ne!(
        fingerprint(&before.semantic_projection),
        fingerprint(&after.semantic_projection),
        "changing the referenced default header text must change semantic identity"
    );
}

#[test]
fn even_header_without_even_and_odd_setting_is_ignored_or_rejected_explicitly() {
    let before = docx_with_headers("Stable default header", Some("Even header before"));
    let after = docx_with_headers("Stable default header", Some("Even header after"));
    assert_settings_are_absent(&before);
    assert_settings_are_absent(&after);
    assert_only_header_text_differs(&before, &after, "word/header2.xml");

    match (inspect_docx(&before), inspect_docx(&after)) {
        (Ok(before), Ok(after)) => assert_eq!(
            fingerprint(&before.semantic_projection),
            fingerprint(&after.semantic_projection),
            "an even header is inactive when word/settings.xml is absent"
        ),
        (Err(before), Err(after))
            if before.code() == ErrorCode::UnsupportedSemanticConstruct
                && after.code() == ErrorCode::UnsupportedSemanticConstruct => {}
        (before, after) => panic!(
            "inactive even-header content must be ignored or explicitly rejected; before={before:?}, after={after:?}"
        ),
    }
}

fn inspect_docx(bytes: &[u8]) -> Result<AdapterOutput, PocError> {
    DocxAdapter.inspect(bytes, &InspectionProfile::default())
}

fn docx_with_headers(default_text: &str, even_text: Option<&str>) -> Vec<u8> {
    let mut parts = read_parts(&docx_fixture("same body text"));

    let document = String::from_utf8(parts["word/document.xml"].clone()).expect("document XML");
    let document = document.replacen(
        &format!("xmlns:w=\"{W_NS}\""),
        &format!("xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\""),
        1,
    );
    assert!(document.contains(&format!("xmlns:r=\"{R_NS}\"")));
    let mut section_references =
        "<w:headerReference w:type=\"default\" r:id=\"rIdDefaultHeader\"/>".to_owned();
    if even_text.is_some() {
        section_references.push_str("<w:headerReference w:type=\"even\" r:id=\"rIdEvenHeader\"/>");
    }
    let document = document.replace(
        "</w:body>",
        &format!("<w:sectPr>{section_references}</w:sectPr></w:body>"),
    );
    assert!(document.contains("w:type=\"default\""));
    assert_eq!(document.contains("w:type=\"even\""), even_text.is_some());
    parts.insert("word/document.xml".into(), document.into_bytes());

    let mut relationships = format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdDefaultHeader\" Type=\"{HEADER_REL}\" Target=\"header1.xml\"/>"
    );
    if even_text.is_some() {
        relationships.push_str(&format!(
            "<Relationship Id=\"rIdEvenHeader\" Type=\"{HEADER_REL}\" Target=\"header2.xml\"/>"
        ));
    }
    relationships.push_str("</Relationships>");
    parts.insert(
        "word/_rels/document.xml.rels".into(),
        relationships.into_bytes(),
    );

    let mut content_type_overrides =
        format!("<Override PartName=\"/word/header1.xml\" ContentType=\"{HEADER_CONTENT_TYPE}\"/>");
    if even_text.is_some() {
        content_type_overrides.push_str(&format!(
            "<Override PartName=\"/word/header2.xml\" ContentType=\"{HEADER_CONTENT_TYPE}\"/>"
        ));
    }
    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone())
        .expect("content types XML")
        .replace("</Types>", &format!("{content_type_overrides}</Types>"));
    parts.insert("[Content_Types].xml".into(), content_types.into_bytes());
    parts.insert(
        "word/header1.xml".into(),
        header_xml(default_text).into_bytes(),
    );
    if let Some(text) = even_text {
        parts.insert("word/header2.xml".into(), header_xml(text).into_bytes());
    }

    write_parts(parts)
}

fn header_xml(text: &str) -> String {
    format!("<w:hdr xmlns:w=\"{W_NS}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:hdr>")
}

fn assert_settings_are_absent(docx: &[u8]) {
    assert!(
        !read_parts(docx).contains_key("word/settings.xml"),
        "fixture must rely on the OOXML default for even-and-odd headers"
    );
}

fn assert_only_header_text_differs(before: &[u8], after: &[u8], changed_part: &str) {
    let before_parts = read_parts(before);
    let after_parts = read_parts(after);
    assert_eq!(
        before_parts.keys().collect::<Vec<_>>(),
        after_parts.keys().collect::<Vec<_>>()
    );

    for (name, before_data) in &before_parts {
        let after_data = &after_parts[name];
        if name == changed_part {
            let before_text = String::from_utf8(before_data.clone()).expect("before header XML");
            let after_text = String::from_utf8(after_data.clone()).expect("after header XML");
            assert!(before_text.contains("Even header before"));
            assert!(after_text.contains("Even header after"));
            assert_eq!(
                before_text.replace("Even header before", "EVEN_HEADER_TEXT"),
                after_text.replace("Even header after", "EVEN_HEADER_TEXT")
            );
        } else {
            assert_eq!(
                before_data, after_data,
                "unexpected package part change: {name}"
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
