use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
    WorkerFailure, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

#[test]
fn title_page_selection_changes_first_header_identity_or_fails_closed() {
    let default_page = docx_with_first_and_default_headers(false);
    let title_page = docx_with_first_and_default_headers(true);
    assert_same_parts_except_document_xml(&default_page, &title_page);

    let default_page_output = inspect(&default_page)
        .expect("valid first/default header references are supported without w:titlePg");
    match inspect(&title_page) {
        Ok(title_page_output) => assert_ne!(
            default_page_output.semantic_fingerprint(),
            title_page_output.semantic_fingerprint(),
            "w:titlePg selects the first-page header while all referenced parts stay identical",
        ),
        Err(error) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "unsupported first-page header selection must fail closed",
        ),
    }
}

fn inspect(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)?;
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn assert_same_parts_except_document_xml(left: &[u8], right: &[u8]) {
    let left_parts = zip_parts(left);
    let right_parts = zip_parts(right);
    assert_eq!(
        left_parts.keys().collect::<Vec<_>>(),
        right_parts.keys().collect::<Vec<_>>(),
        "title-page selection does not add or remove package parts",
    );
    for (name, left_contents) in &left_parts {
        if name != "word/document.xml" {
            assert_eq!(
                left_contents, &right_parts[name],
                "title-page selection leaves {name} unchanged",
            );
        }
    }
}

fn zip_parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open synthetic DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("read synthetic DOCX part");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("read synthetic DOCX part contents");
        assert!(
            parts.insert(name.clone(), contents).is_none(),
            "synthetic DOCX contains one entry named {name}",
        );
    }
    parts
}

fn docx_with_first_and_default_headers(title_page_enabled: bool) -> Vec<u8> {
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
    let parts = [
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", ROOT_RELATIONSHIPS.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", relationships.into_bytes()),
        ("word/header1.xml", header_xml("Running header")),
        ("word/header2.xml", header_xml("First-page header")),
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

fn header_xml(text: &str) -> Vec<u8> {
    format!(r#"<w:hdr xmlns:w="{WORD_NS}"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:hdr>"#)
        .into_bytes()
}
