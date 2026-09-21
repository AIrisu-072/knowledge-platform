use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use office_oxide::docx::DocxDocument;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::{
    canonical_json_bytes, AdapterOutput, CommentEvidence, EditorialEvidence, FormatId,
    InspectionAdapter, InspectionProfile, PocError, TrackedChangeEvidence,
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

    validate_relationship_graph(&parts)?;

    let mut relationships = BTreeMap::new();
    if let Some(data) = parts.get("word/_rels/document.xml.rels") {
        relationships = parse_relationships(data)?;
    }

    let document = parts
        .get("word/document.xml")
        .ok_or_else(|| PocError::SemanticExtractionFailed("missing word/document.xml".into()))?;
    let editorial = parse_editorial_evidence(&parts, document)?;

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

fn has_any_element(data: &[u8], names: &[&str]) -> Result<bool, PocError> {
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
    tokens.push(format!("image-sha256:{}", image_semantic_digest(bytes)?));
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


fn parse_editorial_evidence(
    parts: &BTreeMap<String, Vec<u8>>,
    document: &[u8],
) -> Result<EditorialEvidence, PocError> {
    let tracked_changes = parse_tracked_changes(document)?;
    let comments = parse_comments(
        parts.get("word/comments.xml").map(Vec::as_slice),
        parts.get("word/commentsExtended.xml").map(Vec::as_slice),
    )?;
    let (mut document_author_labels, last_modified_by, modification_metadata) =
        parse_core_properties(parts.get("docProps/core.xml").map(Vec::as_slice))?;

    let mut labels: BTreeSet<String> = document_author_labels.into_iter().collect();
    for change in &tracked_changes {
        if let Some(author) = &change.author_label {
            if !author.is_empty() {
                labels.insert(author.clone());
            }
        }
    }
    for comment in &comments {
        if let Some(author) = &comment.author_label {
            if !author.is_empty() {
                labels.insert(author.clone());
            }
        }
    }
    if let Some(author) = &last_modified_by {
        if !author.is_empty() {
            labels.insert(author.clone());
        }
    }
    document_author_labels = labels.into_iter().collect();

    Ok(EditorialEvidence {
        tracked_changes_present: !tracked_changes.is_empty(),
        comments_present: !comments.is_empty() || has_comment_markup(document)?,
        tracked_changes,
        comments,
        document_author_labels,
        last_modified_by,
        modification_metadata,
    })
}

fn revision_kind(local: &str) -> Option<&'static str> {
    match local {
        "ins" => Some("insertion"),
        "del" => Some("deletion"),
        "moveFrom" => Some("move_from"),
        "moveTo" => Some("move_to"),
        _ if local.ends_with("PrChange") => Some("format"),
        _ => None,
    }
}

fn parse_tracked_changes(data: &[u8]) -> Result<Vec<TrackedChangeEvidence>, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("document XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut changes = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                let local = event.local_name();
                if let Some(kind) = revision_kind(local.as_ref()) {
                    let id = attr(&event, "id")?;
                    changes.push(TrackedChangeEvidence {
                        kind: kind.to_owned(),
                        author_label: attr(&event, "author")?,
                        timestamp: attr(&event, "date")?,
                        source_locator: match id {
                            Some(id) => format!("word/document.xml#{kind}:{id}"),
                            None => format!("word/document.xml#{kind}:{}", changes.len()),
                        },
                        unresolved: true,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "tracked-change XML parse failed: {error}"
                )));
            }
        }
    }
    Ok(changes)
}

fn parse_comments(
    comments_data: Option<&[u8]>,
    extended_data: Option<&[u8]>,
) -> Result<Vec<CommentEvidence>, PocError> {
    let Some(data) = comments_data else {
        return Ok(Vec::new());
    };
    let resolved_by_para = parse_comment_resolution(extended_data)?;
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("comments XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);

    #[derive(Default)]
    struct PendingComment {
        id: Option<String>,
        author: Option<String>,
        timestamp: Option<String>,
        para_id: Option<String>,
        text: Vec<String>,
    }

    let mut current: Option<PendingComment> = None;
    let mut in_text = false;
    let mut comments = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "comment" => {
                    current = Some(PendingComment {
                        id: attr(&event, "id")?,
                        author: attr(&event, "author")?,
                        timestamp: attr(&event, "date")?,
                        ..PendingComment::default()
                    });
                }
                "p" => {
                    if let Some(comment) = current.as_mut() {
                        if comment.para_id.is_none() {
                            comment.para_id = attr(&event, "paraId")?;
                        }
                    }
                }
                "t" if current.is_some() => in_text = true,
                _ => {}
            },
            Ok(Event::Empty(event)) if event.local_name().as_ref() == "p" => {
                if let Some(comment) = current.as_mut() {
                    if comment.para_id.is_none() {
                        comment.para_id = attr(&event, "paraId")?;
                    }
                }
            }
            Ok(Event::Text(value)) if in_text => {
                if let Some(comment) = current.as_mut() {
                    let value = quick_xml::escape::unescape(value.as_ref()).map_err(|error| {
                        PocError::SemanticExtractionFailed(format!(
                            "comment text entity decode failed: {error}"
                        ))
                    })?;
                    comment.text.push(value.into_owned());
                }
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "comment" => {
                    if let Some(comment) = current.take() {
                        let resolved = comment
                            .para_id
                            .as_ref()
                            .and_then(|id| resolved_by_para.get(id))
                            .copied()
                            .unwrap_or(false);
                        comments.push(CommentEvidence {
                            author_label: comment.author,
                            timestamp: comment.timestamp,
                            resolved_state: if resolved {
                                "resolved".to_owned()
                            } else {
                                "unresolved".to_owned()
                            },
                            source_locator: format!(
                                "word/comments.xml#comment:{}",
                                comment.id.unwrap_or_else(|| comments.len().to_string())
                            ),
                            content: normalize_text(&comment.text.join(" ")),
                        });
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "comments XML parse failed: {error}"
                )));
            }
        }
    }

    if current.is_some() {
        return Err(PocError::SemanticExtractionFailed(
            "comments XML ended inside a comment".into(),
        ));
    }
    Ok(comments)
}

fn parse_comment_resolution(
    data: Option<&[u8]>,
) -> Result<BTreeMap<String, bool>, PocError> {
    let Some(data) = data else {
        return Ok(BTreeMap::new());
    };
    let text = std::str::from_utf8(data).map_err(|_| {
        PocError::SemanticExtractionFailed("commentsExtended XML is not UTF-8".into())
    })?;
    let mut reader = Reader::from_str(text);
    let mut result = BTreeMap::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "commentEx" =>
            {
                if let Some(para_id) = attr(&event, "paraId")? {
                    let done = attr(&event, "done")?
                        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "on"));
                    result.insert(para_id, done);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "commentsExtended XML parse failed: {error}"
                )));
            }
        }
    }
    Ok(result)
}

fn parse_core_properties(
    data: Option<&[u8]>,
) -> Result<(Vec<String>, Option<String>, BTreeMap<String, String>), PocError> {
    let Some(data) = data else {
        return Ok((Vec::new(), None, BTreeMap::new()));
    };
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("core properties are not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut current: Option<&'static str> = None;
    let mut values: BTreeMap<String, String> = BTreeMap::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                current = match event.local_name().as_ref() {
                    "creator" => Some("creator"),
                    "lastModifiedBy" => Some("lastModifiedBy"),
                    "created" => Some("created"),
                    "modified" => Some("modified"),
                    _ => None,
                };
            }
            Ok(Event::Text(value)) => {
                if let Some(key) = current {
                    let value = quick_xml::escape::unescape(value.as_ref()).map_err(|error| {
                        PocError::SemanticExtractionFailed(format!(
                            "core-property entity decode failed: {error}"
                        ))
                    })?;
                    values.insert(key.to_owned(), value.into_owned());
                }
            }
            Ok(Event::End(_)) => current = None,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "core properties XML parse failed: {error}"
                )));
            }
        }
    }

    let mut authors = Vec::new();
    if let Some(creator) = values.get("creator") {
        if !creator.is_empty() {
            authors.push(creator.clone());
        }
    }
    let last_modified_by = values.get("lastModifiedBy").cloned();
    let modification_metadata = values
        .into_iter()
        .filter(|(key, _)| matches!(key.as_str(), "created" | "modified"))
        .collect();
    Ok((authors, last_modified_by, modification_metadata))
}

#[derive(Debug)]
struct PackageRelationship {
    target: String,
    external: bool,
}

fn validate_relationship_graph(parts: &BTreeMap<String, Vec<u8>>) -> Result<(), PocError> {
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (rels_name, data) in parts {
        if !rels_name.ends_with(".rels") {
            continue;
        }
        let source = source_part_for_relationships(rels_name).ok_or_else(|| {
            PocError::SemanticExtractionFailed(format!(
                "invalid relationships part path {rels_name}"
            ))
        })?;
        for rel in parse_package_relationships(data)? {
            if rel.external {
                continue;
            }
            let target = resolve_package_target(&source, &rel.target)?;
            if !parts.contains_key(&target) {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "relationship target {target} from {rels_name} is missing"
                )));
            }
            graph.entry(source.clone()).or_default().push(target);
        }
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in graph.keys() {
        visit_relationship_node(node, &graph, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn source_part_for_relationships(rels_name: &str) -> Option<String> {
    if rels_name == "_rels/.rels" {
        return Some(String::new());
    }
    let (dir, file) = rels_name.rsplit_once("/_rels/")?;
    let source_name = file.strip_suffix(".rels")?;
    Some(if dir.is_empty() {
        source_name.to_owned()
    } else {
        format!("{dir}/{source_name}")
    })
}

fn parse_package_relationships(data: &[u8]) -> Result<Vec<PackageRelationship>, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("relationships are not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut result = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Relationship" =>
            {
                result.push(PackageRelationship {
                    target: attr_required(&event, "Target")?,
                    external: attr(&event, "TargetMode")?
                        .is_some_and(|value| value.eq_ignore_ascii_case("External")),
                });
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "relationship graph XML parse failed: {error}"
                )));
            }
        }
    }
    Ok(result)
}

fn resolve_package_target(source: &str, target: &str) -> Result<String, PocError> {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains('\\')
        || target.split('/').any(|segment| segment == "..")
    {
        return Err(PocError::SemanticExtractionFailed(format!(
            "unsafe relationship target {target:?}"
        )));
    }

    let base = source.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let mut result = String::new();
    if !base.is_empty() {
        result.push_str(base);
        result.push('/');
    }
    result.push_str(target.trim_start_matches("./"));
    Ok(result)
}

fn visit_relationship_node(
    node: &str,
    graph: &BTreeMap<String, Vec<String>>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> Result<(), PocError> {
    if visited.contains(node) {
        return Ok(());
    }
    if !visiting.insert(node.to_owned()) {
        return Err(PocError::SemanticExtractionFailed(format!(
            "relationship cycle detected at {node}"
        )));
    }

    if let Some(targets) = graph.get(node) {
        for target in targets {
            visit_relationship_node(target, graph, visiting, visited)?;
        }
    }
    visiting.remove(node);
    visited.insert(node.to_owned());
    Ok(())
}

fn image_semantic_digest(bytes: &[u8]) -> Result<String, PocError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Ok(hex::encode(Sha256::digest(bytes)));
    }

    let mut normalized = Vec::new();
    normalized.extend_from_slice(PNG_SIGNATURE);
    let mut offset = PNG_SIGNATURE.len();
    let mut saw_iend = false;

    while offset < bytes.len() {
        if bytes.len().saturating_sub(offset) < 12 {
            return Err(PocError::SemanticExtractionFailed(
                "truncated PNG chunk".into(),
            ));
        }
        let length = u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| PocError::SemanticExtractionFailed("invalid PNG length".into()))?,
        ) as usize;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(length)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        if chunk_end > bytes.len() {
            return Err(PocError::SemanticExtractionFailed(
                "PNG chunk exceeds image bytes".into(),
            ));
        }

        let chunk_type = &bytes[offset + 4..offset + 8];
        let ignorable_metadata = matches!(
            chunk_type,
            b"tEXt" | b"zTXt" | b"iTXt" | b"tIME" | b"eXIf"
        );
        if !ignorable_metadata {
            normalized.extend_from_slice(chunk_type);
            normalized.extend_from_slice(&bytes[data_start..data_end]);
        }
        if chunk_type == b"IEND" {
            saw_iend = true;
            break;
        }
        offset = chunk_end;
    }

    if !saw_iend {
        return Err(PocError::SemanticExtractionFailed(
            "PNG has no IEND chunk".into(),
        ));
    }
    Ok(hex::encode(Sha256::digest(&normalized)))
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
