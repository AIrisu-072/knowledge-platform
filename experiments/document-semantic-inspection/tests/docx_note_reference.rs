use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile, PocError,
    fingerprint as semantic_fingerprint,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const PACKAGE_RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_RELATIONSHIPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WORDPROCESSINGML: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[derive(Clone, Copy, Debug)]
enum NoteKind {
    Footnote,
    Endnote,
}

impl NoteKind {
    fn part_name(self) -> &'static str {
        match self {
            Self::Footnote => "footnotes",
            Self::Endnote => "endnotes",
        }
    }

    fn relationship_name(self) -> &'static str {
        self.part_name()
    }

    fn reference_name(self) -> &'static str {
        match self {
            Self::Footnote => "footnoteReference",
            Self::Endnote => "endnoteReference",
        }
    }

    fn note_name(self) -> &'static str {
        match self {
            Self::Footnote => "footnote",
            Self::Endnote => "endnote",
        }
    }
}

#[test]
fn referenced_footnote_identity_and_body_order_are_semantic_or_fail_closed() {
    assert_note_reference_order_changes_or_fails_closed(NoteKind::Footnote);
}

#[test]
fn referenced_endnote_identity_and_body_order_are_semantic_or_fail_closed() {
    assert_note_reference_order_changes_or_fails_closed(NoteKind::Endnote);
}

#[test]
fn dangling_footnote_reference_fails_closed() {
    assert_dangling_note_reference_fails_closed(NoteKind::Footnote);
}

#[test]
fn dangling_endnote_reference_fails_closed() {
    assert_dangling_note_reference_fails_closed(NoteKind::Endnote);
}

fn assert_note_reference_order_changes_or_fails_closed(kind: NoteKind) {
    let forward = two_note_docx(kind, [1, 2]);
    let reversed = two_note_docx(kind, [2, 1]);
    assert_valid_note_pair(&forward, kind, [1, 2]);
    assert_valid_note_pair(&reversed, kind, [2, 1]);
    assert_only_document_part_differs(&forward, &reversed);

    let forward_result = inspect(&forward);
    let reversed_result = inspect(&reversed);
    match (forward_result, reversed_result) {
        (Ok(forward), Ok(reversed)) => assert_ne!(
            semantic_fingerprint(&forward.semantic_projection),
            semantic_fingerprint(&reversed.semantic_projection),
            "swapping which note IDs the unchanged body references changed while the semantic fingerprint stayed equal"
        ),
        (Err(forward), Err(reversed))
            if forward.code() == ErrorCode::UnsupportedSemanticConstruct
                && reversed.code() == ErrorCode::UnsupportedSemanticConstruct => {}
        (forward, reversed) => panic!(
            "two-note packages must either differ semantically or both fail closed as UnsupportedSemanticConstruct; forward={:?}, reversed={:?}",
            error_summary(forward),
            error_summary(reversed)
        ),
    }
}

fn assert_dangling_note_reference_fails_closed(kind: NoteKind) {
    let valid = single_note_docx(kind, 1);
    let dangling = single_note_docx(kind, 99);
    assert_single_note_reference(&valid, kind, 1);
    assert_single_note_reference(&dangling, kind, 99);
    assert_only_document_part_differs(&valid, &dangling);

    inspect(&valid).expect("single-note baseline is valid and supported");
    let result = inspect(&dangling);
    assert!(
        result.is_err(),
        "a {} reference to missing note ID 99 was silently accepted",
        kind.part_name()
    );
}

fn assert_valid_note_pair(docx: &[u8], kind: NoteKind, expected_references: [u8; 2]) {
    let parts = read_parts(docx);
    let document = std::str::from_utf8(&parts["word/document.xml"]).expect("document XML");
    let notes =
        std::str::from_utf8(&parts[&format!("word/{}.xml", kind.part_name())]).expect("notes XML");
    let relationship = std::str::from_utf8(&parts["word/_rels/document.xml.rels"])
        .expect("document relationships XML");
    let content_types =
        std::str::from_utf8(&parts["[Content_Types].xml"]).expect("content types XML");

    for id in expected_references {
        assert!(document.contains(&format!("<w:{} w:id=\"{id}\"/>", kind.reference_name())));
    }
    let first_reference = document
        .find(&format!(
            "<w:{} w:id=\"{}\"/>",
            kind.reference_name(),
            expected_references[0]
        ))
        .expect("first note reference");
    let second_reference = document
        .find(&format!(
            "<w:{} w:id=\"{}\"/>",
            kind.reference_name(),
            expected_references[1]
        ))
        .expect("second note reference");
    assert!(
        first_reference < second_reference,
        "note references are in body order"
    );
    assert!(document.contains("same body text"));
    assert!(notes.contains("w:id=\"1\""));
    assert!(notes.contains("w:id=\"2\""));
    assert!(notes.contains("Alpha note"));
    assert!(notes.contains("Beta note"));
    assert!(relationship.contains(&format!(
        "Type=\"{OFFICE_RELATIONSHIPS}/{}\" Target=\"{}.xml\"",
        kind.relationship_name(),
        kind.part_name()
    )));
    assert!(content_types.contains(&format!("PartName=\"/word/{}.xml\"", kind.part_name())));
}

fn assert_single_note_reference(docx: &[u8], kind: NoteKind, reference_id: u8) {
    let parts = read_parts(docx);
    let document = std::str::from_utf8(&parts["word/document.xml"]).expect("document XML");
    let notes =
        std::str::from_utf8(&parts[&format!("word/{}.xml", kind.part_name())]).expect("notes XML");
    let relationship = std::str::from_utf8(&parts["word/_rels/document.xml.rels"])
        .expect("document relationships XML");
    let content_types =
        std::str::from_utf8(&parts["[Content_Types].xml"]).expect("content types XML");

    assert!(document.contains(&format!(
        "<w:{} w:id=\"{reference_id}\"/>",
        kind.reference_name()
    )));
    assert!(document.contains("same body text"));
    assert!(notes.contains("w:id=\"1\""));
    assert!(notes.contains("Alpha note"));
    assert!(!notes.contains("w:id=\"2\""));
    assert!(!notes.contains("Beta note"));
    assert!(relationship.contains(&format!(
        "Type=\"{OFFICE_RELATIONSHIPS}/{}\" Target=\"{}.xml\"",
        kind.relationship_name(),
        kind.part_name()
    )));
    assert!(content_types.contains(&format!("PartName=\"/word/{}.xml\"", kind.part_name())));
}

fn two_note_docx(kind: NoteKind, reference_ids: [u8; 2]) -> Vec<u8> {
    note_docx(kind, &reference_ids, &[(1, "Alpha note"), (2, "Beta note")])
}

fn single_note_docx(kind: NoteKind, reference_id: u8) -> Vec<u8> {
    note_docx(kind, &[reference_id], &[(1, "Alpha note")])
}

fn note_docx(kind: NoteKind, reference_ids: &[u8], definitions: &[(u8, &str)]) -> Vec<u8> {
    let notes_name = kind.part_name();
    let note_name = kind.note_name();
    let note_reference = kind.reference_name();
    let content_type =
        format!("application/vnd.openxmlformats-officedocument.wordprocessingml.{notes_name}+xml");
    let body_reference = |id| format!("<w:{note_reference} w:id=\"{id}\"/>");
    let body_references = reference_ids
        .iter()
        .map(|id| body_reference(id))
        .collect::<String>();
    let note_entries = definitions
        .iter()
        .map(|(id, text)| {
            format!(
                "<w:{note_name} w:id=\"{id}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:{note_name}>"
            )
        })
        .collect::<String>();

    let parts = BTreeMap::from([
        (
            "[Content_Types].xml".into(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/{notes_name}.xml" ContentType="{content_type}"/>
</Types>"#
            )
            .into_bytes(),
        ),
        (
            "_rels/.rels".into(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_RELATIONSHIPS}">
<Relationship Id="rIdOffice" Type="{OFFICE_RELATIONSHIPS}/officeDocument" Target="word/document.xml"/>
</Relationships>"#
            )
            .into_bytes(),
        ),
        (
            "word/_rels/document.xml.rels".into(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_RELATIONSHIPS}">
<Relationship Id="rIdNotes" Type="{OFFICE_RELATIONSHIPS}/{}" Target="{notes_name}.xml"/>
</Relationships>"#,
                kind.relationship_name()
            )
            .into_bytes(),
        ),
        (
            "word/document.xml".into(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORDPROCESSINGML}">
<w:body><w:p><w:r><w:t>same body text</w:t></w:r><w:r>{body_references}</w:r></w:p><w:sectPr/></w:body>
</w:document>"#,
            )
            .into_bytes(),
        ),
        (
            format!("word/{notes_name}.xml"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:{notes_name} xmlns:w="{WORDPROCESSINGML}">
<w:{note_name} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{note_name}>
<w:{note_name} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{note_name}>
{note_entries}
</w:{notes_name}>"#
            )
            .into_bytes(),
        ),
    ]);

    write_parts(parts)
}

fn assert_only_document_part_differs(before: &[u8], after: &[u8]) {
    let before_parts = read_parts(before);
    let after_parts = read_parts(after);
    assert_eq!(
        before_parts.keys().collect::<Vec<_>>(),
        after_parts.keys().collect::<Vec<_>>()
    );
    for (name, before_part) in &before_parts {
        if name == "word/document.xml" {
            assert_ne!(before_part, &after_parts[name]);
        } else {
            assert_eq!(
                before_part, &after_parts[name],
                "unexpected changed part: {name}"
            );
        }
    }
}

fn inspect(docx: &[u8]) -> Result<document_semantic_inspection_poc::AdapterOutput, PocError> {
    DocxAdapter.inspect(docx, &InspectionProfile::default())
}

fn error_summary<T>(result: Result<T, PocError>) -> Result<(), (ErrorCode, String)> {
    result
        .map(|_| ())
        .map_err(|error| (error.code(), error.to_string()))
}

fn read_parts(docx: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(docx)).expect("DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("DOCX entry");
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read DOCX entry");
        assert!(parts.insert(file.name().to_owned(), data).is_none());
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
