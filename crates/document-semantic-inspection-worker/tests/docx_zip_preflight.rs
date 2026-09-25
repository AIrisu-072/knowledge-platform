use document_semantic_inspection_worker::{OoxmlCoverageSentinel, WorkerFailureCode};

const CONTENT_TYPES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const ROOT_RELATIONSHIPS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOCUMENT_RELATIONSHIPS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

const DOCUMENT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>minimal fixture</w:t></w:r></w:p></w:body>
</w:document>"#;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const EOCD_SIZE: usize = 22;
const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4b50;

#[test]
fn eocd_declared_entry_count_over_limit_is_resource_failure_before_archive_parse() {
    let mut bytes = minimal_docx();
    let eocd = find_last_signature(&bytes, EOCD_SIGNATURE);
    let actual_entries = read_u16(&bytes, eocd + 10);
    assert_eq!(
        actual_entries, 4,
        "valid base fixture has four package parts"
    );
    assert_eq!(read_u16(&bytes, eocd + 8), actual_entries);
    assert_eq!(
        count_central_entries(&bytes, eocd),
        usize::from(actual_entries)
    );

    // The central directory remains intact, while EOCD claims 257 entries.
    // The declared count must hit the resource cap before ZipArchive parses it.
    write_u16(&mut bytes, eocd + 8, 257);
    write_u16(&mut bytes, eocd + 10, 257);
    assert_eq!(read_u16(&bytes, eocd + 10), 257);
    assert_eq!(count_central_entries(&bytes, eocd), 4);

    assert_sentinel_failure(&bytes, WorkerFailureCode::InspectionResourceLimitExceeded);
}

#[test]
fn fake_eocd_in_archive_comment_is_rejected() {
    let mut bytes = minimal_docx();
    let real_eocd = bytes.len() - EOCD_SIZE;
    assert_eq!(read_u32(&bytes, real_eocd), EOCD_SIGNATURE);
    let mut fake_eocd = bytes[real_eocd..].to_vec();
    write_u16(&mut fake_eocd, 20, 0);
    write_u16(&mut bytes, real_eocd + 20, EOCD_SIZE as u16);
    bytes.extend_from_slice(&fake_eocd);
    assert_eq!(read_u16(&bytes, real_eocd + 20), EOCD_SIZE as u16);
    let fake_eocd = bytes.len() - EOCD_SIZE;
    assert_eq!(read_u32(&bytes, fake_eocd), EOCD_SIGNATURE);
    assert_eq!(read_u16(&bytes, fake_eocd + 20), 0);
    assert_eq!(
        read_u16(&bytes, fake_eocd + 8),
        read_u16(&bytes, real_eocd + 8)
    );
    assert_eq!(
        read_u16(&bytes, fake_eocd + 10),
        read_u16(&bytes, real_eocd + 10)
    );
    assert_eq!(
        read_u32(&bytes, fake_eocd + 12),
        read_u32(&bytes, real_eocd + 12)
    );
    assert_eq!(
        read_u32(&bytes, fake_eocd + 16),
        read_u32(&bytes, real_eocd + 16)
    );
    assert_eq!(count_central_entries(&bytes, fake_eocd), 4);

    assert_sentinel_failure(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn central_directory_tail_length_mismatch_is_rejected() {
    let mut bytes = minimal_docx();
    let eocd = find_last_signature(&bytes, EOCD_SIGNATURE);
    let central_size = read_u32(&bytes, eocd + 12);
    assert!(central_size > 0);
    assert_eq!(count_central_entries(&bytes, eocd), 4);

    write_u32(&mut bytes, eocd + 12, central_size - 1);
    assert_eq!(read_u32(&bytes, eocd + 12), central_size - 1);
    assert_sentinel_failure(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn central_and_local_entry_names_must_match() {
    let entries = minimal_docx_entries()
        .into_iter()
        .map(|entry| {
            if entry.central_name.as_slice() == b"word/document.xml" {
                StoredEntry::with_local_name(b"word/document.xml", b"word/documenx.xml", DOCUMENT)
            } else {
                entry
            }
        })
        .collect::<Vec<_>>();
    let bytes = stored_zip(&entries);
    assert_ne!(b"word/document.xml", b"word/documenx.xml");
    assert_eq!(b"word/document.xml".len(), b"word/documenx.xml".len());
    assert!(contains_bytes(&bytes, b"word/document.xml"));
    assert!(contains_bytes(&bytes, b"word/documenx.xml"));

    assert_sentinel_failure(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn file_and_directory_with_the_same_part_name_are_rejected() {
    let mut entries = minimal_docx_entries();
    entries.push(StoredEntry::new(b"word/document.xml/", b""));
    let bytes = stored_zip(&entries);
    let eocd = find_last_signature(&bytes, EOCD_SIGNATURE);
    assert_eq!(count_central_entries(&bytes, eocd), 5);
    assert!(contains_bytes(&bytes, b"word/document.xml/"));
    assert!(contains_bytes(&bytes, b"word/document.xml"));

    assert_sentinel_failure(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

fn assert_sentinel_failure(bytes: &[u8], expected: WorkerFailureCode) {
    let failure = OoxmlCoverageSentinel::validate_package(bytes)
        .expect_err("malformed or over-limit ZIP must fail closed");
    assert_eq!(failure.code(), expected, "{failure}");
}

#[derive(Clone)]
struct StoredEntry {
    central_name: Vec<u8>,
    local_name: Vec<u8>,
    data: Vec<u8>,
}

impl StoredEntry {
    fn new(name: &[u8], data: &[u8]) -> Self {
        Self::with_local_name(name, name, data)
    }

    fn with_local_name(central_name: &[u8], local_name: &[u8], data: &[u8]) -> Self {
        Self {
            central_name: central_name.to_vec(),
            local_name: local_name.to_vec(),
            data: data.to_vec(),
        }
    }
}

fn minimal_docx_entries() -> Vec<StoredEntry> {
    vec![
        StoredEntry::new(b"[Content_Types].xml", CONTENT_TYPES),
        StoredEntry::new(b"_rels/.rels", ROOT_RELATIONSHIPS),
        StoredEntry::new(b"word/document.xml", DOCUMENT),
        StoredEntry::new(b"word/_rels/document.xml.rels", DOCUMENT_RELATIONSHIPS),
    ]
}

fn minimal_docx() -> Vec<u8> {
    let bytes = stored_zip(&minimal_docx_entries());
    OoxmlCoverageSentinel::validate_package(&bytes)
        .expect("hand-built minimal stored DOCX fixture is valid");
    bytes
}

fn stored_zip(entries: &[StoredEntry]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut local_offsets = Vec::with_capacity(entries.len());

    for entry in entries {
        let offset = u32::try_from(bytes.len()).expect("small fixture local offset");
        local_offsets.push(offset);
        let size = u32::try_from(entry.data.len()).expect("small fixture entry size");
        let name_len = u16::try_from(entry.local_name.len()).expect("fixture name length");

        push_u32(&mut bytes, LOCAL_HEADER_SIGNATURE);
        push_u16(&mut bytes, 20); // version needed
        push_u16(&mut bytes, 0); // flags
        push_u16(&mut bytes, 0); // stored
        push_u16(&mut bytes, 0); // modification time
        push_u16(&mut bytes, 0); // modification date
        push_u32(&mut bytes, crc32(&entry.data));
        push_u32(&mut bytes, size);
        push_u32(&mut bytes, size);
        push_u16(&mut bytes, name_len);
        push_u16(&mut bytes, 0); // extra length
        bytes.extend_from_slice(&entry.local_name);
        bytes.extend_from_slice(&entry.data);
    }

    let central_offset = u32::try_from(bytes.len()).expect("small fixture central offset");
    for (entry, local_offset) in entries.iter().zip(local_offsets) {
        let size = u32::try_from(entry.data.len()).expect("small fixture entry size");
        let name_len = u16::try_from(entry.central_name.len()).expect("fixture name length");

        push_u32(&mut bytes, CENTRAL_HEADER_SIGNATURE);
        push_u16(&mut bytes, 20); // version made by
        push_u16(&mut bytes, 20); // version needed
        push_u16(&mut bytes, 0); // flags
        push_u16(&mut bytes, 0); // stored
        push_u16(&mut bytes, 0); // modification time
        push_u16(&mut bytes, 0); // modification date
        push_u32(&mut bytes, crc32(&entry.data));
        push_u32(&mut bytes, size);
        push_u32(&mut bytes, size);
        push_u16(&mut bytes, name_len);
        push_u16(&mut bytes, 0); // extra length
        push_u16(&mut bytes, 0); // entry comment length
        push_u16(&mut bytes, 0); // disk number start
        push_u16(&mut bytes, 0); // internal attributes
        push_u32(&mut bytes, 0); // external attributes
        push_u32(&mut bytes, local_offset);
        bytes.extend_from_slice(&entry.central_name);
    }
    let central_end = u32::try_from(bytes.len()).expect("small fixture central end");
    let central_size = central_end - central_offset;
    let count = u16::try_from(entries.len()).expect("fixture entry count");

    push_u32(&mut bytes, EOCD_SIGNATURE);
    push_u16(&mut bytes, 0); // current disk
    push_u16(&mut bytes, 0); // disk with central directory
    push_u16(&mut bytes, count);
    push_u16(&mut bytes, count);
    push_u32(&mut bytes, central_size);
    push_u32(&mut bytes, central_offset);
    push_u16(&mut bytes, 0); // archive comment length
    bytes
}

fn find_last_signature(bytes: &[u8], signature: u32) -> usize {
    let signature = signature.to_le_bytes();
    bytes
        .windows(signature.len())
        .rposition(|window| window == signature)
        .expect("fixture ZIP signature")
}

fn count_central_entries(bytes: &[u8], eocd: usize) -> usize {
    let central_offset = read_u32(bytes, eocd + 16) as usize;
    let central_size = read_u32(bytes, eocd + 12) as usize;
    let central_end = central_offset
        .checked_add(central_size)
        .expect("fixture central directory end");
    assert!(central_end <= eocd, "central directory precedes EOCD");

    let mut offset = central_offset;
    let mut count = 0;
    while offset < central_end {
        assert_eq!(read_u32(bytes, offset), CENTRAL_HEADER_SIGNATURE);
        let name_len = read_u16(bytes, offset + 28) as usize;
        let extra_len = read_u16(bytes, offset + 30) as usize;
        let comment_len = read_u16(bytes, offset + 32) as usize;
        offset = offset + 46 + name_len + extra_len + comment_len;
        assert!(offset <= central_end, "central record fits declared tail");
        count += 1;
    }
    assert_eq!(offset, central_end, "central directory tail is exact");
    count
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
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

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
