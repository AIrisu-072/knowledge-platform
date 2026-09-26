use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile, PocError,
    fingerprint as semantic_fingerprint,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const WORDPROCESSINGML: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const PACKAGE_RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_RELATIONSHIPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NUMBERING_PART: &str = "word/numbering.xml";
const STYLES_PART: &str = "word/styles.xml";

#[test]
fn style_defined_list_level_text_changes_semantics_or_fails_closed() {
    let before = style_numbered_docx("%1.", "decimal");
    let after = style_numbered_docx("§%1)", "decimal");

    assert_style_numbering_is_referenced(&before, "%1.", "decimal");
    assert_style_numbering_is_referenced(&after, "§%1)", "decimal");
    assert_only_numbering_part_differs(&before, &after);
    assert_semantics_distinguish_or_fail_closed(
        &before,
        &after,
        "style-defined list w:lvlText visible marker",
    );
}

#[test]
fn style_defined_list_number_format_changes_semantics_or_fails_closed() {
    let before = style_numbered_docx("%1.", "decimal");
    let after = style_numbered_docx("%1.", "upperRoman");

    assert_style_numbering_is_referenced(&before, "%1.", "decimal");
    assert_style_numbering_is_referenced(&after, "%1.", "upperRoman");
    assert_only_numbering_part_differs(&before, &after);
    assert_semantics_distinguish_or_fail_closed(
        &before,
        &after,
        "style-defined list w:numFmt visible marker",
    );
}

fn style_numbered_docx(level_text: &str, number_format: &str) -> Vec<u8> {
    let mut parts = BTreeMap::from([
        (
            "[Content_Types].xml".into(),
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#.to_vec(),
        ),
        (
            "_rels/.rels".into(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{PACKAGE_RELATIONSHIPS}"><Relationship Id="rIdOffice" Type="{OFFICE_RELATIONSHIPS}/officeDocument" Target="word/document.xml"/></Relationships>"#
            )
            .into_bytes(),
        ),
        (
            "word/document.xml".into(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{WORDPROCESSINGML}"><w:body><w:p><w:pPr><w:pStyle w:val="ListItem"/></w:pPr><w:r><w:t>same style-defined list item text</w:t></w:r></w:p></w:body></w:document>"#
            )
            .into_bytes(),
        ),
    ]);

    parts.insert(
        "word/_rels/document.xml.rels".into(),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{PACKAGE_RELATIONSHIPS}"><Relationship Id="rIdStyles" Type="{OFFICE_RELATIONSHIPS}/styles" Target="styles.xml"/><Relationship Id="rIdNumbering" Type="{OFFICE_RELATIONSHIPS}/numbering" Target="numbering.xml"/></Relationships>"#
        )
        .into_bytes(),
    );
    parts.insert(
        STYLES_PART.into(),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{WORDPROCESSINGML}"><w:style w:type="paragraph" w:styleId="ListItem"><w:name w:val="List Item"/><w:pPr><w:numPr><w:numId w:val="42"/></w:numPr></w:pPr></w:style></w:styles>"#
        )
        .into_bytes(),
    );
    parts.insert(
        NUMBERING_PART.into(),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="{WORDPROCESSINGML}"><w:abstractNum w:abstractNumId="7"><w:multiLevelType w:val="singleLevel"/><w:lvl w:ilvl="0"><w:pStyle w:val="ListItem"/><w:start w:val="1"/><w:numFmt w:val="{number_format}"/><w:lvlText w:val="{level_text}"/><w:lvlJc w:val="left"/><w:pPr><w:tabs><w:tab w:val="num" w:pos="720"/></w:tabs><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="42"><w:abstractNumId w:val="7"/></w:num></w:numbering>"#
        )
        .into_bytes(),
    );

    write_parts(parts)
}

fn assert_style_numbering_is_referenced(docx: &[u8], level_text: &str, number_format: &str) {
    let parts = read_parts(docx);
    let document = std::str::from_utf8(&parts["word/document.xml"]).expect("document XML");
    let styles = std::str::from_utf8(&parts[STYLES_PART]).expect("styles XML");
    let relationships =
        std::str::from_utf8(&parts["word/_rels/document.xml.rels"]).expect("document rels XML");
    let content_types =
        std::str::from_utf8(&parts["[Content_Types].xml"]).expect("content types XML");
    let numbering = std::str::from_utf8(&parts[NUMBERING_PART]).expect("numbering XML");

    assert!(document.contains("<w:pStyle w:val=\"ListItem\"/>"));
    assert!(document.contains("same style-defined list item text"));
    assert!(
        !document.contains("<w:numPr"),
        "body paragraph must not have direct numbering"
    );
    assert!(
        !document.contains("<w:numId"),
        "body paragraph must not have direct numId"
    );
    assert!(styles.contains("<w:style w:type=\"paragraph\" w:styleId=\"ListItem\">"));
    assert!(styles.contains("<w:numPr><w:numId w:val=\"42\"/></w:numPr>"));
    assert!(relationships.contains(&format!(
        "Type=\"{OFFICE_RELATIONSHIPS}/styles\" Target=\"styles.xml\""
    )));
    assert!(relationships.contains(&format!(
        "Type=\"{OFFICE_RELATIONSHIPS}/numbering\" Target=\"numbering.xml\""
    )));
    assert!(content_types.contains("/word/styles.xml"));
    assert!(content_types.contains("/word/numbering.xml"));
    assert!(numbering.contains("<w:lvl w:ilvl=\"0\"><w:pStyle w:val=\"ListItem\"/>"));
    assert!(numbering.contains("<w:num w:numId=\"42\"><w:abstractNumId w:val=\"7\"/></w:num>"));
    assert!(numbering.contains(&format!("<w:numFmt w:val=\"{number_format}\"/>")));
    assert!(numbering.contains(&format!("<w:lvlText w:val=\"{level_text}\"/>")));
}

fn assert_only_numbering_part_differs(before: &[u8], after: &[u8]) {
    let before_parts = read_parts(before);
    let after_parts = read_parts(after);

    assert_eq!(
        before_parts.keys().collect::<Vec<_>>(),
        after_parts.keys().collect::<Vec<_>>()
    );
    for (name, before_part) in &before_parts {
        if name == NUMBERING_PART {
            assert_ne!(
                before_part, &after_parts[name],
                "numbering part must change"
            );
        } else {
            assert_eq!(
                before_part, &after_parts[name],
                "unexpected package change in {name}"
            );
        }
    }
}

fn assert_semantics_distinguish_or_fail_closed(before: &[u8], after: &[u8], context: &str) {
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
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read DOCX entry");
        assert!(parts.insert(file.name().to_owned(), data).is_none());
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
