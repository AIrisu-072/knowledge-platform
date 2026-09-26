use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, PptxAdapter, SemanticAdapter, WorkerFailureCode,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const CONTENT_TYPES: &str = "[Content_Types].xml";
const FIRST_THEME: &str = "ppt/theme/theme1.xml";
const SECOND_THEME: &str = "ppt/theme/theme2.xml";
const THIRD_THEME: &str = "ppt/theme/theme3.xml";
const MAX_XML_NODES: usize = 2_000_000;
const NODES_PER_THEME: usize = 700_000;

#[test]
fn pptx_xml_node_budget_is_aggregate_across_theme_parts() {
    PptxAdapter
        .inspect(BASE, &AdapterProfile::default())
        .expect("qualified base PPTX must inspect");

    let mutant = package_with_padded_themes();
    assert_package_valid(&mutant);

    let parts = read_parts(&mutant);
    let themes = [FIRST_THEME, SECOND_THEME, THIRD_THEME];
    let mut total_nodes = 0usize;
    for theme in themes {
        let data = part(&parts, theme);
        let nodes = count_xml_nodes(data);
        assert!(
            nodes < MAX_XML_NODES,
            "each theme part stays below the per-part limit: {theme} has {nodes} nodes"
        );
        total_nodes += nodes;
    }
    assert!(
        total_nodes > MAX_XML_NODES,
        "the three individually bounded theme parts exceed the package-wide limit: {total_nodes}"
    );

    match PptxAdapter.inspect(&mutant, &AdapterProfile::default()) {
        Err(failure) => assert_eq!(
            failure.code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        ),
        Ok(_) => panic!("XML nodes across PPTX parts must share one aggregate package budget"),
    }
}

fn package_with_padded_themes() -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let theme = String::from_utf8(part(&parts, FIRST_THEME).to_vec()).expect("theme is UTF-8");
    let padded_theme = pad_theme(&theme);

    let (_, original_theme) = parts
        .iter_mut()
        .find(|(name, _)| name == FIRST_THEME)
        .expect("base theme exists");
    *original_theme = padded_theme.clone();

    let content_types = parts
        .iter_mut()
        .find(|(name, _)| name == CONTENT_TYPES)
        .expect("content-types part exists");
    let xml = String::from_utf8(content_types.1.clone()).expect("content types are UTF-8");
    assert!(xml.contains("</Types>"));
    let overrides = format!(
        "<Override PartName=\"/{SECOND_THEME}\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/><Override PartName=\"/{THIRD_THEME}\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>"
    );
    content_types.1 = xml
        .replacen("</Types>", &format!("{overrides}</Types>"), 1)
        .into_bytes();

    parts.push((SECOND_THEME.to_owned(), padded_theme.clone()));
    parts.push((THIRD_THEME.to_owned(), padded_theme));
    write_parts(parts)
}

fn pad_theme(theme: &str) -> Vec<u8> {
    let padding = "<a:ext uri=\"urn:example:budget-probe\"/>".repeat(NODES_PER_THEME);
    let expanded = format!("<a:extLst>{padding}</a:extLst></a:theme>");
    assert_eq!(theme.matches("</a:theme>").count(), 1);
    theme.replacen("</a:theme>", &expanded, 1).into_bytes()
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("PPTX is a ZIP archive");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("ZIP entry has a valid CRC");
        parts.push((name, contents));
    }
    parts
}

fn part<'a>(parts: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    &parts
        .iter()
        .find(|(part_name, _)| part_name == name)
        .unwrap_or_else(|| panic!("PPTX package contains {name}"))
        .1
}

fn count_xml_nodes(data: &[u8]) -> usize {
    let xml = std::str::from_utf8(data).expect("XML part is UTF-8");
    let mut reader = Reader::from_str(xml);
    let mut nodes = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) | Ok(Event::Empty(_)) => nodes += 1,
            Ok(Event::Eof) => return nodes,
            Ok(_) => {}
            Err(error) => panic!("well-formed XML: {error}"),
        }
    }
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

fn assert_package_valid(bytes: &[u8]) {
    let parts = read_parts(bytes);
    for (name, data) in parts {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            count_xml_nodes(&data);
        }
    }
}
