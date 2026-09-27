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
const STYLES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const NUMBERING_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const ROOT_RELATIONSHIPS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

#[test]
fn style_defined_list_level_text_changes_semantic_identity_or_fails_closed() {
    let filled_bullet = style_numbered_docx("bullet", "•");
    let hollow_bullet = style_numbered_docx("bullet", "◦");
    assert_only_numbering_part_changes(&filled_bullet, &hollow_bullet);
    assert_style_defined_numbering(&filled_bullet);
    assert_style_defined_numbering(&hollow_bullet);

    assert_numbering_change_is_semantic_or_unsupported(
        &filled_bullet,
        &hollow_bullet,
        "a style-defined list marker template change",
    );
}

#[test]
fn style_defined_list_number_format_changes_semantic_identity_or_fails_closed() {
    let decimal = style_numbered_docx("decimal", "%1.");
    let lower_letter = style_numbered_docx("lowerLetter", "%1.");
    assert_only_numbering_part_changes(&decimal, &lower_letter);
    assert_style_defined_numbering(&decimal);
    assert_style_defined_numbering(&lower_letter);

    assert_numbering_change_is_semantic_or_unsupported(
        &decimal,
        &lower_letter,
        "a style-defined list number format change",
    );
}

fn assert_numbering_change_is_semantic_or_unsupported(
    original: &[u8],
    changed: &[u8],
    description: &str,
) {
    let original_output = inspect_sentinel_valid(original);
    let changed_output = inspect_sentinel_valid(changed);

    match (original_output, changed_output) {
        (Ok(original), Ok(changed)) => assert_ne!(
            original.semantic_fingerprint(),
            changed.semantic_fingerprint(),
            "{description} must change semantic identity",
        ),
        (Err(original), Err(changed)) => {
            assert_eq!(
                original.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the original {description} package must be explicitly unsupported",
            );
            assert_eq!(
                changed.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the changed {description} package must be explicitly unsupported",
            );
        }
        (original, changed) => panic!(
            "{description} packages must both be distinguished or both fail explicitly: {original:?}, {changed:?}"
        ),
    }
}

fn inspect_sentinel_valid(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic style-defined numbering DOCX passes the OOXML coverage sentinel");
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn assert_style_defined_numbering(bytes: &[u8]) {
    let parts = package_parts(bytes);
    let document =
        std::str::from_utf8(&parts["word/document.xml"]).expect("synthetic document XML is UTF-8");
    let styles = std::str::from_utf8(&parts["word/styles.xml"]).expect("styles XML is UTF-8");

    assert!(
        document.contains("<w:pStyle w:val=\"ListItem\"/>") && !document.contains("<w:numPr"),
        "the body paragraph must reference the list style without direct numbering properties",
    );
    assert!(
        styles.contains("<w:numPr>")
            && styles.contains("<w:numId w:val=\"1\"/>")
            && parts["word/numbering.xml"]
                .windows(b"<w:pStyle w:val=\"ListItem\"/>".len())
                .any(|window| window == b"<w:pStyle w:val=\"ListItem\"/>"),
        "the referenced paragraph style must define the list numId",
    );
}

fn style_numbered_docx(number_format: &str, level_text: &str) -> Vec<u8> {
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="{MAIN_CONTENT_TYPE}"/><Override PartName="/word/styles.xml" ContentType="{STYLES_CONTENT_TYPE}"/><Override PartName="/word/numbering.xml" ContentType="{NUMBERING_CONTENT_TYPE}"/></Types>"#
    );
    let document = format!(
        r#"<w:document xmlns:w="{WORD_NS}"><w:body><w:p><w:pPr><w:pStyle w:val="ListItem"/></w:pPr><w:r><w:t>Same visible list item</w:t></w:r></w:p></w:body></w:document>"#
    );
    let styles = format!(
        r#"<w:styles xmlns:w="{WORD_NS}"><w:style w:type="paragraph" w:styleId="ListItem"><w:name w:val="List Item"/><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr></w:style></w:styles>"#
    );
    let document_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdStyles" Type="{OFFICE_REL_NS}/styles" Target="styles.xml"/><Relationship Id="rIdNumbering" Type="{OFFICE_REL_NS}/numbering" Target="numbering.xml"/></Relationships>"#
    );
    let numbering = format!(
        r#"<w:numbering xmlns:w="{WORD_NS}"><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="singleLevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="{number_format}"/><w:pStyle w:val="ListItem"/><w:lvlText w:val="{level_text}"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );

    stored_docx(vec![
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", ROOT_RELATIONSHIPS.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        (
            "word/_rels/document.xml.rels",
            document_relationships.into_bytes(),
        ),
        ("word/styles.xml", styles.into_bytes()),
        ("word/numbering.xml", numbering.into_bytes()),
    ])
}

fn assert_only_numbering_part_changes(original: &[u8], changed: &[u8]) {
    let original_parts = package_parts(original);
    let changed_parts = package_parts(changed);
    assert_eq!(
        original_parts.keys().collect::<Vec<_>>(),
        changed_parts.keys().collect::<Vec<_>>()
    );
    assert!(original_parts.contains_key("word/numbering.xml"));

    for (name, original_contents) in &original_parts {
        let changed_contents = &changed_parts[name];
        if name == "word/numbering.xml" {
            assert_ne!(original_contents, changed_contents);
        } else {
            assert_eq!(
                original_contents, changed_contents,
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
