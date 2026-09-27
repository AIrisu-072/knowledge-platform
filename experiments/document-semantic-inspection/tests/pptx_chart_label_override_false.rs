use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";

#[test]
fn explicit_false_point_label_override_changes_chart_semantics() {
    let global_labels = package_with_point_override(false);
    let first_label_hidden = package_with_point_override(true);

    assert_valid_package(&global_labels);
    assert_valid_package(&first_label_hidden);
    assert_only_chart_xml_changed(BASE, &global_labels);
    assert_only_chart_xml_changed(BASE, &first_label_hidden);
    assert_only_chart_xml_changed(&global_labels, &first_label_hidden);

    let global_xml = String::from_utf8(read_part(&global_labels, CHART))
        .expect("global-label chart XML is UTF-8");
    let override_xml = String::from_utf8(read_part(&first_label_hidden, CHART))
        .expect("point-override chart XML is UTF-8");
    assert!(global_xml.contains("<c:dLbls><c:showCatName val=\"1\"/></c:dLbls>"));
    assert!(override_xml.contains("<c:dLbl><c:idx val=\"0\"/><c:showCatName val=\"0\"/></c:dLbl>"));
    assert!(override_xml.contains("<c:showCatName val=\"1\"/>"));

    let global_output = inspect(
        &global_labels,
        "PPTX with globally enabled category labels must inspect",
    );
    let override_output = inspect(
        &first_label_hidden,
        "PPTX with an explicit point label override must inspect",
    );

    assert_ne!(
        fingerprint(&global_output.semantic_projection),
        fingerprint(&override_output.semantic_projection),
        "an explicit false override hides the first category label and changes chart semantics"
    );
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn package_with_point_override(override_first_point: bool) -> Vec<u8> {
    replace_chart(BASE, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("base chart XML is UTF-8");
        assert_eq!(xml.matches("</c:ser></c:barChart>").count(), 1);

        let point_override = if override_first_point {
            "<c:dLbl><c:idx val=\"0\"/><c:showCatName val=\"0\"/></c:dLbl>"
        } else {
            ""
        };
        xml.replace(
            "</c:ser></c:barChart>",
            &format!(
                "</c:ser><c:dLbls>{point_override}<c:showCatName val=\"1\"/></c:dLbls></c:barChart>"
            ),
        )
        .into_bytes()
    })
}

fn replace_chart(archive: &[u8], transform: impl FnOnce(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let mut parts = read_parts(archive);
    let chart = parts
        .iter_mut()
        .find(|(name, _)| name == CHART)
        .unwrap_or_else(|| panic!("missing PPTX part {CHART}"));
    chart.1 = transform(&chart.1);
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
        "the chart-label variant keeps package entries in the same order"
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
