use std::io::{Cursor, Read, Write};

use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

pub fn docx_fixture(body_text: &str) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{body_text}</w:t></w:r></w:p></w:body></w:document>"#
    );
    write_parts(vec![
        (
            "[Content_Types].xml".to_owned(),
            CONTENT_TYPES.as_bytes().to_vec(),
        ),
        ("_rels/.rels".to_owned(), ROOT_RELS.as_bytes().to_vec()),
        ("word/document.xml".to_owned(), document.into_bytes()),
    ])
}

pub fn mutate_zip_entry_order(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    parts.reverse();
    write_parts(parts)
}

pub fn add_unknown_relationship(input: &[u8], rel_type: &str) -> Vec<u8> {
    let mut parts = read_parts(input);
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdUnknown" Type="{rel_type}" Target="semantic.xml"/></Relationships>"#
    );
    parts.push(("word/_rels/document.xml.rels".to_owned(), rels.into_bytes()));
    parts.push((
        "word/semantic.xml".to_owned(),
        br#"<?xml version="1.0"?><semantic><value>meaning</value></semantic>"#.to_vec(),
    ));
    write_parts(parts)
}

fn read_parts(input: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("fixture ZIP");
    let mut parts = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("fixture entry");
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read fixture entry");
        parts.push((name, data));
    }
    parts
}

fn write_parts(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, data) in parts {
        writer
            .start_file(name, options)
            .expect("start fixture entry");
        writer.write_all(&data).expect("write fixture entry");
    }
    writer
        .finish()
        .expect("finish fixture package")
        .into_inner()
}

pub fn xml_serialization_noise(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/document.xml", |data| {
        String::from_utf8(data)
            .expect("document XML")
            .replace("><", ">\n<")
            .into_bytes()
    })
}

pub fn margin_only_noise(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/document.xml", |data| {
        let text = String::from_utf8(data).expect("document XML");
        text.replace(
            "</w:body>",
            r#"<w:sectPr><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body>"#,
        )
        .into_bytes()
    })
}

pub fn image_ancillary_noise(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/media/image1.png", |mut data| {
        const IEND_LEN: usize = 12;
        assert!(data.len() >= IEND_LEN, "PNG fixture");
        let chunk = hex::decode(
            "0000002574455874436f6d6d656e740073616d6520706978656c20646966666572656e7420656e636f64696e67b825bc2a",
        )
        .expect("valid tEXt chunk");
        let insert_at = data.len() - IEND_LEN;
        data.splice(insert_at..insert_at, chunk);
        data
    })
}

pub fn replace_docx_image_png(input: &[u8], png: &[u8]) -> Vec<u8> {
    map_part(input, "word/media/image1.png", |_| png.to_vec())
}

#[derive(Clone, Copy, Debug)]
pub enum PngDeflateEncoding {
    Stored,
    FixedHuffman,
}

pub fn rgba8_png_fixture(pixel: [u8; 4], encoding: PngDeflateEncoding) -> Vec<u8> {
    let scanline = [0, pixel[0], pixel[1], pixel[2], pixel[3]];
    let compressed = match encoding {
        PngDeflateEncoding::Stored => zlib_stored(&scanline),
        PngDeflateEncoding::FixedHuffman => zlib_fixed_huffman(&scanline),
    };

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    append_png_chunk(&mut png, *b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    append_png_chunk(&mut png, *b"IDAT", &compressed);
    append_png_chunk(&mut png, *b"IEND", &[]);
    png
}

pub fn png_idat_payload(png: &[u8]) -> Vec<u8> {
    parse_png_chunks(png)
        .into_iter()
        .filter(|(kind, _)| kind == b"IDAT")
        .flat_map(|(_, data)| data)
        .collect()
}

pub fn decode_rgba8_png_fixture(png: &[u8]) -> [u8; 4] {
    assert_eq!(png_crc32(b"IEND"), 0xae42_6082, "CRC32 sanity check");
    assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398, "Adler-32 sanity check");

    let chunks = parse_png_chunks(png);
    assert_eq!(chunks.len(), 3, "fixture has exactly IHDR, IDAT, IEND");
    assert_eq!(chunks[0].0, *b"IHDR");
    assert_eq!(chunks[1].0, *b"IDAT");
    assert_eq!(chunks[2], (*b"IEND", Vec::new()));

    let ihdr = &chunks[0].1;
    assert_eq!(ihdr.len(), 13);
    assert_eq!(&ihdr[..8], &[0, 0, 0, 1, 0, 0, 0, 1]);
    assert_eq!(&ihdr[8..], &[8, 6, 0, 0, 0]);

    let zlib = &chunks[1].1;
    assert!(zlib.len() >= 6, "complete zlib stream");
    let cmf = zlib[0];
    let flg = zlib[1];
    assert_eq!(cmf & 0x0f, 8, "DEFLATE compression method");
    assert!(cmf >> 4 <= 7, "valid zlib window size");
    assert_eq!(flg & 0x20, 0, "PNG stream does not use a preset dictionary");
    assert_eq!((u16::from(cmf) << 8 | u16::from(flg)) % 31, 0);

    let checksum_offset = zlib.len() - 4;
    let expected_adler = u32::from_be_bytes(
        zlib[checksum_offset..]
            .try_into()
            .expect("four-byte Adler-32"),
    );
    let pixels = inflate_fixture_deflate(&zlib[2..checksum_offset]);
    assert_eq!(adler32(&pixels), expected_adler, "valid zlib Adler-32");
    assert_eq!(pixels.len(), 5, "one 1x1 RGBA scanline");
    assert_eq!(pixels[0], 0, "PNG filter type None");
    pixels[1..].try_into().expect("RGBA pixel")
}

fn parse_png_chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    assert!(png.starts_with(PNG_SIGNATURE), "PNG signature");

    let mut chunks = Vec::new();
    let mut offset = PNG_SIGNATURE.len();
    while offset < png.len() {
        assert!(
            png.len() - offset >= 12,
            "complete PNG chunk header and CRC"
        );
        let length = u32::from_be_bytes(
            png[offset..offset + 4]
                .try_into()
                .expect("PNG chunk length"),
        ) as usize;
        let data_start = offset + 8;
        let data_end = data_start.checked_add(length).expect("PNG length overflow");
        let chunk_end = data_end.checked_add(4).expect("PNG CRC offset overflow");
        assert!(chunk_end <= png.len(), "complete PNG chunk");

        let kind: [u8; 4] = png[offset + 4..data_start]
            .try_into()
            .expect("four-byte PNG chunk type");
        let data = png[data_start..data_end].to_vec();
        let mut crc_input = Vec::with_capacity(4 + length);
        crc_input.extend_from_slice(&kind);
        crc_input.extend_from_slice(&data);
        let expected_crc = u32::from_be_bytes(
            png[data_end..chunk_end]
                .try_into()
                .expect("four-byte PNG CRC"),
        );
        assert_eq!(png_crc32(&crc_input), expected_crc, "valid {kind:?} CRC");
        chunks.push((kind, data));
        offset = chunk_end;
    }
    assert_eq!(offset, png.len(), "no trailing bytes after PNG chunks");
    chunks
}

fn append_png_chunk(png: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(&kind);
    png.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(&kind);
    crc_input.extend_from_slice(data);
    png.extend_from_slice(&png_crc32(&crc_input).to_be_bytes());
}

fn png_crc32(bytes: &[u8]) -> u32 {
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

fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % MODULUS;
        b = (b + a) % MODULUS;
    }
    (b << 16) | a
}

fn zlib_stored(bytes: &[u8]) -> Vec<u8> {
    let length = u16::try_from(bytes.len()).expect("small PNG fixture");
    let mut zlib = vec![0x78, 0x01, 0x01]; // zlib header, then a final stored DEFLATE block
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(bytes);
    zlib.extend_from_slice(&adler32(bytes).to_be_bytes());
    zlib
}

fn zlib_fixed_huffman(bytes: &[u8]) -> Vec<u8> {
    let mut deflate = DeflateBitWriter::default();
    deflate.write_bits_lsb(1, 1); // final block
    deflate.write_bits_lsb(1, 2); // fixed Huffman block (BTYPE=01)
    for byte in bytes {
        deflate.write_fixed_symbol(u16::from(*byte));
    }
    deflate.write_fixed_symbol(256); // end-of-block

    let mut zlib = vec![0x78, 0x01];
    zlib.extend_from_slice(&deflate.finish());
    zlib.extend_from_slice(&adler32(bytes).to_be_bytes());
    zlib
}

#[derive(Default)]
struct DeflateBitWriter {
    bytes: Vec<u8>,
    current: u8,
    used_bits: u8,
}

impl DeflateBitWriter {
    fn write_bit(&mut self, bit: bool) {
        if bit {
            self.current |= 1 << self.used_bits;
        }
        self.used_bits += 1;
        if self.used_bits == 8 {
            self.bytes.push(self.current);
            self.current = 0;
            self.used_bits = 0;
        }
    }

    fn write_bits_lsb(&mut self, bits: u16, count: u8) {
        for shift in 0..count {
            self.write_bit((bits >> shift) & 1 == 1);
        }
    }

    fn write_fixed_symbol(&mut self, symbol: u16) {
        let (code, count) = match symbol {
            0..=143 => (0x30 + symbol, 8),
            144..=255 => (0x190 + symbol - 144, 9),
            256..=279 => (symbol - 256, 7),
            280..=287 => (0xc0 + symbol - 280, 8),
            _ => panic!("fixture writer only emits literal and end-of-block symbols"),
        };
        for shift in (0..count).rev() {
            self.write_bit((code >> shift) & 1 == 1);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used_bits != 0 {
            self.bytes.push(self.current);
        }
        self.bytes
    }
}

fn inflate_fixture_deflate(bytes: &[u8]) -> Vec<u8> {
    let mut bits = DeflateBitReader::new(bytes);
    assert_eq!(bits.read_bits_lsb(1), 1, "single final DEFLATE block");
    let block_type = bits.read_bits_lsb(2);
    let decoded = match block_type {
        0 => {
            bits.align_to_byte();
            let length = bits.read_u16_le();
            let complement = bits.read_u16_le();
            assert_eq!(length ^ complement, u16::MAX, "stored block LEN/NLEN");
            let payload = bits.read_bytes(usize::from(length)).to_vec();
            bits.finish();
            payload
        }
        1 => {
            let mut output = Vec::new();
            loop {
                match bits.read_fixed_symbol() {
                    symbol @ 0..=255 => output.push(symbol as u8),
                    256 => break,
                    symbol => panic!("unexpected fixture Huffman symbol {symbol}"),
                }
            }
            bits.finish();
            output
        }
        other => panic!("unexpected fixture DEFLATE block type {other}"),
    };
    decoded
}

struct DeflateBitReader<'a> {
    bytes: &'a [u8],
    byte_index: usize,
    used_bits: u8,
}

impl<'a> DeflateBitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_index: 0,
            used_bits: 0,
        }
    }

    fn read_bit(&mut self) -> u8 {
        assert!(self.byte_index < self.bytes.len(), "truncated DEFLATE bits");
        let bit = (self.bytes[self.byte_index] >> self.used_bits) & 1;
        self.used_bits += 1;
        if self.used_bits == 8 {
            self.byte_index += 1;
            self.used_bits = 0;
        }
        bit
    }

    fn read_bits_lsb(&mut self, count: u8) -> u16 {
        let mut value = 0_u16;
        for shift in 0..count {
            value |= u16::from(self.read_bit()) << shift;
        }
        value
    }

    fn align_to_byte(&mut self) {
        while self.used_bits != 0 {
            assert_eq!(self.read_bit(), 0, "zero DEFLATE alignment padding");
        }
    }

    fn read_u16_le(&mut self) -> u16 {
        let bytes = self.read_bytes(2);
        u16::from_le_bytes(bytes.try_into().expect("two-byte DEFLATE length"))
    }

    fn read_bytes(&mut self, count: usize) -> &'a [u8] {
        assert_eq!(self.used_bits, 0, "byte-aligned DEFLATE data");
        let end = self
            .byte_index
            .checked_add(count)
            .expect("DEFLATE length overflow");
        assert!(end <= self.bytes.len(), "complete DEFLATE data");
        let result = &self.bytes[self.byte_index..end];
        self.byte_index = end;
        result
    }

    fn read_fixed_symbol(&mut self) -> u16 {
        let mut code = 0_u16;
        for count in 1..=9 {
            code = (code << 1) | u16::from(self.read_bit());
            let symbol = match count {
                7 if code <= 23 => Some(256 + code),
                8 if (0x30..=0xbf).contains(&code) => Some(code - 0x30),
                8 if (0xc0..=0xc7).contains(&code) => Some(280 + code - 0xc0),
                9 if (0x190..=0x1ff).contains(&code) => Some(144 + code - 0x190),
                _ => None,
            };
            if let Some(symbol) = symbol {
                return symbol;
            }
        }
        panic!("invalid fixed Huffman code")
    }

    fn finish(&mut self) {
        if self.used_bits != 0 {
            let remaining_mask = u8::MAX << self.used_bits;
            assert_eq!(
                self.bytes[self.byte_index] & remaining_mask,
                0,
                "zero DEFLATE final-byte padding"
            );
            self.byte_index += 1;
            self.used_bits = 0;
        }
        assert_eq!(
            self.byte_index,
            self.bytes.len(),
            "no trailing DEFLATE data"
        );
    }
}

pub fn format_only_tracked_change(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/document.xml", |data| {
        let text = String::from_utf8(data).expect("document XML");
        text.replacen(
            "<w:r>",
            r#"<w:r><w:rPr><w:rPrChange w:id="90" w:author="Format Editor"><w:rPr><w:b/></w:rPr></w:rPrChange></w:rPr>"#,
            1,
        )
        .into_bytes()
    })
}

pub fn move_markup_same_final_text(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/document.xml", |data| {
        let text = String::from_utf8(data).expect("document XML");
        let original = "<w:r><w:t>fixture text</w:t></w:r>";
        let moved = r#"<w:moveFrom w:id="91" w:author="Mover"><w:r><w:t>fixture text</w:t></w:r></w:moveFrom><w:moveTo w:id="92" w:author="Mover"><w:r><w:t>fixture text</w:t></w:r></w:moveTo>"#;
        assert!(text.contains(original), "fixture paragraph");
        text.replacen(original, moved, 1).into_bytes()
    })
}

pub fn add_relationship_cycle(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    map_parts_text(&mut parts, "[Content_Types].xml", |text| {
        text.replace(
            "</Types>",
            r#"<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#,
        )
    });
    parts.push((
        "word/header1.xml".into(),
        br#"<?xml version="1.0"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p/></w:hdr>"#.to_vec(),
    ));
    parts.push((
        "word/_rels/document.xml.rels".into(),
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#.to_vec(),
    ));
    parts.push((
        "word/_rels/header1.xml.rels".into(),
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdBack" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="document.xml"/></Relationships>"#.to_vec(),
    ));
    write_parts(parts)
}

pub fn add_traversal_relationship(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    parts.push((
        "word/_rels/document.xml.rels".into(),
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdTraversal" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="../escape.xml"/></Relationships>"#.to_vec(),
    ));
    write_parts(parts)
}

pub fn add_traversal_entry(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    parts.push(("../escape.xml".into(), b"<escape/>".to_vec()));
    write_parts(parts)
}

pub fn add_archive_bomb(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    parts.push(("word/bomb.xml".into(), vec![b'a'; 9 * 1024 * 1024]));
    write_parts_with_method(parts, CompressionMethod::Deflated)
}

pub fn add_duplicate_entry(input: &[u8]) -> Vec<u8> {
    const ALIAS: &[u8] = b"word/documenx.xml";
    const ORIGINAL: &[u8] = b"word/document.xml";
    debug_assert_eq!(ALIAS.len(), ORIGINAL.len());

    let mut parts = read_parts(input);
    let document = parts
        .iter()
        .find(|(name, _)| name == "word/document.xml")
        .map(|(_, data)| data.clone())
        .expect("fixture document part");
    parts.push(("word/documenx.xml".into(), document));

    let mut bytes = write_parts(parts);
    let mut replacements = 0usize;
    let mut offset = 0usize;
    while offset + ALIAS.len() <= bytes.len() {
        if &bytes[offset..offset + ALIAS.len()] == ALIAS {
            bytes[offset..offset + ORIGINAL.len()].copy_from_slice(ORIGINAL);
            replacements += 1;
            offset += ALIAS.len();
        } else {
            offset += 1;
        }
    }
    assert!(
        replacements >= 2,
        "alias must appear in local and central ZIP headers"
    );
    bytes
}

fn map_part<F>(input: &[u8], name: &str, f: F) -> Vec<u8>
where
    F: FnOnce(Vec<u8>) -> Vec<u8>,
{
    let mut parts = read_parts(input);
    let (_, data) = parts
        .iter_mut()
        .find(|(part, _)| part == name)
        .unwrap_or_else(|| panic!("missing part {name}"));
    *data = f(std::mem::take(data));
    write_parts(parts)
}

fn map_parts_text<F>(parts: &mut [(String, Vec<u8>)], name: &str, f: F)
where
    F: FnOnce(String) -> String,
{
    let (_, data) = parts
        .iter_mut()
        .find(|(part, _)| part == name)
        .unwrap_or_else(|| panic!("missing part {name}"));
    let text = String::from_utf8(std::mem::take(data)).expect("XML fixture");
    *data = f(text).into_bytes();
}

fn write_parts_with_method(parts: Vec<(String, Vec<u8>)>, method: CompressionMethod) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .unix_permissions(0o600);
    for (name, data) in parts {
        writer
            .start_file(name, options)
            .expect("start fixture entry");
        writer.write_all(&data).expect("write fixture entry");
    }
    writer
        .finish()
        .expect("finish fixture package")
        .into_inner()
}
