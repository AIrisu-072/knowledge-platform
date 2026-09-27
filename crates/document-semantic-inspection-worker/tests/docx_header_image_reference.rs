use std::io::{Cursor, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const DRAWING_NAMESPACES: &str = r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;

const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

#[test]
fn referenced_header_and_footer_images_change_semantics_or_fail_closed() {
    let baseline_package =
        referenced_images_docx([220, 20, 30, 255], [20, 40, 220, 255], "default", "default");
    let baseline = inspect_valid(&baseline_package);

    let changed_header =
        referenced_images_docx([20, 200, 40, 255], [20, 40, 220, 255], "default", "default");
    assert_image_change_is_semantic_or_unsupported(
        baseline.semantic_fingerprint(),
        &changed_header,
        "referenced header image",
    );

    let changed_footer = referenced_images_docx(
        [220, 20, 30, 255],
        [230, 190, 10, 255],
        "default",
        "default",
    );
    assert_image_change_is_semantic_or_unsupported(
        baseline.semantic_fingerprint(),
        &changed_footer,
        "referenced footer image",
    );
}

#[test]
fn header_and_footer_image_relationship_targets_change_semantics_or_fail_closed() {
    let baseline_package = referenced_images_docx_with_targets(
        [220, 20, 30, 255],
        [20, 40, 220, 255],
        "default",
        "default",
        "header.png",
        "footer.png",
    );
    let baseline = inspect_valid(&baseline_package);

    // Keep both media parts and their bytes in every package. Only the part
    // relationship target changes, so a package-wide media hash cannot pass.
    let changed_header = referenced_images_docx_with_targets(
        [220, 20, 30, 255],
        [20, 40, 220, 255],
        "default",
        "default",
        "footer.png",
        "footer.png",
    );
    assert_image_change_is_semantic_or_unsupported(
        baseline.semantic_fingerprint(),
        &changed_header,
        "referenced header image target",
    );

    let changed_footer = referenced_images_docx_with_targets(
        [220, 20, 30, 255],
        [20, 40, 220, 255],
        "default",
        "default",
        "header.png",
        "header.png",
    );
    assert_image_change_is_semantic_or_unsupported(
        baseline.semantic_fingerprint(),
        &changed_footer,
        "referenced footer image target",
    );
}

#[test]
fn unknown_header_reference_type_fails_closed() {
    assert_unsupported_reference_type("bogus", "default", "headerReference");
}

#[test]
fn unknown_footer_reference_type_fails_closed() {
    assert_unsupported_reference_type("default", "bogus", "footerReference");
}

fn inspect_valid(bytes: &[u8]) -> document_semantic_inspection_worker::SemanticAdapterOutput {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("standards-valid header/footer image DOCX passes the coverage sentinel");
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("valid referenced header/footer image DOCX is accepted")
}

fn assert_image_change_is_semantic_or_unsupported(
    baseline: document_semantic_inspection_core::SemanticFingerprint,
    changed_package: &[u8],
    label: &str,
) {
    match DocxAdapter.inspect(changed_package, &AdapterProfile::default()) {
        Ok(changed) => assert_ne!(
            baseline,
            changed.semantic_fingerprint(),
            "a visible {label} change must alter the semantic fingerprint",
        ),
        Err(error) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "an uninspected {label} must fail closed as unsupported: {error}",
        ),
    }
}

fn assert_unsupported_reference_type(header_type: &str, footer_type: &str, label: &str) {
    let bytes = referenced_images_docx(
        [220, 20, 30, 255],
        [20, 40, 220, 255],
        header_type,
        footer_type,
    );
    let error = match DocxAdapter.inspect(&bytes, &AdapterProfile::default()) {
        Ok(_) => panic!("unknown {label} type must fail closed"),
        Err(error) => error,
    };

    assert_eq!(
        error.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct,
        "unknown {label} type must fail closed as unsupported: {error}",
    );
}

fn referenced_images_docx(
    header_pixel: [u8; 4],
    footer_pixel: [u8; 4],
    header_type: &str,
    footer_type: &str,
) -> Vec<u8> {
    referenced_images_docx_with_targets(
        header_pixel,
        footer_pixel,
        header_type,
        footer_type,
        "header.png",
        "footer.png",
    )
}

fn referenced_images_docx_with_targets(
    header_pixel: [u8; 4],
    footer_pixel: [u8; 4],
    header_type: &str,
    footer_type: &str,
    header_target: &str,
    footer_target: &str,
) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body><w:p><w:r><w:t>Stable body</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="{header_type}" r:id="rIdHeader"/><w:footerReference w:type="{footer_type}" r:id="rIdFooter"/><w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:body></w:document>"#
    );
    let document_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdHeader" Type="{REL_NS}/header" Target="header1.xml"/><Relationship Id="rIdFooter" Type="{REL_NS}/footer" Target="footer1.xml"/></Relationships>"#
    );
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#
    );
    let header = drawing_part("hdr", "header image", "rIdHeaderImage", 1);
    let footer = drawing_part("ftr", "footer image", "rIdFooterImage", 2);
    let header_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdHeaderImage" Type="{REL_NS}/image" Target="media/{header_target}"/></Relationships>"#
    );
    let footer_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdFooterImage" Type="{REL_NS}/image" Target="media/{footer_target}"/></Relationships>"#
    );

    package(
        &content_types,
        &document,
        &document_relationships,
        vec![
            ("word/header1.xml", header.into_bytes()),
            (
                "word/_rels/header1.xml.rels",
                header_relationships.into_bytes(),
            ),
            ("word/footer1.xml", footer.into_bytes()),
            (
                "word/_rels/footer1.xml.rels",
                footer_relationships.into_bytes(),
            ),
            ("word/media/header.png", rgba_png(header_pixel)),
            ("word/media/footer.png", rgba_png(footer_pixel)),
        ],
    )
}

fn drawing_part(root: &str, name: &str, relationship_id: &str, doc_pr_id: u32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{root} xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}" {DRAWING_NAMESPACES}><w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="{doc_pr_id}" name="{name}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="{name}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{relationship_id}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:{root}>"#
    )
}

fn package(
    content_types: &str,
    document: &str,
    document_relationships: &str,
    extras: Vec<(&str, Vec<u8>)>,
) -> Vec<u8> {
    let mut parts = vec![
        ("[Content_Types].xml", content_types.as_bytes().to_vec()),
        ("_rels/.rels", ROOT_RELATIONSHIPS.as_bytes().to_vec()),
        ("word/document.xml", document.as_bytes().to_vec()),
        (
            "word/_rels/document.xml.rels",
            document_relationships.as_bytes().to_vec(),
        ),
    ];
    parts.extend(extras);

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

fn rgba_png(pixel: [u8; 4]) -> Vec<u8> {
    let scanline = [0, pixel[0], pixel[1], pixel[2], pixel[3]];
    let length = u16::try_from(scanline.len()).expect("PNG scanline length");
    let mut zlib = vec![0x78, 0x01, 0x01];
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

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn append_png_chunk(png: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    png.extend_from_slice(
        &u32::try_from(data.len())
            .expect("PNG chunk size")
            .to_be_bytes(),
    );
    png.extend_from_slice(&kind);
    png.extend_from_slice(data);
    let mut checksum_input = kind.to_vec();
    checksum_input.extend_from_slice(data);
    png.extend_from_slice(&crc32(&checksum_input).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut checksum = !0u32;
    for byte in bytes {
        checksum ^= u32::from(*byte);
        for _ in 0..8 {
            checksum = if checksum & 1 == 0 {
                checksum >> 1
            } else {
                (checksum >> 1) ^ 0xedb8_8320
            };
        }
    }
    !checksum
}
