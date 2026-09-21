use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use office_oxide::docx::DocxDocument;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::{
    canonical_json_bytes, AdapterOutput, EditorialEvidence, FormatId, InspectionAdapter,
    InspectionProfile, PocError,
};

const MAX_ENTRIES: usize = 256;
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 64;

const WORD_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

#[derive(Debug, Clone, Copy, Default)]
pub struct DocxAdapter;

#[derive(Debug)]
struct PackageInspection {
    parts: BTreeMap<String, Vec<u8>>,
    relationships: BTreeMap<String, Relationship>,
    editorial: EditorialEvidence,
}

#[derive(Debug, Clone)]
struct Relationship {
    kind: String,
    target: String,
    external: bool,
}

#[derive(Debug, Serialize)]
struct DocxProjection {
    plain_text: String,
    body_tokens: Vec<String>,
    headers: Vec<String>,
    footers: Vec<String>,
    footnotes: Vec<String>,
    endnotes: Vec<String>,
}

impl InspectionAdapter for DocxAdapter {
    fn format(&self) -> FormatId {
        FormatId::Docx
    }

    fn inspect(
        &self,
        input: &[u8],
        _profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError> {
        let package = inspect_package(input)?;

        let numbering = parse_numbering(
            package
                .parts
                .get("word/numbering.xml")
                .map(Vec::as_slice)
                .unwrap_or_default(),
        )?;

        let document = package
            .parts
            .get("word/document.xml")
            .ok_or_else(|| PocError::SemanticExtractionFailed("DOCX has no word/document.xml".into()))?;

        let (body_tokens, body_text) =
            parse_document_projection(document, &package, &numbering)?;

        let headers = texts_for_prefix(&package.parts, "word/header")?;
        let footers = texts_for_prefix(&package.parts, "word/footer")?;
        let footnotes = note_texts(package.parts.get("word/footnotes.xml"))?;
        let endnotes = note_texts(package.parts.get("word/endnotes.xml"))?;

        let mut visible = Vec::new();
        visible.extend(headers.iter().cloned());
        if !body_text.is_empty() {
            visible.push(body_text.clone());
        }
        visible.extend(footers.iter().cloned());
        let raw_visible_text = normalize_text(&visible.join(" "));

        // Candidate parser is an independent typed interpretation. A successful
        // raw projection is not enough if the candidate disagrees on reader-visible text.
        let candidate = DocxDocument::from_reader(Cursor::new(input))
            .map_err(|error| PocError::SemanticExtractionFailed(format!(
                "office_oxide rejected DOCX: {error}"
            )))?;
        let candidate_text = normalize_text(&candidate.plain_text());
        if candidate_text != raw_visible_text {
            return Err(PocError::ParserDisagreement(format!(
                "raw OOXML visible text differs from office_oxide: raw={:?}, candidate={:?}",
                raw_visible_text, candidate_text
            )));
        }

        let projection = DocxProjection {
            plain_text: body_text,
            body_tokens,
            headers,
            footers,
            footnotes,
            endnotes,
        };
        let semantic_projection = canonical_json_bytes(&projection)
            .map_err(|error| PocError::InvalidWorkerResult(error.to_string()))?;

        Ok(AdapterOutput {
            semantic_projection,
            capabilities: Vec::new(),
            editorial: package.editorial,
            external_dependencies: Vec::new(),
            signatures: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

pub(crate) fn is_docx_package(input: &[u8]) -> bool {
    if !input.starts_with(b"PK\x03\x04") {
        return false;
    }
    let mut archive = match ZipArchive::new(Cursor::new(input)) {
        Ok(archive) => archive,
        Err(_) => return false,
    };
    let mut content_types = match archive.by_name("[Content_Types].xml") {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut text = String::new();
    if content_types.read_to_string(&mut text).is_err() {
        return false;
    }
    text.contains(WORD_MAIN)
}

fn inspect_package(input: &[u8]) -> Result<PackageInspection, PocError> {
    let mut archive = ZipArchive::new(Cursor::new(input))
        .map_err(|error| PocError::SemanticExtractionFailed(format!("invalid DOCX ZIP: {error}")))?;

    if archive.len() > MAX_ENTRIES {
        return Err(PocError::InspectionResourceLimitExceeded);
    }

    let mut total = 0u64;
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("invalid ZIP entry: {error}")))?;
        let name = file.name().to_owned();
        validate_part_name(&name)?;
        let size = file.size();
        if size > MAX_ENTRY_BYTES {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
        total = total
            .checked_add(size)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        if total > MAX_TOTAL_BYTES {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
        let mut data = Vec::with_capacity(size as usize);
        file.read_to_end(&mut data)
            .map_err(|error| PocError::SemanticExtractionFailed(format!(
                "cannot read ZIP entry {name}: {error}"
            )))?;
        if parts.insert(name.clone(), data).is_some() {
            return Err(PocError::SemanticExtractionFailed(format!(
                "duplicate ZIP entry {name}"
            )));
        }
    }

    for (name, data) in &parts {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            validate_xml(name, data)?;
        }
    }

    let content_types = parts
        .get("[Content_Types].xml")
        .ok_or_else(|| PocError::SemanticExtractionFailed("missing [Content_Types].xml".into()))?;
    validate_content_types(content_types)?;

    let mut relationships = BTreeMap::new();
    if let Some(data) = parts.get("word/_rels/document.xml.rels") {
        relationships = parse_relationships(data)?;
    }

    let document = parts
        .get("word/document.xml")
        .ok_or_else(|| PocError::SemanticExtractionFailed("missing word/document.xml".into()))?;
    let editorial = EditorialEvidence {
        tracked_changes_present: has_revision_markup(document)?,
        comments_present: parts.contains_key("word/comments.xml") || has_comment_markup(document)?,
    };

    Ok(PackageInspection {
        parts,
        relationships,
        editorial,
    })
}

fn validate_part_name(name: &str) -> Result<(), PocError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.split('/').any(|segment| segment == "..")
    {
        return Err(PocError::SemanticExtractionFailed(format!(
            "unsafe OOXML part name {name:?}"
        )));
    }
    Ok(())
}

fn validate_xml(name: &str, data: &[u8]) -> Result<(), PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed(format!("{name} is not UTF-8 XML")))?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut depth = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => {
                depth += 1;
                if depth > MAX_XML_DEPTH {
                    return Err(PocError::InspectionResourceLimitExceeded);
                }
            }
            Ok(Event::End(_)) => {
                if depth == 0 {
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "{name} has unmatched closing tag"
                    )));
                }
                depth -= 1;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "{name} XML parse failed: {error}"
                )));
            }
        }
    }
    if depth != 0 {
        return Err(PocError::SemanticExtractionFailed(format!(
            "{name} ended with {depth} unclosed XML elements"
        )));
    }
    Ok(())
}

fn validate_content_types(data: &[u8]) -> Result<(), PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("content types are not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let known: BTreeSet<&str> = [
        "application/vnd.openxmlformats-package.relationships+xml",
        "application/xml",
        "image/png",
        WORD_MAIN,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml",
        "application/vnd.openxmlformats-package.core-properties+xml",
        "application/vnd.ms-word.commentsExtended+xml",
    ]
    .into_iter()
    .collect();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if matches!(event.local_name().as_ref(), "Default" | "Override") =>
            {
                if let Some(content_type) = attr(&event, "ContentType")? {
                    if !known.contains(content_type.as_str()) {
                        return Err(PocError::UnsupportedSemanticConstruct(format!(
                            "unknown OOXML content type {content_type}"
                        )));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "content type XML parse failed: {error}"
                )));
            }
        }
    }
    Ok(())
}

fn parse_relationships(data: &[u8]) -> Result<BTreeMap<String, Relationship>, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("relationships are not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let allowed: BTreeSet<&str> = [
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments",
        "http://schemas.microsoft.com/office/2011/relationships/commentsExtended",
    ]
    .into_iter()
    .collect();

    let mut relationships = BTreeMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Relationship" =>
            {
                let id = attr_required(&event, "Id")?;
                let kind = attr_required(&event, "Type")?;
                let target = attr_required(&event, "Target")?;
                let external = attr(&event, "TargetMode")?
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("External"));
                if !allowed.contains(kind.as_str()) {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "unknown DOCX relationship type {kind}"
                    )));
                }
                if external
                    && kind
                        != "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink"
                {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "external non-hyperlink relationship {kind}"
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
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "duplicate relationship id {id}"
                    )));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "relationship XML parse failed: {error}"
                )));
            }
        }
    }
    Ok(relationships)
}

fn has_revision_markup(data: &[u8]) -> Result<bool, PocError> {
    has_any_element(data, &["ins", "del", "moveFrom", "moveTo"])
}

fn has_comment_markup(data: &[u8]) -> Result<bool, PocError> {
    has_any_element(
        data,
        &["commentRangeStart", "commentRangeEnd", "commentReference"],
    )
}

fn has_any_element(data: &[u8], names: &[&[u8]]) -> Result<bool, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("Word XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if names.iter().any(|name| *name == event.local_name().as_ref()) =>
            {
                return Ok(true);
            }
            Ok(Event::Eof) => return Ok(false),
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "Word XML parse failed: {error}"
                )));
            }
        }
    }
}

fn parse_document_projection(
    data: &[u8],
    package: &PackageInspection,
    numbering: &BTreeMap<(u32, u8), String>,
) -> Result<(Vec<String>, String), PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("document XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut tokens = Vec::new();
    let mut visible_text = Vec::new();
    let mut in_text = false;
    let mut deleted_depth = 0usize;
    let mut paragraph_level = 0u8;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let name = event.local_name();
                match name.as_ref() {
                    "del" | "moveFrom" => deleted_depth += 1,
                    _ if deleted_depth > 0 => {}
                    "p" => {
                        tokens.push("p+".into());
                        paragraph_level = 0;
                    }
                    "tbl" => tokens.push("table+".into()),
                    "tr" => tokens.push("row+".into()),
                    "tc" => tokens.push("cell+".into()),
                    "hyperlink" => {
                        if let Some(id) = attr(&event, "id")? {
                            if let Some(rel) = package.relationships.get(&id) {
                                if rel.kind.ends_with("/hyperlink") {
                                    tokens.push(format!("link:{}", rel.target));
                                }
                            }
                        }
                    }
                    "t" => in_text = true,
                    "pStyle" => push_style_token(&event, &mut tokens)?,
                    "ilvl" => paragraph_level = attr_u8(&event, "val")?.unwrap_or(0),
                    "numId" => {
                        push_numbering_token(&event, paragraph_level, numbering, &mut tokens)?
                    }
                    "gridSpan" => {
                        if let Some(value) = attr(&event, "val")? {
                            tokens.push(format!("grid-span:{value}"));
                        }
                    }
                    "vMerge" => tokens.push(format!(
                        "vmerge:{}",
                        attr(&event, "val")?.unwrap_or_else(|| "continue".into())
                    )),
                    "blip" => push_image_token(&event, package, &mut tokens)?,
                    "pgSz" => push_page_token(&event, &mut tokens)?,
                    "footnoteReference" => tokens.push("footnote-ref".into()),
                    "endnoteReference" => tokens.push("endnote-ref".into()),
                    _ => {}
                }
            }
            Ok(Event::Empty(event)) => {
                if deleted_depth > 0 {
                    continue;
                }
                match event.local_name().as_ref() {
                    "pStyle" => push_style_token(&event, &mut tokens)?,
                    "ilvl" => paragraph_level = attr_u8(&event, "val")?.unwrap_or(0),
                    "numId" => {
                        push_numbering_token(&event, paragraph_level, numbering, &mut tokens)?
                    }
                    "gridSpan" => {
                        if let Some(value) = attr(&event, "val")? {
                            tokens.push(format!("grid-span:{value}"));
                        }
                    }
                    "vMerge" => tokens.push(format!(
                        "vmerge:{}",
                        attr(&event, "val")?.unwrap_or_else(|| "continue".into())
                    )),
                    "blip" => push_image_token(&event, package, &mut tokens)?,
                    "pgSz" => push_page_token(&event, &mut tokens)?,
                    "footnoteReference" => tokens.push("footnote-ref".into()),
                    "endnoteReference" => tokens.push("endnote-ref".into()),
                    _ => {}
                }
            }
            Ok(Event::Text(value)) if in_text && deleted_depth == 0 => {
                let value = quick_xml::escape::unescape(value.as_ref())
                    .map_err(|error| PocError::SemanticExtractionFailed(format!(
                        "text entity decode failed: {error}"
                    )))?;
                let value = value.into_owned();
                if !value.is_empty() {
                    tokens.push(format!("text:{value}"));
                    visible_text.push(value);
                }
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "del" | "moveFrom" => deleted_depth = deleted_depth.saturating_sub(1),
                "p" if deleted_depth == 0 => tokens.push("p-".into()),
                "tbl" if deleted_depth == 0 => tokens.push("table-".into()),
                "tr" if deleted_depth == 0 => tokens.push("row-".into()),
                "tc" if deleted_depth == 0 => tokens.push("cell-".into()),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "document projection parse failed: {error}"
                )));
            }
        }
    }

    Ok((tokens, normalize_text(&visible_text.join(" "))))
}

fn push_style_token(event: &BytesStart<'_>, tokens: &mut Vec<String>) -> Result<(), PocError> {
    if let Some(style) = attr(event, "val")? {
        if style != "Normal" {
            tokens.push(format!("style:{style}"));
        }
    }
    Ok(())
}

fn push_numbering_token(
    event: &BytesStart<'_>,
    level: u8,
    numbering: &BTreeMap<(u32, u8), String>,
    tokens: &mut Vec<String>,
) -> Result<(), PocError> {
    let Some(num_id) = attr(event, "val")?.and_then(|value| value.parse::<u32>().ok()) else {
        return Ok(());
    };
    let format = numbering
        .get(&(num_id, level))
        .cloned()
        .unwrap_or_else(|| format!("unknown-num:{num_id}"));
    tokens.push(format!("list:{format}:level:{level}"));
    Ok(())
}

fn push_image_token(
    event: &BytesStart<'_>,
    package: &PackageInspection,
    tokens: &mut Vec<String>,
) -> Result<(), PocError> {
    let Some(id) = attr(event, "embed")? else {
        return Ok(());
    };
    let Some(rel) = package.relationships.get(&id) else {
        return Err(PocError::SemanticExtractionFailed(format!(
            "image relationship {id} is missing"
        )));
    };
    if !rel.kind.ends_with("/image") || rel.external {
        return Err(PocError::UnsupportedSemanticConstruct(format!(
            "drawing relationship {id} is not an internal image"
        )));
    }
    let path = resolve_word_target(&rel.target)?;
    let bytes = package.parts.get(&path).ok_or_else(|| {
        PocError::SemanticExtractionFailed(format!("image target {path} is missing"))
    })?;
    tokens.push(format!("image-sha256:{}", hex::encode(Sha256::digest(bytes))));
    Ok(())
}

fn push_page_token(event: &BytesStart<'_>, tokens: &mut Vec<String>) -> Result<(), PocError> {
    let width = attr(event, "w")?.unwrap_or_default();
    let height = attr(event, "h")?.unwrap_or_default();
    let orient = attr(event, "orient")?.unwrap_or_else(|| "portrait".into());
    tokens.push(format!("page:{width}x{height}:{orient}"));
    Ok(())
}

fn parse_numbering(data: &[u8]) -> Result<BTreeMap<(u32, u8), String>, PocError> {
    if data.is_empty() {
        return Ok(BTreeMap::new());
    }
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("numbering XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut abstract_formats: BTreeMap<(u32, u8), String> = BTreeMap::new();
    let mut instances: BTreeMap<u32, u32> = BTreeMap::new();
    let mut abstract_id = None;
    let mut level = 0u8;
    let mut num_id = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "abstractNum" => {
                    abstract_id = attr(&event, "abstractNumId")?
                        .and_then(|value| value.parse::<u32>().ok());
                }
                "lvl" => {
                    level = attr(&event, "ilvl")?
                        .and_then(|value| value.parse::<u8>().ok())
                        .unwrap_or(0);
                }
                "num" => {
                    num_id = attr(&event, "numId")?
                        .and_then(|value| value.parse::<u32>().ok());
                }
                "numFmt" => {
                    if let (Some(id), Some(format)) = (abstract_id, attr(&event, "val")?) {
                        abstract_formats.insert((id, level), format);
                    }
                }
                "abstractNumId" => {
                    if let (Some(num), Some(abs)) = (
                        num_id,
                        attr(&event, "val")?.and_then(|value| value.parse::<u32>().ok()),
                    ) {
                        instances.insert(num, abs);
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(event)) => match event.local_name().as_ref() {
                "numFmt" => {
                    if let (Some(id), Some(format)) = (abstract_id, attr(&event, "val")?) {
                        abstract_formats.insert((id, level), format);
                    }
                }
                "abstractNumId" => {
                    if let (Some(num), Some(abs)) = (
                        num_id,
                        attr(&event, "val")?.and_then(|value| value.parse::<u32>().ok()),
                    ) {
                        instances.insert(num, abs);
                    }
                }
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "abstractNum" => abstract_id = None,
                "num" => num_id = None,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "numbering XML parse failed: {error}"
                )));
            }
        }
    }

    let mut resolved = BTreeMap::new();
    for (num, abs) in instances {
        for ((candidate_abs, candidate_level), format) in &abstract_formats {
            if *candidate_abs == abs {
                resolved.insert((num, *candidate_level), format.clone());
            }
        }
    }
    Ok(resolved)
}

fn texts_for_prefix(
    parts: &BTreeMap<String, Vec<u8>>,
    prefix: &str,
) -> Result<Vec<String>, PocError> {
    let mut values = Vec::new();
    for (name, data) in parts {
        if name.starts_with(prefix) && name.ends_with(".xml") {
            let text = extract_final_text(data)?;
            if !text.is_empty() {
                values.push(text);
            }
        }
    }
    Ok(values)
}

fn note_texts(data: Option<&Vec<u8>>) -> Result<Vec<String>, PocError> {
    let Some(data) = data else {
        return Ok(Vec::new());
    };
    let text = extract_final_text(data)?;
    Ok(if text.is_empty() { Vec::new() } else { vec![text] })
}

fn extract_final_text(data: &[u8]) -> Result<String, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("OOXML text part is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut in_text = false;
    let mut deleted_depth = 0usize;
    let mut values = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "del" | "moveFrom" => deleted_depth += 1,
                "t" if deleted_depth == 0 => in_text = true,
                _ => {}
            },
            Ok(Event::Text(value)) if in_text && deleted_depth == 0 => {
                let value = quick_xml::escape::unescape(value.as_ref())
                    .map_err(|error| PocError::SemanticExtractionFailed(format!(
                        "text entity decode failed: {error}"
                    )))?;
                values.push(value.into_owned());
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "del" | "moveFrom" => deleted_depth = deleted_depth.saturating_sub(1),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "OOXML text parse failed: {error}"
                )));
            }
        }
    }
    Ok(normalize_text(&values.join(" ")))
}

fn resolve_word_target(target: &str) -> Result<String, PocError> {
    if target.starts_with('/') || target.contains('\\') || target.split('/').any(|part| part == "..") {
        return Err(PocError::SemanticExtractionFailed(format!(
            "unsafe relationship target {target:?}"
        )));
    }
    Ok(format!("word/{target}"))
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn attr_required(event: &BytesStart<'_>, name: &str) -> Result<String, PocError> {
    attr(event, name)?.ok_or_else(|| {
        PocError::SemanticExtractionFailed(format!("missing required attribute {name}"))
    })
}

fn attr_u8(event: &BytesStart<'_>, name: &str) -> Result<Option<u8>, PocError> {
    Ok(attr(event, name)?.and_then(|value| value.parse().ok()))
}

fn attr(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, PocError> {
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        let key = item.key.as_ref();
        let matches = key == name
            || key.rsplit_once(':').is_some_and(|(_, local)| local == name);
        if matches {
            let value = quick_xml::escape::unescape(item.value.as_ref())
                .map_err(|error| PocError::SemanticExtractionFailed(format!(
                    "attribute entity decode failed: {error}"
                )))?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}
