mod support;

use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile, PocError,
    fingerprint as semantic_fingerprint,
};
use support::ooxml::docx_fixture;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const WORDPROCESSINGML: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const PACKAGE_RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_RELATIONSHIPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NUMBERING_PART: &str = "word/numbering.xml";

#[test]
fn new_numbering_instance_restarts_list_marker_or_fails_closed() {
    let continuing_instance = numbered_docx_with_second_instance(42);
    let restarted_instance = numbered_docx_with_second_instance(43);

    assert_numbering_fixture(&continuing_instance, [42, 42]);
    assert_numbering_fixture(&restarted_instance, [42, 43]);
    assert_only_second_paragraph_num_id_differs(&continuing_instance, &restarted_instance);

    assert_semantics_differ_or_both_fail_closed(
        &continuing_instance,
        &restarted_instance,
        "switching the second paragraph to a new numId instance",
    );
}

fn numbered_docx_with_second_instance(second_num_id: u32) -> Vec<u8> {
    let mut parts = read_parts(&docx_fixture("same list item body text"));
    let document = String::from_utf8(parts["word/document.xml"].clone()).expect("document XML");
    let original_body =
        "<w:body><w:p><w:r><w:t>same list item body text</w:t></w:r></w:p></w:body>";
    let numbered_body = format!(
        "<w:body>{}{}</w:body>",
        numbered_paragraph(42),
        numbered_paragraph(second_num_id)
    );
    let document = document.replacen(original_body, &numbered_body, 1);
    assert_ne!(
        document,
        String::from_utf8(parts["word/document.xml"].clone()).unwrap()
    );
    parts.insert("word/document.xml".into(), document.into_bytes());

    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone())
        .expect("content types XML")
        .replace(
            "</Types>",
            "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/></Types>",
        );
    parts.insert("[Content_Types].xml".into(), content_types.into_bytes());
    parts.insert(
        "word/_rels/document.xml.rels".into(),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{PACKAGE_RELATIONSHIPS}"><Relationship Id="rIdNumbering" Type="{OFFICE_RELATIONSHIPS}/numbering" Target="numbering.xml"/></Relationships>"#
        )
        .into_bytes(),
    );
    parts.insert(
        NUMBERING_PART.into(),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="{WORDPROCESSINGML}"><w:abstractNum w:abstractNumId="7"><w:multiLevelType w:val="singleLevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/></w:lvl></w:abstractNum><w:num w:numId="42"><w:abstractNumId w:val="7"/></w:num><w:num w:numId="43"><w:abstractNumId w:val="7"/></w:num></w:numbering>"#
        )
        .into_bytes(),
    );

    write_parts(parts)
}

fn numbered_paragraph(num_id: u32) -> String {
    format!(
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="{num_id}"/></w:numPr></w:pPr><w:r><w:t>same list item body text</w:t></w:r></w:p>"#
    )
}

fn assert_numbering_fixture(docx: &[u8], expected_num_ids: [u32; 2]) {
    let parts = read_parts(docx);
    let document = std::str::from_utf8(&parts["word/document.xml"]).expect("document XML");
    let relationships =
        std::str::from_utf8(&parts["word/_rels/document.xml.rels"]).expect("document rels XML");
    let content_types =
        std::str::from_utf8(&parts["[Content_Types].xml"]).expect("content types XML");
    let numbering = std::str::from_utf8(&parts[NUMBERING_PART]).expect("numbering XML");

    assert_eq!(document.matches("<w:p>").count(), 2);
    assert_eq!(document.matches("same list item body text").count(), 2);
    assert_eq!(document.matches("<w:numId ").count(), 2);
    for num_id in [42, 43] {
        let reference_count = expected_num_ids
            .iter()
            .filter(|expected| **expected == num_id)
            .count();
        assert_eq!(
            document
                .matches(&format!("<w:numId w:val=\"{num_id}\"/>"))
                .count(),
            reference_count,
            "unexpected document references for numId {num_id}"
        );
    }
    assert!(relationships.contains(&format!(
        "Type=\"{OFFICE_RELATIONSHIPS}/numbering\" Target=\"numbering.xml\""
    )));
    assert!(content_types.contains("/word/numbering.xml"));
    assert_eq!(numbering.matches("<w:abstractNum ").count(), 1);
    assert_eq!(numbering.matches("<w:num ").count(), 2);
    assert!(numbering.contains("<w:num w:numId=\"42\"><w:abstractNumId w:val=\"7\"/></w:num>"));
    assert!(numbering.contains("<w:num w:numId=\"43\"><w:abstractNumId w:val=\"7\"/></w:num>"));
}

fn assert_only_second_paragraph_num_id_differs(before: &[u8], after: &[u8]) {
    let before_parts = read_parts(before);
    let after_parts = read_parts(after);

    assert_eq!(
        before_parts.keys().collect::<Vec<_>>(),
        after_parts.keys().collect::<Vec<_>>()
    );
    for (name, before_part) in &before_parts {
        if name == "word/document.xml" {
            let before_document = std::str::from_utf8(before_part).expect("baseline document XML");
            let after_document =
                std::str::from_utf8(&after_parts[name]).expect("variant document XML");
            let expected_after = before_document.replacen(
                &format!("{}</w:body></w:document>", numbered_paragraph(42)),
                &format!("{}</w:body></w:document>", numbered_paragraph(43)),
                1,
            );
            assert_ne!(before_document, after_document, "second numId must change");
            assert_eq!(after_document, expected_after);
        } else {
            assert_eq!(
                before_part, &after_parts[name],
                "unexpected change in {name}"
            );
        }
    }
}

fn assert_semantics_differ_or_both_fail_closed(before: &[u8], after: &[u8], context: &str) {
    let before_result = inspect_docx(before);
    let after_result = inspect_docx(after);
    match (before_result, after_result) {
        (Ok(before), Ok(after)) => assert_ne!(
            semantic_fingerprint(&before.semantic_projection),
            semantic_fingerprint(&after.semantic_projection),
            "{context} changed while the semantic fingerprint stayed equal"
        ),
        (Err(before), Err(after))
            if before.code() == ErrorCode::UnsupportedSemanticConstruct
                && after.code() == ErrorCode::UnsupportedSemanticConstruct => {}
        (before, after) => panic!(
            "{context} packages must either differ semantically or both fail closed as UnsupportedSemanticConstruct; before={:?}, after={:?}",
            error_summary(before),
            error_summary(after)
        ),
    }
}

fn inspect_docx(bytes: &[u8]) -> Result<document_semantic_inspection_poc::AdapterOutput, PocError> {
    DocxAdapter.inspect(bytes, &InspectionProfile::default())
}

fn error_summary<T>(result: Result<T, PocError>) -> Result<(), (ErrorCode, String)> {
    result
        .map(|_| ())
        .map_err(|error| (error.code(), error.to_string()))
}

fn read_parts(docx: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(docx)).expect("DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("DOCX entry");
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read DOCX entry");
        assert!(
            parts.insert(name, data).is_none(),
            "duplicate fixture entry"
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
        writer.start_file(name, options).expect("start DOCX entry");
        writer.write_all(&data).expect("write DOCX entry");
    }
    writer.finish().expect("finish DOCX").into_inner()
}
