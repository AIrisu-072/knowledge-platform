use std::io::{Cursor, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, SemanticAdapter, SemanticAdapterOutput, SpreadsheetAdapter, WorkerFailure,
    WorkerFailureCode,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#;

const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;

const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#;

const WORKBOOK_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#;

const SHEET: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>baseline</t></is></c></row></sheetData>
</worksheet>"#;

const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
const CENTRAL_HEADER_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
const EOCD_FIXED_SIZE: usize = 22;
const CENTRAL_HEADER_FIXED_SIZE: usize = 46;
const XLSX_TOTAL_UNCOMPRESSED_LIMIT: u32 = 512 * 1024 * 1024;

fn inspect_xlsx(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    SpreadsheetAdapter::XLSX.inspect(bytes, &AdapterProfile::default())
}

fn assert_baseline_accepted() {
    inspect_xlsx(&minimal_xlsx(CompressionMethod::Stored))
        .expect("minimal synthetic XLSX must be accepted as a valid baseline");
}

fn assert_rejected(bytes: &[u8], expected: WorkerFailureCode) {
    let failure = inspect_xlsx(bytes).expect_err("hostile OOXML package must fail closed");
    assert_eq!(failure.code(), expected, "{failure}");
}

fn minimal_xlsx(compression: CompressionMethod) -> Vec<u8> {
    write_parts(
        vec![
            (
                "[Content_Types].xml".to_owned(),
                CONTENT_TYPES.as_bytes().to_vec(),
            ),
            (
                "_rels/.rels".to_owned(),
                ROOT_RELATIONSHIPS.as_bytes().to_vec(),
            ),
            ("xl/workbook.xml".to_owned(), WORKBOOK.as_bytes().to_vec()),
            (
                "xl/_rels/workbook.xml.rels".to_owned(),
                WORKBOOK_RELATIONSHIPS.as_bytes().to_vec(),
            ),
            (
                "xl/worksheets/sheet1.xml".to_owned(),
                SHEET.as_bytes().to_vec(),
            ),
        ],
        compression,
    )
}

fn write_parts(parts: Vec<(String, Vec<u8>)>, compression: CompressionMethod) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(compression)
        .unix_permissions(0o600);
    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("start synthetic XLSX part");
        writer
            .write_all(&contents)
            .expect("write synthetic XLSX part");
    }
    writer
        .finish()
        .expect("finish synthetic XLSX ZIP")
        .into_inner()
}

#[test]
fn valid_minimal_xlsx_is_accepted() {
    assert_baseline_accepted();
}

#[test]
fn archive_path_traversal_fails_as_unsupported_construct() {
    assert_baseline_accepted();
    let mut parts = xlsx_parts();
    parts.push(("../escape.xml".to_owned(), b"<escape/>".to_vec()));
    let bytes = write_parts(parts, CompressionMethod::Stored);
    assert!(zip_has_entry(&bytes, "../escape.xml"));

    assert_rejected(&bytes, WorkerFailureCode::UnsupportedSemanticConstruct);
}

#[test]
fn duplicate_conflicting_package_part_fails_as_unsupported_construct() {
    assert_baseline_accepted();
    let mut parts = xlsx_parts();
    parts.push((
        "xl/workboox.xml".to_owned(),
        b"<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheets/></workbook>"
            .to_vec(),
    ));
    let unique_names = write_parts(parts, CompressionMethod::Stored);
    let bytes = patch_entry_name(&unique_names, b"xl/workboox.xml", b"xl/workbook.xml");
    assert_eq!(count_central_entries(&bytes, b"xl/workbook.xml"), 2);

    assert_rejected(&bytes, WorkerFailureCode::UnsupportedSemanticConstruct);
}

#[test]
fn unknown_potentially_semantic_part_fails_as_unsupported_construct() {
    assert_baseline_accepted();
    let mut parts = xlsx_parts();
    let content_types = String::from_utf8(part(&parts, "[Content_Types].xml"))
        .expect("content types XML")
        .replace(
            "</Types>",
            "<Override PartName=\"/xl/semantic.xml\" ContentType=\"application/vnd.example.unknown-semantic+xml\"/></Types>",
        );
    replace_part(
        &mut parts,
        "[Content_Types].xml",
        content_types.into_bytes(),
    );
    let workbook_rels = String::from_utf8(part(&parts, "xl/_rels/workbook.xml.rels"))
        .expect("workbook relationships XML")
        .replace(
            "</Relationships>",
            "<Relationship Id=\"rIdUnknown\" Type=\"https://example.invalid/relationships/semantic\" Target=\"semantic.xml\"/></Relationships>",
        );
    replace_part(
        &mut parts,
        "xl/_rels/workbook.xml.rels",
        workbook_rels.into_bytes(),
    );
    parts.push((
        "xl/semantic.xml".to_owned(),
        b"<semantic><value>meaning</value></semantic>".to_vec(),
    ));
    let bytes = write_parts(parts, CompressionMethod::Stored);

    assert_rejected(&bytes, WorkerFailureCode::UnsupportedSemanticConstruct);
}

#[test]
fn oversized_declared_decompressed_entry_fails_with_resource_limit() {
    assert_baseline_accepted();
    let mut bytes = minimal_xlsx(CompressionMethod::Deflated);
    let oversized = XLSX_TOTAL_UNCOMPRESSED_LIMIT + 1;
    let central_entry = central_directory_entry(&bytes, b"xl/worksheets/sheet1.xml");
    let local_entry = read_u32(&bytes, central_entry + 42) as usize;
    let compressed_size = read_u32(&bytes, central_entry + 20);
    assert!(compressed_size < oversized);

    write_u32(&mut bytes, central_entry + 24, oversized);
    write_u32(&mut bytes, local_entry + 22, oversized);
    assert_eq!(read_u32(&bytes, central_entry + 24), oversized);
    assert_eq!(read_u32(&bytes, local_entry + 22), oversized);

    assert_rejected(&bytes, WorkerFailureCode::InspectionResourceLimitExceeded);
}

fn xlsx_parts() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "[Content_Types].xml".to_owned(),
            CONTENT_TYPES.as_bytes().to_vec(),
        ),
        (
            "_rels/.rels".to_owned(),
            ROOT_RELATIONSHIPS.as_bytes().to_vec(),
        ),
        ("xl/workbook.xml".to_owned(), WORKBOOK.as_bytes().to_vec()),
        (
            "xl/_rels/workbook.xml.rels".to_owned(),
            WORKBOOK_RELATIONSHIPS.as_bytes().to_vec(),
        ),
        (
            "xl/worksheets/sheet1.xml".to_owned(),
            SHEET.as_bytes().to_vec(),
        ),
    ]
}

fn part(parts: &[(String, Vec<u8>)], name: &str) -> Vec<u8> {
    parts
        .iter()
        .find(|(part_name, _)| part_name == name)
        .unwrap_or_else(|| panic!("missing synthetic XLSX part {name}"))
        .1
        .clone()
}

fn replace_part(parts: &mut [(String, Vec<u8>)], name: &str, contents: Vec<u8>) {
    let (_, part_contents) = parts
        .iter_mut()
        .find(|(part_name, _)| part_name == name)
        .unwrap_or_else(|| panic!("missing synthetic XLSX part {name}"));
    *part_contents = contents;
}

fn zip_has_entry(bytes: &[u8], name: &str) -> bool {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("synthetic XLSX ZIP");
    for index in 0..archive.len() {
        if archive.by_index(index).expect("synthetic ZIP entry").name() == name {
            return true;
        }
    }
    false
}

fn patch_entry_name(input: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    assert_eq!(from.len(), to.len(), "ZIP aliases must have equal lengths");
    let mut output = input.to_vec();
    let mut replacements = 0;
    let mut offset = 0;
    while offset + from.len() <= output.len() {
        if &output[offset..offset + from.len()] == from {
            output[offset..offset + to.len()].copy_from_slice(to);
            replacements += 1;
            offset += from.len();
        } else {
            offset += 1;
        }
    }
    assert!(
        replacements >= 2,
        "ZIP alias must appear in local and central headers"
    );
    output
}

fn count_central_entries(input: &[u8], expected_name: &[u8]) -> usize {
    let (mut cursor, central_end) = central_directory_bounds(input);
    let mut count = 0;
    while cursor < central_end {
        assert_eq!(&input[cursor..cursor + 4], CENTRAL_HEADER_SIGNATURE);
        let name_len = read_u16(input, cursor + 28) as usize;
        let extra_len = read_u16(input, cursor + 30) as usize;
        let comment_len = read_u16(input, cursor + 32) as usize;
        let name_start = cursor + CENTRAL_HEADER_FIXED_SIZE;
        if &input[name_start..name_start + name_len] == expected_name {
            count += 1;
        }
        cursor += CENTRAL_HEADER_FIXED_SIZE + name_len + extra_len + comment_len;
    }
    assert_eq!(cursor, central_end, "central directory size must be exact");
    count
}

fn central_directory_entry(input: &[u8], expected_name: &[u8]) -> usize {
    let (mut cursor, central_end) = central_directory_bounds(input);
    while cursor < central_end {
        assert_eq!(&input[cursor..cursor + 4], CENTRAL_HEADER_SIGNATURE);
        let name_len = read_u16(input, cursor + 28) as usize;
        let extra_len = read_u16(input, cursor + 30) as usize;
        let comment_len = read_u16(input, cursor + 32) as usize;
        let name_start = cursor + CENTRAL_HEADER_FIXED_SIZE;
        if &input[name_start..name_start + name_len] == expected_name {
            return cursor;
        }
        cursor += CENTRAL_HEADER_FIXED_SIZE + name_len + extra_len + comment_len;
    }
    panic!("missing central directory entry {:?}", expected_name);
}

fn central_directory_bounds(input: &[u8]) -> (usize, usize) {
    let eocd = input
        .windows(4)
        .enumerate()
        .rev()
        .find_map(|(offset, signature)| {
            if signature != EOCD_SIGNATURE || offset + EOCD_FIXED_SIZE > input.len() {
                return None;
            }
            let comment_len = read_u16(input, offset + 20) as usize;
            (offset + EOCD_FIXED_SIZE + comment_len == input.len()).then_some(offset)
        })
        .expect("synthetic XLSX end-of-central-directory record");
    let central_start = read_u32(input, eocd + 16) as usize;
    let central_end = central_start + read_u32(input, eocd + 12) as usize;
    (central_start, central_end)
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
