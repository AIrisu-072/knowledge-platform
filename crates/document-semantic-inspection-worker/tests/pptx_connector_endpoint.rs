use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, PptxAdapter, SemanticAdapter, WorkerFailureCode,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const SLIDE: &str = "ppt/slides/slide1.xml";

#[test]
fn connector_endpoint_target_changes_are_version_significant_or_fail_closed() {
    let endpoint_shape_3 = package_with_connector_end(3);
    let endpoint_shape_4 = package_with_connector_end(4);
    assert_valid_endpoint_pair(&endpoint_shape_3, &endpoint_shape_4);

    let profile = AdapterProfile::default();
    let first = PptxAdapter
        .inspect(&endpoint_shape_3, &profile)
        .map(|output| output.semantic_fingerprint());
    let second = PptxAdapter
        .inspect(&endpoint_shape_4, &profile)
        .map(|output| output.semantic_fingerprint());

    match (&first, &second) {
        (Ok(first), Ok(second)) => assert_ne!(
            first, second,
            "changing a connector endpoint from shape 3 to shape 4 changes the meaningful object relationship"
        ),
        _ => {
            for result in [&first, &second] {
                if let Err(error) = result {
                    assert_eq!(
                        error.code(),
                        WorkerFailureCode::UnsupportedSemanticConstruct,
                        "unsupported connector endpoint mapping must fail closed explicitly"
                    );
                }
            }
        }
    }
}

fn package_with_connector_end(end_shape_id: u32) -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let slide = parts
        .iter_mut()
        .find(|(name, _)| name == SLIDE)
        .expect("base PPTX contains slide1.xml");
    let xml = std::str::from_utf8(&slide.1).expect("base slide XML is UTF-8");
    assert!(!xml.contains("<p:cxnSp>"), "base has no connector shape");
    let connector = format!(
        concat!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"4\" name=\"Shape C\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>",
            "<p:spPr><a:xfrm><a:off x=\"5600000\" y=\"1000000\"/><a:ext cx=\"2500000\" cy=\"600000\"/></a:xfrm>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr>",
            "<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Gamma</a:t></a:r></a:p></p:txBody></p:sp>",
            "<p:cxnSp><p:nvCxnSpPr><p:cNvPr id=\"70\" name=\"Connector 1\"/>",
            "<p:cNvCxnSpPr><a:stCxn id=\"2\" idx=\"0\"/><a:endCxn id=\"{}\" idx=\"0\"/></p:cNvCxnSpPr>",
            "<p:nvPr/></p:nvCxnSpPr><p:spPr><a:xfrm><a:off x=\"0\" y=\"800000\"/>",
            "<a:ext cx=\"2500000\" cy=\"0\"/></a:xfrm><a:prstGeom prst=\"line\"><a:avLst/></a:prstGeom>",
            "<a:ln/></p:spPr></p:cxnSp>"
        ),
        end_shape_id
    );
    assert_eq!(xml.matches("</p:spTree>").count(), 1);
    slide.1 = xml
        .replace("</p:spTree>", &format!("{connector}</p:spTree>"))
        .into_bytes();
    write_parts(parts)
}

fn assert_valid_endpoint_pair(endpoint_shape_3: &[u8], endpoint_shape_4: &[u8]) {
    let base_parts = read_parts(BASE);
    let first_parts = read_parts(endpoint_shape_3);
    let second_parts = read_parts(endpoint_shape_4);
    for mutant_parts in [&first_parts, &second_parts] {
        assert_eq!(
            base_parts.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            mutant_parts
                .iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            "connector variant preserves the base package entries and order"
        );
        let changed_parts = base_parts
            .iter()
            .zip(mutant_parts.iter())
            .filter_map(|((name, original), (_, changed))| {
                (original != changed).then_some(name.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            changed_parts,
            [SLIDE],
            "only slide1.xml is mutated from base"
        );
    }
    assert_eq!(
        first_parts.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        second_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        "endpoint variants preserve package entries and order"
    );

    let mut changed_parts = Vec::new();
    for ((name, first), (_, second)) in first_parts.iter().zip(&second_parts) {
        if first != second {
            changed_parts.push(name.as_str());
        }
        for contents in [first, second] {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                let xml = std::str::from_utf8(contents).expect("mutant package XML is UTF-8");
                let mut reader = Reader::from_str(xml);
                reader.config_mut().check_end_names = true;
                loop {
                    match reader.read_event() {
                        Ok(Event::Eof) => break,
                        Ok(_) => {}
                        Err(error) => panic!("package part {name} is malformed XML: {error}"),
                    }
                }
            }
        }
    }
    assert_eq!(
        changed_parts,
        [SLIDE],
        "only the connector endpoint differs"
    );

    let first = String::from_utf8(read_part(endpoint_shape_3, SLIDE)).expect("slide is UTF-8");
    let second = String::from_utf8(read_part(endpoint_shape_4, SLIDE)).expect("slide is UTF-8");
    assert!(first.contains("<a:stCxn id=\"2\" idx=\"0\"/>"));
    assert!(first.contains("<a:endCxn id=\"3\" idx=\"0\"/>"));
    assert!(second.contains("<a:endCxn id=\"4\" idx=\"0\"/>"));
    assert_eq!(
        first.replace("<a:endCxn id=\"3\"", "<a:endCxn id=\"ENDPOINT\""),
        second.replace("<a:endCxn id=\"4\"", "<a:endCxn id=\"ENDPOINT\""),
        "valid endpoint variants differ only in the target shape id"
    );
    for (shape_id, name) in [("2", "Shape A"), ("3", "Shape B"), ("4", "Shape C")] {
        assert!(first.contains(&format!("<p:cNvPr id=\"{shape_id}\" name=\"{name}\"/>")));
    }
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("mutant is a ZIP package");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("ZIP entry decompresses with valid CRC");
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
