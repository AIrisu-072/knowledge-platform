use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile, fingerprint,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const INLINE_PICTURE: &str = concat!(
    r#"<wp:inline distT="0" distB="0" distL="0" distR="0">"#,
    r#"<wp:extent cx="914400" cy="457200"/>"#,
    r#"<wp:docPr id="1" name="Picture"/>"#,
    r#"<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>"#,
    r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
    r#"<pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="Picture"/><pic:cNvPicPr/></pic:nvPicPr>"#,
    r#"<pic:blipFill><a:blip r:embed="rIdImage"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
    r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm>"#,
    r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>"#,
    r#"</a:graphicData></a:graphic></wp:inline>"#,
);

#[test]
fn src_rect_crop_changes_image_identity_or_fails_closed() {
    let baseline = inline_png_docx();
    let baseline_document = document_xml(&baseline);
    let cropped_document = replace_exactly_once(
        &baseline_document,
        r#"<a:blip r:embed="rIdImage"/>"#,
        r#"<a:blip r:embed="rIdImage"/><a:srcRect l="50000" t="0" r="0" b="0"/>"#,
    );
    let cropped = docx_with_document(&baseline, &cropped_document);
    assert_only_document_xml_changed(&baseline, &cropped);

    assert_semantics_distinguish_or_explicitly_reject(&baseline, &cropped, "a:srcRect left crop");
}

#[test]
fn xfrm_rotation_changes_image_identity_or_fails_closed() {
    let baseline = inline_png_docx();
    let baseline_document = document_xml(&baseline);
    let rotated_document =
        replace_exactly_once(&baseline_document, "<a:xfrm>", r#"<a:xfrm rot="5400000">"#);
    let rotated = docx_with_document(&baseline, &rotated_document);
    assert_only_document_xml_changed(&baseline, &rotated);

    assert_semantics_distinguish_or_explicitly_reject(
        &baseline,
        &rotated,
        "a:xfrm 90-degree rotation",
    );
}

#[test]
fn shape_extent_changes_image_identity_or_fails_closed() {
    let baseline = inline_png_docx();
    let changed_document = replace_exactly_once(
        &document_xml(&baseline),
        r#"<a:ext cx="914400" cy="457200"/>"#,
        r#"<a:ext cx="1828800" cy="457200"/>"#,
    );
    let changed = docx_with_document(&baseline, &changed_document);
    assert_only_document_xml_changed(&baseline, &changed);
    assert_semantics_distinguish_or_explicitly_reject(&baseline, &changed, "picture shape extent");
}

#[test]
fn frame_extent_changes_image_identity_or_fails_closed() {
    let baseline = inline_png_docx();
    let changed_document = replace_exactly_once(
        &document_xml(&baseline),
        r#"<wp:extent cx="914400" cy="457200"/>"#,
        r#"<wp:extent cx="1828800" cy="457200"/>"#,
    );
    let changed = docx_with_document(&baseline, &changed_document);
    assert_only_document_xml_changed(&baseline, &changed);
    assert_semantics_distinguish_or_explicitly_reject(&baseline, &changed, "inline frame extent");
}

fn assert_semantics_distinguish_or_explicitly_reject(
    baseline: &[u8],
    changed: &[u8],
    description: &str,
) {
    let baseline_output = DocxAdapter
        .inspect(baseline, &InspectionProfile::default())
        .expect("valid referenced inline PNG baseline should be accepted");
    match DocxAdapter.inspect(changed, &InspectionProfile::default()) {
        Ok(changed_output) => assert_ne!(
            fingerprint(&baseline_output.semantic_projection),
            fingerprint(&changed_output.semantic_projection),
            "{description} changed the visible image while the accepted semantic fingerprint stayed equal",
        ),
        Err(error) => assert_eq!(
            error.code(),
            ErrorCode::UnsupportedSemanticConstruct,
            "{description} must be represented in identity or explicitly fail closed; valid variant was rejected for another reason: {error:?}",
        ),
    }
}

fn inline_png_docx() -> Vec<u8> {
    let fixture_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/docx/base.docx");
    let fixture = std::fs::read(fixture_path).expect("base DOCX fixture");
    let mut parts = package_parts(&fixture);
    let document = String::from_utf8(parts["word/document.xml"].clone()).expect("document XML");
    let opening_tag = "<wp:inline>";
    let inline_start = document
        .find(opening_tag)
        .expect("inline picture opening tag");
    assert_eq!(document.matches(opening_tag).count(), 1);
    let closing_tag = "</wp:inline>";
    let inline_end = document[inline_start..]
        .find(closing_tag)
        .map(|offset| inline_start + offset + closing_tag.len())
        .expect("inline picture closing tag");
    let document = format!(
        "{}{INLINE_PICTURE}{}",
        &document[..inline_start],
        &document[inline_end..]
    );
    let png = two_pixel_png();
    assert_two_pixel_png(&png);
    parts.insert("word/media/image1.png".to_owned(), png);
    assert_referenced_inline_png(&parts, &document);
    parts.insert("word/document.xml".to_owned(), document.into_bytes());
    write_package(parts)
}

fn two_pixel_png() -> Vec<u8> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header");
        writer
            .write_image_data(&[255, 0, 0, 255, 0, 0, 255, 255])
            .expect("PNG pixels");
    }
    png
}

fn assert_two_pixel_png(bytes: &[u8]) {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("referenced PNG header");
    assert_eq!(reader.info().width, 2);
    assert_eq!(reader.info().height, 1);
    let mut pixels = vec![0; reader.output_buffer_size().expect("bounded PNG buffer")];
    let frame = reader
        .next_frame(&mut pixels)
        .expect("referenced PNG pixels");
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    assert_eq!(
        &pixels[..frame.buffer_size()],
        &[255, 0, 0, 255, 0, 0, 255, 255]
    );
}

fn assert_referenced_inline_png(parts: &BTreeMap<String, Vec<u8>>, document: &str) {
    assert_eq!(document.matches("<wp:inline ").count(), 1);
    assert_eq!(document.matches("<pic:pic>").count(), 1);
    assert!(document.contains(r#"<a:blip r:embed="rIdImage"/>"#));
    assert!(parts.contains_key("word/media/image1.png"));
    let relationships = String::from_utf8(parts["word/_rels/document.xml.rels"].clone())
        .expect("document relationships XML");
    assert!(relationships.contains(r#"Id="rIdImage""#));
    assert!(relationships.contains(
        r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image""#
    ));
    assert!(relationships.contains(r#"Target="media/image1.png""#));
}

fn document_xml(docx: &[u8]) -> String {
    let parts = package_parts(docx);
    let document = String::from_utf8(parts["word/document.xml"].clone()).expect("document XML");
    assert_referenced_inline_png(&parts, &document);
    document
}

fn replace_exactly_once(source: &str, needle: &str, replacement: &str) -> String {
    assert_eq!(source.matches(needle).count(), 1, "fixture marker {needle}");
    source.replacen(needle, replacement, 1)
}

fn docx_with_document(baseline: &[u8], document: &str) -> Vec<u8> {
    let mut parts = package_parts(baseline);
    assert_referenced_inline_png(&parts, document);
    parts.insert("word/document.xml".to_owned(), document.as_bytes().to_vec());
    write_package(parts)
}

fn assert_only_document_xml_changed(baseline: &[u8], changed: &[u8]) {
    let baseline_parts = package_parts(baseline);
    let changed_parts = package_parts(changed);
    assert_eq!(
        baseline_parts.keys().collect::<Vec<_>>(),
        changed_parts.keys().collect::<Vec<_>>(),
        "both fixtures must contain the same package parts",
    );
    for (name, bytes) in &baseline_parts {
        if name != "word/document.xml" {
            assert_eq!(bytes, &changed_parts[name], "package part {name}");
        }
    }
    assert_ne!(
        baseline_parts["word/document.xml"], changed_parts["word/document.xml"],
        "the isolated picture change must alter document.xml",
    );
}

fn package_parts(docx: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(docx)).expect("DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("DOCX part");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).expect("read DOCX part");
        parts.insert(name, contents);
    }
    parts
}

fn write_package(parts: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, contents) in parts {
        writer.start_file(name, options).expect("start DOCX part");
        writer.write_all(&contents).expect("write DOCX part");
    }
    writer.finish().expect("finish DOCX").into_inner()
}
