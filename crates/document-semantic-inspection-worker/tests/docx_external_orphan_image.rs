use std::io::{Cursor, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
    WorkerFailure, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const OFFICE_REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

#[test]
fn external_picture_link_definition_is_semantic_or_fails_closed() {
    let image = tiny_png([255, 0, 0, 255]);
    let embedded = picture_docx(PictureSource::Embedded(&image));
    inspect_valid(&embedded);

    let first = picture_docx(PictureSource::External("https://first.invalid/picture.png"));
    let second = picture_docx(PictureSource::External(
        "https://second.invalid/picture.png",
    ));
    match (inspect(&first), inspect(&second)) {
        (Ok(first), Ok(second)) => assert_ne!(
            first.semantic_fingerprint(),
            second.semantic_fingerprint(),
            "a changed external image reference definition must change semantic identity",
        ),
        (Err(first), Err(second)) => {
            assert_eq!(
                first.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct
            );
            assert_eq!(
                second.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct
            );
        }
        (first, second) => panic!(
            "external image definitions must be consistently supported or rejected: {first:?}, {second:?}"
        ),
    }
}

#[test]
fn unreferenced_image_part_is_ignored_or_fails_closed() {
    let referenced = tiny_png([255, 0, 0, 255]);
    let orphan = tiny_png([0, 0, 255, 255]);
    let baseline = picture_docx(PictureSource::Embedded(&referenced));
    let with_orphan = picture_docx_with_orphan(&referenced, &orphan);

    let baseline_output = inspect_valid(&baseline);
    match inspect(&with_orphan) {
        Ok(orphan_output) => assert_eq!(
            orphan_output.semantic_fingerprint(),
            baseline_output.semantic_fingerprint(),
            "an unreferenced media part cannot change the visible document's semantic identity",
        ),
        Err(error) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "an unreferenced media part must be ignored or explicitly rejected",
        ),
    }
}

fn inspect_valid(bytes: &[u8]) -> SemanticAdapterOutput {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic DOCX passes the OOXML coverage sentinel");
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("standards-valid synthetic DOCX is accepted by the DOCX adapter")
}

fn inspect(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)?;
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn picture_docx_with_orphan(image: &[u8], orphan: &[u8]) -> Vec<u8> {
    let mut parts = picture_parts(PictureSource::Embedded(image));
    parts.push(("word/media/orphan.png", orphan.to_vec()));
    stored_docx(parts)
}

enum PictureSource<'a> {
    Embedded(&'a [u8]),
    External(&'a str),
}

fn picture_docx(source: PictureSource<'_>) -> Vec<u8> {
    stored_docx(picture_parts(source))
}

fn picture_parts(source: PictureSource<'_>) -> Vec<(&'static str, Vec<u8>)> {
    let (blip, image_relationship, media) = match source {
        PictureSource::Embedded(image) => (
            r#"<a:blip r:embed="rIdImage"/>"#,
            format!(
                r#"<Relationship Id="rIdImage" Type="{OFFICE_REL_NS}/image" Target="media/image.png"/>"#
            ),
            Some(image.to_vec()),
        ),
        PictureSource::External(target) => (
            r#"<a:blip r:link="rIdImage"/>"#,
            format!(
                r#"<Relationship Id="rIdImage" Type="{OFFICE_REL_NS}/image" Target="{target}" TargetMode="External"/>"#
            ),
            None,
        ),
    };
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{OFFICE_REL_NS}" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
<w:body><w:p><w:r><w:t>Stable picture text</w:t></w:r><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="picture" descr="A solid color pixel" title="Synthetic picture"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="picture"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill>{blip}<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p><w:sectPr/></w:body>
</w:document>"#
    );
    let document_relationships =
        format!(r#"<Relationships xmlns="{PACKAGE_REL_NS}">{image_relationship}</Relationships>"#);
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#
    );
    let mut parts = vec![
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", ROOT_RELATIONSHIPS.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        (
            "word/_rels/document.xml.rels",
            document_relationships.into_bytes(),
        ),
    ];
    if let Some(image) = media {
        parts.push(("word/media/image.png", image));
    }
    parts
}

fn stored_docx(parts: Vec<(&'static str, Vec<u8>)>) -> Vec<u8> {
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

fn tiny_png(pixel: [u8; 4]) -> Vec<u8> {
    let scanline = [0, pixel[0], pixel[1], pixel[2], pixel[3]];
    let mut zlib = vec![0x78, 0x01, 0x01];
    let length = u16::try_from(scanline.len()).expect("PNG scanline length");
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(&scanline);
    zlib.extend_from_slice(&adler32(&scanline).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
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
