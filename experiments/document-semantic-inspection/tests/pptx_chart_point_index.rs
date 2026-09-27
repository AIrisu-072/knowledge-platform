use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";
const VALUE_POINTS: &str =
    "<c:pt idx=\"0\"><c:v>10</c:v></c:pt><c:pt idx=\"1\"><c:v>20</c:v></c:pt>";
const REINDEXED_VALUE_POINTS: &str =
    "<c:pt idx=\"1\"><c:v>10</c:v></c:pt><c:pt idx=\"0\"><c:v>20</c:v></c:pt>";
const CATEGORY_POINTS: &str = concat!(
    "<c:cat><c:strLit><c:ptCount val=\"2\"/>",
    "<c:pt idx=\"0\"><c:v>Q1</c:v></c:pt>",
    "<c:pt idx=\"1\"><c:v>Q2</c:v></c:pt>",
    "</c:strLit></c:cat>"
);

#[test]
fn chart_value_point_indices_preserve_category_value_association() {
    let mutant = package_with_reindexed_chart_values();
    assert_only_chart_xml_changed(BASE, &mutant);

    let baseline_chart = String::from_utf8(read_part(BASE, CHART)).expect("chart XML is UTF-8");
    let mutant_chart = String::from_utf8(read_part(&mutant, CHART)).expect("chart XML is UTF-8");
    assert!(baseline_chart.contains(CATEGORY_POINTS));
    assert!(mutant_chart.contains(CATEGORY_POINTS));
    assert!(baseline_chart.contains(VALUE_POINTS));
    assert!(mutant_chart.contains(REINDEXED_VALUE_POINTS));

    let baseline = inspect(BASE, "base PPTX must inspect");
    let changed = inspect(&mutant, "reindexed chart PPTX must inspect");

    assert_ne!(
        fingerprint(&baseline.semantic_projection),
        fingerprint(&changed.semantic_projection),
        "changing chart point indices changes the category-to-value association and chart semantics"
    );
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn package_with_reindexed_chart_values() -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let chart = parts
        .iter_mut()
        .find(|(name, _)| name == CHART)
        .unwrap_or_else(|| panic!("missing ZIP part {CHART}"));
    let xml = std::str::from_utf8(&chart.1).expect("base chart fixture is UTF-8");
    assert_eq!(xml.matches(VALUE_POINTS).count(), 1);
    assert!(xml.contains("<c:val><c:numLit><c:ptCount val=\"2\"/>"));
    chart.1 = xml
        .replace(VALUE_POINTS, REINDEXED_VALUE_POINTS)
        .into_bytes();
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
            .expect("base ZIP entry reads with a valid CRC");
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
        "the chart-index mutant keeps package entries in the same order"
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
}
