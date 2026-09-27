use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PptxAdapter,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";
const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const EXTENSION_URI: &str = "urn:vendor:test";

#[test]
fn unknown_chart_extension_is_not_interpreted_as_a_chartml_title() {
    PptxAdapter
        .inspect(BASE, &InspectionProfile::default())
        .expect("qualified baseline PPTX must inspect");

    let mutant = chart_with_foreign_extension();
    assert_valid_package(&mutant);
    assert_only_chart_xml_changed(BASE, &mutant);

    let chart = String::from_utf8(read_part(&mutant, CHART)).expect("chart XML is UTF-8");
    assert!(chart.contains(&format!("xmlns:c=\"{CHART_NS}\"")));
    assert!(chart.contains(&format!("<c:ext uri=\"{EXTENSION_URI}\">")));
    assert!(chart.contains("<x:title xmlns:x=\"urn:vendor:test\"><x:v>Decoy</x:v></x:title>"));

    let error = match PptxAdapter.inspect(&mutant, &InspectionProfile::default()) {
        Ok(_) => panic!("unknown chart extension was accepted or interpreted as a ChartML title"),
        Err(error) => error,
    };
    assert_eq!(error.code(), ErrorCode::UnsupportedSemanticConstruct);
}

fn chart_with_foreign_extension() -> Vec<u8> {
    replace_part(BASE, CHART, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("base chart XML is UTF-8");
        assert!(xml.contains("</c:plotArea></c:chart>"));
        let extension = concat!(
            "</c:plotArea><c:extLst><c:ext uri=\"urn:vendor:test\">",
            "<x:title xmlns:x=\"urn:vendor:test\"><x:v>Decoy</x:v></x:title>",
            "</c:ext></c:extLst></c:chart>"
        );
        xml.replace("</c:plotArea></c:chart>", extension)
            .into_bytes()
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

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("base is a ZIP archive");
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

fn assert_valid_package(bytes: &[u8]) {
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

fn assert_only_chart_xml_changed(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(
        baseline_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        mutant_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        "the mutant keeps the package entries in the same order"
    );

    let changed_parts: Vec<_> = baseline_parts
        .iter()
        .zip(&mutant_parts)
        .filter_map(|((name, original), (_, changed))| {
            (original != changed).then_some(name.as_str())
        })
        .collect();
    assert_eq!(changed_parts, [CHART]);
}
