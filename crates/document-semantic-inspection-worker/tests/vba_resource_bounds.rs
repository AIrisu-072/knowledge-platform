use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, SemanticAdapter, SpreadsheetAdapter, WorkerFailureCode,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const VBA_SOURCE_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const OVBA_CHUNK_OUTPUT_BYTES: usize = 4096;
const MODULE_NAME: &str = "testVBA";
const SOURCE_SENTINEL: &str = "DSI_OVBA_BOUNDS_SECRET_SENTINEL";
const DIR_SENTINEL: &str = "DSI_OVBA_DIR_TRAILING_SECRET_SENTINEL";

fn xlsm_seed() -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../experiments/document-semantic-inspection/fixtures/xlsm/calamine-vba.xlsm"),
    )
    .expect("PoC-qualified XLSM seed")
}

fn inspect_xlsm(
    bytes: &[u8],
) -> Result<
    document_semantic_inspection_worker::SemanticAdapterOutput,
    document_semantic_inspection_worker::WorkerFailure,
> {
    SpreadsheetAdapter::XLSM.inspect(bytes, &AdapterProfile::default())
}

#[test]
fn static_xlsm_rejects_vba_source_over_16_mib_without_echoing_source() {
    let seed = xlsm_seed();
    inspect_xlsm(&seed).expect("qualified XLSM seed is a valid baseline");

    let oversized = replace_module_source_with_repeating_chunks(&seed);
    assert_ovba_module_source_is_bounded(&oversized);
    let error =
        inspect_xlsm(&oversized).expect_err("decompressed VBA source over 16 MiB must fail closed");

    assert_eq!(
        error.code(),
        WorkerFailureCode::InspectionResourceLimitExceeded,
        "an oversized static VBA source must be rejected at the resource boundary"
    );
    assert!(
        !error.message().contains(SOURCE_SENTINEL),
        "resource failures must not expose VBA source text"
    );
}

fn assert_ovba_module_source_is_bounded(input: &[u8]) {
    let vba_project = read_zip_entry(input, "xl/vbaProject.bin");
    let project = ovba::open_project(vba_project).expect("oversized fixture has valid .dir data");
    let result = project.module_source_raw(MODULE_NAME);
    let error = match result {
        Err(error) => error,
        Ok(source) => panic!(
            "bounded ovba source extraction returned {} bytes above the {} byte limit",
            source.len(),
            VBA_SOURCE_LIMIT_BYTES
        ),
    };

    assert!(
        format!("{error:?}")
            .to_ascii_lowercase()
            .contains("resource"),
        "oversized VBA source must return a resource-limit error"
    );
    assert!(
        !format!("{error:?}").contains(SOURCE_SENTINEL),
        "ovba resource errors must not expose VBA source text"
    );
}

#[test]
fn static_xlsm_rejects_undecoded_dir_bytes_after_terminator_without_echoing_them() {
    let seed = xlsm_seed();
    inspect_xlsm(&seed).expect("qualified XLSM seed is a valid baseline");

    let trailing = append_undecoded_dir_bytes(&seed, DIR_SENTINEL.as_bytes());
    let result = std::panic::catch_unwind(|| inspect_xlsm(&trailing));
    assert!(
        result.is_ok(),
        "trailing /VBA/dir bytes must return a controlled failure, not panic"
    );
    let error = result
        .expect("no parser panic")
        .expect_err("undecoded bytes after the .dir terminator must fail closed in release");

    assert_eq!(
        error.code(),
        WorkerFailureCode::SemanticExtractionFailed,
        "a structurally incomplete VBA project must fail closed"
    );
    assert!(
        !error.message().contains(DIR_SENTINEL),
        "VBA parser failures must not expose undecoded stream bytes"
    );
}

fn replace_module_source_with_repeating_chunks(input: &[u8]) -> Vec<u8> {
    let vba_project = read_zip_entry(input, "xl/vbaProject.bin");
    let project = ovba::open_project(vba_project.clone()).expect("parse valid VBA seed");
    let module = project
        .modules
        .iter()
        .find(|module| module.name == MODULE_NAME)
        .expect("qualified seed module");
    let stream_path = format!("/VBA/{}", module.stream_name);
    let text_offset = module.text_offset;
    drop(project);

    let mut compound = cfb::CompoundFile::open(Cursor::new(vba_project)).expect("open VBA CFB");
    let mut stream_bytes = Vec::new();
    compound
        .open_stream(&stream_path)
        .expect("open VBA module stream")
        .read_to_end(&mut stream_bytes)
        .expect("read VBA module stream");
    assert!(
        text_offset <= stream_bytes.len(),
        "module source offset is valid"
    );

    let mut replacement = stream_bytes[..text_offset].to_vec();
    replacement.extend_from_slice(&oversized_comment_container());
    {
        let mut stream = compound
            .create_stream(&stream_path)
            .expect("replace VBA module stream");
        stream
            .write_all(&replacement)
            .expect("write oversized VBA module stream");
    }
    compound.flush().expect("flush VBA CFB");
    replace_zip_entry(
        input,
        "xl/vbaProject.bin",
        Some(&compound.into_inner().into_inner()),
    )
}

fn oversized_comment_container() -> Vec<u8> {
    let module_header = format!("Attribute VB_Name = \"{MODULE_NAME}\"\r\n' {SOURCE_SENTINEL}\r\n");
    let mut container = vec![0x01]; // MS-OVBA CompressedContainer signature.
    container.extend_from_slice(&compressed_literal_chunk(module_header.as_bytes()));

    // A one-byte literal followed by a copy token expands to one 4096-byte
    // chunk. More than 4096 chunks exceed the frozen 16 MiB source bound while
    // keeping the synthetic XLSM small and deterministic.
    let repeat_chunk = compressed_repeated_byte_chunk(b'\'');
    let repeat_count = VBA_SOURCE_LIMIT_BYTES / OVBA_CHUNK_OUTPUT_BYTES + 1;
    for _ in 0..repeat_count {
        container.extend_from_slice(&repeat_chunk);
    }

    assert!(
        repeat_count * OVBA_CHUNK_OUTPUT_BYTES > VBA_SOURCE_LIMIT_BYTES,
        "fixture expands beyond the 16 MiB VBA source bound"
    );
    container
}

fn append_undecoded_dir_bytes(input: &[u8], trailing_bytes: &[u8]) -> Vec<u8> {
    let vba_project = read_zip_entry(input, "xl/vbaProject.bin");
    let mut compound = cfb::CompoundFile::open(Cursor::new(vba_project)).expect("open VBA CFB");
    let mut dir_stream = Vec::new();
    compound
        .open_stream(dir_stream_path())
        .expect("open VBA dir stream")
        .read_to_end(&mut dir_stream)
        .expect("read VBA dir stream");

    // The existing compressed container already has its signature. Appending
    // one valid compressed chunk makes these bytes part of the decoded stream
    // after the existing six-byte [MS-OVBA] project terminator.
    dir_stream.extend_from_slice(&compressed_literal_chunk(trailing_bytes));
    {
        let mut stream = compound
            .create_stream(dir_stream_path())
            .expect("replace VBA dir stream");
        stream.write_all(&dir_stream).expect("write VBA dir stream");
    }
    compound.flush().expect("flush VBA CFB");
    replace_zip_entry(
        input,
        "xl/vbaProject.bin",
        Some(&compound.into_inner().into_inner()),
    )
}

#[cfg(target_family = "windows")]
fn dir_stream_path() -> &'static str {
    "/VBA\\dir"
}

#[cfg(not(target_family = "windows"))]
fn dir_stream_path() -> &'static str {
    "/VBA/dir"
}

fn compressed_literal_chunk(decoded: &[u8]) -> Vec<u8> {
    assert!(
        !decoded.is_empty(),
        "compressed fixture chunk must not be empty"
    );

    let mut payload = Vec::with_capacity(decoded.len() + decoded.len().div_ceil(8));
    for group in decoded.chunks(8) {
        payload.push(0); // Eight literal-token flags.
        payload.extend_from_slice(group);
    }
    assert!(
        payload.len() <= OVBA_CHUNK_OUTPUT_BYTES,
        "compressed literal fixture chunk must fit the MS-OVBA chunk boundary"
    );

    let header = 0xB000u16 | u16::try_from(payload.len() - 1).expect("compressed chunk size");
    let mut chunk = Vec::with_capacity(payload.len() + 2);
    chunk.extend_from_slice(&header.to_le_bytes());
    chunk.extend_from_slice(&payload);
    chunk
}

fn compressed_repeated_byte_chunk(byte: u8) -> [u8; 6] {
    // At output offset one, this copy token repeats the first byte 4095 times.
    // 0x0FFC encodes a 4092-byte length field plus the format's three-byte bias.
    [0x03, 0xB0, 0x02, byte, 0xFC, 0x0F]
}

fn read_zip_entry(input: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("qualified XLSM ZIP package");
    let mut entry = archive.by_name(name).expect("required XLSM package entry");
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .expect("read XLSM package entry");
    bytes
}

fn replace_zip_entry(input: &[u8], name: &str, replacement: Option<&[u8]>) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("qualified XLSM ZIP package");
    let mut entries = Vec::with_capacity(archive.len());
    let mut replaced = false;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("XLSM package entry");
        let entry_name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .expect("read XLSM package entry");

        if entry_name == name {
            assert!(!replaced, "XLSM package entry must be unique");
            replaced = true;
            if let Some(replacement) = replacement {
                entries.push((entry_name, replacement.to_vec()));
            }
        } else {
            entries.push((entry_name, bytes));
        }
    }
    assert!(replaced, "XLSM package entry to replace must exist");

    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (entry_name, bytes) in entries {
        if entry_name.ends_with('/') {
            output
                .add_directory(entry_name, options)
                .expect("write XLSM directory entry");
        } else {
            output
                .start_file(entry_name, options)
                .expect("write XLSM package entry");
            output
                .write_all(&bytes)
                .expect("write XLSM package content");
        }
    }

    output.finish().expect("finish XLSM package").into_inner()
}
