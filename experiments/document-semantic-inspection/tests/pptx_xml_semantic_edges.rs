use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const SMARTART_DATA: &str = "ppt/diagrams/data1.xml";
const CONTENT_TYPES: &str = "[Content_Types].xml";
const CUSTOM_XML: &str = "customXml/item1.xml";
const DRAWINGML_MAIN_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const GENERIC_XML_DEFAULT: &str = r#"<Default Extension="xml" ContentType="application/xml"/>"#;

#[test]
fn smartart_drawingml_prefix_noise_preserves_identity_and_text_changes_remain_significant() {
    let baseline = inspect(BASE, "qualified PPTX baseline must inspect");
    let baseline_fingerprint = fingerprint(&baseline.semantic_projection);

    let prefix_noise = smartart_with_alternate_prefix(None);
    assert_valid_pptx_package(&prefix_noise);
    let prefix_noise_output = inspect(
        &prefix_noise,
        "valid SmartArt namespace-prefix mutant must inspect",
    );
    let prefix_noise_fingerprint = fingerprint(&prefix_noise_output.semantic_projection);

    let changed_text = smartart_with_alternate_prefix(Some("Node B"));
    assert_valid_pptx_package(&changed_text);
    let changed_text_output = inspect(
        &changed_text,
        "valid alternate-prefix SmartArt text mutant must inspect",
    );
    let changed_text_fingerprint = fingerprint(&changed_text_output.semantic_projection);

    let prefix_noise_preserved = baseline_fingerprint == prefix_noise_fingerprint;
    let changed_text_is_significant = prefix_noise_fingerprint != changed_text_fingerprint;
    assert!(
        prefix_noise_preserved && changed_text_is_significant,
        "expected prefix-only equality and alternate-prefix text difference; got prefix-only equality={prefix_noise_preserved}, text difference={changed_text_is_significant}"
    );
}

#[test]
fn unknown_custom_xml_with_generic_content_type_fails_closed() {
    let _baseline = inspect(BASE, "qualified PPTX baseline must inspect");
    let content_types = read_part(BASE, CONTENT_TYPES);
    assert!(
        String::from_utf8_lossy(&content_types).contains(GENERIC_XML_DEFAULT),
        "the baseline gives customXml/item1.xml an effective generic XML content type"
    );

    let mutant = append_part(
        BASE,
        CUSTOM_XML,
        br#"<?xml version="1.0" encoding="UTF-8"?><businessData xmlns="urn:example:business-data"><value>Approval condition</value></businessData>"#,
    );
    assert_valid_pptx_package(&mutant);
    assert!(
        String::from_utf8_lossy(&read_part(&mutant, CONTENT_TYPES)).contains(GENERIC_XML_DEFAULT),
        "the custom XML part must remain covered by the generic application/xml default"
    );

    let error = match PptxAdapter.inspect(&mutant, &InspectionProfile::default()) {
        Ok(_) => panic!("an unknown potentially-semantic custom XML part was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), ErrorCode::UnsupportedSemanticConstruct);
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn smartart_with_alternate_prefix(replacement_text: Option<&str>) -> Vec<u8> {
    replace_part(BASE, SMARTART_DATA, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("SmartArt fixture is UTF-8");
        assert!(xml.contains("xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\""));
        assert!(xml.contains("<a:t>Node A</a:t>"));

        let rebound = xml
            .replace("xmlns:a=", "xmlns:drawing=")
            .replace("a:", "drawing:");
        assert!(rebound.contains(&format!("xmlns:drawing=\"{DRAWINGML_MAIN_NS}\"")));
        assert!(!rebound.contains("xmlns:a="));
        assert!(rebound.contains("<drawing:t>Node A</drawing:t>"));

        let rebound = if let Some(replacement_text) = replacement_text {
            rebound.replace("Node A", replacement_text)
        } else {
            rebound
        };
        rebound.into_bytes()
    })
}

fn replace_part(
    archive: &[u8],
    part_name: &str,
    transform: impl FnOnce(&[u8]) -> Vec<u8>,
) -> Vec<u8> {
    let mut parts = read_parts(archive);
    let part = parts
        .iter_mut()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing ZIP part {part_name}"));
    part.1 = transform(&part.1);
    write_parts(parts)
}

fn append_part(archive: &[u8], part_name: &str, contents: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(archive);
    assert!(!parts.iter().any(|(name, _)| name == part_name));
    parts.push((part_name.to_owned(), contents.to_vec()));
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
            .expect("qualified base ZIP entry reads with a valid CRC");
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

fn assert_valid_pptx_package(bytes: &[u8]) {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("mutant remains a valid ZIP archive");
    let mut names = Vec::with_capacity(zip.len());
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
        names.push(name);
    }
    assert!(names.iter().any(|name| name == "[Content_Types].xml"));
    assert!(names.iter().any(|name| name == "ppt/presentation.xml"));
}
