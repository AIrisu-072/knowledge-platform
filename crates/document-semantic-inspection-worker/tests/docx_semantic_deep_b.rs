use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const WORD_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

#[test]
fn styles_outline_level_changes_heading_structure() {
    let level_one = heading_docx("0");
    let level_two = heading_docx("1");

    let level_one = inspect_valid(&level_one);
    let level_two = inspect_valid(&level_two);

    assert_ne!(
        level_one.semantic_fingerprint(),
        level_two.semantic_fingerprint(),
        "a referenced style's outlineLvl changes the heading level and must affect the fingerprint",
    );
}

#[test]
fn image_docpr_description_and_title_are_semantic_alt_text() {
    let image = png_fixture(None);
    let unlabeled = image_docx(&image, "", "");
    let labeled = image_docx(&image, "A red pixel", "Red pixel diagram");

    let unlabeled = inspect_valid(&unlabeled);
    let labeled = inspect_valid(&labeled);

    assert_ne!(
        unlabeled.semantic_fingerprint(),
        labeled.semantic_fingerprint(),
        "wp:docPr descr/title carry image alternative text and must affect semantic identity",
    );
}

#[test]
fn png_idat_reencoding_with_identical_pixels_preserves_fingerprint() {
    let one_deflate_block = png_fixture(None);
    let two_deflate_blocks = png_fixture(Some(2));
    let (one_ihdr, one_idat) = png_chunks(&one_deflate_block);
    let (two_ihdr, two_idat) = png_chunks(&two_deflate_blocks);
    let one_scanline = decode_stored_zlib(&one_idat);
    let two_scanline = decode_stored_zlib(&two_idat);

    assert_eq!(
        one_ihdr, two_ihdr,
        "PNG dimensions and color model must match"
    );
    assert_ne!(one_idat, two_idat, "the PNG encodings must differ");
    assert_eq!(one_scanline, two_scanline, "the decoded pixels must match");
    assert_eq!(one_scanline, [0, 0x21, 0x42, 0x63, 0xff]);

    let first = image_docx(&one_deflate_block, "same image", "same title");
    let second = image_docx(&two_deflate_blocks, "same image", "same title");
    let first = inspect_valid(&first);
    let second = inspect_valid(&second);

    assert_eq!(
        first.semantic_fingerprint(),
        second.semantic_fingerprint(),
        "meaning-equivalent PNG re-encoding must preserve visual semantic identity",
    );
}

#[test]
fn content_types_root_must_use_the_opc_namespace() {
    let document = text_document();
    let valid = text_docx(
        &content_types_xml(CONTENT_TYPES_NS, false, false),
        &document,
    );
    inspect_valid(&valid);

    let wrong_namespace = text_docx(
        &content_types_xml("urn:example:not-opc-content-types", false, false),
        &document,
    );

    assert!(
        OoxmlCoverageSentinel::validate_package(&wrong_namespace).is_err(),
        "[Content_Types].xml elements in a non-OPC namespace must fail closed",
    );
    assert!(
        DocxAdapter
            .inspect(&wrong_namespace, &AdapterProfile::default())
            .is_err(),
        "the semantic adapter must reject a package with a non-OPC content-types namespace",
    );
}

fn inspect_valid(bytes: &[u8]) -> SemanticAdapterOutput {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic DOCX passes coverage sentinel");
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("synthetic DOCX should inspect successfully")
}

fn text_document() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body><w:p><w:r><w:t>Stable body</w:t></w:r></w:p></w:body></w:document>"#
    )
}

fn heading_docx(outline_level: &str) -> Vec<u8> {
    let content_types = content_types_xml(CONTENT_TYPES_NS, true, false);
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body><w:p><w:pPr><w:pStyle w:val="BodyHeading"/></w:pPr><w:r><w:t>Same heading text</w:t></w:r></w:p></w:body></w:document>"#
    );
    let relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdStyles" Type="{REL_NS}/styles" Target="styles.xml"/></Relationships>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{WORD_NS}"><w:style w:type="paragraph" w:styleId="BodyHeading"><w:name w:val="Body Heading"/><w:pPr><w:outlineLvl w:val="{outline_level}"/></w:pPr></w:style></w:styles>"#
    );
    package(
        &content_types,
        &document,
        &relationships,
        vec![("word/styles.xml", styles.into_bytes())],
    )
}

fn image_docx(png: &[u8], description: &str, title: &str) -> Vec<u8> {
    let content_types = content_types_xml(CONTENT_TYPES_NS, false, true);
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="fixture image" descr="{description}" title="{title}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="fixture image"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#
    );
    let relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdImage" Type="{REL_NS}/image" Target="media/image.png"/></Relationships>"#
    );
    package(
        &content_types,
        &document,
        &relationships,
        vec![("word/media/image.png", png.to_vec())],
    )
}

fn text_docx(content_types: &str, document: &str) -> Vec<u8> {
    let relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{PACKAGE_REL_NS}"/>"#
    );
    package(content_types, document, &relationships, Vec::new())
}

fn content_types_xml(namespace: &str, styles: bool, png: bool) -> String {
    let style_override = if styles {
        r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#
    } else {
        ""
    };
    let png_default = if png {
        r#"<Default Extension="png" ContentType="image/png"/>"#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{namespace}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>{png_default}<Override PartName="/word/document.xml" ContentType="{WORD_MAIN}"/>{style_override}</Types>"#
    )
}

fn package(
    content_types: &str,
    document: &str,
    document_relationships: &str,
    extras: Vec<(&str, Vec<u8>)>,
) -> Vec<u8> {
    let mut parts = vec![
        (
            "[Content_Types].xml".to_owned(),
            content_types.as_bytes().to_vec(),
        ),
        (
            "_rels/.rels".to_owned(),
            ROOT_RELATIONSHIPS.as_bytes().to_vec(),
        ),
        ("word/document.xml".to_owned(), document.as_bytes().to_vec()),
        (
            "word/_rels/document.xml.rels".to_owned(),
            document_relationships.as_bytes().to_vec(),
        ),
    ];
    parts.extend(
        extras
            .into_iter()
            .map(|(name, bytes)| (name.to_owned(), bytes)),
    );
    stored_zip(parts)
}

fn png_fixture(split_at: Option<usize>) -> Vec<u8> {
    let scanline = [0, 0x21, 0x42, 0x63, 0xff];
    let mut zlib = vec![0x78, 0x01];
    match split_at {
        Some(split) => {
            append_stored_deflate_block(&mut zlib, false, &scanline[..split]);
            append_stored_deflate_block(&mut zlib, true, &scanline[split..]);
        }
        None => append_stored_deflate_block(&mut zlib, true, &scanline),
    }
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

fn append_stored_deflate_block(output: &mut Vec<u8>, final_block: bool, data: &[u8]) {
    assert!(data.len() <= u16::MAX as usize);
    output.push(u8::from(final_block));
    let length = u16::try_from(data.len()).expect("stored block length");
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(&(!length).to_le_bytes());
    output.extend_from_slice(data);
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

fn png_chunks(png: &[u8]) -> (Vec<u8>, Vec<u8>) {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut offset = 8;
    let mut ihdr = None;
    let mut idat = Vec::new();
    let mut saw_iend = false;
    while offset < png.len() {
        assert!(
            png.len() - offset >= 12,
            "complete PNG chunk header and CRC"
        );
        let length = u32::from_be_bytes(png[offset..offset + 4].try_into().unwrap()) as usize;
        let kind = &png[offset + 4..offset + 8];
        let data_start = offset + 8;
        let data_end = data_start + length;
        let chunk_end = data_end + 4;
        assert!(chunk_end <= png.len(), "PNG chunk fits in fixture");
        let data = &png[data_start..data_end];
        let expected_crc = u32::from_be_bytes(png[data_end..chunk_end].try_into().unwrap());
        let mut checksum_input = kind.to_vec();
        checksum_input.extend_from_slice(data);
        assert_eq!(crc32(&checksum_input), expected_crc, "PNG chunk CRC");
        match kind {
            b"IHDR" => ihdr = Some(data.to_vec()),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => {
                assert!(data.is_empty(), "IEND has no payload");
                saw_iend = true;
                offset = chunk_end;
                break;
            }
            _ => panic!("unexpected PNG chunk in fixture: {kind:?}"),
        }
        offset = chunk_end;
    }
    assert!(saw_iend, "PNG has IEND");
    assert_eq!(offset, png.len(), "no trailing PNG bytes");
    let ihdr = ihdr.expect("PNG has IHDR");
    assert_eq!(&ihdr[..8], &[0, 0, 0, 1, 0, 0, 0, 1]);
    assert_eq!(&ihdr[8..], &[8, 6, 0, 0, 0]);
    (ihdr, idat)
}

fn decode_stored_zlib(zlib: &[u8]) -> Vec<u8> {
    assert!(zlib.len() >= 6, "zlib header, block, and checksum");
    let header = u16::from_be_bytes([zlib[0], zlib[1]]);
    assert_eq!(zlib[0] & 0x0f, 8, "DEFLATE compression method");
    assert_eq!(header % 31, 0, "zlib FCHECK");
    assert_eq!(
        zlib[1] & 0x20,
        0,
        "PNG fixture does not use a preset dictionary"
    );

    let checksum_start = zlib.len() - 4;
    let expected_adler = u32::from_be_bytes(zlib[checksum_start..].try_into().unwrap());
    let mut offset = 2;
    let mut decoded = Vec::new();
    loop {
        assert!(
            offset + 5 <= checksum_start,
            "complete stored DEFLATE block"
        );
        let flags = zlib[offset];
        assert_eq!(flags & 0xf8, 0, "stored block padding bits are zero");
        assert_eq!((flags >> 1) & 0x03, 0, "DEFLATE block is stored");
        let is_final = flags & 1 != 0;
        let length = u16::from_le_bytes([zlib[offset + 1], zlib[offset + 2]]);
        let inverse = u16::from_le_bytes([zlib[offset + 3], zlib[offset + 4]]);
        assert_eq!(length, !inverse, "stored block length complement");
        offset += 5;
        let end = offset + usize::from(length);
        assert!(end <= checksum_start, "stored block data fits");
        decoded.extend_from_slice(&zlib[offset..end]);
        offset = end;
        if is_final {
            break;
        }
    }
    assert_eq!(offset, checksum_start, "one complete zlib stream");
    assert_eq!(adler32(&decoded), expected_adler, "zlib Adler-32");
    decoded
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

fn stored_zip(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
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
