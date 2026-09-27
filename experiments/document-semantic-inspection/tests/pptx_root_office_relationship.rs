use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CONTENT_TYPES: &str = "[Content_Types].xml";
const ROOT_RELATIONSHIPS: &str = "_rels/.rels";
const OFFICE_DOCUMENT_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

#[test]
fn missing_package_root_relationships_fail_closed() {
    assert_baseline_accepted();

    let mutant = without_root_relationships();
    assert_valid_package(&mutant);
    assert_only_root_relationships_removed(BASE, &mutant);

    assert_rejected(&mutant, "missing package root relationships");
}

#[test]
fn package_root_office_document_relationship_must_target_presentation_part() {
    assert_baseline_accepted();

    let mutant = mutate_root_relationships(|xml| {
        replace_once(
            xml,
            "Target=\"ppt/presentation.xml\"",
            "Target=\"ppt/slides/slide1.xml\"",
        )
    });
    assert_valid_package(&mutant);
    assert_only_root_relationships_changed(BASE, &mutant);

    assert_rejected(
        &mutant,
        "officeDocument relationship targeting a slide part",
    );
}

#[test]
fn duplicate_package_root_office_document_relationships_fail_closed() {
    assert_baseline_accepted();

    let mutant = mutate_root_relationships(|xml| {
        let duplicate = format!(
            "<Relationship Id=\"rId2\" Type=\"{OFFICE_DOCUMENT_RELATIONSHIP}\" Target=\"ppt/presentation.xml\"/>"
        );
        replace_once(
            xml,
            "</Relationships>",
            &format!("{duplicate}</Relationships>"),
        )
    });
    assert_valid_package(&mutant);
    assert_only_root_relationships_changed(BASE, &mutant);

    assert_rejected(&mutant, "duplicate officeDocument root relationships");
}

#[test]
fn character_reference_serialization_of_content_type_preserves_fingerprint() {
    assert_baseline_accepted();

    let baseline = inspect(BASE, "qualified PPTX baseline must inspect");
    let mutant = with_character_reference_content_type();
    assert_valid_package(&mutant);
    assert_only_named_part_changed(BASE, &mutant, CONTENT_TYPES);

    let changed = inspect(
        &mutant,
        "XML character reference in a ContentType attribute must inspect",
    );
    assert_eq!(
        fingerprint(&baseline.semantic_projection),
        fingerprint(&changed.semantic_projection),
        "an XML character reference is serialization noise in an attribute value"
    );
}

fn assert_baseline_accepted() {
    PptxAdapter
        .inspect(BASE, &InspectionProfile::default())
        .expect("qualified PPTX baseline must inspect");
}

fn assert_rejected(bytes: &[u8], description: &str) {
    match PptxAdapter.inspect(bytes, &InspectionProfile::default()) {
        Err(error) => assert!(
            matches!(
                error.code(),
                ErrorCode::SemanticExtractionFailed | ErrorCode::UnsupportedSemanticConstruct
            ),
            "{description} must fail with a controlled package error, got {error}"
        ),
        Ok(_) => panic!("{description} must fail closed but was accepted"),
    }
}

fn mutate_root_relationships(mutator: impl FnOnce(String) -> String) -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let (_, relationships) = parts
        .iter_mut()
        .find(|(name, _)| name == ROOT_RELATIONSHIPS)
        .expect("base PPTX has package root relationships");
    let xml = String::from_utf8(relationships.clone()).expect("root relationships are UTF-8");
    *relationships = mutator(xml).into_bytes();
    write_parts(parts)
}

fn without_root_relationships() -> Vec<u8> {
    let parts = read_parts(BASE)
        .into_iter()
        .filter(|(name, _)| name != ROOT_RELATIONSHIPS)
        .collect();
    write_parts(parts)
}

fn with_character_reference_content_type() -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let (_, content_types) = parts
        .iter_mut()
        .find(|(name, _)| name == CONTENT_TYPES)
        .expect("base PPTX has content types");
    let xml = String::from_utf8(content_types.clone()).expect("content types are UTF-8");
    *content_types = replace_once(
        xml,
        "presentationml.presentation.main+xml",
        "presentatio&#x6e;ml.presentation.main+xml",
    )
    .into_bytes();
    write_parts(parts)
}

fn replace_once(value: String, from: &str, to: &str) -> String {
    assert_eq!(value.matches(from).count(), 1, "mutation target is unique");
    value.replacen(from, to, 1)
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("PPTX is a ZIP archive");
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

fn assert_only_root_relationships_changed(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len());

    for ((baseline_name, baseline_contents), (mutant_name, mutant_contents)) in
        baseline_parts.iter().zip(&mutant_parts)
    {
        assert_eq!(baseline_name, mutant_name, "package part order changed");
        if baseline_name == ROOT_RELATIONSHIPS {
            assert_ne!(
                baseline_contents, mutant_contents,
                "root relationships unchanged"
            );
        } else {
            assert_eq!(
                baseline_contents, mutant_contents,
                "unrelated package part {baseline_name} changed"
            );
        }
    }
}

fn assert_only_named_part_changed(baseline: &[u8], mutant: &[u8], changed_part: &str) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len());

    for ((baseline_name, baseline_contents), (mutant_name, mutant_contents)) in
        baseline_parts.iter().zip(&mutant_parts)
    {
        assert_eq!(baseline_name, mutant_name, "package part order changed");
        if baseline_name == changed_part {
            assert_ne!(
                baseline_contents, mutant_contents,
                "{changed_part} unchanged"
            );
        } else {
            assert_eq!(
                baseline_contents, mutant_contents,
                "unrelated package part {baseline_name} changed"
            );
        }
    }
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn assert_only_root_relationships_removed(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len() + 1);
    let expected = baseline_parts
        .into_iter()
        .filter(|(name, _)| name != ROOT_RELATIONSHIPS)
        .collect::<Vec<_>>();
    assert_eq!(expected, mutant_parts, "only _rels/.rels may be removed");
}

fn assert_valid_package(bytes: &[u8]) {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("mutant remains a valid ZIP archive");
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("mutant central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("mutant ZIP entry has a valid CRC");
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(&contents).expect("mutant XML remains UTF-8");
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
