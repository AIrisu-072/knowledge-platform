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
    // The declared count must hit the resource cap before central-entry traversal.
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

#[test]
fn unindexed_conflicting_local_record_before_central_directory_is_rejected() {
    let mut bytes = minimal_docx();
    let original_eocd = find_last_signature(&bytes, EOCD_SIGNATURE);
    let original_central_offset = read_u32(&bytes, original_eocd + 16) as usize;
    let hidden_record = stored_local_record(b"word/document.xml", b"conflicting document");
    let hidden_record_len = u32::try_from(hidden_record.len()).expect("small fixture record");

    bytes.splice(
        original_central_offset..original_central_offset,
        hidden_record.iter().copied(),
    );
    let eocd = find_last_signature(&bytes, EOCD_SIGNATURE);
    let central_offset = original_central_offset + hidden_record.len();
    write_u32(
        &mut bytes,
        eocd + 16,
        u32::try_from(central_offset).expect("small fixture central offset"),
    );
    assert_eq!(
        read_u32(&bytes, eocd + 16),
        u32::try_from(original_central_offset).expect("small fixture central offset")
            + hidden_record_len
    );
    assert_eq!(
        read_u32(&bytes, central_offset),
        CENTRAL_HEADER_SIGNATURE,
        "the indexed central directory remains directly after the inserted local record"
    );
    assert_eq!(count_central_entries(&bytes, eocd), 4);

    let hidden_record_end = assert_stored_local_record(
        &bytes,
        original_central_offset,
        b"word/document.xml",
        b"conflicting document",
    );
    assert_eq!(
        hidden_record_end, central_offset,
        "the complete hidden local record occupies the gap before the central directory"
    );

    let central_entries = central_directory_entries(&bytes, eocd);
    assert_eq!(central_entries.len(), 4);
    assert!(
        central_entries
            .iter()
            .all(|entry| entry.local_offset != original_central_offset)
    );
    let indexed_document = central_entries
        .iter()
        .find(|entry| entry.name == b"word/document.xml")
        .expect("the original indexed document part remains present");
    assert_ne!(indexed_document.local_offset, original_central_offset);
    assert_stored_local_record(
        &bytes,
        indexed_document.local_offset,
        b"word/document.xml",
        DOCUMENT,
    );

    assert_sentinel_failure(&bytes, WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn unicode_path_extra_alias_for_required_part_is_rejected() {
    let raw_name = b"shadow.xml";
    let mapped_name = b"word/document.xml";
    let unicode_path = unicode_path_extra(raw_name, mapped_name);
    let entries = vec![
        StoredEntry::new(b"[Content_Types].xml", CONTENT_TYPES),
        StoredEntry::new(b"_rels/.rels", ROOT_RELATIONSHIPS),
        StoredEntry::new(raw_name, DOCUMENT).with_extra_fields(&unicode_path, &unicode_path),
        StoredEntry::new(b"word/_rels/document.xml.rels", DOCUMENT_RELATIONSHIPS),
    ];
    assert!(
        entries
            .iter()
            .all(|entry| entry.central_name.as_slice() != mapped_name)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.central_name.as_slice() == raw_name)
    );
    let bytes = stored_zip(&entries);
    let eocd = find_last_signature(&bytes, EOCD_SIGNATURE);
    assert_eq!(count_central_entries(&bytes, eocd), 4);

    let central_entries = central_directory_entries(&bytes, eocd);
    assert_eq!(central_entries.len(), 4);
    assert!(
        central_entries
            .iter()
            .all(|entry| entry.name.as_slice() != mapped_name)
    );
    let shadow_entry = central_entries
        .iter()
        .find(|entry| entry.name.as_slice() == raw_name)
        .expect("the raw central name remains present");
    assert_stored_local_record(&bytes, shadow_entry.local_offset, raw_name, DOCUMENT);
    assert_unicode_path_extra(&shadow_entry.extra, raw_name, mapped_name);
    let local_name_len = read_u16(&bytes, shadow_entry.local_offset + 26) as usize;
    let local_extra_len = read_u16(&bytes, shadow_entry.local_offset + 28) as usize;
    let local_extra_start = shadow_entry.local_offset + 30 + local_name_len;
    let local_extra_end = local_extra_start + local_extra_len;
    assert_unicode_path_extra(
        &bytes[local_extra_start..local_extra_end],
        raw_name,
        mapped_name,
    );

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
    local_extra: Vec<u8>,
    central_extra: Vec<u8>,
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
            local_extra: Vec::new(),
            central_extra: Vec::new(),
        }
    }

    fn with_extra_fields(mut self, local_extra: &[u8], central_extra: &[u8]) -> Self {
        self.local_extra = local_extra.to_vec();
        self.central_extra = central_extra.to_vec();
        self
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
        let extra_len = u16::try_from(entry.local_extra.len()).expect("fixture extra length");

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
        push_u16(&mut bytes, extra_len);
        bytes.extend_from_slice(&entry.local_name);
        bytes.extend_from_slice(&entry.local_extra);
        bytes.extend_from_slice(&entry.data);
    }

    let central_offset = u32::try_from(bytes.len()).expect("small fixture central offset");
    for (entry, local_offset) in entries.iter().zip(local_offsets) {
        let size = u32::try_from(entry.data.len()).expect("small fixture entry size");
        let name_len = u16::try_from(entry.central_name.len()).expect("fixture name length");
        let extra_len = u16::try_from(entry.central_extra.len()).expect("fixture extra length");

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
        push_u16(&mut bytes, extra_len);
        push_u16(&mut bytes, 0); // entry comment length
        push_u16(&mut bytes, 0); // disk number start
        push_u16(&mut bytes, 0); // internal attributes
        push_u32(&mut bytes, 0); // external attributes
        push_u32(&mut bytes, local_offset);
        bytes.extend_from_slice(&entry.central_name);
        bytes.extend_from_slice(&entry.central_extra);
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

fn stored_local_record(name: &[u8], data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let size = u32::try_from(data.len()).expect("small fixture entry size");
    let name_len = u16::try_from(name.len()).expect("fixture name length");

    push_u32(&mut bytes, LOCAL_HEADER_SIGNATURE);
    push_u16(&mut bytes, 20); // version needed
    push_u16(&mut bytes, 0); // flags
    push_u16(&mut bytes, 0); // stored
    push_u16(&mut bytes, 0); // modification time
    push_u16(&mut bytes, 0); // modification date
    push_u32(&mut bytes, crc32(data));
    push_u32(&mut bytes, size);
    push_u32(&mut bytes, size);
    push_u16(&mut bytes, name_len);
    push_u16(&mut bytes, 0); // extra length
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(data);
    bytes
}

fn unicode_path_extra(raw_name: &[u8], unicode_name: &[u8]) -> Vec<u8> {
    let data_len = u16::try_from(5 + unicode_name.len()).expect("fixture extra data length");
    let mut extra = Vec::new();
    push_u16(&mut extra, 0x7075); // Info-ZIP Unicode Path extra field
    push_u16(&mut extra, data_len);
    extra.push(1); // version
    push_u32(&mut extra, crc32(raw_name));
    extra.extend_from_slice(unicode_name);
    extra
}

fn find_last_signature(bytes: &[u8], signature: u32) -> usize {
    let signature = signature.to_le_bytes();
    bytes
        .windows(signature.len())
        .rposition(|window| window == signature)
        .expect("fixture ZIP signature")
}

fn count_central_entries(bytes: &[u8], eocd: usize) -> usize {
    central_directory_entries(bytes, eocd).len()
}

struct CentralDirectoryEntry {
    name: Vec<u8>,
    extra: Vec<u8>,
    local_offset: usize,
}

fn central_directory_entries(bytes: &[u8], eocd: usize) -> Vec<CentralDirectoryEntry> {
    let central_offset = read_u32(bytes, eocd + 16) as usize;
    let central_size = read_u32(bytes, eocd + 12) as usize;
    let central_end = central_offset
        .checked_add(central_size)
        .expect("fixture central directory end");
    assert!(central_end <= eocd, "central directory precedes EOCD");

    let mut offset = central_offset;
    let mut entries = Vec::new();
    while offset < central_end {
        let record_offset = offset;
        assert_eq!(read_u32(bytes, offset), CENTRAL_HEADER_SIGNATURE);
        let name_len = read_u16(bytes, offset + 28) as usize;
        let extra_len = read_u16(bytes, offset + 30) as usize;
        let comment_len = read_u16(bytes, offset + 32) as usize;
        let name_start = offset + 46;
        let name_end = name_start + name_len;
        let extra_end = name_end + extra_len;
        offset = extra_end + comment_len;
        assert!(offset <= central_end, "central record fits declared tail");
        entries.push(CentralDirectoryEntry {
            name: bytes[name_start..name_end].to_vec(),
            extra: bytes[name_end..extra_end].to_vec(),
            local_offset: read_u32(bytes, record_offset + 42) as usize,
        });
    }
    assert_eq!(offset, central_end, "central directory tail is exact");
    entries
}

fn assert_stored_local_record(
    bytes: &[u8],
    offset: usize,
    expected_name: &[u8],
    expected_data: &[u8],
) -> usize {
    assert_eq!(read_u32(bytes, offset), LOCAL_HEADER_SIGNATURE);
    assert_eq!(read_u16(bytes, offset + 6), 0, "fixture has no ZIP flags");
    assert_eq!(
        read_u16(bytes, offset + 8),
        0,
        "fixture uses stored entries"
    );
    assert_eq!(read_u32(bytes, offset + 14), crc32(expected_data));
    let compressed_size = read_u32(bytes, offset + 18) as usize;
    let uncompressed_size = read_u32(bytes, offset + 22) as usize;
    assert_eq!(compressed_size, expected_data.len());
    assert_eq!(uncompressed_size, expected_data.len());
    let name_len = read_u16(bytes, offset + 26) as usize;
    let extra_len = read_u16(bytes, offset + 28) as usize;
    let name_start = offset + 30;
    let extra_start = name_start + name_len;
    let data_start = extra_start + extra_len;
    let data_end = data_start + compressed_size;
    assert_eq!(&bytes[name_start..extra_start], expected_name);
    assert_eq!(&bytes[data_start..data_end], expected_data);
    data_end
}

fn assert_unicode_path_extra(extra: &[u8], raw_name: &[u8], unicode_name: &[u8]) {
    assert_eq!(read_u16(extra, 0), 0x7075);
    assert_eq!(read_u16(extra, 2) as usize, extra.len() - 4);
    assert_eq!(extra[4], 1, "Unicode Path extra field version is 1");
    assert_eq!(read_u32(extra, 5), crc32(raw_name));
    assert_eq!(&extra[9..], unicode_name);
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
