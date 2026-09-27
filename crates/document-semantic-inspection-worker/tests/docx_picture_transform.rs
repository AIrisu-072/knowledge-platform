use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
    WorkerFailureCode,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const WORD_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

#[test]
fn referenced_inline_png_crop_changes_identity_or_fails_closed() {
    assert_picture_xml_change_is_semantic_or_unsupported(
        r#"<a:blip r:embed="rIdImage"/>"#,
        r#"<a:blip r:embed="rIdImage"/><a:srcRect l="50000"/>"#,
        "cropping away the left half of an asymmetric image changes its visible content",
    );
}

#[test]
fn referenced_inline_png_rotation_changes_identity_or_fails_closed() {
    assert_picture_xml_change_is_semantic_or_unsupported(
        r#"<a:xfrm rot="0">"#,
        r#"<a:xfrm rot="5400000">"#,
        "a quarter-turn changes the orientation of the asymmetric image",
    );
}

#[test]
fn referenced_inline_png_horizontal_flip_changes_identity_or_fails_closed() {
    assert_picture_xml_change_is_semantic_or_unsupported(
        r#"<a:xfrm rot="0">"#,
        r#"<a:xfrm rot="0" flipH="1">"#,
        "a horizontal flip changes the orientation of the asymmetric image",
    );
}

#[test]
fn referenced_inline_png_vertical_flip_changes_identity_or_fails_closed() {
    assert_picture_xml_change_is_semantic_or_unsupported(
        r#"<a:xfrm rot="0">"#,
        r#"<a:xfrm rot="0" flipV="1">"#,
        "a vertical flip changes the orientation of the asymmetric image",
    );
}

#[test]
fn referenced_inline_png_shape_width_changes_identity_or_fails_closed() {
    assert_picture_xml_change_is_semantic_or_unsupported(
        r#"<a:ext cx="914400" cy="914400"/>"#,
        r#"<a:ext cx="1828800" cy="914400"/>"#,
        "changing only the picture transform width changes its rendered shape",
    );
}

#[test]
fn referenced_inline_png_shape_height_changes_identity_or_fails_closed() {
    assert_picture_xml_change_is_semantic_or_unsupported(
        r#"<a:ext cx="914400" cy="914400"/>"#,
        r#"<a:ext cx="914400" cy="1828800"/>"#,
        "changing only the picture transform height changes its rendered shape",
    );
}

#[test]
fn referenced_inline_png_transform_x_offset_changes_identity_or_fails_closed() {
    assert_picture_xml_change_is_semantic_or_unsupported(
        r#"<a:off x="0" y="0"/>"#,
        r#"<a:off x="914400" y="0"/>"#,
        "changing only the picture transform x position changes its placement",
    );
}

fn assert_picture_xml_change_is_semantic_or_unsupported(before: &str, after: &str, reason: &str) {
    let baseline_xml = image_document();
    assert_eq!(baseline_xml.matches(before).count(), 1, "{reason}");
    let changed_xml = baseline_xml.replacen(before, after, 1);
    assert_eq!(
        changed_xml.replacen(after, before, 1),
        baseline_xml,
        "{reason}"
    );

    let png = asymmetric_png();
    let baseline_package = image_docx(&png, &baseline_xml);
    let changed_package = image_docx(&png, &changed_xml);
    assert_only_document_xml_differs(&baseline_package, &changed_package);

    let baseline = inspect_valid(&baseline_package);
    let changed = OoxmlCoverageSentinel::validate_package(&changed_package)
        .and_then(|()| DocxAdapter.inspect(&changed_package, &AdapterProfile::default()));
    match changed {
        Ok(changed) => assert_ne!(
            baseline.semantic_fingerprint(),
            changed.semantic_fingerprint(),
            "{reason}; the only OOXML change is the selected picture transform field",
        ),
        Err(error) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "{reason}; an unprojected picture transform must fail closed, got {error}",
        ),
    }
}

fn inspect_valid(bytes: &[u8]) -> SemanticAdapterOutput {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic DOCX passes the OOXML coverage sentinel");
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("baseline inline PNG DOCX is accepted by the adapter")
}

fn image_document() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="asymmetric fixture image" descr="Four colored quadrants" title="Picture transform fixture"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="asymmetric fixture image"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm rot="0"><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#
    )
}

fn image_docx(png: &[u8], document: &str) -> Vec<u8> {
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="{WORD_MAIN}"/></Types>"#
    );
    let root_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#
    );
    let document_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdImage" Type="{REL_NS}/image" Target="media/image.png"/></Relationships>"#
    );

    let parts = [
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", root_relationships.into_bytes()),
        ("word/document.xml", document.as_bytes().to_vec()),
        (
            "word/_rels/document.xml.rels",
            document_relationships.into_bytes(),
        ),
        ("word/media/image.png", png.to_vec()),
    ];
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("start synthetic DOCX part");
        writer
            .write_all(&contents)
            .expect("write synthetic DOCX part");
    }
    writer
        .finish()
        .expect("finish synthetic DOCX ZIP")
        .into_inner()
}

fn assert_only_document_xml_differs(baseline: &[u8], changed: &[u8]) {
    let baseline_parts = zip_parts(baseline);
    let changed_parts = zip_parts(changed);
    assert_eq!(baseline_parts.len(), changed_parts.len());

    let mut changed_names = Vec::new();
    for ((baseline_name, baseline_bytes), (changed_name, changed_bytes)) in
        baseline_parts.iter().zip(&changed_parts)
    {
        assert_eq!(baseline_name, changed_name, "ZIP part order is unchanged");
        if baseline_bytes != changed_bytes {
            changed_names.push(baseline_name.as_str());
        }
    }
    assert_eq!(changed_names, ["word/document.xml"]);
}

fn zip_parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("synthetic DOCX ZIP is valid");
    let mut parts = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("read synthetic DOCX entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("read synthetic DOCX entry contents");
        parts.push((name, contents));
    }
    parts
}

fn asymmetric_png() -> Vec<u8> {
    // Red/green over blue/yellow makes crop, flips, and rotation distinguishable.
    let scanlines = [
        0, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 255, 255, 255, 255, 0, 255,
    ];
    let mut zlib = vec![0x78, 0x01, 0x01]; // zlib header + final stored DEFLATE block
    let length = u16::try_from(scanlines.len()).expect("PNG scanline length");
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(&scanlines);
    zlib.extend_from_slice(&adler32(&scanlines).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&2u32.to_be_bytes());
    ihdr.extend_from_slice(&2u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // RGBA, 8 bits per channel
    append_png_chunk(&mut png, *b"IHDR", &ihdr);
    append_png_chunk(&mut png, *b"IDAT", &zlib);
    append_png_chunk(&mut png, *b"IEND", &[]);
    png
}

fn append_png_chunk(output: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    output.extend_from_slice(
        &u32::try_from(data.len())
            .expect("PNG chunk size")
            .to_be_bytes(),
    );
    output.extend_from_slice(&kind);
    output.extend_from_slice(data);
    let mut checksum_input = kind.to_vec();
    checksum_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&checksum_input).to_be_bytes());
}

fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let (mut first, mut second) = (1u32, 0u32);
    for byte in bytes {
        first = (first + u32::from(*byte)) % MODULUS;
        second = (second + first) % MODULUS;
    }
    (second << 16) | first
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
