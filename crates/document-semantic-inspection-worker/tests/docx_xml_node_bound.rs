use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/docx/base.docx");
const MAX_XML_NODES: usize = 2_000_000;
const MAX_PART_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 32 * 1024 * 1024;
const XML_PARTS: [&str; 3] = ["word/document.xml", "word/styles.xml", "word/numbering.xml"];

#[test]
fn package_wide_xml_limit_counts_processing_instructions_and_comments() {
    DocxAdapter
        .inspect(BASE, &AdapterProfile::default())
        .expect("baseline DOCX fixture is valid");

    let oversized_xml = docx_with_non_element_nodes(BASE);
    assert_uncompressed_package_limits(&oversized_xml);

    let failure = OoxmlCoverageSentinel::validate_package(&oversized_xml)
        .expect_err("package-wide XML node count must exceed the resource limit");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::InspectionResourceLimitExceeded,
        "{failure}"
    );
}

fn docx_with_non_element_nodes(bytes: &[u8]) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open baseline DOCX");
    let mut parts = Vec::with_capacity(archive.len());
    let nodes_per_part = MAX_XML_NODES / XML_PARTS.len() + 1;
    let mut injected_nodes = 0;

    for index in 0..archive.len() {
        let (name, mut contents) = {
            let mut entry = archive.by_index(index).expect("read baseline DOCX part");
            let name = entry.name().to_owned();
            let mut contents = Vec::new();
            entry
                .read_to_end(&mut contents)
                .expect("decompress baseline DOCX part");
            (name, contents)
        };

        if XML_PARTS.contains(&name.as_str()) {
            insert_non_element_nodes(&mut contents, nodes_per_part);
            injected_nodes += nodes_per_part;
        }
        parts.push((name, contents));
    }

    assert_eq!(injected_nodes, nodes_per_part * XML_PARTS.len());
    assert!(injected_nodes > MAX_XML_NODES);
    stored_docx(parts)
}

fn insert_non_element_nodes(xml_bytes: &mut Vec<u8>, count: usize) {
    let mut xml = String::from_utf8(std::mem::take(xml_bytes)).expect("baseline XML is UTF-8");
    let root_close = xml
        .rfind("</")
        .expect("baseline XML has a closing root tag");
    let mut nodes = String::with_capacity(count * 6);

    for index in 0..count {
        if index % 2 == 0 {
            nodes.push_str("<?n?>");
        } else {
            nodes.push_str("<!--n-->");
        }
    }

    xml.insert_str(root_close, &nodes);
    *xml_bytes = xml.into_bytes();
}

fn assert_uncompressed_package_limits(bytes: &[u8]) {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open generated DOCX");
    let mut total_bytes = 0u64;

    for index in 0..archive.len() {
        let part = archive
            .by_index(index)
            .expect("read generated DOCX metadata");
        assert!(
            part.size() <= MAX_PART_BYTES,
            "{} exceeds the 8 MiB per-part limit",
            part.name()
        );
        total_bytes = total_bytes
            .checked_add(part.size())
            .expect("total uncompressed size fits u64");
    }

    assert!(
        total_bytes <= MAX_PACKAGE_BYTES,
        "package exceeds the 32 MiB total uncompressed limit"
    );
}

fn stored_docx(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("start generated DOCX part");
        writer
            .write_all(&contents)
            .expect("write generated DOCX part");
    }

    writer
        .finish()
        .expect("finish generated DOCX archive")
        .into_inner()
}
