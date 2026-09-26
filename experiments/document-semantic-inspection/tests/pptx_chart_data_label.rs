use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";

#[test]
fn chart_category_data_label_visibility_changes_chart_semantics() {
    let hidden = package_with_category_labels("0");
    let visible = package_with_category_labels("1");

    assert_valid_package(&hidden);
    assert_valid_package(&visible);
    assert_only_chart_xml_changed(BASE, &hidden);
    assert_only_chart_xml_changed(BASE, &visible);
    assert_only_chart_xml_changed(&hidden, &visible);

    let hidden_xml = String::from_utf8(read_part(&hidden, CHART)).expect("chart XML is UTF-8");
    let visible_xml = String::from_utf8(read_part(&visible, CHART)).expect("chart XML is UTF-8");
    assert!(hidden_xml.contains("<c:dLbls><c:showCatName val=\"0\"/></c:dLbls>"));
    assert!(visible_xml.contains("<c:dLbls><c:showCatName val=\"1\"/></c:dLbls>"));

    let hidden_output = inspect(&hidden, "PPTX with category labels disabled must inspect");
    let visible_output = inspect(&visible, "PPTX with category labels enabled must inspect");

    assert_ne!(
        fingerprint(&hidden_output.semantic_projection),
        fingerprint(&visible_output.semantic_projection),
        "showing category names as chart data labels changes visible chart semantics"
    );
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn package_with_category_labels(value: &str) -> Vec<u8> {
    replace_part(BASE, CHART, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("base chart XML is UTF-8");
        assert_eq!(xml.matches("</c:ser></c:barChart>").count(), 1);
        xml.replace(
            "</c:ser></c:barChart>",
            &format!("</c:ser><c:dLbls><c:showCatName val=\"{value}\"/></c:dLbls></c:barChart>"),
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
        .unwrap_or_else(|| panic!("missing PPTX part {part_name}"));
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
            .expect("ZIP entry reads with a valid CRC");
        parts.push((name, contents));
    }
    parts
}

fn read_part(archive: &[u8], part_name: &str) -> Vec<u8> {
    read_parts(archive)
        .into_iter()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing PPTX part {part_name}"))
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
    let parts = read_parts(bytes);
    let names = parts
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"[Content_Types].xml"));
    assert!(names.contains(&"ppt/presentation.xml"));

    for (name, contents) in parts {
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

fn assert_only_chart_xml_changed(before: &[u8], after: &[u8]) {
    let before_parts = read_parts(before);
    let after_parts = read_parts(after);
    assert_eq!(
        before_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        after_parts.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        "the data-label variant keeps package entries in the same order"
    );

    let changed_parts = before_parts
        .iter()
        .zip(&after_parts)
        .filter_map(|((name, original), (_, changed))| {
            (original != changed).then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(changed_parts, [CHART]);
}
