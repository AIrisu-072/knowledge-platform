use std::io::Cursor;

use document_semantic_inspection_core::{
    InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest,
};
use document_semantic_inspection_worker::{
    AdapterProfile, PptxAdapter, SemanticAdapter, WorkerFailure, WorkerFailureCode,
    run_worker_shell,
};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

const PPTX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";
const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");

const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;
const PPTX_PER_ENTRY_LIMIT: u32 = 64 * 1024 * 1024;
const PPTX_OVERSIZED_ENTRY: u32 = PPTX_PER_ENTRY_LIMIT + 1;

#[test]
fn pptx_rejects_declared_entry_size_over_64_mib_before_decompression() {
    assert_base_accepted();

    let oversized =
        with_declared_uncompressed_size(BASE, b"ppt/media/image1.png", PPTX_OVERSIZED_ENTRY);
    assert_declared_size(&oversized, b"ppt/media/image1.png", PPTX_OVERSIZED_ENTRY);

    let failure = PptxAdapter
        .inspect(&oversized, &AdapterProfile::default())
        .expect_err("a declared OOXML part size over 64 MiB must be bounded before read");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::InspectionResourceLimitExceeded,
        "the ZIP fixture intentionally keeps a small payload while declaring 64 MiB + 1; the resource guard must take precedence over decompression/CRC errors"
    );
}

#[test]
fn pptx_shell_bounds_content_types_entry_before_interpreting_its_xml() {
    assert_base_accepted();

    let oversized =
        with_declared_uncompressed_size(BASE, b"[Content_Types].xml", PPTX_OVERSIZED_ENTRY);
    assert_declared_size(&oversized, b"[Content_Types].xml", PPTX_OVERSIZED_ENTRY);

    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: PPTX_MEDIA_TYPE.to_owned(),
        expected_raw_content_hash: Sha256::digest(&oversized).into(),
        expected_size_bytes: oversized.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).expect("valid worker request");
    let mut input = Cursor::new(oversized);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit = run_worker_shell(
        &request_bytes,
        &mut input,
        &mut stdout,
        &mut stderr,
        64 * 1024,
        BASE.len() + 1024,
    );

    assert_eq!(
        exit, 65,
        "resource-limit failures use the worker input-error exit"
    );
    assert!(
        stdout.is_empty(),
        "over-limit input must not produce partial success"
    );
    let failure: WorkerFailure = serde_json::from_slice(&stderr).expect("structured failure");
    assert_eq!(
        failure.code(),
        WorkerFailureCode::InspectionResourceLimitExceeded,
        "the small, intentionally inconsistent ZIP declares [Content_Types].xml as 64 MiB + 1; the package bound must fire before XML interpretation"
    );
}

fn assert_base_accepted() {
    PptxAdapter
        .inspect(BASE, &AdapterProfile::default())
        .expect("PoC-qualified base.pptx remains a valid baseline");
}

fn with_declared_uncompressed_size(input: &[u8], target: &[u8], size: u32) -> Vec<u8> {
    let mut output = input.to_vec();
    let central_offset = find_central_entry(&output, target);
    let local_offset = read_u32(&output, central_offset + 42) as usize;
    let name_len = usize::from(read_u16(&output, central_offset + 28));
    assert_eq!(
        &output[central_offset + 46..central_offset + 46 + name_len],
        target,
        "test fixture targets the expected central-directory part"
    );
    assert_eq!(read_u32(&output, local_offset), LOCAL_FILE_HEADER);
    let local_name_len = usize::from(read_u16(&output, local_offset + 26));
    assert_eq!(local_name_len, name_len);
    assert_eq!(
        &output[local_offset + 30..local_offset + 30 + local_name_len],
        target,
        "central and local names agree before the declared-size mutation"
    );

    // This compact mutant intentionally advertises an oversized expanded part
    // while retaining the original small payload. The guard must reject the
    // declared size before trying to decompress or interpret the part.
    write_u32(&mut output, central_offset + 24, size);
    write_u32(&mut output, local_offset + 22, size);
    output
}

fn assert_declared_size(input: &[u8], target: &[u8], expected: u32) {
    let mut archive = ZipArchive::new(Cursor::new(input))
        .expect("size mutant keeps a readable ZIP central directory");
    let mut found = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .expect("mutant central-directory entry");
        if entry.name().as_bytes() == target {
            assert_eq!(entry.size(), u64::from(expected));
            assert!(
                entry.compressed_size() < u64::from(expected),
                "fixture keeps a small payload while advertising an oversized expansion"
            );
            found = true;
            break;
        }
    }
    assert!(found, "mutant retains the target ZIP entry");
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
