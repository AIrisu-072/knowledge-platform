use document_semantic_inspection_core::{FormatId, SemanticFingerprint};
use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
    WorkerFailureCode,
};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/docx/base.docx");
const BODY_TEXT_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/body-text-change.docx"
);
const HEADING_LIST_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/heading-list-change.docx"
);
const TABLE_MERGE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/table-merge-change.docx"
);
const HEADER_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/header-change.docx"
);
const FOOTER_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/footer-change.docx"
);
const FOOTNOTE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/footnote-change.docx"
);
const ENDNOTE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/endnote-change.docx"
);
const HYPERLINK_TARGET_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/hyperlink-target-change.docx"
);
const IMAGE_CONTENT_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/image-content-change.docx"
);
const SECTION_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/section-change.docx"
);
const TRACKED_REPLACEMENT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/tracked-replacement.docx"
);
const COMMENT_UNRESOLVED: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/comment-unresolved.docx"
);
const COMMENT_RESOLVED: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/comment-resolved.docx"
);
const METADATA_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/metadata-noise.docx"
);
const FONT_ONLY: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/font-only.docx"
);
const RELATIONSHIP_ID_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/relationship-id-noise.docx"
);
const PACKAGE_ORDER_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/package-order-noise.docx"
);
const UNKNOWN_SEMANTIC_PART: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/unknown-semantic-part.docx"
);
const MALFORMED: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/malformed.docx"
);
const DEEP_OOXML: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/docx/deep-ooxml.docx"
);

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

fn inspect(bytes: &[u8]) -> SemanticAdapterOutput {
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("qualified DOCX fixture should inspect")
}

fn fingerprint(bytes: &[u8]) -> SemanticFingerprint {
    inspect(bytes).semantic_fingerprint()
}

fn assert_same_as_base(bytes: &[u8]) {
    assert_eq!(fingerprint(BASE), fingerprint(bytes));
}

fn assert_different_from_base(bytes: &[u8]) {
    assert_ne!(fingerprint(BASE), fingerprint(bytes));
}

fn assert_failure(bytes: &[u8], expected: WorkerFailureCode) {
    let failure = DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect_err("fixture should fail closed");
    assert_eq!(failure.code(), expected, "{failure}");
}

#[test]
fn adapter_reports_the_frozen_docx_format() {
    assert_eq!(DocxAdapter.format(), FormatId::Docx);
}

#[test]
fn body_order_and_heading_list_structure_are_version_significant() {
    assert_different_from_base(BODY_TEXT_CHANGE);
    assert_different_from_base(HEADING_LIST_CHANGE);

    let first = minimal_document(
        "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>         <w:p><w:r><w:t>First paragraph</w:t></w:r></w:p>         <w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p>",
    );
    let reordered = minimal_document(
        "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>         <w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p>         <w:p><w:r><w:t>First paragraph</w:t></w:r></w:p>",
    );
    assert_ne!(
        fingerprint(&simple_docx(&first, EMPTY_DOCUMENT_RELATIONSHIPS, false)),
        fingerprint(&simple_docx(
            &reordered,
            EMPTY_DOCUMENT_RELATIONSHIPS,
            false
        )),
        "reader-visible paragraph order must be preserved",
    );
}

#[test]
fn tables_headers_footers_notes_links_images_and_sections_are_version_significant() {
    for fixture in [
        TABLE_MERGE_CHANGE,
        HEADER_CHANGE,
        FOOTER_CHANGE,
        FOOTNOTE_CHANGE,
        ENDNOTE_CHANGE,
        HYPERLINK_TARGET_CHANGE,
        IMAGE_CONTENT_CHANGE,
        SECTION_CHANGE,
    ] {
        assert_different_from_base(fixture);
    }
}

#[test]
fn hyperlink_relationship_order_and_identifiers_are_noise_but_targets_are_semantic() {
    let first_order = linked_docx("rIdOne", "rIdTwo", false);
    let reversed_relationship_order = linked_docx("linkA", "linkB", true);
    assert_eq!(
        fingerprint(&first_order),
        fingerprint(&reversed_relationship_order)
    );

    let changed_target = linked_docx_with_targets(
        "rIdOne",
        "rIdTwo",
        "https://example.test/changed",
        "https://example.test/two",
        false,
    );
    assert_ne!(fingerprint(&first_order), fingerprint(&changed_target));
}

#[test]
fn xml_serialization_package_order_and_pure_margin_noise_are_invariant() {
    let compact = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>stable text</w:t></w:r></w:p></w:body></w:document>"#;
    let serialized = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes'?>
<d:document xmlns:d='http://schemas.openxmlformats.org/wordprocessingml/2006/main'>
  <d:body>
    <d:p><d:r><d:t>stable text</d:t></d:r></d:p>
  </d:body>
</d:document>"#;
    let compact_package = simple_docx(compact, EMPTY_DOCUMENT_RELATIONSHIPS, false);
    let serialized_reordered_package = simple_docx(serialized, EMPTY_DOCUMENT_RELATIONSHIPS, true);
    assert_eq!(
        fingerprint(&compact_package),
        fingerprint(&serialized_reordered_package)
    );

    assert_same_as_base(METADATA_NOISE);
    assert_same_as_base(FONT_ONLY);
    assert_same_as_base(RELATIONSHIP_ID_NOISE);
    assert_same_as_base(PACKAGE_ORDER_NOISE);

    let without_page_margins =
        minimal_document("<w:p><w:r><w:t>stable section text</w:t></w:r></w:p>");
    let with_page_margins = minimal_document(
        "<w:p><w:r><w:t>stable section text</w:t></w:r></w:p>         <w:sectPr><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr>",
    );
    assert_eq!(
        fingerprint(&simple_docx(
            &without_page_margins,
            EMPTY_DOCUMENT_RELATIONSHIPS,
            false
        )),
        fingerprint(&simple_docx(
            &with_page_margins,
            EMPTY_DOCUMENT_RELATIONSHIPS,
            false
        )),
        "pure page-margin formatting is not version identity",
    );
}

#[test]
fn tracked_changes_use_proposed_final_projection_and_remain_editorial_evidence() {
    assert_same_as_base(TRACKED_REPLACEMENT);

    let editorial = inspect(TRACKED_REPLACEMENT).editorial_provenance();
    assert!(
        editorial
            .tracked_changes
            .iter()
            .any(|change| change.kind == "insertion")
    );
    assert!(
        editorial
            .tracked_changes
            .iter()
            .any(|change| change.kind == "deletion")
    );
    assert!(
        editorial
            .tracked_changes
            .iter()
            .all(|change| change.unresolved)
    );
}

#[test]
fn comments_are_reported_as_editorial_evidence_without_changing_identity() {
    for (fixture, resolved_state) in [
        (COMMENT_UNRESOLVED, "unresolved"),
        (COMMENT_RESOLVED, "resolved"),
    ] {
        assert_same_as_base(fixture);
        let comments = &inspect(fixture).editorial_provenance().comments;
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].resolved_state, resolved_state);
        assert!(!comments[0].content.is_empty());
    }
}

#[test]
fn ooxml_coverage_sentinel_accepts_known_parts_and_rejects_unknown_semantics() {
    OoxmlCoverageSentinel::validate_package(BASE).expect("qualified known DOCX package");

    let sentinel_error = OoxmlCoverageSentinel::validate_package(UNKNOWN_SEMANTIC_PART)
        .expect_err("unknown potentially semantic part must fail closed");
    assert_eq!(
        sentinel_error.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
    assert_failure(
        UNKNOWN_SEMANTIC_PART,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );

    let unknown_relationship = docx_with_unknown_relationship();
    let sentinel_error = OoxmlCoverageSentinel::validate_package(&unknown_relationship)
        .expect_err("unknown potentially semantic relationship must fail closed");
    assert_eq!(
        sentinel_error.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
    assert_failure(
        &unknown_relationship,
        WorkerFailureCode::UnsupportedSemanticConstruct,
    );
}

#[test]
fn malformed_deep_and_oversized_packages_fail_with_qualified_errors() {
    assert_failure(MALFORMED, WorkerFailureCode::SemanticExtractionFailed);
    assert_failure(
        DEEP_OOXML,
        WorkerFailureCode::InspectionResourceLimitExceeded,
    );

    let oversized = oversized_docx();
    assert_failure(
        &oversized,
        WorkerFailureCode::InspectionResourceLimitExceeded,
    );
}

fn minimal_document(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>{body}</w:body>
</w:document>"#
    )
}

fn simple_docx(
    document_xml: &str,
    document_relationships_xml: &str,
    reverse_package_order: bool,
) -> Vec<u8> {
    package_with_parts(
        CONTENT_TYPES,
        document_xml,
        document_relationships_xml,
        Vec::new(),
        reverse_package_order,
    )
}

fn package_with_parts(
    content_types_xml: &str,
    document_xml: &str,
    document_relationships_xml: &str,
    extras: Vec<(String, Vec<u8>)>,
    reverse_package_order: bool,
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
    if reverse_package_order {
        parts.reverse();
    }
    stored_zip(parts)
}

fn linked_docx(first_id: &str, second_id: &str, reverse_relationship_order: bool) -> Vec<u8> {
    linked_docx_with_targets(
        first_id,
        second_id,
        "https://example.test/one",
        "https://example.test/two",
        reverse_relationship_order,
    )
}

fn linked_docx_with_targets(
    first_id: &str,
    second_id: &str,
    first_target: &str,
    second_target: &str,
    reverse_relationship_order: bool,
) -> Vec<u8> {
    let document = minimal_document(&format!(
        "<w:p>         <w:hyperlink r:id=\"{first_id}\"><w:r><w:t>one</w:t></w:r></w:hyperlink>         <w:hyperlink r:id=\"{second_id}\"><w:r><w:t>two</w:t></w:r></w:hyperlink>         </w:p>"
    ));
    let first = format!(
        r#"<Relationship Id="{first_id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{first_target}" TargetMode="External"/>"#
    );
    let second = format!(
        r#"<Relationship Id="{second_id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{second_target}" TargetMode="External"/>"#
    );
    let (first, second) = if reverse_relationship_order {
        (second, first)
    } else {
        (first, second)
    };
    let relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{first}{second}</Relationships>"#
    );
    simple_docx(&document, &relationships, false)
}

fn docx_with_unknown_relationship() -> Vec<u8> {
    let content_types = CONTENT_TYPES.replace(
        "</Types>",
        r#"<Override PartName="/word/semantic.xml" ContentType="application/vnd.example.wordprocessingml.semantic+xml"/></Types>"#,
    );
    let relationships = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdSemantic" Type="urn:example:relationships/semantic" Target="semantic.xml"/>
</Relationships>"#;
    package_with_parts(
        &content_types,
        &minimal_document("<w:p><w:r><w:t>known body</w:t></w:r></w:p>"),
        relationships,
        vec![(
            "word/semantic.xml".to_owned(),
            br#"<?xml version="1.0"?><semantic><value>potential meaning</value></semantic>"#
                .to_vec(),
        )],
        false,
    )
}

fn oversized_docx() -> Vec<u8> {
    let document = minimal_document("<w:p><w:r><w:t>bounded package</w:t></w:r></w:p>");
    package_with_parts(
        CONTENT_TYPES,
        &document,
        EMPTY_DOCUMENT_RELATIONSHIPS,
        vec![(
            "word/oversized.xml".to_owned(),
            vec![b'x'; 8 * 1024 * 1024 + 1],
        )],
        false,
    )
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
