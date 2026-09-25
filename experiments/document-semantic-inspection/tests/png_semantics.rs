use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile, PocError,
    fingerprint as semantic_fingerprint,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

type PngChunk = ([u8; 4], Vec<u8>);

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const DRAWING_DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
 <w:body><w:p><w:r><w:t>fixture</w:t></w:r></w:p><w:p><w:r><w:drawing>
  <wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="Image"/>
   <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">
    <pic:pic><pic:blipFill><a:blip r:embed="rIdImage"/></pic:blipFill></pic:pic>
   </a:graphicData></a:graphic>
  </wp:inline>
 </w:drawing></w:r></w:p><w:sectPr/></w:body>
</w:document>"#;

fn docx_with_png(png: &[u8]) -> Vec<u8> {
    let parts = BTreeMap::from([
        (
            "[Content_Types].xml".to_owned(),
            br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec(),
        ),
        (
            "_rels/.rels".to_owned(),
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec(),
        ),
        (
            "word/document.xml".to_owned(),
            DRAWING_DOCUMENT.as_bytes().to_vec(),
        ),
        (
            "word/_rels/document.xml.rels".to_owned(),
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#.to_vec(),
        ),
        ("word/media/image1.png".to_owned(), png.to_vec()),
    ]);

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, bytes) in parts {
        writer.start_file(name, options).expect("start DOCX part");
        writer.write_all(&bytes).expect("write DOCX part");
    }
    writer.finish().expect("finish DOCX").into_inner()
}

fn inspect_png(png: &[u8]) -> Result<[u8; 32], PocError> {
    let output = DocxAdapter.inspect(&docx_with_png(png), &InspectionProfile::default())?;
    Ok(semantic_fingerprint(&output.semantic_projection))
}

fn assert_png_error(png: &[u8], expected: ErrorCode) {
    let error = inspect_png(png).expect_err("PNG must fail closed");
    assert_eq!(error.code(), expected, "{error}");
}

fn rgba_png(pixel: [u8; 4], filter: u8, idat_split: Option<usize>) -> Vec<u8> {
    let mut scanline = vec![filter];
    scanline.extend_from_slice(&pixel);
    let zlib = zlib_stored(&scanline);
    let ihdr = [
        0, 0, 0, 1, // width
        0, 0, 0, 1, // height
        8, 6, 0, 0, 0, // depth, RGBA, compression, filter, interlace
    ];
    let mut chunks = vec![(*b"IHDR", ihdr.to_vec())];
    match idat_split {
        Some(split) => {
            assert!(split > 0 && split < zlib.len(), "interior IDAT split");
            chunks.push((*b"IDAT", zlib[..split].to_vec()));
            chunks.push((*b"IDAT", zlib[split..].to_vec()));
        }
        None => chunks.push((*b"IDAT", zlib)),
    }
    chunks.push((*b"IEND", Vec::new()));
    encode_png_chunks(&chunks)
}

fn png_with_dimensions(width: u32, height: u32) -> Vec<u8> {
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    encode_png_chunks(&[
        (*b"IHDR", ihdr),
        (*b"IDAT", Vec::new()),
        (*b"IEND", Vec::new()),
    ])
}

fn indexed_png(index: u8, background_index: Option<u8>) -> Vec<u8> {
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&[1, 3, 0, 0, 0]); // 1-bit indexed color
    let mut chunks = vec![(*b"IHDR", ihdr), (*b"PLTE", vec![0x12, 0x34, 0x56])];
    if let Some(background_index) = background_index {
        chunks.push((*b"bKGD", vec![background_index]));
    }
    chunks.push((*b"IDAT", zlib_stored(&[0, index])));
    chunks.push((*b"IEND", Vec::new()));
    encode_png_chunks(&chunks)
}

fn rgb_png(
    pixel: [u8; 3],
    background: Option<[u16; 3]>,
    physical_dimensions: Option<(u32, u32)>,
) -> Vec<u8> {
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB
    let mut chunks = vec![(*b"IHDR", ihdr)];
    if let Some(background) = background {
        let mut data = Vec::with_capacity(6);
        for channel in background {
            data.extend_from_slice(&channel.to_be_bytes());
        }
        chunks.push((*b"bKGD", data));
    }
    if let Some((x, y)) = physical_dimensions {
        let mut data = Vec::with_capacity(9);
        data.extend_from_slice(&x.to_be_bytes());
        data.extend_from_slice(&y.to_be_bytes());
        data.push(0); // unit unknown; x/y still define pixel aspect ratio
        chunks.push((*b"pHYs", data));
    }
    let mut scanline = vec![0];
    scanline.extend_from_slice(&pixel);
    chunks.push((*b"IDAT", zlib_stored(&scanline)));
    chunks.push((*b"IEND", Vec::new()));
    encode_png_chunks(&chunks)
}

fn parse_png_chunks(png: &[u8]) -> Vec<PngChunk> {
    assert!(png.starts_with(PNG_SIGNATURE), "PNG signature");
    let mut chunks = Vec::new();
    let mut offset = PNG_SIGNATURE.len();
    while offset < png.len() {
        assert!(png.len() - offset >= 12, "complete PNG chunk");
        let length =
            u32::from_be_bytes(png[offset..offset + 4].try_into().expect("chunk length")) as usize;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(length)
            .expect("chunk length overflow");
        let chunk_end = data_end.checked_add(4).expect("CRC offset overflow");
        assert!(chunk_end <= png.len(), "complete PNG chunk body");
        let kind = png[offset + 4..data_start].try_into().expect("chunk type");
        chunks.push((kind, png[data_start..data_end].to_vec()));
        offset = chunk_end;
    }
    chunks
}

fn encode_png_chunks(chunks: &[PngChunk]) -> Vec<u8> {
    let mut png = PNG_SIGNATURE.to_vec();
    for (kind, data) in chunks {
        png.extend_from_slice(
            &u32::try_from(data.len())
                .expect("small test chunk")
                .to_be_bytes(),
        );
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        let mut crc_input = Vec::with_capacity(4 + data.len());
        crc_input.extend_from_slice(kind);
        crc_input.extend_from_slice(data);
        png.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    }
    png
}

fn insert_after_ihdr(png: &[u8], kind: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunks = parse_png_chunks(png);
    let ihdr = chunks
        .iter()
        .position(|(chunk_kind, _)| chunk_kind == b"IHDR")
        .expect("IHDR");
    chunks.insert(ihdr + 1, (kind, data.to_vec()));
    encode_png_chunks(&chunks)
}

fn corrupt_chunk_crc(png: &[u8], target: &[u8; 4]) -> Vec<u8> {
    let mut corrupted = png.to_vec();
    let mut offset = PNG_SIGNATURE.len();
    while offset < corrupted.len() {
        let length = u32::from_be_bytes(
            corrupted[offset..offset + 4]
                .try_into()
                .expect("chunk length"),
        ) as usize;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(length)
            .expect("chunk length overflow");
        let chunk_end = data_end.checked_add(4).expect("CRC offset overflow");
        assert!(chunk_end <= corrupted.len(), "complete PNG chunk");
        if &corrupted[offset + 4..offset + 8] == target {
            corrupted[data_end] ^= 1;
            return corrupted;
        }
        offset = chunk_end;
    }
    panic!("missing target chunk {target:?}");
}

fn corrupt_adler32(png: &[u8]) -> Vec<u8> {
    let mut chunks = parse_png_chunks(png);
    let (_, data) = chunks
        .iter_mut()
        .find(|(kind, _)| kind == b"IDAT")
        .expect("IDAT");
    assert!(data.len() >= 4, "zlib Adler-32 checksum");
    let last = data.len() - 1;
    data[last] ^= 1;
    encode_png_chunks(&chunks)
}

fn zlib_stored(bytes: &[u8]) -> Vec<u8> {
    let length = u16::try_from(bytes.len()).expect("small scanline");
    let mut zlib = vec![0x78, 0x01, 0x01]; // zlib header and one final stored DEFLATE block
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(bytes);
    zlib.extend_from_slice(&adler32(bytes).to_be_bytes());
    zlib
}

fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % MODULUS;
        b = (b + a) % MODULUS;
    }
    (b << 16) | a
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
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

#[test]
fn valid_png_is_accepted_and_pixel_changes_are_semantic() {
    let first = inspect_png(&rgba_png([19, 77, 141, 255], 0, None)).expect("valid PNG");
    let changed = inspect_png(&rgba_png([19, 77, 142, 255], 0, None)).expect("valid PNG");
    assert_ne!(first, changed, "a changed decoded pixel changes identity");
}

#[test]
fn equivalent_png_filters_and_idat_segmentation_are_noise() {
    let pixel = [19, 77, 141, 255];
    let expected = inspect_png(&rgba_png(pixel, 0, None)).expect("filter None PNG");
    for filter in 1..=4 {
        let encoded = rgba_png(pixel, filter, None);
        assert_ne!(encoded, rgba_png(pixel, 0, None));
        assert_eq!(inspect_png(&encoded).expect("valid filtered PNG"), expected);
    }

    let segmented = rgba_png(pixel, 0, Some(2));
    assert_eq!(inspect_png(&segmented).expect("split IDAT PNG"), expected);
}

#[test]
fn png_crc_corruption_fails_closed() {
    let png = rgba_png([19, 77, 141, 255], 0, None);
    assert_png_error(
        &corrupt_chunk_crc(&png, b"IDAT"),
        ErrorCode::SemanticExtractionFailed,
    );
}

#[test]
fn png_adler32_corruption_fails_closed() {
    let png = rgba_png([19, 77, 141, 255], 0, None);
    assert_png_error(&corrupt_adler32(&png), ErrorCode::SemanticExtractionFailed);
}

#[test]
fn invalid_png_chunk_order_fails_closed() {
    let png = rgba_png([19, 77, 141, 255], 0, None);
    let mut chunks = parse_png_chunks(&png);
    chunks.swap(0, 1); // IDAT before IHDR
    assert_png_error(
        &encode_png_chunks(&chunks),
        ErrorCode::SemanticExtractionFailed,
    );

    let split = rgba_png([19, 77, 141, 255], 0, Some(2));
    let mut chunks = parse_png_chunks(&split);
    let first_idat = chunks
        .iter()
        .position(|(kind, _)| kind == b"IDAT")
        .expect("first IDAT");
    chunks.insert(first_idat + 1, (*b"tEXt", b"x\0y".to_vec()));
    assert_png_error(
        &encode_png_chunks(&chunks), // IDAT split by an ancillary chunk
        ErrorCode::SemanticExtractionFailed,
    );
}

#[test]
fn bytes_after_png_iend_fail_closed() {
    let mut png = rgba_png([19, 77, 141, 255], 0, None);
    png.extend_from_slice(b"trailing");
    assert_png_error(&png, ErrorCode::SemanticExtractionFailed);
}

#[test]
fn apng_is_rejected_as_unsupported() {
    let png = rgba_png([19, 77, 141, 255], 0, None);
    let animated = insert_after_ihdr(&png, *b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]);
    assert_png_error(&animated, ErrorCode::UnsupportedSemanticConstruct);
}

#[test]
fn indexed_png_palette_index_out_of_range_fails_closed() {
    // The 1-bit image can encode index 1, but PLTE contains only index 0.
    let invalid = indexed_png(1, None);
    let error = inspect_png(&invalid).expect_err("out-of-range palette index");
    assert!(
        matches!(
            error.code(),
            ErrorCode::SemanticExtractionFailed | ErrorCode::UnsupportedSemanticConstruct
        ),
        "invalid indexed pixel must fail closed; got {error}"
    );
}

#[test]
fn equivalent_indexed_and_rgb_background_colors_are_noise() {
    let indexed = indexed_png(0, Some(0));
    let rgb = rgb_png([0x12, 0x34, 0x56], Some([0x1212, 0x3434, 0x5656]), None);
    assert_eq!(
        inspect_png(&indexed).expect("indexed PNG"),
        inspect_png(&rgb).expect("RGB PNG"),
        "same pixels and same bKGD color should share semantic identity"
    );
}

#[test]
fn equivalent_unknown_unit_physical_dimension_ratios_are_noise() {
    let first = rgb_png([0x12, 0x34, 0x56], None, Some((72, 36)));
    let scaled = rgb_png([0x12, 0x34, 0x56], None, Some((144, 72)));
    assert_eq!(
        inspect_png(&first).expect("first pHYs PNG"),
        inspect_png(&scaled).expect("scaled pHYs PNG"),
        "unit-unknown pHYs values with the same ratio should be equivalent"
    );
}

#[test]
fn unsupported_critical_chunks_and_color_profiles_are_rejected() {
    let png = rgba_png([19, 77, 141, 255], 0, None);
    let unknown_critical = insert_after_ihdr(&png, *b"ABCD", &[]);
    assert_png_error(&unknown_critical, ErrorCode::UnsupportedSemanticConstruct);

    let srgb_profile = insert_after_ihdr(&png, *b"sRGB", &[0]);
    assert_png_error(&srgb_profile, ErrorCode::UnsupportedSemanticConstruct);
}

#[test]
fn decoded_pixel_and_output_budget_rejects_oversized_dimensions_before_decode() {
    // 8192 x 8193 exceeds the configured 64 Mi-pixel budget by one row.
    let oversized = png_with_dimensions(8_192, 8_193);
    assert_png_error(&oversized, ErrorCode::InspectionResourceLimitExceeded);
}
