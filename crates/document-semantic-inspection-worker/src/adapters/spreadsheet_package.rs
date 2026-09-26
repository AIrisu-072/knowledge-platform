use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Cursor, Read};

use quick_xml::{
    NsReader,
    events::{BytesStart, Event},
    name::ResolveResult,
};
use zip::ZipArchive;

use crate::{WorkerFailure, WorkerFailureCode};

use super::AdapterProfile;

const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 256;
const MAX_XML_NODES: usize = 2_000_000;
const MAX_SHEETS: usize = 1_024;
const MAX_CELLS: usize = 1_000_000;
// Keep this pre-parser bound aligned with the DSI_V0 adapter image limit.
const MAX_SPREADSHEET_IMAGES: usize = 4_096;

const PACKAGE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
const PACKAGE_CONTENT_TYPES_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/content-types";
const SPREADSHEET_DRAWING_NS: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const OFFICE_RELATIONSHIPS_PREFIX: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

#[derive(Debug)]
struct ZipPreflight {
    archive_offset: u64,
    central_directory_start: u64,
    entries: Vec<ZipEntry>,
}

#[derive(Debug)]
struct ZipEntry {
    name: String,
    raw_name: Vec<u8>,
    directory: bool,
    method: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
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

#[derive(Debug, Default)]
struct ContentTypes {
    defaults: BTreeMap<String, String>,
    overrides: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
struct PackageCoverage {
    part_names: BTreeMap<String, String>,
    relationships: BTreeMap<String, BTreeSet<String>>,
    content_types: Option<ContentTypes>,
    office_document_count: usize,
}

/// Performs bounded ZIP, XML, content-type, and relationship preflight for an
/// XLSX/XLSM package. External relationship targets are recorded only as
/// strings and are never opened or dereferenced.
pub(super) fn preflight_spreadsheet_package(
    input: &[u8],
    _profile: &AdapterProfile,
) -> Result<(), WorkerFailure> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(resource_limit(
            "spreadsheet input exceeds the DSI_V0 byte limit",
        ));
    }

    let preflight = preflight_zip(input)?;
    let mut archive = ZipArchive::new(Cursor::new(input))
        .map_err(|_| semantic_failure("invalid SpreadsheetML ZIP package"))?;
    if archive.offset() != preflight.archive_offset
        || archive.central_directory_start() != preflight.central_directory_start
        || archive.len() != preflight.entries.len()
    {
        return Err(semantic_failure(
            "ZIP preflight disagrees with parsed archive metadata",
        ));
    }

    let mut coverage = PackageCoverage::default();
    let mut actual_total = 0u64;
    let mut xml_nodes_total = 0usize;
    let mut sheet_count = 0usize;
    let mut cell_count = 0usize;
    let mut picture_count = 0usize;

    for (index, metadata) in preflight.entries.iter().enumerate() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| semantic_failure("invalid SpreadsheetML ZIP entry"))?;
        if entry.name_raw() != metadata.raw_name.as_slice()
            || entry.size() != metadata.uncompressed_size
            || entry.compressed_size() != metadata.compressed_size
            || entry.crc32() != metadata.crc32
            || !matches!(
                (entry.compression(), metadata.method),
                (zip::CompressionMethod::Stored, 0) | (zip::CompressionMethod::Deflated, 8)
            )
            || entry.is_dir() != metadata.directory
        {
            return Err(semantic_failure(
                "ZIP reader disagrees with validated central-directory metadata",
            ));
        }

        if metadata.directory {
            drain_entry(&mut entry, metadata.uncompressed_size, &mut actual_total)?;
            continue;
        }

        let part_key = metadata.name.to_ascii_lowercase();
        if coverage
            .part_names
            .insert(part_key, metadata.name.clone())
            .is_some()
        {
            return Err(unsupported(
                "duplicate or case-aliased SpreadsheetML package part",
            ));
        }

        if is_xml_part(&metadata.name) {
            let bytes = read_xml_entry(&mut entry, metadata.uncompressed_size, &mut actual_total)?;
            let (nodes, sheets, cells) =
                validate_xml_part(&metadata.name, &bytes, &mut picture_count)?;
            xml_nodes_total = xml_nodes_total
                .checked_add(nodes)
                .ok_or_else(|| resource_limit("SpreadsheetML XML node count overflow"))?;
            if xml_nodes_total > MAX_XML_NODES {
                return Err(resource_limit(
                    "SpreadsheetML package exceeds the XML node limit",
                ));
            }
            sheet_count = sheet_count
                .checked_add(sheets)
                .ok_or_else(|| resource_limit("SpreadsheetML sheet count overflow"))?;
            if sheet_count > MAX_SHEETS {
                return Err(resource_limit(
                    "SpreadsheetML package exceeds the worksheet limit",
                ));
            }
            cell_count = cell_count
                .checked_add(cells)
                .ok_or_else(|| resource_limit("SpreadsheetML cell count overflow"))?;
            if cell_count > MAX_CELLS {
                return Err(resource_limit(
                    "SpreadsheetML package exceeds the cell limit",
                ));
            }

            if metadata.name == "[Content_Types].xml" {
                if coverage.content_types.is_some() {
                    return Err(unsupported(
                        "SpreadsheetML package contains aliased content-type manifests",
                    ));
                }
                coverage.content_types = Some(parse_content_types(&bytes)?);
            } else if metadata.name.ends_with(".rels") {
                let source = source_part_for_relationships(&metadata.name)?;
                validate_relationships(&metadata.name, &source, &bytes, &mut coverage)?;
            }
        } else {
            drain_entry(&mut entry, metadata.uncompressed_size, &mut actual_total)?;
        }

        if actual_total > MAX_TOTAL_UNCOMPRESSED_BYTES {
            return Err(resource_limit(
                "SpreadsheetML package exceeds the actual decompressed-size limit",
            ));
        }
    }

    validate_package_coverage(&coverage)?;
    Ok(())
}

fn read_xml_entry<R: Read>(
    entry: &mut R,
    declared_size: u64,
    actual_total: &mut u64,
) -> Result<Vec<u8>, WorkerFailure> {
    if declared_size > MAX_ENTRY_BYTES {
        return Err(resource_limit(
            "SpreadsheetML XML entry exceeds the per-entry byte limit",
        ));
    }
    let capacity = usize::try_from(declared_size)
        .map_err(|_| resource_limit("SpreadsheetML entry size is out of range"))?;
    let mut bytes = Vec::with_capacity(capacity);
    entry
        .take(MAX_ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| semantic_failure("cannot decompress SpreadsheetML XML part"))?;
    let actual = u64::try_from(bytes.len())
        .map_err(|_| resource_limit("SpreadsheetML entry size is out of range"))?;
    if actual > MAX_ENTRY_BYTES {
        return Err(resource_limit(
            "SpreadsheetML XML entry exceeds the per-entry byte limit",
        ));
    }
    if actual != declared_size {
        return Err(semantic_failure(
            "ZIP size metadata disagrees with decompressed SpreadsheetML XML size",
        ));
    }
    *actual_total = actual_total
        .checked_add(actual)
        .ok_or_else(|| resource_limit("SpreadsheetML decompressed-size total overflow"))?;
    if *actual_total > MAX_TOTAL_UNCOMPRESSED_BYTES {
        return Err(resource_limit(
            "SpreadsheetML package exceeds the actual decompressed-size limit",
        ));
    }
    Ok(bytes)
}

fn drain_entry<R: Read>(
    entry: &mut R,
    declared_size: u64,
    actual_total: &mut u64,
) -> Result<(), WorkerFailure> {
    if declared_size > MAX_ENTRY_BYTES {
        return Err(resource_limit(
            "SpreadsheetML entry exceeds the per-entry byte limit",
        ));
    }
    let mut actual = 0u64;
    let mut buffer = [0u8; 32 * 1024];
    loop {
        let read = entry
            .read(&mut buffer)
            .map_err(|_| semantic_failure("cannot decompress SpreadsheetML package entry"))?;
        if read == 0 {
            break;
        }
        actual = actual
            .checked_add(read as u64)
            .ok_or_else(|| resource_limit("SpreadsheetML entry size overflow"))?;
        if actual > MAX_ENTRY_BYTES {
            return Err(resource_limit(
                "SpreadsheetML entry exceeds the per-entry byte limit",
            ));
        }
        *actual_total = actual_total
            .checked_add(read as u64)
            .ok_or_else(|| resource_limit("SpreadsheetML decompressed-size total overflow"))?;
        if *actual_total > MAX_TOTAL_UNCOMPRESSED_BYTES {
            return Err(resource_limit(
                "SpreadsheetML package exceeds the actual decompressed-size limit",
            ));
        }
    }
    if actual != declared_size {
        return Err(semantic_failure(
            "ZIP size metadata disagrees with actual SpreadsheetML decompressed size",
        ));
    }
    Ok(())
}

fn is_xml_part(name: &str) -> bool {
    name == "[Content_Types].xml" || name.ends_with(".xml") || name.ends_with(".rels")
}

fn validate_xml_part(
    name: &str,
    bytes: &[u8],
    picture_count: &mut usize,
) -> Result<(usize, usize, usize), WorkerFailure> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| semantic_failure("SpreadsheetML XML is not UTF-8"))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let drawing_part = numbered_path(name, "xl/drawings/drawing", ".xml");
    let mut drawing_root = false;
    let mut drawing_anchor_depths = Vec::new();
    let mut depth = 0usize;
    let mut roots = 0usize;
    let mut nodes = 0usize;
    let mut sheets = 0usize;
    let mut cells = 0usize;

    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| semantic_failure("SpreadsheetML XML parse failed"))?;
        if !matches!(&event, Event::End(_) | Event::Eof) {
            nodes = nodes
                .checked_add(1)
                .ok_or_else(|| resource_limit("SpreadsheetML XML node count overflow"))?;
            if nodes > MAX_XML_NODES {
                return Err(resource_limit(
                    "SpreadsheetML XML part exceeds the XML node limit",
                ));
            }
        }
        match event {
            Event::Start(element) => {
                if depth == 0 {
                    roots += 1;
                    drawing_root = drawing_part
                        && element.local_name().as_ref() == "wsDr"
                        && has_namespace(&namespace, SPREADSHEET_DRAWING_NS);
                }
                if drawing_root && has_namespace(&namespace, SPREADSHEET_DRAWING_NS) {
                    let local_name = element.local_name();
                    if matches!(
                        local_name.as_ref(),
                        "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor"
                    ) {
                        if !drawing_anchor_depths.is_empty() || depth == 1 {
                            drawing_anchor_depths.push(depth + 1);
                        }
                    } else if local_name.as_ref() == "pic" && drawing_anchor_depths.len() == 1 {
                        count_spreadsheet_picture(picture_count)?;
                    }
                }
                count_spreadsheet_element(
                    name,
                    element.local_name().as_ref(),
                    &mut sheets,
                    &mut cells,
                )?;
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| resource_limit("SpreadsheetML XML depth overflow"))?;
                if depth > MAX_XML_DEPTH {
                    return Err(resource_limit(
                        "SpreadsheetML XML part exceeds the XML depth limit",
                    ));
                }
            }
            Event::Empty(element) => {
                if depth == 0 {
                    roots += 1;
                    drawing_root = drawing_part
                        && element.local_name().as_ref() == "wsDr"
                        && has_namespace(&namespace, SPREADSHEET_DRAWING_NS);
                }
                if drawing_root
                    && drawing_anchor_depths.len() == 1
                    && has_namespace(&namespace, SPREADSHEET_DRAWING_NS)
                    && element.local_name().as_ref() == "pic"
                {
                    count_spreadsheet_picture(picture_count)?;
                }
                count_spreadsheet_element(
                    name,
                    element.local_name().as_ref(),
                    &mut sheets,
                    &mut cells,
                )?;
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(semantic_failure(
                        "SpreadsheetML XML has an unmatched closing element",
                    ));
                }
                if drawing_root && drawing_anchor_depths.last() == Some(&depth) {
                    drawing_anchor_depths.pop();
                }
                depth -= 1;
            }
            Event::DocType(_) => {
                return Err(semantic_failure(
                    "SpreadsheetML XML document type declarations are unsupported",
                ));
            }
            Event::Text(value) if depth == 0 => {
                let decoded = quick_xml::escape::unescape(value.as_ref())
                    .map_err(|_| semantic_failure("SpreadsheetML XML text decode failed"))?;
                if !decoded.trim().is_empty() {
                    return Err(semantic_failure(
                        "SpreadsheetML XML contains text outside its root element",
                    ));
                }
            }
            Event::CData(_) if depth == 0 => {
                return Err(semantic_failure(
                    "SpreadsheetML XML contains CDATA outside its root element",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if roots != 1 || depth != 0 {
        return Err(semantic_failure(
            "SpreadsheetML XML must contain exactly one complete root element",
        ));
    }
    Ok((nodes, sheets, cells))
}

fn count_spreadsheet_picture(picture_count: &mut usize) -> Result<(), WorkerFailure> {
    *picture_count = picture_count
        .checked_add(1)
        .ok_or_else(|| resource_limit("SpreadsheetML image count overflow"))?;
    if *picture_count > MAX_SPREADSHEET_IMAGES {
        return Err(resource_limit(
            "SpreadsheetML image count exceeds the limit",
        ));
    }
    Ok(())
}

fn count_spreadsheet_element(
    part_name: &str,
    local_name: &str,
    sheets: &mut usize,
    cells: &mut usize,
) -> Result<(), WorkerFailure> {
    if part_name == "xl/workbook.xml" && local_name == "sheet" {
        *sheets = sheets
            .checked_add(1)
            .ok_or_else(|| resource_limit("SpreadsheetML sheet count overflow"))?;
    } else if numbered_path(part_name, "xl/worksheets/sheet", ".xml") && local_name == "c" {
        *cells = cells
            .checked_add(1)
            .ok_or_else(|| resource_limit("SpreadsheetML cell count overflow"))?;
    }
    Ok(())
}

fn parse_content_types(bytes: &[u8]) -> Result<ContentTypes, WorkerFailure> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| semantic_failure("[Content_Types].xml is not UTF-8"))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut result = ContentTypes::default();
    let mut depth = 0usize;
    let mut saw_root = false;
    let mut root_closed = false;

    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| semantic_failure("content-types XML parse failed"))?;
        match event {
            Event::Start(event) => {
                if depth == 0 {
                    if saw_root
                        || event.local_name().as_ref() != "Types"
                        || !has_namespace(&namespace, PACKAGE_CONTENT_TYPES_NS)
                    {
                        return Err(semantic_failure(
                            "[Content_Types].xml must have one Types root",
                        ));
                    }
                    validate_attributes(&event, &[])?;
                    saw_root = true;
                } else if depth == 1 {
                    require_namespace(&namespace, PACKAGE_CONTENT_TYPES_NS)?;
                    process_content_type_entry(event.local_name().as_ref(), &event, &mut result)?;
                } else {
                    return Err(unsupported(
                        "[Content_Types].xml contains an unsupported nested element",
                    ));
                }
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| resource_limit("content-types XML depth overflow"))?;
            }
            Event::Empty(event) => {
                if depth == 0 {
                    if saw_root
                        || event.local_name().as_ref() != "Types"
                        || !has_namespace(&namespace, PACKAGE_CONTENT_TYPES_NS)
                    {
                        return Err(semantic_failure(
                            "[Content_Types].xml must have one Types root",
                        ));
                    }
                    validate_attributes(&event, &[])?;
                    saw_root = true;
                    root_closed = true;
                } else if depth == 1 {
                    require_namespace(&namespace, PACKAGE_CONTENT_TYPES_NS)?;
                    process_content_type_entry(event.local_name().as_ref(), &event, &mut result)?;
                } else {
                    return Err(unsupported(
                        "[Content_Types].xml contains an unsupported nested element",
                    ));
                }
            }
            Event::End(event) => {
                if depth == 0 {
                    return Err(semantic_failure(
                        "[Content_Types].xml has an unmatched closing element",
                    ));
                }
                depth -= 1;
                if depth == 0 {
                    if event.local_name().as_ref() != "Types"
                        || !has_namespace(&namespace, PACKAGE_CONTENT_TYPES_NS)
                    {
                        return Err(semantic_failure(
                            "[Content_Types].xml closed a non-Types root",
                        ));
                    }
                    root_closed = true;
                }
            }
            Event::Text(value) => {
                if !is_xml_whitespace(value.as_ref())? {
                    return Err(semantic_failure(
                        "[Content_Types].xml contains unsupported text content",
                    ));
                }
            }
            Event::CData(value) => {
                if !is_xml_whitespace(value.as_ref())? {
                    return Err(semantic_failure(
                        "[Content_Types].xml contains unsupported CDATA content",
                    ));
                }
            }
            Event::DocType(_) => {
                return Err(semantic_failure(
                    "[Content_Types].xml must not contain a document type declaration",
                ));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) => {}
            Event::PI(_) => {
                return Err(unsupported(
                    "[Content_Types].xml contains an unsupported processing instruction",
                ));
            }
            _ => {}
        }
    }
    if !saw_root || !root_closed || depth != 0 {
        return Err(semantic_failure(
            "[Content_Types].xml is incomplete or has no Types root",
        ));
    }
    Ok(result)
}

fn process_content_type_entry(
    element: &str,
    event: &BytesStart<'_>,
    result: &mut ContentTypes,
) -> Result<(), WorkerFailure> {
    if element == "Default" {
        let attributes = validate_attributes(event, &["Extension", "ContentType"])?;
        let extension = required_attribute(&attributes, "Extension", "content-type Default")?
            .to_ascii_lowercase();
        let content_type =
            required_attribute(&attributes, "ContentType", "content-type Default")?.to_owned();
        if extension.is_empty()
            || !extension
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(semantic_failure(
                "content-type Default has a non-canonical extension",
            ));
        }
        ensure_known_content_type(&content_type)?;
        if result.defaults.insert(extension, content_type).is_some() {
            return Err(semantic_failure(
                "content-types XML contains a duplicate Default declaration",
            ));
        }
        return Ok(());
    }
    if element == "Override" {
        let attributes = validate_attributes(event, &["PartName", "ContentType"])?;
        let part_name = required_attribute(&attributes, "PartName", "content-type Override")?;
        let part_name = part_name.strip_prefix('/').ok_or_else(|| {
            semantic_failure("content-type Override PartName must begin with a slash")
        })?;
        validate_part_name(part_name)?;
        let content_type =
            required_attribute(&attributes, "ContentType", "content-type Override")?.to_owned();
        ensure_known_content_type(&content_type)?;
        let key = part_name.to_ascii_lowercase();
        if result.overrides.insert(key, content_type).is_some() {
            return Err(semantic_failure(
                "content-types XML contains a duplicate Override declaration",
            ));
        }
        return Ok(());
    }
    Err(unsupported(
        "[Content_Types].xml contains an unknown declaration element",
    ))
}

fn validate_relationships(
    _name: &str,
    source: &str,
    bytes: &[u8],
    coverage: &mut PackageCoverage,
) -> Result<(), WorkerFailure> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| semantic_failure("relationships XML is not UTF-8"))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut depth = 0usize;
    let mut saw_root = false;
    let mut root_closed = false;
    let mut ids = BTreeSet::new();

    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| semantic_failure("relationships XML parse failed"))?;
        match event {
            Event::Start(event) => {
                if depth == 0 {
                    if saw_root
                        || event.local_name().as_ref() != "Relationships"
                        || !has_namespace(&namespace, PACKAGE_RELATIONSHIPS_NS)
                    {
                        return Err(semantic_failure(
                            "relationships XML must have one Relationships root",
                        ));
                    }
                    validate_attributes(&event, &[])?;
                    saw_root = true;
                } else if depth == 1 {
                    require_namespace(&namespace, PACKAGE_RELATIONSHIPS_NS)?;
                    process_relationship(
                        event.local_name().as_ref(),
                        &event,
                        source,
                        coverage,
                        &mut ids,
                    )?;
                } else {
                    return Err(unsupported(
                        "relationships XML contains an unsupported nested element",
                    ));
                }
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| resource_limit("relationship XML depth overflow"))?;
            }
            Event::Empty(event) => {
                if depth == 0 {
                    if saw_root
                        || event.local_name().as_ref() != "Relationships"
                        || !has_namespace(&namespace, PACKAGE_RELATIONSHIPS_NS)
                    {
                        return Err(semantic_failure(
                            "relationships XML must have one Relationships root",
                        ));
                    }
                    validate_attributes(&event, &[])?;
                    saw_root = true;
                    root_closed = true;
                } else if depth == 1 {
                    require_namespace(&namespace, PACKAGE_RELATIONSHIPS_NS)?;
                    process_relationship(
                        event.local_name().as_ref(),
                        &event,
                        source,
                        coverage,
                        &mut ids,
                    )?;
                } else {
                    return Err(unsupported(
                        "relationships XML contains an unsupported nested element",
                    ));
                }
            }
            Event::End(event) => {
                if depth == 0 {
                    return Err(semantic_failure(
                        "relationships XML has an unmatched closing element",
                    ));
                }
                depth -= 1;
                if depth == 0 {
                    if event.local_name().as_ref() != "Relationships"
                        || !has_namespace(&namespace, PACKAGE_RELATIONSHIPS_NS)
                    {
                        return Err(semantic_failure(
                            "relationships XML closed a non-Relationships root",
                        ));
                    }
                    root_closed = true;
                }
            }
            Event::Text(value) => {
                if !is_xml_whitespace(value.as_ref())? {
                    return Err(semantic_failure(
                        "relationships XML contains unsupported text content",
                    ));
                }
            }
            Event::CData(value) => {
                if !is_xml_whitespace(value.as_ref())? {
                    return Err(semantic_failure(
                        "relationships XML contains unsupported CDATA content",
                    ));
                }
            }
            Event::DocType(_) => {
                return Err(semantic_failure(
                    "relationships XML must not contain a document type declaration",
                ));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) => {}
            Event::PI(_) => {
                return Err(unsupported(
                    "relationships XML contains an unsupported processing instruction",
                ));
            }
            _ => {}
        }
    }

    if !saw_root || !root_closed || depth != 0 {
        return Err(semantic_failure(
            "relationships XML is incomplete or has no Relationships root",
        ));
    }
    Ok(())
}

fn process_relationship(
    element: &str,
    event: &BytesStart<'_>,
    source: &str,
    coverage: &mut PackageCoverage,
    ids: &mut BTreeSet<String>,
) -> Result<(), WorkerFailure> {
    if element != "Relationship" {
        return Err(unsupported("relationships XML contains an unknown element"));
    }
    let attributes = validate_attributes(event, &["Id", "Type", "Target", "TargetMode"])?;
    let id = required_attribute(&attributes, "Id", "")?;
    if id.is_empty()
        || id
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(semantic_failure(
            "relationships XML contains an invalid relationship Id",
        ));
    }
    if !ids.insert(id.to_owned()) {
        return Err(semantic_failure(
            "relationships XML contains duplicate relationship Ids",
        ));
    }
    let kind = required_attribute(&attributes, "Type", "")?;
    if !known_relationship_type(kind) {
        return Err(unsupported(
            "relationships XML contains an unknown relationship type",
        ));
    }
    let target = required_attribute(&attributes, "Target", "")?;
    let external = match attributes.get("TargetMode").map(String::as_str) {
        None | Some("Internal") => false,
        Some("External") => true,
        Some(_) => {
            return Err(semantic_failure(
                "relationships XML contains an invalid TargetMode",
            ));
        }
    };
    if external {
        if !matches!(
            relationship_suffix(kind),
            Some("hyperlink" | "externalLinkPath")
        ) {
            return Err(unsupported(
                "external relationship is not a supported hyperlink or external workbook definition",
            ));
        }
    } else {
        let resolved = resolve_internal_target(source, target)?;
        coverage
            .relationships
            .entry(source.to_owned())
            .or_default()
            .insert(resolved.clone());
        if kind.ends_with("/officeDocument") {
            coverage.office_document_count += 1;
            if !source.is_empty() || resolved != "xl/workbook.xml" {
                return Err(semantic_failure(
                    "officeDocument relationship must target xl/workbook.xml",
                ));
            }
        }
    }
    Ok(())
}

fn validate_attributes(
    event: &BytesStart<'_>,
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, WorkerFailure> {
    let mut attributes = BTreeMap::new();
    for item in event.attributes().with_checks(true) {
        let item = item.map_err(|_| semantic_failure("invalid SpreadsheetML XML attribute"))?;
        let key = item.key.as_ref();
        let raw_value = item.value.as_ref();
        let value = quick_xml::escape::unescape(raw_value)
            .map_err(|_| semantic_failure("SpreadsheetML XML attribute decode failed"))?
            .into_owned();
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        if key.contains(':') {
            return Err(unsupported(
                "prefixed SpreadsheetML XML attributes are unsupported",
            ));
        }
        if !allowed.contains(&key) {
            return Err(unsupported(
                "SpreadsheetML XML contains an unexpected attribute",
            ));
        }
        if attributes.insert(key.to_owned(), value).is_some() {
            return Err(semantic_failure(
                "SpreadsheetML XML contains a duplicate attribute",
            ));
        }
    }
    Ok(attributes)
}

fn has_namespace(namespace: &ResolveResult<'_>, expected: &str) -> bool {
    matches!(namespace, ResolveResult::Bound(value) if value.as_ref() == expected)
}

fn require_namespace(namespace: &ResolveResult<'_>, expected: &str) -> Result<(), WorkerFailure> {
    if has_namespace(namespace, expected) {
        Ok(())
    } else {
        Err(unsupported(
            "SpreadsheetML XML element has an unexpected namespace",
        ))
    }
}

fn required_attribute<'a>(
    attributes: &'a BTreeMap<String, String>,
    name: &str,
    _context: &str,
) -> Result<&'a str, WorkerFailure> {
    attributes
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            semantic_failure("SpreadsheetML XML element is missing a required attribute")
        })
}

fn is_xml_whitespace(value: &str) -> Result<bool, WorkerFailure> {
    let value = quick_xml::escape::unescape(value)
        .map_err(|_| semantic_failure("SpreadsheetML XML text decode failed"))?;
    Ok(value.chars().all(char::is_whitespace))
}

fn source_part_for_relationships(name: &str) -> Result<String, WorkerFailure> {
    if name == "_rels/.rels" {
        return Ok(String::new());
    }
    let (directory, tail) = if let Some(tail) = name.strip_prefix("_rels/") {
        ("", tail)
    } else if let Some((directory, tail)) = name.split_once("/_rels/") {
        (directory, tail)
    } else {
        return Err(unsupported(
            "malformed SpreadsheetML relationships part path",
        ));
    };
    let source_file = tail
        .strip_suffix(".rels")
        .ok_or_else(|| unsupported("malformed SpreadsheetML relationships part path"))?;
    if source_file.is_empty() || source_file.contains('/') {
        return Err(unsupported(
            "malformed SpreadsheetML relationships part path",
        ));
    }
    let source = if directory.is_empty() {
        source_file.to_owned()
    } else {
        format!("{directory}/{source_file}")
    };
    validate_part_name(&source)?;
    Ok(source)
}

fn resolve_internal_target(source: &str, target: &str) -> Result<String, WorkerFailure> {
    if target.is_empty()
        || target.contains(['\\', '%', '?', '#', ':'])
        || target.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(unsupported(
            "unsafe or non-canonical internal SpreadsheetML relationship target",
        ));
    }
    let absolute = target.starts_with('/');
    if target.starts_with("//") {
        return Err(unsupported(
            "internal SpreadsheetML relationship target has an ambiguous root",
        ));
    }
    let target = if absolute { &target[1..] } else { target };
    let mut segments = Vec::<&str>::new();
    if !absolute && let Some((directory, _)) = source.rsplit_once('/') {
        segments.extend(directory.split('/'));
    }
    if target.is_empty() {
        return Err(unsupported(
            "internal SpreadsheetML relationship target is empty",
        ));
    }
    for segment in target.split('/') {
        match segment {
            "" => {
                return Err(unsupported(
                    "internal SpreadsheetML relationship target contains an empty segment",
                ));
            }
            "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(unsupported(
                        "internal SpreadsheetML relationship target escapes the package root",
                    ));
                }
            }
            value => segments.push(value),
        }
    }
    let resolved = segments.join("/");
    validate_part_name(&resolved)?;
    Ok(resolved)
}

fn known_content_type(value: &str) -> bool {
    matches!(
        value,
        "application/vnd.openxmlformats-package.relationships+xml"
            | "application/xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
            | "application/vnd.ms-excel.sheet.macroEnabled.main+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.externalLink+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml"
            | "application/vnd.openxmlformats-officedocument.drawing+xml"
            | "application/vnd.openxmlformats-officedocument.drawingml.chart+xml"
            | "application/vnd.openxmlformats-officedocument.theme+xml"
            | "application/vnd.openxmlformats-package.core-properties+xml"
            | "application/vnd.openxmlformats-officedocument.extended-properties+xml"
            | "application/vnd.ms-office.vbaProject"
            | "application/vnd.ms-excel.controlproperties+xml"
            | "application/vnd.ms-excel.printerSettings"
            | "image/png"
            | "image/jpeg"
    )
}

fn ensure_known_content_type(value: &str) -> Result<(), WorkerFailure> {
    if known_content_type(value) {
        Ok(())
    } else {
        Err(unsupported("unknown SpreadsheetML content type"))
    }
}

fn known_relationship_type(value: &str) -> bool {
    if matches!(
        value,
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"
            | "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties"
            | "http://schemas.microsoft.com/office/2006/relationships/vbaProject"
    ) {
        return true;
    }
    relationship_suffix(value).is_some()
}

fn relationship_suffix(value: &str) -> Option<&str> {
    const KNOWN_SUFFIXES: &[&str] = &[
        "officeDocument",
        "worksheet",
        "styles",
        "sharedStrings",
        "theme",
        "hyperlink",
        "drawing",
        "image",
        "chart",
        "table",
        "comments",
        "vbaProject",
        "calcChain",
        "externalLink",
        "externalLinkPath",
        "connections",
        "printerSettings",
        "pivotCacheDefinition",
        "pivotCacheRecords",
        "pivotTable",
        "control",
        "ctrlProp",
        "legacyDrawing",
    ];
    value
        .strip_prefix(OFFICE_RELATIONSHIPS_PREFIX)
        .filter(|suffix| KNOWN_SUFFIXES.contains(suffix))
}

fn validate_package_coverage(coverage: &PackageCoverage) -> Result<(), WorkerFailure> {
    let content_types = coverage
        .content_types
        .as_ref()
        .ok_or_else(|| semantic_failure("SpreadsheetML package is missing [Content_Types].xml"))?;
    require_part(coverage, "[Content_Types].xml")?;
    require_part(coverage, "_rels/.rels")?;
    require_part(coverage, "xl/workbook.xml")?;
    if coverage.office_document_count != 1 {
        return Err(semantic_failure(
            "SpreadsheetML package must have exactly one officeDocument relationship",
        ));
    }

    for (part_key, content_type) in &content_types.overrides {
        if !coverage.part_names.contains_key(part_key) {
            return Err(semantic_failure(
                "content-type Override references a missing SpreadsheetML part",
            ));
        }
        ensure_known_content_type(content_type)?;
    }

    let mut workbook_main_type = None;
    for actual_name in coverage.part_names.values() {
        if actual_name == "[Content_Types].xml" {
            continue;
        }
        validate_known_part_path(actual_name, coverage)?;
        let content_type = lookup_content_type(actual_name, content_types).ok_or_else(|| {
            semantic_failure("SpreadsheetML package part has no declared content type")
        })?;
        validate_part_content_type(actual_name, content_type)?;
        if actual_name == "xl/workbook.xml" {
            workbook_main_type = Some(content_type);
        }
    }

    let workbook_main_type = workbook_main_type.ok_or_else(|| {
        semantic_failure("SpreadsheetML workbook part has no declared main content type")
    })?;
    let macro_enabled =
        workbook_main_type == "application/vnd.ms-excel.sheet.macroEnabled.main+xml";
    if !matches!(
        workbook_main_type,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
            | "application/vnd.ms-excel.sheet.macroEnabled.main+xml"
    ) {
        return Err(unsupported(
            "SpreadsheetML workbook has an unsupported main content type",
        ));
    }

    if has_part(coverage, "xl/vbaProject.bin") {
        if !macro_enabled {
            return Err(semantic_failure(
                "XLSX package contains a VBA project without a macro-enabled workbook type",
            ));
        }
        if !coverage
            .relationships
            .get("xl/workbook.xml")
            .is_some_and(|targets| targets.contains("xl/vbaProject.bin"))
        {
            return Err(semantic_failure(
                "VBA project part is not related from the workbook",
            ));
        }
    }

    for (source, targets) in &coverage.relationships {
        if !source.is_empty() && !has_part(coverage, source) {
            return Err(semantic_failure(
                "SpreadsheetML relationship source part is missing",
            ));
        }
        for target in targets {
            if !has_part(coverage, target) {
                return Err(semantic_failure(
                    "SpreadsheetML relationship target part is missing",
                ));
            }
        }
    }
    validate_relationship_graph(&coverage.relationships)?;
    Ok(())
}

fn validate_known_part_path(name: &str, coverage: &PackageCoverage) -> Result<(), WorkerFailure> {
    if name == "_rels/.rels" {
        return Ok(());
    }
    if name.ends_with(".rels") {
        let source = source_part_for_relationships(name)?;
        if source.is_empty() || !has_part(coverage, &source) {
            return Err(semantic_failure(
                "SpreadsheetML relationships part has no source part",
            ));
        }
        if !is_known_non_relationship_part(&source) {
            return Err(unsupported(
                "SpreadsheetML relationships part belongs to an unknown package part",
            ));
        }
        return Ok(());
    }
    if is_known_non_relationship_part(name) {
        Ok(())
    } else {
        Err(unsupported(
            "SpreadsheetML package contains an unknown semantic part",
        ))
    }
}

fn is_known_non_relationship_part(name: &str) -> bool {
    matches!(
        name,
        "[Content_Types].xml"
            | "docProps/core.xml"
            | "docProps/app.xml"
            | "xl/workbook.xml"
            | "xl/styles.xml"
            | "xl/sharedStrings.xml"
            | "xl/calcChain.xml"
            | "xl/connections.xml"
            | "xl/vbaProject.bin"
    ) || numbered_path(name, "xl/worksheets/sheet", ".xml")
        || numbered_path(name, "xl/tables/table", ".xml")
        || numbered_path(name, "xl/comments", ".xml")
        || numbered_path(name, "xl/externalLinks/externalLink", ".xml")
        || numbered_path(name, "xl/drawings/drawing", ".xml")
        || numbered_path(name, "xl/charts/chart", ".xml")
        || numbered_path(name, "xl/theme/theme", ".xml")
        || numbered_path(name, "xl/printerSettings/printerSettings", ".bin")
        || numbered_path(name, "xl/ctrlProps/ctrlProp", ".xml")
        || valid_media_path(name)
}

fn numbered_path(name: &str, prefix: &str, suffix: &str) -> bool {
    let Some(number) = name
        .strip_prefix(prefix)
        .and_then(|tail| tail.strip_suffix(suffix))
    else {
        return false;
    };
    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_media_path(name: &str) -> bool {
    let Some(filename) = name.strip_prefix("xl/media/") else {
        return false;
    };
    if filename.is_empty() || filename.contains('/') {
        return false;
    }
    matches!(
        filename.rsplit_once('.').map(|(_, extension)| extension),
        Some("png" | "jpg" | "jpeg")
    )
}

fn lookup_content_type<'a>(name: &str, content_types: &'a ContentTypes) -> Option<&'a str> {
    if let Some(content_type) = content_types.overrides.get(&name.to_ascii_lowercase()) {
        return Some(content_type);
    }
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    content_types.defaults.get(&extension).map(String::as_str)
}

fn validate_part_content_type(name: &str, content_type: &str) -> Result<(), WorkerFailure> {
    let expected = if name == "xl/workbook.xml" {
        return if matches!(
            content_type,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
                | "application/vnd.ms-excel.sheet.macroEnabled.main+xml"
        ) {
            Ok(())
        } else {
            Err(unsupported(
                "SpreadsheetML workbook has an unsupported main content type",
            ))
        };
    } else if name == "_rels/.rels" || name.ends_with(".rels") {
        "application/vnd.openxmlformats-package.relationships+xml"
    } else if name == "docProps/core.xml" {
        "application/vnd.openxmlformats-package.core-properties+xml"
    } else if name == "docProps/app.xml" {
        "application/vnd.openxmlformats-officedocument.extended-properties+xml"
    } else if name == "xl/worksheets/sheet1.xml"
        || numbered_path(name, "xl/worksheets/sheet", ".xml")
    {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"
    } else if name == "xl/styles.xml" {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"
    } else if name == "xl/sharedStrings.xml" {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"
    } else if numbered_path(name, "xl/tables/table", ".xml") {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"
    } else if numbered_path(name, "xl/comments", ".xml") {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml"
    } else if name == "xl/calcChain.xml" {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"
    } else if numbered_path(name, "xl/externalLinks/externalLink", ".xml") {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.externalLink+xml"
    } else if name == "xl/connections.xml" {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml"
    } else if numbered_path(name, "xl/drawings/drawing", ".xml") {
        "application/vnd.openxmlformats-officedocument.drawing+xml"
    } else if numbered_path(name, "xl/charts/chart", ".xml") {
        "application/vnd.openxmlformats-officedocument.drawingml.chart+xml"
    } else if numbered_path(name, "xl/theme/theme", ".xml") {
        "application/vnd.openxmlformats-officedocument.theme+xml"
    } else if name == "xl/vbaProject.bin" {
        "application/vnd.ms-office.vbaProject"
    } else if numbered_path(name, "xl/ctrlProps/ctrlProp", ".xml") {
        "application/vnd.ms-excel.controlproperties+xml"
    } else if numbered_path(name, "xl/printerSettings/printerSettings", ".bin") {
        "application/vnd.ms-excel.printerSettings"
    } else if name.starts_with("xl/media/") {
        if name.ends_with(".png") {
            "image/png"
        } else {
            "image/jpeg"
        }
    } else {
        return Err(unsupported(
            "SpreadsheetML package part has an unsupported content type mapping",
        ));
    };
    if content_type == expected {
        Ok(())
    } else {
        Err(unsupported(
            "SpreadsheetML package part has an unexpected content type",
        ))
    }
}

fn require_part(coverage: &PackageCoverage, name: &str) -> Result<(), WorkerFailure> {
    if has_part(coverage, name) {
        Ok(())
    } else {
        Err(semantic_failure(
            "SpreadsheetML package is missing a required part",
        ))
    }
}

fn has_part(coverage: &PackageCoverage, name: &str) -> bool {
    coverage.part_names.contains_key(&name.to_ascii_lowercase())
}

fn validate_relationship_graph(
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), WorkerFailure> {
    let mut in_degree = BTreeMap::<String, usize>::new();
    for (source, targets) in graph {
        in_degree.entry(source.clone()).or_default();
        for target in targets {
            *in_degree.entry(target.clone()).or_default() += 1;
        }
    }

    let mut ready = VecDeque::new();
    for (part, degree) in &in_degree {
        if *degree == 0 {
            ready.push_back(part.clone());
        }
    }
    let mut visited = 0usize;
    while let Some(part) = ready.pop_front() {
        visited = visited
            .checked_add(1)
            .ok_or_else(|| resource_limit("relationship graph size overflow"))?;
        if let Some(targets) = graph.get(&part) {
            for target in targets {
                let degree = in_degree.get_mut(target).ok_or_else(|| {
                    semantic_failure("SpreadsheetML relationship graph is inconsistent")
                })?;
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(target.clone());
                }
            }
        }
    }
    if visited != in_degree.len() {
        return Err(semantic_failure(
            "SpreadsheetML package contains a relationship cycle",
        ));
    }
    Ok(())
}

fn preflight_zip(input: &[u8]) -> Result<ZipPreflight, WorkerFailure> {
    const EOCD_SIGNATURE: u32 = 0x0605_4b50;
    const EOCD_FIXED_SIZE: usize = 22;
    const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4b50;
    const CENTRAL_HEADER_SIZE: usize = 46;

    if input.len() < EOCD_FIXED_SIZE {
        return Err(zip_failure("truncated ZIP end-of-central-directory record"));
    }

    let search_start = input
        .len()
        .saturating_sub(EOCD_FIXED_SIZE + usize::from(u16::MAX));
    let search_end = input.len() - EOCD_FIXED_SIZE;
    let mut eocd_offset = None;
    for offset in search_start..=search_end {
        if read_u32(input, offset) != Some(EOCD_SIGNATURE) {
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
                "ambiguous ZIP end-of-central-directory records",
            ));
        }
    }
    let eocd_offset =
        eocd_offset.ok_or_else(|| zip_failure("ZIP end-of-central-directory record not found"))?;

    let disk_number =
        read_u16(input, eocd_offset + 4).ok_or_else(|| zip_failure("truncated ZIP disk number"))?;
    let central_disk = read_u16(input, eocd_offset + 6)
        .ok_or_else(|| zip_failure("truncated ZIP central-directory disk number"))?;
    let entries_on_disk = read_u16(input, eocd_offset + 8)
        .ok_or_else(|| zip_failure("truncated ZIP disk entry count"))?;
    let entries_total = read_u16(input, eocd_offset + 10)
        .ok_or_else(|| zip_failure("truncated ZIP entry count"))?;
    if entries_on_disk == u16::MAX || entries_total == u16::MAX {
        return Err(zip_failure("ZIP64 archives are unsupported"));
    }
    if usize::from(entries_total) > MAX_ARCHIVE_ENTRIES {
        return Err(resource_limit(
            "SpreadsheetML package exceeds the archive-entry limit",
        ));
    }
    if disk_number != 0 || central_disk != 0 || entries_on_disk != entries_total {
        return Err(zip_failure("multi-disk ZIP archives are unsupported"));
    }

    let central_size = read_u32(input, eocd_offset + 12)
        .ok_or_else(|| zip_failure("truncated ZIP central-directory size"))?;
    let central_offset = read_u32(input, eocd_offset + 16)
        .ok_or_else(|| zip_failure("truncated ZIP central-directory offset"))?;
    if central_size == u32::MAX || central_offset == u32::MAX {
        return Err(zip_failure("ZIP64 central directories are unsupported"));
    }
    let central_size = usize::try_from(central_size)
        .map_err(|_| zip_failure("ZIP central-directory size is out of range"))?;
    let central_offset = usize::try_from(central_offset)
        .map_err(|_| zip_failure("ZIP central-directory offset is out of range"))?;
    let central_start = eocd_offset
        .checked_sub(central_size)
        .ok_or_else(|| zip_failure("ZIP central directory extends beyond EOF"))?;
    if central_offset > central_start
        || central_start.checked_add(central_size) != Some(eocd_offset)
    {
        return Err(zip_failure("invalid ZIP central-directory bounds"));
    }
    let archive_offset = central_start - central_offset;

    let mut offset = central_start;
    let mut names = BTreeSet::new();
    let mut entries = Vec::with_capacity(usize::from(entries_total));
    let mut local_ranges = Vec::with_capacity(usize::from(entries_total));
    let mut declared_total = 0u64;

    for _ in 0..entries_total {
        if read_u32(input, offset) != Some(CENTRAL_HEADER_SIGNATURE) {
            return Err(zip_failure("unexpected record in ZIP central directory"));
        }
        let header_end = offset
            .checked_add(CENTRAL_HEADER_SIZE)
            .ok_or_else(|| zip_failure("ZIP central-directory offset overflow"))?;
        let header = input
            .get(offset..header_end)
            .ok_or_else(|| zip_failure("truncated ZIP central-directory entry"))?;
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
            return Err(zip_failure("ZIP64 entries are unsupported"));
        }
        if disk_start != 0 {
            return Err(zip_failure("multi-disk ZIP entries are unsupported"));
        }
        if flags & (0x0001 | 0x0040 | 0x2000) != 0 {
            return Err(zip_failure("encrypted ZIP entries are unsupported"));
        }
        if method != 0 && method != 8 {
            return Err(zip_failure(
                "SpreadsheetML ZIP uses an unsupported compression method",
            ));
        }

        let name_start = header_end;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or_else(|| zip_failure("ZIP entry name length overflow"))?;
        let extra_start = name_end;
        let extra_end = extra_start
            .checked_add(extra_len)
            .ok_or_else(|| zip_failure("ZIP entry extra-field length overflow"))?;
        let entry_end = extra_end
            .checked_add(comment_len)
            .ok_or_else(|| zip_failure("ZIP entry comment length overflow"))?;
        if entry_end > eocd_offset {
            return Err(zip_failure(
                "truncated ZIP central-directory variable field",
            ));
        }
        let raw_name = input
            .get(name_start..name_end)
            .ok_or_else(|| zip_failure("truncated ZIP central-directory name"))?;
        if raw_name.is_empty() {
            return Err(zip_failure("empty ZIP entry name"));
        }
        validate_zip_extra_fields(input, extra_start, extra_end)?;
        let (name, directory, normalized_name) = normalize_zip_name(raw_name)?;
        if !names.insert(normalized_name) {
            return Err(unsupported(
                "duplicate or case-aliased SpreadsheetML ZIP entry",
            ));
        }

        let compressed_size = u64::from(compressed_size);
        let uncompressed_size = u64::from(uncompressed_size);
        if uncompressed_size > MAX_ENTRY_BYTES {
            return Err(resource_limit(
                "SpreadsheetML entry exceeds the per-entry byte limit",
            ));
        }
        if directory && (compressed_size != 0 || uncompressed_size != 0) {
            return Err(zip_failure(
                "ZIP directory entry unexpectedly contains data",
            ));
        }
        declared_total = declared_total
            .checked_add(uncompressed_size)
            .ok_or_else(|| resource_limit("SpreadsheetML declared-size total overflow"))?;
        if declared_total > MAX_TOTAL_UNCOMPRESSED_BYTES {
            return Err(resource_limit(
                "SpreadsheetML package exceeds the declared decompressed-size limit",
            ));
        }

        let local_start = archive_offset
            .checked_add(
                usize::try_from(local_header_offset)
                    .map_err(|_| zip_failure("ZIP local-header offset is out of range"))?,
            )
            .ok_or_else(|| zip_failure("ZIP local-header offset overflow"))?;
        let central_entry = ZipEntryMetadata {
            name: raw_name,
            flags,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
        };
        let local_end = validate_local_header(input, local_start, central_start, central_entry)?;
        local_ranges.push((local_start, local_end));
        entries.push(ZipEntry {
            name,
            raw_name: raw_name.to_vec(),
            directory,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
        });
        offset = entry_end;
    }

    if offset != eocd_offset {
        return Err(zip_failure(
            "ZIP central-directory size disagrees with its entry count",
        ));
    }
    local_ranges.sort_unstable_by_key(|range| range.0);
    let mut covered_until = archive_offset;
    for (local_start, local_end) in local_ranges {
        if local_start != covered_until {
            return Err(zip_failure(
                "ZIP local records overlap or leave unrecognized bytes",
            ));
        }
        covered_until = local_end;
    }
    if covered_until != central_start {
        return Err(zip_failure(
            "ZIP local records do not exactly cover archive data",
        ));
    }

    Ok(ZipPreflight {
        archive_offset: u64::try_from(archive_offset)
            .map_err(|_| zip_failure("ZIP archive offset is out of range"))?,
        central_directory_start: u64::try_from(central_start)
            .map_err(|_| zip_failure("ZIP central-directory offset is out of range"))?,
        entries,
    })
}

fn validate_local_header(
    input: &[u8],
    start: usize,
    central_directory_start: usize,
    central: ZipEntryMetadata<'_>,
) -> Result<usize, WorkerFailure> {
    const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;
    const LOCAL_HEADER_SIZE: usize = 30;

    if start >= central_directory_start || read_u32(input, start) != Some(LOCAL_FILE_HEADER) {
        return Err(zip_failure("invalid ZIP local-file header offset"));
    }
    let header_end = start
        .checked_add(LOCAL_HEADER_SIZE)
        .ok_or_else(|| zip_failure("ZIP local-header offset overflow"))?;
    let header = input
        .get(start..header_end)
        .ok_or_else(|| zip_failure("truncated ZIP local-file header"))?;
    let flags = u16::from_le_bytes([header[6], header[7]]);
    let method = u16::from_le_bytes([header[8], header[9]]);
    let crc32 = u32::from_le_bytes([header[14], header[15], header[16], header[17]]);
    let compressed_size = u32::from_le_bytes([header[18], header[19], header[20], header[21]]);
    let uncompressed_size = u32::from_le_bytes([header[22], header[23], header[24], header[25]]);
    let name_len = usize::from(u16::from_le_bytes([header[26], header[27]]));
    let extra_len = usize::from(u16::from_le_bytes([header[28], header[29]]));
    if compressed_size == u32::MAX || uncompressed_size == u32::MAX {
        return Err(zip_failure("ZIP64 local entries are unsupported"));
    }
    if flags != central.flags || method != central.method {
        return Err(zip_failure(
            "ZIP local and central entry flags or methods disagree",
        ));
    }
    let name_start = header_end;
    let name_end = name_start
        .checked_add(name_len)
        .ok_or_else(|| zip_failure("ZIP local name length overflow"))?;
    let extra_start = name_end;
    let extra_end = extra_start
        .checked_add(extra_len)
        .ok_or_else(|| zip_failure("ZIP local extra-field length overflow"))?;
    if extra_end > central_directory_start {
        return Err(zip_failure(
            "ZIP local header extends into the central directory",
        ));
    }
    let local_name = input
        .get(name_start..name_end)
        .ok_or_else(|| zip_failure("truncated ZIP local-file name"))?;
    if local_name != central.name {
        return Err(zip_failure("ZIP local and central entry names disagree"));
    }
    validate_zip_extra_fields(input, extra_start, extra_end)?;

    let descriptor = central.flags & 0x0008 != 0;
    let local_compressed = u64::from(compressed_size);
    let local_uncompressed = u64::from(uncompressed_size);
    if descriptor {
        if (crc32 != 0 && crc32 != central.crc32)
            || (local_compressed != 0 && local_compressed != central.compressed_size)
            || (local_uncompressed != 0 && local_uncompressed != central.uncompressed_size)
        {
            return Err(zip_failure(
                "ZIP data-descriptor placeholders disagree with central metadata",
            ));
        }
    } else if crc32 != central.crc32
        || local_compressed != central.compressed_size
        || local_uncompressed != central.uncompressed_size
    {
        return Err(zip_failure(
            "ZIP local and central sizes or checksums disagree",
        ));
    }

    let data_start = extra_end;
    let data_end = data_start
        .checked_add(
            usize::try_from(central.compressed_size)
                .map_err(|_| zip_failure("ZIP compressed size is out of range"))?,
        )
        .ok_or_else(|| zip_failure("ZIP compressed-data range overflow"))?;
    if data_end > central_directory_start {
        return Err(zip_failure(
            "ZIP compressed data extends into the central directory",
        ));
    }
    if !descriptor {
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
        _ => Err(zip_failure("invalid or ambiguous ZIP data descriptor")),
    }
}

fn validate_zip_extra_fields(input: &[u8], start: usize, end: usize) -> Result<(), WorkerFailure> {
    let mut offset = start;
    while offset < end {
        let header_end = offset
            .checked_add(4)
            .ok_or_else(|| zip_failure("ZIP extra-field offset overflow"))?;
        let header = input
            .get(offset..header_end)
            .filter(|_| header_end <= end)
            .ok_or_else(|| zip_failure("truncated ZIP extra-field header"))?;
        let field_id = u16::from_le_bytes([header[0], header[1]]);
        let field_len = usize::from(u16::from_le_bytes([header[2], header[3]]));
        if field_id == 0x0001 {
            return Err(zip_failure("ZIP64 extra fields are unsupported"));
        }
        if field_id == 0x7075 {
            return Err(zip_failure("ZIP Unicode Path extra fields are unsupported"));
        }
        offset = header_end
            .checked_add(field_len)
            .filter(|value| *value <= end)
            .ok_or_else(|| zip_failure("truncated ZIP extra-field value"))?;
    }
    Ok(())
}

fn normalize_zip_name(raw_name: &[u8]) -> Result<(String, bool, String), WorkerFailure> {
    if raw_name.is_empty() || raw_name.contains(&0) {
        return Err(unsupported(
            "empty or NUL-containing SpreadsheetML ZIP entry name",
        ));
    }
    let directory = raw_name.ends_with(b"/");
    if directory && raw_name.ends_with(b"//") {
        return Err(unsupported(
            "non-canonical SpreadsheetML ZIP directory name",
        ));
    }
    let part_bytes = if directory {
        &raw_name[..raw_name.len() - 1]
    } else {
        raw_name
    };
    if part_bytes.is_empty() {
        return Err(unsupported("empty SpreadsheetML ZIP part name"));
    }
    if !part_bytes.is_ascii() {
        return Err(unsupported(
            "non-ASCII SpreadsheetML ZIP part names are unsupported",
        ));
    }
    let name = std::str::from_utf8(part_bytes)
        .map_err(|_| unsupported("invalid UTF-8 SpreadsheetML ZIP part name"))?;
    validate_part_name(name)?;
    Ok((name.to_owned(), directory, name.to_ascii_lowercase()))
}

fn validate_part_name(name: &str) -> Result<(), WorkerFailure> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains(['\\', '%', '?', '#', ':'])
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || name.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(unsupported(
            "unsafe or non-canonical SpreadsheetML part name",
        ));
    }
    Ok(())
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
    semantic_failure(message)
}

fn resource_limit(message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::new(WorkerFailureCode::InspectionResourceLimitExceeded, message)
}

fn unsupported(message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::new(WorkerFailureCode::UnsupportedSemanticConstruct, message)
}

fn semantic_failure(message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::new(WorkerFailureCode::SemanticExtractionFailed, message)
}

#[cfg(test)]
mod namespace_tests {
    use super::*;

    #[test]
    fn prefixed_content_types_use_the_resolved_namespace() {
        let xml = br#"<ct:Types xmlns:ct="http://schemas.openxmlformats.org/package/2006/content-types"><ct:Default Extension="xml" ContentType="application/xml"/></ct:Types>"#;
        assert!(parse_content_types(xml).is_ok());
    }

    #[test]
    fn content_types_reject_wrong_namespace_even_with_expected_local_name() {
        let xml = br#"<ct:Types xmlns:ct="urn:unexpected"><ct:Default Extension="xml" ContentType="application/xml"/></ct:Types>"#;
        assert!(parse_content_types(xml).is_err());
    }

    #[test]
    fn prefixed_relationships_use_the_resolved_namespace() {
        let xml = br#"<rel:Relationships xmlns:rel="http://schemas.openxmlformats.org/package/2006/relationships"><rel:Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.invalid/" TargetMode="External"/></rel:Relationships>"#;
        let mut coverage = PackageCoverage::default();
        assert!(validate_relationships("ignored", "xl/workbook.xml", xml, &mut coverage).is_ok());
    }
}

#[cfg(test)]
mod picture_anchor_bound_tests {
    use std::io::Write;

    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::*;

    const MAX_PICTURE_ANCHORS: usize = 4_096;
    const DRAWING_PART: &str = "xl/drawings/drawing1.xml";
    const QUALIFIED_IMAGE_XLSX: &[u8] = include_bytes!(
        "../../../../experiments/document-semantic-inspection/fixtures/xlsx/image-add.xlsx"
    );

    #[test]
    fn drawing_picture_anchor_limit_is_enforced_before_parser_work() {
        let at_limit = xlsx_with_picture_anchors(MAX_PICTURE_ANCHORS);
        assert!(
            preflight_spreadsheet_package(&at_limit, &AdapterProfile::default()).is_ok(),
            "the qualified drawing fixture with exactly 4096 picture anchors should fit"
        );

        let over_limit = xlsx_with_picture_anchors(MAX_PICTURE_ANCHORS + 1);
        let result = preflight_spreadsheet_package(&over_limit, &AdapterProfile::default());
        assert!(
            result.as_ref().is_err_and(|error| {
                error.code() == WorkerFailureCode::InspectionResourceLimitExceeded
            }),
            "package preflight must reject 4097 picture anchors before rxls; got {result:?}"
        );
    }

    fn xlsx_with_picture_anchors(anchor_count: usize) -> Vec<u8> {
        let mut archive = ZipArchive::new(Cursor::new(QUALIFIED_IMAGE_XLSX))
            .expect("PoC-qualified image XLSX is a readable ZIP package");
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        for index in 0..archive.len() {
            let (name, mut bytes) = {
                let mut entry = archive
                    .by_index(index)
                    .expect("qualified XLSX ZIP entry is readable");
                let name = entry.name().to_owned();
                let mut bytes = Vec::new();
                entry
                    .read_to_end(&mut bytes)
                    .expect("qualified XLSX ZIP entry is complete");
                (name, bytes)
            };

            if name == DRAWING_PART {
                let drawing = std::str::from_utf8(&bytes)
                    .expect("qualified SpreadsheetDrawing part is UTF-8");
                let start = drawing
                    .find("<xdr:oneCellAnchor>")
                    .expect("qualified image fixture has one picture anchor");
                let end = drawing[start..]
                    .find("</xdr:oneCellAnchor>")
                    .map(|offset| start + offset + "</xdr:oneCellAnchor>".len())
                    .expect("qualified picture anchor is closed");
                let anchor = &drawing[start..end];
                assert!(anchor.contains("<xdr:pic>"));
                let repeated_anchors = anchor.repeat(anchor_count);
                let rewritten = format!(
                    "{}{}{}",
                    &drawing[..start],
                    repeated_anchors,
                    &drawing[end..]
                );
                assert_eq!(
                    rewritten.matches("<xdr:oneCellAnchor>").count(),
                    anchor_count,
                    "fixture must contain the requested number of separate picture anchors"
                );
                assert_eq!(
                    rewritten.matches("<xdr:pic>").count(),
                    anchor_count,
                    "each drawing anchor must contain one picture"
                );
                bytes = rewritten.into_bytes();
            }

            writer
                .start_file(name, options)
                .expect("synthetic XLSX part starts");
            writer
                .write_all(&bytes)
                .expect("synthetic XLSX part is written");
        }

        writer
            .finish()
            .expect("synthetic XLSX ZIP package is complete")
            .into_inner()
    }
}
