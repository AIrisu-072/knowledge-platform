use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
    WorkerFailureCode,
};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const WORD_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

#[test]
fn image_preset_geometry_changes_identity_or_fails_closed() {
    let png = png_fixture();
    let rect_document = image_document();
    assert_eq!(rect_document.matches("prst=\"rect\"").count(), 1);
    let ellipse_document = rect_document.replace("prst=\"rect\"", "prst=\"ellipse\"");
    assert_eq!(
        ellipse_document.replace("prst=\"ellipse\"", "prst=\"rect\""),
        rect_document,
        "the only OOXML change is a:prstGeom/@prst from rect to ellipse",
    );

    let rect_docx = image_docx(&png, &rect_document);
    let ellipse_docx = image_docx(&png, &ellipse_document);
    let rect_output = inspect_valid(&rect_docx);

    let ellipse_result = DocxAdapter.inspect(&ellipse_docx, &AdapterProfile::default());
    match ellipse_result {
        Ok(ellipse_output) => assert_ne!(
            rect_output.semantic_fingerprint(),
            ellipse_output.semantic_fingerprint(),
            "ellipse geometry clips opaque corner pixels that are visible with rect geometry",
        ),
        Err(error) => assert_eq!(
            error.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "unsupported image geometry must fail closed with an explicit semantic classification",
        ),
    }
}

fn inspect_valid(bytes: &[u8]) -> SemanticAdapterOutput {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic DOCX passes coverage sentinel");
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("rect image geometry baseline should inspect successfully")
}

fn image_document() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="fixture image" descr="Opaque red square" title="Geometry fixture"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="fixture image"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#
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

    stored_zip(vec![
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", root_relationships.into_bytes()),
        ("word/document.xml", document.as_bytes().to_vec()),
        (
            "word/_rels/document.xml.rels",
            document_relationships.into_bytes(),
        ),
        ("word/media/image.png", png.to_vec()),
    ])
}

fn png_fixture() -> Vec<u8> {
    const WIDTH: u32 = 4;
    const HEIGHT: u32 = 4;

    let mut scanline = Vec::with_capacity(WIDTH as usize * 4 + 1);
    scanline.push(0); // PNG filter: None
    for _ in 0..WIDTH {
        scanline.extend_from_slice(&[0xff, 0x20, 0x20, 0xff]);
    }
    let scanlines = scanline.repeat(HEIGHT as usize);

    let mut zlib = vec![0x78, 0x01, 0x01]; // zlib header + final stored DEFLATE block
    let length = u16::try_from(scanlines.len()).expect("fixture scanline length");
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(&scanlines);
    zlib.extend_from_slice(&adler32(&scanlines).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&WIDTH.to_be_bytes());
    ihdr.extend_from_slice(&HEIGHT.to_be_bytes());
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

fn stored_zip(parts: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut archive = Vec::new();
    let mut local_offsets = Vec::with_capacity(parts.len());
    let mut checksums = Vec::with_capacity(parts.len());

    for (name, contents) in &parts {
        let name_bytes = name.as_bytes();
        let name_length = u16::try_from(name_bytes.len()).expect("ZIP part name length");
        let size = u32::try_from(contents.len()).expect("ZIP part size");
        local_offsets.push(u32::try_from(archive.len()).expect("ZIP local-header offset"));
        checksums.push(crc32(contents));

        archive.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&checksums.last().unwrap().to_le_bytes());
        archive.extend_from_slice(&size.to_le_bytes());
        archive.extend_from_slice(&size.to_le_bytes());
        archive.extend_from_slice(&name_length.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(name_bytes);
        archive.extend_from_slice(contents);
    }

    let central_directory_offset =
        u32::try_from(archive.len()).expect("ZIP central-directory offset");
    for (index, (name, contents)) in parts.iter().enumerate() {
        let name_bytes = name.as_bytes();
        let name_length = u16::try_from(name_bytes.len()).expect("ZIP part name length");
        let size = u32::try_from(contents.len()).expect("ZIP part size");
        archive.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&checksums[index].to_le_bytes());
        archive.extend_from_slice(&size.to_le_bytes());
        archive.extend_from_slice(&size.to_le_bytes());
        archive.extend_from_slice(&name_length.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u32.to_le_bytes());
        archive.extend_from_slice(&local_offsets[index].to_le_bytes());
        archive.extend_from_slice(name_bytes);
    }

    let central_directory_size = u32::try_from(archive.len())
        .expect("ZIP archive size")
        .checked_sub(central_directory_offset)
        .expect("central-directory range fits");
    let entry_count = u16::try_from(parts.len()).expect("ZIP entry count");
    archive.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&entry_count.to_le_bytes());
    archive.extend_from_slice(&entry_count.to_le_bytes());
    archive.extend_from_slice(&central_directory_size.to_le_bytes());
    archive.extend_from_slice(&central_directory_offset.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive
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
