use document_semantic_inspection_core::SemanticFingerprint;
use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, WorkerFailureCode,
};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const PACKAGE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const CONTENT_TYPES_WITH_HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
</Types>"#;

const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const EMPTY_DOCUMENT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>known document text</w:t></w:r></w:p></w:body>
</w:document>"#;

const HEADER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:p><w:r><w:t>header semantic text</w:t></w:r></w:p>
</w:hdr>"#;

fn assert_rejected_by_sentinel_and_adapter(bytes: &[u8], expected: WorkerFailureCode) {
    let sentinel_error = OoxmlCoverageSentinel::validate_package(bytes)
        .expect_err("unsupported or malformed OOXML QName must fail the coverage sentinel");
    assert_eq!(sentinel_error.code(), expected, "{sentinel_error}");

    let adapter_error = DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect_err("unsupported or malformed OOXML QName must fail the DOCX adapter");
    assert_eq!(adapter_error.code(), expected, "{adapter_error}");
}

fn inspect_fingerprint(bytes: &[u8]) -> SemanticFingerprint {
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("known namespace prefixes may be renamed without changing semantics")
        .semantic_fingerprint()
}

#[test]
fn unknown_namespace_element_with_known_word_local_name_fails_closed() {
    let document = document_with_body(
        r#"<w:p><w:r><w:t>known text</w:t></w:r></w:p>
<x:p xmlns:x="urn:example:unknown-word-namespace"><w:r><w:t>spoofed text</w:t></w:r></x:p>"#,
    );
    let bytes = package(
        CONTENT_TYPES,
        ROOT_RELATIONSHIPS,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        &document,
        Vec::new(),
    );

    assert_rejected_by_sentinel_and_adapter(
        &bytes,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn unknown_namespace_attribute_with_known_word_local_name_fails_closed() {
    let document = document_with_body(
        r#"<w:p xmlns:x="urn:example:unknown-word-namespace">
<w:pPr><w:pStyle w:val="Title" x:val="SpoofedTitle"/></w:pPr>
<w:r><w:t>known text</w:t></w:r></w:p>"#,
    );
    let bytes = package(
        CONTENT_TYPES,
        ROOT_RELATIONSHIPS,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        &document,
        Vec::new(),
    );

    assert_rejected_by_sentinel_and_adapter(
        &bytes,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn incorrect_relationships_root_namespace_fails_closed() {
    let wrong_root_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="urn:example:not-the-package-relationships-namespace">
<Relationship Id="rIdOffice" Type="{OFFICE_RELATIONSHIPS_NS}/officeDocument" Target="word/document.xml"/>
</Relationships>"#
    );
    let bytes = package(
        CONTENT_TYPES,
        &wrong_root_relationships,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        DOCUMENT_XML,
        Vec::new(),
    );

    assert_rejected_by_sentinel_and_adapter(
        &bytes,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn unknown_namespace_relationship_element_with_known_local_name_fails_closed() {
    let document = document_with_body(
        r#"<w:p><w:r><w:t>known body</w:t></w:r></w:p>
<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/></w:sectPr>"#,
    );
    let document = document.replace(
        "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"",
        "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"",
    );
    let document_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_RELATIONSHIPS_NS}" xmlns:x="urn:example:unknown-relationships-namespace">
<x:Relationship Id="rIdHeader" Type="{OFFICE_RELATIONSHIPS_NS}/header" Target="header1.xml"/>
</Relationships>"#
    );
    let bytes = package(
        CONTENT_TYPES_WITH_HEADER,
        ROOT_RELATIONSHIPS,
        &document_relationships,
        &document,
        vec![(
            "word/header1.xml".to_owned(),
            HEADER_XML.as_bytes().to_vec(),
        )],
    );

    assert_rejected_by_sentinel_and_adapter(
        &bytes,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn prefix_bound_only_by_default_namespace_does_not_bind_a_prefixed_qname() {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<document xmlns="{WORD_NS}"><body><w:p><w:r><w:t>known text</w:t></w:r></w:p></body></document>"#
    );
    let bytes = package(
        CONTENT_TYPES,
        ROOT_RELATIONSHIPS,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        &document,
        Vec::new(),
    );

    assert_rejected_by_sentinel_and_adapter(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn renaming_a_known_word_namespace_prefix_preserves_semantics() {
    let renamed_document = DOCUMENT_XML
        .replace("xmlns:w=", "xmlns:word=")
        .replace("w:", "word:");
    let standard = package(
        CONTENT_TYPES,
        ROOT_RELATIONSHIPS,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        DOCUMENT_XML,
        Vec::new(),
    );
    let renamed = package(
        CONTENT_TYPES,
        ROOT_RELATIONSHIPS,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        &renamed_document,
        Vec::new(),
    );

    OoxmlCoverageSentinel::validate_package(&standard).expect("standard package QName");
    OoxmlCoverageSentinel::validate_package(&renamed).expect("renamed package QName");
    assert_eq!(
        inspect_fingerprint(&standard),
        inspect_fingerprint(&renamed)
    );
}

fn document_with_body(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}"><w:body>{body}</w:body></w:document>"#
    )
}

fn package(
    content_types: &str,
    root_relationships: &str,
    document_relationships: &str,
    document_xml: &str,
    extra_parts: Vec<(String, Vec<u8>)>,
) -> Vec<u8> {
    let mut parts = vec![
        (
            "[Content_Types].xml".to_owned(),
            content_types.as_bytes().to_vec(),
        ),
        (
            "_rels/.rels".to_owned(),
            root_relationships.as_bytes().to_vec(),
        ),
        (
            "word/document.xml".to_owned(),
            document_xml.as_bytes().to_vec(),
        ),
        (
            "word/_rels/document.xml.rels".to_owned(),
            document_relationships.as_bytes().to_vec(),
        ),
    ];
    parts.extend(extra_parts);
    stored_zip(parts)
}

fn stored_zip(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    assert!(parts.len() <= u16::MAX as usize);

    let mut archive = Vec::new();
    let mut local_offsets = Vec::with_capacity(parts.len());
    let mut checksums = Vec::with_capacity(parts.len());

    for (name, contents) in &parts {
        let name_bytes = name.as_bytes();
        let name_length = u16::try_from(name_bytes.len()).expect("ZIP part name length");
        let size = u32::try_from(contents.len()).expect("ZIP part size");
        let local_offset = u32::try_from(archive.len()).expect("ZIP local-header offset");
        let checksum = crc32(contents);

        local_offsets.push(local_offset);
        checksums.push(checksum);

        archive.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&checksum.to_le_bytes());
        archive.extend_from_slice(&size.to_le_bytes());
        archive.extend_from_slice(&size.to_le_bytes());
        archive.extend_from_slice(&name_length.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(name_bytes);
        archive.extend_from_slice(contents);
    }

    let central_directory_offset =
        u32::try_from(archive.len()).expect("ZIP central-directory offset");
    for (index, (name, contents)) in parts.iter().enumerate() {
        let name_bytes = name.as_bytes();
        let name_length = u16::try_from(name_bytes.len()).expect("ZIP part name length");
        let size = u32::try_from(contents.len()).expect("ZIP part size");

        archive.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&checksums[index].to_le_bytes());
        archive.extend_from_slice(&size.to_le_bytes());
        archive.extend_from_slice(&size.to_le_bytes());
        archive.extend_from_slice(&name_length.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u32.to_le_bytes());
        archive.extend_from_slice(&local_offsets[index].to_le_bytes());
        archive.extend_from_slice(name_bytes);
    }

    let central_directory_size = u32::try_from(archive.len())
        .expect("ZIP archive size")
        .checked_sub(central_directory_offset)
        .expect("central-directory range fits");
    let entry_count = u16::try_from(parts.len()).expect("ZIP entry count");

    archive.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&entry_count.to_le_bytes());
    archive.extend_from_slice(&entry_count.to_le_bytes());
    archive.extend_from_slice(&central_directory_size.to_le_bytes());
    archive.extend_from_slice(&central_directory_offset.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
