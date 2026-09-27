use std::io::Cursor;

use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PptxAdapter,
};
use zip::ZipArchive;

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CONTENT_TYPES: &[u8] = b"[Content_Types].xml";
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;
const PPTX_PER_ENTRY_LIMIT: u32 = 64 * 1024 * 1024;
const OVERSIZED_DECLARED_SIZE: u32 = PPTX_PER_ENTRY_LIMIT + 1;

#[test]
fn content_types_declared_size_is_bounded_before_format_sniffing() {
    PptxAdapter
        .inspect(BASE, &InspectionProfile::default())
        .expect("the qualified base PPTX must remain a valid baseline");
    assert_declared_entry(BASE, CONTENT_TYPES, None);

    let mutant = with_declared_uncompressed_size(BASE, CONTENT_TYPES, OVERSIZED_DECLARED_SIZE);
    assert_only_declared_size_fields_changed(BASE, &mutant, CONTENT_TYPES);
    assert_declared_entry(&mutant, CONTENT_TYPES, Some(OVERSIZED_DECLARED_SIZE));

    let error = PptxAdapter
        .inspect(&mutant, &InspectionProfile::default())
        .expect_err("an oversized content-types declaration must fail closed before XML parsing");
    assert_eq!(error.code(), ErrorCode::InspectionResourceLimitExceeded);
}

fn with_declared_uncompressed_size(input: &[u8], target: &[u8], size: u32) -> Vec<u8> {
    let mut output = input.to_vec();
    let central_offset = find_central_entry(&output, target);
    let local_offset = read_u32(&output, central_offset + 42) as usize;
    let name_len = usize::from(read_u16(&output, central_offset + 28));
    assert_eq!(
        &output[central_offset + 46..central_offset + 46 + name_len],
        target,
        "central-directory entry has the expected target name"
    );
    assert_eq!(read_u32(&output, local_offset), LOCAL_FILE_HEADER);
    let local_name_len = usize::from(read_u16(&output, local_offset + 26));
    assert_eq!(local_name_len, name_len);
    assert_eq!(
        &output[local_offset + 30..local_offset + 30 + local_name_len],
        target,
        "local and central entry names agree before mutation"
    );
    assert_eq!(
        read_u32(&output, local_offset + 22),
        read_u32(&output, central_offset + 24),
        "local and central uncompressed sizes agree before mutation"
    );

    // Keep the stored bytes, CRC, compressed size, and all other metadata intact.
    write_u32(&mut output, central_offset + 24, size);
    write_u32(&mut output, local_offset + 22, size);
    output
}

fn assert_declared_entry(input: &[u8], target: &[u8], expected_size: Option<u32>) {
    let mut archive = ZipArchive::new(Cursor::new(input))
        .expect("the central directory remains readable before decompression");
    let mut found = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).expect("central-directory entry");
        if entry.name().as_bytes() == target {
            if let Some(expected) = expected_size {
                assert_eq!(entry.size(), u64::from(expected));
                assert!(
                    entry.compressed_size() < u64::from(expected),
                    "the mutant keeps a small compressed payload"
                );
            } else {
                assert!(entry.size() <= u64::from(PPTX_PER_ENTRY_LIMIT));
                assert!(entry.compressed_size() <= u64::from(PPTX_PER_ENTRY_LIMIT));
            }
            found = true;
            break;
        }
    }
    assert!(found, "central directory retains the target entry");
}

fn assert_only_declared_size_fields_changed(original: &[u8], mutant: &[u8], target: &[u8]) {
    assert_eq!(original.len(), mutant.len());
    let central_offset = find_central_entry(original, target);
    let local_offset = read_u32(original, central_offset + 42) as usize;
    for (offset, (original_byte, mutant_byte)) in original.iter().zip(mutant).enumerate() {
        let is_size_field = (local_offset + 22..local_offset + 26).contains(&offset)
            || (central_offset + 24..central_offset + 28).contains(&offset);
        if !is_size_field {
            assert_eq!(
                mutant_byte, original_byte,
                "unexpected mutation at byte {offset}"
            );
        }
    }
}

fn find_central_entry(input: &[u8], target: &[u8]) -> usize {
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
        if &input[offset + 46..offset + 46 + name_len] == target {
            return offset;
        }
        offset += 46 + name_len + extra_len + comment_len;
    }

    panic!(
        "central-directory entry not found: {}",
        String::from_utf8_lossy(target)
    );
}

fn find_eocd(input: &[u8]) -> usize {
    let minimum = input.len().saturating_sub(22 + usize::from(u16::MAX));
    (minimum..=input.len() - 22)
        .rev()
        .find(|offset| read_u32(input, *offset) == END_OF_CENTRAL_DIRECTORY)
        .expect("end-of-central-directory record")
}

fn read_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(input[offset..offset + 2].try_into().expect("u16 field"))
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(input[offset..offset + 4].try_into().expect("u32 field"))
}

fn write_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
