use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, SemanticAdapter};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const CONTENT_TYPES: &str = "[Content_Types].xml";
const CONTENT_TYPES_ANCHOR: &str = "/><Default Extension=\"png\" ContentType=\"image/png\"/>";

#[test]
fn content_types_inter_element_whitespace_character_reference_is_noise_same() {
    let profile = AdapterProfile::default();
    let baseline = PptxAdapter
        .inspect(BASE, &profile)
        .expect("qualified base PPTX must inspect")
        .semantic_fingerprint();

    let literal = package_with_content_types_whitespace(" ");
    let character_reference = package_with_content_types_whitespace("&#x20;");
    assert_only_part_changed(BASE, &literal, CONTENT_TYPES);
    assert_only_part_changed(BASE, &character_reference, CONTENT_TYPES);
    assert_valid_pptx_package(&literal);
    assert_valid_pptx_package(&character_reference);

    assert_eq!(
        decoded_character_data(&content_types_xml(&literal)),
        decoded_character_data(&content_types_xml(&character_reference)),
        "literal XML S and its numeric character reference have equal decoded content"
    );
    assert_eq!(decoded_character_data(&content_types_xml(&literal)), " ");

    let literal_fingerprint = PptxAdapter
        .inspect(&literal, &profile)
        .expect("literal inter-element XML whitespace must remain semantically neutral")
        .semantic_fingerprint();
    let reference_fingerprint = PptxAdapter
        .inspect(&character_reference, &profile)
        .expect(
            "character-referenced inter-element XML whitespace must remain semantically neutral",
        )
        .semantic_fingerprint();
    assert_eq!(
        literal_fingerprint, baseline,
        "literal inter-element XML whitespace must not change semantic identity"
    );
    assert_eq!(
        reference_fingerprint, baseline,
        "character-referenced inter-element XML whitespace must not change semantic identity"
    );
}

fn package_with_content_types_whitespace(whitespace: &str) -> Vec<u8> {
    mutate_part(CONTENT_TYPES, |xml| {
        let replacement =
            format!("/>{whitespace}<Default Extension=\"png\" ContentType=\"image/png\"/>");
        replace_once(xml, CONTENT_TYPES_ANCHOR, &replacement)
    })
}

fn content_types_xml(package: &[u8]) -> String {
    let mut archive = ZipArchive::new(Cursor::new(package)).expect("package is a ZIP archive");
    let mut file = archive
        .by_name(CONTENT_TYPES)
        .expect("package contains [Content_Types].xml");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("content-types XML is UTF-8 and CRC-valid");
    contents
}

fn decoded_character_data(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut decoded = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(text)) => decoded.push_str(text.as_ref()),
            Ok(Event::GeneralRef(reference)) => decoded.push(
                reference
                    .resolve_char_ref()
                    .expect("numeric XML character reference is valid")
                    .expect("fixture uses a numeric character reference"),
            ),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => panic!("content-types XML is malformed: {error}"),
        }
    }
    decoded
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
