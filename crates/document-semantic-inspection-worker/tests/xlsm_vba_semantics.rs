use std::io::{Cursor, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use document_semantic_inspection_core::FormatId;
use document_semantic_inspection_worker::{
    AdapterProfile, SemanticAdapter, SemanticAdapterOutput, SpreadsheetAdapter, WorkerFailure,
    WorkerFailureCode,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const VBA_WHITESPACE_ONLY: &str =
    "Attribute VB_Name = \"testVBA\"\nPublic Sub test()\nMsgBox  \"Hello from vba!\"\nEnd Sub\n";
const VBA_CASE_ONLY: &str =
    "Attribute VB_Name = \"testVBA\"\nPublic Sub test()\nmsgbox  \"Hello from vba!\"\nEnd Sub\n";
const VBA_LOGIC_CHANGE: &str =
    "Attribute VB_Name = \"testVBA\"\nPublic Sub test()\nMsgBox  \"Hello from vba?\"\nEnd Sub\n";
const VBA_INVALID_SYNTAX: &str =
    "Attribute VB_Name = \"testVBA\"\nPublic Sub test(\nMsgBox  \"Hello from vba!\"\nEnd Sub \n";

static NEXT_EXECUTION_PROBE_ID: AtomicU64 = AtomicU64::new(0);

fn xlsm_seed() -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../experiments/document-semantic-inspection/fixtures/xlsm/calamine-vba.xlsm"),
    )
    .expect("PoC-qualified XLSM seed")
}

fn inspect_xlsm(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    SpreadsheetAdapter::XLSM.inspect(bytes, &AdapterProfile::default())
}

#[test]
fn xlsm_vba_logic_change_changes_the_semantic_fingerprint() {
    assert_eq!(VBA_LOGIC_CHANGE.len(), 82);

    let seed = xlsm_seed();
    let baseline = inspect_xlsm(&seed).expect("qualified XLSM seed");
    let changed = replace_vba_source(&seed, VBA_LOGIC_CHANGE);
    let inspected = inspect_xlsm(&changed).expect("valid VBA logic-change fixture");

    assert_ne!(
        baseline.semantic_fingerprint(),
        inspected.semantic_fingerprint(),
        "a changed string literal in VBA procedure logic must change XLSM identity"
    );
}

#[test]
fn xlsm_vba_whitespace_comment_and_case_noise_preserve_the_semantic_fingerprint() {
    assert_eq!(VBA_WHITESPACE_ONLY.len(), 82);
    assert_eq!(VBA_CASE_ONLY.len(), 82);

    let seed = xlsm_seed();
    let baseline = inspect_xlsm(&seed).expect("qualified XLSM seed");
    let expected = baseline.semantic_fingerprint();

    // Generated from the PoC-qualified `calamine-vba.xlsm` with
    // `tests/support/spreadsheetml.rs::mutate_vba_module`; the PoC adapter
    // confirmed this standalone-comment variant keeps the seed fingerprint.
    // The binary fixture is 31,134 bytes and avoids changing the seed's fixed
    // 96-byte compressed VBA source chunk in this dependency-free test.
    let comment_variant = include_bytes!("fixtures/xlsm_vba_comment_noise.xlsm");
    let comment_inspected =
        inspect_xlsm(comment_variant).expect("PoC-qualified standalone-comment XLSM fixture");
    assert_eq!(
        comment_inspected.semantic_fingerprint(),
        expected,
        "standalone VBA comments must not alter XLSM identity"
    );

    for (label, source) in [
        ("whitespace and line-ending only", VBA_WHITESPACE_ONLY),
        ("meaning-neutral keyword case", VBA_CASE_ONLY),
    ] {
        let variant = replace_vba_source(&seed, source);
        let inspected = inspect_xlsm(&variant).unwrap_or_else(|error| {
            panic!("{label} VBA variant should remain inspectable: {error}")
        });
        assert_eq!(
            inspected.semantic_fingerprint(),
            expected,
            "{label} VBA changes must not alter XLSM identity"
        );
    }
}

#[test]
fn invalid_vba_syntax_fails_closed() {
    assert_eq!(VBA_INVALID_SYNTAX.len(), 82);

    let invalid = replace_vba_source(&xlsm_seed(), VBA_INVALID_SYNTAX);
    let error = inspect_xlsm(&invalid).expect_err("incomplete procedure syntax must fail closed");

    assert_eq!(error.code(), WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn incomplete_vba_project_fails_closed() {
    let seed = xlsm_seed();
    let incomplete = replace_zip_entry(&seed, "xl/vbaProject.bin", Some(&[]));
    let error = inspect_xlsm(&incomplete).expect_err("truncated VBA project must fail closed");

    assert_eq!(error.code(), WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn macro_enabled_workbook_must_bind_to_its_vba_project_part() {
    let seed = xlsm_seed();
    assert_eq!(SpreadsheetAdapter::XLSM.format(), FormatId::Xlsm);
    inspect_xlsm(&seed).expect("qualified macro-enabled workbook with VBA project");

    let without_vba_part = replace_zip_entry(&seed, "xl/vbaProject.bin", None);
    let error = inspect_xlsm(&without_vba_part)
        .expect_err("macro-enabled workbook with a missing VBA project must fail closed");

    assert_eq!(error.code(), WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn static_xlsm_inspection_never_executes_vba() {
    let (owner_dir, owner_name) = create_owned_probe_directory();
    let _cleanup = RemoveOwnedDirectoryOnDrop(owner_dir.clone());
    let marker_path = owner_dir.join("m");
    let literal_backslash_marker_path = owner_dir
        .parent()
        .expect("execution-probe parent")
        .join(format!("{owner_name}\\m"));
    assert!(
        !marker_path.exists(),
        "unique execution probe must start absent"
    );
    assert!(
        !literal_backslash_marker_path.exists(),
        "literal-backslash execution probe must start absent"
    );
    let _literal_marker_cleanup = RemoveAbsentPathOnDrop(literal_backslash_marker_path.clone());

    let source = side_effecting_vba_source(&format!(r"{owner_name}\m"));
    let input = replace_vba_source(&xlsm_seed(), &source);

    inspect_xlsm(&input).expect("static inspection of valid side-effecting VBA source");

    assert!(
        !marker_path.exists(),
        "VBA source that would create a directory must be inspected as data only"
    );
    assert!(
        !literal_backslash_marker_path.exists(),
        "VBA source must not create a POSIX filename containing a literal backslash"
    );
}

fn side_effecting_vba_source(marker: &str) -> String {
    let mut source =
        format!("Attribute VB_Name = \"testVBA\"\nSub Auto_Open()\nMkDir \"{marker}\"\nEnd Sub\n");
    assert!(
        source.len() <= 80,
        "execution probe must fit the qualified chunk"
    );
    let comment_padding = 82 - source.len();
    assert!(comment_padding >= 2);
    source.push('\'');
    source.push_str(&"x".repeat(comment_padding - 2));
    source.push('\n');
    assert_eq!(source.len(), 82);
    source
}

fn create_owned_probe_directory() -> (std::path::PathBuf, String) {
    let working_directory = std::env::current_dir().expect("test working directory");

    for _ in 0..100 {
        let probe_id = NEXT_EXECUTION_PROBE_ID.fetch_add(1, Ordering::Relaxed);
        let name = format!("v{:x}_{probe_id:x}", std::process::id());
        let path = working_directory.join(&name);

        match std::fs::create_dir(&path) {
            Ok(()) => return (path, name),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create owned execution-probe directory: {error}"),
        }
    }

    panic!("could not allocate a unique execution-probe directory")
}

fn replace_vba_source(input: &[u8], source: &str) -> Vec<u8> {
    let mut vba_project = read_zip_entry(input, "xl/vbaProject.bin");
    let source_chunk = find_test_vba_source_chunk(&vba_project);
    let replacement = compress_vba_literals(source.as_bytes());

    assert_eq!(
        source_chunk.len(),
        replacement.len(),
        "replacement source must preserve the PoC-qualified CFB stream boundary"
    );
    vba_project[source_chunk].copy_from_slice(&replacement);

    replace_zip_entry(input, "xl/vbaProject.bin", Some(&vba_project))
}

fn find_test_vba_source_chunk(vba_project: &[u8]) -> std::ops::Range<usize> {
    let module_name = b"Attribute VB_Name = \"testVBA\"";
    let source_marker = b"Hello from vba!";
    let mut found = None;

    for start in 0..vba_project.len().saturating_sub(3) {
        let Some((decoded, chunk_len)) = decode_vba_source_chunk(&vba_project[start..]) else {
            continue;
        };
        if decoded
            .windows(module_name.len())
            .any(|window| window == module_name)
            && decoded
                .windows(source_marker.len())
                .any(|window| window == source_marker)
        {
            assert!(
                found.is_none(),
                "fixture must contain one testVBA source chunk"
            );
            found = Some(start..start + chunk_len);
        }
    }

    found.expect("testVBA source chunk in PoC-qualified XLSM seed")
}

fn decode_vba_source_chunk(input: &[u8]) -> Option<(Vec<u8>, usize)> {
    if input.first().copied()? != 0x01 || input.len() < 3 {
        return None;
    }

    let header = u16::from_le_bytes([input[1], input[2]]);
    if header & 0x8000 == 0 || header & 0x7000 != 0x3000 {
        return None;
    }

    let compressed_chunk_size = usize::from(header & 0x0fff) + 3;
    let container_size = compressed_chunk_size.checked_add(1)?;
    if container_size > input.len() {
        return None;
    }

    let mut cursor = 3;
    let mut output = Vec::new();
    while cursor < container_size {
        let flags = input[cursor];
        cursor += 1;

        for bit in 0..8 {
            if cursor >= container_size {
                break;
            }

            if flags & (1 << bit) == 0 {
                output.push(input[cursor]);
                cursor += 1;
            } else {
                if cursor + 2 > container_size {
                    return None;
                }

                let token = u16::from_le_bytes([input[cursor], input[cursor + 1]]);
                cursor += 2;
                let chunk_output_len = output.len();
                let offset_bits =
                    (usize::BITS - chunk_output_len.saturating_sub(1).leading_zeros()).max(4);
                let length_mask = u16::MAX >> offset_bits;
                let copy_len = usize::from(token & length_mask) + 3;
                let copy_offset = usize::from((token & !length_mask) >> (16 - offset_bits)) + 1;
                if copy_offset > chunk_output_len || output.len() + copy_len > 4096 {
                    return None;
                }

                for _ in 0..copy_len {
                    let source = output.len() - copy_offset;
                    output.push(output[source]);
                }
            }
        }
    }

    Some((output, container_size))
}

fn compress_vba_literals(source: &[u8]) -> Vec<u8> {
    assert!(!source.is_empty(), "VBA source fixture must not be empty");

    let mut chunk = Vec::with_capacity(source.len() + source.len().div_ceil(8));
    for group in source.chunks(8) {
        chunk.push(0); // all tokens in this group are literals
        chunk.extend_from_slice(group);
    }
    assert!(
        chunk.len() <= 4096,
        "VBA source must fit one compressed chunk"
    );

    let header = 0xb000u16 | u16::try_from(chunk.len() - 1).expect("compressed chunk size");
    let mut compressed = Vec::with_capacity(3 + chunk.len());
    compressed.push(0x01); // MS-OVBA CompressedContainer signature
    compressed.extend_from_slice(&header.to_le_bytes());
    compressed.extend_from_slice(&chunk);
    compressed
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
                .expect("write XLSM file entry");
            output
                .write_all(&bytes)
                .expect("write XLSM package content");
        }
    }

    output.finish().expect("finish XLSM package").into_inner()
}

struct RemoveOwnedDirectoryOnDrop(std::path::PathBuf);

impl Drop for RemoveOwnedDirectoryOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct RemoveAbsentPathOnDrop(std::path::PathBuf);

impl Drop for RemoveAbsentPathOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
