use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const DOCX_ENTRY_LIMIT: usize = 8 * 1024 * 1024;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const EMPTY_DOCUMENT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

const EMPTY_DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>known document text</w:t></w:r></w:p></w:body>
</w:document>"#;

fn assert_rejected_by_sentinel_and_adapter(bytes: &[u8], expected: WorkerFailureCode) {
    let sentinel_error = OoxmlCoverageSentinel::validate_package(bytes)
        .expect_err("hostile or unrecognized OOXML must fail the coverage sentinel");
    assert_eq!(sentinel_error.code(), expected, "{sentinel_error}");

    let adapter_error = DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect_err("hostile or unrecognized OOXML must fail the DOCX adapter");
    assert_eq!(adapter_error.code(), expected, "{adapter_error}");
}

#[test]
fn deflated_entry_over_eight_mib_fails_with_resource_limit() {
    let oversized_text = "x".repeat(DOCX_ENTRY_LIMIT + 1);
    let document = document_with_body(&format!(
        "<w:p><w:r><w:t>{oversized_text}</w:t></w:r></w:p>"
    ));
    let bytes = package_with_document(
        &document,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        Vec::new(),
        CompressionMethod::Deflated,
    );

    let mut archive = ZipArchive::new(Cursor::new(bytes.as_slice())).expect("synthetic DOCX ZIP");
    let document_entry = archive
        .by_name("word/document.xml")
        .expect("synthetic document part");
    assert!(document_entry.size() as usize > DOCX_ENTRY_LIMIT);
    assert!(document_entry.compressed_size() < document_entry.size());

    assert_rejected_by_sentinel_and_adapter(
        &bytes,
        WorkerFailureCode::InspectionResourceLimitExceeded,
    );
}

#[test]
fn duplicate_zip_entry_fails_as_malformed_package() {
    let base = minimal_docx();
    let document = read_part(&base, "word/document.xml");
    let mut parts = read_parts(&base);
    parts.push(("word/documenx.xml".to_owned(), document));
    let bytes = patch_entry_name(
        &write_parts(parts, CompressionMethod::Stored),
        b"word/documenx.xml",
        b"word/document.xml",
    );

    let matching_entries = count_central_directory_entries(&bytes, b"word/document.xml");
    assert_eq!(
        matching_entries, 2,
        "fixture central directory must contain the exact part name twice"
    );

    assert_rejected_by_sentinel_and_adapter(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn traversal_zip_part_fails_as_malformed_package() {
    let mut parts = read_parts(&minimal_docx());
    parts.push(("../escape.xml".to_owned(), b"<escape/>".to_vec()));
    let bytes = write_parts(parts, CompressionMethod::Stored);
    assert!(
        contains_bytes(&bytes, b"../escape.xml"),
        "fixture must retain the raw traversal name"
    );

    assert_rejected_by_sentinel_and_adapter(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn traversal_relationship_target_fails_as_malformed_package() {
    let relationships = relationships_with(
        r#"<Relationship Id="rIdEscape" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="../../escape.xml"/>"#,
    );
    let bytes = package_with_document(
        EMPTY_DOCUMENT,
        &relationships,
        Vec::new(),
        CompressionMethod::Stored,
    );
    assert!(
        contains_bytes(&bytes, b"Target=\"../../escape.xml\""),
        "relationship target must escape above the package root; root-level escape.xml is a decoy"
    );

    assert_rejected_by_sentinel_and_adapter(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn dangling_internal_relationship_target_fails_as_malformed_package() {
    let content_types = content_types_with_one_header("missing-header.xml");
    let relationships = relationships_with(
        r#"<Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="missing-header.xml"/>"#,
    );
    let bytes = package_with_parts(
        &content_types,
        EMPTY_DOCUMENT,
        &relationships,
        Vec::new(),
        CompressionMethod::Stored,
    );

    assert_rejected_by_sentinel_and_adapter(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn office_document_relationship_targeting_header_fails_as_malformed_package() {
    let content_types = content_types_with_one_header("header1.xml");
    let root_relationships = relationships_with(
        r#"<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/header1.xml"/>"#,
    );
    let mut parts = read_parts(&minimal_docx());
    for (name, contents) in &mut parts {
        match name.as_str() {
            "[Content_Types].xml" => *contents = content_types.as_bytes().to_vec(),
            "_rels/.rels" => *contents = root_relationships.as_bytes().to_vec(),
            _ => {}
        }
    }
    parts.push((
        "word/header1.xml".to_owned(),
        HEADER_XML.as_bytes().to_vec(),
    ));
    let bytes = write_parts(parts, CompressionMethod::Stored);

    assert_rejected_by_sentinel_and_adapter(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn unknown_generic_xml_part_under_word_fails_as_unsupported() {
    let bytes = package_with_document(
        EMPTY_DOCUMENT,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        vec![(
            "word/custom.xml".to_owned(),
            br#"<?xml version="1.0"?><customMeaning><value>not covered</value></customMeaning>"#
                .to_vec(),
        )],
        CompressionMethod::Stored,
    );

    assert_rejected_by_sentinel_and_adapter(
        &bytes,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn unknown_wordprocessingml_qname_fails_as_unsupported() {
    let document = document_with_body(
        r#"<w:p><w:r><w:t>known text</w:t></w:r><w:futureSemanticElement w:val="meaning"/></w:p>"#,
    );
    let bytes = package_with_document(
        &document,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        Vec::new(),
        CompressionMethod::Stored,
    );

    assert_rejected_by_sentinel_and_adapter(
        &bytes,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn unknown_relationship_in_reachable_secondary_rels_fails_as_unsupported() {
    let content_types = content_types_with_headers();
    let document = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<w:body><w:p/><w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/></w:sectPr></w:body>
</w:document>"#;
    let document_relationships = relationships_with(
        r#"<Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>"#,
    );
    let header_relationships = relationships_with(
        r#"<Relationship Id="rIdUnknown" Type="urn:example:relationships/semantic" Target="header2.xml"/>"#,
    );
    let bytes = package_with_parts(
        &content_types,
        document,
        &document_relationships,
        vec![
            (
                "word/header1.xml".to_owned(),
                HEADER_XML.as_bytes().to_vec(),
            ),
            (
                "word/_rels/header1.xml.rels".to_owned(),
                header_relationships.into_bytes(),
            ),
            (
                "word/header2.xml".to_owned(),
                HEADER_XML.as_bytes().to_vec(),
            ),
        ],
        CompressionMethod::Stored,
    );

    assert_rejected_by_sentinel_and_adapter(
        &bytes,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

const HEADER_XML: &str = r#"<?xml version="1.0"?>
<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p/></w:hdr>"#;

fn minimal_docx() -> Vec<u8> {
    package_with_document(
        EMPTY_DOCUMENT,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        Vec::new(),
        CompressionMethod::Stored,
    )
}

fn package_with_document(
    document_xml: &str,
    document_relationships_xml: &str,
    extras: Vec<(String, Vec<u8>)>,
    compression: CompressionMethod,
) -> Vec<u8> {
    package_with_parts(
        CONTENT_TYPES,
        document_xml,
        document_relationships_xml,
        extras,
        compression,
    )
}

fn package_with_parts(
    content_types_xml: &str,
    document_xml: &str,
    document_relationships_xml: &str,
    extras: Vec<(String, Vec<u8>)>,
    compression: CompressionMethod,
) -> Vec<u8> {
    let mut parts = vec![
        (
            "[Content_Types].xml".to_owned(),
            content_types_xml.as_bytes().to_vec(),
        ),
        (
            "_rels/.rels".to_owned(),
            ROOT_RELATIONSHIPS.as_bytes().to_vec(),
        ),
        (
            "word/document.xml".to_owned(),
            document_xml.as_bytes().to_vec(),
        ),
        (
            "word/_rels/document.xml.rels".to_owned(),
            document_relationships_xml.as_bytes().to_vec(),
        ),
    ];
    parts.extend(extras);
    write_parts(parts, compression)
}

fn document_with_body(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    )
}

fn relationships_with(relationships: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationships}</Relationships>"#
    )
}

fn content_types_with_headers() -> String {
    CONTENT_TYPES.replace(
        "</Types>",
        r#"<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#,
    )
}

fn content_types_with_one_header(part_name: &str) -> String {
    let header_override = format!(
        r#"<Override PartName="/word/{part_name}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>"#
    );
    CONTENT_TYPES.replace("</Types>", &format!("{header_override}</Types>"))
}

fn read_parts(input: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("synthetic DOCX ZIP");
    let mut parts = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("synthetic ZIP entry");
        let name = entry.name().to_owned();
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .expect("read synthetic ZIP entry");
        parts.push((name, contents));
    }
    parts
}

fn read_part(input: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("synthetic DOCX ZIP");
    let mut entry = archive.by_name(name).expect("synthetic DOCX part");
    let mut contents = Vec::new();
    entry
        .read_to_end(&mut contents)
        .expect("read synthetic DOCX part");
    contents
}

fn write_parts(parts: Vec<(String, Vec<u8>)>, compression: CompressionMethod) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(compression)
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

fn patch_entry_name(input: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    assert_eq!(
        from.len(),
        to.len(),
        "fixture aliases must have equal lengths"
    );
    let mut output = input.to_vec();
    let mut replacements = 0;
    let mut offset = 0;
    while offset + from.len() <= output.len() {
        if &output[offset..offset + from.len()] == from {
            output[offset..offset + to.len()].copy_from_slice(to);
            replacements += 1;
            offset += from.len();
        } else {
            offset += 1;
        }
    }
    assert!(
        replacements >= 2,
        "ZIP name must appear in local and central headers"
    );
    output
}

fn count_central_directory_entries(input: &[u8], expected_name: &[u8]) -> usize {
    const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
    const CENTRAL_HEADER_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
    const EOCD_SIZE: usize = 22;
    const CENTRAL_HEADER_SIZE: usize = 46;

    let eocd_offset = input
        .windows(4)
        .enumerate()
        .rev()
        .find_map(|(offset, signature)| {
            if signature != EOCD_SIGNATURE || offset + EOCD_SIZE > input.len() {
                return None;
            }
            let comment_len = u16::from_le_bytes([input[offset + 20], input[offset + 21]]) as usize;
            (offset + EOCD_SIZE + comment_len == input.len()).then_some(offset)
        })
        .expect("synthetic ZIP end-of-central-directory record");

    let entry_count =
        u16::from_le_bytes([input[eocd_offset + 10], input[eocd_offset + 11]]) as usize;
    let central_size = u32::from_le_bytes([
        input[eocd_offset + 12],
        input[eocd_offset + 13],
        input[eocd_offset + 14],
        input[eocd_offset + 15],
    ]) as usize;
    let mut cursor = u32::from_le_bytes([
        input[eocd_offset + 16],
        input[eocd_offset + 17],
        input[eocd_offset + 18],
        input[eocd_offset + 19],
    ]) as usize;
    let central_end = cursor
        .checked_add(central_size)
        .expect("central directory end offset");
    assert!(
        central_end <= eocd_offset,
        "central directory must precede EOCD"
    );

    let mut matching_entries = 0;
    for _ in 0..entry_count {
        assert_eq!(
            &input[cursor..cursor + 4],
            CENTRAL_HEADER_SIGNATURE,
            "central directory entry signature"
        );
        let name_len = u16::from_le_bytes([input[cursor + 28], input[cursor + 29]]) as usize;
        let extra_len = u16::from_le_bytes([input[cursor + 30], input[cursor + 31]]) as usize;
        let comment_len = u16::from_le_bytes([input[cursor + 32], input[cursor + 33]]) as usize;
        let name_start = cursor + CENTRAL_HEADER_SIZE;
        let name_end = name_start
            .checked_add(name_len)
            .expect("entry name end offset");
        assert!(
            name_end <= central_end,
            "central directory entry name bounds"
        );
        if &input[name_start..name_end] == expected_name {
            matching_entries += 1;
        }
        cursor = name_end
            .checked_add(extra_len)
            .and_then(|offset| offset.checked_add(comment_len))
            .expect("next central directory entry offset");
        assert!(cursor <= central_end, "central directory entry bounds");
    }
    assert_eq!(
        cursor, central_end,
        "central directory size matches entries"
    );
    matching_entries
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
