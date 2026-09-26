use std::io::{Cursor, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOCUMENT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

const DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>ZIP preflight baseline</w:t></w:r></w:p></w:body>
</w:document>"#;

const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;

#[test]
fn orphan_local_header_in_gap_before_central_directory_fails_closed() {
    let baseline = minimal_docx();
    assert_docx_accepted(&baseline);

    let mutated = insert_orphan_local_header(&baseline);
    let archive = ZipArchive::new(Cursor::new(mutated.as_slice()))
        .expect("central directory still opens when an unlisted local header is present");
    assert_eq!(
        archive.len(),
        4,
        "orphan header is absent from the central directory"
    );
    drop(archive);

    assert_rejected_as_malformed(&mutated);
}

#[test]
fn unicode_path_extra_cannot_change_the_name_seen_after_preflight() {
    let baseline = minimal_docx();
    assert_docx_accepted(&baseline);

    let mutated = add_unicode_path_extra(&baseline, b"word/document.xml", b"../escape.xml");
    let mut archive = ZipArchive::new(Cursor::new(mutated.as_slice()))
        .expect("Unicode Path extra-field fixture is a readable ZIP archive");
    let names = (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .expect("central-directory entry")
                .name()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        names.iter().any(|name| name == "../escape.xml"),
        "the fixture must make ZipFile::name() disagree with the raw central-directory name"
    );
    drop(archive);

    assert_rejected_as_malformed(&mutated);
}

fn assert_docx_accepted(bytes: &[u8]) {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("stored minimal DOCX passes the OOXML coverage sentinel");
    DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect("stored minimal DOCX passes the DOCX adapter");
}

fn assert_rejected_as_malformed(bytes: &[u8]) {
    let sentinel_error = OoxmlCoverageSentinel::validate_package(bytes)
        .expect_err("ambiguous ZIP structure must fail the OOXML coverage sentinel");
    assert_eq!(
        sentinel_error.code(),
        WorkerFailureCode::SemanticExtractionFailed,
        "{sentinel_error}"
    );

    let adapter_error = DocxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect_err("ambiguous ZIP structure must fail the DOCX adapter");
    assert_eq!(
        adapter_error.code(),
        WorkerFailureCode::SemanticExtractionFailed,
        "{adapter_error}"
    );
}

fn minimal_docx() -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for (name, contents) in [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELATIONSHIPS),
        ("word/document.xml", DOCUMENT),
        ("word/_rels/document.xml.rels", DOCUMENT_RELATIONSHIPS),
    ] {
        writer
            .start_file(name, options)
            .expect("start synthetic DOCX part");
        writer
            .write_all(contents.as_bytes())
            .expect("write synthetic DOCX part");
    }

    writer
        .finish()
        .expect("finish synthetic DOCX ZIP")
        .into_inner()
}

fn insert_orphan_local_header(input: &[u8]) -> Vec<u8> {
    let eocd = find_eocd(input);
    let old_central_start = read_u32(input, eocd + 16) as usize;
    let orphan_name = b"unlisted-orphan.xml";
    let mut orphan = Vec::with_capacity(30 + orphan_name.len());
    push_u32(&mut orphan, 0x0403_4b50);
    push_u16(&mut orphan, 20);
    push_u16(&mut orphan, 0);
    push_u16(&mut orphan, 0);
    push_u16(&mut orphan, 0);
    push_u16(&mut orphan, 0);
    push_u32(&mut orphan, 0);
    push_u32(&mut orphan, 0);
    push_u32(&mut orphan, 0);
    push_u16(&mut orphan, orphan_name.len() as u16);
    push_u16(&mut orphan, 0);
    orphan.extend_from_slice(orphan_name);

    let mut output = Vec::with_capacity(input.len() + orphan.len());
    output.extend_from_slice(&input[..old_central_start]);
    output.extend_from_slice(&orphan);
    output.extend_from_slice(&input[old_central_start..]);

    let new_eocd = eocd + orphan.len();
    write_u32(
        &mut output,
        new_eocd + 16,
        (old_central_start + orphan.len()) as u32,
    );
    output
}

fn add_unicode_path_extra(input: &[u8], raw_name: &[u8], alternate_name: &[u8]) -> Vec<u8> {
    let eocd = find_eocd(input);
    let central_start = read_u32(input, eocd + 16) as usize;
    let central_size = read_u32(input, eocd + 12) as usize;
    let central_end = central_start + central_size;
    let mut offset = central_start;

    while offset < central_end {
        assert_eq!(read_u32(input, offset), CENTRAL_DIRECTORY_HEADER);
        let name_len = usize::from(read_u16(input, offset + 28));
        let extra_len = usize::from(read_u16(input, offset + 30));
        let comment_len = usize::from(read_u16(input, offset + 32));
        let name_start = offset + 46;
        let name_end = name_start + name_len;

        if &input[name_start..name_end] == raw_name {
            let mut field_data = Vec::with_capacity(5 + alternate_name.len());
            field_data.push(1);
            push_u32(&mut field_data, crc32(raw_name));
            field_data.extend_from_slice(alternate_name);

            let mut extra = Vec::with_capacity(4 + field_data.len());
            push_u16(&mut extra, 0x7075);
            push_u16(&mut extra, field_data.len() as u16);
            extra.extend_from_slice(&field_data);

            let insert_at = name_end + extra_len;
            let mut output = Vec::with_capacity(input.len() + extra.len());
            output.extend_from_slice(&input[..insert_at]);
            output.extend_from_slice(&extra);
            output.extend_from_slice(&input[insert_at..]);

            write_u16(&mut output, offset + 30, (extra_len + extra.len()) as u16);
            let new_eocd = eocd + extra.len();
            write_u32(
                &mut output,
                new_eocd + 12,
                (central_size + extra.len()) as u32,
            );
            return output;
        }

        offset = name_end + extra_len + comment_len;
    }

    panic!(
        "target central-directory entry not found: {}",
        String::from_utf8_lossy(raw_name)
    );
}

fn find_eocd(input: &[u8]) -> usize {
    let minimum = input.len().saturating_sub(22 + usize::from(u16::MAX));
    (minimum..=input.len() - 22)
        .rev()
        .find(|offset| read_u32(input, *offset) == END_OF_CENTRAL_DIRECTORY)
        .expect("end-of-central-directory record")
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let low_bit_mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & low_bit_mask);
        }
    }
    !crc
}

fn read_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(input[offset..offset + 2].try_into().expect("u16 field"))
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(input[offset..offset + 4].try_into().expect("u32 field"))
}

fn write_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}
