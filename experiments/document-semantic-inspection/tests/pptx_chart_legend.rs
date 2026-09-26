use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";
const LEGEND: &str = "<c:legend><c:legendPos val=\"r\"/></c:legend>";

#[test]
fn adding_a_visible_series_legend_changes_chart_semantics() {
    let mutant = package_with_legend();
    assert_only_chart_xml_changed(BASE, &mutant);

    let baseline_output = inspect(BASE, "qualified baseline PPTX must inspect");
    let mutant_output = inspect(&mutant, "PPTX with a visible chart legend must inspect");

    assert_ne!(
        fingerprint(&baseline_output.semantic_projection),
        fingerprint(&mutant_output.semantic_projection),
        "adding a visible legend for Series A changes reader-visible chart labels"
    );
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn package_with_legend() -> Vec<u8> {
    replace_part(BASE, CHART, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("base chart XML is UTF-8");
        assert!(xml.contains("<c:v>Series A</c:v>"));
        assert!(!xml.contains("<c:legend"), "base chart has no legend");
        assert!(xml.contains("</c:plotArea></c:chart>"));
        xml.replace(
            "</c:plotArea></c:chart>",
            &format!("</c:plotArea>{LEGEND}</c:chart>"),
        )
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
        "the legend mutant keeps the same package entries in the same order"
    );

    let mut changed_parts = Vec::new();
    for ((name, original), (_, changed)) in baseline_parts.iter().zip(&mutant_parts) {
        if original != changed {
            changed_parts.push(name.as_str());
        }
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(changed).expect("mutant XML remains UTF-8");
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
    assert_eq!(changed_parts, [CHART]);

    let chart = String::from_utf8(read_part(mutant, CHART)).expect("mutant chart XML is UTF-8");
    assert!(chart.contains(LEGEND));
    assert!(chart.contains("<c:v>Series A</c:v>"));
}

fn read_part(archive: &[u8], part_name: &str) -> Vec<u8> {
    read_parts(archive)
        .into_iter()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing ZIP part {part_name}"))
        .1
}
