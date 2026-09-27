use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const SLIDE: &str = "ppt/slides/slide1.xml";
const IMAGE: &str = "ppt/media/image1.png";
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

#[test]
fn picture_source_crop_changes_visible_image_identity() {
    let baseline = package_with_asymmetric_image();
    let baseline_pixels = decoded_pixels(&read_part(&baseline, IMAGE));
    assert_eq!(baseline_pixels, (2, 1, [RED, BLUE].concat()));

    // Cropping the left half removes the red source pixel and leaves blue.
    let cropped_pixels = (1, 1, baseline_pixels.2[4..8].to_vec());
    assert_eq!(cropped_pixels.2, BLUE);
    assert_ne!(baseline_pixels, cropped_pixels);

    let cropped = mutate_picture(&baseline, |picture| {
        replace_exactly_once(
            picture,
            r#"<a:blip r:embed="rIdImage"/>"#,
            r#"<a:blip r:embed="rIdImage"/><a:srcRect l="50000" t="0" r="0" b="0"/>"#,
        )
    });
    assert_only_slide_xml_changed(&baseline, &cropped);
    assert_distinct_fingerprint(&baseline, &cropped, "left-half picture crop");
}

#[test]
fn picture_rotation_changes_visible_image_identity() {
    let baseline = package_with_asymmetric_image();
    let baseline_pixels = decoded_pixels(&read_part(&baseline, IMAGE));
    assert_eq!(baseline_pixels, (2, 1, [RED, BLUE].concat()));

    // A 90-degree rotation turns the horizontal red/blue split into a vertical split.
    let rotated_pixels = (1, 2, baseline_pixels.2.clone());
    assert_ne!(baseline_pixels, rotated_pixels);

    let rotated = mutate_picture(&baseline, |picture| {
        replace_exactly_once(picture, "<a:xfrm>", r#"<a:xfrm rot="5400000">"#)
    });
    assert_only_slide_xml_changed(&baseline, &rotated);
    assert_distinct_fingerprint(&baseline, &rotated, "90-degree picture rotation");
}

#[test]
fn picture_horizontal_flip_changes_visible_image_identity() {
    let baseline = package_with_asymmetric_image();
    let baseline_pixels = decoded_pixels(&read_part(&baseline, IMAGE));
    assert_eq!(baseline_pixels, (2, 1, [RED, BLUE].concat()));

    let flipped_pixels = (2, 1, [BLUE, RED].concat());
    assert_ne!(baseline_pixels, flipped_pixels);

    let flipped = mutate_picture(&baseline, |picture| {
        replace_exactly_once(picture, "<a:xfrm>", r#"<a:xfrm flipH="1">"#)
    });
    assert_only_slide_xml_changed(&baseline, &flipped);
    assert_distinct_fingerprint(&baseline, &flipped, "horizontal picture flip");
}

fn assert_distinct_fingerprint(baseline: &[u8], changed: &[u8], description: &str) {
    let profile = InspectionProfile::default();
    let baseline_output = PptxAdapter
        .inspect(baseline, &profile)
        .expect("qualified PPTX baseline with asymmetric referenced image should inspect");
    let changed_output = PptxAdapter
        .inspect(changed, &profile)
        .unwrap_or_else(|error| panic!("valid {description} variant should inspect: {error}"));

    assert_ne!(
        fingerprint(&baseline_output.semantic_projection),
        fingerprint(&changed_output.semantic_projection),
        "{description} changes visible picture content but the semantic fingerprint stayed equal"
    );
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

fn decoded_pixels(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("referenced PNG header");
    let mut pixels = vec![0; reader.output_buffer_size().expect("bounded PNG buffer")];
    let frame = reader
        .next_frame(&mut pixels)
        .expect("referenced PNG pixels");
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    pixels.truncate(frame.buffer_size());
    (frame.width, frame.height, pixels)
}

fn mutate_slide_xml(baseline: &[u8], mutation: impl FnOnce(&str) -> String) -> Vec<u8> {
    replace_part(baseline, SLIDE, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("base slide XML is UTF-8");
        mutation(xml).into_bytes()
    })
}

fn mutate_picture(baseline: &[u8], mutation: impl FnOnce(&str) -> String) -> Vec<u8> {
    mutate_slide_xml(baseline, |xml| {
        let start = xml.find("<p:pic>").expect("fixture picture opening tag");
        let relative_end = xml[start..]
            .find("</p:pic>")
            .expect("fixture picture closing tag");
        let end = start + relative_end + "</p:pic>".len();
        let changed_picture = mutation(&xml[start..end]);
        format!("{}{}{}", &xml[..start], changed_picture, &xml[end..])
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
        "picture-transform mutant preserves ZIP entries and their order"
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
