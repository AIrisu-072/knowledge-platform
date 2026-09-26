use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const SLIDE_PART: &str = "ppt/slides/slide1.xml";
const RELATIONSHIPS_PART: &str = "ppt/slides/_rels/slide1.xml.rels";
const SHAPE_MARKER: &str = r#"<p:cNvPr id="3" name="Shape B"/>"#;
const SHAPE_CLICK: &str =
    r#"<p:cNvPr id="3" name="Shape B"><a:hlinkClick r:id="rIdShapeLink"/></p:cNvPr>"#;
const TARGET_A: &str = "https://example.com/shape-a";
const TARGET_B: &str = "https://example.com/shape-b";

#[test]
fn shape_level_click_hyperlink_target_changes_semantic_identity() {
    let baseline = package_with_shape_click_target(TARGET_A);
    let mutant = package_with_shape_click_target(TARGET_B);

    assert_valid_zip_and_xml(&baseline);
    assert_valid_zip_and_xml(&mutant);
    assert_shape_click_target(&baseline, TARGET_A);
    assert_shape_click_target(&mutant, TARGET_B);
    assert_only_relationship_target_changed(&baseline, &mutant);

    let baseline_output = inspect(
        &baseline,
        "valid shape-level hyperlink baseline must inspect",
    );
    let mutant_output = inspect(&mutant, "valid changed shape-level hyperlink must inspect");

    assert_ne!(
        fingerprint(&baseline_output.semantic_projection),
        fingerprint(&mutant_output.semantic_projection),
        "changing a p:cNvPr/a:hlinkClick target changes PPTX semantics under Frozen Design §9.4 and §13.5"
    );
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn package_with_shape_click_target(target: &str) -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let mut slide_seen = false;
    let mut relationships_seen = false;

    for (name, contents) in &mut parts {
        if name == SLIDE_PART {
            let xml = std::str::from_utf8(contents).expect("base slide XML is UTF-8");
            assert_eq!(xml.matches(SHAPE_MARKER).count(), 1);
            *contents = xml.replace(SHAPE_MARKER, SHAPE_CLICK).into_bytes();
            slide_seen = true;
        } else if name == RELATIONSHIPS_PART {
            let xml = std::str::from_utf8(contents).expect("base relationships XML is UTF-8");
            assert!(!xml.contains("rIdShapeLink"));
            let relationship = format!(
                r#"<Relationship Id="rIdShapeLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{target}" TargetMode="External"/>"#
            );
            assert_eq!(xml.matches("</Relationships>").count(), 1);
            *contents = xml
                .replace(
                    "</Relationships>",
                    &format!("{relationship}</Relationships>"),
                )
                .into_bytes();
            relationships_seen = true;
        }
    }

    assert!(slide_seen, "base PPTX has the target slide part");
    assert!(
        relationships_seen,
        "base PPTX has the target relationships part"
    );
    write_parts(parts)
}

fn assert_shape_click_target(archive: &[u8], target: &str) {
    let slide = String::from_utf8(read_part(archive, SLIDE_PART)).expect("slide XML is UTF-8");
    assert_eq!(slide.matches(SHAPE_CLICK).count(), 1);
    let relationships = String::from_utf8(read_part(archive, RELATIONSHIPS_PART))
        .expect("relationships XML is UTF-8");
    let expected = format!(
        r#"<Relationship Id="rIdShapeLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{target}" TargetMode="External"/>"#
    );
    assert_eq!(relationships.matches(&expected).count(), 1);
}

fn assert_only_relationship_target_changed(baseline: &[u8], mutant: &[u8]) {
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
        "the mutant keeps the same ZIP entries in the same order"
    );

    let changed_parts = baseline_parts
        .iter()
        .zip(&mutant_parts)
        .filter_map(|((name, original), (_, changed))| {
            (original != changed).then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(changed_parts, [RELATIONSHIPS_PART]);

    let baseline_relationships =
        String::from_utf8(read_part(baseline, RELATIONSHIPS_PART)).expect("baseline rels UTF-8");
    let mutant_relationships =
        String::from_utf8(read_part(mutant, RELATIONSHIPS_PART)).expect("mutant rels UTF-8");
    assert_eq!(
        baseline_relationships.replace(TARGET_A, "SHAPE_TARGET"),
        mutant_relationships.replace(TARGET_B, "SHAPE_TARGET"),
        "only the external target value changes in the slide relationship"
    );
}

fn assert_valid_zip_and_xml(archive: &[u8]) {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("valid PPTX ZIP");
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("central-directory entry");
        let name = entry.name().to_owned();
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .expect("PPTX entry data and CRC are valid");
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(&contents).expect("PPTX XML is UTF-8");
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

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("valid PPTX ZIP");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("central-directory entry");
        let name = entry.name().to_owned();
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .expect("PPTX entry and CRC");
        parts.push((name, contents));
    }
    parts
}

fn read_part(archive: &[u8], name: &str) -> Vec<u8> {
    read_parts(archive)
        .into_iter()
        .find(|(part_name, _)| part_name == name)
        .unwrap_or_else(|| panic!("missing PPTX part {name}"))
        .1
}

fn write_parts(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("start PPTX ZIP part");
        writer.write_all(&contents).expect("write PPTX ZIP part");
    }
    writer.finish().expect("finish PPTX ZIP").into_inner()
}
