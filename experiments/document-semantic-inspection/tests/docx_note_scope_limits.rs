mod support;

use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile, PocError,
};
use support::ooxml::docx_fixture;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const PACKAGE_RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_RELATIONSHIPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WORDPROCESSINGML: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

const XML_NODE_LIMIT: usize = 2_000_000;
const XML_COMMENTS_PER_PART: usize = XML_NODE_LIMIT / 2 + 1;
const MAX_DOCX_ENTRY_BYTES: usize = 8 * 1024 * 1024;
const MAX_DOCX_TOTAL_BYTES: usize = 32 * 1024 * 1024;

#[test]
fn note_reference_inside_text_box_fails_closed() {
    let baseline =
        referenced_footnote_docx(r#"<w:p><w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#);
    inspect(&baseline).expect("ordinary body footnote reference is a valid baseline");

    let text_box = referenced_footnote_docx(
        r#"<w:p><w:r><w:pict><v:shape xmlns:v="urn:schemas-microsoft-com:vml"><v:textbox><w:txbxContent><w:p><w:r><w:footnoteReference w:id="1"/></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"#,
    );
    let error = require_rejection(
        inspect(&text_box),
        "text-box note reference must fail closed",
    );

    assert_eq!(
        error.code(),
        ErrorCode::UnsupportedSemanticConstruct,
        "{error}"
    );
    assert!(
        error
            .to_string()
            .contains("note reference inside a text box"),
        "unexpected rejection reason: {error}"
    );
}

#[test]
fn xml_comment_node_limit_is_global_across_package_parts() {
    let baseline = docx_fixture("same body text");
    inspect(&baseline).expect("small DOCX baseline is valid");

    let over_limit = docx_with_comments_across_parts();
    {
        let parts = read_parts(&over_limit);
        let uncompressed_bytes = parts.values().map(Vec::len).sum::<usize>();
        assert!(
            parts["[Content_Types].xml"].len() <= MAX_DOCX_ENTRY_BYTES,
            "content types entry exceeds the 8 MiB DOCX per-entry cap"
        );
        assert!(
            parts["word/document.xml"].len() <= MAX_DOCX_ENTRY_BYTES,
            "document entry exceeds the 8 MiB DOCX per-entry cap"
        );
        assert!(
            uncompressed_bytes <= MAX_DOCX_TOTAL_BYTES,
            "fixture exceeds the 32 MiB DOCX total uncompressed-byte cap"
        );
    }

    let error = require_rejection(
        inspect(&over_limit),
        "package-wide XML comment count exceeds the cap",
    );
    assert_eq!(
        error.code(),
        ErrorCode::InspectionResourceLimitExceeded,
        "{error}"
    );
}

fn referenced_footnote_docx(body_markup: &str) -> Vec<u8> {
    let mut parts = read_parts(&docx_fixture("same body text"));
    let document = String::from_utf8(parts["word/document.xml"].clone()).expect("document XML");
    let document = document.replace("</w:body>", &format!("{body_markup}</w:body>"));
    parts.insert("word/document.xml".into(), document.into_bytes());

    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone())
        .expect("content types XML")
        .replace(
            "</Types>",
            "<Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/></Types>",
        );
    parts.insert("[Content_Types].xml".into(), content_types.into_bytes());
    parts.insert(
        "word/_rels/document.xml.rels".into(),
        format!(
            r#"<Relationships xmlns="{PACKAGE_RELATIONSHIPS}"><Relationship Id="rIdNotes" Type="{OFFICE_RELATIONSHIPS}/footnotes" Target="footnotes.xml"/></Relationships>"#
        )
        .into_bytes(),
    );
    parts.insert(
        "word/footnotes.xml".into(),
        format!(
            r#"<w:footnotes xmlns:w="{WORDPROCESSINGML}"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>Alpha note</w:t></w:r></w:p></w:footnote></w:footnotes>"#
        )
        .into_bytes(),
    );
    write_parts(parts)
}

fn docx_with_comments_across_parts() -> Vec<u8> {
    let mut parts = read_parts(&docx_fixture("same body text"));
    insert_xml_comments_before_closing_tag(
        parts
            .get_mut("[Content_Types].xml")
            .expect("content types part"),
        "</Types>",
        XML_COMMENTS_PER_PART,
    );
    insert_xml_comments_before_closing_tag(
        parts.get_mut("word/document.xml").expect("document part"),
        "</w:body>",
        XML_COMMENTS_PER_PART,
    );
    write_parts(parts)
}

fn insert_xml_comments_before_closing_tag(xml: &mut Vec<u8>, closing_tag: &str, count: usize) {
    let closing_tag = closing_tag.as_bytes();
    let position = xml
        .windows(closing_tag.len())
        .rposition(|window| window == closing_tag)
        .expect("fixture XML closing tag");
    let comments = "<!---->".repeat(count);
    xml.splice(position..position, comments.into_bytes());
}

fn inspect(docx: &[u8]) -> Result<document_semantic_inspection_poc::AdapterOutput, PocError> {
    DocxAdapter.inspect(docx, &InspectionProfile::default())
}

fn require_rejection<T>(result: Result<T, PocError>, message: &str) -> PocError {
    match result {
        Ok(_) => panic!("{message}: adapter unexpectedly succeeded"),
        Err(error) => error,
    }
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
