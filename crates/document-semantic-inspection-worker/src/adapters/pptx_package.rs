use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use quick_xml::{
    NsReader, Reader,
    events::{BytesStart, Event},
    name::ResolveResult,
};
use zip::ZipArchive;

use crate::{WorkerFailure, WorkerFailureCode};

const PRESENTATION_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const RELATIONSHIPS_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";
const OFFICE_DOCUMENT_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const SLIDE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const SLIDE_MASTER_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
const SLIDE_LAYOUT_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
const NOTES_SLIDE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
const NOTES_MASTER_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
const HANDOUT_MASTER_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.handoutMaster+xml";
const THEME_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const CHART_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const CHART_STYLE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chartStyle+xml";
const CHART_COLOR_STYLE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chartColorStyle+xml";
const DIAGRAM_DATA_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml";
const DIAGRAM_LAYOUT_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramLayout+xml";
const DIAGRAM_STYLE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramStyle+xml";
const DIAGRAM_COLORS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramColors+xml";
const GENERIC_XML_CONTENT_TYPE: &str = "application/xml";
const PACKAGE_CONTENT_TYPES_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/content-types";
const PACKAGE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";

const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_ENTRY_UNCOMPRESSED: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_UNCOMPRESSED: u64 = 512 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 256;
const MAX_XML_NODES: usize = 2_000_000;
const MAX_PPTX_SLIDES: usize = 4_096;
const MAX_PPTX_SHAPES: usize = 100_000;
const MAX_PPTX_IMAGE_REFERENCES: usize = 4_096;

#[derive(Debug, Clone)]
pub(super) struct Relationship {
    pub(super) kind: String,
    pub(super) target: String,
    pub(super) external: bool,
}

#[derive(Debug)]
pub(super) struct PackageInspection {
    pub(super) parts: BTreeMap<String, Vec<u8>>,
    pub(super) relationships: BTreeMap<String, BTreeMap<String, Relationship>>,
}

#[derive(Debug)]
struct ZipPreflight {
    entry_names: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct ZipEntryMetadata<'a> {
    name: &'a [u8],
    flags: u16,
    method: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
}

pub(super) fn is_pptx_package_checked(input: &[u8]) -> Result<bool, WorkerFailure> {
    if !looks_like_zip(input) {
        return Ok(false);
    }
    let preflight = preflight_zip(input)?;
    let Some(content_types_index) = preflight
        .entry_names
        .iter()
        .position(|name| name == "[Content_Types].xml")
    else {
        return Ok(false);
    };
    if !preflight
        .entry_names
        .iter()
        .any(|name| name == "ppt/presentation.xml")
    {
        return Ok(false);
    }

    let mut archive = ZipArchive::new(Cursor::new(input))
        .map_err(|error| zip_failure(format!("PPTX ZIP archive could not be opened: {error}")))?;
    if archive.len() != preflight.entry_names.len() {
        return Err(zip_failure(
            "PPTX ZIP reader disagrees with raw central-directory entry count",
        ));
    }
    let mut entry = archive
        .by_index(content_types_index)
        .map_err(|error| zip_failure(format!("read [Content_Types].xml metadata: {error}")))?;
    if entry.name() != "[Content_Types].xml" {
        return Err(zip_failure(
            "PPTX ZIP reader disagrees with raw content-types part name",
        ));
    }
    let declared_size = entry.size();
    let mut content = Vec::new();
    entry
        .by_ref()
        .take(MAX_ENTRY_UNCOMPRESSED + 1)
        .read_to_end(&mut content)
        .map_err(|error| zip_failure(format!("read [Content_Types].xml: {error}")))?;
    if content.len() as u64 > MAX_ENTRY_UNCOMPRESSED {
        return Err(resource_limit());
    }
    if content.len() as u64 != declared_size {
        return Err(zip_failure(
            "[Content_Types].xml declared and decompressed sizes disagree",
        ));
    }
    content_types_declares_presentation(&content)
}

pub(super) fn inspect_package(input: &[u8]) -> Result<PackageInspection, WorkerFailure> {
    let parts = read_package_parts(input)?;
    validate_presentation_resource_counts(&parts)?;
    let content_types = parts
        .get("[Content_Types].xml")
        .ok_or_else(|| zip_failure("PPTX is missing [Content_Types].xml"))?;
    validate_content_types(content_types)?;
    validate_package_part_coverage(&parts, content_types)?;

    let mut relationships = BTreeMap::new();
    for (name, data) in &parts {
        if !name.ends_with(".rels") {
            continue;
        }
        let source = source_part_for_rels(name)?;
        let parsed = parse_relationships(data)?;
        for relationship in parsed.values() {
            if !relationship.external {
                let target = resolve_target(&source, &relationship.target)?;
                if !parts.contains_key(&target) {
                    return Err(zip_failure(format!(
                        "PPTX relationship target {target} from {source:?} is missing"
                    )));
                }
            }
        }
        relationships.insert(source, parsed);
    }
    validate_root_office_document_relationship(&parts, &relationships)?;

    Ok(PackageInspection {
        parts,
        relationships,
    })
}

fn validate_root_office_document_relationship(
    parts: &BTreeMap<String, Vec<u8>>,
    relationships: &BTreeMap<String, BTreeMap<String, Relationship>>,
) -> Result<(), WorkerFailure> {
    let root_relationships = relationships
        .get("")
        .ok_or_else(|| zip_failure("PPTX is missing package root relationships"))?;
    let office_documents = root_relationships
        .values()
        .filter(|relationship| relationship.kind == OFFICE_DOCUMENT_RELATIONSHIP)
        .collect::<Vec<_>>();
    if office_documents.len() != 1 {
        return Err(zip_failure(
            "PPTX must contain exactly one package root officeDocument relationship",
        ));
    }

    let office_document = office_documents[0];
    if office_document.external
        || resolve_target("", &office_document.target)? != "ppt/presentation.xml"
        || !parts.contains_key("ppt/presentation.xml")
    {
        return Err(zip_failure(
            "PPTX package root officeDocument relationship must target ppt/presentation.xml",
        ));
    }
    Ok(())
}

fn content_types_declares_presentation(data: &[u8]) -> Result<bool, WorkerFailure> {
    validate_opc_package_part_qnames("[Content_Types].xml", data)?;
    let text =
        std::str::from_utf8(data).map_err(|_| zip_failure("PPTX content types are not UTF-8"))?;
    let mut reader = Reader::from_str(text);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element))
                if matches!(element.local_name().as_ref(), "Default" | "Override") =>
            {
                if attr(&element, "ContentType")?.as_deref() == Some(PRESENTATION_MAIN) {
                    return Ok(true);
                }
            }
            Ok(Event::Eof) => return Ok(false),
            Ok(_) => {}
            Err(error) => {
                return Err(zip_failure(format!("PPTX content-types XML: {error}")));
            }
        }
    }
}

fn validate_presentation_resource_counts(
    parts: &BTreeMap<String, Vec<u8>>,
) -> Result<(), WorkerFailure> {
    let slide_parts = parts
        .iter()
        .filter(|(name, _)| name.starts_with("ppt/slides/") && name.ends_with(".xml"))
        .collect::<Vec<_>>();
    if slide_parts.len() > MAX_PPTX_SLIDES {
        return Err(resource_limit());
    }

    let mut shape_count = 0usize;
    let mut image_reference_count = 0usize;
    for (name, data) in slide_parts {
        let text = std::str::from_utf8(data)
            .map_err(|_| zip_failure(format!("{name} is not UTF-8 XML")))?;
        let mut reader = Reader::from_str(text);
        loop {
            match reader.read_event() {
                Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                    let local_name = event.local_name();
                    if matches!(
                        local_name.as_ref(),
                        "sp" | "pic" | "graphicFrame" | "cxnSp" | "grpSp" | "contentPart"
                    ) {
                        shape_count = shape_count.checked_add(1).ok_or_else(resource_limit)?;
                        if shape_count > MAX_PPTX_SHAPES {
                            return Err(resource_limit());
                        }
                    }
                    if local_name.as_ref() == "blip" {
                        image_reference_count = image_reference_count
                            .checked_add(1)
                            .ok_or_else(resource_limit)?;
                        if image_reference_count > MAX_PPTX_IMAGE_REFERENCES {
                            return Err(resource_limit());
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(error) => {
                    return Err(zip_failure(format!(
                        "{name} XML resource scan failed: {error}"
                    )));
                }
            }
        }
    }

    let mut image_relationship_count = 0usize;
    for (name, data) in parts.iter().filter(|(name, _)| name.ends_with(".rels")) {
        let text = std::str::from_utf8(data)
            .map_err(|_| zip_failure(format!("{name} is not UTF-8 XML")))?;
        let mut reader = Reader::from_str(text);
        loop {
            match reader.read_event() {
                Ok(Event::Start(event)) | Ok(Event::Empty(event))
                    if event.local_name().as_ref() == "Relationship" =>
                {
                    let kind = attr_required(&event, "Type")?;
                    if kind.ends_with("/image") {
                        image_relationship_count = image_relationship_count
                            .checked_add(1)
                            .ok_or_else(resource_limit)?;
                        if image_relationship_count > MAX_PPTX_IMAGE_REFERENCES {
                            return Err(resource_limit());
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(error) => {
                    return Err(zip_failure(format!(
                        "{name} XML image-reference scan failed: {error}"
                    )));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn resolve_target(source: &str, target: &str) -> Result<String, WorkerFailure> {
    validate_opc_pack_uri_path(target, "relationship target", false)?;
    if target.is_empty() || target.starts_with('/') || target.contains('\\') {
        return Err(zip_failure(format!(
            "unsafe PPTX relationship target {target:?}"
        )));
    }
    if target
        .split('/')
        .next()
        .is_some_and(|segment| segment.contains(':'))
    {
        return Err(zip_failure(format!(
            "PPTX internal relationship target is not a relative path: {target:?}"
        )));
    }
    let mut segments: Vec<&str> = source
        .rsplit_once('/')
        .map(|(dir, _)| dir.split('/').filter(|part| !part.is_empty()).collect())
        .unwrap_or_default();

    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(zip_failure(format!(
                        "PPTX relationship escapes package root: {target:?}"
                    )));
                }
            }
            other => segments.push(other),
        }
    }
    Ok(segments.join("/"))
}

fn read_package_parts(input: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, WorkerFailure> {
    let preflight = preflight_zip(input)?;
    let mut archive = ZipArchive::new(Cursor::new(input))
        .map_err(|error| zip_failure(format!("PPTX ZIP archive could not be opened: {error}")))?;
    if archive.len() != preflight.entry_names.len() {
        return Err(zip_failure(
            "PPTX ZIP reader disagrees with raw central-directory entry count",
        ));
    }

    let mut parts = BTreeMap::new();
    let mut actual_total = 0u64;
    let mut total_xml_nodes = 0usize;
    for (index, raw_name) in preflight.entry_names.iter().enumerate() {
        let entry = archive
            .by_index(index)
            .map_err(|error| zip_failure(format!("PPTX ZIP entry {index}: {error}")))?;
        if entry.name() != raw_name {
            return Err(zip_failure(format!(
                "PPTX ZIP reader disagrees with raw entry name {raw_name:?}"
            )));
        }
        let is_directory = entry.is_dir();
        let name = entry.name().to_owned();
        validate_part_name(&name, is_directory)?;
        let declared_size = entry.size();
        let remaining_total = MAX_TOTAL_UNCOMPRESSED
            .checked_sub(actual_total)
            .ok_or_else(resource_limit)?;
        let read_limit = MAX_ENTRY_UNCOMPRESSED.min(remaining_total);
        let mut data = Vec::new();
        entry
            .take(read_limit + 1)
            .read_to_end(&mut data)
            .map_err(|error| zip_failure(format!("read {name}: {error}")))?;
        let actual_size = data.len() as u64;
        if actual_size > read_limit {
            return Err(resource_limit());
        }
        if actual_size != declared_size {
            return Err(zip_failure(format!(
                "PPTX entry {name} declared {declared_size} bytes but produced {actual_size}"
            )));
        }
        actual_total = actual_total
            .checked_add(actual_size)
            .ok_or_else(resource_limit)?;
        if actual_total > MAX_TOTAL_UNCOMPRESSED {
            return Err(resource_limit());
        }
        if is_directory {
            if !data.is_empty() {
                return Err(zip_failure(format!(
                    "PPTX directory entry {name} contains data"
                )));
            }
            continue;
        }
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml_nodes = validate_xml(&name, &data)?;
            total_xml_nodes = total_xml_nodes
                .checked_add(xml_nodes)
                .ok_or_else(resource_limit)?;
            if total_xml_nodes > MAX_XML_NODES {
                return Err(resource_limit());
            }
            if name == "[Content_Types].xml" || name.ends_with(".rels") {
                validate_opc_package_part_qnames(&name, &data)?;
            }
        }
        if parts.insert(name.clone(), data).is_some() {
            return Err(zip_failure(format!("duplicate PPTX package part {name}")));
        }
    }
    Ok(parts)
}

fn preflight_zip(input: &[u8]) -> Result<ZipPreflight, WorkerFailure> {
    const EOCD_FIXED_SIZE: usize = 22;
    const MAX_EOCD_COMMENT_SIZE: usize = u16::MAX as usize;

    if input.len() < EOCD_FIXED_SIZE {
        return Err(zip_failure(
            "truncated PPTX ZIP end-of-central-directory record",
        ));
    }
    let search_start = input
        .len()
        .saturating_sub(EOCD_FIXED_SIZE + MAX_EOCD_COMMENT_SIZE);
    let search_end = input.len() - EOCD_FIXED_SIZE;
    let mut eocd_offset = None;
    for offset in search_start..=search_end {
        if read_u32(input, offset) != Some(0x0605_4b50) {
            continue;
        }
        let Some(comment_len) = read_u16(input, offset + 20) else {
            continue;
        };
        let Some(record_end) = offset
            .checked_add(EOCD_FIXED_SIZE)
            .and_then(|value| value.checked_add(usize::from(comment_len)))
        else {
            continue;
        };
        if record_end == input.len() && eocd_offset.replace(offset).is_some() {
            return Err(zip_failure(
                "ambiguous PPTX ZIP end-of-central-directory records",
            ));
        }
    }
    let eocd_offset = eocd_offset
        .ok_or_else(|| zip_failure("PPTX ZIP end-of-central-directory record not found"))?;

    let disk_number = read_u16(input, eocd_offset + 4)
        .ok_or_else(|| zip_failure("truncated PPTX ZIP disk number"))?;
    let central_directory_disk = read_u16(input, eocd_offset + 6)
        .ok_or_else(|| zip_failure("truncated PPTX ZIP central-directory disk number"))?;
    let entries_on_disk = read_u16(input, eocd_offset + 8)
        .ok_or_else(|| zip_failure("truncated PPTX ZIP disk entry count"))?;
    let total_entries = read_u16(input, eocd_offset + 10)
        .ok_or_else(|| zip_failure("truncated PPTX ZIP entry count"))?;
    if entries_on_disk == u16::MAX || total_entries == u16::MAX {
        return Err(zip_failure("PPTX ZIP64 archives are unsupported"));
    }
    if usize::from(entries_on_disk) > MAX_ARCHIVE_ENTRIES
        || usize::from(total_entries) > MAX_ARCHIVE_ENTRIES
    {
        return Err(resource_limit());
    }
    if disk_number != 0 || central_directory_disk != 0 || entries_on_disk != total_entries {
        return Err(zip_failure("multi-disk PPTX ZIP archives are unsupported"));
    }

    let central_directory_size = read_u32(input, eocd_offset + 12)
        .ok_or_else(|| zip_failure("truncated PPTX ZIP central-directory size"))?;
    let central_directory_offset = read_u32(input, eocd_offset + 16)
        .ok_or_else(|| zip_failure("truncated PPTX ZIP central-directory offset"))?;
    if central_directory_size == u32::MAX || central_directory_offset == u32::MAX {
        return Err(zip_failure("PPTX ZIP64 archives are unsupported"));
    }
    let central_directory_size = usize::try_from(central_directory_size)
        .map_err(|_| zip_failure("PPTX ZIP central-directory size is out of range"))?;
    let central_directory_offset = usize::try_from(central_directory_offset)
        .map_err(|_| zip_failure("PPTX ZIP central-directory offset is out of range"))?;
    let central_directory_start = eocd_offset
        .checked_sub(central_directory_size)
        .ok_or_else(|| zip_failure("PPTX ZIP central directory extends beyond EOF"))?;
    if central_directory_offset > central_directory_start {
        return Err(zip_failure(
            "PPTX ZIP central-directory offset precedes the archive boundary",
        ));
    }
    let archive_offset = central_directory_start - central_directory_offset;
    if archive_offset != 0 {
        return Err(zip_failure(
            "PPTX ZIP has unaccounted bytes before its first local record",
        ));
    }
    let central_directory_end = eocd_offset;
    if central_directory_start.checked_add(central_directory_size) != Some(central_directory_end) {
        return Err(zip_failure(
            "PPTX ZIP central directory does not end at its EOCD record",
        ));
    }

    let mut offset = central_directory_start;
    let mut normalized_names = BTreeSet::new();
    let mut entry_names = Vec::with_capacity(usize::from(total_entries));
    let mut local_ranges = Vec::with_capacity(usize::from(total_entries));
    let mut declared_total = 0u64;
    for _ in 0..total_entries {
        const CENTRAL_HEADER_SIZE: usize = 46;
        if read_u32(input, offset) != Some(0x0201_4b50) {
            return Err(zip_failure(
                "unexpected record in PPTX ZIP central directory",
            ));
        }
        let header_end = offset
            .checked_add(CENTRAL_HEADER_SIZE)
            .ok_or_else(|| zip_failure("PPTX ZIP central-directory offset overflow"))?;
        let header = input
            .get(offset..header_end)
            .ok_or_else(|| zip_failure("truncated PPTX ZIP central-directory entry"))?;
        let flags = u16::from_le_bytes([header[8], header[9]]);
        let method = u16::from_le_bytes([header[10], header[11]]);
        let crc32 = u32::from_le_bytes([header[16], header[17], header[18], header[19]]);
        let compressed_size = u32::from_le_bytes([header[20], header[21], header[22], header[23]]);
        let uncompressed_size =
            u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
        let name_len = usize::from(u16::from_le_bytes([header[28], header[29]]));
        let extra_len = usize::from(u16::from_le_bytes([header[30], header[31]]));
        let comment_len = usize::from(u16::from_le_bytes([header[32], header[33]]));
        let disk_start = u16::from_le_bytes([header[34], header[35]]);
        let local_header_offset =
            u32::from_le_bytes([header[42], header[43], header[44], header[45]]);
        if compressed_size == u32::MAX
            || uncompressed_size == u32::MAX
            || local_header_offset == u32::MAX
            || disk_start == u16::MAX
        {
            return Err(zip_failure("PPTX ZIP64 entries are unsupported"));
        }
        if disk_start != 0 {
            return Err(zip_failure("multi-disk PPTX ZIP entries are unsupported"));
        }
        if flags & (0x0001 | 0x0040 | 0x2000) != 0 {
            return Err(zip_failure("encrypted PPTX ZIP entries are unsupported"));
        }
        if method != 0 && method != 8 {
            return Err(zip_failure(
                "PPTX ZIP entry uses an unsupported compression method",
            ));
        }

        let name_start = header_end;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or_else(|| zip_failure("PPTX ZIP entry name length overflow"))?;
        let extra_start = name_end;
        let extra_end = extra_start
            .checked_add(extra_len)
            .ok_or_else(|| zip_failure("PPTX ZIP extra-field length overflow"))?;
        let entry_end = extra_end
            .checked_add(comment_len)
            .ok_or_else(|| zip_failure("PPTX ZIP comment length overflow"))?;
        if entry_end > central_directory_end {
            return Err(zip_failure(
                "truncated PPTX ZIP central-directory variable field",
            ));
        }
        let name = input
            .get(name_start..name_end)
            .ok_or_else(|| zip_failure("truncated PPTX ZIP central-directory name"))?;
        if name.is_empty() {
            return Err(zip_failure("empty PPTX ZIP entry name"));
        }
        validate_zip_extra_fields(input, extra_start, extra_end)?;
        let name_text = std::str::from_utf8(name)
            .map_err(|_| zip_failure("PPTX ZIP entry name is not UTF-8"))?;
        if flags & 0x0800 == 0 && !name.is_ascii() {
            return Err(zip_failure(
                "non-ASCII PPTX ZIP entry name lacks the UTF-8 flag",
            ));
        }
        let normalized = normalized_zip_name(name_text)?;
        if !normalized_names.insert(normalized) {
            return Err(zip_failure(
                "duplicate or file/directory-aliased PPTX ZIP entry name",
            ));
        }
        let is_directory = name_text.ends_with('/');
        validate_part_name(name_text, is_directory)?;
        entry_names.push(name_text.to_owned());

        let uncompressed_size = u64::from(uncompressed_size);
        let compressed_size = u64::from(compressed_size);
        if uncompressed_size > MAX_ENTRY_UNCOMPRESSED {
            return Err(resource_limit());
        }
        declared_total = declared_total
            .checked_add(uncompressed_size)
            .ok_or_else(resource_limit)?;
        if declared_total > MAX_TOTAL_UNCOMPRESSED {
            return Err(resource_limit());
        }

        let local_start = usize::try_from(local_header_offset)
            .map_err(|_| zip_failure("PPTX ZIP local-header offset is out of range"))?;
        let metadata = ZipEntryMetadata {
            name,
            flags,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
        };
        let local_end =
            validate_local_header(input, local_start, central_directory_start, metadata)?;
        local_ranges.push((local_start, local_end));
        offset = entry_end;
    }
    if offset != central_directory_end {
        return Err(zip_failure(
            "PPTX ZIP central-directory size disagrees with its declared entry count",
        ));
    }

    local_ranges.sort_unstable_by_key(|range| range.0);
    let mut covered_until = archive_offset;
    for (local_start, local_end) in local_ranges {
        if local_start != covered_until {
            return Err(zip_failure(
                "PPTX ZIP local records do not contiguously cover archive data",
            ));
        }
        covered_until = local_end;
    }
    if covered_until != central_directory_start {
        return Err(zip_failure(
            "PPTX ZIP local records do not contiguously cover archive data",
        ));
    }

    Ok(ZipPreflight { entry_names })
}

fn validate_local_header(
    input: &[u8],
    start: usize,
    central_directory_start: usize,
    central: ZipEntryMetadata<'_>,
) -> Result<usize, WorkerFailure> {
    const LOCAL_HEADER_SIZE: usize = 30;
    if start >= central_directory_start || read_u32(input, start) != Some(0x0403_4b50) {
        return Err(zip_failure("invalid PPTX ZIP local-file header offset"));
    }
    let header_end = start
        .checked_add(LOCAL_HEADER_SIZE)
        .ok_or_else(|| zip_failure("PPTX ZIP local-header offset overflow"))?;
    let header = input
        .get(start..header_end)
        .ok_or_else(|| zip_failure("truncated PPTX ZIP local-file header"))?;
    let flags = u16::from_le_bytes([header[6], header[7]]);
    let method = u16::from_le_bytes([header[8], header[9]]);
    let crc32 = u32::from_le_bytes([header[14], header[15], header[16], header[17]]);
    let compressed_size = u32::from_le_bytes([header[18], header[19], header[20], header[21]]);
    let uncompressed_size = u32::from_le_bytes([header[22], header[23], header[24], header[25]]);
    let name_len = usize::from(u16::from_le_bytes([header[26], header[27]]));
    let extra_len = usize::from(u16::from_le_bytes([header[28], header[29]]));
    if compressed_size == u32::MAX || uncompressed_size == u32::MAX {
        return Err(zip_failure("PPTX ZIP64 local entries are unsupported"));
    }
    if flags != central.flags || method != central.method {
        return Err(zip_failure(
            "PPTX ZIP local and central entry flags or methods disagree",
        ));
    }
    let name_start = header_end;
    let name_end = name_start
        .checked_add(name_len)
        .ok_or_else(|| zip_failure("PPTX ZIP local name length overflow"))?;
    let extra_start = name_end;
    let extra_end = extra_start
        .checked_add(extra_len)
        .ok_or_else(|| zip_failure("PPTX ZIP local extra-field length overflow"))?;
    if extra_end > central_directory_start {
        return Err(zip_failure(
            "PPTX ZIP local header extends into the central directory",
        ));
    }
    let local_name = input
        .get(name_start..name_end)
        .ok_or_else(|| zip_failure("truncated PPTX ZIP local-file name"))?;
    if local_name != central.name {
        return Err(zip_failure(
            "PPTX ZIP local and central entry names disagree",
        ));
    }
    validate_zip_extra_fields(input, extra_start, extra_end)?;

    let has_data_descriptor = central.flags & 0x0008 != 0;
    let local_compressed_size = u64::from(compressed_size);
    let local_uncompressed_size = u64::from(uncompressed_size);
    if has_data_descriptor {
        if (crc32 != 0 && crc32 != central.crc32)
            || (local_compressed_size != 0 && local_compressed_size != central.compressed_size)
            || (local_uncompressed_size != 0
                && local_uncompressed_size != central.uncompressed_size)
        {
            return Err(zip_failure(
                "PPTX ZIP local data-descriptor placeholders disagree with central metadata",
            ));
        }
    } else if crc32 != central.crc32
        || local_compressed_size != central.compressed_size
        || local_uncompressed_size != central.uncompressed_size
    {
        return Err(zip_failure(
            "PPTX ZIP local and central entry sizes or checksums disagree",
        ));
    }

    let data_start = extra_end;
    let data_end = data_start
        .checked_add(
            usize::try_from(central.compressed_size)
                .map_err(|_| zip_failure("PPTX ZIP compressed size is out of range"))?,
        )
        .ok_or_else(|| zip_failure("PPTX ZIP compressed-data range overflow"))?;
    if data_end > central_directory_start {
        return Err(zip_failure(
            "PPTX ZIP compressed data extends into the central directory",
        ));
    }
    if !has_data_descriptor {
        return Ok(data_end);
    }

    let unsigned_end = data_end.checked_add(12);
    let unsigned_matches = unsigned_end.is_some_and(|end| {
        end <= central_directory_start
            && read_u32(input, data_end) == Some(central.crc32)
            && read_u32(input, data_end + 4) == u32::try_from(central.compressed_size).ok()
            && read_u32(input, data_end + 8) == u32::try_from(central.uncompressed_size).ok()
    });
    let signed_end = data_end.checked_add(16);
    let signed_matches = signed_end.is_some_and(|end| {
        end <= central_directory_start
            && read_u32(input, data_end) == Some(0x0807_4b50)
            && read_u32(input, data_end + 4) == Some(central.crc32)
            && read_u32(input, data_end + 8) == u32::try_from(central.compressed_size).ok()
            && read_u32(input, data_end + 12) == u32::try_from(central.uncompressed_size).ok()
    });
    match (unsigned_matches, signed_matches) {
        (true, false) => Ok(unsigned_end.unwrap_or(data_end)),
        (false, true) => Ok(signed_end.unwrap_or(data_end)),
        _ => Err(zip_failure("invalid or ambiguous PPTX ZIP data descriptor")),
    }
}

fn normalized_zip_name(name: &str) -> Result<String, WorkerFailure> {
    if name.is_empty() || name.contains('\0') {
        return Err(zip_failure("empty or NUL-containing PPTX ZIP entry name"));
    }
    let normalized = name.strip_suffix('/').unwrap_or(name);
    if normalized.is_empty() {
        return Err(zip_failure("empty PPTX ZIP entry name"));
    }
    Ok(normalized.to_ascii_lowercase())
}

fn validate_part_name(name: &str, is_directory: bool) -> Result<(), WorkerFailure> {
    validate_opc_pack_uri_path(name, "part name", name == "[Content_Types].xml")?;
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || (is_directory && !name.ends_with('/'))
        || (!is_directory && name.ends_with('/'))
    {
        return Err(zip_failure(format!("unsafe PPTX part name {name:?}")));
    }
    let path = if is_directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        || path
            .split('/')
            .next()
            .is_some_and(|segment| segment.contains(':'))
    {
        return Err(zip_failure(format!("unsafe PPTX part name {name:?}")));
    }
    Ok(())
}

fn validate_opc_pack_uri_path(
    value: &str,
    description: &str,
    allow_content_types_brackets: bool,
) -> Result<(), WorkerFailure> {
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut has_percent_encoding = false;

    while index < bytes.len() {
        match bytes[index] {
            b'?' | b'#' => {
                return Err(zip_failure(format!(
                    "PPTX {description} contains an unsupported URI query or fragment delimiter"
                )));
            }
            b'%' => {
                let escape = bytes.get(index + 1..index + 3).ok_or_else(|| {
                    zip_failure(format!("malformed percent escape in PPTX {description}"))
                })?;
                if !escape.iter().all(|digit| digit.is_ascii_hexdigit()) {
                    return Err(zip_failure(format!(
                        "malformed percent escape in PPTX {description}"
                    )));
                }
                has_percent_encoding = true;
                index += 3;
                continue;
            }
            byte if is_supported_opc_uri_path_byte(byte, allow_content_types_brackets) => {}
            _ => {
                return Err(unsupported(format!(
                    "PPTX {description} is outside the qualified ASCII OPC URI path subset"
                )));
            }
        }
        index += 1;
    }

    if has_percent_encoding {
        return Err(unsupported(format!(
            "percent-encoded PPTX {description} is outside the qualified OPC URI path subset"
        )));
    }

    Ok(())
}

fn is_supported_opc_uri_path_byte(byte: u8, allow_content_types_brackets: bool) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'/' | b'-'
                | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
        || (allow_content_types_brackets && matches!(byte, b'[' | b']'))
}

fn validate_xml(name: &str, data: &[u8]) -> Result<usize, WorkerFailure> {
    let text =
        std::str::from_utf8(data).map_err(|_| zip_failure(format!("{name} is not UTF-8 XML")))?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut depth = 0usize;
    let mut nodes = 0usize;
    let mut roots = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => {
                nodes += 1;
                if nodes > MAX_XML_NODES {
                    return Err(resource_limit());
                }
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
                if depth > MAX_XML_DEPTH {
                    return Err(resource_limit());
                }
            }
            Ok(Event::Empty(_)) => {
                nodes += 1;
                if nodes > MAX_XML_NODES {
                    return Err(resource_limit());
                }
                if depth == 0 {
                    roots += 1;
                }
            }
            Ok(Event::End(_)) => {
                if depth == 0 {
                    return Err(zip_failure(format!("{name} has unmatched closing tag")));
                }
                depth -= 1;
            }
            Ok(Event::DocType(_)) => {
                return Err(unsupported(format!(
                    "{name} contains a document type declaration"
                )));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(zip_failure(format!("{name} XML parse failed: {error}")));
            }
        }
    }
    if depth != 0 {
        return Err(zip_failure(format!(
            "{name} ended with {depth} unclosed XML elements"
        )));
    }
    if roots != 1 {
        return Err(zip_failure(format!(
            "{name} must contain one XML root element"
        )));
    }
    Ok(nodes)
}

fn validate_opc_package_part_qnames(name: &str, data: &[u8]) -> Result<(), WorkerFailure> {
    let (namespace, root_name, child_names): (&str, &str, &[&str]) =
        if name == "[Content_Types].xml" {
            (PACKAGE_CONTENT_TYPES_NS, "Types", &["Default", "Override"])
        } else if name.ends_with(".rels") {
            (PACKAGE_RELATIONSHIPS_NS, "Relationships", &["Relationship"])
        } else {
            return Ok(());
        };
    let text =
        std::str::from_utf8(data).map_err(|_| zip_failure(format!("{name} is not UTF-8 XML")))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut depth = 0usize;
    let mut saw_root = false;
    let mut root_closed = false;

    loop {
        let (resolved_namespace, event) = reader
            .read_resolved_event()
            .map_err(|error| zip_failure(format!("{name} QName parse failed: {error}")))?;
        match event {
            Event::Start(element) => {
                if !saw_root {
                    validate_opc_element_qname(
                        name,
                        &resolved_namespace,
                        element.local_name().as_ref(),
                        namespace,
                        root_name,
                    )?;
                    validate_opc_attributes(name, &element, &[])?;
                    saw_root = true;
                    depth = 1;
                } else if root_closed {
                    return Err(zip_failure(format!("{name} contains multiple XML roots")));
                } else if depth == 1 {
                    let local_name = element.local_name();
                    let local_name = local_name.as_ref();
                    if !child_names.contains(&local_name) {
                        return Err(unsupported(format!("unsupported {name} child QName")));
                    }
                    validate_opc_element_qname(
                        name,
                        &resolved_namespace,
                        local_name,
                        namespace,
                        local_name,
                    )?;
                    validate_opc_child_attributes(name, &element, local_name)?;
                    depth = 2;
                } else {
                    return Err(unsupported(format!(
                        "nested elements are unsupported in {name}"
                    )));
                }
            }
            Event::Empty(element) => {
                if !saw_root {
                    validate_opc_element_qname(
                        name,
                        &resolved_namespace,
                        element.local_name().as_ref(),
                        namespace,
                        root_name,
                    )?;
                    validate_opc_attributes(name, &element, &[])?;
                    saw_root = true;
                    root_closed = true;
                } else if !root_closed && depth == 1 {
                    let local_name = element.local_name();
                    let local_name = local_name.as_ref();
                    if !child_names.contains(&local_name) {
                        return Err(unsupported(format!(
                            "unsupported {name} child QName {{{namespace}}}{local_name}"
                        )));
                    }
                    validate_opc_element_qname(
                        name,
                        &resolved_namespace,
                        local_name,
                        namespace,
                        local_name,
                    )?;
                    validate_opc_child_attributes(name, &element, local_name)?;
                } else {
                    return Err(zip_failure(format!(
                        "{name} contains an element outside its root"
                    )));
                }
            }
            Event::End(_) => match depth {
                2 => depth = 1,
                1 => {
                    depth = 0;
                    root_closed = true;
                }
                _ => {
                    return Err(zip_failure(format!(
                        "{name} has an unmatched closing element"
                    )));
                }
            },
            Event::Text(text)
                if !text
                    .as_ref()
                    .chars()
                    .all(|character| character.is_ascii_whitespace()) =>
            {
                return Err(unsupported(format!(
                    "text content is unsupported in {name}"
                )));
            }
            Event::DocType(_) => {
                return Err(unsupported(format!(
                    "{name} contains a document type declaration"
                )));
            }
            Event::GeneralRef(reference) => {
                let is_inter_element_xml_whitespace = depth == 1
                    && !root_closed
                    && reference
                        .resolve_char_ref()
                        .ok()
                        .flatten()
                        .is_some_and(|character| matches!(character, ' ' | '\t' | '\r' | '\n'));
                if !is_inter_element_xml_whitespace {
                    return Err(unsupported(format!(
                        "text constructs are unsupported in {name}"
                    )));
                }
            }
            Event::CData(_) => {
                return Err(unsupported(format!(
                    "text constructs are unsupported in {name}"
                )));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_root || !root_closed || depth != 0 {
        return Err(zip_failure(format!("{name} has an incomplete OPC root")));
    }
    Ok(())
}

fn validate_opc_element_qname(
    name: &str,
    resolved_namespace: &ResolveResult<'_>,
    local_name: &str,
    expected_namespace: &str,
    expected_local_name: &str,
) -> Result<(), WorkerFailure> {
    if !matches!(resolved_namespace, ResolveResult::Bound(uri) if uri.as_ref() == expected_namespace)
        || local_name != expected_local_name
    {
        return Err(unsupported(format!("{name} has an unexpected OPC QName")));
    }
    Ok(())
}

fn validate_opc_child_attributes(
    name: &str,
    element: &BytesStart<'_>,
    local_name: &str,
) -> Result<(), WorkerFailure> {
    let allowed: &[&str] = match (name == "[Content_Types].xml", local_name) {
        (true, "Default") => &["Extension", "ContentType"],
        (true, "Override") => &["PartName", "ContentType"],
        (false, "Relationship") => &["Id", "Type", "Target", "TargetMode"],
        _ => {
            return Err(unsupported(format!(
                "unsupported {name} child {local_name}"
            )));
        }
    };
    validate_opc_attributes(name, element, allowed)
}

fn validate_opc_attributes(
    name: &str,
    element: &BytesStart<'_>,
    allowed: &[&str],
) -> Result<(), WorkerFailure> {
    let mut seen = BTreeSet::new();
    for attribute in element.attributes() {
        let attribute =
            attribute.map_err(|error| zip_failure(format!("invalid {name} attribute: {error}")))?;
        let key = attribute.key.as_ref();
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        if key.contains(':') || !allowed.contains(&key) || !seen.insert(key.to_owned()) {
            return Err(unsupported(format!(
                "unsupported or qualified attribute in {name}"
            )));
        }
    }
    Ok(())
}

fn validate_content_types(data: &[u8]) -> Result<(), WorkerFailure> {
    let text =
        std::str::from_utf8(data).map_err(|_| zip_failure("PPTX content types are not UTF-8"))?;
    let known: BTreeSet<&str> = [
        RELATIONSHIPS_CONTENT_TYPE,
        "application/xml",
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/bmp",
        PRESENTATION_MAIN,
        SLIDE_CONTENT_TYPE,
        SLIDE_MASTER_CONTENT_TYPE,
        SLIDE_LAYOUT_CONTENT_TYPE,
        NOTES_SLIDE_CONTENT_TYPE,
        NOTES_MASTER_CONTENT_TYPE,
        HANDOUT_MASTER_CONTENT_TYPE,
        "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml",
        THEME_CONTENT_TYPE,
        CHART_CONTENT_TYPE,
        CHART_STYLE_CONTENT_TYPE,
        CHART_COLOR_STYLE_CONTENT_TYPE,
        DIAGRAM_DATA_CONTENT_TYPE,
        DIAGRAM_LAYOUT_CONTENT_TYPE,
        DIAGRAM_STYLE_CONTENT_TYPE,
        DIAGRAM_COLORS_CONTENT_TYPE,
        "application/vnd.openxmlformats-package.core-properties+xml",
        "application/vnd.openxmlformats-officedocument.extended-properties+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.comments+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.commentAuthors+xml",
    ]
    .into_iter()
    .collect();
    let mut reader = Reader::from_str(text);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if matches!(event.local_name().as_ref(), "Default" | "Override") =>
            {
                let value = attr(&event, "ContentType")?.ok_or_else(|| {
                    zip_failure("PPTX content-type declaration has no ContentType")
                })?;
                if !known.contains(value.as_str()) {
                    return Err(unsupported(format!("unknown PPTX content type {value}")));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(zip_failure(format!("PPTX content-types XML: {error}")));
            }
        }
    }
    Ok(())
}

fn validate_package_part_coverage(
    parts: &BTreeMap<String, Vec<u8>>,
    data: &[u8],
) -> Result<(), WorkerFailure> {
    let text =
        std::str::from_utf8(data).map_err(|_| zip_failure("PPTX content types are not UTF-8"))?;
    let mut reader = Reader::from_str(text);
    let mut defaults = BTreeMap::<String, String>::new();
    let mut overrides = BTreeMap::<String, String>::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Default" =>
            {
                let extension = attr_required(&event, "Extension")?.to_ascii_lowercase();
                let content_type = attr_required(&event, "ContentType")?;
                if extension.is_empty() || extension.contains('/') || extension.contains('\\') {
                    return Err(zip_failure("invalid PPTX default content-type extension"));
                }
                if defaults.insert(extension.clone(), content_type).is_some() {
                    return Err(zip_failure(format!(
                        "duplicate PPTX default content type for {extension}"
                    )));
                }
            }
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Override" =>
            {
                let part_name = attr_required(&event, "PartName")?;
                let part_name = part_name.strip_prefix('/').ok_or_else(|| {
                    zip_failure(format!(
                        "PPTX content-type override path is not absolute: {part_name}"
                    ))
                })?;
                validate_part_name(part_name, false)?;
                let content_type = attr_required(&event, "ContentType")?;
                if overrides
                    .insert(part_name.to_owned(), content_type)
                    .is_some()
                {
                    return Err(zip_failure(format!(
                        "duplicate PPTX content-type override for {part_name}"
                    )));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(zip_failure(format!("PPTX content-types XML: {error}")));
            }
        }
    }

    for name in overrides.keys() {
        if !parts.contains_key(name) {
            return Err(unsupported(format!(
                "PPTX content-type override references missing part {name}"
            )));
        }
    }
    for name in parts
        .keys()
        .filter(|name| name.as_str() != "[Content_Types].xml")
    {
        let Some(expected) = expected_part_content_type(name, parts)? else {
            return Err(unsupported(format!("unmodeled PPTX package part {name}")));
        };
        let extension = name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase());
        let content_type = overrides
            .get(name)
            .or_else(|| {
                extension
                    .as_deref()
                    .and_then(|extension| defaults.get(extension))
            })
            .ok_or_else(|| unsupported(format!("PPTX package part has no content type: {name}")))?;
        if content_type != expected {
            return Err(unsupported(format!(
                "PPTX part path/content-type mismatch for {name}: expected {expected}, found {content_type}"
            )));
        }
        if content_type == GENERIC_XML_CONTENT_TYPE {
            return Err(unsupported(format!(
                "unmodeled generic XML PPTX part {name}"
            )));
        }
    }
    Ok(())
}

fn expected_part_content_type(
    name: &str,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Result<Option<&'static str>, WorkerFailure> {
    if name == "_rels/.rels" {
        return Ok(Some(RELATIONSHIPS_CONTENT_TYPE));
    }
    if name.contains("/_rels/") {
        let source = source_part_for_rels(name)?;
        if !parts.contains_key(&source) {
            return Err(unsupported(format!(
                "PPTX relationships part {name} has no source part {source}"
            )));
        }
        if expected_non_relationship_content_type(&source).is_none() {
            return Err(unsupported(format!(
                "PPTX relationships part {name} has an unmodeled source part {source}"
            )));
        }
        return Ok(Some(RELATIONSHIPS_CONTENT_TYPE));
    }
    Ok(expected_non_relationship_content_type(name))
}

fn expected_non_relationship_content_type(name: &str) -> Option<&'static str> {
    match name {
        "ppt/presentation.xml" => Some(PRESENTATION_MAIN),
        "ppt/commentAuthors.xml" => {
            Some("application/vnd.openxmlformats-officedocument.presentationml.commentAuthors+xml")
        }
        "ppt/presProps.xml" => {
            Some("application/vnd.openxmlformats-officedocument.presentationml.presProps+xml")
        }
        "ppt/viewProps.xml" => {
            Some("application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml")
        }
        "ppt/tableStyles.xml" => {
            Some("application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml")
        }
        "docProps/core.xml" => Some("application/vnd.openxmlformats-package.core-properties+xml"),
        "docProps/app.xml" => {
            Some("application/vnd.openxmlformats-officedocument.extended-properties+xml")
        }
        _ if indexed_xml_part(name, "ppt/slides/slide") => Some(SLIDE_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/slideMasters/slideMaster") => {
            Some(SLIDE_MASTER_CONTENT_TYPE)
        }
        _ if indexed_xml_part(name, "ppt/slideLayouts/slideLayout") => {
            Some(SLIDE_LAYOUT_CONTENT_TYPE)
        }
        _ if indexed_xml_part(name, "ppt/notesSlides/notesSlide") => Some(NOTES_SLIDE_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/notesMasters/notesMaster") => {
            Some(NOTES_MASTER_CONTENT_TYPE)
        }
        _ if indexed_xml_part(name, "ppt/handoutMasters/handoutMaster") => {
            Some(HANDOUT_MASTER_CONTENT_TYPE)
        }
        _ if indexed_xml_part(name, "ppt/theme/theme") => Some(THEME_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/charts/chart") => Some(CHART_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/charts/style") => Some(CHART_STYLE_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/charts/colors") => Some(CHART_COLOR_STYLE_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/diagrams/data") => Some(DIAGRAM_DATA_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/diagrams/layout") => Some(DIAGRAM_LAYOUT_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/diagrams/quickStyle") => Some(DIAGRAM_STYLE_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/diagrams/colors") => Some(DIAGRAM_COLORS_CONTENT_TYPE),
        _ if indexed_xml_part(name, "ppt/comments/comment") => {
            Some("application/vnd.openxmlformats-officedocument.presentationml.comments+xml")
        }
        _ if name.starts_with("ppt/media/") && name.matches('/').count() == 2 => {
            match name.rsplit_once('.').map(|(_, extension)| extension) {
                Some("png") => Some("image/png"),
                Some("jpg" | "jpeg") => Some("image/jpeg"),
                Some("gif") => Some("image/gif"),
                Some("bmp") => Some("image/bmp"),
                _ => None,
            }
        }
        _ => None,
    }
}

fn indexed_xml_part(name: &str, prefix: &str) -> bool {
    let Some(index) = name
        .strip_prefix(prefix)
        .and_then(|suffix| suffix.strip_suffix(".xml"))
    else {
        return false;
    };
    if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    index
        .parse::<u32>()
        .is_ok_and(|value| value > 0 && value.to_string() == index)
}

fn parse_relationships(data: &[u8]) -> Result<BTreeMap<String, Relationship>, WorkerFailure> {
    let text =
        std::str::from_utf8(data).map_err(|_| zip_failure("PPTX relationships are not UTF-8"))?;
    let mut reader = Reader::from_str(text);
    let mut relationships = BTreeMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Relationship" =>
            {
                let id = attr_required(&event, "Id")?;
                let kind = attr_required(&event, "Type")?;
                let target = attr_required(&event, "Target")?;
                let target_mode = attr(&event, "TargetMode")?;
                let external = match target_mode.as_deref() {
                    None | Some("Internal") => false,
                    Some("External") => true,
                    Some(other) => {
                        return Err(zip_failure(format!(
                            "unsupported PPTX relationship TargetMode {other}"
                        )));
                    }
                };
                if !known_relationship_type(&kind) {
                    return Err(unsupported(format!(
                        "unknown PPTX relationship type {kind}"
                    )));
                }
                if external && !kind.ends_with("/hyperlink") {
                    return Err(unsupported(format!(
                        "external non-hyperlink PPTX relationship {kind}"
                    )));
                }
                if relationships
                    .insert(
                        id.clone(),
                        Relationship {
                            kind,
                            target,
                            external,
                        },
                    )
                    .is_some()
                {
                    return Err(zip_failure(format!("duplicate PPTX relationship id {id}")));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(zip_failure(format!("PPTX relationship XML: {error}")));
            }
        }
    }
    Ok(relationships)
}

fn known_relationship_type(value: &str) -> bool {
    if value
        == "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"
    {
        return true;
    }
    let Some(suffix) =
        value.strip_prefix("http://schemas.openxmlformats.org/officeDocument/2006/relationships/")
    else {
        return false;
    };
    matches!(
        suffix,
        "officeDocument"
            | "slide"
            | "slideMaster"
            | "slideLayout"
            | "theme"
            | "notesSlide"
            | "notesMaster"
            | "handoutMaster"
            | "image"
            | "hyperlink"
            | "chart"
            | "diagramData"
            | "diagramLayout"
            | "diagramQuickStyle"
            | "diagramColors"
            | "comments"
            | "commentAuthors"
            | "tableStyles"
            | "presProps"
            | "viewProps"
            | "extended-properties"
    )
}

fn source_part_for_rels(name: &str) -> Result<String, WorkerFailure> {
    if name == "_rels/.rels" {
        return Ok(String::new());
    }
    let Some((directory, file)) = name.rsplit_once("/_rels/") else {
        return Err(zip_failure(format!(
            "invalid PPTX relationship part path {name}"
        )));
    };
    let Some(file) = file.strip_suffix(".rels") else {
        return Err(zip_failure(format!(
            "invalid PPTX relationship part suffix {name}"
        )));
    };
    if file.is_empty() || file.contains('/') {
        return Err(zip_failure(format!(
            "invalid PPTX relationship part path {name}"
        )));
    }
    Ok(format!("{directory}/{file}"))
}

fn attr_required(event: &BytesStart<'_>, name: &str) -> Result<String, WorkerFailure> {
    attr(event, name)?.ok_or_else(|| {
        zip_failure(format!(
            "PPTX XML element is missing required attribute {name}"
        ))
    })
}

fn attr(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, WorkerFailure> {
    let mut matched = None;
    for item in event.attributes() {
        let item =
            item.map_err(|error| zip_failure(format!("invalid PPTX XML attribute: {error}")))?;
        let key = item.key.as_ref();
        let local = key.rsplit(':').next().unwrap_or(key);
        if local == name {
            if matched.is_some() {
                return Err(zip_failure(format!(
                    "ambiguous PPTX XML attribute local name {name}"
                )));
            }
            matched = Some(
                quick_xml::escape::unescape(item.value.as_ref())
                    .map_err(|error| {
                        zip_failure(format!("PPTX XML attribute decode failed: {error}"))
                    })?
                    .into_owned(),
            );
        }
    }
    Ok(matched)
}

fn validate_zip_extra_fields(input: &[u8], start: usize, end: usize) -> Result<(), WorkerFailure> {
    let mut offset = start;
    while offset < end {
        let header_end = offset
            .checked_add(4)
            .ok_or_else(|| zip_failure("PPTX ZIP extra-field offset overflow"))?;
        let header = input
            .get(offset..header_end)
            .filter(|_| header_end <= end)
            .ok_or_else(|| zip_failure("truncated PPTX ZIP extra-field header"))?;
        let field_id = u16::from_le_bytes([header[0], header[1]]);
        let field_len = usize::from(u16::from_le_bytes([header[2], header[3]]));
        if field_id == 0x0001 {
            return Err(zip_failure("PPTX ZIP64 extra fields are unsupported"));
        }
        if field_id == 0x7075 {
            return Err(zip_failure(
                "PPTX ZIP Unicode Path extra fields are unsupported",
            ));
        }
        offset = header_end
            .checked_add(field_len)
            .filter(|value| *value <= end)
            .ok_or_else(|| zip_failure("truncated PPTX ZIP extra-field value"))?;
    }
    Ok(())
}

fn looks_like_zip(input: &[u8]) -> bool {
    input.starts_with(b"PK\x03\x04")
        || input.starts_with(b"PK\x01\x02")
        || input.starts_with(b"PK\x05\x06")
}

fn read_u16(input: &[u8], offset: usize) -> Option<u16> {
    let bytes = input.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(input: &[u8], offset: usize) -> Option<u32> {
    let bytes = input.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn zip_failure(message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::new(WorkerFailureCode::SemanticExtractionFailed, message)
}

fn unsupported(message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::new(WorkerFailureCode::UnsupportedSemanticConstruct, message)
}

fn resource_limit() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::InspectionResourceLimitExceeded,
        "PPTX package exceeded a configured resource limit",
    )
}
