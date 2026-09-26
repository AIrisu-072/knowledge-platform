use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PptxAdapter,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CONTENT_TYPES: &str = "[Content_Types].xml";
const SPOOFED_PART: &str = "customXml/item1.xml";
const SPOOFED_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";

#[test]
fn unmodeled_custom_xml_cannot_hide_behind_a_known_semantic_content_type() {
    PptxAdapter
        .inspect(BASE, &InspectionProfile::default())
        .expect("qualified PPTX baseline must inspect");

    let mutant = package_with_known_type_spoof();
    assert_only_content_types_and_new_part_differ(BASE, &mutant);
    assert_valid_pptx_package(&mutant);

    let content_types = String::from_utf8(read_part(&mutant, CONTENT_TYPES))
        .expect("content-types XML remains UTF-8");
    assert!(content_types.contains(&format!(
        "PartName=\"/{SPOOFED_PART}\" ContentType=\"{SPOOFED_CONTENT_TYPE}\""
    )));
    assert!(
        read_part(&mutant, SPOOFED_PART)
            .windows(b"approvalRequirement".len())
            .any(|window| window == b"approvalRequirement")
    );

    let error = match PptxAdapter.inspect(&mutant, &InspectionProfile::default()) {
        Ok(_) => {
            panic!("unmodeled package part with a known semantic content type must fail closed")
        }
        Err(error) => error,
    };
    assert_eq!(error.code(), ErrorCode::UnsupportedSemanticConstruct);
}

fn package_with_known_type_spoof() -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let content_types = parts
        .iter_mut()
        .find(|(name, _)| name == CONTENT_TYPES)
        .expect("base PPTX has content types");
    let xml = String::from_utf8(content_types.1.clone()).expect("base XML is UTF-8");
    assert!(xml.contains("<Types "));
    assert!(xml.contains("</Types>"));
    let override_xml =
        format!("<Override PartName=\"/{SPOOFED_PART}\" ContentType=\"{SPOOFED_CONTENT_TYPE}\"/>");
    content_types.1 = xml
        .replace("</Types>", &format!("{override_xml}</Types>"))
        .into_bytes();

    assert!(!parts.iter().any(|(name, _)| name == SPOOFED_PART));
    parts.push((
        SPOOFED_PART.to_owned(),
        br#"<?xml version="1.0" encoding="UTF-8"?><businessData xmlns="urn:example:business-data"><approvalRequirement>Approval condition</approvalRequirement></businessData>"#.to_vec(),
    ));
    write_parts(parts)
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("qualified base is a ZIP archive");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("ZIP entry reads with a valid CRC");
        parts.push((name, contents));
    }
    parts
}

fn read_part(archive: &[u8], part_name: &str) -> Vec<u8> {
    read_parts(archive)
        .into_iter()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing ZIP part {part_name}"))
        .1
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

fn assert_only_content_types_and_new_part_differ(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(mutant_parts.len(), baseline_parts.len() + 1);

    let baseline_names = baseline_parts
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    let mutant_existing_names = mutant_parts[..baseline_parts.len()]
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(baseline_names, mutant_existing_names);
    assert_eq!(mutant_parts.last().unwrap().0, SPOOFED_PART);

    for ((name, original), (_, changed)) in baseline_parts.iter().zip(&mutant_parts) {
        if name != CONTENT_TYPES {
            assert_eq!(original, changed, "unrelated PPTX part {name} changed");
        }
    }
}

fn assert_valid_pptx_package(bytes: &[u8]) {
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
