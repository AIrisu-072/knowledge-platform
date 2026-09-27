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
const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

#[test]
fn accepts_a_valid_referenced_title_page_header_section() {
    inspect(&title_page_docx(true, "Cover header"))
        .expect("a valid title-page section with referenced first/default headers is supported");
}

#[test]
fn toggling_title_page_header_selection_changes_identity_or_fails_closed() {
    let title_page_enabled = inspect(&title_page_docx(true, "Cover header"));
    let title_page_disabled = inspect(&title_page_docx(false, "Cover header"));

    assert_only_unsupported_failures(&[&title_page_enabled, &title_page_disabled]);
    if let (Ok(enabled), Ok(disabled)) = (&title_page_enabled, &title_page_disabled) {
        assert_ne!(
            enabled.semantic_fingerprint(),
            disabled.semantic_fingerprint(),
            "w:titlePg selects a different visible header for the first page",
        );
    }
}

#[test]
fn changing_an_unselected_first_page_header_is_noise_or_fails_closed() {
    let original = inspect(&title_page_docx(false, "Unused first header"));
    let changed_unselected_header = inspect(&title_page_docx(false, "Different unused header"));

    assert_only_unsupported_failures(&[&original, &changed_unselected_header]);
    if let (Ok(original), Ok(changed)) = (&original, &changed_unselected_header) {
        assert_eq!(
            original.semantic_fingerprint(),
            changed.semantic_fingerprint(),
            "a first-page header is not selected when w:titlePg is absent",
        );
    }
}

fn inspect(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)?;
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn assert_only_unsupported_failures(outcomes: &[&Result<SemanticAdapterOutput, WorkerFailure>]) {
    for outcome in outcomes {
        if let Err(error) = outcome {
            assert_eq!(
                error.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "an unmodeled title-page header rule must fail closed as unsupported",
            );
        }
    }
}

fn title_page_docx(title_page_enabled: bool, first_header_text: &str) -> Vec<u8> {
    let title_page = if title_page_enabled {
        "<w:titlePg/>"
    } else {
        ""
    };
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body>
<w:p><w:r><w:t>Stable body text</w:t></w:r></w:p>
<w:sectPr>
<w:headerReference w:type="default" r:id="rIdDefaultHeader"/>
<w:headerReference w:type="first" r:id="rIdFirstHeader"/>
<w:pgSz w:w="12240" w:h="15840"/>
<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/>
{title_page}
</w:sectPr>
</w:body></w:document>"#
    );
    let relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_REL_NS}">
<Relationship Id="rIdDefaultHeader" Type="{REL_NS}/header" Target="header1.xml"/>
<Relationship Id="rIdFirstHeader" Type="{REL_NS}/header" Target="header2.xml"/>
</Relationships>"#
    );
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{CONTENT_TYPES_NS}">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
<Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
</Types>"#
    );
    let default_header = format!(
        r#"<w:hdr xmlns:w="{WORD_NS}"><w:p><w:r><w:t>Running header</w:t></w:r></w:p></w:hdr>"#
    );
    let first_header = format!(
        r#"<w:hdr xmlns:w="{WORD_NS}"><w:p><w:r><w:t>{first_header_text}</w:t></w:r></w:p></w:hdr>"#
    );

    let parts = [
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", ROOT_RELATIONSHIPS.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", relationships.into_bytes()),
        ("word/header1.xml", default_header.into_bytes()),
        ("word/header2.xml", first_header.into_bytes()),
    ];
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
