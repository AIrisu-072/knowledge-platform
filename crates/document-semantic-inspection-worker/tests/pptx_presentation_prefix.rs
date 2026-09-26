use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, SemanticAdapter};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const PRESENTATION_XML: &str = "ppt/presentation.xml";
const OFFICE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

#[test]
fn presentation_relationship_prefix_rename_is_serialization_noise() {
    let base = PptxAdapter
        .inspect(BASE, &AdapterProfile::default())
        .expect("qualified base.pptx remains a valid semantic baseline");
    let mutated = rename_presentation_relationship_prefix(BASE);

    assert_only_presentation_xml_changed(BASE, &mutated);

    let variant = PptxAdapter
        .inspect(&mutated, &AdapterProfile::default())
        .expect("a namespace-prefix-only PresentationML serialization remains valid");
    assert_eq!(
        variant.semantic_fingerprint(),
        base.semantic_fingerprint(),
        "renaming the relationship namespace prefix must preserve PPTX semantic identity"
    );
}

fn rename_presentation_relationship_prefix(input: &[u8]) -> Vec<u8> {
    let mut entries = read_entries(input);
    let presentation = entries
        .iter_mut()
        .find(|(name, _)| name == PRESENTATION_XML)
        .expect("qualified PPTX contains ppt/presentation.xml");
    let xml = String::from_utf8(presentation.1.clone()).expect("PresentationML is UTF-8");
    let old_declaration = format!(r#"xmlns:r="{OFFICE_RELATIONSHIPS_NS}""#);
    let new_declaration = format!(r#"xmlns:rel="{OFFICE_RELATIONSHIPS_NS}""#);
    assert_eq!(xml.matches(&old_declaration).count(), 1);
    assert!(xml.contains(" r:id="), "fixture has slide relationship IDs");

    let renamed = xml
        .replace(&old_declaration, &new_declaration)
        .replace(" r:id=", " rel:id=");
    assert!(!renamed.contains(" r:id="));
    assert!(renamed.contains(" rel:id="));
    presentation.1 = renamed.into_bytes();

    write_entries(entries)
}

fn read_entries(input: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("qualified PPTX ZIP");
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("central-directory entry");
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .expect("read qualified package entry");
        entries.push((name, bytes));
    }
    entries
}

fn write_entries(entries: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries {
        writer
            .start_file(name, options)
            .expect("write synthetic PPTX entry");
        writer
            .write_all(&contents)
            .expect("write synthetic PPTX contents");
    }
    writer
        .finish()
        .expect("finish synthetic PPTX ZIP")
        .into_inner()
}

fn assert_only_presentation_xml_changed(before: &[u8], after: &[u8]) {
    let before = read_entries(before);
    let after = read_entries(after);
    assert_eq!(before.len(), after.len(), "entry count is unchanged");
    for ((before_name, before_bytes), (after_name, after_bytes)) in before.iter().zip(&after) {
        assert_eq!(
            before_name, after_name,
            "entry order and names are unchanged"
        );
        if before_name == PRESENTATION_XML {
            assert_ne!(before_bytes, after_bytes, "target XML prefix was renamed");
        } else {
            assert_eq!(
                before_bytes, after_bytes,
                "only the target XML part changes"
            );
        }
    }
}
