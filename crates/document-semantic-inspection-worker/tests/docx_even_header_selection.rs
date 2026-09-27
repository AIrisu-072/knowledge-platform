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
fn even_header_text_is_noise_without_settings_even_and_odd_headers() {
    let original = inspect(&header_docx("DEFAULT ALPHA", Some("EVEN ALPHA")));
    let changed_even_header = inspect(&header_docx("DEFAULT ALPHA", Some("EVEN BRAVO")));

    assert_same_fingerprint_or_unsupported(&original, &changed_even_header);
}

#[test]
fn default_header_only_baseline_succeeds_and_default_header_changes_identity() {
    let original = inspect(&header_docx("DEFAULT ALPHA", None))
        .expect("a valid DOCX with only a default header and no settings part is supported");
    let changed_default_header = inspect(&header_docx("DEFAULT BRAVO", None))
        .expect("a valid DOCX with only a default header and no settings part is supported");

    assert_ne!(
        original.semantic_fingerprint(),
        changed_default_header.semantic_fingerprint(),
        "changing the selected default header must change semantic identity",
    );
}

fn inspect(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)?;
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn assert_same_fingerprint_or_unsupported(
    original: &Result<SemanticAdapterOutput, WorkerFailure>,
    changed: &Result<SemanticAdapterOutput, WorkerFailure>,
) {
    match (original, changed) {
        (Ok(original), Ok(changed)) => assert_eq!(
            original.semantic_fingerprint(),
            changed.semantic_fingerprint(),
            "an even header is not selected when settings.xml is absent",
        ),
        (Err(original), Err(changed)) => {
            assert_eq!(
                original.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "an unmodeled even-header selection rule must fail closed",
            );
            assert_eq!(
                changed.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "an unmodeled even-header selection rule must fail closed",
            );
        }
        _ => panic!(
            "both even-header variants must either be supported or fail closed as unsupported"
        ),
    }
}

fn header_docx(default_header_text: &str, even_header_text: Option<&str>) -> Vec<u8> {
    let even_reference = if even_header_text.is_some() {
        r#"<w:headerReference w:type="even" r:id="rIdEvenHeader"/>"#
    } else {
        ""
    };
    let even_relationship = if even_header_text.is_some() {
        format!(r#"<Relationship Id="rIdEvenHeader" Type="{REL_NS}/header" Target="header2.xml"/>"#)
    } else {
        String::new()
    };
    let even_header_content_type = if even_header_text.is_some() {
        r#"<Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>"#
    } else {
        ""
    };
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body>
<w:p><w:r><w:t>Stable body text</w:t></w:r></w:p>
<w:sectPr>
<w:headerReference w:type="default" r:id="rIdDefaultHeader"/>
{even_reference}
<w:pgSz w:w="12240" w:h="15840"/>
</w:sectPr>
</w:body></w:document>"#
    );
    let relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_REL_NS}">
<Relationship Id="rIdDefaultHeader" Type="{REL_NS}/header" Target="header1.xml"/>
{even_relationship}
</Relationships>"#
    );
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{CONTENT_TYPES_NS}">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
{even_header_content_type}
</Types>"#
    );

    let mut parts = vec![
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", ROOT_RELATIONSHIPS.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", relationships.into_bytes()),
        ("word/header1.xml", header_xml(default_header_text)),
    ];
    if let Some(even_header_text) = even_header_text {
        parts.push(("word/header2.xml", header_xml(even_header_text)));
    }

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

fn header_xml(text: &str) -> Vec<u8> {
    format!(r#"<w:hdr xmlns:w="{WORD_NS}"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:hdr>"#)
        .into_bytes()
}
