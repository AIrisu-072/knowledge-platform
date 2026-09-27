use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";
const TITLE_CACHE: &str = "Revenue by quarter";

#[test]
fn chart_title_cache_label_changes_are_version_significant() {
    let baseline = package_with_chart_title(TITLE_CACHE);
    let mutant = package_with_chart_title("Profit by quarter");
    assert_only_chart_xml_changed(&baseline, &mutant);

    let baseline_output = inspect(
        &baseline,
        "valid PPTX with a cached chart title must inspect",
    );
    let mutant_output = inspect(
        &mutant,
        "valid PPTX with a changed cached chart title must inspect",
    );

    assert_ne!(
        fingerprint(&baseline_output.semantic_projection),
        fingerprint(&mutant_output.semantic_projection),
        "changing the chart title cache label changes chart semantics"
    );
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn package_with_chart_title(title: &str) -> Vec<u8> {
    replace_part(BASE, CHART, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("chart fixture is UTF-8");
        assert!(xml.contains("<c:chart><c:plotArea>"));
        let title = format!(
            "<c:title><c:tx><c:strRef><c:f>Sheet1!$A$1</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{title}</c:v></c:pt></c:strCache></c:strRef></c:tx><c:overlay val=\"0\"/></c:title>"
        );
        xml.replace(
            "<c:chart><c:plotArea>",
            &format!("<c:chart>{title}<c:plotArea>"),
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
        "the chart-title mutant keeps the same package entries in the same order"
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
    assert!(String::from_utf8_lossy(&read_part(baseline, CHART)).contains(TITLE_CACHE));
    assert!(String::from_utf8_lossy(&read_part(mutant, CHART)).contains("Profit by quarter"));
}

fn read_part(archive: &[u8], part_name: &str) -> Vec<u8> {
    read_parts(archive)
        .into_iter()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing ZIP part {part_name}"))
        .1
}
