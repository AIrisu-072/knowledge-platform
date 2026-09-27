use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const PRESENTATION: &str = "ppt/presentation.xml";
const RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

#[test]
fn presentation_relationship_prefix_spelling_is_serialization_noise() {
    let baseline = inspect(BASE, "qualified PPTX baseline must inspect");
    let mutant = presentation_with_alternate_relationship_prefix(false);
    assert_valid_package_and_only_presentation_xml_changed(&mutant);

    let alternate_prefix = inspect(
        &mutant,
        "valid presentation with an alternate relationship prefix must inspect",
    );
    assert_eq!(
        fingerprint(&baseline.semantic_projection),
        fingerprint(&alternate_prefix.semantic_projection),
        "rebinding the relationship namespace prefix must preserve slide identity"
    );
}

#[test]
fn slide_order_remains_significant_with_an_alternate_relationship_prefix() {
    let baseline = inspect(BASE, "qualified PPTX baseline must inspect");
    let mutant = presentation_with_alternate_relationship_prefix(true);
    assert_valid_package_and_only_presentation_xml_changed(&mutant);

    let reordered = inspect(
        &mutant,
        "valid reordered presentation with an alternate relationship prefix must inspect",
    );
    assert_ne!(
        fingerprint(&baseline.semantic_projection),
        fingerprint(&reordered.semantic_projection),
        "rebinding the relationship prefix must not hide a slide-order change"
    );
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn presentation_with_alternate_relationship_prefix(reorder_slides: bool) -> Vec<u8> {
    replace_part(BASE, PRESENTATION, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("presentation XML is UTF-8");
        assert!(xml.contains(&format!("xmlns:r=\"{RELATIONSHIPS_NS}\"")));
        assert!(xml.contains("r:id=\"rIdSlide1\""));
        assert!(xml.contains("r:id=\"rIdSlide2\""));

        let rebound = xml
            .replace("xmlns:r=", "xmlns:rel=")
            .replace("r:id=", "rel:id=");
        assert!(rebound.contains(&format!("xmlns:rel=\"{RELATIONSHIPS_NS}\"")));
        assert!(!rebound.contains("xmlns:r="));
        assert!(rebound.contains("rel:id=\"rIdSlide1\""));
        assert!(rebound.contains("rel:id=\"rIdSlide2\""));

        if reorder_slides {
            let reordered = rebound.replace(
                "<p:sldId id=\"256\" rel:id=\"rIdSlide1\"/><p:sldId id=\"257\" rel:id=\"rIdSlide2\"/>",
                "<p:sldId id=\"257\" rel:id=\"rIdSlide2\"/><p:sldId id=\"256\" rel:id=\"rIdSlide1\"/>",
            );
            assert!(reordered.contains(
                "<p:sldId id=\"257\" rel:id=\"rIdSlide2\"/><p:sldId id=\"256\" rel:id=\"rIdSlide1\"/>"
            ));
            reordered
        } else {
            rebound
        }
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

fn assert_valid_package_and_only_presentation_xml_changed(bytes: &[u8]) {
    let baseline_parts = read_parts(BASE);
    let mutant_parts = read_parts(bytes);
    assert_eq!(
        baseline_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        mutant_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        "the mutant keeps the same package parts in the same order"
    );

    let mut changed_parts = Vec::new();
    for ((name, original), (_, mutant)) in baseline_parts.iter().zip(&mutant_parts) {
        if original != mutant {
            changed_parts.push(name.as_str());
        }
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(mutant).expect("mutant XML is UTF-8");
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
    assert_eq!(changed_parts, [PRESENTATION]);
}
