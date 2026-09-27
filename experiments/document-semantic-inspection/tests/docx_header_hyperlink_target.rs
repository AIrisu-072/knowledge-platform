use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile,
    fingerprint as semantic_fingerprint,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const HEADER_PART: &str = "word/header1.xml";
const HEADER_RELS_PART: &str = "word/_rels/header1.xml.rels";
const TARGET_A: &str = "https://example.invalid/a";
const TARGET_B: &str = "https://example.invalid/b";

fn selected_header_hyperlink_docx() -> Vec<u8> {
    write_package(BTreeMap::from([
        (
            "[Content_Types].xml".into(),
            br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#.to_vec(),
        ),
        (
            "_rels/.rels".into(),
            br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec(),
        ),
        (
            "word/document.xml".into(),
            br#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>stable body text</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/></w:sectPr></w:body></w:document>"#.to_vec(),
        ),
        (
            "word/_rels/document.xml.rels".into(),
            br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#.to_vec(),
        ),
        (
            HEADER_PART.into(),
            br#"<?xml version="1.0" encoding="UTF-8"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:p><w:hyperlink r:id="rIdExternalLink"><w:r><w:t>Open link</w:t></w:r></w:hyperlink></w:p></w:hdr>"#.to_vec(),
        ),
        (
            HEADER_RELS_PART.into(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdExternalLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{TARGET_A}" TargetMode="External"/></Relationships>"#
            )
            .into_bytes(),
        ),
    ]))
}

fn with_hyperlink_target(input: &[u8], target: &str) -> Vec<u8> {
    let mut parts = read_package(input);
    let rels = String::from_utf8(parts[HEADER_RELS_PART].clone()).expect("header rels XML");
    assert_eq!(rels.matches(TARGET_A).count(), 1, "baseline target A");
    parts.insert(
        HEADER_RELS_PART.into(),
        rels.replace(TARGET_A, target).into_bytes(),
    );
    write_package(parts)
}

fn read_package(input: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("DOCX ZIP entry");
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("read DOCX part");
        assert!(
            parts.insert(name, bytes).is_none(),
            "unique DOCX part names"
        );
    }
    parts
}

fn write_package(parts: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, bytes) in parts {
        writer.start_file(name, options).expect("start DOCX part");
        writer.write_all(&bytes).expect("write DOCX part");
    }
    writer.finish().expect("finish DOCX ZIP").into_inner()
}

#[test]
fn selected_header_hyperlink_target_change_is_semantic_or_fails_closed() {
    let baseline = selected_header_hyperlink_docx();
    let changed = with_hyperlink_target(&baseline, TARGET_B);
    let baseline_parts = read_package(&baseline);
    let changed_parts = read_package(&changed);

    assert!(
        String::from_utf8_lossy(&baseline_parts["word/document.xml"])
            .contains("<w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/>")
    );
    assert!(
        String::from_utf8_lossy(&baseline_parts[HEADER_PART])
            .contains("<w:hyperlink r:id=\"rIdExternalLink\">")
    );
    assert!(
        String::from_utf8_lossy(&baseline_parts[HEADER_RELS_PART])
            .contains(&format!("Target=\"{TARGET_A}\" TargetMode=\"External\""))
    );
    assert_eq!(
        baseline_parts.keys().collect::<Vec<_>>(),
        changed_parts.keys().collect::<Vec<_>>()
    );
    let changed_parts_only = baseline_parts
        .iter()
        .filter_map(|(name, before)| (changed_parts[name] != *before).then_some(name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(changed_parts_only, [HEADER_RELS_PART]);

    let baseline_result = DocxAdapter.inspect(&baseline, &InspectionProfile::default());
    let changed_result = DocxAdapter.inspect(&changed, &InspectionProfile::default());

    match (baseline_result, changed_result) {
        (Ok(baseline), Ok(changed)) => assert_ne!(
            semantic_fingerprint(&baseline.semantic_projection),
            semantic_fingerprint(&changed.semantic_projection),
            "changing only the selected header hyperlink target must change the fingerprint"
        ),
        (Ok(_), Err(error)) => assert_eq!(
            error.code(),
            ErrorCode::UnsupportedSemanticConstruct,
            "an unhandled target change must fail closed"
        ),
        (Err(baseline_error), Err(changed_error)) => {
            assert_eq!(
                baseline_error.code(),
                ErrorCode::UnsupportedSemanticConstruct,
                "a rejected baseline must be rejected specifically as unsupported semantics"
            );
            assert_eq!(
                changed_error.code(),
                ErrorCode::UnsupportedSemanticConstruct,
                "the changed package must also fail closed"
            );
        }
        (Err(error), Ok(_)) => {
            panic!("changed header hyperlink was accepted although its baseline failed: {error}")
        }
    }
}
