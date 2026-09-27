use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, WorkerFailureCode,
};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPE_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const MAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const FOOTNOTES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const ENDNOTES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
const FOOTNOTES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes";
const ENDNOTES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes";

#[test]
fn referenced_note_revisions_keep_part_locator_and_original_metadata() {
    let baseline = notes_docx(false);
    let revised = notes_docx(true);
    let baseline_output = inspect_valid(&baseline);
    let revised_output = inspect_valid(&revised);

    assert_eq!(
        baseline_output.semantic_fingerprint(),
        revised_output.semantic_fingerprint(),
        "the insertion is included in the proposed-final note text"
    );

    let changes = &revised_output.editorial_provenance().tracked_changes;
    for (locator, author, timestamp) in [
        (
            "word/footnotes.xml#insertion:31",
            "Footnote Editor",
            "2025-02-03T04:05:06Z",
        ),
        (
            "word/endnotes.xml#insertion:47",
            "Endnote Editor",
            "2025-03-04T05:06:07Z",
        ),
    ] {
        let change = changes
            .iter()
            .find(|change| change.source_locator == locator)
            .unwrap_or_else(|| panic!("missing tracked-change evidence for {locator}"));
        assert_eq!(change.kind, "insertion", "{locator}");
        assert_eq!(change.author_label.as_deref(), Some(author), "{locator}");
        assert_eq!(change.timestamp.as_deref(), Some(timestamp), "{locator}");
        assert!(change.unresolved, "{locator}");
    }
}

#[test]
fn complex_field_hyperlink_instruction_target_is_semantic_or_rejected() {
    let first = complex_field_hyperlink_docx("https://example.test/one");
    let changed_target = complex_field_hyperlink_docx("https://example.test/two");

    assert_distinct_or_fail_closed(&first, &changed_target);
}

#[test]
fn section_break_type_next_page_and_continuous_are_distinct() {
    let next_page = section_type_docx("nextPage");
    let continuous = section_type_docx("continuous");
    let next_page_output = inspect_valid(&next_page);
    let continuous_output = inspect_valid(&continuous);

    assert_ne!(
        next_page_output.semantic_fingerprint(),
        continuous_output.semantic_fingerprint(),
        "section break type changes where the following section begins"
    );
}

#[test]
fn horizontal_table_merge_structure_is_semantic_or_rejected() {
    let unmerged = horizontal_merge_docx(false);
    let merged = horizontal_merge_docx(true);

    assert_distinct_or_fail_closed(&unmerged, &merged);
}

fn inspect_valid(bytes: &[u8]) -> document_semantic_inspection_worker::SemanticAdapterOutput {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic package passes OOXML sentinel");
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("synthetic package is supported by the DOCX adapter")
}

fn assert_distinct_or_fail_closed(first: &[u8], changed: &[u8]) {
    let inspect = |bytes| {
        OoxmlCoverageSentinel::validate_package(bytes)
            .and_then(|()| DocxAdapter.inspect(bytes, &AdapterProfile::default()))
    };
    let first_output = inspect(first);
    let changed_output = inspect(changed);
    match (first_output, changed_output) {
        (Ok(first), Ok(changed)) => assert_ne!(
            first.semantic_fingerprint(),
            changed.semantic_fingerprint(),
            "different semantic forms cannot share a successful fingerprint"
        ),
        (Err(first), Err(changed)) => {
            assert_eq!(
                first.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the first form must fail closed with an explicit unsupported-construct result"
            );
            assert_eq!(
                changed.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the changed form must fail closed with an explicit unsupported-construct result"
            );
        }
        (Err(error), Ok(_)) | (Ok(_), Err(error)) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "a form may fail only as an explicit unsupported construct"
        ),
    }
}

fn notes_docx(with_revisions: bool) -> Vec<u8> {
    let footnote_content = if with_revisions {
        r#"<w:ins w:id="31" w:author="Footnote Editor" w:date="2025-02-03T04:05:06Z"><w:r><w:t>Footnote text</w:t></w:r></w:ins>"#
    } else {
        r#"<w:r><w:t>Footnote text</w:t></w:r>"#
    };
    let endnote_content = if with_revisions {
        r#"<w:ins w:id="47" w:author="Endnote Editor" w:date="2025-03-04T05:06:07Z"><w:r><w:t>Endnote text</w:t></w:r></w:ins>"#
    } else {
        r#"<w:r><w:t>Endnote text</w:t></w:r>"#
    };
    let footnotes = format!(
        r#"<w:footnotes xmlns:w="{WORD_NS}"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p>{footnote_content}</w:p></w:footnote></w:footnotes>"#
    );
    let endnotes = format!(
        r#"<w:endnotes xmlns:w="{WORD_NS}"><w:endnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:endnote><w:endnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:endnote><w:endnote w:id="1"><w:p>{endnote_content}</w:p></w:endnote></w:endnotes>"#
    );
    let body = r#"<w:p><w:r><w:t>Referenced notes</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:endnoteReference w:id="1"/></w:r></w:p>"#;
    let relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdFootnotes" Type="{FOOTNOTES_REL}" Target="footnotes.xml"/><Relationship Id="rIdEndnotes" Type="{ENDNOTES_REL}" Target="endnotes.xml"/></Relationships>"#
    );
    docx(
        body,
        &relationships,
        vec![
            XmlPart::new("word/footnotes.xml", FOOTNOTES_CONTENT_TYPE, footnotes),
            XmlPart::new("word/endnotes.xml", ENDNOTES_CONTENT_TYPE, endnotes),
        ],
    )
}

fn complex_field_hyperlink_docx(target: &str) -> Vec<u8> {
    let body = format!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>HYPERLINK "{target}"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Open document</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#
    );
    docx(&body, &empty_document_relationships(), Vec::new())
}

fn section_type_docx(section_type: &str) -> Vec<u8> {
    let body = format!(
        r#"<w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>First section content</w:t></w:r></w:p><w:p><w:r><w:t>Second section content</w:t></w:r></w:p><w:sectPr><w:type w:val="{section_type}"/></w:sectPr>"#
    );
    docx(&body, &empty_document_relationships(), Vec::new())
}

fn horizontal_merge_docx(merged: bool) -> Vec<u8> {
    let (first_properties, second_properties) = if merged {
        (
            r#"<w:tcPr><w:hMerge w:val="restart"/></w:tcPr>"#,
            r#"<w:tcPr><w:hMerge w:val="continue"/></w:tcPr>"#,
        )
    } else {
        ("", "")
    };
    let body = format!(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="2400"/><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc>{first_properties}<w:p><w:r><w:t>Left cell</w:t></w:r></w:p></w:tc><w:tc>{second_properties}<w:p><w:r><w:t>Right cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#
    );
    docx(&body, &empty_document_relationships(), Vec::new())
}

fn empty_document_relationships() -> String {
    format!(r#"<Relationships xmlns="{PACKAGE_REL_NS}"/>"#)
}

struct XmlPart {
    name: &'static str,
    content_type: &'static str,
    data: String,
}

impl XmlPart {
    fn new(name: &'static str, content_type: &'static str, data: String) -> Self {
        Self {
            name,
            content_type,
            data,
        }
    }
}

fn docx(body: &str, document_relationships: &str, extras: Vec<XmlPart>) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body>{body}</w:body></w:document>"#
    );
    let mut overrides =
        format!(r#"<Override PartName="/word/document.xml" ContentType="{MAIN_CONTENT_TYPE}"/>"#);
    for part in &extras {
        overrides.push_str(&format!(
            r#"<Override PartName="/{}" ContentType="{}"/>"#,
            part.name, part.content_type
        ));
    }
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="{CONTENT_TYPE_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>{overrides}</Types>"#
    );
    let root_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#
    );
    let mut parts = vec![
        ("[Content_Types].xml".to_owned(), content_types.into_bytes()),
        ("_rels/.rels".to_owned(), root_relationships.into_bytes()),
        ("word/document.xml".to_owned(), document.into_bytes()),
        (
            "word/_rels/document.xml.rels".to_owned(),
            document_relationships.as_bytes().to_vec(),
        ),
    ];
    parts.extend(
        extras
            .into_iter()
            .map(|part| (part.name.to_owned(), part.data.into_bytes())),
    );
    stored_zip(parts)
}

fn stored_zip(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
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
