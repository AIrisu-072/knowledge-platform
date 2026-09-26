use std::io::{Cursor, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

#[test]
fn referenced_header_and_footer_insertions_keep_part_qualified_provenance() {
    let earlier_final = inspect_valid(&header_footer_docx(false, "Draft heading", "Public copy"));
    let baseline = inspect_valid(&header_footer_docx(false, "Annual report", "Confidential"));
    let revised = inspect_valid(&header_footer_docx(true, "Annual report", "Confidential"));

    assert_ne!(
        earlier_final.semantic_fingerprint(),
        baseline.semantic_fingerprint(),
        "referenced header/footer text contributes to semantic identity",
    );
    assert_eq!(
        baseline.semantic_fingerprint(),
        revised.semantic_fingerprint(),
        "header/footer revisions with the same proposed-final text must preserve semantic identity",
    );

    let changes = &revised.editorial_provenance().tracked_changes;
    let mut missing = Vec::new();
    for (locator, kind, author, timestamp) in [
        (
            "word/header1.xml#deletion:17",
            "deletion",
            "Header Editor",
            "2026-08-01T02:03:04Z",
        ),
        (
            "word/header1.xml#insertion:18",
            "insertion",
            "Header Editor",
            "2026-08-01T02:03:04Z",
        ),
        (
            "word/footer1.xml#deletion:27",
            "deletion",
            "Footer Editor",
            "2026-08-02T03:04:05Z",
        ),
        (
            "word/footer1.xml#insertion:28",
            "insertion",
            "Footer Editor",
            "2026-08-02T03:04:05Z",
        ),
    ] {
        let Some(change) = changes
            .iter()
            .find(|change| change.source_locator == locator)
        else {
            missing.push(locator);
            continue;
        };
        assert_eq!(change.kind, kind, "{locator}");
        assert_eq!(change.author_label.as_deref(), Some(author), "{locator}");
        assert_eq!(change.timestamp.as_deref(), Some(timestamp), "{locator}");
        assert!(change.unresolved, "{locator}");
    }
    assert!(
        missing.is_empty(),
        "missing tracked-change evidence: {}",
        missing.join(", ")
    );
}

#[test]
fn quarter_turn_of_asymmetric_image_changes_semantic_identity_or_fails_closed() {
    let image = asymmetric_png();
    let unrotated = inspect_valid(&image_docx(&image, None));
    let rotated_package = image_docx(&image, Some(5_400_000));
    let rotated = OoxmlCoverageSentinel::validate_package(&rotated_package)
        .and_then(|()| DocxAdapter.inspect(&rotated_package, &AdapterProfile::default()));

    match rotated {
        Ok(rotated) => assert_ne!(
            unrotated.semantic_fingerprint(),
            rotated.semantic_fingerprint(),
            "a 90-degree picture transform changes the visible orientation of an asymmetric image",
        ),
        Err(error) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "an image transform the adapter cannot interpret must fail closed as unsupported",
        ),
    }
}

#[test]
fn full_turn_of_image_is_visual_noise_or_fails_closed() {
    let image = asymmetric_png();
    let unrotated = inspect_valid(&image_docx(&image, None));
    let full_turn_package = image_docx(&image, Some(21_600_000));
    let full_turn = OoxmlCoverageSentinel::validate_package(&full_turn_package)
        .and_then(|()| DocxAdapter.inspect(&full_turn_package, &AdapterProfile::default()));

    match full_turn {
        Ok(full_turn) => assert_eq!(
            unrotated.semantic_fingerprint(),
            full_turn.semantic_fingerprint(),
            "a full turn leaves the same visible image orientation",
        ),
        Err(error) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "an unmodeled image transform must fail closed",
        ),
    }
}

fn inspect_valid(bytes: &[u8]) -> document_semantic_inspection_worker::SemanticAdapterOutput {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic DOCX passes the OOXML coverage sentinel");
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("synthetic DOCX is accepted by the DOCX adapter")
}

fn header_footer_docx(
    tracked_revisions: bool,
    final_header_text: &str,
    final_footer_text: &str,
) -> Vec<u8> {
    let header = if tracked_revisions {
        format!(
            r#"<w:hdr xmlns:w="{WORD_NS}"><w:p><w:del w:id="17" w:author="Header Editor" w:date="2026-08-01T02:03:04Z"><w:r><w:delText>Draft heading</w:delText></w:r></w:del><w:ins w:id="18" w:author="Header Editor" w:date="2026-08-01T02:03:04Z"><w:r><w:t>{final_header_text}</w:t></w:r></w:ins></w:p></w:hdr>"#
        )
    } else {
        format!(
            r#"<w:hdr xmlns:w="{WORD_NS}"><w:p><w:r><w:t>{final_header_text}</w:t></w:r></w:p></w:hdr>"#
        )
    };
    let footer = if tracked_revisions {
        format!(
            r#"<w:ftr xmlns:w="{WORD_NS}"><w:p><w:del w:id="27" w:author="Footer Editor" w:date="2026-08-02T03:04:05Z"><w:r><w:delText>Public copy</w:delText></w:r></w:del><w:ins w:id="28" w:author="Footer Editor" w:date="2026-08-02T03:04:05Z"><w:r><w:t>{final_footer_text}</w:t></w:r></w:ins></w:p></w:ftr>"#
        )
    } else {
        format!(
            r#"<w:ftr xmlns:w="{WORD_NS}"><w:p><w:r><w:t>{final_footer_text}</w:t></w:r></w:p></w:ftr>"#
        )
    };
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body><w:p><w:r><w:t>Stable body</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/><w:footerReference w:type="default" r:id="rIdFooter"/></w:sectPr></w:body></w:document>"#
    );
    let relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdHeader" Type="{REL_NS}/header" Target="header1.xml"/><Relationship Id="rIdFooter" Type="{REL_NS}/footer" Target="footer1.xml"/></Relationships>"#
    );
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#
    );
    package(
        &content_types,
        &document,
        &relationships,
        vec![
            ("word/header1.xml", header.into_bytes()),
            ("word/footer1.xml", footer.into_bytes()),
        ],
    )
}

fn image_docx(image: &[u8], rotation: Option<i64>) -> Vec<u8> {
    let rotation = rotation
        .map(|angle| format!(r#" rot="{angle}""#))
        .unwrap_or_default();
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p><w:r><w:drawing><wp:inline><wp:extent cx="1828800" cy="914400"/><wp:docPr id="1" name="two-color horizontal image" descr="Red and blue horizontal cells" title="Direction-sensitive diagram"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="two-color horizontal image"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm{rotation}><a:off x="0" y="0"/><a:ext cx="1828800" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#
    );
    let relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdImage" Type="{REL_NS}/image" Target="media/image.png"/></Relationships>"#
    );
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#
    );
    package(
        &content_types,
        &document,
        &relationships,
        vec![("word/media/image.png", image.to_vec())],
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

fn asymmetric_png() -> Vec<u8> {
    // One opaque red pixel beside one opaque blue pixel; the 90-degree rotation is visible.
    let scanline = [0, 255, 0, 0, 255, 0, 0, 255, 255, 255];
    let mut zlib = vec![0x78, 0x01];
    let length = u16::try_from(scanline.len()).expect("PNG scanline length");
    zlib.push(1); // One final, uncompressed DEFLATE block.
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(&scanline);
    zlib.extend_from_slice(&adler32(&scanline).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&2u32.to_be_bytes());
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
