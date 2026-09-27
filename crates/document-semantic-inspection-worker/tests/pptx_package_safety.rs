use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, PptxAdapter, SemanticAdapter, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");

const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;

#[test]
fn qualified_pptx_baseline_is_accepted_before_package_mutations() {
    assert_base_accepted();
}

#[test]
fn orphan_local_header_before_central_directory_fails_closed() {
    assert_base_accepted();

    let mutated = insert_orphan_local_header(BASE);
    let mut archive = ZipArchive::new(Cursor::new(mutated.as_slice()))
        .expect("central directory remains readable with an unindexed local record");
    let baseline_entry_count = ZipArchive::new(Cursor::new(BASE))
        .expect("qualified base is a ZIP archive")
        .len();
    assert_eq!(archive.len(), baseline_entry_count);
    assert!(
        (0..archive.len()).all(|index| {
            archive
                .by_index(index)
                .is_ok_and(|entry| entry.name() != "unlisted-orphan.xml")
        }),
        "the inserted local record is not indexed in the central directory"
    );

    assert_failure(&mutated, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn central_and_local_raw_filenames_must_match() {
    assert_base_accepted();

    let central_name = b"ppt/slides/slide1.xml";
    let local_name = b"ppt/slides/slideX.xml";
    let mutated = replace_local_filename(BASE, central_name, local_name);
    let entry = find_central_entry(&mutated, central_name);
    assert_eq!(central_name.len(), local_name.len());
    assert_eq!(
        &mutated[entry.local_offset + 30..entry.local_offset + 30 + local_name.len()],
        local_name
    );
    assert_ne!(central_name, local_name);

    assert_failure(&mutated, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn unicode_path_extra_alias_for_semantic_part_fails_closed() {
    assert_base_accepted();

    let raw_name = b"ppt/theme/theme1.xml";
    let alias_name = b"ppt/slides/slide1.xml";
    let mutated = add_unicode_path_alias(BASE, raw_name, alias_name);
    let mut archive = ZipArchive::new(Cursor::new(mutated.as_slice()))
        .expect("Unicode Path extra field leaves a readable ZIP archive");
    let visible_names = (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .expect("central-directory entry")
                .name()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        visible_names
            .iter()
            .any(|name| name == "ppt/slides/slide1.xml")
    );
    assert!(
        !visible_names
            .iter()
            .any(|name| name == "ppt/theme/theme1.xml"),
        "the valid Unicode Path field makes ZIP consumers disagree on this part name"
    );

    assert_failure(&mutated, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn generic_xml_custom_xml_part_fails_closed_as_unknown_semantics() {
    assert_base_accepted();

    let content_types = read_part(BASE, "[Content_Types].xml");
    assert!(
        String::from_utf8_lossy(&content_types)
            .contains(r#"<Default Extension="xml" ContentType="application/xml"/>"#),
        "the base fixture assigns generic application/xml to otherwise undeclared XML parts"
    );

    let mutated = append_part(
        BASE,
        "customXml/item1.xml",
        br#"<?xml version="1.0" encoding="UTF-8"?><businessData xmlns="urn:example:business-data"><value>Presentation approval condition</value></businessData>"#,
    );
    assert_archive_contains(&mutated, "customXml/item1.xml");

    assert_failure(&mutated, WorkerFailureCode::UnsupportedSemanticConstruct);
}

fn assert_base_accepted() {
    PptxAdapter
        .inspect(BASE, &AdapterProfile::default())
        .expect("qualified base.pptx remains a valid PPTX semantic baseline");
}

fn assert_failure(bytes: &[u8], expected: WorkerFailureCode) {
    let failure = PptxAdapter
        .inspect(bytes, &AdapterProfile::default())
        .expect_err("ambiguous or unknown PPTX semantics must fail closed");
    assert_eq!(failure.code(), expected, "{failure}");
}

fn insert_orphan_local_header(input: &[u8]) -> Vec<u8> {
    let eocd = find_eocd(input);
    let old_central_start = read_u32(input, eocd + 16) as usize;
    let orphan_name = b"unlisted-orphan.xml";
    let mut orphan = Vec::with_capacity(30 + orphan_name.len());
    push_u32(&mut orphan, LOCAL_FILE_HEADER);
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
        u32::try_from(old_central_start + orphan.len()).expect("small PPTX central offset"),
    );
    output
}

fn replace_local_filename(input: &[u8], central_name: &[u8], replacement: &[u8]) -> Vec<u8> {
    assert_eq!(central_name.len(), replacement.len());
    let entry = find_central_entry(input, central_name);
    let local_name_len = usize::from(read_u16(input, entry.local_offset + 26));
    assert_eq!(local_name_len, central_name.len());
    assert_eq!(read_u32(input, entry.local_offset), LOCAL_FILE_HEADER);
    assert_eq!(
        &input[entry.local_offset + 30..entry.local_offset + 30 + local_name_len],
        central_name
    );

    let mut output = input.to_vec();
    output[entry.local_offset + 30..entry.local_offset + 30 + replacement.len()]
        .copy_from_slice(replacement);
    output
}

fn add_unicode_path_alias(input: &[u8], raw_name: &[u8], alias_name: &[u8]) -> Vec<u8> {
    let entry = find_central_entry(input, raw_name);
    let raw_name_len = usize::from(read_u16(input, entry.central_offset + 28));
    let extra_len = usize::from(read_u16(input, entry.central_offset + 30));
    assert_eq!(raw_name_len, raw_name.len());
    assert_eq!(
        &input[entry.central_offset + 46..entry.central_offset + 46 + raw_name_len],
        raw_name
    );

    // Info-ZIP Unicode Path (0x7075): version, CRC32 of the raw name, UTF-8 alias.
    let mut field_data = Vec::with_capacity(5 + alias_name.len());
    field_data.push(1);
    push_u32(&mut field_data, crc32(raw_name));
    field_data.extend_from_slice(alias_name);
    let mut extra_field = Vec::with_capacity(4 + field_data.len());
    push_u16(&mut extra_field, 0x7075);
    push_u16(&mut extra_field, field_data.len() as u16);
    extra_field.extend_from_slice(&field_data);

    let extra_start = entry.central_offset + 46 + raw_name_len;
    let insert_at = extra_start + extra_len;
    let mut output = Vec::with_capacity(input.len() + extra_field.len());
    output.extend_from_slice(&input[..insert_at]);
    output.extend_from_slice(&extra_field);
    output.extend_from_slice(&input[insert_at..]);

    write_u16(
        &mut output,
        entry.central_offset + 30,
        u16::try_from(extra_len + extra_field.len()).expect("small ZIP extra field"),
    );
    let eocd = find_eocd(&output);
    let central_size = read_u32(input, find_eocd(input) + 12);
    write_u32(
        &mut output,
        eocd + 12,
        central_size + u32::try_from(extra_field.len()).expect("small ZIP extra field"),
    );
    output
}

fn append_part(archive: &[u8], part_name: &str, contents: &[u8]) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("qualified base is a ZIP archive");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        assert_ne!(name, part_name, "mutant part must not already exist");
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .expect("base part CRC and payload");
        writer
            .start_file(name, options)
            .expect("copy qualified PPTX part into mutant ZIP");
        writer.write_all(&bytes).expect("write copied PPTX part");
    }

    writer
        .start_file(part_name, options)
        .expect("start unknown custom XML part");
    writer
        .write_all(contents)
        .expect("write unknown custom XML part");
    writer
        .finish()
        .expect("finish mutant PPTX ZIP")
        .into_inner()
}

fn read_part(archive: &[u8], part_name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("qualified base is a ZIP archive");
    let mut part = zip.by_name(part_name).expect("required base part");
    let mut contents = Vec::new();
    part.read_to_end(&mut contents).expect("read base part");
    contents
}

fn assert_archive_contains(archive: &[u8], expected_name: &str) {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("mutant remains a readable ZIP");
    assert!((0..zip.len()).any(|index| {
        zip.by_index(index)
            .is_ok_and(|entry| entry.name() == expected_name)
    }));
}

struct CentralEntry {
    central_offset: usize,
    local_offset: usize,
}

fn find_central_entry(input: &[u8], target_name: &[u8]) -> CentralEntry {
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
        if &input[name_start..name_end] == target_name {
            return CentralEntry {
                central_offset: offset,
                local_offset: read_u32(input, offset + 42) as usize,
            };
        }
        offset = name_end + extra_len + comment_len;
    }

    panic!(
        "central-directory entry not found: {}",
        String::from_utf8_lossy(target_name)
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
