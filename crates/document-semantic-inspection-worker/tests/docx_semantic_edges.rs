use std::io::Cursor;

use document_semantic_inspection_core::{
    CapabilityState, InspectionProfileVersion, SemanticFingerprint, WorkerProtocolVersion,
    WorkerRequest, WorkerResponse,
};
use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, SemanticAdapter, SemanticAdapterOutput, run_worker_shell,
};
use sha2::{Digest, Sha256};

const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RELATIONSHIP_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPE_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const HEADER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const FOOTER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
const FOOTNOTES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes";
const ENDNOTES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes";
const HYPERLINK_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";

const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

#[test]
fn adjacent_text_and_run_fragment_boundaries_do_not_change_fingerprint() {
    let whole = docx(
        r#"<w:p><w:r><w:t>semanticidentity</w:t></w:r></w:p>"#,
        Vec::new(),
        Vec::new(),
    );
    let split_text_elements = docx(
        r#"<w:p><w:r><w:t>semantic</w:t><w:t>identity</w:t></w:r></w:p>"#,
        Vec::new(),
        Vec::new(),
    );
    let split_runs = docx(
        r#"<w:p><w:r><w:t>semantic</w:t></w:r><w:r><w:t>identity</w:t></w:r></w:p>"#,
        Vec::new(),
        Vec::new(),
    );

    let expected = fingerprint(&whole);
    assert_eq!(fingerprint(&split_text_elements), expected);
    assert_eq!(fingerprint(&split_runs), expected);
}

#[test]
fn formatting_revision_old_style_and_page_size_are_editorial_not_semantic() {
    let current = docx(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Current heading</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr>"#,
        Vec::new(),
        Vec::new(),
    );
    let with_old_formatting = docx(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/><w:pPrChange w:id="17" w:author="Editor"><w:pPr><w:pStyle w:val="OldHeading"/><w:sectPr><w:pgSz w:w="9000" w:h="12000"/></w:sectPr></w:pPr></w:pPrChange></w:pPr><w:r><w:t>Current heading</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr>"#,
        Vec::new(),
        Vec::new(),
    );

    assert_eq!(fingerprint(&with_old_formatting), fingerprint(&current));

    let changes = inspect(&with_old_formatting)
        .editorial_provenance()
        .tracked_changes
        .clone();
    assert!(changes.iter().any(|change| {
        change.kind == "format"
            && change.source_locator == "word/document.xml#format:17"
            && change.author_label.as_deref() == Some("Editor")
            && change.unresolved
    }));
}

#[test]
fn swapping_note_reference_ids_changes_fingerprint_for_footnotes_and_endnotes() {
    let original = notes_docx(false, false);
    let swapped_footnotes = notes_docx(true, false);
    let swapped_endnotes = notes_docx(false, true);

    assert_ne!(fingerprint(&original), fingerprint(&swapped_footnotes));
    assert_ne!(fingerprint(&original), fingerprint(&swapped_endnotes));
}

#[test]
fn swapping_section_header_and_footer_references_changes_fingerprint() {
    let original = section_header_footer_docx(false);
    let swapped = section_header_footer_docx(true);

    assert_ne!(fingerprint(&original), fingerprint(&swapped));
}

#[test]
fn note_capabilities_are_absent_without_notes_and_present_with_notes() {
    let without_notes = worker_response(&docx(
        r#"<w:p><w:r><w:t>Body text</w:t></w:r></w:p>"#,
        Vec::new(),
        Vec::new(),
    ));
    let with_notes = worker_response(&notes_docx(false, false));

    assert_capability_state(&without_notes, "footnotes", CapabilityState::Absent);
    assert_capability_state(&without_notes, "endnotes", CapabilityState::Absent);
    assert_capability_state(&with_notes, "footnotes", CapabilityState::Present);
    assert_capability_state(&with_notes, "endnotes", CapabilityState::Present);
}

fn inspect(bytes: &[u8]) -> SemanticAdapterOutput {
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("synthetic qualified DOCX package should inspect")
}

fn fingerprint(bytes: &[u8]) -> SemanticFingerprint {
    inspect(bytes).semantic_fingerprint()
}

fn worker_response(bytes: &[u8]) -> WorkerResponse {
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: DOCX_MEDIA_TYPE.to_owned(),
        expected_raw_content_hash: Sha256::digest(bytes).into(),
        expected_size_bytes: bytes.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).expect("serialize worker request");
    let mut input = Cursor::new(bytes);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_worker_shell(
        &request_bytes,
        &mut input,
        &mut stdout,
        &mut stderr,
        64 * 1024,
        8 * 1024 * 1024,
    );

    assert_eq!(
        exit,
        0,
        "worker shell failed: {}",
        String::from_utf8_lossy(&stderr)
    );
    assert!(stderr.is_empty());
    let response: WorkerResponse = serde_json::from_slice(&stdout).expect("decode worker response");
    response.validate().expect("worker response contract");
    response
}

fn assert_capability_state(response: &WorkerResponse, capability_id: &str, state: CapabilityState) {
    let capability = response
        .semantic_capabilities
        .iter()
        .find(|capability| capability.capability_id == capability_id)
        .unwrap_or_else(|| panic!("missing {capability_id} capability"));
    assert_eq!(
        capability.presence, state,
        "{capability_id} capability state"
    );
}

fn notes_docx(swap_footnotes: bool, swap_endnotes: bool) -> Vec<u8> {
    let (footnote_first, footnote_second) = if swap_footnotes {
        ("2", "1")
    } else {
        ("1", "2")
    };
    let (endnote_first, endnote_second) = if swap_endnotes {
        ("2", "1")
    } else {
        ("1", "2")
    };
    let body = format!(
        r#"<w:p><w:r><w:t>First marker</w:t></w:r><w:r><w:footnoteReference w:id="{footnote_first}"/></w:r><w:r><w:endnoteReference w:id="{endnote_first}"/></w:r><w:r><w:t>Second marker</w:t></w:r><w:r><w:footnoteReference w:id="{footnote_second}"/></w:r><w:r><w:endnoteReference w:id="{endnote_second}"/></w:r></w:p>"#
    );
    let parts = vec![
        (
            "word/footnotes.xml".to_owned(),
            format!(
                r#"<w:footnotes xmlns:w="{WORD_NS}"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>Footnote one</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>Footnote two</w:t></w:r></w:p></w:footnote></w:footnotes>"#
            ),
        ),
        (
            "word/endnotes.xml".to_owned(),
            format!(
                r#"<w:endnotes xmlns:w="{WORD_NS}"><w:endnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:endnote><w:endnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:endnote><w:endnote w:id="1"><w:p><w:r><w:t>Endnote one</w:t></w:r></w:p></w:endnote><w:endnote w:id="2"><w:p><w:r><w:t>Endnote two</w:t></w:r></w:p></w:endnote></w:endnotes>"#
            ),
        ),
    ];
    let relationships = vec![
        relationship("rIdFootnotes", FOOTNOTES_REL, "footnotes.xml"),
        relationship("rIdEndnotes", ENDNOTES_REL, "endnotes.xml"),
    ];
    docx(&body, parts, relationships)
}

fn section_header_footer_docx(swap_references: bool) -> Vec<u8> {
    let (section_one_header, section_two_header, section_one_footer, section_two_footer) =
        if swap_references {
            (
                "rIdHeaderTwo",
                "rIdHeaderOne",
                "rIdFooterTwo",
                "rIdFooterOne",
            )
        } else {
            (
                "rIdHeaderOne",
                "rIdHeaderTwo",
                "rIdFooterOne",
                "rIdFooterTwo",
            )
        };
    let body = format!(
        r#"<w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="{section_one_header}"/><w:footerReference w:type="default" r:id="{section_one_footer}"/><w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:pPr><w:r><w:t>First section</w:t></w:r></w:p><w:p><w:r><w:t>Second section</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="{section_two_header}"/><w:footerReference w:type="default" r:id="{section_two_footer}"/><w:pgSz w:w="12240" w:h="15840"/></w:sectPr>"#
    );
    let mut parts = Vec::new();
    for (name, link_id, label) in [
        ("word/header1.xml", "rIdHeaderLinkOne", "Section heading"),
        ("word/header2.xml", "rIdHeaderLinkTwo", "Section heading"),
        ("word/footer1.xml", "rIdFooterLinkOne", "Section footer"),
        ("word/footer2.xml", "rIdFooterLinkTwo", "Section footer"),
    ] {
        let root = if name.contains("/header") {
            "hdr"
        } else {
            "ftr"
        };
        parts.push((
            name.to_owned(),
            format!(
                r#"<w:{root} xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:p><w:hyperlink r:id="{link_id}"><w:r><w:t>{label}</w:t></w:r></w:hyperlink></w:p></w:{root}>"#
            ),
        ));
        let relationship_name = name.replacen("word/", "word/_rels/", 1) + ".rels";
        let target = format!("https://example.test/{name}");
        parts.push((
            relationship_name,
            format!(
                r#"<Relationships xmlns="{RELATIONSHIP_NS}"><Relationship Id="{link_id}" Type="{HYPERLINK_REL}" Target="{target}" TargetMode="External"/></Relationships>"#
            ),
        ));
    }
    let relationships = vec![
        relationship("rIdHeaderOne", HEADER_REL, "header1.xml"),
        relationship("rIdHeaderTwo", HEADER_REL, "header2.xml"),
        relationship("rIdFooterOne", FOOTER_REL, "footer1.xml"),
        relationship("rIdFooterTwo", FOOTER_REL, "footer2.xml"),
    ];
    docx(&body, parts, relationships)
}

fn relationship(id: &str, kind: &str, target: &str) -> (String, String, String) {
    (id.to_owned(), kind.to_owned(), target.to_owned())
}

fn docx(
    body: &str,
    extra_parts: Vec<(String, String)>,
    relationships: Vec<(String, String, String)>,
) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body>{body}</w:body></w:document>"#
    );
    let content_types = content_types(&extra_parts);
    let document_relationships = document_relationships(&relationships);
    let mut parts = vec![
        ("[Content_Types].xml".to_owned(), content_types.into_bytes()),
        (
            "_rels/.rels".to_owned(),
            ROOT_RELATIONSHIPS.as_bytes().to_vec(),
        ),
        ("word/document.xml".to_owned(), document.into_bytes()),
        (
            "word/_rels/document.xml.rels".to_owned(),
            document_relationships.into_bytes(),
        ),
    ];
    parts.extend(
        extra_parts
            .into_iter()
            .map(|(name, content)| (name, content.into_bytes())),
    );
    stored_zip(parts)
}

fn content_types(extra_parts: &[(String, String)]) -> String {
    let mut overrides = r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#.to_string();
    for (name, _) in extra_parts {
        let content_type = match name.as_str() {
            "word/footnotes.xml" => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"
            }
            "word/endnotes.xml" => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"
            }
            name if name.starts_with("word/header") => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"
            }
            name if name.starts_with("word/footer") => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"
            }
            name if name.ends_with(".rels") => continue,
            other => panic!("unexpected synthetic DOCX part {other}"),
        };
        overrides.push_str(&format!(
            r#"<Override PartName="/{name}" ContentType="{content_type}"/>"#
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="{CONTENT_TYPE_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>{overrides}</Types>"#
    )
}

fn document_relationships(relationships: &[(String, String, String)]) -> String {
    let entries = relationships
        .iter()
        .map(|(id, kind, target)| {
            format!(r#"<Relationship Id="{id}" Type="{kind}" Target="{target}"/>"#)
        })
        .collect::<String>();
    format!(r#"<Relationships xmlns="{RELATIONSHIP_NS}">{entries}</Relationships>"#)
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
