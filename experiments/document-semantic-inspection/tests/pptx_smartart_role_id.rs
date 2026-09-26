use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const SMARTART_DATA: &str = "ppt/diagrams/data1.xml";

#[test]
fn smartart_point_role_changes_identity_or_fails_closed() {
    let baseline = inspect(BASE, "qualified PPTX baseline must inspect");
    let baseline_fingerprint = fingerprint(&baseline.semantic_projection);
    let original_xml = String::from_utf8(read_part(BASE, SMARTART_DATA))
        .expect("qualified SmartArt part is UTF-8");
    assert_eq!(original_xml.matches("<dgm:pt ").count(), 1);
    assert!(original_xml.contains("<dgm:pt modelId=\"1\">"));

    let mutant = replace_part(BASE, SMARTART_DATA, |bytes| {
        let xml = String::from_utf8(bytes.to_vec()).expect("SmartArt part is UTF-8");
        xml.replace(
            "<dgm:pt modelId=\"1\">",
            "<dgm:pt modelId=\"1\" type=\"asst\">",
        )
        .into_bytes()
    });
    assert_valid_pptx_package(&mutant);
    assert_only_smartart_data_differs(BASE, &mutant);

    match PptxAdapter.inspect(&mutant, &InspectionProfile::default()) {
        Ok(output) => assert_ne!(
            baseline_fingerprint,
            fingerprint(&output.semantic_projection),
            "a SmartArt point role change must affect identity"
        ),
        Err(error) => assert_eq!(
            error.code(),
            ErrorCode::UnsupportedSemanticConstruct,
            "an unmodeled SmartArt point role must fail closed"
        ),
    }
}

#[test]
fn duplicate_smartart_model_id_referenced_by_connection_fails_closed() {
    let _baseline = inspect(BASE, "qualified PPTX baseline must inspect");
    let original_xml = String::from_utf8(read_part(BASE, SMARTART_DATA))
        .expect("qualified SmartArt part is UTF-8");
    assert_eq!(original_xml.matches("modelId=\"1\"").count(), 1);
    assert!(!original_xml.contains("<dgm:cxn"));

    let mutant = replace_part(BASE, SMARTART_DATA, |bytes| {
        let xml = String::from_utf8(bytes.to_vec()).expect("SmartArt part is UTF-8");
        let with_duplicate = xml.replace(
            "</dgm:ptLst>",
            "<dgm:pt modelId=\"1\"><dgm:t><a:p><a:r><a:t>Node B</a:t></a:r></a:p></dgm:t></dgm:pt></dgm:ptLst>",
        );
        with_duplicate.replace(
            "</dgm:dataModel>",
            "<dgm:cxnLst><dgm:cxn modelId=\"c1\" type=\"parOf\" srcId=\"1\" destId=\"1\"/></dgm:cxnLst></dgm:dataModel>",
        )
        .into_bytes()
    });
    assert_valid_pptx_package(&mutant);
    assert_only_smartart_data_differs(BASE, &mutant);

    let mutant_xml = String::from_utf8(read_part(&mutant, SMARTART_DATA))
        .expect("mutant SmartArt part is UTF-8");
    assert_eq!(mutant_xml.matches("modelId=\"1\"").count(), 2);
    assert!(mutant_xml.contains("srcId=\"1\" destId=\"1\""));

    let error = match PptxAdapter.inspect(&mutant, &InspectionProfile::default()) {
        Ok(_) => panic!("a connection referencing duplicate SmartArt modelId values was accepted"),
        Err(error) => error,
    };
    assert!(
        matches!(
            error.code(),
            ErrorCode::SemanticExtractionFailed | ErrorCode::UnsupportedSemanticConstruct
        ),
        "ambiguous SmartArt identity must fail closed, got {:?}: {error}",
        error.code()
    );
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
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

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("PPTX is a ZIP archive");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("ZIP entry has valid CRC");
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

fn assert_only_smartart_data_differs(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len());
    let mut differing_parts = Vec::new();
    for ((baseline_name, baseline_contents), (mutant_name, mutant_contents)) in
        baseline_parts.iter().zip(&mutant_parts)
    {
        assert_eq!(baseline_name, mutant_name, "ZIP entry order is preserved");
        if baseline_contents != mutant_contents {
            differing_parts.push(baseline_name.as_str());
        }
    }
    assert_eq!(differing_parts, [SMARTART_DATA]);
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
