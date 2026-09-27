use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use document_semantic_inspection_core::{
    CommentEvidence, EditorialProvenance, ExternalDependency as CoreExternalDependency, FormatId,
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

use crate::{WorkerFailure, WorkerFailureCode};

use super::canonical_json_bytes;
use super::pptx_package::{
    PackageInspection, Relationship, inspect_package, is_pptx_package_checked,
};

const DRAWINGML_MAIN_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const SMARTART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
const PRESENTATION_MAIN_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const OFFICE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
const MAX_SLIDES: usize = 4_096;
const MAX_SHAPES: usize = 100_000;
const MAX_IMAGES: usize = 4_096;
const MAX_RESULT_BYTES: usize = 16 * 1024 * 1024;
const PARSER_LIBRARIES: [(&str, &str); 4] = [
    ("office_oxide", "0.1.11"),
    ("zip", "8.6.0"),
    ("quick-xml", "0.42.0"),
    ("png", "0.18.1"),
];

#[derive(Debug, Clone, Copy, Default)]
pub struct PptxAdapter;

#[derive(Debug)]
struct ExternalDependency {
    kind: String,
    definition: String,
}

struct ShapeClickContext<'a> {
    slide_index: usize,
    slide_part: &'a str,
    rels: &'a BTreeMap<String, Relationship>,
    slide_order_by_part: &'a BTreeMap<&'a str, usize>,
    external_dependencies: &'a mut Vec<ExternalDependency>,
}

type RawSmartArtConnection = (String, String, String, Option<u32>, Option<u32>);

#[derive(Debug)]
enum PptxError {
    SemanticExtractionFailed(String),
    UnsupportedSemanticConstruct(String),
    ParserDisagreement(String),
    InspectionResourceLimitExceeded,
}

impl From<PptxError> for WorkerFailure {
    fn from(error: PptxError) -> Self {
        match error {
            PptxError::SemanticExtractionFailed(message) => {
                failure(WorkerFailureCode::SemanticExtractionFailed, message)
            }
            PptxError::UnsupportedSemanticConstruct(message) => {
                failure(WorkerFailureCode::UnsupportedSemanticConstruct, message)
            }
            PptxError::ParserDisagreement(message) => {
                failure(WorkerFailureCode::ParserDisagreement, message)
            }
            PptxError::InspectionResourceLimitExceeded => resource_limit(),
        }
    }
}

impl super::SemanticAdapter for PptxAdapter {
    fn format(&self) -> FormatId {
        FormatId::Pptx
    }

    fn inspect(
        &self,
        input: &[u8],
        _profile: &super::AdapterProfile,
    ) -> Result<super::SemanticAdapterOutput, WorkerFailure> {
        if input.len() > MAX_INPUT_BYTES {
            return Err(resource_limit());
        }
        if !is_pptx_package_checked(input)? {
            return Err(failure(
                WorkerFailureCode::FormatMismatch,
                "input is not a qualified PPTX package",
            ));
        }

        let package = inspect_package(input)?;
        let ordered_slide_parts = ordered_slide_parts(&package)?;
        let slide_order_by_part = ordered_slide_parts
            .iter()
            .enumerate()
            .map(|(index, part)| (part.as_str(), index))
            .collect::<BTreeMap<_, _>>();
        if ordered_slide_parts.len() > MAX_SLIDES {
            return Err(resource_limit());
        }

        let document = PptxDocument::from_reader(Cursor::new(input)).map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("office_oxide PPTX: {error}"),
            )
        })?;

        let office_slide_parts = office_oxide_slide_parts(&package, &document)?;
        if ordered_slide_parts.len() != document.slides.len()
            || office_slide_parts.len() != document.slides.len()
        {
            return Err(failure(
                WorkerFailureCode::ParserDisagreement,
                format!(
                    "slide count mismatch: raw={}, office_oxide={}, office_oxide_paths={}",
                    ordered_slide_parts.len(),
                    document.slides.len(),
                    office_slide_parts.len()
                ),
            ));
        }

        validate_shape_budgets(&document)?;
        let office_slide_index_by_part = office_slide_parts
            .iter()
            .enumerate()
            .map(|(index, part)| (part.as_str(), index))
            .collect::<BTreeMap<_, _>>();
        if office_slide_index_by_part.len() != office_slide_parts.len() {
            return Err(failure(
                WorkerFailureCode::ParserDisagreement,
                "office_oxide mapped multiple slide entries to one part",
            ));
        }

        let mut slides = Vec::with_capacity(document.slides.len());
        let mut external_dependencies = Vec::new();
        let mut editorial = EditorialProvenance::default();
        let mut visual_present = false;
        let mut notes_present = false;
        for (index, slide_part) in ordered_slide_parts.iter().enumerate() {
            let raw_slide = package.parts.get(slide_part).ok_or_else(|| {
                failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("missing slide part {slide_part}"),
                )
            })?;
            let slide_index = office_slide_index_by_part
                .get(slide_part.as_str())
                .copied()
                .ok_or_else(|| {
                    failure(
                        WorkerFailureCode::ParserDisagreement,
                        format!(
                            "office_oxide has no slide mapped to raw relationship target {slide_part}"
                        ),
                    )
                })?;
            let slide = &document.slides[slide_index];

            let rels = package
                .relationships
                .get(slide_part)
                .cloned()
                .unwrap_or_default();

            let raw_notes = raw_notes_text(slide_part, &rels, &package)?;
            if raw_notes.as_deref() != slide.notes.as_deref() {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!(
                        "speaker-note mismatch on slide {index}: raw={raw_notes:?}, office_oxide={:?}",
                        slide.notes
                    ),
                ));
            }
            if slide.notes.is_some() {
                notes_present = true;
            }

            let raw_graphics = raw_graphic_semantics(slide_part, raw_slide, &rels, &package)?;
            if !raw_graphics.is_empty() {
                visual_present = true;
            }

            let shape_click_hyperlinks = raw_shape_click_hyperlinks(
                index,
                slide_part,
                raw_slide,
                &rels,
                &slide_order_by_part,
                &mut external_dependencies,
            )?;

            let connector_relationships = raw_connector_relationships(raw_slide)?;
            let parsed_connector_count = slide
                .shapes
                .iter()
                .map(shape_connector_count)
                .sum::<usize>();
            if connector_relationships.len() != parsed_connector_count {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!(
                        "connector count mismatch on slide {index}: raw={}, office_oxide={parsed_connector_count}",
                        connector_relationships.len()
                    ),
                ));
            }

            let (picture_transforms, raw_picture_digests) =
                raw_picture_transform_semantics(slide_part, raw_slide, &rels, &package)?;

            let mut shape_values = Vec::with_capacity(slide.shapes.len());
            for shape in &slide.shapes {
                shape_values.push(shape_projection(
                    shape,
                    &mut external_dependencies,
                    &mut visual_present,
                )?);
            }
            let office_picture_digests = projected_picture_digests(&shape_values)?;
            if raw_picture_digests != office_picture_digests {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!(
                        "picture mapping mismatch on slide {index}: raw={}, office_oxide={}",
                        raw_picture_digests.len(),
                        office_picture_digests.len()
                    ),
                ));
            }

            for (comment_index, comment) in slide.comments.iter().enumerate() {
                editorial.comments.push(CommentEvidence {
                    author_label: comment.author.clone(),
                    timestamp: None,
                    resolved_state: "unknown".into(),
                    source_locator: format!("slide[{index}]/comment[{comment_index}]"),
                    content: comment.text.clone(),
                });
            }

            let mut slide_projection = json!({
                "index": index,
                "name": slide.name,
                "hidden": slide.hidden,
                "shapes": shape_values,
                "connector_relationships": connector_relationships,
                "picture_transforms": picture_transforms,
                "raw_graphics": raw_graphics,
                "speaker_notes": slide.notes,
            });
            if !shape_click_hyperlinks.is_empty() {
                slide_projection["shape_click_hyperlinks"] = json!(shape_click_hyperlinks);
            }
            slides.push(slide_projection);
        }

        external_dependencies.sort_by(|left, right| {
            (left.kind.as_str(), left.definition.as_str())
                .cmp(&(right.kind.as_str(), right.definition.as_str()))
        });
        external_dependencies
            .dedup_by(|left, right| left.kind == right.kind && left.definition == right.definition);

        let semantic_projection =
            canonical_json_bytes(&json!({"slides": slides})).map_err(|error| {
                failure(
                    WorkerFailureCode::InvalidWorkerResult,
                    format!("PPTX projection serialization: {error}"),
                )
            })?;
        if semantic_projection.len() > MAX_RESULT_BYTES {
            return Err(resource_limit());
        }

        let dependencies = external_dependencies
            .into_iter()
            .map(|dependency| CoreExternalDependency {
                dependency_kind: dependency.kind,
                normalized_reference: dependency.definition,
                source_locator: "pptx/external-hyperlink".into(),
                version_significant: true,
            })
            .collect();
        let mut output = super::SemanticAdapterOutput::from_projection(
            &semantic_projection,
            &[
                "reader_content",
                "presentation_structure",
                "visual_content",
                "speaker_notes",
                "formula_logic",
                "vba_logic",
            ],
            "pptx",
            &PARSER_LIBRARIES,
        )
        .with_editorial_provenance(editorial)
        .with_external_dependencies(dependencies);
        if !visual_present {
            output = output.with_capability_state(
                "visual_content",
                document_semantic_inspection_core::CapabilityState::Absent,
            )?;
        }
        if !notes_present {
            output = output.with_capability_state(
                "speaker_notes",
                document_semantic_inspection_core::CapabilityState::Absent,
            )?;
        }
        output = output.with_capability_state(
            "formula_logic",
            document_semantic_inspection_core::CapabilityState::NotRepresentable,
        )?;
        output = output.with_capability_state(
            "vba_logic",
            document_semantic_inspection_core::CapabilityState::NotRepresentable,
        )?;
        Ok(output)
    }
}

fn failure(code: WorkerFailureCode, message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::new(code, message)
}

fn resource_limit() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::InspectionResourceLimitExceeded,
        "PPTX package exceeded a configured resource limit",
    )
}

fn resolve_target(source: &str, target: &str) -> Result<String, PptxError> {
    super::pptx_package::resolve_target(source, target).map_err(|error| match error.code() {
        WorkerFailureCode::UnsupportedSemanticConstruct => {
            PptxError::UnsupportedSemanticConstruct(error.message().to_owned())
        }
        WorkerFailureCode::InspectionResourceLimitExceeded => {
            PptxError::InspectionResourceLimitExceeded
        }
        WorkerFailureCode::ParserDisagreement => {
            PptxError::ParserDisagreement(error.message().to_owned())
        }
        _ => PptxError::SemanticExtractionFailed(error.message().to_owned()),
    })
}

fn validate_shape_budgets(document: &PptxDocument) -> Result<(), WorkerFailure> {
    let mut shape_count = 0usize;
    let mut image_count = 0usize;
    for slide in &document.slides {
        for shape in &slide.shapes {
            count_shape(shape, 0, &mut shape_count, &mut image_count)
                .map_err(WorkerFailure::from)?;
        }
    }
    Ok(())
}

fn count_shape(
    shape: &Shape,
    depth: usize,
    shape_count: &mut usize,
    image_count: &mut usize,
) -> Result<(), PptxError> {
    *shape_count = shape_count
        .checked_add(1)
        .ok_or(PptxError::InspectionResourceLimitExceeded)?;
    if *shape_count > MAX_SHAPES || depth > 256 {
        return Err(PptxError::InspectionResourceLimitExceeded);
    }
    match shape {
        Shape::Picture(_) => {
            *image_count = image_count
                .checked_add(1)
                .ok_or(PptxError::InspectionResourceLimitExceeded)?;
            if *image_count > MAX_IMAGES {
                return Err(PptxError::InspectionResourceLimitExceeded);
            }
        }
        Shape::Group(group) => {
            for child in &group.children {
                count_shape(child, depth + 1, shape_count, image_count)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn ordered_slide_parts(package: &PackageInspection) -> Result<Vec<String>, PptxError> {
    let presentation = package.parts.get("ppt/presentation.xml").ok_or_else(|| {
        PptxError::SemanticExtractionFailed("PPTX missing ppt/presentation.xml".into())
    })?;
    let rels = package
        .relationships
        .get("ppt/presentation.xml")
        .ok_or_else(|| {
            PptxError::SemanticExtractionFailed("presentation relationships missing".into())
        })?;

    let text = std::str::from_utf8(presentation)
        .map_err(|_| PptxError::SemanticExtractionFailed("presentation XML is not UTF-8".into()))?;
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
                    PptxError::SemanticExtractionFailed(format!("slide relationship {rid} missing"))
                })?;
                if !rel.kind.ends_with("/slide") || rel.external {
                    return Err(PptxError::UnsupportedSemanticConstruct(format!(
                        "presentation slide relationship {rid} is not an internal slide"
                    )));
                }
                result.push(resolve_target("ppt/presentation.xml", &rel.target)?);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PptxError::SemanticExtractionFailed(format!(
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
) -> Result<Vec<String>, PptxError> {
    let slide_ids = &document.presentation.slides;
    if slide_ids.len() != document.slides.len() {
        return Err(PptxError::ParserDisagreement(format!(
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
                PptxError::SemanticExtractionFailed("presentation relationships missing".into())
            })?;
        let mut parts = Vec::with_capacity(slide_ids.len());
        for slide in slide_ids {
            let rel = relationships.get(&slide.rel_id).ok_or_else(|| {
                PptxError::ParserDisagreement(format!(
                    "office_oxide slide relationship {} is missing from raw package",
                    slide.rel_id
                ))
            })?;
            if !rel.kind.ends_with("/slide") || rel.external {
                return Err(PptxError::UnsupportedSemanticConstruct(format!(
                    "office_oxide slide relationship {} is not an internal slide",
                    slide.rel_id
                )));
            }
            parts.push(resolve_target("ppt/presentation.xml", &rel.target)?);
        }
        if parts != raw_slide_parts {
            return Err(PptxError::ParserDisagreement(
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
            return Err(PptxError::UnsupportedSemanticConstruct(
                "office_oxide positional slide fallback is not safe for nonconventional slide part paths".into(),
            ));
        }
        fallback_parts
    } else {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "office_oxide mixed literal relationship IDs and positional slide fallback cannot be mapped safely".into(),
        ));
    };

    let unique = parts.iter().collect::<BTreeSet<_>>();
    if unique.len() != parts.len() {
        return Err(PptxError::ParserDisagreement(
            "office_oxide mapped multiple slides to one package part".into(),
        ));
    }
    Ok(parts)
}

fn raw_notes_text(
    slide_part: &str,
    rels: &BTreeMap<String, Relationship>,
    package: &PackageInspection,
) -> Result<Option<String>, PptxError> {
    let Some(rel) = rels.values().find(|rel| rel.kind.ends_with("/notesSlide")) else {
        return Ok(None);
    };
    if rel.external {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "external notes slide relationship".into(),
        ));
    }
    let target = resolve_target(slide_part, &rel.target)?;
    let data = package.parts.get(&target).ok_or_else(|| {
        PptxError::SemanticExtractionFailed(format!("notes target {target} missing"))
    })?;
    let texts = xml_text_values(data, "t")?;
    let joined = texts.join("\n");
    Ok((!joined.is_empty()).then_some(joined))
}

fn raw_shape_click_hyperlinks(
    slide_index: usize,
    slide_part: &str,
    slide_data: &[u8],
    rels: &BTreeMap<String, Relationship>,
    slide_order_by_part: &BTreeMap<&str, usize>,
    external_dependencies: &mut Vec<ExternalDependency>,
) -> Result<Vec<Value>, PptxError> {
    let text = std::str::from_utf8(slide_data)
        .map_err(|_| PptxError::SemanticExtractionFailed("slide XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut in_shape_tree = false;
    let mut next_shape_ordinal = 0usize;
    let mut shape_stack = Vec::<(usize, &'static str)>::new();
    let mut current_non_visual_shape = None::<usize>;
    let mut linked_shapes = BTreeSet::new();
    let mut hyperlinks = Vec::new();
    let mut hyperlink_context = ShapeClickContext {
        slide_index,
        slide_part,
        rels,
        slide_order_by_part,
        external_dependencies,
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                if element_in_namespace(&reader, &event, PRESENTATION_MAIN_NS, "spTree") {
                    if in_shape_tree {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "nested slide shape tree".into(),
                        ));
                    }
                    in_shape_tree = true;
                    continue;
                }
                if !in_shape_tree {
                    continue;
                }

                if let Some((element_name, _)) = presentation_shape_kind(&reader, &event) {
                    if next_shape_ordinal >= MAX_SHAPES {
                        return Err(PptxError::InspectionResourceLimitExceeded);
                    }
                    let ordinal = next_shape_ordinal;
                    next_shape_ordinal += 1;
                    shape_stack.push((ordinal, element_name));
                    continue;
                }

                if element_in_namespace(&reader, &event, PRESENTATION_MAIN_NS, "cNvPr") {
                    let Some((ordinal, _)) = shape_stack.last().copied() else {
                        continue;
                    };
                    if current_non_visual_shape.replace(ordinal).is_some() {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "nested non-visual shape properties".into(),
                        ));
                    }
                    continue;
                }

                if element_in_namespace(&reader, &event, DRAWINGML_MAIN_NS, "hlinkClick")
                    && let Some(shape_ordinal) = current_non_visual_shape
                {
                    if !linked_shapes.insert(shape_ordinal) {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "multiple shape-level click hyperlinks on one shape".into(),
                        ));
                    }
                    hyperlinks.push(project_shape_click_hyperlink(
                        &reader,
                        &event,
                        shape_ordinal,
                        &mut hyperlink_context,
                    )?);
                }
            }
            Ok(Event::Empty(event)) => {
                if in_shape_tree && presentation_shape_kind(&reader, &event).is_some() {
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "empty PresentationML shape cannot be mapped safely".into(),
                    ));
                }
                if in_shape_tree
                    && element_in_namespace(&reader, &event, PRESENTATION_MAIN_NS, "cNvPr")
                {
                    continue;
                }
                if element_in_namespace(&reader, &event, DRAWINGML_MAIN_NS, "hlinkClick")
                    && let Some(shape_ordinal) = current_non_visual_shape
                {
                    if !linked_shapes.insert(shape_ordinal) {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "multiple shape-level click hyperlinks on one shape".into(),
                        ));
                    }
                    hyperlinks.push(project_shape_click_hyperlink(
                        &reader,
                        &event,
                        shape_ordinal,
                        &mut hyperlink_context,
                    )?);
                }
            }
            Ok(Event::End(event)) => {
                if in_shape_tree
                    && element_end_in_namespace(&reader, &event, PRESENTATION_MAIN_NS, "cNvPr")
                {
                    current_non_visual_shape = None;
                } else if in_shape_tree
                    && let Some(element_name) = presentation_shape_end_name(&reader, &event)
                {
                    let Some((_, expected_name)) = shape_stack.pop() else {
                        return Err(PptxError::SemanticExtractionFailed(
                            "slide shape stack ended unexpectedly".into(),
                        ));
                    };
                    if expected_name != element_name {
                        return Err(PptxError::SemanticExtractionFailed(
                            "slide shape nesting is inconsistent".into(),
                        ));
                    }
                } else if element_end_in_namespace(&reader, &event, PRESENTATION_MAIN_NS, "spTree")
                {
                    if !shape_stack.is_empty() || current_non_visual_shape.is_some() {
                        return Err(PptxError::SemanticExtractionFailed(
                            "slide shape tree ended with unclosed shapes".into(),
                        ));
                    }
                    in_shape_tree = false;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PptxError::SemanticExtractionFailed(format!(
                    "slide shape hyperlink XML: {error}"
                )));
            }
        }
    }

    if in_shape_tree || !shape_stack.is_empty() || current_non_visual_shape.is_some() {
        return Err(PptxError::SemanticExtractionFailed(
            "slide shape hyperlink XML ended with an incomplete shape tree".into(),
        ));
    }
    Ok(hyperlinks)
}

fn project_shape_click_hyperlink<R: std::io::BufRead>(
    reader: &NsReader<R>,
    event: &BytesStart<'_>,
    shape_ordinal: usize,
    context: &mut ShapeClickContext<'_>,
) -> Result<Value, PptxError> {
    if attr(event, "action")?.is_some() {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "shape-level click actions are outside the qualified relationship subset".into(),
        ));
    }
    let relationship_id = namespaced_attr_required(reader, event, OFFICE_RELATIONSHIPS_NS, "id")?;
    let relationship = context.rels.get(&relationship_id).ok_or_else(|| {
        PptxError::SemanticExtractionFailed(format!(
            "shape hyperlink relationship {relationship_id} is missing"
        ))
    })?;
    if !relationship.kind.ends_with("/hyperlink") {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "shape click target does not use a hyperlink relationship".into(),
        ));
    }

    let target = if relationship.external {
        context.external_dependencies.push(ExternalDependency {
            kind: "hyperlink".into(),
            definition: relationship.target.clone(),
        });
        json!({
            "kind": "external",
            "target": relationship.target,
        })
    } else {
        let target_part = resolve_target(context.slide_part, &relationship.target)?;
        let target_slide_order = context
            .slide_order_by_part
            .get(target_part.as_str())
            .copied()
            .ok_or_else(|| {
                PptxError::UnsupportedSemanticConstruct(
                    "internal shape click target is not a slide in this presentation".into(),
                )
            })?;
        json!({
            "kind": "internal_slide",
            "target_slide_order": target_slide_order,
        })
    };
    Ok(json!({
        "source_slide_order": context.slide_index,
        "shape_order": shape_ordinal,
        "target": target,
        "target_frame": normalized_unqualified_attr(event, "tgtFrame")?,
        "tooltip": normalized_unqualified_attr(event, "tooltip")?,
    }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RawPptxShapeKind {
    Other,
    Connector,
}

struct RawConnectorEndpoint {
    target_id: u32,
    connection_site: u32,
}

struct RawPptxShape {
    kind: RawPptxShapeKind,
    ordinal: usize,
    element_name: &'static str,
    non_visual_id: Option<String>,
    saw_non_visual_properties: bool,
    start: Option<RawConnectorEndpoint>,
    end: Option<RawConnectorEndpoint>,
}

fn raw_connector_relationships(slide_data: &[u8]) -> Result<Vec<Value>, PptxError> {
    let text = std::str::from_utf8(slide_data)
        .map_err(|_| PptxError::SemanticExtractionFailed("slide XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut in_shape_tree = false;
    let mut next_shape_ordinal = 0usize;
    let mut shape_stack = Vec::<usize>::new();
    let mut shapes = Vec::<RawPptxShape>::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                if element_in_namespace(&reader, &event, PRESENTATION_MAIN_NS, "spTree") {
                    if in_shape_tree {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "nested slide shape tree".into(),
                        ));
                    }
                    in_shape_tree = true;
                    continue;
                }

                if !in_shape_tree {
                    continue;
                }

                if let Some((element_name, kind)) = presentation_shape_kind(&reader, &event) {
                    let ordinal = next_shape_ordinal;
                    next_shape_ordinal = next_shape_ordinal
                        .checked_add(1)
                        .ok_or(PptxError::InspectionResourceLimitExceeded)?;
                    shapes.push(RawPptxShape {
                        kind,
                        ordinal,
                        element_name,
                        non_visual_id: None,
                        saw_non_visual_properties: false,
                        start: None,
                        end: None,
                    });
                    shape_stack.push(shapes.len() - 1);
                    continue;
                }

                if element_in_namespace(&reader, &event, PRESENTATION_MAIN_NS, "cNvPr") {
                    capture_non_visual_id(&event, &mut shapes, &shape_stack)?;
                    continue;
                }

                if element_in_namespace(&reader, &event, DRAWINGML_MAIN_NS, "stCxn") {
                    capture_connector_endpoint(&event, true, &mut shapes, &shape_stack)?;
                    continue;
                }

                if element_in_namespace(&reader, &event, DRAWINGML_MAIN_NS, "endCxn") {
                    capture_connector_endpoint(&event, false, &mut shapes, &shape_stack)?;
                }
            }
            Ok(Event::Empty(event)) => {
                if in_shape_tree && presentation_shape_kind(&reader, &event).is_some() {
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "empty PresentationML shape cannot be mapped safely".into(),
                    ));
                }

                if !in_shape_tree {
                    continue;
                }

                if element_in_namespace(&reader, &event, PRESENTATION_MAIN_NS, "cNvPr") {
                    capture_non_visual_id(&event, &mut shapes, &shape_stack)?;
                    continue;
                }

                if element_in_namespace(&reader, &event, DRAWINGML_MAIN_NS, "stCxn") {
                    capture_connector_endpoint(&event, true, &mut shapes, &shape_stack)?;
                    continue;
                }

                if element_in_namespace(&reader, &event, DRAWINGML_MAIN_NS, "endCxn") {
                    capture_connector_endpoint(&event, false, &mut shapes, &shape_stack)?;
                }
            }
            Ok(Event::End(event)) => {
                if in_shape_tree
                    && let Some(element_name) = presentation_shape_end_name(&reader, &event)
                {
                    let Some(shape_index) = shape_stack.pop() else {
                        return Err(PptxError::SemanticExtractionFailed(
                            "slide shape stack ended unexpectedly".into(),
                        ));
                    };
                    if shapes[shape_index].element_name != element_name {
                        return Err(PptxError::SemanticExtractionFailed(
                            "slide shape nesting is inconsistent".into(),
                        ));
                    }
                } else if element_end_in_namespace(&reader, &event, PRESENTATION_MAIN_NS, "spTree")
                {
                    if !shape_stack.is_empty() {
                        return Err(PptxError::SemanticExtractionFailed(
                            "slide shape tree ended with unclosed shapes".into(),
                        ));
                    }
                    in_shape_tree = false;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PptxError::SemanticExtractionFailed(format!(
                    "slide connector XML: {error}"
                )));
            }
        }
    }

    let connectors = shapes
        .iter()
        .enumerate()
        .filter(|(_, shape)| shape.kind == RawPptxShapeKind::Connector)
        .collect::<Vec<_>>();
    if connectors.is_empty() {
        return Ok(Vec::new());
    }

    let mut shape_ordinal_by_id = BTreeMap::<u32, usize>::new();
    for shape in &shapes {
        if !shape.saw_non_visual_properties {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "shape with connector references has no non-visual properties".into(),
            ));
        }
        let raw_id = shape.non_visual_id.as_deref().ok_or_else(|| {
            PptxError::UnsupportedSemanticConstruct(
                "shape with connector references has no internal id".into(),
            )
        })?;
        let id = raw_id.parse::<u32>().map_err(|_| {
            PptxError::UnsupportedSemanticConstruct(
                "shape connector mapping has a malformed internal id".into(),
            )
        })?;
        if shape_ordinal_by_id.insert(id, shape.ordinal).is_some() {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "duplicate PresentationML shape id makes connector mapping ambiguous".into(),
            ));
        }
    }

    let mut relationships = Vec::with_capacity(connectors.len());
    for (_, connector) in connectors {
        let project_endpoint = |endpoint: &Option<RawConnectorEndpoint>| {
            endpoint
                .as_ref()
                .map(|endpoint| {
                    let target_ordinal = shape_ordinal_by_id
                        .get(&endpoint.target_id)
                        .copied()
                        .ok_or_else(|| {
                            PptxError::UnsupportedSemanticConstruct(
                                "connector endpoint refers to an unknown shape id".into(),
                            )
                        })?;
                    Ok(json!({
                        "target_shape_ordinal": target_ordinal,
                        "connection_site": endpoint.connection_site,
                    }))
                })
                .transpose()
                .map(|endpoint| endpoint.unwrap_or(Value::Null))
        };

        relationships.push(json!({
            "connector_shape_ordinal": connector.ordinal,
            "start": project_endpoint(&connector.start)?,
            "end": project_endpoint(&connector.end)?,
        }));
    }
    Ok(relationships)
}

fn presentation_shape_kind<R>(
    reader: &NsReader<R>,
    event: &BytesStart<'_>,
) -> Option<(&'static str, RawPptxShapeKind)> {
    let (namespace, local_name) = reader.resolver().resolve_element(event.name());
    if !matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == PRESENTATION_MAIN_NS)
    {
        return None;
    }
    match local_name.as_ref() {
        "sp" => Some(("sp", RawPptxShapeKind::Other)),
        "pic" => Some(("pic", RawPptxShapeKind::Other)),
        "graphicFrame" => Some(("graphicFrame", RawPptxShapeKind::Other)),
        "cxnSp" => Some(("cxnSp", RawPptxShapeKind::Connector)),
        "grpSp" => Some(("grpSp", RawPptxShapeKind::Other)),
        _ => None,
    }
}

fn presentation_shape_end_name<R>(
    reader: &NsReader<R>,
    event: &quick_xml::events::BytesEnd<'_>,
) -> Option<&'static str> {
    let (namespace, local_name) = reader.resolver().resolve_element(event.name());
    if !matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == PRESENTATION_MAIN_NS)
    {
        return None;
    }
    match local_name.as_ref() {
        "sp" => Some("sp"),
        "pic" => Some("pic"),
        "graphicFrame" => Some("graphicFrame"),
        "cxnSp" => Some("cxnSp"),
        "grpSp" => Some("grpSp"),
        _ => None,
    }
}

fn element_end_in_namespace<R>(
    reader: &NsReader<R>,
    event: &quick_xml::events::BytesEnd<'_>,
    namespace_uri: &str,
    local_name: &str,
) -> bool {
    let (namespace, local) = reader.resolver().resolve_element(event.name());
    matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == namespace_uri)
        && local.as_ref() == local_name
}

fn capture_non_visual_id(
    event: &BytesStart<'_>,
    shapes: &mut [RawPptxShape],
    shape_stack: &[usize],
) -> Result<(), PptxError> {
    let Some(shape_index) = shape_stack.last().copied() else {
        // `p:spTree/p:nvGrpSpPr/p:cNvPr` identifies the required shape-tree
        // root and is not an attachable presentation shape.
        return Ok(());
    };
    let shape = &mut shapes[shape_index];
    if shape.saw_non_visual_properties {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "shape has multiple non-visual property records".into(),
        ));
    }
    shape.saw_non_visual_properties = true;
    shape.non_visual_id = unqualified_attr(event, "id")?;
    Ok(())
}

fn capture_connector_endpoint(
    event: &BytesStart<'_>,
    is_start: bool,
    shapes: &mut [RawPptxShape],
    shape_stack: &[usize],
) -> Result<(), PptxError> {
    let Some(shape_index) = shape_stack.last().copied() else {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "connector endpoint has no enclosing shape".into(),
        ));
    };
    let shape = &mut shapes[shape_index];
    if shape.kind != RawPptxShapeKind::Connector {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "connector endpoint is outside a connector shape".into(),
        ));
    }
    let slot = if is_start {
        &mut shape.start
    } else {
        &mut shape.end
    };
    if slot.is_some() {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "connector has duplicate endpoint records".into(),
        ));
    }
    let target_id = unqualified_attr(event, "id")?
        .ok_or_else(|| {
            PptxError::UnsupportedSemanticConstruct(
                "connector endpoint is missing its shape id".into(),
            )
        })?
        .parse::<u32>()
        .map_err(|_| {
            PptxError::UnsupportedSemanticConstruct(
                "connector endpoint has a malformed shape id".into(),
            )
        })?;
    let connection_site = unqualified_attr(event, "idx")?
        .ok_or_else(|| {
            PptxError::UnsupportedSemanticConstruct(
                "connector endpoint is missing its connection site".into(),
            )
        })?
        .parse::<u32>()
        .map_err(|_| {
            PptxError::UnsupportedSemanticConstruct(
                "connector endpoint has a malformed connection site".into(),
            )
        })?;
    *slot = Some(RawConnectorEndpoint {
        target_id,
        connection_site,
    });
    Ok(())
}

fn unqualified_attr(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, PptxError> {
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PptxError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        if item.key.as_ref() == name {
            return Ok(Some(item.value.as_ref().to_owned()));
        }
    }
    Ok(None)
}

fn normalized_unqualified_attr(
    event: &BytesStart<'_>,
    name: &str,
) -> Result<Option<String>, PptxError> {
    let mut found = None;
    let mut attributes = event.attributes();
    attributes.with_checks(true);
    for item in attributes {
        let item = item.map_err(|error| {
            PptxError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        let key = item.key.as_ref();
        if key == name {
            if found.is_some() {
                return Err(PptxError::SemanticExtractionFailed(format!(
                    "duplicate unqualified attribute {name}"
                )));
            }
            let value = item
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|error| {
                    PptxError::SemanticExtractionFailed(format!(
                        "invalid XML attribute value: {error}"
                    ))
                })?;
            found = Some(value.into_owned());
        } else if key.rsplit_once(':').is_some_and(|(_, local)| local == name) {
            return Err(PptxError::SemanticExtractionFailed(format!(
                "attribute {name} must be unqualified"
            )));
        }
    }
    Ok(found)
}

fn shape_connector_count(shape: &Shape) -> usize {
    match shape {
        Shape::Connector(_) => 1,
        Shape::Group(group) => group.children.iter().map(shape_connector_count).sum(),
        _ => 0,
    }
}

struct ActivePictureTransform {
    relationship_digest: Option<String>,
    saw_blip_fill: bool,
    saw_blip: bool,
    saw_source_rect: bool,
    saw_shape_properties: bool,
    saw_transform: bool,
    rotation: i64,
    flip_horizontal: bool,
    flip_vertical: bool,
    crop_left: u32,
    crop_top: u32,
    crop_right: u32,
    crop_bottom: u32,
}

impl ActivePictureTransform {
    fn new() -> Self {
        Self {
            relationship_digest: None,
            saw_blip_fill: false,
            saw_blip: false,
            saw_source_rect: false,
            saw_shape_properties: false,
            saw_transform: false,
            rotation: 0,
            flip_horizontal: false,
            flip_vertical: false,
            crop_left: 0,
            crop_top: 0,
            crop_right: 0,
            crop_bottom: 0,
        }
    }

    fn finish(self) -> Result<(Value, String), PptxError> {
        if !self.saw_blip_fill
            || !self.saw_blip
            || !self.saw_shape_properties
            || !self.saw_transform
        {
            return Err(PptxError::SemanticExtractionFailed(
                "picture is missing required fill or transform structure".into(),
            ));
        }
        let crop_horizontal = self
            .crop_left
            .checked_add(self.crop_right)
            .ok_or(PptxError::InspectionResourceLimitExceeded)?;
        let crop_vertical = self
            .crop_top
            .checked_add(self.crop_bottom)
            .ok_or(PptxError::InspectionResourceLimitExceeded)?;
        if crop_horizontal >= 100_000 || crop_vertical >= 100_000 {
            return Err(PptxError::SemanticExtractionFailed(
                "picture source crop removes the complete image".into(),
            ));
        }
        let digest = self.relationship_digest.ok_or_else(|| {
            PptxError::SemanticExtractionFailed(
                "picture image relationship did not resolve to bytes".into(),
            )
        })?;
        let projection = json!({
            "rotation": self.rotation,
            "flip_horizontal": self.flip_horizontal,
            "flip_vertical": self.flip_vertical,
            "crop": {
                "left": self.crop_left,
                "top": self.crop_top,
                "right": self.crop_right,
                "bottom": self.crop_bottom,
            },
        });
        Ok((projection, digest))
    }
}

fn raw_picture_transform_semantics(
    slide_part: &str,
    slide_data: &[u8],
    rels: &BTreeMap<String, Relationship>,
    package: &PackageInspection,
) -> Result<(Vec<Value>, Vec<String>), PptxError> {
    type XmlName = (String, String);

    let text = std::str::from_utf8(slide_data)
        .map_err(|_| PptxError::SemanticExtractionFailed("slide XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut stack = Vec::<XmlName>::new();
    let mut in_shape_tree = false;
    let mut saw_shape_tree = false;
    let mut active_picture = None::<ActivePictureTransform>;
    let mut transforms = Vec::new();
    let mut image_digests = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let name = expanded_element_name(&reader, &event)?;
                if name == (PRESENTATION_MAIN_NS.into(), "spTree".into()) {
                    if in_shape_tree || saw_shape_tree {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "slide has multiple or nested shape trees".into(),
                        ));
                    }
                    in_shape_tree = true;
                    saw_shape_tree = true;
                } else if in_shape_tree && name == (PRESENTATION_MAIN_NS.into(), "pic".into()) {
                    if active_picture.is_some() {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "nested picture shape cannot be mapped safely".into(),
                        ));
                    }
                    if !stack_ends_with_shape_parent(&stack) {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "picture is outside a supported slide shape position".into(),
                        ));
                    }
                    active_picture = Some(ActivePictureTransform::new());
                }

                if let Some(picture) = active_picture.as_mut() {
                    inspect_picture_start(
                        &reader, &event, &name, &stack, picture, slide_part, rels, package,
                    )?;
                }
                stack.push(name);
            }
            Ok(Event::Empty(event)) => {
                let name = expanded_element_name(&reader, &event)?;
                if name == (PRESENTATION_MAIN_NS.into(), "pic".into()) && in_shape_tree {
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "empty picture shape cannot be mapped safely".into(),
                    ));
                }
                if let Some(picture) = active_picture.as_mut() {
                    inspect_picture_start(
                        &reader, &event, &name, &stack, picture, slide_part, rels, package,
                    )?;
                }
            }
            Ok(Event::End(event)) => {
                let name = expanded_end_name(&reader, &event)?;
                if name == (PRESENTATION_MAIN_NS.into(), "pic".into()) {
                    let picture = active_picture.take().ok_or_else(|| {
                        PptxError::UnsupportedSemanticConstruct(
                            "picture end has no matching picture start".into(),
                        )
                    })?;
                    let (projection, digest) = picture.finish()?;
                    transforms.push(projection);
                    image_digests.push(digest);
                }
                if name == (PRESENTATION_MAIN_NS.into(), "spTree".into()) {
                    if !in_shape_tree {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "shape tree end has no matching start".into(),
                        ));
                    }
                    in_shape_tree = false;
                }
                let popped = stack.pop().ok_or_else(|| {
                    PptxError::SemanticExtractionFailed("slide XML element stack underflow".into())
                })?;
                if popped != name {
                    return Err(PptxError::SemanticExtractionFailed(
                        "slide XML element stack is inconsistent".into(),
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PptxError::SemanticExtractionFailed(format!(
                    "picture transform XML: {error}"
                )));
            }
        }
    }

    if !saw_shape_tree || in_shape_tree || active_picture.is_some() || !stack.is_empty() {
        return Err(PptxError::SemanticExtractionFailed(
            "slide has incomplete picture or shape-tree structure".into(),
        ));
    }
    Ok((transforms, image_digests))
}

fn stack_ends_with_shape_parent(stack: &[(String, String)]) -> bool {
    stack.last().is_some_and(|(namespace, local)| {
        namespace == PRESENTATION_MAIN_NS && matches!(local.as_str(), "spTree" | "grpSp")
    })
}

fn expanded_element_name<R>(
    reader: &NsReader<R>,
    event: &BytesStart<'_>,
) -> Result<(String, String), PptxError> {
    let (namespace, local) = reader.resolver().resolve_element(event.name());
    let namespace = match namespace {
        ResolveResult::Bound(namespace) => namespace.as_ref().to_owned(),
        ResolveResult::Unbound => String::new(),
        ResolveResult::Unknown(prefix) => {
            return Err(PptxError::UnsupportedSemanticConstruct(format!(
                "unbound picture XML namespace prefix {}",
                String::from_utf8_lossy(prefix.as_ref())
            )));
        }
    };
    Ok((namespace, local.as_ref().to_owned()))
}

fn expanded_end_name<R>(
    reader: &NsReader<R>,
    event: &quick_xml::events::BytesEnd<'_>,
) -> Result<(String, String), PptxError> {
    let (namespace, local) = reader.resolver().resolve_element(event.name());
    let namespace = match namespace {
        ResolveResult::Bound(namespace) => namespace.as_ref().to_owned(),
        ResolveResult::Unbound => String::new(),
        ResolveResult::Unknown(prefix) => {
            return Err(PptxError::UnsupportedSemanticConstruct(format!(
                "unbound picture XML namespace prefix {}",
                String::from_utf8_lossy(prefix.as_ref())
            )));
        }
    };
    Ok((namespace, local.as_ref().to_owned()))
}

#[allow(clippy::too_many_arguments)]
fn inspect_picture_start<R>(
    reader: &NsReader<R>,
    event: &BytesStart<'_>,
    name: &(String, String),
    stack: &[(String, String)],
    picture: &mut ActivePictureTransform,
    slide_part: &str,
    rels: &BTreeMap<String, Relationship>,
    package: &PackageInspection,
) -> Result<(), PptxError> {
    let (namespace, local) = name;
    if !matches!(namespace.as_str(), PRESENTATION_MAIN_NS | DRAWINGML_MAIN_NS) {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "foreign namespace content inside a picture is unsupported".into(),
        ));
    }
    validate_picture_element_position(name, stack)?;

    let picture_fill_parent = stack_ends_with(
        stack,
        &[
            (PRESENTATION_MAIN_NS, "pic"),
            (PRESENTATION_MAIN_NS, "blipFill"),
        ],
    );
    let picture_properties_parent = stack_ends_with(
        stack,
        &[
            (PRESENTATION_MAIN_NS, "pic"),
            (PRESENTATION_MAIN_NS, "spPr"),
        ],
    );

    if namespace == PRESENTATION_MAIN_NS && local == "blipFill" {
        if !stack_ends_with(stack, &[(PRESENTATION_MAIN_NS, "pic")])
            || std::mem::replace(&mut picture.saw_blip_fill, true)
        {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "picture has duplicate or misplaced blipFill".into(),
            ));
        }
        return Ok(());
    }
    if namespace == PRESENTATION_MAIN_NS && local == "spPr" {
        if !stack_ends_with(stack, &[(PRESENTATION_MAIN_NS, "pic")])
            || std::mem::replace(&mut picture.saw_shape_properties, true)
        {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "picture has duplicate or misplaced shape properties".into(),
            ));
        }
        return Ok(());
    }
    if namespace == DRAWINGML_MAIN_NS && local == "blip" && picture_fill_parent {
        if std::mem::replace(&mut picture.saw_blip, true) {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "picture has multiple image references".into(),
            ));
        }
        let embedded = namespaced_attr_unique(reader, event, OFFICE_RELATIONSHIPS_NS, "embed")?;
        if namespaced_attr_unique(reader, event, OFFICE_RELATIONSHIPS_NS, "link")?.is_some() {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "externally linked PPTX pictures are unsupported".into(),
            ));
        }
        let relationship_id = embedded.ok_or_else(|| {
            PptxError::UnsupportedSemanticConstruct(
                "picture has no embedded image relationship".into(),
            )
        })?;
        let relationship = rels.get(&relationship_id).ok_or_else(|| {
            PptxError::SemanticExtractionFailed(format!(
                "picture relationship {relationship_id} is missing"
            ))
        })?;
        if relationship.external || !relationship.kind.ends_with("/image") {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "picture relationship is not an internal image relationship".into(),
            ));
        }
        let target = resolve_target(slide_part, &relationship.target)?;
        let bytes = package.parts.get(&target).ok_or_else(|| {
            PptxError::SemanticExtractionFailed(format!("picture target {target} is missing"))
        })?;
        picture.relationship_digest = Some(image_semantic_digest(bytes)?);
        return Ok(());
    }
    if namespace == DRAWINGML_MAIN_NS
        && local == "stretch"
        && stack_ends_with(
            stack,
            &[
                (PRESENTATION_MAIN_NS, "pic"),
                (PRESENTATION_MAIN_NS, "blipFill"),
            ],
        )
    {
        return Ok(());
    }
    if namespace == DRAWINGML_MAIN_NS && local == "srcRect" && picture_fill_parent {
        if std::mem::replace(&mut picture.saw_source_rect, true) {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "picture has duplicate source crop rectangles".into(),
            ));
        }
        let (left, top, right, bottom) = picture_crop_attributes(event)?;
        picture.crop_left = left;
        picture.crop_top = top;
        picture.crop_right = right;
        picture.crop_bottom = bottom;
        return Ok(());
    }
    if namespace == DRAWINGML_MAIN_NS && local == "xfrm" && picture_properties_parent {
        if std::mem::replace(&mut picture.saw_transform, true) {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "picture has duplicate shape transforms".into(),
            ));
        }
        let (rotation, flip_horizontal, flip_vertical) = picture_transform_attributes(event)?;
        picture.rotation = rotation.rem_euclid(21_600_000);
        picture.flip_horizontal = flip_horizontal;
        picture.flip_vertical = flip_vertical;
    }
    Ok(())
}

fn validate_picture_element_position(
    name: &(String, String),
    stack: &[(String, String)],
) -> Result<(), PptxError> {
    let (namespace, local) = name;
    let presentation = PRESENTATION_MAIN_NS;
    let drawing = DRAWINGML_MAIN_NS;
    let supported = if stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "blipFill"),
            (drawing, "blip"),
        ],
    ) || stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "blipFill"),
            (drawing, "srcRect"),
        ],
    ) || stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "nvPicPr"),
            (presentation, "cNvPr"),
        ],
    ) || stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "nvPicPr"),
            (presentation, "cNvPicPr"),
        ],
    ) || stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "nvPicPr"),
            (presentation, "nvPr"),
        ],
    ) || stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "spPr"),
            (drawing, "xfrm"),
            (drawing, "off"),
        ],
    ) || stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "spPr"),
            (drawing, "xfrm"),
            (drawing, "ext"),
        ],
    ) || stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "spPr"),
            (drawing, "prstGeom"),
            (drawing, "avLst"),
        ],
    ) || stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "blipFill"),
            (drawing, "stretch"),
            (drawing, "fillRect"),
        ],
    ) {
        false
    } else if stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "blipFill"),
            (drawing, "stretch"),
        ],
    ) {
        namespace == drawing && local == "fillRect"
    } else if stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "spPr"),
            (drawing, "xfrm"),
        ],
    ) {
        namespace == drawing && matches!(local.as_str(), "off" | "ext")
    } else if stack_ends_with(
        stack,
        &[
            (presentation, "pic"),
            (presentation, "spPr"),
            (drawing, "prstGeom"),
        ],
    ) {
        namespace == drawing && local == "avLst"
    } else if stack_ends_with(stack, &[(presentation, "pic")]) {
        namespace == presentation && matches!(local.as_str(), "nvPicPr" | "blipFill" | "spPr")
    } else if stack_ends_with(stack, &[(presentation, "nvPicPr")]) {
        namespace == presentation && matches!(local.as_str(), "cNvPr" | "cNvPicPr" | "nvPr")
    } else if stack_ends_with(stack, &[(presentation, "blipFill")]) {
        namespace == drawing && matches!(local.as_str(), "blip" | "srcRect" | "stretch")
    } else if stack_ends_with(stack, &[(presentation, "spPr")]) {
        namespace == drawing && matches!(local.as_str(), "xfrm" | "prstGeom")
    } else if stack_ends_with(stack, &[(drawing, "prstGeom")]) {
        namespace == drawing && local == "avLst"
    } else {
        stack_ends_with_shape_parent(stack) && namespace == presentation && local == "pic"
    };

    if supported {
        Ok(())
    } else {
        Err(PptxError::UnsupportedSemanticConstruct(format!(
            "unsupported or misplaced picture element {{{namespace}}}{local}"
        )))
    }
}

fn stack_ends_with(stack: &[(String, String)], suffix: &[(&str, &str)]) -> bool {
    stack.len() >= suffix.len()
        && stack[stack.len() - suffix.len()..].iter().zip(suffix).all(
            |((namespace, local), (expected_namespace, expected_local))| {
                namespace == expected_namespace && local == expected_local
            },
        )
}

fn namespaced_attr_unique<R>(
    reader: &NsReader<R>,
    event: &BytesStart<'_>,
    namespace_uri: &str,
    local_name: &str,
) -> Result<Option<String>, PptxError> {
    let mut value = None;
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PptxError::SemanticExtractionFailed(format!("invalid picture XML attribute: {error}"))
        })?;
        let (namespace, local) = reader.resolver().resolve_attribute(item.key);
        if matches!(namespace, ResolveResult::Bound(ref uri) if uri.as_ref() == namespace_uri)
            && local.as_ref() == local_name
            && value.replace(item.value.as_ref().to_owned()).is_some()
        {
            return Err(PptxError::SemanticExtractionFailed(format!(
                "duplicate picture relationship attribute {local_name}"
            )));
        }
    }
    Ok(value)
}

fn picture_transform_attributes(event: &BytesStart<'_>) -> Result<(i64, bool, bool), PptxError> {
    let mut rotation = 0i64;
    let mut flip_horizontal = false;
    let mut flip_vertical = false;
    let mut seen = BTreeSet::new();
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PptxError::SemanticExtractionFailed(format!(
                "invalid picture transform attribute: {error}"
            ))
        })?;
        let key = item.key.as_ref();
        if key.starts_with("xmlns") {
            continue;
        }
        let value = item.value.as_ref();
        if !seen.insert(key.to_owned()) {
            return Err(PptxError::SemanticExtractionFailed(
                "duplicate picture transform attribute".into(),
            ));
        }
        match key {
            "rot" => {
                let value = value.parse::<i32>().map_err(|_| {
                    PptxError::SemanticExtractionFailed(
                        "picture rotation is not a signed integer angle".into(),
                    )
                })?;
                rotation = i64::from(value);
            }
            "flipH" => flip_horizontal = parse_drawingml_boolean(value, "flipH")?,
            "flipV" => flip_vertical = parse_drawingml_boolean(value, "flipV")?,
            _ => {
                return Err(PptxError::UnsupportedSemanticConstruct(format!(
                    "unsupported picture transform attribute {}",
                    key
                )));
            }
        }
    }
    Ok((rotation, flip_horizontal, flip_vertical))
}

fn picture_crop_attributes(event: &BytesStart<'_>) -> Result<(u32, u32, u32, u32), PptxError> {
    let mut crop = [0u32; 4];
    let mut seen = BTreeSet::new();
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PptxError::SemanticExtractionFailed(format!("invalid picture crop attribute: {error}"))
        })?;
        let key = item.key.as_ref();
        if key.starts_with("xmlns") {
            continue;
        }
        if !seen.insert(key.to_owned()) {
            return Err(PptxError::SemanticExtractionFailed(
                "duplicate picture crop attribute".into(),
            ));
        }
        let slot = match key {
            "l" => 0,
            "t" => 1,
            "r" => 2,
            "b" => 3,
            _ => {
                return Err(PptxError::UnsupportedSemanticConstruct(format!(
                    "unsupported picture crop attribute {}",
                    key
                )));
            }
        };
        let value = item
            .value
            .as_ref()
            .parse::<u32>()
            .ok()
            .filter(|value| *value <= 100_000)
            .ok_or_else(|| {
                PptxError::SemanticExtractionFailed(
                    "picture crop is outside the normalized percentage range".into(),
                )
            })?;
        crop[slot] = value;
    }
    Ok((crop[0], crop[1], crop[2], crop[3]))
}

fn parse_drawingml_boolean(value: &str, name: &str) -> Result<bool, PptxError> {
    match value {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(PptxError::SemanticExtractionFailed(format!(
            "picture {name} must be a DrawingML boolean"
        ))),
    }
}

fn projected_picture_digests(shapes: &[Value]) -> Result<Vec<String>, PptxError> {
    let mut digests = Vec::new();
    for shape in shapes {
        match shape.get("kind").and_then(Value::as_str) {
            Some("picture") => {
                let digest = shape
                    .get("image_semantic_sha256")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        PptxError::ParserDisagreement(
                            "office_oxide picture has no semantic image digest".into(),
                        )
                    })?;
                digests.push(digest.to_owned());
            }
            Some("group") => {
                let children =
                    shape
                        .get("children")
                        .and_then(Value::as_array)
                        .ok_or_else(|| {
                            PptxError::ParserDisagreement(
                                "office_oxide group has no child shape list".into(),
                            )
                        })?;
                digests.extend(projected_picture_digests(children)?);
            }
            _ => {}
        }
    }
    Ok(digests)
}

fn raw_graphic_semantics(
    slide_part: &str,
    slide_data: &[u8],
    rels: &BTreeMap<String, Relationship>,
    package: &PackageInspection,
) -> Result<Vec<Value>, PptxError> {
    let text = std::str::from_utf8(slide_data)
        .map_err(|_| PptxError::SemanticExtractionFailed("slide XML is not UTF-8".into()))?;
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
                    PptxError::SemanticExtractionFailed(format!("chart relationship {rid} missing"))
                })?;
                let target = resolve_target(slide_part, &rel.target)?;
                let data = package.parts.get(&target).ok_or_else(|| {
                    PptxError::SemanticExtractionFailed(format!("chart target {target} missing"))
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
                    PptxError::SemanticExtractionFailed(format!(
                        "SmartArt relationship {rid} missing"
                    ))
                })?;
                let target = resolve_target(slide_part, &rel.target)?;
                let data = package.parts.get(&target).ok_or_else(|| {
                    PptxError::SemanticExtractionFailed(format!("SmartArt target {target} missing"))
                })?;
                let mut semantic = parse_smartart_semantic(data)?;
                if let Some(layout_relationship_id) = attr(&event, "lo")? {
                    let layout_relationship =
                        rels.get(&layout_relationship_id).ok_or_else(|| {
                            PptxError::SemanticExtractionFailed(format!(
                                "SmartArt layout relationship {layout_relationship_id} is missing"
                            ))
                        })?;
                    if layout_relationship.external
                        || !layout_relationship.kind.ends_with("/diagramLayout")
                    {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "SmartArt layout reference is not an internal diagram-layout relationship".into(),
                        ));
                    }
                    let layout_target = resolve_target(slide_part, &layout_relationship.target)?;
                    let layout_data = package.parts.get(&layout_target).ok_or_else(|| {
                        PptxError::SemanticExtractionFailed(format!(
                            "SmartArt layout target {layout_target} is missing"
                        ))
                    })?;
                    semantic["layout"] = parse_smartart_layout_semantic(layout_data)?;
                }
                result.push(json!({
                    "frame_ordinal": frame_index,
                    "kind": "smartart",
                    "semantic": semantic,
                }));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PptxError::SemanticExtractionFailed(format!(
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
    name_formula: Option<String>,
    category_formula: Option<String>,
    value_formula: Option<String>,
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
            name_formula: None,
            category_formula: None,
            value_formula: None,
            section: None,
            current_point: None,
            saw_categories: false,
            saw_values: false,
        }
    }
}

fn set_chart_series_formula(series: &mut ChartSeries, formula: String) -> Result<(), PptxError> {
    if formula.trim().is_empty() {
        return Err(PptxError::SemanticExtractionFailed(
            "chart source formula is empty".into(),
        ));
    }
    let slot = match series.section {
        Some(ChartSeriesSection::Name) => &mut series.name_formula,
        Some(ChartSeriesSection::Categories) => &mut series.category_formula,
        Some(ChartSeriesSection::Values) => &mut series.value_formula,
        None => {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "chart source formula is outside a series data section".into(),
            ));
        }
    };
    if slot.replace(formula).is_some() {
        return Err(PptxError::SemanticExtractionFailed(
            "duplicate chart source formula in a series data section".into(),
        ));
    }
    Ok(())
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
    formula: Option<String>,
}

struct ActiveChartLegend {
    position: String,
    overlay: bool,
    visible: bool,
    seen_settings: BTreeSet<String>,
    open_leaf: Option<String>,
}

impl ActiveChartLegend {
    fn new() -> Self {
        Self {
            position: "r".into(),
            overlay: false,
            visible: true,
            seen_settings: BTreeSet::new(),
            open_leaf: None,
        }
    }

    fn projection(self) -> Option<Value> {
        self.visible.then(|| {
            json!({
                "position": self.position,
                "overlay": self.overlay,
            })
        })
    }
}

fn parse_chart_semantic(data: &[u8]) -> Result<Value, PptxError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PptxError::SemanticExtractionFailed("chart XML is not UTF-8".into()))?;
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
                if is_chart
                    && is_unmodeled_reader_visible_chart_construct(&local, labels_stack.is_empty())
                {
                    return Err(PptxError::UnsupportedSemanticConstruct(format!(
                        "unsupported reader-visible chart construct {local}"
                    )));
                }
                if !is_chart && !is_drawing {
                    return Err(PptxError::UnsupportedSemanticConstruct(format!(
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
                if !root_seen {
                    if !is_chart || local != "chartSpace" {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "chart part root is not ChartML chartSpace".into(),
                        ));
                    }
                    root_seen = true;
                }
                if extension_depth.is_some_and(|depth| element_stack.len() >= depth) {
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "PPTX chart extension payload is not understood".into(),
                    ));
                }
                if is_chart && !labels_stack.is_empty() && !chart_label_element_supported(&local) {
                    return Err(PptxError::UnsupportedSemanticConstruct(format!(
                        "unsupported chart data-label element {local}"
                    )));
                }
                if is_chart && local == "ext" {
                    if element_stack.last().and_then(Option::as_deref) != Some("extLst") {
                        return Err(PptxError::UnsupportedSemanticConstruct(
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
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "non-ChartML content inside chart legend is unsupported".into(),
                        ));
                    }
                    if !matches!(local.as_str(), "legendPos" | "overlay") {
                        return Err(PptxError::UnsupportedSemanticConstruct(format!(
                            "unsupported chart legend element {local}"
                        )));
                    }
                    let legend = chart_legend.as_mut().ok_or_else(|| {
                        PptxError::SemanticExtractionFailed(
                            "chart legend settings have no legend scope".into(),
                        )
                    })?;
                    if legend.open_leaf.is_some()
                        || parent_chart_path.last().map(String::as_str) != Some("legend")
                    {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "nested chart legend setting is unsupported".into(),
                        ));
                    }
                    set_chart_legend_setting(legend, &local, &event)?;
                    legend.open_leaf = Some(local.clone());
                }
                element_stack.push(chart_local);

                if is_chart && local == "legend" {
                    if parent_chart_path.last().map(String::as_str) != Some("chart") {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "chart legend is outside the chart element".into(),
                        ));
                    }
                    if std::mem::replace(&mut saw_chart_legend, true) {
                        return Err(PptxError::SemanticExtractionFailed(
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
                        return Err(PptxError::SemanticExtractionFailed(
                            "nested chart series".into(),
                        ));
                    }
                    let group = chart_groups.last().ok_or_else(|| {
                        PptxError::UnsupportedSemanticConstruct(
                            "chart series is outside a chart type group".into(),
                        )
                    })?;
                    current_series = Some(ChartSeries::new(group));
                } else if is_chart && local == "title" {
                    if active_title.is_some() {
                        return Err(PptxError::SemanticExtractionFailed(
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
                        formula: None,
                    });
                } else if is_chart && local == "dLbls" {
                    if labels_stack
                        .last()
                        .is_some_and(|labels| labels.kind == ChartLabelKind::Group)
                    {
                        return Err(PptxError::SemanticExtractionFailed(
                            "nested chart data-label groups".into(),
                        ));
                    }
                    let group = chart_groups.last().ok_or_else(|| {
                        PptxError::UnsupportedSemanticConstruct(
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
                        PptxError::UnsupportedSemanticConstruct(
                            "individual chart data label is outside dLbls".into(),
                        )
                    })?;
                    if parent.kind != ChartLabelKind::Group {
                        return Err(PptxError::SemanticExtractionFailed(
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
                            return Err(PptxError::SemanticExtractionFailed(
                                "duplicate chart category cache".into(),
                            ));
                        }
                        series.saw_categories = true;
                        series.section = Some(ChartSeriesSection::Categories);
                    }
                } else if is_chart && local == "val" && labels_stack.is_empty() {
                    if let Some(series) = current_series.as_mut() {
                        if series.saw_values {
                            return Err(PptxError::SemanticExtractionFailed(
                                "duplicate chart value cache".into(),
                            ));
                        }
                        series.saw_values = true;
                        series.section = Some(ChartSeriesSection::Values);
                    }
                } else if is_chart && local == "f" && labels_stack.is_empty() {
                    let formula = reader
                        .read_text(event.name())
                        .map_err(|error| {
                            PptxError::SemanticExtractionFailed(format!(
                                "chart formula XML: {error}"
                            ))
                        })?
                        .trim()
                        .to_owned();
                    let supported_formula_parent = matches!(
                        element_stack.iter().rev().nth(1).and_then(Option::as_deref),
                        Some("strRef" | "numRef" | "multiLvlStrRef")
                    );
                    element_stack.pop();
                    if formula.is_empty() {
                        return Err(PptxError::SemanticExtractionFailed(
                            "chart source formula is empty".into(),
                        ));
                    }
                    if !supported_formula_parent {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "chart formula is outside a supported reference".into(),
                        ));
                    }
                    if let Some(title) = active_title.as_mut() {
                        if title.formula.replace(formula).is_some() {
                            return Err(PptxError::SemanticExtractionFailed(
                                "chart title has duplicate source formulas".into(),
                            ));
                        }
                    } else if let Some(series) = current_series.as_mut() {
                        set_chart_series_formula(series, formula)?;
                    } else {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "chart formula has no title or series scope".into(),
                        ));
                    }
                } else if is_chart && local == "pt" && labels_stack.is_empty() {
                    if let Some(series) = current_series.as_mut()
                        && let Some(section) = series.section
                    {
                        if series.current_point.is_some() {
                            return Err(PptxError::SemanticExtractionFailed(
                                "nested chart cache points".into(),
                            ));
                        }
                        let index = chart_u32_attribute(&event, "idx", "chart point index")?;
                        series.current_point = Some((section, index, false));
                    }
                } else if is_chart && local == "idx" {
                    let index = chart_u32_attribute(&event, "val", "chart series index")?;
                    if let Some(labels) = labels_stack.last_mut() {
                        if labels.kind != ChartLabelKind::Point
                            || labels.point_index.replace(index).is_some()
                        {
                            return Err(PptxError::SemanticExtractionFailed(
                                "duplicate or unscoped chart data-label point index".into(),
                            ));
                        }
                    } else if let Some(series) = current_series.as_mut()
                        && series.index.replace(index).is_some()
                    {
                        return Err(PptxError::SemanticExtractionFailed(
                            "duplicate chart series index".into(),
                        ));
                    }
                } else if is_chart && local == "order" {
                    let order = chart_u32_attribute(&event, "val", "chart series order")?;
                    let series = current_series.as_mut().ok_or_else(|| {
                        PptxError::SemanticExtractionFailed(
                            "chart series order is outside a series".into(),
                        )
                    })?;
                    if series.order.replace(order).is_some() {
                        return Err(PptxError::SemanticExtractionFailed(
                            "duplicate chart series order".into(),
                        ));
                    }
                } else if is_chart && is_chart_data_label_flag(&local) {
                    let enabled = chart_boolean_attribute(&event, "val", &local)?;
                    if labels_stack.is_empty() {
                        return Err(PptxError::UnsupportedSemanticConstruct(format!(
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
                        PptxError::UnsupportedSemanticConstruct(
                            "chart data-label position is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "position", Some(json!(value)))?;
                } else if is_chart && local == "delete" {
                    let deleted = chart_boolean_attribute(&event, "val", "delete")?;
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PptxError::UnsupportedSemanticConstruct(
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
                        PptxError::UnsupportedSemanticConstruct(
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
                        PptxError::UnsupportedSemanticConstruct(
                            "chart leader lines are outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "leader_lines", Some(json!(true)))?;
                } else if is_chart && local == "separator" {
                    let value = reader
                        .read_text(event.name())
                        .map_err(|error| {
                            PptxError::SemanticExtractionFailed(format!(
                                "chart label separator XML: {error}"
                            ))
                        })?
                        .to_string();
                    element_stack.pop();
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PptxError::UnsupportedSemanticConstruct(
                            "chart label separator is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "separator", Some(json!(value)))?;
                } else if (is_chart && local == "v") || (is_drawing && local == "t") {
                    let value = reader
                        .read_text(event.name())
                        .map_err(|error| {
                            PptxError::SemanticExtractionFailed(format!("chart text XML: {error}"))
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
                                    return Err(PptxError::SemanticExtractionFailed(
                                        "duplicate chart series name cache point".into(),
                                    ));
                                }
                                if series.indexed_names.insert(*index, value).is_some() {
                                    return Err(PptxError::SemanticExtractionFailed(
                                        "duplicate chart series name cache index".into(),
                                    ));
                                }
                                *seen = true;
                            }
                            (Some(ChartSeriesSection::Categories), Some((_, index, seen))) => {
                                if *seen || series.categories.insert(*index, value).is_some() {
                                    return Err(PptxError::SemanticExtractionFailed(
                                        "duplicate chart category point".into(),
                                    ));
                                }
                                *seen = true;
                            }
                            (Some(ChartSeriesSection::Values), Some((_, index, seen))) => {
                                if *seen || series.values.insert(*index, value).is_some() {
                                    return Err(PptxError::SemanticExtractionFailed(
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
                                return Err(PptxError::SemanticExtractionFailed(
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
                if is_chart
                    && is_unmodeled_reader_visible_chart_construct(&local, labels_stack.is_empty())
                {
                    return Err(PptxError::UnsupportedSemanticConstruct(format!(
                        "unsupported reader-visible chart construct {local}"
                    )));
                }
                if !is_chart && !is_drawing {
                    return Err(PptxError::UnsupportedSemanticConstruct(format!(
                        "chart XML empty element {local} has an unsupported namespace"
                    )));
                }
                if !root_seen && (!is_chart || local != "chartSpace") {
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "chart part root is not ChartML chartSpace".into(),
                    ));
                }
                if extension_depth.is_some_and(|depth| element_stack.len() >= depth) {
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "PPTX chart extension payload is not understood".into(),
                    ));
                }
                let inside_chart_legend = element_stack
                    .iter()
                    .any(|name| name.as_deref() == Some("legend"));
                if inside_chart_legend {
                    if !is_chart {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "non-ChartML content inside chart legend is unsupported".into(),
                        ));
                    }
                    if !matches!(local.as_str(), "legendPos" | "overlay") {
                        return Err(PptxError::UnsupportedSemanticConstruct(format!(
                            "unsupported chart legend element {local}"
                        )));
                    }
                    if element_stack.last().and_then(Option::as_deref) != Some("legend") {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "nested chart legend setting is unsupported".into(),
                        ));
                    }
                    let legend = chart_legend.as_mut().ok_or_else(|| {
                        PptxError::SemanticExtractionFailed(
                            "chart legend settings have no legend scope".into(),
                        )
                    })?;
                    set_chart_legend_setting(legend, &local, &event)?;
                    continue;
                }
                if is_chart && local == "legend" {
                    if element_stack.last().and_then(Option::as_deref) != Some("chart") {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "chart legend is outside the chart element".into(),
                        ));
                    }
                    if std::mem::replace(&mut saw_chart_legend, true) {
                        return Err(PptxError::SemanticExtractionFailed(
                            "duplicate chart legend".into(),
                        ));
                    }
                    chart_legend = Some(ActiveChartLegend::new());
                    continue;
                }
                if is_chart && is_chart_type(&local) {
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "empty chart type group is unsupported".into(),
                    ));
                }
                if is_chart && matches!(local.as_str(), "ser" | "pt" | "title") {
                    return Err(PptxError::SemanticExtractionFailed(format!(
                        "empty chart {local} element is malformed"
                    )));
                }
                if is_chart && local == "idx" {
                    let index = chart_u32_attribute(&event, "val", "chart index")?;
                    if let Some(labels) = labels_stack.last_mut() {
                        if labels.kind != ChartLabelKind::Point
                            || labels.point_index.replace(index).is_some()
                        {
                            return Err(PptxError::SemanticExtractionFailed(
                                "duplicate or unscoped chart data-label point index".into(),
                            ));
                        }
                    } else if let Some(series) = current_series.as_mut()
                        && series.index.replace(index).is_some()
                    {
                        return Err(PptxError::SemanticExtractionFailed(
                            "duplicate chart series index".into(),
                        ));
                    }
                } else if is_chart && local == "order" {
                    let order = chart_u32_attribute(&event, "val", "chart series order")?;
                    let series = current_series.as_mut().ok_or_else(|| {
                        PptxError::SemanticExtractionFailed(
                            "chart series order is outside a series".into(),
                        )
                    })?;
                    if series.order.replace(order).is_some() {
                        return Err(PptxError::SemanticExtractionFailed(
                            "duplicate chart series order".into(),
                        ));
                    }
                } else if is_chart && is_chart_data_label_flag(&local) {
                    let enabled = chart_boolean_attribute(&event, "val", &local)?;
                    if labels_stack.is_empty() {
                        return Err(PptxError::UnsupportedSemanticConstruct(format!(
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
                        PptxError::UnsupportedSemanticConstruct(
                            "chart data-label position is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "position", Some(json!(value)))?;
                } else if is_chart && local == "delete" {
                    let deleted = chart_boolean_attribute(&event, "val", "delete")?;
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PptxError::UnsupportedSemanticConstruct(
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
                        PptxError::UnsupportedSemanticConstruct(
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
                        PptxError::UnsupportedSemanticConstruct(
                            "chart leader lines are outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "leader_lines", Some(json!(true)))?;
                } else if is_chart && local == "separator" {
                    let labels = labels_stack.last_mut().ok_or_else(|| {
                        PptxError::UnsupportedSemanticConstruct(
                            "chart label separator is outside dLbls".into(),
                        )
                    })?;
                    set_chart_label_setting(labels, "separator", Some(json!("")))?;
                } else if is_chart && local == "ext" {
                    if element_stack.last().and_then(Option::as_deref) != Some("extLst") {
                        return Err(PptxError::UnsupportedSemanticConstruct(
                            "PPTX chart extension is outside extLst".into(),
                        ));
                    }
                } else if is_chart && is_semantic_chart_label_setting(&local) {
                    return Err(PptxError::UnsupportedSemanticConstruct(format!(
                        "unsupported chart data-label setting {local}"
                    )));
                } else if is_chart && local == "f" {
                    return Err(PptxError::SemanticExtractionFailed(
                        "chart source formula is empty".into(),
                    ));
                }
            }
            Ok(Event::End(_event)) => {
                let current_depth = element_stack.len();
                let local = element_stack.pop().ok_or_else(|| {
                    PptxError::SemanticExtractionFailed("chart XML has an unmatched end tag".into())
                })?;
                if extension_depth == Some(current_depth) {
                    if local.as_deref() != Some("ext") {
                        return Err(PptxError::SemanticExtractionFailed(
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
                        return Err(PptxError::SemanticExtractionFailed(
                            "chart legend ended inside a setting element".into(),
                        ));
                    }
                }
                match local.as_str() {
                    "title" => {
                        if let Some(title) = active_title.take()
                            && !title.text.is_empty()
                        {
                            titles.push(json!({
                                "scope": title.scope,
                                "text": title.text,
                                "source_formula": title.formula,
                            }));
                        }
                    }
                    "tx" | "cat" | "val" => {
                        if labels_stack.is_empty()
                            && let Some(series) = current_series.as_mut()
                        {
                            series.section = None;
                        }
                    }
                    "pt" => {
                        if labels_stack.is_empty()
                            && let Some(series) = current_series.as_mut()
                            && let Some((_, _, saw_value)) = series.current_point.take()
                            && !saw_value
                        {
                            return Err(PptxError::SemanticExtractionFailed(
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
                            PptxError::SemanticExtractionFailed(format!(
                                "chart {local} closing tag has no matching scope"
                            ))
                        })?;
                        if labels.kind != expected {
                            return Err(PptxError::SemanticExtractionFailed(format!(
                                "chart {local} scopes are improperly nested"
                            )));
                        }
                        if !labels.settings.is_empty() {
                            data_labels.push(labels);
                        }
                    }
                    "ser" => {
                        let current = current_series.take().ok_or_else(|| {
                            PptxError::SemanticExtractionFailed(
                                "chart series closing tag has no matching series".into(),
                            )
                        })?;
                        if current.current_point.is_some() {
                            return Err(PptxError::SemanticExtractionFailed(
                                "chart series ended inside a cache point".into(),
                            ));
                        }
                        let index = current.index.ok_or_else(|| {
                            PptxError::UnsupportedSemanticConstruct(
                                "chart series has no stable idx".into(),
                            )
                        })?;
                        let order = current.order.ok_or_else(|| {
                            PptxError::UnsupportedSemanticConstruct(
                                "chart series has no visible order".into(),
                            )
                        })?;
                        if !seen_series_indices.insert((current.group_ordinal, index)) {
                            return Err(PptxError::SemanticExtractionFailed(
                                "duplicate chart series idx in one chart group".into(),
                            ));
                        }
                        if !seen_series_orders.insert((current.group_ordinal, order)) {
                            return Err(PptxError::SemanticExtractionFailed(
                                "duplicate chart series order in one chart group".into(),
                            ));
                        }
                        series.push(current);
                    }
                    name if is_chart_type(name) => {
                        let group = chart_groups.pop().ok_or_else(|| {
                            PptxError::SemanticExtractionFailed(
                                "chart type closing tag has no matching group".into(),
                            )
                        })?;
                        if group.name != name {
                            return Err(PptxError::SemanticExtractionFailed(
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
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "chart legend text content is unsupported".into(),
                    ));
                }
                if extension_depth.is_some() && !event.as_ref().trim().is_empty() {
                    return Err(PptxError::UnsupportedSemanticConstruct(
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
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "chart legend CDATA is unsupported".into(),
                    ));
                }
                if extension_depth.is_some() && !event.as_ref().trim().is_empty() {
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "PPTX chart extension text is not understood".into(),
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PptxError::SemanticExtractionFailed(format!(
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
        return Err(PptxError::SemanticExtractionFailed(
            "chart XML ended with an incomplete semantic structure".into(),
        ));
    }

    series.sort_by_key(|series| (series.group_ordinal, series.order.unwrap_or(u32::MAX)));
    let series = series
        .into_iter()
        .map(|mut series| {
            let index = series.index.ok_or_else(|| {
                PptxError::SemanticExtractionFailed("chart series index disappeared".into())
            })?;
            let order = series.order.ok_or_else(|| {
                PptxError::SemanticExtractionFailed("chart series order disappeared".into())
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
            let mut source_formulas = json!({});
            if let Some(formula) = series.name_formula.take() {
                source_formulas["name"] = json!(formula);
            }
            if let Some(formula) = series.category_formula.take() {
                source_formulas["categories"] = json!(formula);
            }
            if let Some(formula) = series.value_formula.take() {
                source_formulas["values"] = json!(formula);
            }
            let mut projection = json!({
                "chart_group": series.group_ordinal,
                "chart_type": series.group_name,
                "idx": index,
                "order": order,
                "name": name,
                "points": points,
            });
            if !source_formulas
                .as_object()
                .is_some_and(|formulas| formulas.is_empty())
            {
                projection["source_formulas"] = source_formulas;
            }
            Ok(projection)
        })
        .collect::<Result<Vec<_>, PptxError>>()?;

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
    if let Some(legend) = chart_legend.and_then(ActiveChartLegend::projection) {
        projection["legend"] = legend;
    }
    Ok(projection)
}

fn set_chart_legend_setting(
    legend: &mut ActiveChartLegend,
    name: &str,
    event: &BytesStart<'_>,
) -> Result<(), PptxError> {
    if !legend.seen_settings.insert(name.to_owned()) {
        return Err(PptxError::SemanticExtractionFailed(format!(
            "duplicate chart legend setting {name}"
        )));
    }
    match name {
        "legendPos" => {
            let position = chart_legend_value_attribute(event, name)?;
            if !matches!(position.as_str(), "b" | "l" | "r" | "t" | "tr") {
                return Err(PptxError::SemanticExtractionFailed(
                    "invalid chart legend position".into(),
                ));
            }
            legend.position = position;
        }
        "overlay" => {
            let value = chart_legend_value_attribute(event, name)?;
            legend.overlay = parse_chart_boolean(&value, "legend overlay")?;
        }
        _ => {
            return Err(PptxError::UnsupportedSemanticConstruct(format!(
                "unsupported chart legend setting {name}"
            )));
        }
    }
    Ok(())
}

fn chart_legend_value_attribute(
    event: &BytesStart<'_>,
    element_name: &str,
) -> Result<String, PptxError> {
    let mut value = None;
    let mut invalid = None;
    let mut attributes = event.attributes();
    for item in attributes.with_checks(false) {
        match item {
            Err(error) => {
                invalid.get_or_insert_with(|| {
                    PptxError::SemanticExtractionFailed(format!(
                        "invalid {element_name} attribute: {error}"
                    ))
                });
            }
            Ok(attribute) => {
                let key = attribute.key.as_ref();
                if key == "val" {
                    if value.replace(attribute.value.as_ref().to_owned()).is_some() {
                        invalid.get_or_insert_with(|| {
                            PptxError::SemanticExtractionFailed(format!(
                                "duplicate unqualified {element_name} val attributes"
                            ))
                        });
                    }
                    continue;
                }

                let qualified_name = key.to_owned();
                let error = if key.rsplit(':').next() == Some("val") {
                    PptxError::UnsupportedSemanticConstruct(format!(
                        "{element_name} val must be an unqualified ChartML attribute"
                    ))
                } else {
                    PptxError::UnsupportedSemanticConstruct(format!(
                        "unsupported chart legend attribute {qualified_name} on {element_name}"
                    ))
                };
                invalid.get_or_insert(error);
            }
        }
    }
    if let Some(error) = invalid {
        return Err(error);
    }
    value.ok_or_else(|| {
        PptxError::SemanticExtractionFailed(format!(
            "missing unqualified {element_name} val attribute"
        ))
    })
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

fn is_unmodeled_reader_visible_chart_construct(name: &str, outside_data_labels: bool) -> bool {
    matches!(
        name,
        "catAx"
            | "dateAx"
            | "serAx"
            | "valAx"
            | "dTable"
            | "trendline"
            | "errBars"
            | "plotVisOnly"
            | "dispBlanksAs"
    ) || (outside_data_labels && name == "numFmt")
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

fn chart_u32_attribute(
    event: &BytesStart<'_>,
    name: &str,
    context: &str,
) -> Result<u32, PptxError> {
    attr_required(event, name)?
        .parse::<u32>()
        .map_err(|_| PptxError::SemanticExtractionFailed(format!("invalid {context}")))
}

fn chart_boolean_attribute(
    event: &BytesStart<'_>,
    name: &str,
    context: &str,
) -> Result<bool, PptxError> {
    let value = attr_required(event, name)?;
    parse_chart_boolean(&value, context)
}

fn parse_chart_boolean(value: &str, context: &str) -> Result<bool, PptxError> {
    match value {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(PptxError::SemanticExtractionFailed(format!(
            "invalid boolean chart setting {context}"
        ))),
    }
}

fn set_chart_label_setting(
    labels: &mut ActiveChartDataLabels,
    name: &str,
    value: Option<Value>,
) -> Result<(), PptxError> {
    if !labels.seen_settings.insert(name.to_owned()) {
        return Err(PptxError::SemanticExtractionFailed(format!(
            "duplicate chart data-label setting {name}"
        )));
    }
    if let Some(value) = value {
        labels.settings.insert(name.to_owned(), value);
    }
    Ok(())
}

fn parse_smartart_semantic(data: &[u8]) -> Result<Value, PptxError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PptxError::SemanticExtractionFailed("SmartArt XML is not UTF-8".into()))?;
    let mut reader = NsReader::from_str(text);
    let mut ids = BTreeMap::<String, usize>::new();
    let mut current_point: Option<usize> = None;
    let mut points: Vec<(String, Vec<String>)> = Vec::new();
    let mut raw_connections = Vec::<RawSmartArtConnection>::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "pt" =>
            {
                let id = attr_required(&event, "modelId")?;
                let ordinal = points.len();
                if ids.insert(id, ordinal).is_some() {
                    return Err(PptxError::SemanticExtractionFailed(
                        "duplicate SmartArt point modelId".into(),
                    ));
                }
                let role = attr(&event, "type")?.unwrap_or_else(|| "node".into());
                if !matches!(
                    role.as_str(),
                    "node" | "asst" | "doc" | "pres" | "parTrans" | "sibTrans"
                ) {
                    return Err(PptxError::UnsupportedSemanticConstruct(format!(
                        "unsupported SmartArt point role {role}"
                    )));
                }
                points.push((role, Vec::new()));
                current_point = Some(ordinal);
            }
            Ok(Event::Start(event))
                if element_in_namespace(&reader, &event, DRAWINGML_MAIN_NS, "t") =>
            {
                let value = reader
                    .read_text(event.name())
                    .map_err(|error| {
                        PptxError::SemanticExtractionFailed(format!("SmartArt text XML: {error}"))
                    })?
                    .to_string();
                if let Some(point) = current_point
                    && !value.trim().is_empty()
                {
                    points[point].1.push(value);
                }
            }
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "cxn" =>
            {
                let source = attr_required(&event, "srcId")?;
                let target = attr_required(&event, "destId")?;
                let kind = attr(&event, "type")?.unwrap_or_default();
                let source_order = smartart_order_attribute(&event, "srcOrd")?;
                let destination_order = smartart_order_attribute(&event, "destOrd")?;
                raw_connections.push((source, target, kind, source_order, destination_order));
            }
            Ok(Event::End(event)) if event.local_name().as_ref() == "pt" => {
                current_point = None;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PptxError::SemanticExtractionFailed(format!(
                    "SmartArt XML: {error}"
                )));
            }
        }
    }

    let mut connections = Vec::with_capacity(raw_connections.len());
    let mut source_orders = BTreeSet::new();
    let mut destination_orders = BTreeSet::new();
    for (source, target, kind, source_order, destination_order) in raw_connections {
        let source = ids.get(&source).copied().ok_or_else(|| {
            PptxError::SemanticExtractionFailed("SmartArt connection source is unknown".into())
        })?;
        let target = ids.get(&target).copied().ok_or_else(|| {
            PptxError::SemanticExtractionFailed("SmartArt connection target is unknown".into())
        })?;
        if let Some(order) = source_order
            && !source_orders.insert((source, order))
        {
            return Err(PptxError::SemanticExtractionFailed(
                "duplicate SmartArt outgoing connection srcOrd".into(),
            ));
        }
        if let Some(order) = destination_order
            && !destination_orders.insert((target, order))
        {
            return Err(PptxError::SemanticExtractionFailed(
                "duplicate SmartArt incoming connection destOrd".into(),
            ));
        }
        connections.push((source, target, kind, source_order, destination_order));
    }
    connections.sort_unstable_by(|left, right| {
        (left.0, left.3, left.1, left.4, &left.2)
            .cmp(&(right.0, right.3, right.1, right.4, &right.2))
    });
    let connections = connections
        .into_iter()
        .map(|(source, target, kind, source_order, destination_order)| {
            json!({
                "source": source,
                "target": target,
                "kind": kind,
                "source_order": source_order,
                "destination_order": destination_order,
            })
        })
        .collect::<Vec<_>>();
    let points = points
        .into_iter()
        .map(|(role, text)| json!({"role": role, "text": text}))
        .collect::<Vec<_>>();
    Ok(json!({"points": points, "connections": connections}))
}

fn parse_smartart_layout_semantic(data: &[u8]) -> Result<Value, PptxError> {
    let text = std::str::from_utf8(data).map_err(|_| {
        PptxError::SemanticExtractionFailed("SmartArt layout XML is not UTF-8".into())
    })?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut stack = Vec::<(String, BTreeMap<String, String>, Vec<Value>)>::new();
    let mut root = None::<Value>;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let (kind, attributes) = smartart_layout_element(&reader, &event)?;
                validate_smartart_layout_child(stack.last().map(|node| node.0.as_str()), &kind)?;
                stack.push((kind, attributes, Vec::new()));
            }
            Ok(Event::Empty(event)) => {
                let (kind, attributes) = smartart_layout_element(&reader, &event)?;
                validate_smartart_layout_child(stack.last().map(|node| node.0.as_str()), &kind)?;
                if matches!(kind.as_str(), "layoutDef" | "layoutNode" | "forEach") {
                    return Err(PptxError::UnsupportedSemanticConstruct(format!(
                        "empty SmartArt layout {kind} has no modeled meaning"
                    )));
                }
                let value = smartart_layout_projection(kind, attributes, Vec::new());
                if let Some(parent) = stack.last_mut() {
                    parent.2.push(value);
                } else if root.replace(value).is_some() {
                    return Err(PptxError::SemanticExtractionFailed(
                        "SmartArt layout has multiple roots".into(),
                    ));
                }
            }
            Ok(Event::End(event)) => {
                let (namespace, local) = reader.resolver().resolve_element(event.name());
                if !matches!(namespace, ResolveResult::Bound(uri) if uri.as_ref() == SMARTART_NS) {
                    return Err(PptxError::UnsupportedSemanticConstruct(
                        "SmartArt layout contains an element outside the diagram namespace".into(),
                    ));
                }
                let (kind, attributes, children) = stack.pop().ok_or_else(|| {
                    PptxError::SemanticExtractionFailed(
                        "SmartArt layout has an unmatched end".into(),
                    )
                })?;
                if local.as_ref() != kind {
                    return Err(PptxError::SemanticExtractionFailed(
                        "SmartArt layout element nesting is inconsistent".into(),
                    ));
                }
                let value = smartart_layout_projection(kind, attributes, children);
                if let Some(parent) = stack.last_mut() {
                    parent.2.push(value);
                } else if root.replace(value).is_some() {
                    return Err(PptxError::SemanticExtractionFailed(
                        "SmartArt layout has multiple roots".into(),
                    ));
                }
            }
            Ok(Event::Text(event)) if !event.as_ref().trim().is_empty() => {
                return Err(PptxError::UnsupportedSemanticConstruct(
                    "SmartArt layout text content is unsupported".into(),
                ));
            }
            Ok(Event::CData(event)) if !event.as_ref().trim().is_empty() => {
                return Err(PptxError::UnsupportedSemanticConstruct(
                    "SmartArt layout CDATA content is unsupported".into(),
                ));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PptxError::SemanticExtractionFailed(format!(
                    "SmartArt layout XML: {error}"
                )));
            }
        }
    }

    if !stack.is_empty() {
        return Err(PptxError::SemanticExtractionFailed(
            "SmartArt layout ended inside an element".into(),
        ));
    }
    let root = root.ok_or_else(|| {
        PptxError::SemanticExtractionFailed("SmartArt layout has no root element".into())
    })?;
    if root["kind"] != "layoutDef"
        || root["children"]
            .as_array()
            .is_none_or(|children| !children.iter().any(|child| child["kind"] == "layoutNode"))
    {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "SmartArt layout does not contain a modeled layout definition".into(),
        ));
    }
    Ok(root)
}

fn smartart_layout_element<R>(
    reader: &NsReader<R>,
    event: &BytesStart<'_>,
) -> Result<(String, BTreeMap<String, String>), PptxError> {
    let (namespace, local) = reader.resolver().resolve_element(event.name());
    if !matches!(namespace, ResolveResult::Bound(uri) if uri.as_ref() == SMARTART_NS) {
        return Err(PptxError::UnsupportedSemanticConstruct(
            "SmartArt layout contains an element outside the diagram namespace".into(),
        ));
    }
    let kind = local.as_ref().to_owned();
    if !matches!(
        kind.as_str(),
        "layoutDef" | "layoutNode" | "alg" | "param" | "forEach" | "presOf"
    ) {
        return Err(PptxError::UnsupportedSemanticConstruct(format!(
            "unsupported SmartArt layout element {kind}"
        )));
    }

    let mut attributes = BTreeMap::new();
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|error| {
            PptxError::SemanticExtractionFailed(format!(
                "invalid SmartArt layout attribute: {error}"
            ))
        })?;
        if attribute.key.as_ref() == "xmlns" || attribute.key.as_ref().starts_with("xmlns:") {
            continue;
        }
        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
        if !matches!(namespace, ResolveResult::Unbound) {
            return Err(PptxError::UnsupportedSemanticConstruct(
                "SmartArt layout contains a namespaced attribute".into(),
            ));
        }
        let name = local.as_ref().to_owned();
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|error| {
                PptxError::SemanticExtractionFailed(format!(
                    "invalid SmartArt layout attribute value: {error}"
                ))
            })?
            .into_owned();
        if attributes.insert(name, value).is_some() {
            return Err(PptxError::SemanticExtractionFailed(
                "duplicate SmartArt layout attribute".into(),
            ));
        }
    }
    if kind == "layoutDef" {
        attributes.remove("uniqueId");
    }
    if kind == "layoutNode" && !attributes.contains_key("name") {
        return Err(PptxError::SemanticExtractionFailed(
            "SmartArt layout node has no name".into(),
        ));
    }
    if kind == "alg" && !attributes.contains_key("type") {
        return Err(PptxError::SemanticExtractionFailed(
            "SmartArt layout algorithm has no type".into(),
        ));
    }
    if kind == "param" && (!attributes.contains_key("type") || !attributes.contains_key("val")) {
        return Err(PptxError::SemanticExtractionFailed(
            "SmartArt layout parameter has no type or value".into(),
        ));
    }
    Ok((kind, attributes))
}

fn validate_smartart_layout_child(parent: Option<&str>, child: &str) -> Result<(), PptxError> {
    let valid = matches!(
        (parent, child),
        (None, "layoutDef")
            | (Some("layoutDef"), "layoutNode")
            | (
                Some("layoutNode"),
                "layoutNode" | "alg" | "forEach" | "presOf"
            )
            | (Some("alg"), "param")
            | (Some("forEach"), "layoutNode" | "alg" | "presOf")
    );
    if valid {
        Ok(())
    } else {
        Err(PptxError::UnsupportedSemanticConstruct(format!(
            "SmartArt layout element {child} is unsupported in this position"
        )))
    }
}

fn smartart_layout_projection(
    kind: String,
    attributes: BTreeMap<String, String>,
    children: Vec<Value>,
) -> Value {
    json!({
        "kind": kind,
        "attributes": attributes,
        "children": children,
    })
}

fn smartart_order_attribute(event: &BytesStart<'_>, name: &str) -> Result<Option<u32>, PptxError> {
    attr(event, name)?
        .map(|value| {
            value.trim().parse::<u32>().map_err(|_| {
                PptxError::SemanticExtractionFailed(format!(
                    "invalid SmartArt connection order {name}"
                ))
            })
        })
        .transpose()
}

fn shape_projection(
    shape: &Shape,
    external_dependencies: &mut Vec<ExternalDependency>,
    visual_present: &mut bool,
) -> Result<Value, PptxError> {
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
                PptxError::SemanticExtractionFailed(
                    "picture relationship did not resolve to bytes".into(),
                )
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
                children.push(shape_projection(
                    child,
                    external_dependencies,
                    visual_present,
                )?);
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
                                .collect::<Result<Vec<_>, PptxError>>()
                        })
                        .collect::<Result<Vec<_>, PptxError>>()?;
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
) -> Result<Value, PptxError> {
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

fn xml_text_values(data: &[u8], local_name: &str) -> Result<Vec<String>, PptxError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| PptxError::SemanticExtractionFailed("XML is not UTF-8".into()))?;
    let mut reader = Reader::from_str(text);
    let mut values = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.local_name().as_ref() == local_name => {
                let value = reader
                    .read_text(event.name())
                    .map_err(|error| {
                        PptxError::SemanticExtractionFailed(format!("text XML: {error}"))
                    })?
                    .to_string();
                if !value.trim().is_empty() {
                    values.push(value);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PptxError::SemanticExtractionFailed(format!(
                    "text XML: {error}"
                )));
            }
        }
    }
    Ok(values)
}

fn image_semantic_digest(bytes: &[u8]) -> Result<String, PptxError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Ok(encode_hex(Sha256::digest(bytes)));
    }

    let mut normalized = Vec::new();
    normalized.extend_from_slice(PNG_SIGNATURE);
    let mut offset = PNG_SIGNATURE.len();
    let mut saw_iend = false;

    while offset < bytes.len() {
        if bytes.len().saturating_sub(offset) < 12 {
            return Err(PptxError::SemanticExtractionFailed(
                "truncated PNG chunk".into(),
            ));
        }
        let length = u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| PptxError::SemanticExtractionFailed("invalid PNG length".into()))?,
        ) as usize;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(length)
            .ok_or(PptxError::InspectionResourceLimitExceeded)?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or(PptxError::InspectionResourceLimitExceeded)?;
        if chunk_end > bytes.len() {
            return Err(PptxError::SemanticExtractionFailed(
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
        return Err(PptxError::SemanticExtractionFailed(
            "PNG has no IEND chunk".into(),
        ));
    }
    Ok(encode_hex(Sha256::digest(&normalized)))
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
) -> Result<String, PptxError> {
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PptxError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        let (namespace, local) = reader.resolver().resolve_attribute(item.key);
        if matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == namespace_uri)
            && local.as_ref() == local_name
        {
            return Ok(item.value.as_ref().to_owned());
        }
    }
    Err(PptxError::SemanticExtractionFailed(format!(
        "missing required attribute {{{namespace_uri}}}{local_name}"
    )))
}

fn attr_required(event: &BytesStart<'_>, name: &str) -> Result<String, PptxError> {
    attr(event, name)?.ok_or_else(|| {
        PptxError::SemanticExtractionFailed(format!("missing required attribute {name}"))
    })
}

fn attr(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, PptxError> {
    for item in event.attributes() {
        let item = item.map_err(|error| {
            PptxError::SemanticExtractionFailed(format!("invalid XML attribute: {error}"))
        })?;
        let key = item.key.as_ref();
        let matches = key == name || key.rsplit_once(':').is_some_and(|(_, local)| local == name);
        if matches {
            return Ok(Some(item.value.as_ref().to_owned()));
        }
    }
    Ok(None)
}

fn encode_hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
