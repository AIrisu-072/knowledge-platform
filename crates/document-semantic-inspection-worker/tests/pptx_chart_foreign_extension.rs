use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, PptxAdapter, SemanticAdapter, WorkerFailureCode,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const CHART_PART: &str = "ppt/charts/chart1.xml";
const FOREIGN_EXTENSION: &str = "<c:extLst><c:ext uri=\"urn:vendor:test\"><x:title xmlns:x=\"urn:vendor:test\"><x:v>Decoy</x:v></x:title></c:ext></c:extLst>";

#[test]
fn unknown_chart_extension_with_foreign_title_fails_closed() {
    PptxAdapter
        .inspect(BASE, &AdapterProfile::default())
        .expect("qualified PPTX baseline must inspect");

    let mutant = with_foreign_chart_extension(BASE);
    assert_only_chart_part_differs(BASE, &mutant);
    assert_valid_pptx_package(&mutant);

    let chart = String::from_utf8(read_part(&mutant, CHART_PART)).expect("chart XML remains UTF-8");
    assert!(chart.contains(FOREIGN_EXTENSION));
    assert!(chart.contains("<c:plotArea>"));
    assert!(chart.contains("<c:extLst>"));
    assert!(chart.contains("<x:title xmlns:x=\"urn:vendor:test\"><x:v>Decoy</x:v></x:title>"));

    let error = match PptxAdapter.inspect(&mutant, &AdapterProfile::default()) {
        Ok(_) => panic!("unknown chart extensions must fail closed"),
        Err(error) => error,
    };
    assert_eq!(
        error.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct,
        "a foreign x:title inside c:ext must not be mistaken for a semantic c:title"
    );
}

fn with_foreign_chart_extension(input: &[u8]) -> Vec<u8> {
    let mut entries = read_parts(input);
    let chart = entries
        .iter_mut()
        .find(|(name, _)| name == CHART_PART)
        .expect("qualified PPTX has its chart part");
    let xml = String::from_utf8(chart.1.clone()).expect("base chart XML is UTF-8");
    let chart_end = "</c:plotArea></c:chart>";
    assert_eq!(xml.matches(chart_end).count(), 1);
    chart.1 = xml
        .replace(
            chart_end,
            &format!("</c:plotArea>{FOREIGN_EXTENSION}</c:chart>"),
        )
        .into_bytes();
    write_parts(entries)
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("qualified PPTX is a ZIP archive");
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

fn assert_only_chart_part_differs(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len());

    for ((baseline_name, baseline_contents), (mutant_name, mutant_contents)) in
        baseline_parts.iter().zip(&mutant_parts)
    {
        assert_eq!(baseline_name, mutant_name, "PPTX part order/name changed");
        if baseline_name == CHART_PART {
            let original_chart =
                String::from_utf8(baseline_contents.clone()).expect("base chart XML is UTF-8");
            let changed_chart =
                String::from_utf8(mutant_contents.clone()).expect("mutant chart XML is UTF-8");
            assert_eq!(
                changed_chart.replace(FOREIGN_EXTENSION, ""),
                original_chart,
                "only the unknown extension payload may be added to the chart"
            );
        } else {
            assert_eq!(
                baseline_contents, mutant_contents,
                "unrelated PPTX part {baseline_name} changed"
            );
        }
    }
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
            let mut reader = Reader::from_reader(contents.as_slice());
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
