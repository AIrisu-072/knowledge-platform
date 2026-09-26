use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, SemanticAdapter};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";
const BASELINE_TITLE_FORMULA: &str = "Data!$D$1";
const MUTANT_TITLE_FORMULA: &str = "Data!$E$1";
const TITLE_CACHE: &str = "<c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>Quarterly Revenue</c:v></c:pt></c:strCache>";

#[test]
fn chart_title_source_formula_changes_semantics_with_cached_title_unchanged() {
    inspect(BASE, "qualified PPTX fixture must be accepted");

    let baseline = package_with_title_reference(BASE, BASELINE_TITLE_FORMULA);
    let mutant = package_with_chart_title_formula_change(&baseline);

    assert_valid_package(&baseline);
    assert_valid_package(&mutant);
    assert_chart_title_cache_and_formula(&baseline, BASELINE_TITLE_FORMULA);
    assert_chart_title_cache_and_formula(&mutant, MUTANT_TITLE_FORMULA);
    assert_only_chart_xml_changed(&baseline, &mutant);

    let baseline = inspect(
        &baseline,
        "valid chart-title reference baseline must be accepted",
    );
    let changed = inspect(
        &mutant,
        "valid chart-title reference mutant must be accepted",
    );

    assert_ne!(
        baseline.semantic_fingerprint(),
        changed.semantic_fingerprint(),
        "changing the source range of a chart title changes its meaning even when cached title text stays fixed"
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

fn package_with_title_reference(input: &[u8], formula: &str) -> Vec<u8> {
    rewrite_chart(input, |xml| {
        let title = format!(
            "<c:title><c:tx><c:strRef><c:f>{formula}</c:f>{TITLE_CACHE}</c:strRef></c:tx><c:overlay val=\"0\"/></c:title>"
        );
        replace_once(xml, "<c:chart>", &format!("<c:chart>{title}"))
    })
}

fn package_with_chart_title_formula_change(input: &[u8]) -> Vec<u8> {
    rewrite_chart(input, |xml| {
        replace_once(
            xml,
            &format!("<c:f>{BASELINE_TITLE_FORMULA}</c:f>"),
            &format!("<c:f>{MUTANT_TITLE_FORMULA}</c:f>"),
        )
    })
}

fn assert_chart_title_cache_and_formula(input: &[u8], formula: &str) {
    let xml = String::from_utf8(read_part(input, CHART)).expect("chart XML is UTF-8");
    assert!(xml.contains(&format!("<c:f>{formula}</c:f>")));
    assert!(xml.contains(TITLE_CACHE));
    assert!(xml.contains("<c:v>Quarterly Revenue</c:v>"));
}

fn replace_once(input: &str, from: &str, to: &str) -> String {
    assert_eq!(
        input.matches(from).count(),
        1,
        "mutation target occurs once"
    );
    input.replacen(from, to, 1)
}

fn rewrite_chart(input: &[u8], rewrite: impl FnOnce(&str) -> String) -> Vec<u8> {
    let mut entries = read_entries(input);
    let chart = entries
        .iter_mut()
        .find(|(name, _)| name == CHART)
        .unwrap_or_else(|| panic!("missing ZIP part {CHART}"));
    let xml = std::str::from_utf8(&chart.1).expect("chart fixture is UTF-8");
    chart.1 = rewrite(xml).into_bytes();
    write_entries(entries)
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
            .expect("ZIP entry reads with a valid CRC");
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
            .expect("mutant ZIP part starts");
        writer.write_all(&contents).expect("mutant ZIP part writes");
    }
    writer.finish().expect("mutant ZIP finishes").into_inner()
}

fn assert_valid_package(input: &[u8]) {
    let entries = read_entries(input);
    assert!(!entries.is_empty(), "mutant PPTX contains package entries");
    for (name, contents) in entries {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(&contents).expect("PPTX XML remains UTF-8");
            let mut reader = Reader::from_str(xml);
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
        "chart-title formula mutant keeps package entries in the same order"
    );

    let changed_parts = baseline_entries
        .iter()
        .zip(&mutant_entries)
        .filter_map(|((name, before), (_, after))| (before != after).then_some(name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(changed_parts, [CHART]);
}
