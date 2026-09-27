use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, SemanticAdapter};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const SMARTART_DATA: &str = "ppt/diagrams/data1.xml";
const SIBLING_GRAPH: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<dgm:dataModel xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <dgm:ptLst>
    <dgm:pt modelId="0" type="doc"/>
    <dgm:pt modelId="1"><dgm:t><a:p><a:r><a:t>Node A</a:t></a:r></a:p></dgm:t></dgm:pt>
    <dgm:pt modelId="2"><dgm:t><a:p><a:r><a:t>Node B</a:t></a:r></a:p></dgm:t></dgm:pt>
  </dgm:ptLst>
  <dgm:cxnLst>
    <dgm:cxn modelId="3" srcId="0" destId="1" srcOrd="0" destOrd="0"/>
    <dgm:cxn modelId="4" srcId="0" destId="2" srcOrd="1" destOrd="0"/>
  </dgm:cxnLst>
  <dgm:bg/><dgm:whole/>
</dgm:dataModel>"#;
const SHARED_DESTINATION_GRAPH: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<dgm:dataModel xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <dgm:ptLst>
    <dgm:pt modelId="0" type="doc"/>
    <dgm:pt modelId="1"><dgm:t><a:p><a:r><a:t>Parent A</a:t></a:r></a:p></dgm:t></dgm:pt>
    <dgm:pt modelId="2"><dgm:t><a:p><a:r><a:t>Parent B</a:t></a:r></a:p></dgm:t></dgm:pt>
    <dgm:pt modelId="3"><dgm:t><a:p><a:r><a:t>Shared node</a:t></a:r></a:p></dgm:t></dgm:pt>
  </dgm:ptLst>
  <dgm:cxnLst>
    <dgm:cxn modelId="4" srcId="0" destId="1" srcOrd="0" destOrd="0"/>
    <dgm:cxn modelId="5" srcId="0" destId="2" srcOrd="1" destOrd="0"/>
    <dgm:cxn modelId="6" srcId="1" destId="3" srcOrd="0" destOrd="0"/>
    <dgm:cxn modelId="7" srcId="2" destId="3" srcOrd="0" destOrd="1"/>
  </dgm:cxnLst>
  <dgm:bg/><dgm:whole/>
</dgm:dataModel>"#;

#[test]
fn smartart_sibling_src_order_changes_identity() {
    let baseline = replace_part(BASE, SMARTART_DATA, |_| SIBLING_GRAPH.as_bytes().to_vec());
    assert_valid_pptx_package(&baseline);
    assert_only_smartart_data_differs(BASE, &baseline);
    let baseline_xml = String::from_utf8(read_part(&baseline, SMARTART_DATA))
        .expect("synthetic SmartArt data is UTF-8");
    assert_sibling_order_preconditions(&baseline_xml, (0, 1));
    let baseline_output = inspect(&baseline, "qualified sibling-order baseline must inspect");
    let baseline_fingerprint = baseline_output.semantic_fingerprint();

    let mutant = replace_part(&baseline, SMARTART_DATA, |bytes| {
        let xml = String::from_utf8(bytes.to_vec()).expect("SmartArt part is UTF-8");
        let first = xml.replace(
            "srcId=\"0\" destId=\"1\" srcOrd=\"0\"",
            "srcId=\"0\" destId=\"1\" srcOrd=\"1\"",
        );
        let second = first.replace(
            "srcId=\"0\" destId=\"2\" srcOrd=\"1\"",
            "srcId=\"0\" destId=\"2\" srcOrd=\"0\"",
        );
        assert_ne!(xml, first, "first sibling srcOrd is changed");
        assert_ne!(first, second, "second sibling srcOrd is changed");
        second.into_bytes()
    });
    assert_valid_pptx_package(&mutant);
    assert_only_smartart_data_differs(&baseline, &mutant);
    let mutant_xml = String::from_utf8(read_part(&mutant, SMARTART_DATA))
        .expect("mutant SmartArt data is UTF-8");
    assert_sibling_order_preconditions(&mutant_xml, (1, 0));
    assert_eq!(
        node_labels(&baseline_xml),
        node_labels(&mutant_xml),
        "the source/destination point identities and labels remain unchanged"
    );

    let mutant_output = inspect(&mutant, "valid sibling-order mutant must inspect");
    assert_ne!(
        baseline_fingerprint,
        mutant_output.semantic_fingerprint(),
        "reordering children by SmartArt connection srcOrd must affect identity"
    );
}

#[test]
fn smartart_duplicate_sibling_src_order_fails_closed() {
    let baseline = replace_part(BASE, SMARTART_DATA, |_| SIBLING_GRAPH.as_bytes().to_vec());
    assert_valid_pptx_package(&baseline);
    assert_only_smartart_data_differs(BASE, &baseline);
    let baseline_xml = String::from_utf8(read_part(&baseline, SMARTART_DATA))
        .expect("synthetic SmartArt data is UTF-8");
    assert_sibling_order_preconditions(&baseline_xml, (0, 1));
    inspect(&baseline, "qualified sibling-order baseline must inspect");

    let mutant = replace_part(&baseline, SMARTART_DATA, |bytes| {
        let xml = String::from_utf8(bytes.to_vec()).expect("SmartArt part is UTF-8");
        let mutant = xml.replace(
            "srcId=\"0\" destId=\"2\" srcOrd=\"1\"",
            "srcId=\"0\" destId=\"2\" srcOrd=\"0\"",
        );
        assert_ne!(xml, mutant, "second sibling srcOrd is duplicated");
        mutant.into_bytes()
    });
    assert_valid_pptx_package(&mutant);
    assert_only_smartart_data_differs(&baseline, &mutant);
    let mutant_xml = String::from_utf8(read_part(&mutant, SMARTART_DATA))
        .expect("mutant SmartArt data is UTF-8");
    assert_sibling_order_preconditions(&mutant_xml, (0, 0));

    assert!(
        PptxAdapter
            .inspect(&mutant, &AdapterProfile::default())
            .is_err(),
        "duplicate outgoing sibling ordinals are ambiguous and must fail closed"
    );
}

#[test]
fn smartart_duplicate_incoming_dest_order_fails_closed() {
    let baseline = replace_part(BASE, SMARTART_DATA, |_| {
        SHARED_DESTINATION_GRAPH.as_bytes().to_vec()
    });
    assert_valid_pptx_package(&baseline);
    assert_only_smartart_data_differs(BASE, &baseline);
    let baseline_xml = String::from_utf8(read_part(&baseline, SMARTART_DATA))
        .expect("synthetic SmartArt data is UTF-8");
    assert!(baseline_xml.contains("srcId=\"1\" destId=\"3\" srcOrd=\"0\" destOrd=\"0\""));
    assert!(baseline_xml.contains("srcId=\"2\" destId=\"3\" srcOrd=\"0\" destOrd=\"1\""));
    inspect(
        &baseline,
        "qualified shared-destination baseline must inspect",
    );

    let mutant = replace_part(&baseline, SMARTART_DATA, |bytes| {
        let xml = String::from_utf8(bytes.to_vec()).expect("SmartArt part is UTF-8");
        let mutant = xml.replace(
            "srcId=\"2\" destId=\"3\" srcOrd=\"0\" destOrd=\"1\"",
            "srcId=\"2\" destId=\"3\" srcOrd=\"0\" destOrd=\"0\"",
        );
        assert_ne!(xml, mutant, "second incoming destOrd is duplicated");
        mutant.into_bytes()
    });
    assert_valid_pptx_package(&mutant);
    assert_only_smartart_data_differs(&baseline, &mutant);

    assert!(
        PptxAdapter
            .inspect(&mutant, &AdapterProfile::default())
            .is_err(),
        "duplicate incoming destination ordinals are ambiguous and must fail closed"
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

fn node_labels(xml: &str) -> Vec<&str> {
    ["Node A", "Node B"]
        .into_iter()
        .filter(|label| xml.contains(label))
        .collect()
}

fn assert_sibling_order_preconditions(xml: &str, expected: (u8, u8)) {
    assert!(xml.contains("modelId=\"0\" type=\"doc\""));
    assert!(xml.contains("modelId=\"1\""));
    assert!(xml.contains("modelId=\"2\""));
    assert_eq!(node_labels(xml), ["Node A", "Node B"]);
    assert!(xml.contains(&format!(
        "srcId=\"0\" destId=\"1\" srcOrd=\"{}\" destOrd=\"0\"",
        expected.0
    )));
    assert!(xml.contains(&format!(
        "srcId=\"0\" destId=\"2\" srcOrd=\"{}\" destOrd=\"0\"",
        expected.1
    )));
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
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("PPTX is a ZIP archive");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("ZIP entry has valid CRC");
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

fn assert_only_smartart_data_differs(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len());
    let mut differing_parts = Vec::new();
    for ((baseline_name, baseline_contents), (mutant_name, mutant_contents)) in
        baseline_parts.iter().zip(&mutant_parts)
    {
        assert_eq!(baseline_name, mutant_name, "ZIP entry order is preserved");
        if baseline_contents != mutant_contents {
            differing_parts.push(baseline_name.as_str());
        }
    }
    assert_eq!(differing_parts, [SMARTART_DATA]);
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
