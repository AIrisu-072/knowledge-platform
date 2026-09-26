use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, SemanticAdapter};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const CHART_PART: &str = "ppt/charts/chart1.xml";
const LEGEND: &str = "<c:legend><c:legendPos val=\"r\"/></c:legend>";

#[test]
fn pptx_visible_chart_series_legend_changes_version_identity() {
    let with_legend = package_with_visible_legend(BASE);

    assert_only_chart_xml_changed(BASE, &with_legend);
    validate_package(&with_legend);

    let chart = String::from_utf8(read_part(&with_legend, CHART_PART)).expect("chart XML is UTF-8");
    assert!(chart.contains("<c:v>Series A</c:v>"));
    assert!(
        chart.contains(LEGEND),
        "right-side legend is visible by default"
    );

    let profile = AdapterProfile::default();
    let baseline = PptxAdapter
        .inspect(BASE, &profile)
        .expect("base chart should inspect");
    let changed = PptxAdapter
        .inspect(&with_legend, &profile)
        .expect("chart with visible legend should inspect");
    assert_ne!(
        baseline.semantic_fingerprint(),
        changed.semantic_fingerprint(),
        "a visible Series A legend label is semantic under Frozen Design §9.4"
    );
}

fn package_with_visible_legend(input: &[u8]) -> Vec<u8> {
    let mut entries = read_entries(input);
    let chart = entries
        .iter_mut()
        .find(|(name, _)| name == CHART_PART)
        .expect("base PPTX contains the chart part");
    let xml = std::str::from_utf8(&chart.1).expect("chart fixture is UTF-8");
    assert_eq!(xml.matches("</c:plotArea>").count(), 1);
    assert!(!xml.contains("<c:legend>"), "base chart has no legend");
    chart.1 = xml
        .replace("</c:plotArea>", &format!("</c:plotArea>{LEGEND}"))
        .into_bytes();
    write_entries(entries)
}

fn assert_only_chart_xml_changed(baseline: &[u8], mutant: &[u8]) {
    let baseline_entries = read_entries(baseline);
    let mutant_entries = read_entries(mutant);
    assert_eq!(
        baseline_entries
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        mutant_entries
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        "mutant preserves package entries and their order"
    );

    let changed_parts = baseline_entries
        .iter()
        .zip(&mutant_entries)
        .filter_map(|((name, original), (_, changed))| {
            (original != changed).then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(changed_parts, [CHART_PART]);
}

fn validate_package(input: &[u8]) {
    for (name, contents) in read_entries(input) {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let mut reader = Reader::from_reader(contents.as_slice());
            reader.config_mut().check_end_names = true;
            loop {
                match reader.read_event() {
                    Ok(Event::Eof) => break,
                    Ok(_) => {}
                    Err(error) => panic!("PPTX part {name} is malformed XML: {error}"),
                }
            }
        }
    }
}

fn read_entries(input: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("valid PPTX ZIP");
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("valid ZIP central entry");
        let name = entry.name().to_owned();
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .expect("PPTX entry reads with valid CRC");
        entries.push((name, contents));
    }
    entries
}

fn read_part(input: &[u8], name: &str) -> Vec<u8> {
    read_entries(input)
        .into_iter()
        .find(|(entry_name, _)| entry_name == name)
        .unwrap_or_else(|| panic!("missing PPTX part {name}"))
        .1
}

fn write_entries(entries: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries {
        writer
            .start_file(name, options)
            .expect("synthetic PPTX ZIP entry starts");
        writer
            .write_all(&contents)
            .expect("synthetic PPTX ZIP entry writes");
    }
    writer
        .finish()
        .expect("synthetic PPTX ZIP finishes")
        .into_inner()
}
