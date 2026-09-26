use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, SemanticAdapter};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const CHART_PART: &str = "ppt/charts/chart1.xml";
const GLOBAL_LABELS: &str = "<c:dLbls><c:showCatName val=\"1\"/></c:dLbls>";
const CATEGORY_ZERO_OVERRIDE: &str = "<c:dLbls><c:dLbl><c:idx val=\"0\"/><c:showCatName val=\"0\"/></c:dLbl><c:showCatName val=\"1\"/></c:dLbls>";

#[test]
fn pptx_per_point_false_override_changes_visible_category_labels() {
    let global_labels = package_with_data_labels(BASE, false);
    let category_zero_hidden = package_with_data_labels(BASE, true);

    assert_only_chart_xml_changed(BASE, &global_labels);
    assert_only_chart_xml_changed(BASE, &category_zero_hidden);
    assert_only_per_point_override_changed(&global_labels, &category_zero_hidden);
    validate_package(&global_labels);
    validate_package(&category_zero_hidden);

    let global_chart = String::from_utf8(read_part(&global_labels, CHART_PART))
        .expect("global-label chart XML is UTF-8");
    let overridden_chart = String::from_utf8(read_part(&category_zero_hidden, CHART_PART))
        .expect("per-point-label chart XML is UTF-8");
    assert!(global_chart.contains(GLOBAL_LABELS));
    assert!(overridden_chart.contains(CATEGORY_ZERO_OVERRIDE));

    let profile = AdapterProfile::default();
    let global_output = PptxAdapter
        .inspect(&global_labels, &profile)
        .expect("chart with all category labels visible should inspect");
    let overridden_output = PptxAdapter
        .inspect(&category_zero_hidden, &profile)
        .expect("chart with category zero hidden should inspect");
    assert_ne!(
        global_output.semantic_fingerprint(),
        overridden_output.semantic_fingerprint(),
        "a per-point false override hides a visible category label and changes chart semantics"
    );
}

fn package_with_data_labels(input: &[u8], hide_category_zero: bool) -> Vec<u8> {
    let mut entries = read_entries(input);
    let chart = entries
        .iter_mut()
        .find(|(name, _)| name == CHART_PART)
        .expect("base PPTX contains the chart part");
    let xml = std::str::from_utf8(&chart.1).expect("base chart XML is UTF-8");
    assert_eq!(xml.matches("</c:barChart>").count(), 1);
    assert!(
        !xml.contains("<c:dLbls>"),
        "base chart has no data-label settings"
    );

    let settings = if hide_category_zero {
        CATEGORY_ZERO_OVERRIDE
    } else {
        GLOBAL_LABELS
    };
    chart.1 = xml
        .replace("</c:barChart>", &format!("{settings}</c:barChart>"))
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

fn assert_only_per_point_override_changed(global: &[u8], overridden: &[u8]) {
    let global_chart = String::from_utf8(read_part(global, CHART_PART)).expect("global chart XML");
    let overridden_chart =
        String::from_utf8(read_part(overridden, CHART_PART)).expect("overridden chart XML");
    assert_eq!(
        global_chart.replace(GLOBAL_LABELS, CATEGORY_ZERO_OVERRIDE),
        overridden_chart,
        "only the idx=0 false override is added; global category labels remain enabled"
    );
}

fn validate_package(input: &[u8]) {
    let entries = read_entries(input);
    assert!(!entries.is_empty(), "PPTX package contains entries");
    for (name, contents) in entries {
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
