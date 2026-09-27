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
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

#[test]
fn pptx_unsupported_picture_tile_fill_fails_closed() {
    let baseline = picture_with_wide_box();
    assert_asymmetric_source_is_visible(&baseline);
    assert_picture_extent(&baseline, "3600000", "900000");
    assert!(picture_xml(&baseline).contains("<a:stretch><a:fillRect/></a:stretch>"));
    assert_accepted(&baseline);

    // With a 4:1 box and a 2:1 two-color source, tiling and stretching render
    // different red/blue layouts. The unsupported fill must not be ignored.
    let tiled = mutate_picture(&baseline, |picture| {
        replace_once(
            picture,
            "<a:stretch><a:fillRect/></a:stretch>",
            "<a:tile tx=\"0\" ty=\"0\" sx=\"100000\" sy=\"100000\"/>",
        )
    });
    assert_only_slide_changed(&baseline, &tiled);

    assert_rejected(&tiled, "DrawingML tiled picture fill");
}

#[test]
fn pptx_unsupported_picture_grayscale_effect_fails_closed() {
    let baseline = picture_with_asymmetric_source();
    assert_asymmetric_source_is_visible(&baseline);
    assert_picture_extent(&baseline, "900000", "900000");
    assert_accepted(&baseline);

    // The source contains saturated red and blue pixels; a:grayscl removes
    // their chroma and visibly changes the rendered picture.
    let grayscale = mutate_picture(&baseline, |picture| {
        replace_once(
            picture,
            "<a:blip r:embed=\"rIdImage\"/>",
            "<a:blip r:embed=\"rIdImage\"><a:grayscl/></a:blip>",
        )
    });
    assert_only_slide_changed(&baseline, &grayscale);

    assert_rejected(&grayscale, "DrawingML grayscale picture effect");
}

fn assert_accepted(bytes: &[u8]) {
    PptxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("qualified baseline PPTX must inspect");
}

fn assert_rejected(bytes: &[u8], description: &str) {
    let error = match PptxAdapter.inspect(bytes, &AdapterProfile::default()) {
        Err(error) => error,
        Ok(_) => panic!("{description} must fail closed but was accepted"),
    };
    assert!(
        matches!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct
                | WorkerFailureCode::SemanticExtractionFailed
        ),
        "{description} must fail closed with a controlled semantic error, got {error}"
    );
}

fn picture_with_wide_box() -> Vec<u8> {
    let baseline = picture_with_asymmetric_source();
    mutate_picture(&baseline, |picture| {
        replace_once(
            picture,
            "<a:ext cx=\"900000\" cy=\"900000\"/>",
            "<a:ext cx=\"3600000\" cy=\"900000\"/>",
        )
    })
}

fn picture_with_asymmetric_source() -> Vec<u8> {
    let package = replace_part(BASE, IMAGE, |_| asymmetric_png());
    validate_package(&package);
    package
}

fn assert_asymmetric_source_is_visible(package: &[u8]) {
    let image = read_part(package, IMAGE);
    let decoder = png::Decoder::new(Cursor::new(image));
    let mut reader = decoder.read_info().expect("referenced PNG header");
    let mut pixels = vec![0; reader.output_buffer_size().expect("bounded PNG buffer")];
    let frame = reader
        .next_frame(&mut pixels)
        .expect("referenced PNG pixels");
    pixels.truncate(frame.buffer_size());
    assert_eq!(frame.width, 2);
    assert_eq!(frame.height, 1);
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    assert_eq!(pixels, [RED, BLUE].concat());

    let picture = picture_xml(package);
    assert!(picture.contains("<a:blip r:embed=\"rIdImage\""));
}

fn assert_picture_extent(package: &[u8], cx: &str, cy: &str) {
    let picture = picture_xml(package);
    assert!(picture.contains(&format!("<a:ext cx=\"{cx}\" cy=\"{cy}\"/>")));
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

fn picture_xml(package: &[u8]) -> String {
    let xml = String::from_utf8(read_part(package, SLIDE)).expect("slide XML is UTF-8");
    let start = xml.find("<p:pic>").expect("fixture picture opening tag");
    let relative_end = xml[start..]
        .find("</p:pic>")
        .expect("fixture picture closing tag");
    let end = start + relative_end + "</p:pic>".len();
    xml[start..end].to_owned()
}

fn mutate_picture(package: &[u8], mutation: impl FnOnce(&str) -> String) -> Vec<u8> {
    replace_part(package, SLIDE, |bytes| {
        let xml = std::str::from_utf8(bytes).expect("slide XML is UTF-8");
        let start = xml.find("<p:pic>").expect("fixture picture opening tag");
        let relative_end = xml[start..]
            .find("</p:pic>")
            .expect("fixture picture closing tag");
        let end = start + relative_end + "</p:pic>".len();
        let changed_picture = mutation(&xml[start..end]);
        format!("{}{}{}", &xml[..start], changed_picture, &xml[end..]).into_bytes()
    })
}

fn replace_once(xml: &str, from: &str, to: &str) -> String {
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

fn assert_only_slide_changed(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_entries(baseline);
    let mutant_parts = read_entries(mutant);
    assert_eq!(
        baseline_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        mutant_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        "mutant preserves package part names and order"
    );
    let changed_parts = baseline_parts
        .iter()
        .zip(&mutant_parts)
        .filter_map(|((name, original), (_, changed))| {
            (original != changed).then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(changed_parts, [SLIDE]);
    validate_package(mutant);
}

fn read_part(archive: &[u8], part_name: &str) -> Vec<u8> {
    read_entries(archive)
        .into_iter()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing ZIP part {part_name}"))
        .1
}
