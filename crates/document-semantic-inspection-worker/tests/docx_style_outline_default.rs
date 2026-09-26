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
const WORD_STYLES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";

const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

#[test]
fn outline_level_nine_is_equivalent_to_an_omitted_outline_level() {
    let omitted = inspect_valid(&styled_docx(None, None, false));
    let explicit_nine = inspect_valid(&styled_docx(None, Some(9), false));

    assert_eq!(
        omitted.semantic_fingerprint(),
        explicit_nine.semantic_fingerprint(),
        "outlineLvl 9 means no outline level and must match omission for the same paragraph style and text",
    );
}

#[test]
fn explicit_outline_level_nine_overrides_an_inherited_heading_or_fails_closed() {
    let inherited_heading = inspect_valid(&styled_docx(Some(0), None, true));
    let child_without_outline = styled_docx(Some(0), Some(9), true);

    match inspect(&child_without_outline) {
        Ok(explicit_no_outline) => assert_ne!(
            inherited_heading.semantic_fingerprint(),
            explicit_no_outline.semantic_fingerprint(),
            "an explicit child outlineLvl 9 must override the parent's outline level 0",
        ),
        Err(error) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "an unsupported explicit child outline level must fail closed",
        ),
    }
}

#[test]
fn outline_levels_zero_and_one_have_distinct_fingerprints() {
    let level_zero = inspect_valid(&styled_docx(None, Some(0), false));
    let level_one = inspect_valid(&styled_docx(None, Some(1), false));

    assert_ne!(
        level_zero.semantic_fingerprint(),
        level_one.semantic_fingerprint(),
        "different effective heading levels must change semantic identity",
    );
}

fn inspect_valid(bytes: &[u8]) -> SemanticAdapterOutput {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic DOCX passes the OOXML coverage sentinel");
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("synthetic DOCX is accepted by the DOCX adapter")
}

fn inspect(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)?;
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn styled_docx(
    parent_outline: Option<u8>,
    child_outline: Option<u8>,
    based_on_parent: bool,
) -> Vec<u8> {
    let parent = if based_on_parent {
        format!(
            r#"<w:style w:type="paragraph" w:styleId="ParentHeading">{}</w:style>"#,
            outline_properties(parent_outline)
        )
    } else {
        String::new()
    };
    let based_on = if based_on_parent {
        r#"<w:basedOn w:val="ParentHeading"/>"#
    } else {
        ""
    };
    let child = format!(
        r#"<w:style w:type="paragraph" w:styleId="BodyHeading">{based_on}{}</w:style>"#,
        outline_properties(child_outline)
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{WORD_NS}">{parent}{child}</w:styles>"#
    );
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}"><w:body><w:p><w:pPr><w:pStyle w:val="BodyHeading"/></w:pPr><w:r><w:t>Same visible paragraph text</w:t></w:r></w:p></w:body></w:document>"#
    );
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="{WORD_MAIN}"/><Override PartName="/word/styles.xml" ContentType="{WORD_STYLES}"/></Types>"#
    );
    let document_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdStyles" Type="{REL_NS}/styles" Target="styles.xml"/></Relationships>"#
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
    ])
}

fn outline_properties(level: Option<u8>) -> String {
    level.map_or_else(String::new, |value| {
        format!(r#"<w:pPr><w:outlineLvl w:val="{value}"/></w:pPr>"#)
    })
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
