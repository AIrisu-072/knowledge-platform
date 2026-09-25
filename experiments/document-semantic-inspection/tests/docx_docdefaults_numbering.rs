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
fn doc_defaults_numbering_level_text_changes_semantics_or_fails_closed() {
    let before = doc_defaults_numbered_docx("%1.");
    let after = doc_defaults_numbered_docx("item %1:");

    assert_doc_defaults_numbering_is_referenced(&before, "%1.");
    assert_doc_defaults_numbering_is_referenced(&after, "item %1:");
    assert_only_numbering_part_differs(&before, &after);
    assert_semantics_distinguish_or_fail_closed(
        &before,
        &after,
        "docDefaults w:lvlText visible marker",
    );
}

fn doc_defaults_numbered_docx(level_text: &str) -> Vec<u8> {
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
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{WORDPROCESSINGML}"><w:body><w:p><w:r><w:t>same default-numbered paragraph</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#
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
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{WORDPROCESSINGML}"><w:docDefaults><w:pPrDefault><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="42"/></w:numPr></w:pPr></w:pPrDefault><w:rPrDefault><w:rPr/></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>"#
        )
        .into_bytes(),
    );
    parts.insert(
        NUMBERING_PART.into(),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="{WORDPROCESSINGML}"><w:abstractNum w:abstractNumId="7"><w:multiLevelType w:val="singleLevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="{level_text}"/><w:lvlJc w:val="left"/></w:lvl></w:abstractNum><w:num w:numId="42"><w:abstractNumId w:val="7"/></w:num></w:numbering>"#
        )
        .into_bytes(),
    );

    write_parts(parts)
}

fn assert_doc_defaults_numbering_is_referenced(docx: &[u8], level_text: &str) {
    let parts = read_parts(docx);
    let document = std::str::from_utf8(&parts["word/document.xml"]).expect("document XML");
    let styles = std::str::from_utf8(&parts[STYLES_PART]).expect("styles XML");
    let relationships =
        std::str::from_utf8(&parts["word/_rels/document.xml.rels"]).expect("document rels XML");
    let content_types =
        std::str::from_utf8(&parts["[Content_Types].xml"]).expect("content types XML");
    let numbering = std::str::from_utf8(&parts[NUMBERING_PART]).expect("numbering XML");

    assert!(document.contains("same default-numbered paragraph"));
    assert!(
        !document.contains("<w:pPr"),
        "body paragraph must rely on document defaults"
    );
    assert!(
        !document.contains("<w:numPr") && !document.contains("<w:numId"),
        "body paragraph must not carry direct numbering"
    );
    assert!(styles.contains(
        "<w:docDefaults><w:pPrDefault><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"42\"/></w:numPr></w:pPr></w:pPrDefault>"
    ));
    assert!(relationships.contains(&format!(
        "Type=\"{OFFICE_RELATIONSHIPS}/styles\" Target=\"styles.xml\""
    )));
    assert!(relationships.contains(&format!(
        "Type=\"{OFFICE_RELATIONSHIPS}/numbering\" Target=\"numbering.xml\""
    )));
    assert!(content_types.contains("/word/styles.xml"));
    assert!(content_types.contains("/word/numbering.xml"));
    assert!(numbering.contains(
        "<w:abstractNum w:abstractNumId=\"7\"><w:multiLevelType w:val=\"singleLevel\"/><w:lvl w:ilvl=\"0\">"
    ));
    assert!(numbering.contains(&format!("<w:lvlText w:val=\"{level_text}\"/>")));
    assert!(numbering.contains("<w:num w:numId=\"42\"><w:abstractNumId w:val=\"7\"/></w:num>"));
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
