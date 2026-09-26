use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, PptxAdapter, SemanticAdapter, WorkerFailureCode,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const CONTENT_TYPES: &str = "[Content_Types].xml";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const PACKAGE_RELATIONSHIPS: &str = "_rels/.rels";
const PACKAGE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";

#[test]
fn content_types_root_with_wrong_namespace_fails_closed() {
    assert_baseline_accepted();

    let mutant = mutate_part(CONTENT_TYPES, |xml| {
        replace_once(
            xml,
            &format!("<Types xmlns=\"{CONTENT_TYPES_NS}\""),
            "<Types xmlns=\"urn:example:wrong-content-types\"",
        )
    });
    assert_only_part_changed(BASE, &mutant, CONTENT_TYPES);
    assert_valid_pptx_package(&mutant);

    assert_rejected(&mutant, "wrong-namespace content-types root");
}

#[test]
fn content_types_root_with_wrong_local_name_fails_closed() {
    assert_baseline_accepted();

    let mutant = mutate_part(CONTENT_TYPES, |xml| {
        let xml = replace_once(xml, "<Types ", "<NotTypes ");
        replace_once(xml, "</Types>", "</NotTypes>")
    });
    assert_only_part_changed(BASE, &mutant, CONTENT_TYPES);
    assert_valid_pptx_package(&mutant);

    assert_rejected(&mutant, "wrong-local-name content-types root");
}

#[test]
fn package_relationships_root_with_wrong_namespace_fails_closed() {
    assert_baseline_accepted();

    let mutant = mutate_part(PACKAGE_RELATIONSHIPS, |xml| {
        replace_once(
            xml,
            &format!("<Relationships xmlns=\"{PACKAGE_RELATIONSHIPS_NS}\""),
            "<Relationships xmlns=\"urn:example:wrong-package-relationships\"",
        )
    });
    assert_only_part_changed(BASE, &mutant, PACKAGE_RELATIONSHIPS);
    assert_valid_pptx_package(&mutant);

    assert_rejected(&mutant, "wrong-namespace package relationships root");
}

#[test]
fn package_relationships_root_with_wrong_local_name_fails_closed() {
    assert_baseline_accepted();

    let mutant = mutate_part(PACKAGE_RELATIONSHIPS, |xml| {
        let xml = replace_once(xml, "<Relationships ", "<NotRelationships ");
        replace_once(xml, "</Relationships>", "</NotRelationships>")
    });
    assert_only_part_changed(BASE, &mutant, PACKAGE_RELATIONSHIPS);
    assert_valid_pptx_package(&mutant);

    assert_rejected(&mutant, "wrong-local-name package relationships root");
}

fn assert_baseline_accepted() {
    PptxAdapter
        .inspect(BASE, &AdapterProfile::default())
        .expect("qualified base PPTX must inspect");
}

fn assert_rejected(bytes: &[u8], description: &str) {
    match PptxAdapter.inspect(bytes, &AdapterProfile::default()) {
        Err(error) => assert!(
            matches!(
                error.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct
                    | WorkerFailureCode::SemanticExtractionFailed
            ),
            "{description} must fail with a package/XML error, got {:?}: {error}",
            error.code()
        ),
        Ok(_) => panic!("{description} must fail closed but was accepted"),
    }
}

fn mutate_part(part_name: &str, mutator: impl FnOnce(String) -> String) -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let (_, target_part) = parts
        .iter_mut()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("base PPTX contains {part_name}"));
    let xml = String::from_utf8(target_part.clone()).expect("target XML part is UTF-8");
    *target_part = mutator(xml).into_bytes();
    write_parts(parts)
}

fn replace_once(value: String, from: &str, to: &str) -> String {
    assert_eq!(value.matches(from).count(), 1, "mutation target is unique");
    value.replacen(from, to, 1)
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("base is a ZIP archive");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("ZIP part reads with a valid CRC");
        parts.push((name, contents));
    }
    parts
}

fn write_parts(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("mutant ZIP part starts");
        writer.write_all(&contents).expect("mutant ZIP part writes");
    }
    writer.finish().expect("mutant ZIP finishes").into_inner()
}

fn assert_only_part_changed(baseline: &[u8], mutant: &[u8], target: &str) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len());
    for ((baseline_name, baseline_data), (mutant_name, mutant_data)) in
        baseline_parts.iter().zip(&mutant_parts)
    {
        assert_eq!(
            baseline_name, mutant_name,
            "ZIP entry order and names are stable"
        );
        if baseline_name == target {
            assert_ne!(baseline_data, mutant_data, "target XML part changed");
        } else {
            assert_eq!(
                baseline_data, mutant_data,
                "unrelated part {baseline_name} changed"
            );
        }
    }
}

fn assert_valid_pptx_package(bytes: &[u8]) {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("mutant remains a valid ZIP");
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("mutant central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("mutant ZIP entry has a valid CRC");
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(&contents).expect("mutant XML is UTF-8");
            let mut reader = Reader::from_str(xml);
            reader.config_mut().check_end_names = true;
            loop {
                match reader.read_event() {
                    Ok(Event::Eof) => break,
                    Ok(_) => {}
                    Err(error) => panic!("mutant part {name} is malformed XML: {error}"),
                }
            }
        }
    }
}
