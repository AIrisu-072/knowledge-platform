use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, SemanticAdapter};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";

#[test]
fn chart_value_point_indices_preserve_values_but_change_category_association() {
    let baseline = inspect(BASE, "qualified PPTX baseline must inspect");
    let mutant = package_with_swapped_value_point_indices(BASE);

    assert_only_chart_xml_changed(BASE, &mutant);
    assert_valid_package(&mutant);
    assert_chart_cache_association(BASE, ["Q1", "Q2"], ["10", "20"]);
    assert_chart_cache_association(&mutant, ["Q1", "Q2"], ["20", "10"]);

    let changed = inspect(
        &mutant,
        "valid PPTX with swapped chart-value point indices must inspect",
    );
    assert_ne!(
        baseline.semantic_fingerprint(),
        changed.semantic_fingerprint(),
        "swapping value point indices changes the values associated with chart categories"
    );
}

fn inspect(
    bytes: &[u8],
    message: &str,
) -> document_semantic_inspection_worker::SemanticAdapterOutput {
    PptxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn package_with_swapped_value_point_indices(input: &[u8]) -> Vec<u8> {
    let mut entries = read_entries(input);
    let chart = entries
        .iter_mut()
        .find(|(name, _)| name == CHART)
        .expect("base PPTX contains the chart cache");
    let xml = std::str::from_utf8(&chart.1).expect("chart fixture is UTF-8");
    let start = xml.find("<c:val>").expect("chart has value cache") + "<c:val>".len();
    let end = xml[start..]
        .find("</c:val>")
        .map(|offset| start + offset)
        .expect("chart value cache closes");
    let value_cache = &xml[start..end];
    assert_eq!(value_cache.matches("<c:pt idx=\"0\">").count(), 1);
    assert_eq!(value_cache.matches("<c:pt idx=\"1\">").count(), 1);
    assert!(value_cache.contains("<c:v>10</c:v>"));
    assert!(value_cache.contains("<c:v>20</c:v>"));

    let swapped = value_cache
        .replace("<c:pt idx=\"0\">", "<c:pt idx=\"swap\">")
        .replace("<c:pt idx=\"1\">", "<c:pt idx=\"0\">")
        .replace("<c:pt idx=\"swap\">", "<c:pt idx=\"1\">");
    let mut changed_xml = String::with_capacity(xml.len());
    changed_xml.push_str(&xml[..start]);
    changed_xml.push_str(&swapped);
    changed_xml.push_str(&xml[end..]);
    chart.1 = changed_xml.into_bytes();

    write_entries(entries)
}

fn assert_chart_cache_association(bytes: &[u8], categories: [&str; 2], values: [&str; 2]) {
    let chart = String::from_utf8(read_part(bytes, CHART)).expect("chart XML is UTF-8");
    let category_cache = chart
        .split_once("<c:cat>")
        .and_then(|(_, suffix)| suffix.split_once("</c:cat>"))
        .map(|(cache, _)| cache)
        .expect("chart has category cache");
    let value_cache = chart
        .split_once("<c:val>")
        .and_then(|(_, suffix)| suffix.split_once("</c:val>"))
        .map(|(cache, _)| cache)
        .expect("chart has value cache");

    assert!(category_cache.contains(&format!(
        "<c:pt idx=\"0\"><c:v>{}</c:v></c:pt>",
        categories[0]
    )));
    assert!(category_cache.contains(&format!(
        "<c:pt idx=\"1\"><c:v>{}</c:v></c:pt>",
        categories[1]
    )));
    assert!(value_cache.contains(&format!("<c:pt idx=\"0\"><c:v>{}</c:v></c:pt>", values[0])));
    assert!(value_cache.contains(&format!("<c:pt idx=\"1\"><c:v>{}</c:v></c:pt>", values[1])));
}

fn read_entries(input: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("qualified PPTX is a ZIP");
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .expect("ZIP central-directory entry");
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .expect("ZIP part reads with valid CRC");
        entries.push((name, bytes));
    }
    entries
}

fn read_part(input: &[u8], part_name: &str) -> Vec<u8> {
    read_entries(input)
        .into_iter()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing ZIP part {part_name}"))
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
        "mutant keeps package entries and their order"
    );

    let mut changed_parts = Vec::new();
    for ((name, original), (_, changed)) in baseline_entries.iter().zip(&mutant_entries) {
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
                    Err(error) => panic!("mutant part {name} has malformed XML: {error}"),
                }
            }
        }
    }
    assert_eq!(changed_parts, [CHART]);
}

fn assert_valid_package(input: &[u8]) {
    let entries = read_entries(input);
    assert!(!entries.is_empty(), "mutant ZIP contains package entries");
}
