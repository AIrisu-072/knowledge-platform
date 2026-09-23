use crate::{
    canonical_json_bytes, AdapterOutput, CapabilityEvidence, CommentEvidence, EditorialEvidence,
    ExternalDependency, FormatId, InspectionAdapter, InspectionProfile, PocError,
};
use office_oxide::pptx::{
    BulletStyle, GraphicContent, HyperlinkTarget, PptxDocument, Shape, TextBody, TextContent,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use zip::ZipArchive;

const PRESENTATION_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_TOTAL_UNCOMPRESSED: u64 = 512 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 256;

#[derive(Debug, Clone, Copy, Default)]
pub struct PptxAdapter;

#[derive(Debug, Clone)]
struct Relationship {
    kind: String,
    target: String,
    external: bool,
}

#[derive(Debug)]
struct PackageInspection {
    parts: BTreeMap<String, Vec<u8>>,
    relationships: BTreeMap<String, BTreeMap<String, Relationship>>,
}

impl InspectionAdapter for PptxAdapter {
    fn format(&self) -> FormatId {
        FormatId::Pptx
    }

    fn inspect(
        &self,
        input: &[u8],
        _profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError> {
        if !is_pptx_package(input) {
            return Err(PocError::FormatMismatch {
                expected: FormatId::Pptx,
                observed: None,
            });
        }

        let package = inspect_package(input)?;
        let document = PptxDocument::from_reader(Cursor::new(input))
            .map_err(|error| PocError::SemanticExtractionFailed(format!("office_oxide PPTX: {error}")))?;

        let ordered_slide_parts = ordered_slide_parts(&package)?;
        if ordered_slide_parts.len() != document.slides.len() {
            return Err(PocError::ParserDisagreement(format!(
                "slide count mismatch: raw={}, office_oxide={}",
                ordered_slide_parts.len(),
                document.slides.len()
            )));
        }

        let mut slides = Vec::with_capacity(document.slides.len());
        let mut external_dependencies = Vec::new();
        let mut editorial = EditorialEvidence::default();
        let mut visual_present = false;
        let mut notes_present = false;

        for (index, (slide, slide_part)) in document
            .slides
            .iter()
            .zip(ordered_slide_parts.iter())
            .enumerate()
        {
            let raw_slide = package.parts.get(slide_part).ok_or_else(|| {
                PocError::SemanticExtractionFailed(format!("missing slide part {slide_part}"))
            })?;
            let raw_name = slide_name(raw_slide)?;
            if raw_name != slide.name {
                return Err(PocError::ParserDisagreement(format!(
                    "slide name mismatch at {index}: raw={raw_name:?}, office_oxide={:?}",
                    slide.name
                )));
            }

            let rels = package
                .relationships
                .get(slide_part)
                .cloned()
                .unwrap_or_default();

            let raw_notes = raw_notes_text(slide_part, &rels, &package)?;
            if raw_notes.as_deref() != slide.notes.as_deref() {
                return Err(PocError::ParserDisagreement(format!(
                    "speaker-note mismatch on slide {index}: raw={raw_notes:?}, office_oxide={:?}",
                    slide.notes
                )));
            }
            if slide.notes.is_some() {
                notes_present = true;
            }

            let raw_graphics = raw_graphic_semantics(slide_part, raw_slide, &rels, &package)?;
            if !raw_graphics.is_empty() {
                visual_present = true;
            }

            let mut shape_values = Vec::with_capacity(slide.shapes.len());
            for shape in &slide.shapes {
                let value = shape_projection(shape, &mut external_dependencies, &mut visual_present)?;
                shape_values.push(value);
            }

            if !slide.comments.is_empty() {
                editorial.comments_present = true;
                for (comment_index, comment) in slide.comments.iter().enumerate() {
                    editorial.comments.push(CommentEvidence {
                        author_label: comment.author.clone(),
                        timestamp: None,
                        resolved_state: "unknown".into(),
                        source_locator: format!("slide[{index}]/comment[{comment_index}]"),
                        content: comment.text.clone(),
                    });
                }
            }

            slides.push(json!({
                "index": index,
                "name": slide.name,
                "hidden": slide.hidden,
                "shapes": shape_values,
                "raw_graphics": raw_graphics,
                "speaker_notes": slide.notes,
            }));
        }

        external_dependencies.sort_by(|left, right| {
            (left.kind.as_str(), left.definition.as_str())
                .cmp(&(right.kind.as_str(), right.definition.as_str()))
        });
        external_dependencies.dedup_by(|left, right| {
            left.kind == right.kind && left.definition == right.definition
        });

        let projection = json!({
            "slides": slides,
        });

        Ok(AdapterOutput {
            semantic_projection: canonical_json_bytes(&projection).map_err(|error| {
                PocError::InvalidWorkerResult(format!("PPTX projection serialization: {error}"))
            })?,
            capabilities: vec![
                CapabilityEvidence {
                    capability: "reader_content".into(),
                    present: true,
                },
                CapabilityEvidence {
                    capability: "presentation_structure".into(),
                    present: true,
                },
                CapabilityEvidence {
                    capability: "visual_content".into(),
                    present: visual_present,
                },
                CapabilityEvidence {
                    capability: "speaker_notes".into(),
                    present: notes_present,
                },
            ],
            editorial,
            external_dependencies,
            signatures: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

pub(crate) fn is_pptx_package(input: &[u8]) -> bool {
    let Ok(mut archive) = ZipArchive::new(Cursor::new(input)) else {
        return false;
    };
    let Ok(mut part) = archive.by_name("[Content_Types].xml") else {
        return false;
    };
    let mut content = String::new();
    part.read_to_string(&mut content).is_ok() && content.contains(PRESENTATION_MAIN)
}

fn inspect_package(input: &[u8]) -> Result<PackageInspection, PocError> {
    let mut archive = ZipArchive::new(Cursor::new(input))
        .map_err(|error| PocError::SemanticExtractionFailed(format!("PPTX ZIP: {error}")))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(PocError::InspectionResourceLimitExceeded);
    }

    let mut parts = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("PPTX entry: {error}")))?;
        let name = file.name().replace('\\', "/");
        validate_part_name(&name)?;
        if parts.contains_key(&name) {
            return Err(PocError::SemanticExtractionFailed(format!(
                "duplicate PPTX package part {name}"
            )));
        }
        total = total.saturating_add(file.size());
        if total > MAX_TOTAL_UNCOMPRESSED {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("read {name}: {error}")))?;
        if name.ends_with(".xml") || name.ends_with(".rels") {
            validate_xml(&name, &data)?;
        }
        parts.insert(name, data);
    }

    let content_types = parts.get("[Content_Types].xml").ok_or_else(|| {
        PocError::SemanticExtractionFailed("PPTX missing [Content_Types].xml".into())
    })?;
    validate_content_types(content_types)?;

    let mut relationships = BTreeMap::new();
    for (name, data) in &parts {
        if !name.ends_with(".rels") {
            continue;
        }
        let source = source_part_for_rels(name)?;
        let parsed = parse_relationships(data)?;
        for rel in parsed.values() {
            if !rel.external {
                let target = resolve_target(&source, &rel.target)?;
                if !parts.contains_key(&target) {
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "relationship target {target} from {source:?} is missing"
                    )));
                }
            }
        }
        relationships.insert(source, parsed);
    }

    Ok(PackageInspection {
        parts,
        relationships,
    })
}

fn validate_part_name(name: &str) -> Result<(), PocError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.split('/').any(|segment| segment == "..")
    {
        return Err(PocError::SemanticExtractionFailed(format!(
            "unsafe PPTX part name {name:?}"
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
        "image/jpeg",
        "image/gif",
        "image/bmp",
        PRESENTATION_MAIN,
        "application/vnd.openxmlformats-officedocument.presentationml.slide+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.handoutMaster+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml",
        "application/vnd.openxmlformats-officedocument.theme+xml",
        "application/vnd.openxmlformats-officedocument.drawingml.chart+xml",
        "application/vnd.openxmlformats-officedocument.drawingml.chartStyle+xml",
        "application/vnd.openxmlformats-officedocument.drawingml.chartColorStyle+xml",
        "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml",
        "application/vnd.openxmlformats-officedocument.drawingml.diagramLayout+xml",
        "application/vnd.openxmlformats-officedocument.drawingml.diagramStyle+xml",
        "application/vnd.openxmlformats-officedocument.drawingml.diagramColors+xml",
        "application/vnd.openxmlformats-package.core-properties+xml",
        "application/vnd.openxmlformats-officedocument.extended-properties+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.comments+xml",
        "application/vnd.openxmlformats-officedocument.presentationml.commentAuthors+xml",
    ]
    .into_iter()
    .collect();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if matches!(event.local_name().as_ref(), "Default" | "Override") =>
            {
                if let Some(value) = attr(&event, "ContentType")? {
                    if !known.contains(value.as_str()) {
                        return Err(PocError::UnsupportedSemanticConstruct(format!(
                            "unknown PPTX content type {value}"
                        )));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "content-types XML: {error}"
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
    let mut out = BTreeMap::new();

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

                if !known_relationship_type(&kind) {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "unknown PPTX relationship type {kind}"
                    )));
                }
                if external && !kind.ends_with("/hyperlink") {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "external non-hyperlink PPTX relationship {kind}"
                    )));
                }
                if out
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
                        "duplicate PPTX relationship id {id}"
                    )));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "relationship XML: {error}"
                )));
            }
        }
    }
    Ok(out)
}

fn known_relationship_type(value: &str) -> bool {
    if value
        == "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"
    {
        return true;
    }
    let Some(suffix) = value
        .strip_prefix("http://schemas.openxmlformats.org/officeDocument/2006/relationships/")
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

fn source_part_for_rels(name: &str) -> Result<String, PocError> {
    if name == "_rels/.rels" {
        return Ok(String::new());
    }
    let marker = "/_rels/";
    let Some((dir, file)) = name.rsplit_once(marker) else {
        return Err(PocError::SemanticExtractionFailed(format!(
            "invalid relationship part path {name}"
        )));
    };
    let Some(file) = file.strip_suffix(".rels") else {
        return Err(PocError::SemanticExtractionFailed(format!(
            "invalid relationship part suffix {name}"
        )));
    };
    Ok(format!("{dir}/{file}"))
}

fn resolve_target(source: &str, target: &str) -> Result<String, PocError> {
    if target.starts_with('/') || target.contains('\\') {
        return Err(PocError::SemanticExtractionFailed(format!(
            "unsafe PPTX relationship target {target:?}"
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
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "PPTX relationship escapes package root: {target:?}"
                    )));
                }
            }
            other => segments.push(other),
        }
    }
    Ok(segments.join("/"))
}

fn ordered_slide_parts(package: &PackageInspection) -> Result<Vec<String>, PocError> {
    let presentation = package.parts.get("ppt/presentation.xml").ok_or_else(|| {
        PocError::SemanticExtractionFailed("PPTX missing ppt/presentation.xml".into())
    })?;
    let rels = package
        .relationships
        .get("ppt/presentation.xml")
        .ok_or_else(|| PocError::SemanticExtractionFailed("presentation relationships missing".into()))?;

    let text = std::str::from_utf8(presentation)
        .map_err(|_| PocError::SemanticExtractionFailed("presentation XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut result = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "sldId" =>
            {
                let rid = prefixed_attr_required(&event, "r:id")?;
                let rel = rels.get(&rid).ok_or_else(|| {
                    PocError::SemanticExtractionFailed(format!("slide relationship {rid} missing"))
                })?;
                if !rel.kind.ends_with("/slide") || rel.external {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "presentation slide relationship {rid} is not an internal slide"
                    )));
                }
                result.push(resolve_target("ppt/presentation.xml", &rel.target)?);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "presentation XML: {error}"
                )));
            }
        }
    }
    Ok(result)
}

fn slide_name(data: &[u8]) -> Result<String, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("slide XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "cSld" =>
            {
                return Ok(attr(&event, "name")?.unwrap_or_default());
            }
            Ok(Event::Eof) => return Ok(String::new()),
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!("slide XML: {error}")));
            }
        }
    }
}

fn raw_notes_text(
    slide_part: &str,
    rels: &BTreeMap<String, Relationship>,
    package: &PackageInspection,
) -> Result<Option<String>, PocError> {
    let Some(rel) = rels.values().find(|rel| rel.kind.ends_with("/notesSlide")) else {
        return Ok(None);
    };
    if rel.external {
        return Err(PocError::UnsupportedSemanticConstruct(
            "external notes slide relationship".into(),
        ));
    }
    let target = resolve_target(slide_part, &rel.target)?;
    let data = package.parts.get(&target).ok_or_else(|| {
        PocError::SemanticExtractionFailed(format!("notes target {target} missing"))
    })?;
    let texts = xml_text_values(data, "t")?;
    let joined = texts.join("\n");
    Ok((!joined.is_empty()).then_some(joined))
}

fn raw_graphic_semantics(
    slide_part: &str,
    slide_data: &[u8],
    rels: &BTreeMap<String, Relationship>,
    package: &PackageInspection,
) -> Result<Vec<Value>, PocError> {
    let text = std::str::from_utf8(slide_data)
        .map_err(|_| PocError::SemanticExtractionFailed("slide XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut result = Vec::new();
    let mut frame_index = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.local_name().as_ref() == "graphicFrame" => {
                frame_index += 1;
            }
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "chart" =>
            {
                let rid = attr_required(&event, "id")?;
                let rel = rels.get(&rid).ok_or_else(|| {
                    PocError::SemanticExtractionFailed(format!("chart relationship {rid} missing"))
                })?;
                let target = resolve_target(slide_part, &rel.target)?;
                let data = package.parts.get(&target).ok_or_else(|| {
                    PocError::SemanticExtractionFailed(format!("chart target {target} missing"))
                })?;
                result.push(json!({
                    "frame_ordinal": frame_index,
                    "kind": "chart",
                    "semantic": parse_chart_semantic(data)?,
                }));
            }
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "relIds" =>
            {
                let Some(rid) = attr(&event, "dm")? else {
                    continue;
                };
                let rel = rels.get(&rid).ok_or_else(|| {
                    PocError::SemanticExtractionFailed(format!("SmartArt relationship {rid} missing"))
                })?;
                let target = resolve_target(slide_part, &rel.target)?;
                let data = package.parts.get(&target).ok_or_else(|| {
                    PocError::SemanticExtractionFailed(format!("SmartArt target {target} missing"))
                })?;
                result.push(json!({
                    "frame_ordinal": frame_index,
                    "kind": "smartart",
                    "semantic": parse_smartart_semantic(data)?,
                }));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!("slide graphic XML: {error}")));
            }
        }
    }
    Ok(result)
}

fn parse_chart_semantic(data: &[u8]) -> Result<Value, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("chart XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut section: Option<&'static str> = None;
    let mut series = Vec::<Value>::new();
    let mut current_name = Vec::<String>::new();
    let mut current_categories = Vec::<String>::new();
    let mut current_values = Vec::<String>::new();
    let mut in_series = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "ser" => {
                    if in_series {
                        return Err(PocError::SemanticExtractionFailed("nested chart series".into()));
                    }
                    in_series = true;
                    current_name.clear();
                    current_categories.clear();
                    current_values.clear();
                }
                "tx" if in_series => section = Some("name"),
                "cat" if in_series => section = Some("categories"),
                "val" if in_series => section = Some("values"),
                "v" if in_series => {
                    let value = reader
                        .read_text(event.name())
                        .map_err(|error| PocError::SemanticExtractionFailed(format!(
                            "chart value XML: {error}"
                        )))?
                        .into_owned();
                    match section {
                        Some("name") => current_name.push(value),
                        Some("categories") => current_categories.push(value),
                        Some("values") => current_values.push(value),
                        _ => {}
                    }
                }
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "tx" | "cat" | "val" => section = None,
                "ser" => {
                    series.push(json!({
                        "name": current_name,
                        "categories": current_categories,
                        "values": current_values,
                    }));
                    in_series = false;
                    section = None;
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!("chart XML: {error}")));
            }
        }
    }
    Ok(json!({"series": series}))
}

fn parse_smartart_semantic(data: &[u8]) -> Result<Value, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("SmartArt XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut ids = BTreeMap::<String, usize>::new();
    let mut current_point: Option<usize> = None;
    let mut points: Vec<Vec<String>> = Vec::new();
    let mut raw_connections: Vec<(String, String, String)> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "pt" =>
            {
                let id = attr_required(&event, "modelId")?;
                let ordinal = points.len();
                ids.entry(id).or_insert(ordinal);
                points.push(Vec::new());
                current_point = Some(ordinal);
            }
            Ok(Event::Start(event)) if event.name().as_ref() == "a:t" => {
                let value = reader
                    .read_text(event.name())
                    .map_err(|error| PocError::SemanticExtractionFailed(format!(
                        "SmartArt text XML: {error}"
                    )))?
                    .into_owned();
                if let Some(point) = current_point {
                    if !value.trim().is_empty() {
                        points[point].push(value);
                    }
                }
            }
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "cxn" =>
            {
                let source = attr_required(&event, "srcId")?;
                let target = attr_required(&event, "destId")?;
                let kind = attr(&event, "type")?.unwrap_or_default();
                raw_connections.push((source, target, kind));
            }
            Ok(Event::End(event)) if event.local_name().as_ref() == "pt" => {
                current_point = None;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!("SmartArt XML: {error}")));
            }
        }
    }

    let mut connections = Vec::with_capacity(raw_connections.len());
    for (source, target, kind) in raw_connections {
        let source = ids.get(&source).copied().ok_or_else(|| {
            PocError::SemanticExtractionFailed("SmartArt connection source is unknown".into())
        })?;
        let target = ids.get(&target).copied().ok_or_else(|| {
            PocError::SemanticExtractionFailed("SmartArt connection target is unknown".into())
        })?;
        connections.push(json!({"source": source, "target": target, "kind": kind}));
    }
    Ok(json!({"points": points, "connections": connections}))
}

fn shape_projection(
    shape: &Shape,
    external_dependencies: &mut Vec<ExternalDependency>,
    visual_present: &mut bool,
) -> Result<Value, PocError> {
    match shape {
        Shape::AutoShape(shape) => Ok(json!({
            "kind": "auto_shape",
            "position": position_projection(shape.position.as_ref()),
            "alt_text": shape.alt_text,
            "placeholder": shape.placeholder.as_ref().map(|placeholder| json!({
                "type": placeholder.ph_type,
                "index": placeholder.idx,
            })),
            "text": shape.text_body.as_ref().map(|body| {
                text_body_projection(body, external_dependencies)
            }).transpose()?,
        })),
        Shape::Picture(shape) => {
            *visual_present = true;
            let data = shape.data.as_ref().ok_or_else(|| {
                PocError::SemanticExtractionFailed("picture relationship did not resolve to bytes".into())
            })?;
            Ok(json!({
                "kind": "picture",
                "position": position_projection(shape.position.as_ref()),
                "alt_text": shape.alt_text,
                "format": shape.format,
                "image_semantic_sha256": image_semantic_digest(data)?,
            }))
        }
        Shape::Group(group) => {
            let mut children = Vec::with_capacity(group.children.len());
            for child in &group.children {
                children.push(shape_projection(child, external_dependencies, visual_present)?);
            }
            Ok(json!({
                "kind": "group",
                "position": position_projection(group.position.as_ref()),
                "children": children,
            }))
        }
        Shape::GraphicFrame(frame) => {
            *visual_present = true;
            let content = match &frame.content {
                GraphicContent::Table(table) => {
                    let rows = table
                        .rows
                        .iter()
                        .map(|row| {
                            row.cells
                                .iter()
                                .map(|cell| {
                                    Ok(json!({
                                        "grid_span": cell.grid_span,
                                        "row_span": cell.row_span,
                                        "h_merge": cell.h_merge,
                                        "v_merge": cell.v_merge,
                                        "text": cell.text_body.as_ref().map(|body| {
                                            text_body_projection(body, external_dependencies)
                                        }).transpose()?,
                                    }))
                                })
                                .collect::<Result<Vec<_>, PocError>>()
                        })
                        .collect::<Result<Vec<_>, PocError>>()?;
                    json!({
                        "kind": "table",
                        "first_row_header": table.first_row_header,
                        "last_row_header": table.last_row_header,
                        "rows": rows,
                    })
                }
                GraphicContent::Text(text) => json!({"kind": "graphic_text", "text": text}),
                GraphicContent::Unknown => json!({"kind": "graphic_unknown"}),
            };
            Ok(json!({
                "kind": "graphic_frame",
                "position": position_projection(frame.position.as_ref()),
                "content": content,
            }))
        }
        Shape::Connector(connector) => Ok(json!({
            "kind": "connector",
            "position": position_projection(connector.position.as_ref()),
        })),
    }
}

fn position_projection(position: Option<&office_oxide::pptx::ShapePosition>) -> Value {
    match position {
        Some(position) => json!({
            "x": position.x,
            "y": position.y,
            "cx": position.cx,
            "cy": position.cy,
        }),
        None => Value::Null,
    }
}

fn text_body_projection(
    body: &TextBody,
    external_dependencies: &mut Vec<ExternalDependency>,
) -> Result<Value, PocError> {
    let mut paragraphs = Vec::with_capacity(body.paragraphs.len());
    for paragraph in &body.paragraphs {
        let bullet = match &paragraph.bullet {
            Some(BulletStyle::None) => json!({"kind": "none"}),
            Some(BulletStyle::Char(value)) => json!({"kind": "char", "value": value}),
            Some(BulletStyle::AutoNum { scheme, start_at }) => {
                json!({"kind": "auto_num", "scheme": scheme, "start_at": start_at})
            }
            None => Value::Null,
        };
        let mut content = Vec::with_capacity(paragraph.content.len());
        for item in &paragraph.content {
            match item {
                TextContent::Run(run) => {
                    let hyperlink = match &run.hyperlink {
                        Some(link) => match &link.target {
                            HyperlinkTarget::External(target) => {
                                external_dependencies.push(ExternalDependency {
                                    kind: "hyperlink".into(),
                                    definition: target.clone(),
                                });
                                json!({"kind": "external", "target": target, "tooltip": link.tooltip})
                            }
                            HyperlinkTarget::Internal(target) => {
                                json!({"kind": "internal", "target": target, "tooltip": link.tooltip})
                            }
                        },
                        None => Value::Null,
                    };
                    content.push(json!({
                        "kind": "run",
                        "text": run.text,
                        "bold": run.bold,
                        "italic": run.italic,
                        "strikethrough": run.strikethrough,
                        "underline": run.underline,
                        "color_rgb": run.color_rgb,
                        "hyperlink": hyperlink,
                    }));
                }
                TextContent::LineBreak => content.push(json!({"kind": "line_break"})),
                TextContent::Field(field) => content.push(json!({
                    "kind": "field",
                    "field_type": field.field_type,
                    "text": field.text,
                })),
            }
        }
        paragraphs.push(json!({
            "level": paragraph.level,
            "bullet": bullet,
            "content": content,
        }));
    }
    Ok(json!({"paragraphs": paragraphs}))
}

fn xml_text_values(data: &[u8], local_name: &str) -> Result<Vec<String>, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut values = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.local_name().as_ref() == local_name => {
                let value = reader
                    .read_text(event.name())
                    .map_err(|error| PocError::SemanticExtractionFailed(format!(
                        "text XML: {error}"
                    )))?
                    .into_owned();
                if !value.trim().is_empty() {
                    values.push(value);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!("text XML: {error}")));
            }
        }
    }
    Ok(values)
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
            return Err(PocError::SemanticExtractionFailed("truncated PNG chunk".into()));
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
        if !matches!(chunk_type, b"tEXt" | b"zTXt" | b"iTXt" | b"tIME" | b"eXIf") {
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
        return Err(PocError::SemanticExtractionFailed("PNG has no IEND chunk".into()));
    }
    Ok(hex::encode(Sha256::digest(&normalized)))
}

fn prefixed_attr_required(event: &BytesStart<'_>, name: &str) -> Result<String, PocError> {
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        if item.key.as_ref() == name {
            return Ok(item.value.as_ref().to_owned());
        }
    }
    Err(PocError::SemanticExtractionFailed(format!(
        "missing required attribute {name}"
    )))
}

fn attr_required(event: &BytesStart<'_>, name: &str) -> Result<String, PocError> {
    attr(event, name)?.ok_or_else(|| {
        PocError::SemanticExtractionFailed(format!("missing required attribute {name}"))
    })
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
            return Ok(Some(item.value.as_ref().to_owned()));
        }
    }
    Ok(None)
}
