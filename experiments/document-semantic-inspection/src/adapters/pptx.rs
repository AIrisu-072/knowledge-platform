use crate::{
    AdapterOutput, CapabilityEvidence, CommentEvidence, EditorialEvidence, ExternalDependency,
    FormatId, InspectionAdapter, InspectionProfile, PocError, canonical_json_bytes,
};
use office_oxide::pptx::{
    BulletStyle, GraphicContent, HyperlinkTarget, PptxDocument, Shape, TextBody, TextContent,
};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use zip::ZipArchive;

const PRESENTATION_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const RELATIONSHIPS_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";
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
const DRAWINGML_MAIN_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PRESENTATION_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const OFFICE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const OFFICE_DOCUMENT_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_ENTRY_UNCOMPRESSED: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_UNCOMPRESSED: u64 = 512 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 256;
const MAX_XML_NODES: usize = 2_000_000;
const MAX_SLIDE_SHAPES: usize = 100_000;
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const PACKAGE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";

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
        if !is_pptx_package_checked(input)? {
            return Err(PocError::FormatMismatch {
                expected: FormatId::Pptx,
                observed: None,
            });
        }

        let package = inspect_package(input)?;
        let document = PptxDocument::from_reader(Cursor::new(input)).map_err(|error| {
            PocError::SemanticExtractionFailed(format!("office_oxide PPTX: {error}"))
        })?;

        let ordered_slide_parts = ordered_slide_parts(&package)?;
        let office_slide_parts = office_oxide_slide_parts(&package, &document)?;
        if ordered_slide_parts.len() != document.slides.len()
            || office_slide_parts.len() != document.slides.len()
        {
            return Err(PocError::ParserDisagreement(format!(
                "slide count mismatch: raw={}, office_oxide={}, office_oxide_paths={}",
                ordered_slide_parts.len(),
                document.slides.len(),
                office_slide_parts.len()
            )));
        }

        let office_slide_index_by_part = office_slide_parts
            .iter()
            .enumerate()
            .map(|(index, part)| (part.as_str(), index))
            .collect::<BTreeMap<_, _>>();
        if office_slide_index_by_part.len() != office_slide_parts.len() {
            return Err(PocError::ParserDisagreement(
                "office_oxide mapped multiple slide entries to one part".into(),
            ));
        }

        let mut slides = Vec::with_capacity(document.slides.len());
        let mut external_dependencies = Vec::new();
        let mut editorial = EditorialEvidence::default();
        let mut visual_present = false;
        let mut notes_present = false;
        for (index, slide_part) in ordered_slide_parts.iter().enumerate() {
            let raw_slide = package.parts.get(slide_part).ok_or_else(|| {
                PocError::SemanticExtractionFailed(format!("missing slide part {slide_part}"))
            })?;
            let slide_index = office_slide_index_by_part
                .get(slide_part.as_str())
                .copied()
                .ok_or_else(|| {
                    PocError::ParserDisagreement(format!(
                        "office_oxide has no slide mapped to raw relationship target {slide_part}"
                    ))
                })?;
            let slide = &document.slides[slide_index];

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

            let raw_shape_semantics = raw_shape_relationship_semantics(raw_slide, &slide.shapes)?;
            let mut shape_values = Vec::with_capacity(slide.shapes.len());
            let mut shape_path = Vec::new();
            for (shape_index, shape) in slide.shapes.iter().enumerate() {
                shape_path.push(shape_index);
                let value = shape_projection(
                    shape,
                    &mut external_dependencies,
                    &mut visual_present,
                    &mut shape_path,
                    &raw_shape_semantics.connector_relationships,
                    &raw_shape_semantics.picture_transforms,
                )?;
                shape_path.pop();
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
        external_dependencies
            .dedup_by(|left, right| left.kind == right.kind && left.definition == right.definition);

        let projection = json!({
            "slides": slides,
        });
        let semantic_projection = canonical_json_bytes(&projection).map_err(|error| {
            PocError::InvalidWorkerResult(format!("PPTX projection serialization: {error}"))
        })?;
        let semantic_equivalence = hex::encode(crate::fingerprint(&semantic_projection));

        Ok(AdapterOutput {
            semantic_projection,
            capabilities: vec![
                CapabilityEvidence::binary(
                    "reader_content",
                    true,
                    true,
                    Some(semantic_equivalence.clone()),
                ),
                CapabilityEvidence::binary(
                    "presentation_structure",
                    true,
                    true,
                    Some(semantic_equivalence.clone()),
                ),
                CapabilityEvidence::binary(
                    "visual_content",
                    visual_present,
                    true,
                    Some(semantic_equivalence.clone()),
                ),
                CapabilityEvidence::binary(
                    "speaker_notes",
                    notes_present,
                    true,
                    Some(semantic_equivalence.clone()),
                ),
                CapabilityEvidence::new(
                    "formula_logic",
                    crate::CapabilityState::NotRepresentable,
                    true,
                    None,
                ),
                CapabilityEvidence::new(
                    "vba_logic",
                    crate::CapabilityState::NotRepresentable,
                    true,
                    None,
                ),
            ],
            editorial,
            external_dependencies,
            signatures: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

pub(crate) fn is_pptx_package(input: &[u8]) -> bool {
    is_pptx_package_checked(input).unwrap_or(false)
}

fn is_pptx_package_checked(input: &[u8]) -> Result<bool, PocError> {
    let Some(mut archive) = open_bounded_archive(input)? else {
        return Ok(false);
    };
    let Ok(part) = archive.by_name("[Content_Types].xml") else {
        return Ok(false);
    };
    let mut content = Vec::new();
    part.take(MAX_ENTRY_UNCOMPRESSED + 1)
        .read_to_end(&mut content)
        .map_err(|error| {
            PocError::SemanticExtractionFailed(format!("read [Content_Types].xml: {error}"))
        })?;
    if content.len() as u64 > MAX_ENTRY_UNCOMPRESSED {
        return Err(PocError::InspectionResourceLimitExceeded);
    }
    content_types_declares_presentation(&content)
}

fn content_types_declares_presentation(data: &[u8]) -> Result<bool, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("content types are not UTF-8".into()))?;
    validate_xml("[Content_Types].xml", data)?;

    let mut reader = Reader::from_str(text);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Override" =>
            {
                if normalized_unqualified_attr(&event, "ContentType")?.as_deref()
                    == Some(PRESENTATION_MAIN)
                {
                    return Ok(true);
                }
            }
            Ok(Event::Eof) => return Ok(false),
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "content-types XML: {error}"
                )));
            }
        }
    }
}

fn open_bounded_archive(input: &[u8]) -> Result<Option<ZipArchive<Cursor<&[u8]>>>, PocError> {
    let Ok(mut archive) = ZipArchive::new(Cursor::new(input)) else {
        return Ok(None);
    };
    check_archive_size_limits(&mut archive)?;
    Ok(Some(archive))
}

fn check_archive_size_limits(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<(), PocError> {
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(PocError::InspectionResourceLimitExceeded);
    }

    let mut total = 0u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("PPTX entry: {error}")))?;
        if file.size() > MAX_ENTRY_UNCOMPRESSED {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
        total = total
            .checked_add(file.size())
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        if total > MAX_TOTAL_UNCOMPRESSED {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
    }
    Ok(())
}

fn inspect_package(input: &[u8]) -> Result<PackageInspection, PocError> {
    let mut archive = ZipArchive::new(Cursor::new(input))
        .map_err(|error| PocError::SemanticExtractionFailed(format!("PPTX ZIP: {error}")))?;
    check_archive_size_limits(&mut archive)?;

    let mut parts = BTreeMap::new();
    let mut actual_total = 0u64;
    let mut xml_nodes_total = 0usize;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("PPTX entry: {error}")))?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_owned();
        validate_part_name(&name)?;
        if parts.contains_key(&name) {
            return Err(PocError::SemanticExtractionFailed(format!(
                "duplicate PPTX package part {name}"
            )));
        }
        let declared_size = file.size();
        let remaining_total = MAX_TOTAL_UNCOMPRESSED
            .checked_sub(actual_total)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        let read_limit = MAX_ENTRY_UNCOMPRESSED.min(remaining_total);
        let mut data = Vec::new();
        file.take(read_limit + 1)
            .read_to_end(&mut data)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("read {name}: {error}")))?;
        let actual_size = data.len() as u64;
        if actual_size > read_limit {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
        if actual_size != declared_size {
            return Err(PocError::SemanticExtractionFailed(format!(
                "PPTX entry {name} declared {declared_size} bytes but produced {actual_size}"
            )));
        }
        actual_total = actual_total
            .checked_add(actual_size)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        if actual_total > MAX_TOTAL_UNCOMPRESSED {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let nodes = validate_xml(&name, &data)?;
            xml_nodes_total = xml_nodes_total
                .checked_add(nodes)
                .ok_or(PocError::InspectionResourceLimitExceeded)?;
            if xml_nodes_total > MAX_XML_NODES {
                return Err(PocError::InspectionResourceLimitExceeded);
            }
        }
        parts.insert(name, data);
    }

    let content_types = parts.get("[Content_Types].xml").ok_or_else(|| {
        PocError::SemanticExtractionFailed("PPTX missing [Content_Types].xml".into())
    })?;
    validate_content_types(content_types)?;
    validate_package_part_coverage(&parts, content_types)?;

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

    let root_relationships = relationships.get("").ok_or_else(|| {
        PocError::SemanticExtractionFailed("PPTX missing package root relationships".into())
    })?;
    let office_document_relationships = root_relationships
        .values()
        .filter(|relationship| relationship.kind == OFFICE_DOCUMENT_RELATIONSHIP_TYPE)
        .collect::<Vec<_>>();
    if office_document_relationships.len() != 1 {
        return Err(PocError::SemanticExtractionFailed(
            "PPTX must have exactly one officeDocument root relationship".into(),
        ));
    }
    let office_document = office_document_relationships[0];
    if office_document.external
        || resolve_target("", &office_document.target)? != "ppt/presentation.xml"
    {
        return Err(PocError::SemanticExtractionFailed(
            "PPTX officeDocument root relationship must target ppt/presentation.xml internally"
                .into(),
        ));
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

fn validate_xml(name: &str, data: &[u8]) -> Result<usize, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed(format!("{name} is not UTF-8 XML")))?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut depth = 0usize;
    let mut nodes = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => {
                depth += 1;
                if depth > MAX_XML_DEPTH {
                    return Err(PocError::InspectionResourceLimitExceeded);
                }
                count_xml_node(&mut nodes)?;
            }
            Ok(Event::Empty(_)) => count_xml_node(&mut nodes)?,
            Ok(Event::End(_)) => {
                if depth == 0 {
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "{name} has unmatched closing tag"
                    )));
                }
                depth -= 1;
            }
            Ok(Event::Eof) => break,
            Ok(_) => count_xml_node(&mut nodes)?,
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
    Ok(nodes)
}

fn count_xml_node(nodes: &mut usize) -> Result<(), PocError> {
    *nodes = nodes
        .checked_add(1)
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    if *nodes > MAX_XML_NODES {
        return Err(PocError::InspectionResourceLimitExceeded);
    }
    Ok(())
}

fn validate_opc_xml_children(
    data: &[u8],
    namespace_uri: &str,
    root_local_name: &str,
    child_local_names: &[&str],
    description: &str,
) -> Result<(), PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed(format!("{description} is not UTF-8")))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut root_closed = false;

    loop {
        match reader.read_resolved_event() {
            Ok((namespace, Event::Start(event))) => {
                let local = event.local_name();
                if depth == 0 {
                    if root_seen
                        || root_closed
                        || !is_resolved_qname(
                            &namespace,
                            namespace_uri,
                            root_local_name,
                            local.as_ref(),
                        )
                    {
                        return Err(PocError::SemanticExtractionFailed(format!(
                            "{description} has an unexpected root element"
                        )));
                    }
                    root_seen = true;
                } else if depth == 1 {
                    if !child_local_names.iter().any(|child| {
                        is_resolved_qname(&namespace, namespace_uri, child, local.as_ref())
                    }) {
                        return Err(PocError::SemanticExtractionFailed(format!(
                            "{description} has an unexpected child element"
                        )));
                    }
                } else {
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "{description} child elements must be empty"
                    )));
                }
                depth = depth
                    .checked_add(1)
                    .ok_or(PocError::InspectionResourceLimitExceeded)?;
            }
            Ok((namespace, Event::Empty(event))) => {
                let local = event.local_name();
                if depth == 0 {
                    if root_seen
                        || root_closed
                        || !is_resolved_qname(
                            &namespace,
                            namespace_uri,
                            root_local_name,
                            local.as_ref(),
                        )
                    {
                        return Err(PocError::SemanticExtractionFailed(format!(
                            "{description} has an unexpected root element"
                        )));
                    }
                    root_seen = true;
                    root_closed = true;
                } else if depth == 1 {
                    if !child_local_names.iter().any(|child| {
                        is_resolved_qname(&namespace, namespace_uri, child, local.as_ref())
                    }) {
                        return Err(PocError::SemanticExtractionFailed(format!(
                            "{description} has an unexpected child element"
                        )));
                    }
                } else {
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "{description} child elements must be empty"
                    )));
                }
            }
            Ok((_, Event::End(_))) => {
                if depth == 0 {
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "{description} has an unmatched closing tag"
                    )));
                }
                depth -= 1;
                if depth == 0 {
                    root_closed = true;
                }
            }
            Ok((_, Event::Eof)) => break,
            Ok((_, _)) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "{description} XML parse failed: {error}"
                )));
            }
        }
    }

    if !root_seen || !root_closed || depth != 0 {
        return Err(PocError::SemanticExtractionFailed(format!(
            "{description} has no complete root element"
        )));
    }
    Ok(())
}

fn is_resolved_qname(
    namespace: &ResolveResult<'_>,
    expected_namespace: &str,
    expected_local_name: &str,
    local_name: &str,
) -> bool {
    matches!(namespace, ResolveResult::Bound(uri) if uri.as_ref() == expected_namespace)
        && local_name == expected_local_name
}

fn validate_content_types(data: &[u8]) -> Result<(), PocError> {
    validate_opc_xml_children(
        data,
        CONTENT_TYPES_NS,
        "Types",
        &["Default", "Override"],
        "content-types XML",
    )?;
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
                if let Some(value) = normalized_unqualified_attr(&event, "ContentType")? {
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

fn validate_package_part_coverage(
    parts: &BTreeMap<String, Vec<u8>>,
    data: &[u8],
) -> Result<(), PocError> {
    validate_opc_xml_children(
        data,
        CONTENT_TYPES_NS,
        "Types",
        &["Default", "Override"],
        "content-types XML",
    )?;
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("content types are not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut defaults = BTreeMap::<String, String>::new();
    let mut overrides = BTreeMap::<String, String>::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Default" =>
            {
                let extension =
                    unqualified_attr_required(&event, "Extension")?.to_ascii_lowercase();
                let content_type = normalized_unqualified_attr_required(&event, "ContentType")?;
                if defaults.insert(extension.clone(), content_type).is_some() {
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "duplicate PPTX default content type for {extension}"
                    )));
                }
            }
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Override" =>
            {
                let part_name = unqualified_attr_required(&event, "PartName")?;
                let part_name = part_name.strip_prefix('/').ok_or_else(|| {
                    PocError::SemanticExtractionFailed(format!(
                        "PPTX content-type override path is not absolute: {part_name}"
                    ))
                })?;
                validate_part_name(part_name)?;
                let content_type = normalized_unqualified_attr_required(&event, "ContentType")?;
                if overrides
                    .insert(part_name.to_owned(), content_type)
                    .is_some()
                {
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "duplicate PPTX content-type override for {part_name}"
                    )));
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

    for name in overrides.keys() {
        if !parts.contains_key(name) {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "PPTX content-type override references missing part {name}"
            )));
        }
    }

    for name in parts
        .keys()
        .filter(|name| name.as_str() != "[Content_Types].xml")
    {
        let Some(expected) = expected_pptx_part_content_type(name, parts)? else {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "unmodeled PPTX package part {name}"
            )));
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
            .ok_or_else(|| {
                PocError::UnsupportedSemanticConstruct(format!(
                    "PPTX package part has no content type: {name}"
                ))
            })?;
        if content_type != expected {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "PPTX part path/content-type mismatch for {name}: expected {expected}, found {content_type}"
            )));
        }
        if content_type == GENERIC_XML_CONTENT_TYPE {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "unmodeled generic XML PPTX part {name}"
            )));
        }
    }
    Ok(())
}

fn expected_pptx_part_content_type(
    name: &str,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Result<Option<&'static str>, PocError> {
    if name == "_rels/.rels" {
        return Ok(Some(RELATIONSHIPS_CONTENT_TYPE));
    }
    if name.contains("/_rels/") {
        let source = source_part_for_rels(name)?;
        if !parts.contains_key(&source) {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "PPTX relationships part {name} has no source part {source}"
            )));
        }
        if expected_non_relationship_part_content_type(&source).is_none() {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "PPTX relationships part {name} has an unmodeled source part {source}"
            )));
        }
        return Ok(Some(RELATIONSHIPS_CONTENT_TYPE));
    }
    Ok(expected_non_relationship_part_content_type(name))
}

fn expected_non_relationship_part_content_type(name: &str) -> Option<&'static str> {
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

fn parse_relationships(data: &[u8]) -> Result<BTreeMap<String, Relationship>, PocError> {
    validate_opc_xml_children(
        data,
        PACKAGE_RELATIONSHIPS_NS,
        "Relationships",
        &["Relationship"],
        "package relationships XML",
    )?;
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("relationships are not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut out = BTreeMap::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Relationship" =>
            {
                let id = unqualified_attr_required(&event, "Id")?;
                let kind = unqualified_attr_required(&event, "Type")?;
                let target = unqualified_attr_required(&event, "Target")?;
                let external = unqualified_attr(&event, "TargetMode")?
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
        .ok_or_else(|| {
            PocError::SemanticExtractionFailed("presentation relationships missing".into())
        })?;

    let text = std::str::from_utf8(presentation)
        .map_err(|_| PocError::SemanticExtractionFailed("presentation XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    let mut result = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if element_in_namespace(
                    &reader,
                    &event,
                    "http://schemas.openxmlformats.org/presentationml/2006/main",
                    "sldId",
                ) =>
            {
                let rid = namespaced_attr_required(&reader, &event, OFFICE_RELATIONSHIPS_NS, "id")?;
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

fn office_oxide_slide_parts(
    package: &PackageInspection,
    document: &PptxDocument,
) -> Result<Vec<String>, PocError> {
    let slide_ids = &document.presentation.slides;
    if slide_ids.len() != document.slides.len() {
        return Err(PocError::ParserDisagreement(format!(
            "office_oxide presentation/parsed slide count mismatch: presentation={}, parsed={}",
            slide_ids.len(),
            document.slides.len()
        )));
    }

    let raw_slide_parts = ordered_slide_parts(package)?;
    let all_relationship_ids_present = slide_ids.iter().all(|slide| !slide.rel_id.is_empty());
    let all_relationship_ids_missing = slide_ids.iter().all(|slide| slide.rel_id.is_empty());

    let parts = if all_relationship_ids_present {
        let relationships = package
            .relationships
            .get("ppt/presentation.xml")
            .ok_or_else(|| {
                PocError::SemanticExtractionFailed("presentation relationships missing".into())
            })?;
        let mut parts = Vec::with_capacity(slide_ids.len());
        for slide in slide_ids {
            let rel = relationships.get(&slide.rel_id).ok_or_else(|| {
                PocError::ParserDisagreement(format!(
                    "office_oxide slide relationship {} is missing from raw package",
                    slide.rel_id
                ))
            })?;
            if !rel.kind.ends_with("/slide") || rel.external {
                return Err(PocError::UnsupportedSemanticConstruct(format!(
                    "office_oxide slide relationship {} is not an internal slide",
                    slide.rel_id
                )));
            }
            parts.push(resolve_target("ppt/presentation.xml", &rel.target)?);
        }
        if parts != raw_slide_parts {
            return Err(PocError::ParserDisagreement(
                "office_oxide slide relationship order differs from raw PresentationML".into(),
            ));
        }
        parts
    } else if all_relationship_ids_missing {
        // office_oxide 0.1.11 falls back to ppt/slides/slideN.xml in presentation order
        // when its literal `r:id` lookup does not find an attribute. Map that fallback
        // back to the raw relationship order only when the package contains exactly
        // the conventional slide1..slideN path set.
        let fallback_parts = (1..=slide_ids.len())
            .map(|index| format!("ppt/slides/slide{index}.xml"))
            .collect::<Vec<_>>();
        let raw_part_set = raw_slide_parts.iter().collect::<BTreeSet<_>>();
        let fallback_part_set = fallback_parts.iter().collect::<BTreeSet<_>>();
        if raw_part_set != fallback_part_set {
            return Err(PocError::UnsupportedSemanticConstruct(
                "office_oxide positional slide fallback is not safe for nonconventional slide part paths".into(),
            ));
        }
        fallback_parts
    } else {
        return Err(PocError::UnsupportedSemanticConstruct(
            "office_oxide mixed literal relationship IDs and positional slide fallback cannot be mapped safely".into(),
        ));
    };

    let unique = parts.iter().collect::<BTreeSet<_>>();
    if unique.len() != parts.len() {
        return Err(PocError::ParserDisagreement(
            "office_oxide mapped multiple slides to one package part".into(),
        ));
    }
    Ok(parts)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PptxShapeKind {
    AutoShape,
    Picture,
    Group,
    GraphicFrame,
    Connector,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PptxShapeIdentity {
    path: Vec<usize>,
    kind: PptxShapeKind,
    id: u32,
}

#[derive(Debug, Clone, Copy)]
struct ConnectorEndpoint {
    shape_id: u32,
    site_index: u32,
}

struct PptxShapeFrame {
    path: Vec<usize>,
    kind: PptxShapeKind,
    id: Option<u32>,
    next_child_index: usize,
    start: Option<ConnectorEndpoint>,
    end: Option<ConnectorEndpoint>,
    picture_transform: PptxPictureTransform,
    picture_transform_seen: bool,
    picture_blip_fill_seen: bool,
    inside_picture_blip_fill: bool,
    picture_blip_seen: bool,
    picture_blip_open: bool,
    picture_fill_mode_seen: bool,
    picture_stretch_open: bool,
    picture_fill_rect_seen: bool,
    picture_fill_rect_open: bool,
    picture_src_rect_seen: bool,
    picture_src_rect_open: bool,
}

struct RawConnector {
    path: Vec<usize>,
    start: Option<ConnectorEndpoint>,
    end: Option<ConnectorEndpoint>,
}

#[derive(Debug, Clone, Copy, Default)]
struct PptxPictureTransform {
    crop_left: u32,
    crop_top: u32,
    crop_right: u32,
    crop_bottom: u32,
    rotation: u32,
    flip_horizontal: bool,
    flip_vertical: bool,
}

#[derive(Debug, Default)]
struct RawShapeSemantics {
    connector_relationships: BTreeMap<Vec<usize>, Value>,
    picture_transforms: BTreeMap<Vec<usize>, PptxPictureTransform>,
}

fn raw_shape_relationship_semantics(
    slide_data: &[u8],
    office_shapes: &[Shape],
) -> Result<RawShapeSemantics, PocError> {
    let text = std::str::from_utf8(slide_data)
        .map_err(|_| PocError::SemanticExtractionFailed("slide XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut inside_shape_tree = false;
    let mut saw_shape_tree = false;
    let mut root_shape_count = 0usize;
    let mut shape_stack = Vec::<PptxShapeFrame>::new();
    let mut identities = Vec::<PptxShapeIdentity>::new();
    let mut connectors = Vec::<RawConnector>::new();
    let mut picture_transforms = BTreeMap::new();

    loop {
        match reader.read_resolved_event() {
            Ok((namespace, Event::Start(event))) => {
                let local_name = event.local_name();
                let local = local_name.as_ref();
                if let Some(frame) = shape_stack.last_mut() {
                    handle_picture_transform_start(frame, &namespace, &event)?;
                }
                if is_resolved_qname(&namespace, PRESENTATION_NS, "spTree", local) {
                    if saw_shape_tree || inside_shape_tree {
                        return Err(PocError::SemanticExtractionFailed(
                            "slide XML has multiple shape trees".into(),
                        ));
                    }
                    saw_shape_tree = true;
                    inside_shape_tree = true;
                    continue;
                }

                if inside_shape_tree {
                    if let Some(kind) = presentation_shape_kind(&namespace, local) {
                        if identities.len() + shape_stack.len() >= MAX_SLIDE_SHAPES {
                            return Err(PocError::InspectionResourceLimitExceeded);
                        }
                        let path = next_shape_path(&mut shape_stack, &mut root_shape_count)?;
                        shape_stack.push(PptxShapeFrame {
                            path,
                            kind,
                            id: None,
                            next_child_index: 0,
                            start: None,
                            end: None,
                            picture_transform: PptxPictureTransform::default(),
                            picture_transform_seen: false,
                            picture_blip_fill_seen: false,
                            inside_picture_blip_fill: false,
                            picture_blip_seen: false,
                            picture_blip_open: false,
                            picture_fill_mode_seen: false,
                            picture_stretch_open: false,
                            picture_fill_rect_seen: false,
                            picture_fill_rect_open: false,
                            picture_src_rect_seen: false,
                            picture_src_rect_open: false,
                        });
                        continue;
                    }
                    if is_resolved_qname(&namespace, PRESENTATION_NS, "cNvPr", local)
                        && let Some(frame) = shape_stack.last_mut()
                    {
                        if frame.id.is_some() {
                            return Err(PocError::SemanticExtractionFailed(
                                "shape has duplicate non-visual identifiers".into(),
                            ));
                        }
                        frame.id = Some(parse_shape_id(&event)?);
                    }
                    if is_connector_endpoint_name(&namespace, local) {
                        let frame = shape_stack.last_mut().ok_or_else(|| {
                            PocError::UnsupportedSemanticConstruct(
                                "connector endpoint appears outside a connector".into(),
                            )
                        })?;
                        if frame.kind != PptxShapeKind::Connector {
                            return Err(PocError::UnsupportedSemanticConstruct(
                                "connector endpoint appears on a non-connector shape".into(),
                            ));
                        }
                        let endpoint = parse_connector_endpoint(&event)?;
                        let slot =
                            if is_resolved_qname(&namespace, DRAWINGML_MAIN_NS, "stCxn", local) {
                                &mut frame.start
                            } else {
                                &mut frame.end
                            };
                        if slot.replace(endpoint).is_some() {
                            return Err(PocError::SemanticExtractionFailed(
                                "connector has duplicate endpoint declarations".into(),
                            ));
                        }
                    }
                }
            }
            Ok((namespace, Event::Empty(event))) => {
                let local_name = event.local_name();
                let local = local_name.as_ref();
                if let Some(frame) = shape_stack.last_mut() {
                    handle_picture_transform_empty(frame, &namespace, &event)?;
                }
                if is_resolved_qname(&namespace, PRESENTATION_NS, "spTree", local) {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "empty presentation shape tree".into(),
                    ));
                }
                if inside_shape_tree {
                    if presentation_shape_kind(&namespace, local).is_some() {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "empty PresentationML shape object".into(),
                        ));
                    }
                    if is_resolved_qname(&namespace, PRESENTATION_NS, "cNvPr", local)
                        && let Some(frame) = shape_stack.last_mut()
                    {
                        if frame.id.is_some() {
                            return Err(PocError::SemanticExtractionFailed(
                                "shape has duplicate non-visual identifiers".into(),
                            ));
                        }
                        frame.id = Some(parse_shape_id(&event)?);
                    }
                    if is_connector_endpoint_name(&namespace, local) {
                        let frame = shape_stack.last_mut().ok_or_else(|| {
                            PocError::UnsupportedSemanticConstruct(
                                "connector endpoint appears outside a connector".into(),
                            )
                        })?;
                        if frame.kind != PptxShapeKind::Connector {
                            return Err(PocError::UnsupportedSemanticConstruct(
                                "connector endpoint appears on a non-connector shape".into(),
                            ));
                        }
                        let endpoint = parse_connector_endpoint(&event)?;
                        let slot =
                            if is_resolved_qname(&namespace, DRAWINGML_MAIN_NS, "stCxn", local) {
                                &mut frame.start
                            } else {
                                &mut frame.end
                            };
                        if slot.replace(endpoint).is_some() {
                            return Err(PocError::SemanticExtractionFailed(
                                "connector has duplicate endpoint declarations".into(),
                            ));
                        }
                    }
                }
            }
            Ok((namespace, Event::End(event))) => {
                let local_name = event.local_name();
                let local = local_name.as_ref();
                if let Some(frame) = shape_stack.last_mut() {
                    handle_picture_transform_end(frame, &namespace, local)?;
                }
                if inside_shape_tree
                    && is_resolved_qname(&namespace, PRESENTATION_NS, "spTree", local)
                {
                    if !shape_stack.is_empty() {
                        return Err(PocError::SemanticExtractionFailed(
                            "shape tree closed with an unfinished shape object".into(),
                        ));
                    }
                    inside_shape_tree = false;
                    continue;
                }
                if inside_shape_tree && let Some(kind) = presentation_shape_kind(&namespace, local)
                {
                    let frame = shape_stack.pop().ok_or_else(|| {
                        PocError::SemanticExtractionFailed(
                            "shape object closed without a matching start".into(),
                        )
                    })?;
                    if frame.kind != kind {
                        return Err(PocError::SemanticExtractionFailed(
                            "shape object nesting is inconsistent".into(),
                        ));
                    }
                    let id = frame.id.ok_or_else(|| {
                        PocError::SemanticExtractionFailed(
                            "shape object is missing its non-visual identifier".into(),
                        )
                    })?;
                    if frame.picture_src_rect_open || frame.inside_picture_blip_fill {
                        return Err(PocError::SemanticExtractionFailed(
                            "picture fill XML is incomplete".into(),
                        ));
                    }
                    identities.push(PptxShapeIdentity {
                        path: frame.path.clone(),
                        kind: frame.kind,
                        id,
                    });
                    if frame.kind == PptxShapeKind::Picture
                        && picture_transforms
                            .insert(frame.path.clone(), frame.picture_transform)
                            .is_some()
                    {
                        return Err(PocError::SemanticExtractionFailed(
                            "duplicate picture occurrence path".into(),
                        ));
                    }
                    if frame.kind == PptxShapeKind::Connector {
                        connectors.push(RawConnector {
                            path: frame.path,
                            start: frame.start,
                            end: frame.end,
                        });
                    }
                }
            }
            Ok((_, Event::Eof)) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "slide shape relationship XML: {error}"
                )));
            }
        }
    }

    if !saw_shape_tree || inside_shape_tree || !shape_stack.is_empty() {
        return Err(PocError::SemanticExtractionFailed(
            "slide XML has no complete shape tree".into(),
        ));
    }

    let mut office_identities = Vec::new();
    collect_office_shape_identities(office_shapes, &mut Vec::new(), &mut office_identities);
    identities.sort_by(|left, right| left.path.cmp(&right.path));
    office_identities.sort_by(|left, right| left.path.cmp(&right.path));
    if identities != office_identities {
        return Err(PocError::ParserDisagreement(
            "office_oxide slide shapes differ from raw PresentationML object order or identity"
                .into(),
        ));
    }

    let mut shape_path_by_id = BTreeMap::new();
    for identity in identities {
        if shape_path_by_id
            .insert(identity.id, identity.path)
            .is_some()
        {
            return Err(PocError::SemanticExtractionFailed(
                "slide has duplicate shape identifiers".into(),
            ));
        }
    }

    let mut connector_relationships = BTreeMap::new();
    for connector in connectors {
        let start = project_connector_endpoint(connector.start, &shape_path_by_id)?;
        let end = project_connector_endpoint(connector.end, &shape_path_by_id)?;
        connector_relationships.insert(connector.path, json!({"start": start, "end": end}));
    }
    Ok(RawShapeSemantics {
        connector_relationships,
        picture_transforms,
    })
}

fn presentation_shape_kind(namespace: &ResolveResult<'_>, local: &str) -> Option<PptxShapeKind> {
    if !matches!(namespace, ResolveResult::Bound(uri) if uri.as_ref() == PRESENTATION_NS) {
        return None;
    }
    match local {
        "sp" => Some(PptxShapeKind::AutoShape),
        "pic" => Some(PptxShapeKind::Picture),
        "grpSp" => Some(PptxShapeKind::Group),
        "graphicFrame" => Some(PptxShapeKind::GraphicFrame),
        "cxnSp" => Some(PptxShapeKind::Connector),
        _ => None,
    }
}

fn next_shape_path(
    shape_stack: &mut [PptxShapeFrame],
    root_shape_count: &mut usize,
) -> Result<Vec<usize>, PocError> {
    let Some(parent) = shape_stack.last_mut() else {
        let index = *root_shape_count;
        *root_shape_count = root_shape_count
            .checked_add(1)
            .ok_or(PocError::InspectionResourceLimitExceeded)?;
        return Ok(vec![index]);
    };
    if parent.kind != PptxShapeKind::Group {
        return Err(PocError::UnsupportedSemanticConstruct(
            "nested PresentationML object is not contained by a group shape".into(),
        ));
    }
    let mut path = parent.path.clone();
    path.push(parent.next_child_index);
    parent.next_child_index = parent
        .next_child_index
        .checked_add(1)
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    Ok(path)
}

fn parse_shape_id(event: &BytesStart<'_>) -> Result<u32, PocError> {
    unqualified_attr_required(event, "id")?
        .parse::<u32>()
        .map_err(|_| PocError::SemanticExtractionFailed("invalid shape identifier".into()))
}

fn is_connector_endpoint_name(namespace: &ResolveResult<'_>, local: &str) -> bool {
    is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "stCxn", local)
        || is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "endCxn", local)
}

fn parse_connector_endpoint(event: &BytesStart<'_>) -> Result<ConnectorEndpoint, PocError> {
    let shape_id = unqualified_attr_required(event, "id")?
        .parse::<u32>()
        .map_err(|_| PocError::SemanticExtractionFailed("invalid connector target id".into()))?;
    let site_index = unqualified_attr_required(event, "idx")?
        .parse::<u32>()
        .map_err(|_| PocError::SemanticExtractionFailed("invalid connector site index".into()))?;
    Ok(ConnectorEndpoint {
        shape_id,
        site_index,
    })
}

fn collect_office_shape_identities(
    shapes: &[Shape],
    path: &mut Vec<usize>,
    identities: &mut Vec<PptxShapeIdentity>,
) {
    for (index, shape) in shapes.iter().enumerate() {
        path.push(index);
        let (id, kind) = match shape {
            Shape::AutoShape(shape) => (shape.id, PptxShapeKind::AutoShape),
            Shape::Picture(shape) => (shape.id, PptxShapeKind::Picture),
            Shape::Group(shape) => (shape.id, PptxShapeKind::Group),
            Shape::GraphicFrame(shape) => (shape.id, PptxShapeKind::GraphicFrame),
            Shape::Connector(shape) => (shape.id, PptxShapeKind::Connector),
        };
        identities.push(PptxShapeIdentity {
            path: path.clone(),
            kind,
            id,
        });
        if let Shape::Group(group) = shape {
            collect_office_shape_identities(&group.children, path, identities);
        }
        path.pop();
    }
}

fn project_connector_endpoint(
    endpoint: Option<ConnectorEndpoint>,
    shape_path_by_id: &BTreeMap<u32, Vec<usize>>,
) -> Result<Value, PocError> {
    let Some(endpoint) = endpoint else {
        return Ok(Value::Null);
    };
    let target_path = shape_path_by_id.get(&endpoint.shape_id).ok_or_else(|| {
        PocError::UnsupportedSemanticConstruct(
            "connector endpoint references an unknown shape".into(),
        )
    })?;
    Ok(json!({
        "target_occurrence": target_path,
        "connection_site": endpoint.site_index,
    }))
}

fn handle_picture_transform_start(
    frame: &mut PptxShapeFrame,
    namespace: &ResolveResult<'_>,
    event: &BytesStart<'_>,
) -> Result<(), PocError> {
    handle_picture_transform(frame, namespace, event, false)
}

fn handle_picture_transform_empty(
    frame: &mut PptxShapeFrame,
    namespace: &ResolveResult<'_>,
    event: &BytesStart<'_>,
) -> Result<(), PocError> {
    handle_picture_transform(frame, namespace, event, true)
}

fn handle_picture_transform(
    frame: &mut PptxShapeFrame,
    namespace: &ResolveResult<'_>,
    event: &BytesStart<'_>,
    empty: bool,
) -> Result<(), PocError> {
    if frame.kind != PptxShapeKind::Picture {
        if frame.picture_src_rect_open {
            return Err(PocError::UnsupportedSemanticConstruct(
                "nested XML inside picture source rectangle".into(),
            ));
        }
        return Ok(());
    }
    let local = event.local_name();
    let local = local.as_ref();

    if frame.picture_src_rect_open {
        return Err(PocError::UnsupportedSemanticConstruct(
            "nested XML inside picture source rectangle".into(),
        ));
    }
    if frame.picture_fill_rect_open {
        return Err(PocError::UnsupportedSemanticConstruct(
            "nested XML inside picture stretch fill rectangle".into(),
        ));
    }
    if frame.picture_blip_open {
        return Err(PocError::UnsupportedSemanticConstruct(
            "unsupported nested DrawingML picture effect".into(),
        ));
    }
    if frame.picture_stretch_open {
        if !is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "fillRect", local) {
            return Err(PocError::UnsupportedSemanticConstruct(
                "unsupported DrawingML picture stretch parameter".into(),
            ));
        }
        if frame.picture_fill_rect_seen {
            return Err(PocError::UnsupportedSemanticConstruct(
                "multiple picture stretch fill rectangles".into(),
            ));
        }
        checked_unqualified_attributes(event, &[])?;
        frame.picture_fill_rect_seen = true;
        frame.picture_fill_rect_open = !empty;
        return Ok(());
    }

    if frame.inside_picture_blip_fill {
        if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "blip", local) {
            if frame.picture_blip_seen {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "multiple picture image sources".into(),
                ));
            }
            frame.picture_blip_seen = true;
            frame.picture_blip_open = !empty;
        } else if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "srcRect", local) {
            parse_picture_source_rect(frame, event)?;
            frame.picture_src_rect_open = !empty;
        } else if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "stretch", local) {
            if frame.picture_fill_mode_seen {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "multiple picture fill modes".into(),
                ));
            }
            if empty {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "empty picture stretch has no fill rectangle".into(),
                ));
            }
            checked_unqualified_attributes(event, &[])?;
            frame.picture_fill_mode_seen = true;
            frame.picture_stretch_open = true;
            frame.picture_fill_rect_seen = false;
        } else if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "tile", local) {
            return Err(PocError::UnsupportedSemanticConstruct(
                "unsupported DrawingML tiled picture fill".into(),
            ));
        } else {
            return Err(PocError::UnsupportedSemanticConstruct(
                "unsupported DrawingML picture fill construct".into(),
            ));
        }
        return Ok(());
    }

    if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "srcRect", local) {
        return Err(PocError::UnsupportedSemanticConstruct(
            "picture source rectangle is outside a picture fill".into(),
        ));
    } else if is_resolved_qname(namespace, PRESENTATION_NS, "blipFill", local) {
        if frame.picture_blip_fill_seen {
            return Err(PocError::UnsupportedSemanticConstruct(
                "multiple picture fill definitions".into(),
            ));
        }
        if empty {
            return Err(PocError::UnsupportedSemanticConstruct(
                "empty picture blip fill has no supported source or fill mode".into(),
            ));
        }
        frame.picture_blip_fill_seen = true;
        frame.inside_picture_blip_fill = true;
    } else if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "xfrm", local) {
        if frame.picture_transform_seen {
            return Err(PocError::UnsupportedSemanticConstruct(
                "multiple picture transforms".into(),
            ));
        }
        frame.picture_transform_seen = true;
        parse_picture_transform(frame, event)?;
    } else if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "blip", local)
        || is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "stretch", local)
        || is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "tile", local)
        || is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "fillRect", local)
    {
        return Err(PocError::UnsupportedSemanticConstruct(
            "picture fill construct is outside a picture blip fill".into(),
        ));
    }
    Ok(())
}

fn handle_picture_transform_end(
    frame: &mut PptxShapeFrame,
    namespace: &ResolveResult<'_>,
    local: &str,
) -> Result<(), PocError> {
    if frame.kind != PptxShapeKind::Picture {
        if frame.picture_src_rect_open {
            return Err(PocError::UnsupportedSemanticConstruct(
                "nested XML inside picture source rectangle".into(),
            ));
        }
        return Ok(());
    }
    if frame.picture_fill_rect_open {
        if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "fillRect", local) {
            frame.picture_fill_rect_open = false;
            return Ok(());
        }
        return Err(PocError::UnsupportedSemanticConstruct(
            "nested XML inside picture stretch fill rectangle".into(),
        ));
    }
    if frame.picture_blip_open {
        if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "blip", local) {
            frame.picture_blip_open = false;
            return Ok(());
        }
        return Err(PocError::UnsupportedSemanticConstruct(
            "unsupported nested DrawingML picture effect".into(),
        ));
    }
    if frame.picture_stretch_open {
        if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "stretch", local) {
            frame.picture_stretch_open = false;
            if !frame.picture_fill_rect_seen {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "picture stretch is missing its fill rectangle".into(),
                ));
            }
            return Ok(());
        }
        return Err(PocError::UnsupportedSemanticConstruct(
            "unsupported DrawingML picture stretch parameter".into(),
        ));
    }
    if frame.picture_src_rect_open {
        if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "srcRect", local) {
            frame.picture_src_rect_open = false;
            return Ok(());
        }
        return Err(PocError::UnsupportedSemanticConstruct(
            "nested XML inside picture source rectangle".into(),
        ));
    }
    if is_resolved_qname(namespace, PRESENTATION_NS, "blipFill", local) {
        if !frame.inside_picture_blip_fill {
            return Err(PocError::SemanticExtractionFailed(
                "picture fill closes without an opening element".into(),
            ));
        }
        if !frame.picture_blip_seen || !frame.picture_fill_mode_seen {
            return Err(PocError::UnsupportedSemanticConstruct(
                "picture blip fill is missing a source or supported fill mode".into(),
            ));
        }
        frame.inside_picture_blip_fill = false;
    } else if frame.inside_picture_blip_fill {
        if is_resolved_qname(namespace, DRAWINGML_MAIN_NS, "srcRect", local) {
            return Ok(());
        }
        return Err(PocError::UnsupportedSemanticConstruct(
            "unexpected element closing inside picture blip fill".into(),
        ));
    }
    Ok(())
}

fn parse_picture_transform(
    frame: &mut PptxShapeFrame,
    event: &BytesStart<'_>,
) -> Result<(), PocError> {
    let values = checked_unqualified_attributes(event, &["rot", "flipH", "flipV"])?;
    if let Some(rotation) = values.get("rot") {
        let rotation = rotation
            .parse::<i32>()
            .map_err(|_| PocError::SemanticExtractionFailed("invalid picture rotation".into()))?;
        frame.picture_transform.rotation = (i64::from(rotation).rem_euclid(21_600_000)) as u32;
    }
    if let Some(value) = values.get("flipH") {
        frame.picture_transform.flip_horizontal = parse_on_off(value, "horizontal picture flip")?;
    }
    if let Some(value) = values.get("flipV") {
        frame.picture_transform.flip_vertical = parse_on_off(value, "vertical picture flip")?;
    }
    Ok(())
}

fn parse_picture_source_rect(
    frame: &mut PptxShapeFrame,
    event: &BytesStart<'_>,
) -> Result<(), PocError> {
    if !frame.inside_picture_blip_fill {
        return Err(PocError::UnsupportedSemanticConstruct(
            "picture source rectangle is outside a picture fill".into(),
        ));
    }
    if frame.picture_src_rect_seen {
        return Err(PocError::UnsupportedSemanticConstruct(
            "multiple picture source rectangles".into(),
        ));
    }
    let values = checked_unqualified_attributes(event, &["l", "t", "r", "b"])?;
    let crop = |name: &str| -> Result<u32, PocError> {
        let Some(value) = values.get(name) else {
            return Ok(0);
        };
        let value = value.parse::<u32>().map_err(|_| {
            PocError::SemanticExtractionFailed(format!("invalid picture crop {name}"))
        })?;
        if value > 100_000 {
            return Err(PocError::UnsupportedSemanticConstruct(
                "picture crop is outside the supported percentage range".into(),
            ));
        }
        Ok(value)
    };
    let left = crop("l")?;
    let top = crop("t")?;
    let right = crop("r")?;
    let bottom = crop("b")?;
    if left + right >= 100_000 || top + bottom >= 100_000 {
        return Err(PocError::UnsupportedSemanticConstruct(
            "picture crop leaves no visible image area".into(),
        ));
    }
    frame.picture_transform.crop_left = left;
    frame.picture_transform.crop_top = top;
    frame.picture_transform.crop_right = right;
    frame.picture_transform.crop_bottom = bottom;
    frame.picture_src_rect_seen = true;
    Ok(())
}

fn checked_unqualified_attributes(
    event: &BytesStart<'_>,
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, PocError> {
    let mut result = BTreeMap::new();
    let mut attributes = event.attributes();
    attributes.with_checks(true);
    for item in attributes {
        let item = item.map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid picture XML attribute: {error}"))
        })?;
        let name = item.key.as_ref();
        let value = item
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|error| {
                PocError::SemanticExtractionFailed(format!(
                    "invalid picture XML attribute value: {error}"
                ))
            })?;
        if name == "xmlns" || name.starts_with("xmlns:") {
            if value.as_ref() == DRAWINGML_MAIN_NS {
                continue;
            }
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "unsupported local picture namespace declaration {name}"
            )));
        }
        if !allowed.contains(&name) {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "unsupported picture transform attribute {name}"
            )));
        }
        if result.insert(name.to_owned(), value.into_owned()).is_some() {
            return Err(PocError::SemanticExtractionFailed(format!(
                "duplicate picture XML attribute {name}"
            )));
        }
    }
    Ok(result)
}

fn parse_on_off(value: &str, description: &str) -> Result<bool, PocError> {
    match value {
        "1" | "true" | "on" => Ok(true),
        "0" | "false" | "off" => Ok(false),
        _ => Err(PocError::SemanticExtractionFailed(format!(
            "invalid {description} value"
        ))),
    }
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
                    PocError::SemanticExtractionFailed(format!(
                        "SmartArt relationship {rid} missing"
                    ))
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
                return Err(PocError::SemanticExtractionFailed(format!(
                    "slide graphic XML: {error}"
                )));
            }
        }
    }
    Ok(result)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartSeriesSection {
    Name,
    Categories,
    Values,
}

#[derive(Clone)]
struct ActiveChartGroup {
    ordinal: usize,
    name: String,
}

struct ChartSeries {
    group_ordinal: usize,
    group_name: String,
    index: Option<u32>,
    order: Option<u32>,
    direct_names: Vec<String>,
    indexed_names: BTreeMap<u32, String>,
    categories: BTreeMap<u32, String>,
    values: BTreeMap<u32, String>,
    section: Option<ChartSeriesSection>,
    current_point: Option<(ChartSeriesSection, u32, bool)>,
    saw_categories: bool,
    saw_values: bool,
}

impl ChartSeries {
    fn new(group: &ActiveChartGroup) -> Self {
        Self {
            group_ordinal: group.ordinal,
            group_name: group.name.clone(),
            index: None,
            order: None,
            direct_names: Vec::new(),
            indexed_names: BTreeMap::new(),
            categories: BTreeMap::new(),
            values: BTreeMap::new(),
            section: None,
            current_point: None,
            saw_categories: false,
            saw_values: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartLabelKind {
    Group,
    Point,
}

struct ActiveChartDataLabels {
    kind: ChartLabelKind,
    group_ordinal: usize,
    group_name: String,
    series_index: Option<u32>,
    point_index: Option<u32>,
    settings: BTreeMap<String, Value>,
    seen_settings: BTreeSet<String>,
}

struct ActiveChartTitle {
    scope: String,
    text: Vec<String>,
}

struct ActiveChartLegend {
    position: String,
    overlay: bool,
    seen_settings: BTreeSet<String>,
    open_leaf: Option<String>,
}

impl ActiveChartLegend {
    fn new() -> Self {
        Self {
            position: "r".into(),
            overlay: false,
            seen_settings: BTreeSet::new(),
            open_leaf: None,
        }
    }

    fn projection(self) -> Value {
        json!({
            "position": self.position,
            "overlay": self.overlay,
        })
    }
}

fn parse_chart_semantic(data: &[u8]) -> Result<Value, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("chart XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut element_stack = Vec::<Option<String>>::new();
    let mut extension_depth = None;
    let mut root_seen = false;
    let mut next_group_ordinal = 0usize;
    let mut chart_groups = Vec::<ActiveChartGroup>::new();
    let mut all_chart_groups = Vec::<ActiveChartGroup>::new();
    let mut series = Vec::<ChartSeries>::new();
    let mut seen_series_indices = BTreeSet::<(usize, u32)>::new();
    let mut seen_series_orders = BTreeSet::<(usize, u32)>::new();
    let mut current_series = None::<ChartSeries>;
    let mut labels_stack = Vec::<ActiveChartDataLabels>::new();
    let mut data_labels = Vec::<ActiveChartDataLabels>::new();
    let mut active_title = None::<ActiveChartTitle>;
    let mut titles = Vec::<Value>::new();
    let mut chart_legend = None::<ActiveChartLegend>;
    let mut saw_chart_legend = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let (namespace, local) = reader.resolver().resolve_element(event.name());
                let local = local.as_ref().to_owned();
                let is_chart = matches!(
                    namespace,
                    ResolveResult::Bound(ref uri) if uri.as_ref() == CHART_NS
                );
                let is_drawing = matches!(
                    namespace,
                    ResolveResult::Bound(ref uri) if uri.as_ref() == DRAWINGML_MAIN_NS
                );
                if !is_chart && !is_drawing {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "chart XML element {{{}}}{local} is not a supported ChartML or DrawingML element",
                        match namespace {
                            ResolveResult::Bound(uri) => uri.as_ref().to_owned(),
                            ResolveResult::Unbound => String::new(),
                            ResolveResult::Unknown(prefix) => {
                                String::from_utf8_lossy(prefix.as_ref()).into_owned()
                            }
                        }
                    )));
                }
                if is_chart && is_unmodeled_visible_chart_construct(&local) {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "ChartML element {local} affects visible chart semantics but is not modeled"
                    )));
                }
                if !root_seen {
                    if !is_chart || local != "chartSpace" {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "chart part root is not ChartML chartSpace".into(),
                        ));
                    }
                    root_seen = true;
                }
                if extension_depth.is_some_and(|depth| element_stack.len() >= depth) {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "PPTX chart extension payload is not understood".into(),
                    ));
                }
                if is_chart && !labels_stack.is_empty() && !chart_label_element_supported(&local) {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "unsupported chart data-label element {local}"
                    )));
                }
                if is_chart && local == "ext" {
                    if element_stack.last().and_then(Option::as_deref) != Some("extLst") {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "PPTX chart extension is outside extLst".into(),
                        ));
                    }
                    extension_depth = Some(element_stack.len() + 1);
                }

                let chart_local = is_chart.then(|| local.clone());
                let parent_chart_path = element_stack
                    .iter()
                    .filter_map(|name| name.as_deref().map(str::to_owned))
                    .collect::<Vec<_>>();
                let inside_chart_legend = parent_chart_path.iter().any(|name| name == "legend");
                if inside_chart_legend {
                    if !is_chart {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "non-ChartML content inside chart legend is unsupported".into(),
                        ));
                    }
                    if !matches!(local.as_str(), "legendPos" | "overlay") {
                        return Err(PocError::UnsupportedSemanticConstruct(format!(
                            "unsupported chart legend element {local}"
                        )));
                    }
                    let legend = chart_legend.as_mut().ok_or_else(|| {
                        PocError::SemanticExtractionFailed(
                            "chart legend settings have no legend scope".into(),
                        )
                    })?;
                    if legend.open_leaf.is_some()
                        || parent_chart_path.last().map(String::as_str) != Some("legend")
                    {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "nested chart legend setting is unsupported".into(),
                        ));
                    }
                    set_chart_legend_setting(legend, &local, &event)?;
                    legend.open_leaf = Some(local.clone());
                }
                element_stack.push(chart_local);

                if is_chart && local == "legend" {
                    if parent_chart_path.last().map(String::as_str) != Some("chart") {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "chart legend is outside the chart element".into(),
                        ));
                    }
                    if std::mem::replace(&mut saw_chart_legend, true) {
                        return Err(PocError::SemanticExtractionFailed(
                            "duplicate chart legend".into(),
                        ));
                    }
                    chart_legend = Some(ActiveChartLegend::new());
                } else if inside_chart_legend {
                    // Legend settings are captured before pushing the element scope above.
                } else if is_chart && is_chart_type(&local) {
                    let group = ActiveChartGroup {
                        ordinal: next_group_ordinal,
                        name: local.clone(),
                    };
                    all_chart_groups.push(group.clone());
                    chart_groups.push(group);
                    next_group_ordinal += 1;
                } else if is_chart && local == "ser" {
                    if current_series.is_some() {
                        return Err(PocError::SemanticExtractionFailed(
                            "nested chart series".into(),
                        ));
                    }
                    let group = chart_groups.last().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart series is outside a chart type group".into(),
                        )
                    })?;
                    current_series = Some(ChartSeries::new(group));
                } else if is_chart && local == "title" {
                    if active_title.is_some() {
                        return Err(PocError::SemanticExtractionFailed(
                            "nested chart title".into(),
                        ));
                    }
                    let scope = parent_chart_path
                        .into_iter()
                        .filter(|name| name != "chartSpace")
                        .collect::<Vec<_>>()
                        .join("/");
                    active_title = Some(ActiveChartTitle {
                        scope,
                        text: Vec::new(),
                    });
                } else if is_chart && local == "dLbls" {
                    if labels_stack
                        .last()
                        .is_some_and(|labels| labels.kind == ChartLabelKind::Group)
                    {
                        return Err(PocError::SemanticExtractionFailed(
                            "nested chart data-label groups".into(),
                        ));
                    }
                    let group = chart_groups.last().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart data labels are outside a chart type group".into(),
                        )
                    })?;
                    labels_stack.push(ActiveChartDataLabels {
                        kind: ChartLabelKind::Group,
                        group_ordinal: group.ordinal,
                        group_name: group.name.clone(),
                        series_index: current_series.as_ref().and_then(|series| series.index),
                        point_index: None,
                        settings: BTreeMap::new(),
                        seen_settings: BTreeSet::new(),
                    });
                } else if is_chart && local == "dLbl" {
                    let parent = labels_stack.last().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "individual chart data label is outside dLbls".into(),
                        )
                    })?;
                    if parent.kind != ChartLabelKind::Group {
                        return Err(PocError::SemanticExtractionFailed(
                            "nested individual chart data labels".into(),
                        ));
                    }
                    labels_stack.push(ActiveChartDataLabels {
                        kind: ChartLabelKind::Point,
                        group_ordinal: parent.group_ordinal,
                        group_name: parent.group_name.clone(),
                        series_index: parent.series_index,
                        point_index: None,
                        settings: BTreeMap::new(),
                        seen_settings: BTreeSet::new(),
                    });
                } else if is_chart && local == "tx" && labels_stack.is_empty() {
                    if let Some(series) = current_series.as_mut() {
                        series.section = Some(ChartSeriesSection::Name);
                    }
                } else if is_chart && local == "cat" && labels_stack.is_empty() {
                    if let Some(series) = current_series.as_mut() {
                        if series.saw_categories {
                            return Err(PocError::SemanticExtractionFailed(
                                "duplicate chart category cache".into(),
                            ));
                        }
                        series.saw_categories = true;
                        series.section = Some(ChartSeriesSection::Categories);
                    }
                } else if is_chart && local == "val" && labels_stack.is_empty() {
                    if let Some(series) = current_series.as_mut() {
                        if series.saw_values {
                            return Err(PocError::SemanticExtractionFailed(
                                "duplicate chart value cache".into(),
                            ));
                        }
                        series.saw_values = true;
                        series.section = Some(ChartSeriesSection::Values);
                    }
                } else if is_chart && local == "pt" && labels_stack.is_empty() {
                    if let Some(series) = current_series.as_mut() {
                        if let Some(section) = series.section {
                            if series.current_point.is_some() {
                                return Err(PocError::SemanticExtractionFailed(
                                    "nested chart cache points".into(),
                                ));
                            }
                            let index = chart_u32_attribute(&event, "idx", "chart point index")?;
                            series.current_point = Some((section, index, false));
                        }
                    }
                } else if is_chart && local == "idx" {
                    let index = chart_u32_attribute(&event, "val", "chart series index")?;
                    if let Some(labels) = labels_stack.last_mut() {
                        if labels.kind != ChartLabelKind::Point
                            || labels.point_index.replace(index).is_some()
                        {
                            return Err(PocError::SemanticExtractionFailed(
                                "duplicate or unscoped chart data-label point index".into(),
                            ));
                        }
                    } else if let Some(series) = current_series.as_mut() {
                        if series.index.replace(index).is_some() {
                            return Err(PocError::SemanticExtractionFailed(
                                "duplicate chart series index".into(),
                            ));
                        }
                    }
                } else if is_chart && local == "order" {
                    let order = chart_u32_attribute(&event, "val", "chart series order")?;
                    let series = current_series.as_mut().ok_or_else(|| {
                        PocError::SemanticExtractionFailed(
                            "chart series order is outside a series".into(),
                        )
                    })?;
                    if series.order.replace(order).is_some() {
                        return Err(PocError::SemanticExtractionFailed(
                            "duplicate chart series order".into(),
                        ));
                    }
                } else if is_chart && is_chart_data_label_flag(&local) {
                    let enabled = chart_boolean_attribute(&event, "val", &local)?;
                    if labels_stack.is_empty() {
                        return Err(PocError::UnsupportedSemanticConstruct(format!(
                            "chart data-label setting {local} is outside dLbls"
                        )));
                    }
                    set_chart_label_setting(
                        labels_stack.last_mut().expect("checked nonempty"),
                        &local,
                        Some(json!(enabled)),
                    )?;
                } else if is_chart && local == "dLblPos" {
                    let value = attr_required(&event, "val")?;
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart data-label position is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "position", Some(json!(value)))?;
                } else if is_chart && local == "delete" {
                    let deleted = chart_boolean_attribute(&event, "val", "delete")?;
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart data-label delete flag is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "delete", Some(json!(deleted)))?;
                } else if is_chart && local == "numFmt" {
                    let format = attr_required(&event, "formatCode")?;
                    let source_linked = attr(&event, "sourceLinked")?
                        .map(|value| parse_chart_boolean(&value, "numFmt sourceLinked"))
                        .transpose()?;
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart data-label number format is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(
                        labels,
                        "number_format",
                        Some(json!({"format_code": format, "source_linked": source_linked})),
                    )?;
                } else if is_chart && local == "leaderLines" {
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart leader lines are outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "leader_lines", Some(json!(true)))?;
                } else if is_chart && local == "separator" {
                    let value = reader
                        .read_text(event.name())
                        .map_err(|error| {
                            PocError::SemanticExtractionFailed(format!(
                                "chart label separator XML: {error}"
                            ))
                        })?
                        .to_string();
                    element_stack.pop();
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart label separator is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "separator", Some(json!(value)))?;
                } else if (is_chart && local == "v") || (is_drawing && local == "t") {
                    let value = reader
                        .read_text(event.name())
                        .map_err(|error| {
                            PocError::SemanticExtractionFailed(format!("chart text XML: {error}"))
                        })?
                        .to_string();
                    element_stack.pop();
                    let in_label_text = !labels_stack.is_empty()
                        && element_stack
                            .iter()
                            .any(|name| name.as_deref() == Some("tx"));
                    if let Some(title) = active_title.as_mut() {
                        if !value.trim().is_empty() {
                            title.text.push(value);
                        }
                    } else if in_label_text {
                        let labels = labels_stack.last_mut().expect("checked nonempty");
                        let text_values = labels
                            .settings
                            .entry("custom_text".into())
                            .or_insert_with(|| json!([]));
                        text_values
                            .as_array_mut()
                            .expect("custom_text is initialized as an array")
                            .push(json!(value));
                    } else if let Some(series) = current_series.as_mut() {
                        match (series.section, series.current_point.as_mut()) {
                            (Some(ChartSeriesSection::Name), Some((_, index, seen))) => {
                                if *seen {
                                    return Err(PocError::SemanticExtractionFailed(
                                        "duplicate chart series name cache point".into(),
                                    ));
                                }
                                if series.indexed_names.insert(*index, value).is_some() {
                                    return Err(PocError::SemanticExtractionFailed(
                                        "duplicate chart series name cache index".into(),
                                    ));
                                }
                                *seen = true;
                            }
                            (Some(ChartSeriesSection::Categories), Some((_, index, seen))) => {
                                if *seen || series.categories.insert(*index, value).is_some() {
                                    return Err(PocError::SemanticExtractionFailed(
                                        "duplicate chart category point".into(),
                                    ));
                                }
                                *seen = true;
                            }
                            (Some(ChartSeriesSection::Values), Some((_, index, seen))) => {
                                if *seen || series.values.insert(*index, value).is_some() {
                                    return Err(PocError::SemanticExtractionFailed(
                                        "duplicate chart value point".into(),
                                    ));
                                }
                                *seen = true;
                            }
                            (Some(ChartSeriesSection::Name), None) => {
                                series.direct_names.push(value);
                            }
                            (
                                Some(ChartSeriesSection::Categories | ChartSeriesSection::Values),
                                None,
                            ) => {
                                return Err(PocError::SemanticExtractionFailed(
                                    "chart category or value is outside an indexed point".into(),
                                ));
                            }
                            (None, _) => {}
                        }
                    }
                }
            }
            Ok(Event::Empty(event)) => {
                let (namespace, local) = reader.resolver().resolve_element(event.name());
                let local = local.as_ref().to_owned();
                let is_chart = matches!(
                    namespace,
                    ResolveResult::Bound(ref uri) if uri.as_ref() == CHART_NS
                );
                let is_drawing = matches!(
                    namespace,
                    ResolveResult::Bound(ref uri) if uri.as_ref() == DRAWINGML_MAIN_NS
                );
                if !is_chart && !is_drawing {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "chart XML empty element {local} has an unsupported namespace"
                    )));
                }
                if is_chart && is_unmodeled_visible_chart_construct(&local) {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "ChartML element {local} affects visible chart semantics but is not modeled"
                    )));
                }
                if !root_seen && (!is_chart || local != "chartSpace") {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "chart part root is not ChartML chartSpace".into(),
                    ));
                }
                if extension_depth.is_some_and(|depth| element_stack.len() >= depth) {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "PPTX chart extension payload is not understood".into(),
                    ));
                }
                let inside_chart_legend = element_stack
                    .iter()
                    .any(|name| name.as_deref() == Some("legend"));
                if inside_chart_legend {
                    if !is_chart {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "non-ChartML content inside chart legend is unsupported".into(),
                        ));
                    }
                    if !matches!(local.as_str(), "legendPos" | "overlay") {
                        return Err(PocError::UnsupportedSemanticConstruct(format!(
                            "unsupported chart legend element {local}"
                        )));
                    }
                    if element_stack.last().and_then(Option::as_deref) != Some("legend") {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "nested chart legend setting is unsupported".into(),
                        ));
                    }
                    let legend = chart_legend.as_mut().ok_or_else(|| {
                        PocError::SemanticExtractionFailed(
                            "chart legend settings have no legend scope".into(),
                        )
                    })?;
                    set_chart_legend_setting(legend, &local, &event)?;
                    continue;
                }
                if is_chart && local == "legend" {
                    if element_stack.last().and_then(Option::as_deref) != Some("chart") {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "chart legend is outside the chart element".into(),
                        ));
                    }
                    if std::mem::replace(&mut saw_chart_legend, true) {
                        return Err(PocError::SemanticExtractionFailed(
                            "duplicate chart legend".into(),
                        ));
                    }
                    chart_legend = Some(ActiveChartLegend::new());
                    continue;
                }
                if is_chart && is_chart_type(&local) {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "empty chart type group is unsupported".into(),
                    ));
                }
                if is_chart && matches!(local.as_str(), "ser" | "pt" | "title") {
                    return Err(PocError::SemanticExtractionFailed(format!(
                        "empty chart {local} element is malformed"
                    )));
                }
                if is_chart && local == "idx" {
                    let index = chart_u32_attribute(&event, "val", "chart index")?;
                    if let Some(labels) = labels_stack.last_mut() {
                        if labels.kind != ChartLabelKind::Point
                            || labels.point_index.replace(index).is_some()
                        {
                            return Err(PocError::SemanticExtractionFailed(
                                "duplicate or unscoped chart data-label point index".into(),
                            ));
                        }
                    } else if let Some(series) = current_series.as_mut() {
                        if series.index.replace(index).is_some() {
                            return Err(PocError::SemanticExtractionFailed(
                                "duplicate chart series index".into(),
                            ));
                        }
                    }
                } else if is_chart && local == "order" {
                    let order = chart_u32_attribute(&event, "val", "chart series order")?;
                    let series = current_series.as_mut().ok_or_else(|| {
                        PocError::SemanticExtractionFailed(
                            "chart series order is outside a series".into(),
                        )
                    })?;
                    if series.order.replace(order).is_some() {
                        return Err(PocError::SemanticExtractionFailed(
                            "duplicate chart series order".into(),
                        ));
                    }
                } else if is_chart && is_chart_data_label_flag(&local) {
                    let enabled = chart_boolean_attribute(&event, "val", &local)?;
                    if labels_stack.is_empty() {
                        return Err(PocError::UnsupportedSemanticConstruct(format!(
                            "chart data-label setting {local} is outside dLbls"
                        )));
                    }
                    set_chart_label_setting(
                        labels_stack.last_mut().expect("checked nonempty"),
                        &local,
                        Some(json!(enabled)),
                    )?;
                } else if is_chart && local == "dLblPos" {
                    let value = attr_required(&event, "val")?;
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart data-label position is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "position", Some(json!(value)))?;
                } else if is_chart && local == "delete" {
                    let deleted = chart_boolean_attribute(&event, "val", "delete")?;
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart data-label delete flag is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "delete", Some(json!(deleted)))?;
                } else if is_chart && local == "numFmt" {
                    let format = attr_required(&event, "formatCode")?;
                    let source_linked = attr(&event, "sourceLinked")?
                        .map(|value| parse_chart_boolean(&value, "numFmt sourceLinked"))
                        .transpose()?;
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart data-label number format is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(
                        labels,
                        "number_format",
                        Some(json!({"format_code": format, "source_linked": source_linked})),
                    )?;
                } else if is_chart && local == "leaderLines" {
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart leader lines are outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "leader_lines", Some(json!(true)))?;
                } else if is_chart && local == "separator" {
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PocError::UnsupportedSemanticConstruct(
                            "chart label separator is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "separator", Some(json!("")))?;
                } else if is_chart && local == "ext" {
                    if element_stack.last().and_then(Option::as_deref) != Some("extLst") {
                        return Err(PocError::UnsupportedSemanticConstruct(
                            "PPTX chart extension is outside extLst".into(),
                        ));
                    }
                } else if is_chart && is_semantic_chart_label_setting(&local) {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "unsupported chart data-label setting {local}"
                    )));
                }
            }
            Ok(Event::End(_event)) => {
                let current_depth = element_stack.len();
                let local = element_stack.pop().ok_or_else(|| {
                    PocError::SemanticExtractionFailed("chart XML has an unmatched end tag".into())
                })?;
                if extension_depth == Some(current_depth) {
                    if local.as_deref() != Some("ext") {
                        return Err(PocError::SemanticExtractionFailed(
                            "chart extension boundary is malformed".into(),
                        ));
                    }
                    extension_depth = None;
                }
                let Some(local) = local else {
                    continue;
                };
                if let Some(legend) = chart_legend.as_mut() {
                    if legend.open_leaf.as_deref() == Some(local.as_str()) {
                        legend.open_leaf = None;
                    } else if local == "legend" && legend.open_leaf.is_some() {
                        return Err(PocError::SemanticExtractionFailed(
                            "chart legend ended inside a setting element".into(),
                        ));
                    }
                }
                match local.as_str() {
                    "title" => {
                        if let Some(title) = active_title.take() {
                            if !title.text.is_empty() {
                                titles.push(json!({"scope": title.scope, "text": title.text}));
                            }
                        }
                    }
                    "tx" | "cat" | "val" => {
                        if labels_stack.is_empty() {
                            if let Some(series) = current_series.as_mut() {
                                series.section = None;
                            }
                        }
                    }
                    "pt" => {
                        if labels_stack.is_empty()
                            && let Some(series) = current_series.as_mut()
                            && let Some((_, _, saw_value)) = series.current_point.take()
                            && !saw_value
                        {
                            return Err(PocError::SemanticExtractionFailed(
                                "chart cache point has no value".into(),
                            ));
                        }
                    }
                    "dLbl" | "dLbls" => {
                        let expected = if local == "dLbl" {
                            ChartLabelKind::Point
                        } else {
                            ChartLabelKind::Group
                        };
                        let labels = labels_stack.pop().ok_or_else(|| {
                            PocError::SemanticExtractionFailed(format!(
                                "chart {local} closing tag has no matching scope"
                            ))
                        })?;
                        if labels.kind != expected {
                            return Err(PocError::SemanticExtractionFailed(format!(
                                "chart {local} scopes are improperly nested"
                            )));
                        }
                        if !labels.settings.is_empty() {
                            data_labels.push(labels);
                        }
                    }
                    "ser" => {
                        let current = current_series.take().ok_or_else(|| {
                            PocError::SemanticExtractionFailed(
                                "chart series closing tag has no matching series".into(),
                            )
                        })?;
                        if current.current_point.is_some() {
                            return Err(PocError::SemanticExtractionFailed(
                                "chart series ended inside a cache point".into(),
                            ));
                        }
                        let index = current.index.ok_or_else(|| {
                            PocError::UnsupportedSemanticConstruct(
                                "chart series has no stable idx".into(),
                            )
                        })?;
                        let order = current.order.ok_or_else(|| {
                            PocError::UnsupportedSemanticConstruct(
                                "chart series has no visible order".into(),
                            )
                        })?;
                        if !seen_series_indices.insert((current.group_ordinal, index)) {
                            return Err(PocError::SemanticExtractionFailed(
                                "duplicate chart series idx in one chart group".into(),
                            ));
                        }
                        if !seen_series_orders.insert((current.group_ordinal, order)) {
                            return Err(PocError::SemanticExtractionFailed(
                                "duplicate chart series order in one chart group".into(),
                            ));
                        }
                        series.push(current);
                    }
                    name if is_chart_type(name) => {
                        let group = chart_groups.pop().ok_or_else(|| {
                            PocError::SemanticExtractionFailed(
                                "chart type closing tag has no matching group".into(),
                            )
                        })?;
                        if group.name != name {
                            return Err(PocError::SemanticExtractionFailed(
                                "chart type groups are improperly nested".into(),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(event)) => {
                if element_stack
                    .iter()
                    .any(|name| name.as_deref() == Some("legend"))
                    && !event.as_ref().trim().is_empty()
                {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "chart legend text content is unsupported".into(),
                    ));
                }
                if extension_depth.is_some() && !event.as_ref().trim().is_empty() {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "PPTX chart extension text is not understood".into(),
                    ));
                }
            }
            Ok(Event::CData(event)) => {
                if element_stack
                    .iter()
                    .any(|name| name.as_deref() == Some("legend"))
                    && !event.as_ref().trim().is_empty()
                {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "chart legend CDATA is unsupported".into(),
                    ));
                }
                if extension_depth.is_some() && !event.as_ref().trim().is_empty() {
                    return Err(PocError::UnsupportedSemanticConstruct(
                        "PPTX chart extension text is not understood".into(),
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "chart XML: {error}"
                )));
            }
        }
    }

    if !root_seen
        || !element_stack.is_empty()
        || extension_depth.is_some()
        || current_series.is_some()
        || !labels_stack.is_empty()
        || active_title.is_some()
        || chart_legend
            .as_ref()
            .is_some_and(|legend| legend.open_leaf.is_some())
        || !chart_groups.is_empty()
    {
        return Err(PocError::SemanticExtractionFailed(
            "chart XML ended with an incomplete semantic structure".into(),
        ));
    }

    series.sort_by_key(|series| (series.group_ordinal, series.order.unwrap_or(u32::MAX)));
    let series = series
        .into_iter()
        .map(|series| {
            let index = series.index.ok_or_else(|| {
                PocError::SemanticExtractionFailed("chart series index disappeared".into())
            })?;
            let order = series.order.ok_or_else(|| {
                PocError::SemanticExtractionFailed("chart series order disappeared".into())
            })?;
            let name = if series.indexed_names.is_empty() {
                series.direct_names
            } else {
                series.indexed_names.into_values().collect()
            };
            let point_indices = series
                .categories
                .keys()
                .chain(series.values.keys())
                .copied()
                .collect::<BTreeSet<_>>();
            let points = point_indices
                .into_iter()
                .map(|point_index| {
                    json!({
                        "index": point_index,
                        "category": series.categories.get(&point_index),
                        "value": series.values.get(&point_index),
                    })
                })
                .collect::<Vec<_>>();
            Ok(json!({
                "chart_group": series.group_ordinal,
                "chart_type": series.group_name,
                "idx": index,
                "order": order,
                "name": name,
                "points": points,
            }))
        })
        .collect::<Result<Vec<_>, PocError>>()?;

    data_labels.sort_by(|left, right| {
        let left_key = (
            left.group_ordinal,
            left.series_index,
            left.point_index,
            serde_json::to_string(&left.settings).unwrap_or_default(),
        );
        let right_key = (
            right.group_ordinal,
            right.series_index,
            right.point_index,
            serde_json::to_string(&right.settings).unwrap_or_default(),
        );
        left_key.cmp(&right_key)
    });
    let data_labels = data_labels
        .into_iter()
        .map(|labels| {
            json!({
                "chart_group": labels.group_ordinal,
                "chart_type": labels.group_name,
                "series_idx": labels.series_index,
                "point_idx": labels.point_index,
                "settings": labels.settings,
            })
        })
        .collect::<Vec<_>>();
    all_chart_groups.sort_by_key(|group| group.ordinal);
    let chart_groups = all_chart_groups
        .into_iter()
        .map(|group| json!({"order": group.ordinal, "type": group.name}))
        .collect::<Vec<_>>();
    titles.sort_by_key(|title| title["scope"].as_str().unwrap_or_default().to_owned());
    let mut projection = json!({
        "chart_groups": chart_groups,
        "series": series,
        "titles": titles,
        "data_labels": data_labels,
    });
    if let Some(legend) = chart_legend.map(ActiveChartLegend::projection) {
        projection["legend"] = legend;
    }
    Ok(projection)
}

fn set_chart_legend_setting(
    legend: &mut ActiveChartLegend,
    name: &str,
    event: &BytesStart<'_>,
) -> Result<(), PocError> {
    if !legend.seen_settings.insert(name.to_owned()) {
        return Err(PocError::SemanticExtractionFailed(format!(
            "duplicate chart legend setting {name}"
        )));
    }
    match name {
        "legendPos" => {
            let position = chart_legend_value_attribute(event, "legendPos")?;
            if !matches!(position.as_str(), "b" | "l" | "r" | "t" | "tr") {
                return Err(PocError::SemanticExtractionFailed(
                    "invalid chart legend position".into(),
                ));
            }
            legend.position = position;
        }
        "overlay" => {
            let value = chart_legend_value_attribute(event, "overlay")?;
            legend.overlay = parse_chart_boolean(&value, "legend overlay")?;
        }
        _ => {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "unsupported chart legend setting {name}"
            )));
        }
    }
    Ok(())
}

fn chart_legend_value_attribute(event: &BytesStart<'_>, setting: &str) -> Result<String, PocError> {
    let mut value = None;
    let mut attributes = event.attributes();
    attributes.with_checks(true);
    for item in attributes {
        let item = item.map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid chart legend attribute: {error}"))
        })?;
        if item.key.as_ref() != "val" {
            return Err(PocError::SemanticExtractionFailed(format!(
                "unsupported attribute on chart legend {setting}"
            )));
        }
        value = Some(item.value.as_ref().to_owned());
    }
    value.ok_or_else(|| {
        PocError::SemanticExtractionFailed(format!(
            "missing required unqualified val attribute on chart legend {setting}"
        ))
    })
}

fn is_unmodeled_visible_chart_construct(name: &str) -> bool {
    matches!(
        name,
        "max" | "dTable" | "trendline" | "errBars" | "plotVisOnly" | "dispBlanksAs"
    )
}

fn is_chart_type(name: &str) -> bool {
    matches!(
        name,
        "areaChart"
            | "area3DChart"
            | "barChart"
            | "bar3DChart"
            | "bubbleChart"
            | "doughnutChart"
            | "lineChart"
            | "line3DChart"
            | "ofPieChart"
            | "pieChart"
            | "pie3DChart"
            | "radarChart"
            | "scatterChart"
            | "stockChart"
            | "surfaceChart"
            | "surface3DChart"
    )
}

fn is_chart_data_label_flag(name: &str) -> bool {
    matches!(
        name,
        "showLegendKey"
            | "showVal"
            | "showCatName"
            | "showSerName"
            | "showPercent"
            | "showBubbleSize"
            | "showLeaderLines"
    )
}

fn is_semantic_chart_label_setting(name: &str) -> bool {
    name.starts_with("show")
        || matches!(
            name,
            "dLblPos" | "numFmt" | "separator" | "leaderLines" | "delete"
        )
}

fn chart_label_element_supported(name: &str) -> bool {
    is_chart_data_label_flag(name)
        || matches!(
            name,
            "dLbls"
                | "dLbl"
                | "idx"
                | "dLblPos"
                | "numFmt"
                | "separator"
                | "leaderLines"
                | "delete"
                | "tx"
                | "rich"
                | "strRef"
                | "strCache"
                | "ptCount"
                | "pt"
                | "v"
                | "f"
                | "spPr"
                | "txPr"
                | "extLst"
                | "ext"
        )
}

fn chart_u32_attribute(event: &BytesStart<'_>, name: &str, context: &str) -> Result<u32, PocError> {
    attr_required(event, name)?
        .parse::<u32>()
        .map_err(|_| PocError::SemanticExtractionFailed(format!("invalid {context}")))
}

fn chart_boolean_attribute(
    event: &BytesStart<'_>,
    name: &str,
    context: &str,
) -> Result<bool, PocError> {
    let value = attr_required(event, name)?;
    parse_chart_boolean(&value, context)
}

fn parse_chart_boolean(value: &str, context: &str) -> Result<bool, PocError> {
    match value {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(PocError::SemanticExtractionFailed(format!(
            "invalid boolean chart setting {context}"
        ))),
    }
}

fn set_chart_label_setting(
    labels: &mut ActiveChartDataLabels,
    name: &str,
    value: Option<Value>,
) -> Result<(), PocError> {
    if !labels.seen_settings.insert(name.to_owned()) {
        return Err(PocError::SemanticExtractionFailed(format!(
            "duplicate chart data-label setting {name}"
        )));
    }
    if let Some(value) = value {
        labels.settings.insert(name.to_owned(), value);
    }
    Ok(())
}

fn parse_smartart_semantic(data: &[u8]) -> Result<Value, PocError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PocError::SemanticExtractionFailed("SmartArt XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    let mut ids = BTreeMap::<String, usize>::new();
    let mut current_point: Option<usize> = None;
    let mut points: Vec<Vec<String>> = Vec::new();
    let mut point_roles: Vec<Option<String>> = Vec::new();
    let mut raw_connections: Vec<(String, String, String, u32, u32)> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "pt" =>
            {
                let id = attr_required(&event, "modelId")?;
                let ordinal = points.len();
                if ids.insert(id, ordinal).is_some() {
                    return Err(PocError::SemanticExtractionFailed(
                        "duplicate SmartArt point modelId".into(),
                    ));
                }
                points.push(Vec::new());
                point_roles.push(attr(&event, "type")?);
                current_point = Some(ordinal);
            }
            Ok(Event::Start(event))
                if element_in_namespace(&reader, &event, DRAWINGML_MAIN_NS, "t") =>
            {
                let value = reader
                    .read_text(event.name())
                    .map_err(|error| {
                        PocError::SemanticExtractionFailed(format!("SmartArt text XML: {error}"))
                    })?
                    .to_string();
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
                let source_order = smartart_connection_order(&event, "srcOrd")?;
                let destination_order = smartart_connection_order(&event, "destOrd")?;
                raw_connections.push((source, target, kind, source_order, destination_order));
            }
            Ok(Event::End(event)) if event.local_name().as_ref() == "pt" => {
                current_point = None;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "SmartArt XML: {error}"
                )));
            }
        }
    }

    let mut normalized_connections = Vec::with_capacity(raw_connections.len());
    let mut source_orders = BTreeSet::new();
    let mut destination_orders = BTreeSet::new();
    for (source, target, kind, source_order, destination_order) in raw_connections {
        let source = ids.get(&source).copied().ok_or_else(|| {
            PocError::SemanticExtractionFailed("SmartArt connection source is unknown".into())
        })?;
        let target = ids.get(&target).copied().ok_or_else(|| {
            PocError::SemanticExtractionFailed("SmartArt connection target is unknown".into())
        })?;
        if !source_orders.insert((source, source_order)) {
            return Err(PocError::SemanticExtractionFailed(
                "duplicate SmartArt outgoing sibling order".into(),
            ));
        }
        if !destination_orders.insert((target, destination_order)) {
            return Err(PocError::SemanticExtractionFailed(
                "duplicate SmartArt incoming destination order".into(),
            ));
        }
        normalized_connections.push((source, source_order, target, destination_order, kind));
    }
    normalized_connections.sort();
    let connections = normalized_connections
        .into_iter()
        .map(|(source, source_order, target, destination_order, kind)| {
            json!({
                "source": source,
                "source_order": source_order,
                "target": target,
                "destination_order": destination_order,
                "kind": kind,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "points": points,
        "point_roles": point_roles,
        "connections": connections
    }))
}

fn smartart_connection_order(event: &BytesStart<'_>, attribute: &str) -> Result<u32, PocError> {
    let value = normalized_unqualified_attr_required(event, attribute)?;
    value.parse::<u32>().map_err(|_| {
        PocError::SemanticExtractionFailed(format!(
            "SmartArt connection {attribute} must be an unsigned integer"
        ))
    })
}

fn shape_projection(
    shape: &Shape,
    external_dependencies: &mut Vec<ExternalDependency>,
    visual_present: &mut bool,
    shape_path: &mut Vec<usize>,
    connector_relationships: &BTreeMap<Vec<usize>, Value>,
    picture_transforms: &BTreeMap<Vec<usize>, PptxPictureTransform>,
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
            let picture_transform = picture_transforms.get(shape_path).ok_or_else(|| {
                PocError::ParserDisagreement(
                    "raw PresentationML picture occurrence is missing".into(),
                )
            })?;
            let data = shape.data.as_ref().ok_or_else(|| {
                PocError::SemanticExtractionFailed(
                    "picture relationship did not resolve to bytes".into(),
                )
            })?;
            Ok(json!({
                "kind": "picture",
                "position": position_projection(shape.position.as_ref()),
                "alt_text": shape.alt_text,
                "format": shape.format,
                "image_semantic_sha256": image_semantic_digest(data)?,
                "transform": {
                    "crop_left": picture_transform.crop_left,
                    "crop_top": picture_transform.crop_top,
                    "crop_right": picture_transform.crop_right,
                    "crop_bottom": picture_transform.crop_bottom,
                    "rotation": picture_transform.rotation,
                    "flip_horizontal": picture_transform.flip_horizontal,
                    "flip_vertical": picture_transform.flip_vertical,
                },
            }))
        }
        Shape::Group(group) => {
            let mut children = Vec::with_capacity(group.children.len());
            for (index, child) in group.children.iter().enumerate() {
                shape_path.push(index);
                children.push(shape_projection(
                    child,
                    external_dependencies,
                    visual_present,
                    shape_path,
                    connector_relationships,
                    picture_transforms,
                )?);
                shape_path.pop();
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
        Shape::Connector(connector) => {
            let connections = connector_relationships.get(shape_path).ok_or_else(|| {
                PocError::ParserDisagreement(
                    "raw PresentationML connector occurrence is missing".into(),
                )
            })?;
            Ok(json!({
                "kind": "connector",
                "position": position_projection(connector.position.as_ref()),
                "connections": connections,
            }))
        }
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
                    .map_err(|error| {
                        PocError::SemanticExtractionFailed(format!("text XML: {error}"))
                    })?
                    .to_string();
                if !value.trim().is_empty() {
                    values.push(value);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "text XML: {error}"
                )));
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
        return Err(PocError::SemanticExtractionFailed(
            "PNG has no IEND chunk".into(),
        ));
    }
    Ok(hex::encode(Sha256::digest(&normalized)))
}

fn element_in_namespace<R>(
    reader: &NsReader<R>,
    event: &BytesStart<'_>,
    namespace_uri: &str,
    local_name: &str,
) -> bool {
    let (namespace, local) = reader.resolver().resolve_element(event.name());
    matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == namespace_uri)
        && local.as_ref() == local_name
}

fn namespaced_attr_required<R>(
    reader: &NsReader<R>,
    event: &BytesStart<'_>,
    namespace_uri: &str,
    local_name: &str,
) -> Result<String, PocError> {
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        let (namespace, local) = reader.resolver().resolve_attribute(item.key);
        if matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == namespace_uri)
            && local.as_ref() == local_name
        {
            return Ok(item.value.as_ref().to_owned());
        }
    }
    Err(PocError::SemanticExtractionFailed(format!(
        "missing required attribute {{{namespace_uri}}}{local_name}"
    )))
}

fn attr_required(event: &BytesStart<'_>, name: &str) -> Result<String, PocError> {
    attr(event, name)?.ok_or_else(|| {
        PocError::SemanticExtractionFailed(format!("missing required attribute {name}"))
    })
}

fn unqualified_attr_required(event: &BytesStart<'_>, name: &str) -> Result<String, PocError> {
    unqualified_attr(event, name)?.ok_or_else(|| {
        PocError::SemanticExtractionFailed(format!("missing required unqualified attribute {name}"))
    })
}

fn normalized_unqualified_attr_required(
    event: &BytesStart<'_>,
    name: &str,
) -> Result<String, PocError> {
    normalized_unqualified_attr(event, name)?.ok_or_else(|| {
        PocError::SemanticExtractionFailed(format!("missing required unqualified attribute {name}"))
    })
}

fn unqualified_attr(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, PocError> {
    let mut found = None;
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        let key = item.key.as_ref();
        if key == name {
            if found.is_some() {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "duplicate unqualified attribute {name}"
                )));
            }
            found = Some(item.value.as_ref().to_owned());
        } else if key.rsplit_once(':').is_some_and(|(_, local)| local == name) {
            return Err(PocError::SemanticExtractionFailed(format!(
                "attribute {name} must be unqualified"
            )));
        }
    }
    Ok(found)
}

fn normalized_unqualified_attr(
    event: &BytesStart<'_>,
    name: &str,
) -> Result<Option<String>, PocError> {
    let mut found = None;
    let mut attributes = event.attributes();
    attributes.with_checks(true);
    for item in attributes {
        let item = item.map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        let key = item.key.as_ref();
        if key == name {
            if found.is_some() {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "duplicate unqualified attribute {name}"
                )));
            }
            let value = item
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|error| {
                    PocError::SemanticExtractionFailed(format!(
                        "invalid XML attribute value: {error}"
                    ))
                })?;
            found = Some(value.into_owned());
        } else if key.rsplit_once(':').is_some_and(|(_, local)| local == name) {
            return Err(PocError::SemanticExtractionFailed(format!(
                "attribute {name} must be unqualified"
            )));
        }
    }
    Ok(found)
}

fn attr(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, PocError> {
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PocError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        let key = item.key.as_ref();
        let matches = key == name || key.rsplit_once(':').is_some_and(|(_, local)| local == name);
        if matches {
            return Ok(Some(item.value.as_ref().to_owned()));
        }
    }
    Ok(None)
}
