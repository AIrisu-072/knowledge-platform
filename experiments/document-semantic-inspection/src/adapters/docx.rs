use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use office_oxide::docx::DocxDocument;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::{
    AdapterOutput, CapabilityEvidence, CommentEvidence, EditorialEvidence, FormatId,
    InspectionAdapter, InspectionProfile, PocError, TrackedChangeEvidence, canonical_json_bytes,
};

const MAX_ENTRIES: usize = 256;
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 64;
const MAX_DOCX_IMAGES: usize = 4_096;
const MAX_DOCX_DECODED_PIXELS: u64 = 67_108_864;
const MAX_DOCX_DECODED_OUTPUT_BYTES: usize = 268_435_456;

const WORDPROCESSINGML_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const OFFICE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DRAWINGML_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const VML_NS: &str = "urn:schemas-microsoft-com:vml";
const HEADER_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const FOOTER_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";

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

#[derive(Debug, Default)]
struct ImageBudget {
    count: usize,
    decoded_pixels: u64,
    decoded_output_bytes: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct PngMetadata {
    background: Option<[u8; 3]>,
    physical_dimensions: Option<[u8; 9]>,
}

#[derive(Debug, Clone)]
struct PngHeader {
    width: u32,
    height: u32,
    color_type: u8,
    bit_depth: u8,
    palette_entries: usize,
    palette: Vec<[u8; 3]>,
    metadata: PngMetadata,
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

        let document = package.parts.get("word/document.xml").ok_or_else(|| {
            PocError::SemanticExtractionFailed("DOCX has no word/document.xml".into())
        })?;

        reject_unprojected_nonbody_images(document, &package)?;

        let mut image_budget = ImageBudget::default();
        let (body_tokens, body_text) =
            parse_document_projection(document, &package, &numbering, &mut image_budget)?;

        let headers = texts_for_prefix(&package.parts, "word/header")?;
        let footers = texts_for_prefix(&package.parts, "word/footer")?;
        let footnotes = note_texts(package.parts.get("word/footnotes.xml"), "footnotes")?;
        let endnotes = note_texts(package.parts.get("word/endnotes.xml"), "endnotes")?;

        let mut visible = Vec::new();
        visible.extend(headers.iter().cloned());
        if !body_text.is_empty() {
            visible.push(body_text.clone());
        }
        visible.extend(footers.iter().cloned());
        let raw_visible_text = normalize_text(&visible.join(" "));

        // Candidate parser is an independent typed interpretation. A successful
        // raw projection is not enough if the candidate disagrees on reader-visible text.
        let candidate = DocxDocument::from_reader(Cursor::new(input)).map_err(|error| {
            PocError::SemanticExtractionFailed(format!("office_oxide rejected DOCX: {error}"))
        })?;
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
        let semantic_equivalence = hex::encode(crate::fingerprint(&semantic_projection));
        let reader_equivalence = hex::encode(crate::fingerprint(raw_visible_text.as_bytes()));

        Ok(AdapterOutput {
            semantic_projection,
            capabilities: vec![
                CapabilityEvidence::binary("reader_content", true, true, Some(reader_equivalence)),
                CapabilityEvidence::binary(
                    "document_structure",
                    true,
                    true,
                    Some(semantic_equivalence.clone()),
                ),
                CapabilityEvidence::binary(
                    "footnotes",
                    !projection.footnotes.is_empty(),
                    true,
                    Some(semantic_equivalence.clone()),
                ),
                CapabilityEvidence::binary(
                    "endnotes",
                    !projection.endnotes.is_empty(),
                    true,
                    Some(semantic_equivalence),
                ),
            ],
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
    let mut archive = ZipArchive::new(Cursor::new(input)).map_err(|error| {
        PocError::SemanticExtractionFailed(format!("invalid DOCX ZIP: {error}"))
    })?;

    if archive.len() > MAX_ENTRIES {
        return Err(PocError::InspectionResourceLimitExceeded);
    }

    let mut total = 0u64;
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid ZIP entry: {error}"))
        })?;
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
        file.read_to_end(&mut data).map_err(|error| {
            PocError::SemanticExtractionFailed(format!("cannot read ZIP entry {name}: {error}"))
        })?;
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

fn reject_unprojected_nonbody_images(
    document: &[u8],
    package: &PackageInspection,
) -> Result<(), PocError> {
    reject_document_vml_images(document)?;

    let text = std::str::from_utf8(document)
        .map_err(|_| PocError::SemanticExtractionFailed("document XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut inspected_parts = BTreeSet::new();

    loop {
        let (namespace, event) = reader.read_resolved_event().map_err(|error| {
            PocError::SemanticExtractionFailed(format!(
                "document reference XML parse failed: {error}"
            ))
        })?;
        let event = match event {
            Event::Start(event) | Event::Empty(event) => event,
            Event::Eof => break,
            _ => continue,
        };
        let local_name = event.local_name();

        let (relationship_kind, relationship_name) = match local_name.as_ref() {
            "headerReference" => (HEADER_RELATIONSHIP, "header"),
            "footerReference" => (FOOTER_RELATIONSHIP, "footer"),
            _ => continue,
        };

        match &namespace {
            ResolveResult::Bound(namespace) if namespace.as_ref() == WORDPROCESSINGML_NS => {}
            ResolveResult::Unknown(prefix) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "document {relationship_name} reference has an unbound namespace prefix {prefix}"
                )));
            }
            _ => continue,
        }

        let relationship_id =
            namespaced_attribute(&event, reader.resolver(), OFFICE_RELATIONSHIPS_NS, "id")?
                .ok_or_else(|| {
                    PocError::SemanticExtractionFailed(format!(
                        "document {relationship_name} reference has no relationship id"
                    ))
                })?;
        let relationship = package.relationships.get(&relationship_id).ok_or_else(|| {
            PocError::SemanticExtractionFailed(format!(
                "document {relationship_name} relationship {relationship_id} is missing"
            ))
        })?;
        if relationship.kind != relationship_kind || relationship.external {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "document reference {relationship_id} is not an internal {relationship_name}"
            )));
        }

        let part_name = resolve_word_target(&relationship.target)?;
        if inspected_parts.insert(part_name.clone()) {
            let part = package.parts.get(&part_name).ok_or_else(|| {
                PocError::SemanticExtractionFailed(format!(
                    "referenced {relationship_name} part {part_name} is missing"
                ))
            })?;
            if contains_unprojected_image_markup(part, &part_name)? {
                return Err(PocError::UnsupportedSemanticConstruct(format!(
                    "referenced {relationship_name} part {part_name} contains unprojected image semantics"
                )));
            }
        }
    }

    Ok(())
}

fn reject_document_vml_images(document: &[u8]) -> Result<(), PocError> {
    let text = std::str::from_utf8(document)
        .map_err(|_| PocError::SemanticExtractionFailed("document XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut deleted_depth = 0usize;

    loop {
        let (namespace, event) = reader.read_resolved_event().map_err(|error| {
            PocError::SemanticExtractionFailed(format!("document image XML parse failed: {error}"))
        })?;

        match event {
            Event::Start(event) => {
                let local_name = event.local_name();
                reject_unbound_image_prefix(&namespace, local_name.as_ref(), "word/document.xml")?;
                if is_xml_element(&namespace, local_name.as_ref(), WORDPROCESSINGML_NS, "del")
                    || is_xml_element(
                        &namespace,
                        local_name.as_ref(),
                        WORDPROCESSINGML_NS,
                        "moveFrom",
                    )
                {
                    deleted_depth = deleted_depth
                        .checked_add(1)
                        .ok_or(PocError::InspectionResourceLimitExceeded)?;
                } else if deleted_depth == 0
                    && is_xml_element(&namespace, local_name.as_ref(), VML_NS, "imagedata")
                {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "VML image semantics in word/document.xml are not projected".into(),
                    ));
                }
            }
            Event::Empty(event) => {
                let local_name = event.local_name();
                reject_unbound_image_prefix(&namespace, local_name.as_ref(), "word/document.xml")?;
                if deleted_depth == 0
                    && is_xml_element(&namespace, local_name.as_ref(), VML_NS, "imagedata")
                {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "VML image semantics in word/document.xml are not projected".into(),
                    ));
                }
            }
            Event::End(event) => {
                let local_name = event.local_name();
                if is_xml_element(&namespace, local_name.as_ref(), WORDPROCESSINGML_NS, "del")
                    || is_xml_element(
                        &namespace,
                        local_name.as_ref(),
                        WORDPROCESSINGML_NS,
                        "moveFrom",
                    )
                {
                    deleted_depth = deleted_depth.saturating_sub(1);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(())
}

fn contains_unprojected_image_markup(data: &[u8], part_name: &str) -> Result<bool, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed(format!("{part_name} is not UTF-8 XML")))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;

    loop {
        let (namespace, event) = reader.read_resolved_event().map_err(|error| {
            PocError::SemanticExtractionFailed(format!(
                "{part_name} image XML parse failed: {error}"
            ))
        })?;
        let event = match event {
            Event::Start(event) | Event::Empty(event) => event,
            Event::Eof => return Ok(false),
            _ => continue,
        };
        let local_name = event.local_name();
        reject_unbound_image_prefix(&namespace, local_name.as_ref(), part_name)?;
        if is_xml_element(&namespace, local_name.as_ref(), DRAWINGML_NS, "blip")
            || is_xml_element(&namespace, local_name.as_ref(), VML_NS, "imagedata")
        {
            return Ok(true);
        }
    }
}

fn reject_unbound_image_prefix(
    namespace: &ResolveResult<'_>,
    local_name: &str,
    part_name: &str,
) -> Result<(), PocError> {
    if let ResolveResult::Unknown(prefix) = namespace
        && matches!(local_name, "blip" | "imagedata")
    {
        return Err(PocError::SemanticExtractionFailed(format!(
            "{part_name} image element has unbound namespace prefix {prefix}"
        )));
    }
    Ok(())
}

fn is_xml_element(
    namespace: &ResolveResult<'_>,
    local_name: &str,
    expected_namespace: &str,
    expected_name: &str,
) -> bool {
    matches!(namespace, ResolveResult::Bound(namespace)
        if namespace.as_ref() == expected_namespace && local_name == expected_name)
}

fn namespaced_attribute(
    event: &BytesStart<'_>,
    resolver: &quick_xml::name::NamespaceResolver,
    expected_namespace: &str,
    expected_name: &str,
) -> Result<Option<String>, PocError> {
    let mut result = None;
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        let (namespace, local_name) = resolver.resolve_attribute(item.key);
        if local_name.as_ref() == expected_name
            && matches!(namespace, ResolveResult::Bound(namespace)
                if namespace.as_ref() == expected_namespace)
        {
            if result.is_some() {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "duplicate relationship attribute {expected_name}"
                )));
            }
            let value = quick_xml::escape::unescape(item.value.as_ref()).map_err(|error| {
                PocError::SemanticExtractionFailed(format!(
                    "attribute entity decode failed: {error}"
                ))
            })?;
            result = Some(value.into_owned());
        }
    }
    Ok(result)
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
                if names
                    .iter()
                    .any(|name| *name == event.local_name().as_ref()) =>
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
    image_budget: &mut ImageBudget,
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
                    "blip" => push_image_token(&event, package, image_budget, &mut tokens)?,
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
                    "blip" => push_image_token(&event, package, image_budget, &mut tokens)?,
                    "pgSz" => push_page_token(&event, &mut tokens)?,
                    "footnoteReference" => tokens.push("footnote-ref".into()),
                    "endnoteReference" => tokens.push("endnote-ref".into()),
                    _ => {}
                }
            }
            Ok(Event::Text(value)) if in_text && deleted_depth == 0 => {
                let value = quick_xml::escape::unescape(value.as_ref()).map_err(|error| {
                    PocError::SemanticExtractionFailed(format!(
                        "text entity decode failed: {error}"
                    ))
                })?;
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
    image_budget: &mut ImageBudget,
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
    tokens.push(format!(
        "image-sha256:{}",
        image_semantic_digest(bytes, image_budget)?
    ));
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
                    abstract_id =
                        attr(&event, "abstractNumId")?.and_then(|value| value.parse::<u32>().ok());
                }
                "lvl" => {
                    level = attr(&event, "ilvl")?
                        .and_then(|value| value.parse::<u8>().ok())
                        .unwrap_or(0);
                }
                "num" => {
                    num_id = attr(&event, "numId")?.and_then(|value| value.parse::<u32>().ok());
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

fn note_texts(data: Option<&Vec<u8>>, expected_root: &str) -> Result<Vec<String>, PocError> {
    let Some(data) = data else {
        return Ok(Vec::new());
    };
    let text = extract_final_note_text(data, expected_root)?;
    Ok(if text.is_empty() {
        Vec::new()
    } else {
        vec![text]
    })
}

#[derive(Debug, Default)]
struct NoteXmlFrame {
    local_name: String,
    note_type: Option<String>,
    paragraph_count: usize,
    separator_count: usize,
    properties_seen: bool,
    content_started: bool,
}

fn extract_final_note_text(data: &[u8], expected_root: &str) -> Result<String, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("note XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut stack: Vec<NoteXmlFrame> = Vec::new();
    let mut root_seen = false;
    let mut root_closed = false;
    let mut deleted_depth = 0usize;
    let mut values = Vec::new();

    loop {
        let (namespace, event) = reader.read_resolved_event().map_err(|error| {
            PocError::SemanticExtractionFailed(format!("note XML parse failed: {error}"))
        })?;
        match event {
            Event::Start(event) => {
                let element_name = event.local_name();
                let local_name = note_element_name(&namespace, element_name.as_ref())?;
                let is_root = stack.is_empty();
                if is_root {
                    if root_seen || local_name != expected_root {
                        return Err(unsupported_note_construct(local_name));
                    }
                    root_seen = true;
                }
                let frame = validate_note_element(
                    local_name,
                    &event,
                    reader.resolver(),
                    expected_root,
                    &mut stack,
                )?;
                if matches!(local_name, "del" | "moveFrom") {
                    deleted_depth = deleted_depth
                        .checked_add(1)
                        .ok_or(PocError::InspectionResourceLimitExceeded)?;
                }
                append_note_control_character(local_name, deleted_depth, &stack, &mut values)?;
                stack.push(frame);
            }
            Event::Empty(event) => {
                let element_name = event.local_name();
                let local_name = note_element_name(&namespace, element_name.as_ref())?;
                let is_root = stack.is_empty();
                if is_root {
                    if root_seen || local_name != expected_root {
                        return Err(unsupported_note_construct(local_name));
                    }
                    root_seen = true;
                }
                let frame = validate_note_element(
                    local_name,
                    &event,
                    reader.resolver(),
                    expected_root,
                    &mut stack,
                )?;
                append_note_control_character(local_name, deleted_depth, &stack, &mut values)?;
                finish_note_element(&frame)?;
                if is_root {
                    root_closed = true;
                }
            }
            Event::End(event) => {
                let element_name = event.local_name();
                let local_name = note_element_name(&namespace, element_name.as_ref())?;
                let frame = stack.pop().ok_or_else(|| {
                    PocError::SemanticExtractionFailed("note XML has an unmatched end tag".into())
                })?;
                if frame.local_name != local_name {
                    return Err(PocError::SemanticExtractionFailed(
                        "note XML end tag does not match its start tag".into(),
                    ));
                }
                finish_note_element(&frame)?;
                if matches!(local_name, "del" | "moveFrom") {
                    deleted_depth = deleted_depth.saturating_sub(1);
                }
                if stack.is_empty() {
                    root_closed = true;
                }
            }
            Event::Text(value) => {
                let current = stack.last().map(|frame| frame.local_name.as_str());
                if matches!(current, Some("t")) {
                    if current_note_is_special(&stack) {
                        return Err(unsupported_note_construct("text in a separator note"));
                    }
                    if deleted_depth == 0 {
                        let value =
                            quick_xml::escape::unescape(value.as_ref()).map_err(|error| {
                                PocError::SemanticExtractionFailed(format!(
                                    "note text entity decode failed: {error}"
                                ))
                            })?;
                        values.push(value.into_owned());
                    }
                } else if matches!(current, Some("delText")) {
                    if deleted_depth == 0 {
                        return Err(unsupported_note_construct("delText outside a deletion"));
                    }
                } else if !value.as_ref().chars().all(char::is_whitespace) {
                    return Err(unsupported_note_construct("text outside w:t"));
                }
            }
            Event::CData(value) => {
                let current = stack.last().map(|frame| frame.local_name.as_str());
                if matches!(current, Some("t")) {
                    if current_note_is_special(&stack) {
                        return Err(unsupported_note_construct("CDATA in a separator note"));
                    }
                    if deleted_depth == 0 {
                        let value = value.as_ref();
                        values.push(value.to_owned());
                    }
                } else if matches!(current, Some("delText")) {
                    if deleted_depth == 0 {
                        return Err(unsupported_note_construct("delText outside a deletion"));
                    }
                } else if !value.as_ref().chars().all(char::is_whitespace) {
                    return Err(unsupported_note_construct("CDATA outside w:t"));
                }
            }
            Event::GeneralRef(reference) => {
                let current = stack.last().map(|frame| frame.local_name.as_str());
                if matches!(current, Some("t")) {
                    if current_note_is_special(&stack) {
                        return Err(unsupported_note_construct("entity in a separator note"));
                    }
                    if deleted_depth == 0 && !current_note_is_special(&stack) {
                        let name = reference.as_ref();
                        let reference = format!("&{name};");
                        let value = quick_xml::escape::unescape(&reference).map_err(|error| {
                            PocError::SemanticExtractionFailed(format!(
                                "note text entity decode failed: {error}"
                            ))
                        })?;
                        values.push(value.into_owned());
                    }
                } else if matches!(current, Some("delText")) {
                    if deleted_depth == 0 {
                        return Err(unsupported_note_construct(
                            "delText reference outside a deletion",
                        ));
                    }
                } else {
                    return Err(unsupported_note_construct("entity outside w:t"));
                }
            }
            Event::DocType(_) => return Err(unsupported_note_construct("DOCTYPE")),
            Event::Eof => break,
            _ => {}
        }
    }

    if !root_seen || !root_closed || !stack.is_empty() {
        return Err(PocError::SemanticExtractionFailed(
            "note XML did not contain one complete root element".into(),
        ));
    }
    Ok(normalize_text(&values.join("")))
}

fn note_element_name<'a>(
    namespace: &ResolveResult<'_>,
    local_name: &'a str,
) -> Result<&'a str, PocError> {
    match namespace {
        ResolveResult::Bound(namespace) if namespace.as_ref() == WORDPROCESSINGML_NS => {
            Ok(local_name)
        }
        ResolveResult::Unknown(prefix) => Err(unsupported_note_construct(&format!(
            "unbound namespace prefix {prefix}"
        ))),
        _ => Err(unsupported_note_construct(local_name)),
    }
}

fn validate_note_element(
    local_name: &str,
    event: &BytesStart<'_>,
    resolver: &quick_xml::name::NamespaceResolver,
    expected_root: &str,
    stack: &mut [NoteXmlFrame],
) -> Result<NoteXmlFrame, PocError> {
    let Some(parent_index) = stack.len().checked_sub(1) else {
        if local_name != expected_root {
            return Err(unsupported_note_construct(local_name));
        }
        return Ok(NoteXmlFrame {
            local_name: local_name.to_owned(),
            ..NoteXmlFrame::default()
        });
    };

    let parent_name = stack[parent_index].local_name.clone();
    let note_element = if expected_root == "footnotes" {
        "footnote"
    } else {
        "endnote"
    };
    if parent_name == expected_root {
        if local_name != note_element {
            return Err(unsupported_note_construct(local_name));
        }
        let note_type = namespaced_attribute(event, resolver, WORDPROCESSINGML_NS, "type")?;
        if note_type
            .as_deref()
            .is_some_and(|value| !matches!(value, "separator" | "continuationSeparator"))
        {
            return Err(unsupported_note_construct("unknown note type"));
        }
        return Ok(NoteXmlFrame {
            local_name: local_name.to_owned(),
            note_type,
            ..NoteXmlFrame::default()
        });
    }

    if matches!(parent_name.as_str(), "footnote" | "endnote") {
        if local_name != "p" {
            return Err(unsupported_note_construct(local_name));
        }
        stack[parent_index].paragraph_count += 1;
        if stack[parent_index].paragraph_count > 1 {
            return Err(unsupported_note_construct("multiple note paragraphs"));
        }
        return Ok(NoteXmlFrame {
            local_name: local_name.to_owned(),
            ..NoteXmlFrame::default()
        });
    }

    match parent_name.as_str() {
        "p" => match local_name {
            "pPr"
                if !stack[parent_index].properties_seen && !stack[parent_index].content_started =>
            {
                stack[parent_index].properties_seen = true;
            }
            "r" => {
                stack[parent_index].content_started = true;
            }
            "proofErr" | "bookmarkStart" | "bookmarkEnd" => {
                stack[parent_index].content_started = true;
            }
            _ => return Err(unsupported_note_construct(local_name)),
        },
        "r" => match local_name {
            "rPr"
                if !stack[parent_index].properties_seen && !stack[parent_index].content_started =>
            {
                stack[parent_index].properties_seen = true;
            }
            "t" | "delText" | "tab" | "br" | "cr" | "noBreakHyphen" | "softHyphen" => {
                if local_name == "delText"
                    && !stack
                        .iter()
                        .any(|frame| matches!(frame.local_name.as_str(), "del" | "moveFrom"))
                {
                    return Err(unsupported_note_construct("delText outside a deletion"));
                }
                stack[parent_index].content_started = true;
            }
            "separator" | "continuationSeparator" => {
                let note_index = stack
                    .iter()
                    .rposition(|frame| matches!(frame.local_name.as_str(), "footnote" | "endnote"))
                    .ok_or_else(|| unsupported_note_construct(local_name))?;
                if stack[note_index].note_type.as_deref() != Some(local_name) {
                    return Err(unsupported_note_construct(local_name));
                }
                stack[note_index].separator_count += 1;
                stack[parent_index].content_started = true;
            }
            "footnoteRef" | "endnoteRef" => {
                let expected_ref = if expected_root == "footnotes" {
                    "footnoteRef"
                } else {
                    "endnoteRef"
                };
                if local_name != expected_ref || current_note_is_special(stack) {
                    return Err(unsupported_note_construct(local_name));
                }
                stack[parent_index].content_started = true;
            }
            _ => return Err(unsupported_note_construct(local_name)),
        },
        "pPr" => {
            if !is_note_paragraph_formatting(local_name) {
                return Err(unsupported_note_construct(local_name));
            }
            if local_name == "pStyle" {
                let style = namespaced_attribute(event, resolver, WORDPROCESSINGML_NS, "val")?;
                let expected_style = if expected_root == "footnotes" {
                    "FootnoteText"
                } else {
                    "EndnoteText"
                };
                if style.as_deref() != Some(expected_style) {
                    return Err(unsupported_note_construct(
                        "nonstandard note paragraph style",
                    ));
                }
            }
        }
        "rPr" => {
            if local_name == "rStyle" {
                let style = namespaced_attribute(event, resolver, WORDPROCESSINGML_NS, "val")?;
                let expected_style = if expected_root == "footnotes" {
                    "FootnoteReference"
                } else {
                    "EndnoteReference"
                };
                if style.as_deref() != Some(expected_style) {
                    return Err(unsupported_note_construct("nonstandard note run style"));
                }
            } else if !is_note_run_formatting(local_name) {
                return Err(unsupported_note_construct(local_name));
            }
        }
        "t" | "delText" => return Err(unsupported_note_construct(local_name)),
        _ => return Err(unsupported_note_construct(local_name)),
    }

    Ok(NoteXmlFrame {
        local_name: local_name.to_owned(),
        ..NoteXmlFrame::default()
    })
}

fn finish_note_element(frame: &NoteXmlFrame) -> Result<(), PocError> {
    if matches!(frame.local_name.as_str(), "footnote" | "endnote") {
        if frame.paragraph_count != 1 {
            return Err(unsupported_note_construct("note without one paragraph"));
        }
        match frame.note_type.as_deref() {
            Some("separator" | "continuationSeparator") if frame.separator_count != 1 => {
                return Err(unsupported_note_construct("malformed note separator"));
            }
            None if frame.separator_count != 0 => {
                return Err(unsupported_note_construct("separator in a text note"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn append_note_control_character(
    local_name: &str,
    deleted_depth: usize,
    stack: &[NoteXmlFrame],
    values: &mut Vec<String>,
) -> Result<(), PocError> {
    let character = match local_name {
        "tab" | "br" | "cr" => " ",
        "noBreakHyphen" => "\u{2011}",
        "softHyphen" => "\u{00ad}",
        _ => return Ok(()),
    };
    if current_note_is_special(stack) {
        return Err(unsupported_note_construct(
            "text control in a separator note",
        ));
    }
    if deleted_depth == 0 {
        values.push(character.to_owned());
    }
    Ok(())
}

fn current_note_is_special(stack: &[NoteXmlFrame]) -> bool {
    stack
        .iter()
        .rev()
        .find(|frame| matches!(frame.local_name.as_str(), "footnote" | "endnote"))
        .is_some_and(|frame| frame.note_type.is_some())
}

fn is_note_paragraph_formatting(local_name: &str) -> bool {
    matches!(
        local_name,
        "pStyle"
            | "keepNext"
            | "keepLines"
            | "pageBreakBefore"
            | "widowControl"
            | "suppressLineNumbers"
            | "suppressAutoHyphens"
            | "kinsoku"
            | "wordWrap"
            | "overflowPunct"
            | "topLinePunct"
            | "autoSpaceDE"
            | "autoSpaceDN"
            | "bidi"
            | "adjustRightInd"
            | "snapToGrid"
            | "spacing"
            | "ind"
            | "contextualSpacing"
            | "mirrorIndents"
            | "suppressOverlap"
            | "jc"
            | "textAlignment"
            | "textboxTightWrap"
    )
}

fn is_note_run_formatting(local_name: &str) -> bool {
    matches!(
        local_name,
        "rFonts"
            | "b"
            | "bCs"
            | "i"
            | "iCs"
            | "caps"
            | "smallCaps"
            | "strike"
            | "dstrike"
            | "outline"
            | "shadow"
            | "emboss"
            | "imprint"
            | "noProof"
            | "snapToGrid"
            | "color"
            | "spacing"
            | "w"
            | "kern"
            | "position"
            | "sz"
            | "szCs"
            | "highlight"
            | "u"
            | "effect"
            | "bdr"
            | "shd"
            | "fitText"
            | "vertAlign"
            | "rtl"
            | "cs"
            | "em"
            | "lang"
            | "eastAsianLayout"
    )
}

fn unsupported_note_construct(name: &str) -> PocError {
    PocError::UnsupportedSemanticConstruct(format!(
        "note construct {name} is outside the text-only grammar"
    ))
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
                let value = quick_xml::escape::unescape(value.as_ref()).map_err(|error| {
                    PocError::SemanticExtractionFailed(format!(
                        "text entity decode failed: {error}"
                    ))
                })?;
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
    if target.starts_with('/')
        || target.contains('\\')
        || target.split('/').any(|part| part == "..")
    {
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
    let comment_anchors = parse_comment_anchor_paragraphs(document)?;
    let comments = parse_comments(
        parts.get("word/comments.xml").map(Vec::as_slice),
        parts.get("word/commentsExtended.xml").map(Vec::as_slice),
        &comment_anchors,
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

fn parse_comment_anchor_paragraphs(document: &[u8]) -> Result<BTreeMap<String, String>, PocError> {
    let text = std::str::from_utf8(document)
        .map_err(|_| PocError::SemanticExtractionFailed("document XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut current_para_id: Option<String> = None;
    let mut anchors = BTreeMap::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.local_name().as_ref() == "p" => {
                current_para_id = attr(&event, "paraId")?;
            }
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "commentRangeStart" =>
            {
                if let (Some(comment_id), Some(para_id)) =
                    (attr(&event, "id")?, current_para_id.as_ref())
                {
                    anchors.insert(comment_id, para_id.clone());
                }
            }
            Ok(Event::End(event)) if event.local_name().as_ref() == "p" => {
                current_para_id = None;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "comment anchor XML parse failed: {error}"
                )));
            }
        }
    }
    Ok(anchors)
}

fn parse_comments(
    comments_data: Option<&[u8]>,
    extended_data: Option<&[u8]>,
    anchor_paragraphs: &BTreeMap<String, String>,
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
                        let comment_id = comment
                            .id
                            .clone()
                            .unwrap_or_else(|| comments.len().to_string());
                        let para_id = comment
                            .para_id
                            .clone()
                            .or_else(|| anchor_paragraphs.get(&comment_id).cloned());
                        let resolved = para_id
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
                            source_locator: format!("word/comments.xml#comment:{comment_id}"),
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

fn parse_comment_resolution(data: Option<&[u8]>) -> Result<BTreeMap<String, bool>, PocError> {
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

fn image_semantic_digest(bytes: &[u8], budget: &mut ImageBudget) -> Result<String, PocError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    let image_count = budget
        .count
        .checked_add(1)
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    if image_count > MAX_DOCX_IMAGES {
        return Err(PocError::InspectionResourceLimitExceeded);
    }
    budget.count = image_count;

    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err(PocError::UnsupportedSemanticConstruct(
            "DOCX image is not a supported PNG".into(),
        ));
    }

    // Inspect the complete chunk stream before asking png to allocate image buffers.
    // This bounds dimensions and rejects chunks whose display semantics are not in v0.
    let header = preflight_png(bytes)?;
    let pixels = u64::from(header.width)
        .checked_mul(u64::from(header.height))
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    let total_pixels = budget
        .decoded_pixels
        .checked_add(pixels)
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    if total_pixels > MAX_DOCX_DECODED_PIXELS {
        return Err(PocError::InspectionResourceLimitExceeded);
    }
    let output_bound = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    let total_output = budget
        .decoded_output_bytes
        .checked_add(output_bound)
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    if total_output > MAX_DOCX_DECODED_OUTPUT_BYTES {
        return Err(PocError::InspectionResourceLimitExceeded);
    }
    budget.decoded_pixels = total_pixels;
    budget.decoded_output_bytes = total_output;

    if header.color_type == 3 {
        validate_indexed_png_pixels(bytes, &header)?;
    }

    let normalized_png = normalize_png_transparency(bytes, &header)?;
    let decoder_bytes = normalized_png.as_deref().unwrap_or(bytes);
    let mut options = png::DecodeOptions::default();
    options.set_ignore_checksums(false);
    options.set_ignore_text_chunk(true);
    options.set_skip_ancillary_crc_failures(false);

    let mut decoder = png::Decoder::new_with_options(Cursor::new(decoder_bytes), options);
    // Keep png's documented 64 MiB temporary-allocation ceiling; the separately
    // checked frame output is bounded by the frozen DOCX pixel limit below.
    decoder.set_limits(png::Limits {
        bytes: 64 * 1024 * 1024,
    });
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(png_decode_error)?;

    let image_info = reader.info();
    if image_info.width != header.width
        || image_info.height != header.height
        || image_info.animation_control.is_some()
    {
        return Err(PocError::SemanticExtractionFailed(
            "PNG decoder metadata differs from strict preflight".into(),
        ));
    }

    let (output_color_type, output_bit_depth) = reader.output_color_type();
    if output_bit_depth != png::BitDepth::Eight {
        return Err(PocError::UnsupportedSemanticConstruct(
            "PNG decoder produced a non-8-bit pixel format".into(),
        ));
    }
    let channels = match output_color_type {
        png::ColorType::Grayscale => 1usize,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => {
            return Err(PocError::UnsupportedSemanticConstruct(
                "PNG palette was not expanded by the decoder".into(),
            ));
        }
    };
    let expected_output_size = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    let output_size = reader
        .output_buffer_size()
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    if output_size != expected_output_size || output_size > output_bound {
        return Err(PocError::InspectionResourceLimitExceeded);
    }

    let mut decoded = Vec::new();
    decoded
        .try_reserve_exact(output_size)
        .map_err(|_| PocError::InspectionResourceLimitExceeded)?;
    decoded.resize(output_size, 0);
    let output_info = reader.next_frame(&mut decoded).map_err(png_decode_error)?;
    let decoded = &decoded[..output_info.buffer_size()];
    if output_info.width != header.width
        || output_info.height != header.height
        || output_info.color_type != output_color_type
        || output_info.bit_depth != png::BitDepth::Eight
        || decoded.len() != expected_output_size
    {
        return Err(PocError::SemanticExtractionFailed(
            "PNG decoded frame differs from preflight dimensions or format".into(),
        ));
    }
    reader.finish().map_err(png_decode_error)?;

    let mut digest = Sha256::new();
    digest.update(b"docx-png-rgba8-v1\0");
    digest.update(header.width.to_be_bytes());
    digest.update(header.height.to_be_bytes());
    if let Some(background) = header.metadata.background {
        hash_png_semantic_chunk(&mut digest, b"bKGD", &background)?;
    }
    if let Some(physical_dimensions) = header.metadata.physical_dimensions {
        hash_png_semantic_chunk(&mut digest, b"pHYs", &physical_dimensions)?;
    }
    hash_rgba8_pixels(&mut digest, output_color_type, decoded)?;
    Ok(hex::encode(digest.finalize()))
}

fn preflight_png(bytes: &[u8]) -> Result<PngHeader, PocError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err(PocError::UnsupportedSemanticConstruct(
            "DOCX image is not a supported PNG".into(),
        ));
    }

    let mut offset = PNG_SIGNATURE.len();
    let mut header = None;
    let mut saw_idat = false;
    let mut ended_idat = false;
    let mut saw_iend = false;
    let mut saw_palette = false;
    let mut saw_transparency = false;
    let mut saw_background = false;
    let mut saw_physical_dimensions = false;

    while offset < bytes.len() {
        if bytes.len() - offset < 12 {
            return Err(PocError::SemanticExtractionFailed(
                "truncated PNG chunk".into(),
            ));
        }
        let length = usize::try_from(u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| PocError::SemanticExtractionFailed("invalid PNG length".into()))?,
        ))
        .map_err(|_| PocError::InspectionResourceLimitExceeded)?;
        let data_start = offset
            .checked_add(8)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
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
        let data = &bytes[data_start..data_end];
        if !chunk_type.iter().all(u8::is_ascii_alphabetic) || chunk_type[2].is_ascii_lowercase() {
            return Err(PocError::SemanticExtractionFailed(
                "invalid PNG chunk type".into(),
            ));
        }
        if header.is_none() && (chunk_type != b"IHDR" || offset != PNG_SIGNATURE.len()) {
            return Err(PocError::SemanticExtractionFailed(
                "PNG IHDR must be the first chunk".into(),
            ));
        }
        if header.is_some() && chunk_type == b"IHDR" {
            return Err(PocError::SemanticExtractionFailed(
                "PNG has multiple IHDR chunks".into(),
            ));
        }
        if saw_idat && chunk_type != b"IDAT" {
            ended_idat = true;
        }
        if chunk_type == b"IDAT" && ended_idat {
            return Err(PocError::SemanticExtractionFailed(
                "PNG IDAT chunks are not contiguous".into(),
            ));
        }

        match chunk_type {
            b"IHDR" => {
                if length != 13 {
                    return Err(PocError::SemanticExtractionFailed(
                        "PNG IHDR must contain 13 bytes".into(),
                    ));
                }
                let width =
                    u32::from_be_bytes(data[0..4].try_into().map_err(|_| {
                        PocError::SemanticExtractionFailed("invalid PNG width".into())
                    })?);
                let height = u32::from_be_bytes(data[4..8].try_into().map_err(|_| {
                    PocError::SemanticExtractionFailed("invalid PNG height".into())
                })?);
                let bit_depth = data[8];
                let color_type = data[9];
                if width == 0 || height == 0 || data[10] != 0 || data[11] != 0 || data[12] > 1 {
                    return Err(PocError::SemanticExtractionFailed(
                        "invalid PNG dimensions or IHDR method".into(),
                    ));
                }
                let valid_depth = match color_type {
                    0 => matches!(bit_depth, 1 | 2 | 4 | 8),
                    2 => bit_depth == 8,
                    3 => matches!(bit_depth, 1 | 2 | 4 | 8),
                    4 | 6 => bit_depth == 8,
                    _ => false,
                };
                if !valid_depth {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "unsupported PNG color type or bit depth".into(),
                    ));
                }
                header = Some(PngHeader {
                    width,
                    height,
                    color_type,
                    bit_depth,
                    palette_entries: 0,
                    palette: Vec::new(),
                    metadata: PngMetadata::default(),
                });
            }
            b"PLTE" => {
                let image_header = header.as_mut().ok_or_else(|| {
                    PocError::SemanticExtractionFailed("PNG PLTE precedes IHDR".into())
                })?;
                if saw_idat
                    || saw_palette
                    || saw_transparency
                    || saw_background
                    || matches!(image_header.color_type, 0 | 4)
                    || length == 0
                    || length > 768
                    || length % 3 != 0
                {
                    return Err(PocError::SemanticExtractionFailed(
                        "invalid PNG palette chunk".into(),
                    ));
                }
                let palette_entries = length / 3;
                if image_header.color_type == 3
                    && palette_entries > (1usize << image_header.bit_depth)
                {
                    return Err(PocError::SemanticExtractionFailed(
                        "indexed PNG palette exceeds its bit-depth range".into(),
                    ));
                }
                saw_palette = true;
                image_header.palette_entries = palette_entries;
                image_header.palette = data
                    .chunks_exact(3)
                    .map(|entry| [entry[0], entry[1], entry[2]])
                    .collect();
            }
            b"IDAT" => {
                let image_header = header.as_ref().ok_or_else(|| {
                    PocError::SemanticExtractionFailed("PNG IDAT precedes IHDR".into())
                })?;
                if image_header.color_type == 3 && !saw_palette {
                    return Err(PocError::SemanticExtractionFailed(
                        "indexed PNG has no palette".into(),
                    ));
                }
                saw_idat = true;
            }
            b"IEND" => {
                if !saw_idat || length != 0 || chunk_end != bytes.len() {
                    return Err(PocError::SemanticExtractionFailed(
                        "invalid PNG IEND or trailing bytes".into(),
                    ));
                }
                saw_iend = true;
                offset = chunk_end;
                break;
            }
            b"tRNS" => {
                let image_header = header.as_ref().ok_or_else(|| {
                    PocError::SemanticExtractionFailed("PNG tRNS precedes IHDR".into())
                })?;
                let valid_length = match image_header.color_type {
                    0 => length == 2,
                    2 => length == 6,
                    3 => saw_palette && length > 0 && length <= image_header.palette_entries,
                    _ => false,
                };
                if saw_idat || saw_transparency || !valid_length {
                    return Err(PocError::SemanticExtractionFailed(
                        "invalid PNG transparency chunk".into(),
                    ));
                }
                // For sub-16-bit grayscale/truecolor images, PNG requires a decoder
                // to mask unused high sample bits before comparing the transparency key.
                // The decoder input is normalized to that effective key below.
                saw_transparency = true;
            }
            b"bKGD" => {
                let image_header = header.as_mut().ok_or_else(|| {
                    PocError::SemanticExtractionFailed("PNG bKGD precedes IHDR".into())
                })?;
                let valid_length = match image_header.color_type {
                    0 | 4 => length == 2,
                    2 | 6 => length == 6,
                    3 => {
                        saw_palette
                            && length == 1
                            && usize::from(data[0]) < image_header.palette_entries
                    }
                    _ => false,
                };
                if saw_idat || saw_background || !valid_length || length > 6 {
                    return Err(PocError::SemanticExtractionFailed(
                        "invalid PNG background chunk".into(),
                    ));
                }
                let background = match image_header.color_type {
                    0 | 4 => {
                        let sample = u16::from_be_bytes([data[0], data[1]])
                            & png_sample_mask(image_header.bit_depth);
                        let gray = png_sample_to_u8(sample, image_header.bit_depth);
                        [gray; 3]
                    }
                    2 | 6 => {
                        let mut rgb = [0; 3];
                        for (channel, sample_bytes) in data.chunks_exact(2).enumerate() {
                            let sample = u16::from_be_bytes([sample_bytes[0], sample_bytes[1]])
                                & png_sample_mask(image_header.bit_depth);
                            rgb[channel] = png_sample_to_u8(sample, image_header.bit_depth);
                        }
                        rgb
                    }
                    3 => image_header
                        .palette
                        .get(usize::from(data[0]))
                        .copied()
                        .ok_or_else(|| {
                            PocError::SemanticExtractionFailed(
                                "indexed PNG background is outside its palette".into(),
                            )
                        })?,
                    _ => {
                        return Err(PocError::SemanticExtractionFailed(
                            "invalid PNG background color type".into(),
                        ));
                    }
                };
                image_header.metadata.background = Some(background);
                saw_background = true;
            }
            b"pHYs" => {
                let image_header = header.as_mut().ok_or_else(|| {
                    PocError::SemanticExtractionFailed("PNG pHYs precedes IHDR".into())
                })?;
                if saw_idat || saw_physical_dimensions || length != 9 || data[8] > 1 {
                    return Err(PocError::SemanticExtractionFailed(
                        "invalid PNG physical-dimensions chunk".into(),
                    ));
                }
                let mut physical_dimensions: [u8; 9] = data.try_into().map_err(|_| {
                    PocError::SemanticExtractionFailed("invalid PNG physical dimensions".into())
                })?;
                let pixels_per_unit_x =
                    u32::from_be_bytes(physical_dimensions[..4].try_into().map_err(|_| {
                        PocError::SemanticExtractionFailed("invalid PNG pHYs X value".into())
                    })?);
                let pixels_per_unit_y =
                    u32::from_be_bytes(physical_dimensions[4..8].try_into().map_err(|_| {
                        PocError::SemanticExtractionFailed("invalid PNG pHYs Y value".into())
                    })?);
                if pixels_per_unit_x == 0 || pixels_per_unit_y == 0 {
                    return Err(PocError::SemanticExtractionFailed(
                        "PNG pHYs dimensions must be nonzero".into(),
                    ));
                }
                if physical_dimensions[8] == 0 {
                    let divisor = greatest_common_divisor(pixels_per_unit_x, pixels_per_unit_y);
                    physical_dimensions[..4]
                        .copy_from_slice(&(pixels_per_unit_x / divisor).to_be_bytes());
                    physical_dimensions[4..8]
                        .copy_from_slice(&(pixels_per_unit_y / divisor).to_be_bytes());
                }
                image_header.metadata.physical_dimensions = Some(physical_dimensions);
                saw_physical_dimensions = true;
            }
            b"tEXt" | b"zTXt" | b"iTXt" => {}
            b"tIME" if length == 7 => {}
            b"tIME" => {
                return Err(PocError::SemanticExtractionFailed(
                    "invalid PNG modification-time chunk".into(),
                ));
            }
            b"acTL" | b"fcTL" | b"fdAT" => {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "animated PNG is unsupported".into(),
                ));
            }
            b"gAMA" | b"cHRM" | b"iCCP" | b"sRGB" | b"sBIT" | b"cICP" | b"mDCV" | b"cLLI" => {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "PNG color profile or HDR metadata is unsupported".into(),
                ));
            }
            b"eXIf" => {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "PNG EXIF orientation metadata is unsupported".into(),
                ));
            }
            _ if chunk_type[0].is_ascii_uppercase() => {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "unknown critical PNG chunk".into(),
                ));
            }
            _ => {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "unknown ancillary PNG chunk".into(),
                ));
            }
        }
        offset = chunk_end;
    }

    if !saw_iend || offset != bytes.len() || !saw_idat {
        return Err(PocError::SemanticExtractionFailed(
            "PNG is missing a complete IDAT/IEND sequence".into(),
        ));
    }
    header.ok_or_else(|| PocError::SemanticExtractionFailed("PNG has no IHDR".into()))
}

fn validate_indexed_png_pixels(bytes: &[u8], header: &PngHeader) -> Result<(), PocError> {
    let mut options = png::DecodeOptions::default();
    options.set_ignore_checksums(false);
    options.set_ignore_text_chunk(true);
    options.set_skip_ancillary_crc_failures(false);

    let mut decoder = png::Decoder::new_with_options(Cursor::new(bytes), options);
    decoder.set_limits(png::Limits {
        bytes: 64 * 1024 * 1024,
    });
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(png_decode_error)?;

    let (color_type, bit_depth) = reader.output_color_type();
    if color_type != png::ColorType::Indexed
        || bit_depth as u8 != header.bit_depth
        || reader.info().width != header.width
        || reader.info().height != header.height
    {
        return Err(PocError::SemanticExtractionFailed(
            "PNG indexed preflight metadata differs from decoder".into(),
        ));
    }

    let width =
        usize::try_from(header.width).map_err(|_| PocError::InspectionResourceLimitExceeded)?;
    let height =
        usize::try_from(header.height).map_err(|_| PocError::InspectionResourceLimitExceeded)?;
    let bit_depth = usize::from(header.bit_depth);
    let samples_per_byte = 8 / bit_depth;
    let row_bytes = width
        .checked_add(samples_per_byte - 1)
        .and_then(|bits| bits.checked_mul(bit_depth))
        .and_then(|bits| bits.checked_div(8))
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    let expected_size = row_bytes
        .checked_mul(height)
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    if reader.output_buffer_size() != Some(expected_size) {
        return Err(PocError::InspectionResourceLimitExceeded);
    }

    let mut decoded = Vec::new();
    decoded
        .try_reserve_exact(expected_size)
        .map_err(|_| PocError::InspectionResourceLimitExceeded)?;
    decoded.resize(expected_size, 0);
    let output_info = reader.next_frame(&mut decoded).map_err(png_decode_error)?;
    if output_info.width != header.width
        || output_info.height != header.height
        || output_info.color_type != png::ColorType::Indexed
        || output_info.bit_depth as u8 != header.bit_depth
        || output_info.buffer_size() != expected_size
    {
        return Err(PocError::SemanticExtractionFailed(
            "PNG indexed frame differs from strict preflight".into(),
        ));
    }
    reader.finish().map_err(png_decode_error)?;

    let decoded = &decoded[..output_info.buffer_size()];
    let index_mask = ((1u16 << header.bit_depth) - 1) as u8;
    for row in decoded.chunks_exact(row_bytes) {
        for x in 0..width {
            let within_byte = x % samples_per_byte;
            let shift = (samples_per_byte - within_byte - 1) * bit_depth;
            let palette_index = usize::from((row[x / samples_per_byte] >> shift) & index_mask);
            if palette_index >= header.palette_entries {
                return Err(PocError::SemanticExtractionFailed(
                    "indexed PNG pixel is outside its palette".into(),
                ));
            }
        }
    }
    Ok(())
}

fn normalize_png_transparency(
    bytes: &[u8],
    header: &PngHeader,
) -> Result<Option<Vec<u8>>, PocError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !matches!(header.color_type, 0 | 2) {
        return Ok(None);
    }

    let mut offset = PNG_SIGNATURE.len();
    while offset < bytes.len() {
        let length = usize::try_from(u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| PocError::SemanticExtractionFailed("invalid PNG length".into()))?,
        ))
        .map_err(|_| PocError::InspectionResourceLimitExceeded)?;
        let data_start = offset
            .checked_add(8)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        let data_end = data_start
            .checked_add(length)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        let chunk_type = &bytes[offset + 4..offset + 8];
        if chunk_type == b"tRNS" {
            let data = &bytes[data_start..data_end];
            let stored_crc = u32::from_be_bytes(
                bytes[data_end..chunk_end]
                    .try_into()
                    .map_err(|_| PocError::SemanticExtractionFailed("invalid PNG CRC".into()))?,
            );
            if png_chunk_crc32(chunk_type, data) != stored_crc {
                return Err(PocError::SemanticExtractionFailed(
                    "PNG transparency chunk checksum mismatch".into(),
                ));
            }

            let sample_mask = png_sample_mask(header.bit_depth);
            let mut normalized = Vec::with_capacity(data.len());
            for encoded_sample in data.chunks_exact(2) {
                let sample =
                    u16::from_be_bytes([encoded_sample[0], encoded_sample[1]]) & sample_mask;
                normalized.extend_from_slice(&sample.to_be_bytes());
            }
            if normalized == data {
                return Ok(None);
            }

            let mut output = Vec::new();
            output
                .try_reserve_exact(bytes.len())
                .map_err(|_| PocError::InspectionResourceLimitExceeded)?;
            output.extend_from_slice(bytes);
            output[data_start..data_end].copy_from_slice(&normalized);
            let normalized_crc = png_chunk_crc32(chunk_type, &normalized);
            output[data_end..chunk_end].copy_from_slice(&normalized_crc.to_be_bytes());
            return Ok(Some(output));
        }
        offset = chunk_end;
    }
    Ok(None)
}

fn png_chunk_crc32(chunk_type: &[u8], data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in chunk_type.iter().chain(data) {
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

fn png_sample_mask(bit_depth: u8) -> u16 {
    ((1u32 << bit_depth) - 1) as u16
}

fn png_sample_to_u8(sample: u16, bit_depth: u8) -> u8 {
    let max_sample = png_sample_mask(bit_depth);
    let normalized = (u32::from(sample & max_sample) * u32::from(u8::MAX)
        + u32::from(max_sample) / 2)
        / u32::from(max_sample);
    normalized as u8
}

fn greatest_common_divisor(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn hash_rgba8_pixels(
    digest: &mut Sha256,
    color_type: png::ColorType,
    pixels: &[u8],
) -> Result<(), PocError> {
    if color_type == png::ColorType::Rgba {
        digest.update(pixels);
        return Ok(());
    }
    let source_channels = match color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => {
            return Err(PocError::UnsupportedSemanticConstruct(
                "indexed PNG pixels were not expanded".into(),
            ));
        }
    };
    if !pixels.len().is_multiple_of(source_channels) {
        return Err(PocError::SemanticExtractionFailed(
            "PNG decoded pixel buffer has an incomplete pixel".into(),
        ));
    }

    const PIXELS_PER_HASH_BATCH: usize = 1_024;
    let mut rgba = [0u8; PIXELS_PER_HASH_BATCH * 4];
    let mut output_len = 0usize;
    for pixel in pixels.chunks_exact(source_channels) {
        let channels = &mut rgba[output_len..output_len + 4];
        match color_type {
            png::ColorType::Grayscale => {
                channels[..3].fill(pixel[0]);
                channels[3] = u8::MAX;
            }
            png::ColorType::GrayscaleAlpha => {
                channels[..3].fill(pixel[0]);
                channels[3] = pixel[1];
            }
            png::ColorType::Rgb => {
                channels[..3].copy_from_slice(pixel);
                channels[3] = u8::MAX;
            }
            png::ColorType::Rgba => {
                return Err(PocError::SemanticExtractionFailed(
                    "unexpected RGBA branch during PNG canonicalization".into(),
                ));
            }
            png::ColorType::Indexed => {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "indexed PNG pixels were not expanded".into(),
                ));
            }
        }
        output_len += 4;
        if output_len == rgba.len() {
            digest.update(rgba);
            output_len = 0;
        }
    }
    if output_len > 0 {
        digest.update(&rgba[..output_len]);
    }
    Ok(())
}

fn hash_png_semantic_chunk(
    digest: &mut Sha256,
    kind: &[u8; 4],
    data: &[u8],
) -> Result<(), PocError> {
    digest.update(kind);
    let length =
        u64::try_from(data.len()).map_err(|_| PocError::InspectionResourceLimitExceeded)?;
    digest.update(length.to_be_bytes());
    digest.update(data);
    Ok(())
}

fn png_decode_error(error: png::DecodingError) -> PocError {
    PocError::SemanticExtractionFailed(format!("PNG decoding failed: {error}"))
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
        let matches = key == name || key.rsplit_once(':').is_some_and(|(_, local)| local == name);
        if matches {
            let value = quick_xml::escape::unescape(item.value.as_ref()).map_err(|error| {
                PocError::SemanticExtractionFailed(format!(
                    "attribute entity decode failed: {error}"
                ))
            })?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}
