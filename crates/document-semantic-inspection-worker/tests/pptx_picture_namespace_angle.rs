use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, PptxAdapter, SemanticAdapter, WorkerFailureCode,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const SLIDE: &str = "ppt/slides/slide1.xml";
const IMAGE: &str = "ppt/media/image1.png";
const DRAWINGML_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

#[test]
fn pptx_redundant_local_drawingml_namespace_on_picture_transform_is_noise() {
    let baseline = package_with_asymmetric_image();
    let baseline_output = inspect(&baseline);
    let changed = mutate_picture(&baseline, |picture| {
        replace_exactly_once(
            picture,
            "<a:xfrm>",
            &format!(r#"<a:xfrm xmlns:a="{DRAWINGML_NS}">"#),
        )
    });
    assert_only_slide_xml_changed(&baseline, &changed);

    let changed_output = inspect(&changed);
    assert_eq!(
        baseline_output.semantic_fingerprint(),
        changed_output.semantic_fingerprint(),
        "a local namespace declaration bound to the inherited DrawingML URI is serialization noise"
    );
}

#[test]
fn pptx_redundant_local_drawingml_namespace_on_source_rect_is_noise() {
    let baseline = package_with_asymmetric_image();
    let baseline_output = inspect(&baseline);
    let changed = mutate_picture(&baseline, |picture| {
        replace_exactly_once(
            picture,
            r#"<a:blip r:embed="rIdImage"/>"#,
            &format!(
                r#"<a:blip r:embed="rIdImage"/><a:srcRect xmlns:a="{DRAWINGML_NS}" l="0" t="0" r="0" b="0"/>"#
            ),
        )
    });
    assert_only_slide_xml_changed(&baseline, &changed);

    let changed_output = inspect(&changed);
    assert_eq!(
        baseline_output.semantic_fingerprint(),
        changed_output.semantic_fingerprint(),
        "a redundant local namespace declaration and zero crop preserve picture semantics"
    );
}

#[test]
fn pptx_picture_rotation_outside_open_xml_int32_range_fails_closed() {
    let baseline = package_with_asymmetric_image();
    let _baseline_output = inspect(&baseline);
    let overflow = mutate_picture(&baseline, |picture| {
        replace_exactly_once(picture, "<a:xfrm>", r#"<a:xfrm rot="2147483648">"#)
    });
    assert_only_slide_xml_changed(&baseline, &overflow);

    let result = PptxAdapter.inspect(&overflow, &AdapterProfile::default());
    assert_eq!(
        result
            .expect_err("out-of-range picture rotation must fail closed")
            .code(),
        WorkerFailureCode::SemanticExtractionFailed,
        "out-of-range Open XML rotation must be returned as a controlled semantic failure"
    );
}

fn inspect(package: &[u8]) -> document_semantic_inspection_worker::SemanticAdapterOutput {
    PptxAdapter
        .inspect(package, &AdapterProfile::default())
        .expect("qualified PPTX baseline or noise-only picture variant should inspect")
}

fn package_with_asymmetric_image() -> Vec<u8> {
    let image = asymmetric_png();
    let package = replace_part(BASE, IMAGE, |_| image.clone());
    assert_eq!(read_part(&package, IMAGE), image);
    validate_package(&package);
    package
}

fn asymmetric_png() -> Vec<u8> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header");
        writer
            .write_image_data(&[RED, BLUE].concat())
            .expect("asymmetric PNG pixels");
    }
    png
}

fn mutate_picture(baseline: &[u8], mutation: impl FnOnce(&str) -> String) -> Vec<u8> {
    replace_part(baseline, SLIDE, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("base slide XML is UTF-8");
        let start = xml.find("<p:pic>").expect("fixture picture opening tag");
        let relative_end = xml[start..]
            .find("</p:pic>")
            .expect("fixture picture closing tag");
        let end = start + relative_end + "</p:pic>".len();
        let changed_picture = mutation(&xml[start..end]);
        format!("{}{}{}", &xml[..start], changed_picture, &xml[end..]).into_bytes()
    })
}

fn replace_exactly_once(xml: &str, from: &str, to: &str) -> String {
    assert_eq!(
        xml.matches(from).count(),
        1,
        "fixture anchor is unique: {from}"
    );
    xml.replacen(from, to, 1)
}

fn replace_part(
    archive: &[u8],
    part_name: &str,
    transform: impl FnOnce(&[u8]) -> Vec<u8>,
) -> Vec<u8> {
    let mut entries = read_entries(archive);
    let entry = entries
        .iter_mut()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing ZIP part {part_name}"));
    entry.1 = transform(&entry.1);
    write_entries(entries)
}

fn read_entries(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("PPTX is a ZIP archive");
    let mut entries = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("ZIP entry decompresses with valid CRC");
        entries.push((name, contents));
    }
    entries
}

fn write_entries(entries: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries {
        writer
            .start_file(name, options)
            .expect("mutant ZIP part starts");
        writer.write_all(&contents).expect("mutant ZIP part writes");
    }
    writer.finish().expect("mutant ZIP finishes").into_inner()
}

fn validate_package(archive: &[u8]) {
    for (name, contents) in read_entries(archive) {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(&contents).expect("package XML remains UTF-8");
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

fn assert_only_slide_xml_changed(baseline: &[u8], changed: &[u8]) {
    let baseline_entries = read_entries(baseline);
    let changed_entries = read_entries(changed);
    assert_eq!(
        baseline_entries
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        changed_entries
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        "picture mutant preserves ZIP entries and their order"
    );
    let changed_parts = baseline_entries
        .iter()
        .zip(&changed_entries)
        .filter_map(|((name, original), (_, updated))| {
            (original != updated).then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(changed_parts, [SLIDE]);
    validate_package(changed);
}

fn read_part(archive: &[u8], part_name: &str) -> Vec<u8> {
    read_entries(archive)
        .into_iter()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing ZIP part {part_name}"))
        .1
}
