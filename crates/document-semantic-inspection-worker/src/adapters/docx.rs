use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use document_semantic_inspection_core::{
    CapabilityState, CommentEvidence, EditorialProvenance, FormatId, TrackedChangeEvidence,
};
use office_oxide::docx::DocxDocument;
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::{WorkerFailure, WorkerFailureCode};

use super::{AdapterProfile, SemanticAdapter, SemanticAdapterOutput};

const MAX_ENTRIES: usize = 256;
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 64;
const MAX_XML_NODES: usize = 2_000_000;
const MAX_DOCX_IMAGES: usize = 4_096;
const MAX_DOCX_DECODED_PIXELS: u64 = 67_108_864;
const MAX_DOCX_DECODED_OUTPUT_BYTES: usize = 268_435_456;
const DRAWINGML_ANGLE_UNITS_PER_TURN: i32 = 21_600_000;
const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const DRAWING_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const DRAWING_PICTURE_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const OFFICE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WORD_DRAWING_NS: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const WORD_2010_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const WORD_2012_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const PACKAGE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
const PACKAGE_CONTENT_TYPES_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/content-types";
const CORE_PROPERTIES_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const WORD_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const PARSER_LIBRARIES: [(&str, &str); 4] = [
    ("office_oxide", "0.1.11"),
    ("zip", "8.6.0"),
    ("quick-xml", "0.42.0"),
    ("png", "0.18.1"),
];

#[derive(Debug, Clone, Copy, Default)]
pub struct DocxAdapter;

#[derive(Debug, Clone, Copy, Default)]
pub struct OoxmlCoverageSentinel;

#[derive(Debug)]
struct PackageInspection {
    parts: BTreeMap<String, Vec<u8>>,
    relationships_by_source: BTreeMap<String, BTreeMap<String, Relationship>>,
    selected_section_references: BTreeSet<(usize, String, String)>,
    selected_headers: Vec<String>,
    selected_footers: Vec<String>,
    editorial: EditorialProvenance,
    styles: ParagraphStyles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionReferenceKind {
    Header,
    Footer,
}

impl SectionReferenceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Footer => "footer",
        }
    }
}

#[derive(Debug)]
struct SectionReference {
    section_index: usize,
    kind: SectionReferenceKind,
    id: String,
    reference_type: String,
    title_page_enabled: bool,
}

#[derive(Debug, Clone, Copy)]
struct SectionReferenceKey<'a> {
    kind: &'a str,
    section_index: usize,
}

struct DocumentProjectionContext<'a> {
    package: &'a PackageInspection,
    numbering: &'a BTreeMap<(u32, u8), NumberingLevel>,
    numbering_instances: &'a mut BTreeMap<u32, usize>,
    footnotes: &'a BTreeMap<String, NoteEntry>,
    endnotes: &'a BTreeMap<String, NoteEntry>,
    image_budget: &'a mut ImageBudget,
}

#[derive(Debug, Default)]
struct SelectedSectionParts {
    references: BTreeSet<(usize, String, String)>,
    headers: Vec<String>,
    footers: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct ZipPreflight {
    archive_offset: u64,
    central_directory_start: u64,
    entry_count: usize,
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
struct ImageBudget {
    count: usize,
    decoded_pixels: u64,
    decoded_output_bytes: usize,
}

#[derive(Debug, Default)]
struct PictureProjection {
    referenced_image: bool,
    unsupported_geometry: Option<String>,
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

#[derive(Debug)]
enum PngError {
    SemanticExtractionFailed(String),
    UnsupportedSemanticConstruct(String),
    InspectionResourceLimitExceeded,
}

impl From<PngError> for WorkerFailure {
    fn from(error: PngError) -> Self {
        match error {
            PngError::SemanticExtractionFailed(message) => {
                failure(WorkerFailureCode::SemanticExtractionFailed, message)
            }
            PngError::UnsupportedSemanticConstruct(message) => {
                failure(WorkerFailureCode::UnsupportedSemanticConstruct, message)
            }
            PngError::InspectionResourceLimitExceeded => resource_limit(),
        }
    }
}

#[derive(Debug, Clone)]
struct Relationship {
    kind: String,
    target: String,
    external: bool,
}

#[derive(Debug, Default)]
struct ComplexField {
    instruction: String,
    separated: bool,
}

#[derive(Debug)]
struct StyleDefinition {
    kind: String,
    based_on: Option<String>,
    outline_level: Option<u8>,
    has_numbering: bool,
    is_default: bool,
}

#[derive(Debug, Default)]
struct ParagraphStyles {
    outline_levels: BTreeMap<String, Option<u8>>,
    numbered_styles: BTreeSet<String>,
    default_outline_level: Option<u8>,
    default_numbering_style: Option<String>,
}

#[derive(Debug)]
struct NumberingLevel {
    format: String,
    start: u32,
    level_text: Option<String>,
    multi_level_type: Option<String>,
}

#[derive(Debug)]
struct NoteEntry {
    content: String,
    unsupported_construct: Option<String>,
}

#[derive(Debug)]
struct CurrentNote {
    id: String,
    content: String,
    meaningful_type: bool,
    unsupported_construct: Option<String>,
    paragraph_count: usize,
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

impl SemanticAdapter for DocxAdapter {
    fn format(&self) -> FormatId {
        FormatId::Docx
    }

    fn inspect(
        &self,
        input: &[u8],
        _profile: &AdapterProfile,
    ) -> Result<SemanticAdapterOutput, WorkerFailure> {
        OoxmlCoverageSentinel::validate_package(input)?;
        let package = inspect_package(input)?;
        let numbering = parse_numbering(
            package
                .parts
                .get("word/numbering.xml")
                .map(Vec::as_slice)
                .unwrap_or_default(),
        )?;
        let document_bytes = package.parts.get("word/document.xml").ok_or_else(|| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                "missing word/document.xml",
            )
        })?;
        let footnote_entries = note_entries(package.parts.get("word/footnotes.xml"), "footnote")?;
        let endnote_entries = note_entries(package.parts.get("word/endnotes.xml"), "endnote")?;
        let mut image_budget = ImageBudget::default();
        let mut numbering_instances = BTreeMap::new();
        let (body_tokens, body_text) = parse_document_projection(
            document_bytes,
            "word/document.xml",
            DocumentProjectionContext {
                package: &package,
                numbering: &numbering,
                numbering_instances: &mut numbering_instances,
                footnotes: &footnote_entries,
                endnotes: &endnote_entries,
                image_budget: &mut image_budget,
            },
        )?;
        let all_header_texts = texts_for_prefix(&package.parts, "word/header")?;
        let all_footer_texts = texts_for_prefix(&package.parts, "word/footer")?;
        let headers = texts_for_parts(&package.parts, &package.selected_headers)?;
        let footers = texts_for_parts(&package.parts, &package.selected_footers)?;
        let footnotes = note_values(&footnote_entries);
        let endnotes = note_values(&endnote_entries);

        let mut visible = Vec::new();
        visible.extend(all_header_texts);
        if !body_text.is_empty() {
            visible.push(body_text.clone());
        }
        visible.extend(all_footer_texts);
        let raw_visible = normalize_text(&visible.join(" "));
        let candidate = DocxDocument::from_reader(Cursor::new(input)).map_err(|error| {
            WorkerFailure::new(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("office_oxide rejected DOCX: {error}"),
            )
        })?;
        let candidate_text = normalize_text(&candidate.plain_text());
        if candidate_text != raw_visible {
            return Err(parser_disagreement_failure(&raw_visible, &candidate_text));
        }
        let projection = DocxProjection {
            plain_text: body_text,
            body_tokens,
            headers,
            footers,
            footnotes,
            endnotes,
        };
        let semantic_projection = super::canonical_json_bytes(&projection).map_err(|error| {
            failure(
                WorkerFailureCode::InvalidWorkerResult,
                format!("DOCX projection serialization failed: {error}"),
            )
        })?;
        let footnote_state = if footnote_entries
            .values()
            .any(|entry| !entry.content.is_empty())
        {
            CapabilityState::Present
        } else {
            CapabilityState::Absent
        };
        let endnote_state = if endnote_entries
            .values()
            .any(|entry| !entry.content.is_empty())
        {
            CapabilityState::Present
        } else {
            CapabilityState::Absent
        };
        Ok(SemanticAdapterOutput::from_projection(
            &semantic_projection,
            &[
                "reader_content",
                "document_structure",
                "footnotes",
                "endnotes",
            ],
            "docx",
            &PARSER_LIBRARIES,
        )
        .with_capability_state("footnotes", footnote_state)?
        .with_capability_state("endnotes", endnote_state)?
        .with_editorial_provenance(package.editorial))
    }
}

fn parser_disagreement_failure(raw_visible: &str, candidate_text: &str) -> WorkerFailure {
    failure(
        WorkerFailureCode::ParserDisagreement,
        format!(
            "raw OOXML visible text differs from office_oxide: raw_bytes={}, candidate_bytes={}",
            raw_visible.len(),
            candidate_text.len()
        ),
    )
}

impl OoxmlCoverageSentinel {
    pub fn validate_package(input: &[u8]) -> Result<(), WorkerFailure> {
        let parts = load_package_parts(input)?;
        validate_raw_package(&parts)
    }
}

fn failure(code: WorkerFailureCode, message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::new(code, message)
}

fn inspect_package(input: &[u8]) -> Result<PackageInspection, WorkerFailure> {
    let parts = load_package_parts(input)?;
    validate_raw_package(&parts)?;
    let styles = parse_styles(parts.get("word/styles.xml").map(Vec::as_slice))?;
    let mut relationships_by_source = BTreeMap::new();
    for (rels_name, data) in parts.iter().filter(|(name, _)| name.ends_with(".rels")) {
        let source = source_part_for_relationships(rels_name)?;
        let relationships = parse_all_relationships(data, rels_name, &source)?
            .into_iter()
            .collect();
        relationships_by_source.insert(source, relationships);
    }
    let document = parts.get("word/document.xml").ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "missing word/document.xml",
        )
    })?;
    let section_references = parse_section_references(document)?;
    let selected_sections =
        resolve_selected_section_parts(&section_references, &parts, &relationships_by_source)?;
    let editorial = parse_editorial_evidence(
        &parts,
        document,
        &selected_sections.headers,
        &selected_sections.footers,
    )?;
    Ok(PackageInspection {
        parts,
        relationships_by_source,
        selected_section_references: selected_sections.references,
        selected_headers: selected_sections.headers,
        selected_footers: selected_sections.footers,
        editorial,
        styles,
    })
}

fn parse_section_references(document: &[u8]) -> Result<Vec<SectionReference>, WorkerFailure> {
    let text = std::str::from_utf8(document).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "document XML is not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut references = Vec::new();
    let mut pending = Vec::new();
    let mut in_section = false;
    let mut title_page_enabled = false;
    let mut title_page_seen = false;
    let mut formatting_revision_depth = 0usize;
    let mut deleted_depth = 0usize;
    let mut section_index = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let local = event.local_name();
                if matches!(local.as_ref(), "pPrChange" | "rPrChange" | "sectPrChange") {
                    formatting_revision_depth += 1;
                    continue;
                }
                if matches!(local.as_ref(), "del" | "moveFrom") {
                    deleted_depth += 1;
                    continue;
                }
                if deleted_depth > 0 || formatting_revision_depth > 0 {
                    continue;
                }
                match local.as_ref() {
                    "sectPr" => {
                        if in_section {
                            return Err(failure(
                                WorkerFailureCode::SemanticExtractionFailed,
                                "document XML contains nested section properties",
                            ));
                        }
                        in_section = true;
                        section_index = section_index.saturating_add(1);
                        title_page_enabled = false;
                        title_page_seen = false;
                        pending.clear();
                    }
                    "titlePg" if in_section => {
                        if title_page_seen {
                            return Err(failure(
                                WorkerFailureCode::SemanticExtractionFailed,
                                "section properties contain multiple titlePg elements",
                            ));
                        }
                        title_page_seen = true;
                        title_page_enabled = parse_on_off_element(&event, "val")?;
                    }
                    "headerReference" if in_section => {
                        pending.push((
                            SectionReferenceKind::Header,
                            attr_required(&event, "id")?,
                            validated_reference_type(&event, "header")?,
                        ));
                    }
                    "footerReference" if in_section => {
                        pending.push((
                            SectionReferenceKind::Footer,
                            attr_required(&event, "id")?,
                            validated_reference_type(&event, "footer")?,
                        ));
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(event)) => {
                let local = event.local_name();
                if deleted_depth > 0 || formatting_revision_depth > 0 {
                    continue;
                }
                match local.as_ref() {
                    "sectPr" => {
                        if in_section {
                            return Err(failure(
                                WorkerFailureCode::SemanticExtractionFailed,
                                "document XML contains nested section properties",
                            ));
                        }
                        section_index = section_index.saturating_add(1);
                    }
                    "titlePg" if in_section => {
                        if title_page_seen {
                            return Err(failure(
                                WorkerFailureCode::SemanticExtractionFailed,
                                "section properties contain multiple titlePg elements",
                            ));
                        }
                        title_page_seen = true;
                        title_page_enabled = parse_on_off_element(&event, "val")?;
                    }
                    "headerReference" if in_section => {
                        pending.push((
                            SectionReferenceKind::Header,
                            attr_required(&event, "id")?,
                            validated_reference_type(&event, "header")?,
                        ));
                    }
                    "footerReference" if in_section => {
                        pending.push((
                            SectionReferenceKind::Footer,
                            attr_required(&event, "id")?,
                            validated_reference_type(&event, "footer")?,
                        ));
                    }
                    _ => {}
                }
            }
            Ok(Event::End(event)) => {
                let local = event.local_name();
                if matches!(local.as_ref(), "pPrChange" | "rPrChange" | "sectPrChange") {
                    formatting_revision_depth = formatting_revision_depth.saturating_sub(1);
                    continue;
                }
                if matches!(local.as_ref(), "del" | "moveFrom") {
                    deleted_depth = deleted_depth.saturating_sub(1);
                    continue;
                }
                if deleted_depth > 0 || formatting_revision_depth > 0 {
                    continue;
                }
                if local.as_ref() == "sectPr" {
                    if !in_section {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            "section properties closed without an opening element",
                        ));
                    }
                    references.extend(pending.drain(..).map(|(kind, id, reference_type)| {
                        SectionReference {
                            section_index,
                            kind,
                            id,
                            reference_type,
                            title_page_enabled,
                        }
                    }));
                    in_section = false;
                    title_page_enabled = false;
                    title_page_seen = false;
                }
            }
            Ok(Event::DocType(_)) => {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    "document XML must not contain a DTD/entity declaration",
                ));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("section-reference parse failed: {error}"),
                ));
            }
        }
    }

    if in_section || formatting_revision_depth != 0 || deleted_depth != 0 {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "document XML ended inside section properties",
        ));
    }
    Ok(references)
}

fn validated_reference_type(event: &BytesStart<'_>, kind: &str) -> Result<String, WorkerFailure> {
    let reference_type = attr(event, "type")?.unwrap_or_else(|| "default".into());
    if reference_type == "even" {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("even {kind} selection is unsupported without settings-aware projection"),
        ));
    }
    if !matches!(reference_type.as_str(), "default" | "first") {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unsupported {kind} reference type {reference_type}"),
        ));
    }
    Ok(reference_type)
}

fn resolve_selected_section_parts(
    references: &[SectionReference],
    parts: &BTreeMap<String, Vec<u8>>,
    relationships_by_source: &BTreeMap<String, BTreeMap<String, Relationship>>,
) -> Result<SelectedSectionParts, WorkerFailure> {
    let mut selected = SelectedSectionParts::default();
    let mut seen_headers = BTreeSet::new();
    let mut seen_footers = BTreeSet::new();

    for reference in references {
        if reference.reference_type == "first" && !reference.title_page_enabled {
            continue;
        }
        let kind = reference.kind.as_str();
        let relation = relationships_by_source
            .get("word/document.xml")
            .and_then(|relationships| relationships.get(&reference.id))
            .ok_or_else(|| {
                failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!(
                        "{kind} relationship {} is missing from word/document.xml",
                        reference.id
                    ),
                )
            })?;
        if !relation.kind.ends_with(&format!("/{kind}")) || relation.external {
            return Err(failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!("relationship {} is not an internal {kind}", reference.id),
            ));
        }
        let path = resolve_word_target(&relation.target)?;
        if !parts.contains_key(&path) {
            return Err(failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("{kind} target {path} is missing"),
            ));
        }
        selected.references.insert((
            reference.section_index,
            kind.to_owned(),
            reference.id.clone(),
        ));
        let (paths, seen) = match reference.kind {
            SectionReferenceKind::Header => (&mut selected.headers, &mut seen_headers),
            SectionReferenceKind::Footer => (&mut selected.footers, &mut seen_footers),
        };
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    Ok(selected)
}

fn load_package_parts(input: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, WorkerFailure> {
    let preflight = preflight_docx_zip(input)?;
    let mut archive = ZipArchive::new(Cursor::new(input)).map_err(|error| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("invalid DOCX ZIP: {error}"),
        )
    })?;
    if archive.offset() != preflight.archive_offset
        || archive.central_directory_start() != preflight.central_directory_start
        || archive.len() != preflight.entry_count
    {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "ZIP preflight disagrees with parsed archive metadata",
        ));
    }
    let mut total_actual = 0u64;
    let mut xml_nodes_total = 0usize;
    let mut seen_names = BTreeSet::new();
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("invalid DOCX ZIP entry: {error}"),
            )
        })?;
        let name = entry.name().to_owned();
        let is_directory = entry.is_dir();
        let normalized_name = name.strip_suffix('/').unwrap_or(&name).to_ascii_lowercase();
        if !seen_names.insert(normalized_name) {
            return Err(failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("duplicate or case-aliased ZIP entry {name}"),
            ));
        }
        if is_directory {
            validate_part_name(name.trim_end_matches('/'))?;
            continue;
        }
        validate_part_name(&name)?;
        let declared_size = entry.size();
        if declared_size > MAX_ENTRY_BYTES {
            return Err(resource_limit());
        }
        let mut actual = Vec::new();
        entry
            .by_ref()
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut actual)
            .map_err(|error| {
                failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("cannot decompress DOCX part {name}: {error}"),
                )
            })?;
        if actual.len() as u64 > MAX_ENTRY_BYTES {
            return Err(resource_limit());
        }
        total_actual = total_actual
            .checked_add(actual.len() as u64)
            .ok_or_else(resource_limit)?;
        if total_actual > MAX_TOTAL_BYTES {
            return Err(resource_limit());
        }
        if declared_size != actual.len() as u64 {
            return Err(failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("ZIP size metadata disagrees with decompressed size for {name}"),
            ));
        }
        if name.ends_with(".xml") || name.ends_with(".rels") {
            xml_nodes_total = xml_nodes_total
                .checked_add(validate_xml(&name, &actual)?)
                .ok_or_else(resource_limit)?;
            if xml_nodes_total > MAX_XML_NODES {
                return Err(resource_limit());
            }
        }
        if parts.insert(name.clone(), actual).is_some() {
            return Err(failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("duplicate ZIP entry {name}"),
            ));
        }
    }
    Ok(parts)
}

fn preflight_docx_zip(input: &[u8]) -> Result<ZipPreflight, WorkerFailure> {
    const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
    const EOCD_FIXED_SIZE: usize = 22;
    const MAX_EOCD_COMMENT_SIZE: usize = u16::MAX as usize;

    if input.len() < EOCD_FIXED_SIZE {
        return Err(zip_structure_failure(
            "truncated ZIP end-of-central-directory record",
        ));
    }

    // A valid EOCD must end exactly at EOF after its declared comment. More than one such
    // candidate is ambiguous: zip readers may select different records from the same bytes.
    let search_start = input
        .len()
        .saturating_sub(EOCD_FIXED_SIZE + MAX_EOCD_COMMENT_SIZE);
    let search_end = input.len() - EOCD_FIXED_SIZE;
    let mut eocd_offset = None;
    for offset in search_start..=search_end {
        if read_zip_u32(input, offset) != Some(END_OF_CENTRAL_DIRECTORY) {
            continue;
        }
        let Some(comment_len) = read_zip_u16(input, offset + 20) else {
            continue;
        };
        let Some(record_end) = offset
            .checked_add(EOCD_FIXED_SIZE)
            .and_then(|value| value.checked_add(usize::from(comment_len)))
        else {
            continue;
        };
        if record_end != input.len() {
            continue;
        }
        if eocd_offset.replace(offset).is_some() {
            return Err(zip_structure_failure(
                "ambiguous ZIP end-of-central-directory records",
            ));
        }
    }
    let eocd_offset = eocd_offset
        .ok_or_else(|| zip_structure_failure("ZIP end-of-central-directory record not found"))?;

    let disk_number = read_zip_u16(input, eocd_offset + 4)
        .ok_or_else(|| zip_structure_failure("truncated ZIP disk number"))?;
    let central_directory_disk = read_zip_u16(input, eocd_offset + 6)
        .ok_or_else(|| zip_structure_failure("truncated ZIP central-directory disk number"))?;
    let entries_on_disk = read_zip_u16(input, eocd_offset + 8)
        .ok_or_else(|| zip_structure_failure("truncated ZIP disk entry count"))?;
    let total_entries = read_zip_u16(input, eocd_offset + 10)
        .ok_or_else(|| zip_structure_failure("truncated ZIP entry count"))?;
    if entries_on_disk == u16::MAX || total_entries == u16::MAX {
        return Err(zip_structure_failure("ZIP64 archives are unsupported"));
    }
    if usize::from(entries_on_disk) > MAX_ENTRIES || usize::from(total_entries) > MAX_ENTRIES {
        return Err(resource_limit());
    }
    if disk_number != 0 || central_directory_disk != 0 || entries_on_disk != total_entries {
        return Err(zip_structure_failure(
            "multi-disk ZIP archives are unsupported",
        ));
    }

    let central_directory_size = read_zip_u32(input, eocd_offset + 12)
        .ok_or_else(|| zip_structure_failure("truncated ZIP central-directory size"))?;
    let central_directory_offset = read_zip_u32(input, eocd_offset + 16)
        .ok_or_else(|| zip_structure_failure("truncated ZIP central-directory offset"))?;
    if central_directory_size == u32::MAX || central_directory_offset == u32::MAX {
        return Err(zip_structure_failure("ZIP64 archives are unsupported"));
    }

    let central_directory_size = usize::try_from(central_directory_size)
        .map_err(|_| zip_structure_failure("ZIP central-directory size is out of range"))?;
    let central_directory_offset = usize::try_from(central_directory_offset)
        .map_err(|_| zip_structure_failure("ZIP central-directory offset is out of range"))?;
    let central_directory_start = eocd_offset
        .checked_sub(central_directory_size)
        .ok_or_else(|| zip_structure_failure("ZIP central directory extends beyond EOF"))?;
    if central_directory_offset > central_directory_start {
        return Err(zip_structure_failure(
            "ZIP central-directory offset precedes the archive boundary",
        ));
    }
    let archive_offset = central_directory_start - central_directory_offset;
    if central_directory_start.checked_add(central_directory_size) != Some(eocd_offset) {
        return Err(zip_structure_failure(
            "ZIP central directory does not end at its EOCD record",
        ));
    }

    let mut offset = central_directory_start;
    let central_directory_end = eocd_offset;
    let mut names = BTreeSet::new();
    let mut local_ranges = Vec::with_capacity(usize::from(total_entries));
    let mut declared_total = 0u64;

    for _ in 0..total_entries {
        const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
        const CENTRAL_HEADER_SIZE: usize = 46;

        if read_zip_u32(input, offset) != Some(CENTRAL_DIRECTORY_HEADER) {
            return Err(zip_structure_failure(
                "unexpected record in ZIP central directory",
            ));
        }
        let header_end = offset
            .checked_add(CENTRAL_HEADER_SIZE)
            .ok_or_else(|| zip_structure_failure("ZIP central-directory offset overflow"))?;
        let header = input
            .get(offset..header_end)
            .ok_or_else(|| zip_structure_failure("truncated ZIP central-directory entry"))?;
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
            return Err(zip_structure_failure("ZIP64 entries are unsupported"));
        }
        if disk_start != 0 {
            return Err(zip_structure_failure(
                "multi-disk ZIP entries are unsupported",
            ));
        }
        if flags & (0x0001 | 0x0040 | 0x2000) != 0 {
            return Err(zip_structure_failure(
                "encrypted ZIP entries are unsupported",
            ));
        }
        if method != 0 && method != 8 {
            return Err(zip_structure_failure(
                "DOCX ZIP entry uses an unsupported compression method",
            ));
        }

        let name_start = header_end;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or_else(|| zip_structure_failure("ZIP entry name length overflow"))?;
        let extra_start = name_end;
        let extra_end = extra_start
            .checked_add(extra_len)
            .ok_or_else(|| zip_structure_failure("ZIP entry extra-field length overflow"))?;
        let entry_end = extra_end
            .checked_add(comment_len)
            .ok_or_else(|| zip_structure_failure("ZIP entry comment length overflow"))?;
        if entry_end > central_directory_end {
            return Err(zip_structure_failure(
                "truncated ZIP central-directory variable field",
            ));
        }
        let name = input
            .get(name_start..name_end)
            .ok_or_else(|| zip_structure_failure("truncated ZIP central-directory name"))?;
        if name.is_empty() {
            return Err(zip_structure_failure("empty ZIP entry name"));
        }
        validate_zip_extra_fields(input, extra_start, extra_end)?;

        let normalized_name = normalized_zip_name(name)?;
        if !names.insert(normalized_name) {
            return Err(zip_structure_failure(
                "duplicate or file/directory-aliased ZIP entry name",
            ));
        }

        let uncompressed_size = u64::from(uncompressed_size);
        let compressed_size = u64::from(compressed_size);
        if uncompressed_size > MAX_ENTRY_BYTES {
            return Err(resource_limit());
        }
        declared_total = declared_total
            .checked_add(uncompressed_size)
            .ok_or_else(resource_limit)?;
        if declared_total > MAX_TOTAL_BYTES {
            return Err(resource_limit());
        }

        let local_start =
            archive_offset
                .checked_add(usize::try_from(local_header_offset).map_err(|_| {
                    zip_structure_failure("ZIP local-header offset is out of range")
                })?)
                .ok_or_else(|| zip_structure_failure("ZIP local-header offset overflow"))?;
        let entry = ZipEntryMetadata {
            name,
            flags,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
        };
        let local_end = validate_local_header(input, local_start, central_directory_start, entry)?;
        local_ranges.push((local_start, local_end));
        offset = entry_end;
    }

    if offset != central_directory_end {
        return Err(zip_structure_failure(
            "ZIP central-directory size disagrees with its declared entry count",
        ));
    }

    local_ranges.sort_unstable_by_key(|range| range.0);
    let mut covered_until = archive_offset;
    for (local_start, local_end) in local_ranges {
        if local_start != covered_until {
            return Err(zip_structure_failure(
                "ZIP local records do not contiguously cover archive data",
            ));
        }
        covered_until = local_end;
    }
    if covered_until != central_directory_start {
        return Err(zip_structure_failure(
            "ZIP local records do not contiguously cover archive data",
        ));
    }

    Ok(ZipPreflight {
        archive_offset: u64::try_from(archive_offset)
            .map_err(|_| zip_structure_failure("ZIP archive offset is out of range"))?,
        central_directory_start: u64::try_from(central_directory_start)
            .map_err(|_| zip_structure_failure("ZIP central-directory offset is out of range"))?,
        entry_count: usize::from(total_entries),
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

    if start >= central_directory_start || read_zip_u32(input, start) != Some(LOCAL_FILE_HEADER) {
        return Err(zip_structure_failure(
            "invalid ZIP local-file header offset",
        ));
    }
    let header_end = start
        .checked_add(LOCAL_HEADER_SIZE)
        .ok_or_else(|| zip_structure_failure("ZIP local-header offset overflow"))?;
    let header = input
        .get(start..header_end)
        .ok_or_else(|| zip_structure_failure("truncated ZIP local-file header"))?;
    let flags = u16::from_le_bytes([header[6], header[7]]);
    let method = u16::from_le_bytes([header[8], header[9]]);
    let crc32 = u32::from_le_bytes([header[14], header[15], header[16], header[17]]);
    let compressed_size = u32::from_le_bytes([header[18], header[19], header[20], header[21]]);
    let uncompressed_size = u32::from_le_bytes([header[22], header[23], header[24], header[25]]);
    let name_len = usize::from(u16::from_le_bytes([header[26], header[27]]));
    let extra_len = usize::from(u16::from_le_bytes([header[28], header[29]]));
    if compressed_size == u32::MAX || uncompressed_size == u32::MAX {
        return Err(zip_structure_failure("ZIP64 local entries are unsupported"));
    }
    if flags != central.flags || method != central.method {
        return Err(zip_structure_failure(
            "ZIP local and central entry flags or methods disagree",
        ));
    }
    let name_start = header_end;
    let name_end = name_start
        .checked_add(name_len)
        .ok_or_else(|| zip_structure_failure("ZIP local name length overflow"))?;
    let extra_start = name_end;
    let extra_end = extra_start
        .checked_add(extra_len)
        .ok_or_else(|| zip_structure_failure("ZIP local extra-field length overflow"))?;
    if extra_end > central_directory_start {
        return Err(zip_structure_failure(
            "ZIP local header extends into the central directory",
        ));
    }
    let local_name = input
        .get(name_start..name_end)
        .ok_or_else(|| zip_structure_failure("truncated ZIP local-file name"))?;
    if local_name != central.name {
        return Err(zip_structure_failure(
            "ZIP local and central entry names disagree",
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
            return Err(zip_structure_failure(
                "ZIP local data-descriptor placeholders disagree with central metadata",
            ));
        }
    } else if crc32 != central.crc32
        || local_compressed_size != central.compressed_size
        || local_uncompressed_size != central.uncompressed_size
    {
        return Err(zip_structure_failure(
            "ZIP local and central entry sizes or checksums disagree",
        ));
    }

    let data_start = extra_end;
    let data_end = data_start
        .checked_add(
            usize::try_from(central.compressed_size)
                .map_err(|_| zip_structure_failure("ZIP compressed size is out of range"))?,
        )
        .ok_or_else(|| zip_structure_failure("ZIP compressed-data range overflow"))?;
    if data_end > central_directory_start {
        return Err(zip_structure_failure(
            "ZIP compressed data extends into the central directory",
        ));
    }
    if !has_data_descriptor {
        return Ok(data_end);
    }

    let unsigned_descriptor_end = data_end.checked_add(12);
    let unsigned_matches = unsigned_descriptor_end.is_some_and(|end| {
        end <= central_directory_start
            && read_zip_u32(input, data_end) == Some(central.crc32)
            && read_zip_u32(input, data_end + 4) == u32::try_from(central.compressed_size).ok()
            && read_zip_u32(input, data_end + 8) == u32::try_from(central.uncompressed_size).ok()
    });
    let signed_descriptor_end = data_end.checked_add(16);
    let signed_matches = signed_descriptor_end.is_some_and(|end| {
        end <= central_directory_start
            && read_zip_u32(input, data_end) == Some(0x0807_4b50)
            && read_zip_u32(input, data_end + 4) == Some(central.crc32)
            && read_zip_u32(input, data_end + 8) == u32::try_from(central.compressed_size).ok()
            && read_zip_u32(input, data_end + 12) == u32::try_from(central.uncompressed_size).ok()
    });
    match (unsigned_matches, signed_matches) {
        (true, false) => Ok(unsigned_descriptor_end.unwrap_or(data_end)),
        (false, true) => Ok(signed_descriptor_end.unwrap_or(data_end)),
        _ => Err(zip_structure_failure(
            "invalid or ambiguous ZIP data descriptor",
        )),
    }
}

fn validate_zip_extra_fields(input: &[u8], start: usize, end: usize) -> Result<(), WorkerFailure> {
    let mut offset = start;
    while offset < end {
        let field_header_end = offset
            .checked_add(4)
            .ok_or_else(|| zip_structure_failure("ZIP extra-field offset overflow"))?;
        let header = input
            .get(offset..field_header_end)
            .filter(|_| field_header_end <= end)
            .ok_or_else(|| zip_structure_failure("truncated ZIP extra-field header"))?;
        let field_id = u16::from_le_bytes([header[0], header[1]]);
        let field_len = usize::from(u16::from_le_bytes([header[2], header[3]]));
        match field_id {
            0x0001 => {
                return Err(zip_structure_failure("ZIP64 extra fields are unsupported"));
            }
            0x7075 => {
                return Err(zip_structure_failure(
                    "ZIP Unicode Path extra fields are unsupported",
                ));
            }
            _ => {}
        }
        offset = field_header_end
            .checked_add(field_len)
            .filter(|value| *value <= end)
            .ok_or_else(|| zip_structure_failure("truncated ZIP extra-field value"))?;
    }
    Ok(())
}

fn normalized_zip_name(name: &[u8]) -> Result<Vec<u8>, WorkerFailure> {
    if name.is_empty() || name.contains(&0) {
        return Err(zip_structure_failure(
            "empty or NUL-containing ZIP entry name",
        ));
    }
    let is_directory = name.ends_with(b"/");
    if is_directory && name.ends_with(b"//") {
        return Err(zip_structure_failure("non-canonical ZIP directory name"));
    }
    let name = if is_directory {
        &name[..name.len() - 1]
    } else {
        name
    };
    if name.is_empty() {
        return Err(zip_structure_failure("empty ZIP entry name"));
    }
    Ok(name.to_ascii_lowercase())
}

fn zip_structure_failure(message: impl Into<String>) -> WorkerFailure {
    failure(WorkerFailureCode::SemanticExtractionFailed, message)
}

fn read_zip_u16(input: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let bytes = input.get(offset..end)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_zip_u32(input: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes = input.get(offset..end)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn validate_raw_package(parts: &BTreeMap<String, Vec<u8>>) -> Result<(), WorkerFailure> {
    let content_types = parts.get("[Content_Types].xml").ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "missing [Content_Types].xml",
        )
    })?;
    validate_content_types(parts, content_types)?;
    validate_known_parts_and_qnames(parts)?;
    validate_relationship_coverage(parts)?;
    if !parts.contains_key("word/document.xml") {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "missing word/document.xml",
        ));
    }
    Ok(())
}

fn validate_part_name(name: &str) -> Result<(), WorkerFailure> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains(['\\', '%', '?', '#', ':'])
        || name
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(WorkerFailure::new(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("unsafe or non-canonical OOXML part name {name:?}"),
        ));
    }
    Ok(())
}

fn validate_xml(name: &str, bytes: &[u8]) -> Result<usize, WorkerFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        WorkerFailure::new(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("{name} is not UTF-8 XML"),
        )
    })?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut depth = 0usize;
    let mut nodes = 0usize;
    let mut roots = 0usize;
    loop {
        let event = reader.read_event().map_err(|error| {
            WorkerFailure::new(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("{name} XML parse failed: {error}"),
            )
        })?;
        if !matches!(&event, Event::End(_) | Event::Eof) {
            // Count node-producing events; an End event closes an already-counted element.
            nodes = nodes.checked_add(1).ok_or_else(resource_limit)?;
            if nodes > MAX_XML_NODES {
                return Err(resource_limit());
            }
        }
        match event {
            Event::Start(_) => {
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
                if depth > MAX_XML_DEPTH {
                    return Err(resource_limit());
                }
            }
            Event::Empty(_) => {
                if depth == 0 {
                    roots += 1;
                }
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(WorkerFailure::new(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("{name} has unmatched closing tag"),
                    ));
                }
                depth -= 1;
            }
            Event::DocType(_) => {
                return Err(WorkerFailure::new(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("{name} contains a DTD/entity declaration"),
                ));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || roots != 1 {
        return Err(WorkerFailure::new(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("{name} must contain exactly one complete XML root"),
        ));
    }
    Ok(nodes)
}

fn parse_styles(data: Option<&[u8]>) -> Result<ParagraphStyles, WorkerFailure> {
    let Some(data) = data else {
        return Ok(ParagraphStyles::default());
    };
    let text = std::str::from_utf8(data).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "styles XML is not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut definitions = BTreeMap::new();
    let mut current: Option<(String, StyleDefinition)> = None;
    let mut paragraph_properties_depth = 0usize;
    let mut formatting_revision_depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "style" => {
                    if current.is_some() {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            "styles XML contains a nested style definition",
                        ));
                    }
                    let style_id = attr_required(&event, "styleId")?;
                    let kind = attr_required(&event, "type")?;
                    if !matches!(
                        kind.as_str(),
                        "paragraph" | "character" | "table" | "numbering"
                    ) {
                        return Err(failure(
                            WorkerFailureCode::UnsupportedSemanticConstruct,
                            format!("unsupported Word style type {kind}"),
                        ));
                    }
                    let is_default = parse_on_off_attribute(&event, "default")?;
                    current = Some((
                        style_id,
                        StyleDefinition {
                            kind,
                            based_on: None,
                            outline_level: None,
                            has_numbering: false,
                            is_default,
                        },
                    ));
                }
                "pPrChange" | "rPrChange" | "sectPrChange" if current.is_some() => {
                    formatting_revision_depth += 1;
                }
                "pPr" if current.is_some() && formatting_revision_depth == 0 => {
                    paragraph_properties_depth += 1;
                }
                "outlineLvl"
                    if current.is_some()
                        && paragraph_properties_depth > 0
                        && formatting_revision_depth == 0 =>
                {
                    let level = parse_outline_level(&event)?;
                    set_style_outline_level(&mut current, level)?;
                }
                "numPr"
                    if current.is_some()
                        && paragraph_properties_depth > 0
                        && formatting_revision_depth == 0 =>
                {
                    set_style_has_numbering(&mut current);
                }
                "basedOn"
                    if current.is_some()
                        && paragraph_properties_depth == 0
                        && formatting_revision_depth == 0 =>
                {
                    let based_on = attr_required(&event, "val")?;
                    set_style_base(&mut current, based_on)?;
                }
                _ => {}
            },
            Ok(Event::Empty(event)) => match event.local_name().as_ref() {
                "style" => {
                    if current.is_some() {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            "styles XML contains a nested style definition",
                        ));
                    }
                    let style_id = attr_required(&event, "styleId")?;
                    let kind = attr_required(&event, "type")?;
                    if !matches!(
                        kind.as_str(),
                        "paragraph" | "character" | "table" | "numbering"
                    ) {
                        return Err(failure(
                            WorkerFailureCode::UnsupportedSemanticConstruct,
                            format!("unsupported Word style type {kind}"),
                        ));
                    }
                    let definition = StyleDefinition {
                        kind,
                        based_on: None,
                        outline_level: None,
                        has_numbering: false,
                        is_default: parse_on_off_attribute(&event, "default")?,
                    };
                    if definitions.insert(style_id.clone(), definition).is_some() {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            format!("duplicate Word style id {style_id}"),
                        ));
                    }
                }
                "outlineLvl"
                    if current.is_some()
                        && paragraph_properties_depth > 0
                        && formatting_revision_depth == 0 =>
                {
                    let level = parse_outline_level(&event)?;
                    set_style_outline_level(&mut current, level)?;
                }
                "numPr"
                    if current.is_some()
                        && paragraph_properties_depth > 0
                        && formatting_revision_depth == 0 =>
                {
                    set_style_has_numbering(&mut current);
                }
                "basedOn"
                    if current.is_some()
                        && paragraph_properties_depth == 0
                        && formatting_revision_depth == 0 =>
                {
                    let based_on = attr_required(&event, "val")?;
                    set_style_base(&mut current, based_on)?;
                }
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "pPr" if current.is_some() && formatting_revision_depth == 0 => {
                    paragraph_properties_depth = paragraph_properties_depth.saturating_sub(1);
                }
                "pPrChange" | "rPrChange" | "sectPrChange" if current.is_some() => {
                    formatting_revision_depth = formatting_revision_depth.saturating_sub(1);
                }
                "style" => {
                    let Some((style_id, definition)) = current.take() else {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            "styles XML closed a missing style definition",
                        ));
                    };
                    if paragraph_properties_depth != 0 || formatting_revision_depth != 0 {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            "styles XML ended a style inside nested paragraph properties",
                        ));
                    }
                    if definitions.insert(style_id.clone(), definition).is_some() {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            format!("duplicate Word style id {style_id}"),
                        ));
                    }
                }
                _ => {}
            },
            Ok(Event::DocType(_)) => {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    "styles XML must not contain a DTD/entity declaration",
                ));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("styles XML parse failed: {error}"),
                ));
            }
        }
    }

    if current.is_some() || paragraph_properties_depth != 0 || formatting_revision_depth != 0 {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "styles XML ended inside a style definition",
        ));
    }

    let default_ids: Vec<_> = definitions
        .iter()
        .filter(|(_, style)| style.kind == "paragraph" && style.is_default)
        .map(|(style_id, _)| style_id.clone())
        .collect();
    if default_ids.len() > 1 {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "styles XML has multiple default paragraph styles",
        ));
    }

    let mut resolved_outlines = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    for (style_id, style) in &definitions {
        if style.kind == "paragraph" {
            resolve_style_outline(
                style_id,
                &definitions,
                &mut visiting,
                &mut resolved_outlines,
            )?;
        }
    }
    let mut resolved_numbering = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    for (style_id, style) in &definitions {
        if style.kind == "paragraph" {
            resolve_style_numbering(
                style_id,
                &definitions,
                &mut visiting,
                &mut resolved_numbering,
            )?;
        }
    }
    let default_outline_level = default_ids
        .first()
        .and_then(|style_id| resolved_outlines.get(style_id).copied().flatten());
    let default_numbering_style = default_ids
        .first()
        .filter(|style_id| resolved_numbering.get(*style_id).copied().unwrap_or(false))
        .cloned();
    let numbered_styles = resolved_numbering
        .into_iter()
        .filter_map(|(style_id, has_numbering)| has_numbering.then_some(style_id))
        .collect();
    Ok(ParagraphStyles {
        outline_levels: resolved_outlines,
        numbered_styles,
        default_outline_level,
        default_numbering_style,
    })
}

fn parse_on_off_attribute(event: &BytesStart<'_>, name: &str) -> Result<bool, WorkerFailure> {
    match attr(event, name)?.as_deref() {
        None | Some("0" | "false" | "off") => Ok(false),
        Some("1" | "true" | "on") => Ok(true),
        Some(value) => Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unsupported Word on/off value {value} for {name}"),
        )),
    }
}

fn parse_on_off_element(event: &BytesStart<'_>, name: &str) -> Result<bool, WorkerFailure> {
    match attr(event, name)?.as_deref() {
        None | Some("1" | "true" | "on") => Ok(true),
        Some("0" | "false" | "off") => Ok(false),
        Some(value) => Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unsupported Word on/off value {value} for {name}"),
        )),
    }
}

fn parse_outline_level(event: &BytesStart<'_>) -> Result<u8, WorkerFailure> {
    let value = attr_required(event, "val")?;
    let level = value.parse::<u8>().map_err(|_| {
        failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("invalid paragraph outline level {value}"),
        )
    })?;
    if level > 9 {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unsupported paragraph outline level {level}"),
        ));
    }
    Ok(level)
}

fn set_style_outline_level(
    current: &mut Option<(String, StyleDefinition)>,
    level: u8,
) -> Result<(), WorkerFailure> {
    let Some((style_id, definition)) = current.as_mut() else {
        return Ok(());
    };
    if definition.outline_level.replace(level).is_some() {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("style {style_id} contains multiple paragraph outline levels"),
        ));
    }
    Ok(())
}

fn set_style_has_numbering(current: &mut Option<(String, StyleDefinition)>) {
    if let Some((_, definition)) = current.as_mut() {
        definition.has_numbering = true;
    }
}

fn set_style_base(
    current: &mut Option<(String, StyleDefinition)>,
    based_on: String,
) -> Result<(), WorkerFailure> {
    let Some((style_id, definition)) = current.as_mut() else {
        return Ok(());
    };
    if definition.based_on.replace(based_on).is_some() {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("style {style_id} contains multiple basedOn references"),
        ));
    }
    Ok(())
}

fn resolve_style_outline(
    style_id: &str,
    definitions: &BTreeMap<String, StyleDefinition>,
    visiting: &mut BTreeSet<String>,
    resolved: &mut BTreeMap<String, Option<u8>>,
) -> Result<Option<u8>, WorkerFailure> {
    if let Some(level) = resolved.get(style_id) {
        return Ok(*level);
    }
    let definition = definitions.get(style_id).ok_or_else(|| {
        failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("paragraph style {style_id} is missing"),
        )
    })?;
    if definition.kind != "paragraph" {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("paragraph style {style_id} is based on a non-paragraph style"),
        ));
    }
    if !visiting.insert(style_id.to_owned()) {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("paragraph style basedOn cycle contains {style_id}"),
        ));
    }
    let inherited = match definition.based_on.as_deref() {
        Some(base_id) => {
            let base = definitions.get(base_id).ok_or_else(|| {
                failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("paragraph style {style_id} is based on missing style {base_id}"),
                )
            })?;
            if base.kind != "paragraph" {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("paragraph style {style_id} is based on non-paragraph style {base_id}"),
                ));
            }
            resolve_style_outline(base_id, definitions, visiting, resolved)?
        }
        None => None,
    };
    visiting.remove(style_id);
    let outline_level = match definition.outline_level {
        Some(9) => None,
        Some(level) => Some(level),
        None => inherited,
    };
    resolved.insert(style_id.to_owned(), outline_level);
    Ok(outline_level)
}

fn resolve_style_numbering(
    style_id: &str,
    definitions: &BTreeMap<String, StyleDefinition>,
    visiting: &mut BTreeSet<String>,
    resolved: &mut BTreeMap<String, bool>,
) -> Result<bool, WorkerFailure> {
    if let Some(has_numbering) = resolved.get(style_id) {
        return Ok(*has_numbering);
    }
    let definition = definitions.get(style_id).ok_or_else(|| {
        failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("paragraph style {style_id} is missing"),
        )
    })?;
    if definition.kind != "paragraph" {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("paragraph style {style_id} is based on a non-paragraph style"),
        ));
    }
    if !visiting.insert(style_id.to_owned()) {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("paragraph style basedOn cycle contains {style_id}"),
        ));
    }
    let inherited = match definition.based_on.as_deref() {
        Some(base_id) => {
            let base = definitions.get(base_id).ok_or_else(|| {
                failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("paragraph style {style_id} is based on missing style {base_id}"),
                )
            })?;
            if base.kind != "paragraph" {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("paragraph style {style_id} is based on non-paragraph style {base_id}"),
                ));
            }
            resolve_style_numbering(base_id, definitions, visiting, resolved)?
        }
        None => false,
    };
    visiting.remove(style_id);
    let has_numbering = definition.has_numbering || inherited;
    resolved.insert(style_id.to_owned(), has_numbering);
    Ok(has_numbering)
}

fn resource_limit() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::InspectionResourceLimitExceeded,
        "DOCX package exceeded a configured resource limit",
    )
}

fn validate_content_types(
    parts: &BTreeMap<String, Vec<u8>>,
    bytes: &[u8],
) -> Result<(), WorkerFailure> {
    const KNOWN: &[&str] = &[
        "application/vnd.openxmlformats-package.relationships+xml",
        "application/xml",
        WORD_MAIN,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml",
        "application/vnd.ms-word.commentsExtended+xml",
        "application/vnd.openxmlformats-package.core-properties+xml",
        "application/vnd.openxmlformats-officedocument.extended-properties+xml",
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/bmp",
        "image/tiff",
        "image/svg+xml",
        "image/emf",
        "image/wmf",
    ];
    validate_content_types_qnames(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "content types are not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut defaults = BTreeMap::new();
    let mut overrides = BTreeMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if matches!(event.local_name().as_ref(), "Default" | "Override") =>
            {
                let content_type = attr_required_unqualified(&event, "ContentType")?;
                if !KNOWN.contains(&content_type.as_str()) {
                    return Err(failure(
                        WorkerFailureCode::UnsupportedSemanticConstruct,
                        format!("unknown OOXML content type {content_type}"),
                    ));
                }
                if event.local_name().as_ref() == "Default" {
                    let extension =
                        attr_required_unqualified(&event, "Extension")?.to_ascii_lowercase();
                    if defaults.insert(extension.clone(), content_type).is_some() {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            format!("duplicate content-type default for {extension}"),
                        ));
                    }
                } else {
                    let part = attr_required_unqualified(&event, "PartName")?;
                    let part = part.strip_prefix('/').ok_or_else(|| {
                        failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            "content-type PartName must be absolute",
                        )
                    })?;
                    validate_part_name(part)?;
                    if overrides.insert(part.to_owned(), content_type).is_some() {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            format!("duplicate content-type override for {part}"),
                        ));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("content-type XML parse failed: {error}"),
                ));
            }
        }
    }
    for (part, content_type) in &overrides {
        if !parts.contains_key(part) {
            return Err(failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("content-type override references missing part {part}"),
            ));
        }
        validate_part_type(part, content_type)?;
    }
    for part in parts
        .keys()
        .filter(|part| part.as_str() != "[Content_Types].xml")
    {
        let content_type = overrides.get(part).or_else(|| {
            let extension = part.rsplit_once('.')?.1;
            defaults.get(&extension.to_ascii_lowercase())
        });
        let Some(content_type) = content_type else {
            return Err(failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("package part has no content type: {part}"),
            ));
        };
        validate_part_type(part, content_type)?;
    }
    Ok(())
}

fn validate_content_types_qnames(bytes: &[u8]) -> Result<(), WorkerFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "content types are not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut namespaces = BTreeMap::new();
    let mut namespace_scopes = Vec::new();
    let mut elements: Vec<(String, String)> = Vec::new();
    let mut saw_root = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                if elements.len() > 1 || (elements.is_empty() && saw_root) {
                    return Err(failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        "content types XML contains an invalid root or nested element",
                    ));
                }
                namespace_scopes.push(namespaces.clone());
                add_namespace_declarations(&event, &mut namespaces)?;
                let qname = resolve_qname(
                    event.name().as_ref(),
                    &namespaces,
                    true,
                    "[Content_Types].xml",
                )?;
                if elements.is_empty() {
                    if qname != (PACKAGE_CONTENT_TYPES_NS.to_owned(), "Types".to_owned()) {
                        return Err(failure(
                            WorkerFailureCode::UnsupportedSemanticConstruct,
                            "[Content_Types].xml root must use the OPC content-types QName",
                        ));
                    }
                    saw_root = true;
                } else {
                    validate_content_type_child_qname(&qname)?;
                }
                elements.push(qname);
            }
            Ok(Event::Empty(event)) => {
                let mut local_namespaces = namespaces.clone();
                add_namespace_declarations(&event, &mut local_namespaces)?;
                let qname = resolve_qname(
                    event.name().as_ref(),
                    &local_namespaces,
                    true,
                    "[Content_Types].xml",
                )?;
                if elements.is_empty() {
                    if saw_root
                        || qname != (PACKAGE_CONTENT_TYPES_NS.to_owned(), "Types".to_owned())
                    {
                        return Err(failure(
                            WorkerFailureCode::UnsupportedSemanticConstruct,
                            "[Content_Types].xml must have one OPC Types root",
                        ));
                    }
                    saw_root = true;
                } else {
                    if elements.len() != 1 {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            "content types XML child elements cannot contain nested elements",
                        ));
                    }
                    validate_content_type_child_qname(&qname)?;
                }
            }
            Ok(Event::End(_)) => {
                elements.pop().ok_or_else(|| {
                    failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        "content types XML has an unmatched closing element",
                    )
                })?;
                namespaces = namespace_scopes.pop().ok_or_else(|| {
                    failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        "content types XML has an unmatched namespace scope",
                    )
                })?;
            }
            Ok(Event::DocType(_)) => {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    "[Content_Types].xml must not contain a DTD/entity declaration",
                ));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("content-type QName parse failed: {error}"),
                ));
            }
        }
    }

    if !saw_root || !elements.is_empty() || !namespace_scopes.is_empty() {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "[Content_Types].xml must contain one complete root",
        ));
    }
    Ok(())
}

fn validate_content_type_child_qname(qname: &(String, String)) -> Result<(), WorkerFailure> {
    if qname.0 == PACKAGE_CONTENT_TYPES_NS && matches!(qname.1.as_str(), "Default" | "Override") {
        Ok(())
    } else {
        Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!(
                "unsupported [Content_Types].xml child QName {{{}}}{}",
                qname.0, qname.1
            ),
        ))
    }
}

fn validate_part_type(part: &str, actual: &str) -> Result<(), WorkerFailure> {
    let expected = match part {
        _ if is_relationships_part(part) => {
            "application/vnd.openxmlformats-package.relationships+xml"
        }
        "word/document.xml" => WORD_MAIN,
        "word/styles.xml" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
        }
        "word/numbering.xml" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"
        }
        "word/footnotes.xml" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"
        }
        "word/endnotes.xml" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"
        }
        "word/comments.xml" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"
        }
        "word/commentsExtended.xml" => "application/vnd.ms-word.commentsExtended+xml",
        "docProps/core.xml" => "application/vnd.openxmlformats-package.core-properties+xml",
        "docProps/app.xml" => {
            "application/vnd.openxmlformats-officedocument.extended-properties+xml"
        }
        _ if is_header_part(part) => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"
        }
        _ if is_footer_part(part) => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"
        }
        _ if part.starts_with("word/media/") => {
            if actual.starts_with("image/") {
                actual
            } else {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("non-image content type for media part {part}: {actual}"),
                ));
            }
        }
        _ => {
            return Err(failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!("unknown potentially semantic OOXML part {part}"),
            ));
        }
    };
    if actual != expected {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("content type {actual} is unsupported for part {part}"),
        ));
    }
    Ok(())
}

fn is_header_part(name: &str) -> bool {
    numbered_word_part(name, "word/header")
}

fn is_footer_part(name: &str) -> bool {
    numbered_word_part(name, "word/footer")
}

fn numbered_word_part(name: &str, prefix: &str) -> bool {
    let Some(number) = name
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(".xml"))
    else {
        return false;
    };
    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_relationships_part(name: &str) -> bool {
    name == "_rels/.rels"
        || name
            .strip_prefix("word/_rels/")
            .is_some_and(|tail| tail.ends_with(".xml.rels"))
}

fn source_part_for_relationships(name: &str) -> Result<String, WorkerFailure> {
    if name == "_rels/.rels" {
        return Ok(String::new());
    }
    let tail = name.strip_prefix("word/_rels/").ok_or_else(|| {
        failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unsupported relationships part {name}"),
        )
    })?;
    let source = tail.strip_suffix(".rels").ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("malformed relationships part path {name}"),
        )
    })?;
    if source.is_empty() || source.contains('/') {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("malformed relationships part path {name}"),
        ));
    }
    Ok(format!("word/{source}"))
}

fn parse_all_relationships(
    bytes: &[u8],
    rels_name: &str,
    source: &str,
) -> Result<Vec<(String, Relationship)>, WorkerFailure> {
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
    const MS: &str = "http://schemas.microsoft.com/office/2011/relationships/";
    let known_root = [
        format!("{R}officeDocument"),
        CORE_PROPERTIES_RELATIONSHIP_TYPE.to_owned(),
        format!("{R}extended-properties"),
    ];
    let known_word = [
        format!("{R}styles"),
        format!("{R}numbering"),
        format!("{R}header"),
        format!("{R}footer"),
        format!("{R}footnotes"),
        format!("{R}endnotes"),
        format!("{R}image"),
        format!("{R}hyperlink"),
        format!("{R}comments"),
        format!("{MS}commentsExtended"),
    ];
    if !source.is_empty() && !source.starts_with("word/") {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unsupported relationship source {source}"),
        ));
    }
    validate_relationship_qnames(rels_name, bytes)?;
    let mut reader = Reader::from_reader(bytes);
    let mut ids = BTreeSet::new();
    let mut result = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Relationship" =>
            {
                let id = attr_required_unqualified(&event, "Id")?;
                let kind = attr_required_unqualified(&event, "Type")?;
                let target = attr_required_unqualified(&event, "Target")?;
                if !ids.insert(id.clone()) {
                    return Err(failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("duplicate relationship id {id} in {rels_name}"),
                    ));
                }
                let external = match attr_unqualified(&event, "TargetMode")?.as_deref() {
                    None | Some("Internal") => false,
                    Some("External") => true,
                    Some(value) => {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            format!("invalid TargetMode {value:?} in {rels_name}"),
                        ));
                    }
                };
                let known = if source.is_empty() {
                    known_root.contains(&kind)
                } else {
                    known_word.contains(&kind)
                };
                if !known {
                    return Err(failure(
                        WorkerFailureCode::UnsupportedSemanticConstruct,
                        format!("unknown relationship type {kind} from {source}"),
                    ));
                }
                if external && !kind.ends_with("/hyperlink") {
                    return Err(failure(
                        WorkerFailureCode::UnsupportedSemanticConstruct,
                        format!("external non-hyperlink relationship {kind}"),
                    ));
                }
                if !external {
                    validate_internal_target(&target)?;
                }
                result.push((
                    id,
                    Relationship {
                        kind,
                        target,
                        external,
                    },
                ));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("{rels_name} parse failed: {error}"),
                ));
            }
        }
    }
    Ok(result)
}

fn validate_relationship_qnames(name: &str, bytes: &[u8]) -> Result<(), WorkerFailure> {
    let mut reader = Reader::from_reader(bytes);
    let mut scopes: Vec<BTreeMap<String, String>> = Vec::new();
    let mut namespaces = BTreeMap::new();
    let mut elements: Vec<(String, String)> = Vec::new();
    let mut saw_root = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                if elements.is_empty() && saw_root {
                    return Err(failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("{name} contains multiple relationship roots"),
                    ));
                }
                scopes.push(namespaces.clone());
                add_namespace_declarations(&event, &mut namespaces)?;
                let qname = resolve_qname(event.name().as_ref(), &namespaces, true, name)?;
                validate_relationship_element(name, &event, &qname, &namespaces, elements.len())?;
                elements.push(qname);
                if elements.len() == 1 {
                    saw_root = true;
                }
            }
            Ok(Event::Empty(event)) => {
                if elements.is_empty() && saw_root {
                    return Err(failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("{name} contains multiple relationship roots"),
                    ));
                }
                let mut local_namespaces = namespaces.clone();
                add_namespace_declarations(&event, &mut local_namespaces)?;
                let qname = resolve_qname(event.name().as_ref(), &local_namespaces, true, name)?;
                validate_relationship_element(
                    name,
                    &event,
                    &qname,
                    &local_namespaces,
                    elements.len(),
                )?;
                if elements.is_empty() {
                    saw_root = true;
                }
            }
            Ok(Event::End(event)) => {
                let actual = resolve_qname(event.name().as_ref(), &namespaces, true, name)?;
                let expected = elements.pop().ok_or_else(|| {
                    failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("{name} has an unmatched closing element"),
                    )
                })?;
                if actual != expected {
                    return Err(failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("{name} has mismatched relationship elements"),
                    ));
                }
                namespaces = scopes.pop().ok_or_else(|| {
                    failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("{name} has unmatched namespace scope"),
                    )
                })?;
            }
            Ok(Event::DocType(_)) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("{name} must not contain a document type declaration"),
                ));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("{name} relationship QName parse failed: {error}"),
                ));
            }
        }
    }

    if !saw_root || !elements.is_empty() || !scopes.is_empty() {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("{name} has an incomplete relationship document"),
        ));
    }
    Ok(())
}

fn validate_relationship_element(
    name: &str,
    event: &BytesStart<'_>,
    qname: &(String, String),
    namespaces: &BTreeMap<String, String>,
    depth: usize,
) -> Result<(), WorkerFailure> {
    let expected_local = if depth == 0 {
        "Relationships"
    } else {
        "Relationship"
    };
    if qname.0 != PACKAGE_RELATIONSHIPS_NS || qname.1 != expected_local || depth > 1 {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!(
                "unsupported relationship QName {{{}}}{} in {name}",
                qname.0, qname.1
            ),
        ));
    }

    let mut local_names = BTreeSet::new();
    for item in event.attributes() {
        let item = item.map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("invalid relationship XML attribute in {name}: {error}"),
            )
        })?;
        let raw = item.key.as_ref();
        if raw == "xmlns" || raw.starts_with("xmlns:") {
            continue;
        }
        let attribute = resolve_qname(raw, namespaces, false, name)?;
        let allowed = depth == 1
            && attribute.0.is_empty()
            && matches!(
                attribute.1.as_str(),
                "Id" | "Type" | "Target" | "TargetMode"
            );
        if !allowed || !local_names.insert(attribute.1.clone()) {
            return Err(failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!(
                    "unsupported relationship attribute QName {{{}}}{} in {name}",
                    attribute.0, attribute.1
                ),
            ));
        }
    }
    Ok(())
}

fn validate_relationship_coverage(parts: &BTreeMap<String, Vec<u8>>) -> Result<(), WorkerFailure> {
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut office_document_count = 0usize;
    for (rels_name, bytes) in parts.iter().filter(|(name, _)| name.ends_with(".rels")) {
        let source = source_part_for_relationships(rels_name)?;
        for (_, relation) in parse_all_relationships(bytes, rels_name, &source)? {
            if source.is_empty() && relation.kind.ends_with("/officeDocument") {
                office_document_count += 1;
                if relation.target != "word/document.xml" {
                    return Err(failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        "officeDocument relationship must target word/document.xml",
                    ));
                }
            }
            if relation.external {
                continue;
            }
            let target = resolve_package_target(&source, &relation.target)?;
            if !parts.contains_key(&target) {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("dangling relationship target {target} from {rels_name}"),
                ));
            }
            graph.entry(source.clone()).or_default().push(target);
        }
    }
    if office_document_count != 1 {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "package must have exactly one officeDocument relationship",
        ));
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in graph.keys() {
        visit_relationship_node(node, &graph, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn resolve_package_target(source: &str, target: &str) -> Result<String, WorkerFailure> {
    validate_internal_target(target)?;
    let base = source.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    Ok(if base.is_empty() {
        target.to_owned()
    } else {
        format!("{base}/{target}")
    })
}

fn validate_internal_target(target: &str) -> Result<(), WorkerFailure> {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains(['\\', '%', '?', '#', ':'])
        || target
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("unsafe or non-canonical relationship target {target:?}"),
        ));
    }
    Ok(())
}

fn visit_relationship_node(
    node: &str,
    graph: &BTreeMap<String, Vec<String>>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> Result<(), WorkerFailure> {
    if visited.contains(node) {
        return Ok(());
    }
    if !visiting.insert(node.to_owned()) {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("relationship cycle detected at {node}"),
        ));
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

fn validate_known_parts_and_qnames(parts: &BTreeMap<String, Vec<u8>>) -> Result<(), WorkerFailure> {
    for (name, bytes) in parts {
        if is_relationships_part(name)
            || matches!(
                name.as_str(),
                "[Content_Types].xml" | "docProps/core.xml" | "docProps/app.xml"
            )
            || name.starts_with("word/media/")
        {
            continue;
        }
        if !(matches!(
            name.as_str(),
            "word/document.xml"
                | "word/styles.xml"
                | "word/numbering.xml"
                | "word/footnotes.xml"
                | "word/endnotes.xml"
                | "word/comments.xml"
                | "word/commentsExtended.xml"
        ) || is_header_part(name)
            || is_footer_part(name))
        {
            return Err(failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!("unknown potentially semantic OOXML part {name}"),
            ));
        }
        validate_word_qnames(name, bytes)?;
    }
    Ok(())
}

fn validate_word_qnames(name: &str, bytes: &[u8]) -> Result<(), WorkerFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("{name} is not UTF-8"),
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut scopes: Vec<BTreeMap<String, String>> = Vec::new();
    let mut namespaces = BTreeMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                scopes.push(namespaces.clone());
                add_namespace_declarations(&event, &mut namespaces)?;
                validate_element_qname(name, &event, &namespaces)?;
            }
            Ok(Event::Empty(event)) => {
                let mut local = namespaces.clone();
                add_namespace_declarations(&event, &mut local)?;
                validate_element_qname(name, &event, &local)?;
            }
            Ok(Event::End(_)) => {
                namespaces = scopes.pop().ok_or_else(|| {
                    failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("{name} has unmatched namespace scope"),
                    )
                })?;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("{name} QName parse failed: {error}"),
                ));
            }
        }
    }
    if !scopes.is_empty() {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("{name} has unclosed namespace scopes"),
        ));
    }
    Ok(())
}

fn add_namespace_declarations(
    event: &BytesStart<'_>,
    namespaces: &mut BTreeMap<String, String>,
) -> Result<(), WorkerFailure> {
    for item in event.attributes() {
        let item = item.map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("invalid XML attribute: {error}"),
            )
        })?;
        let key = item.key.as_ref();
        if key == "xmlns" {
            namespaces.insert(String::new(), decode_xml(item.value.as_ref(), "namespace")?);
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            namespaces.insert(
                prefix.to_owned(),
                decode_xml(item.value.as_ref(), "namespace")?,
            );
        }
    }
    Ok(())
}

fn validate_element_qname(
    name: &str,
    event: &BytesStart<'_>,
    namespaces: &BTreeMap<String, String>,
) -> Result<(), WorkerFailure> {
    let qname = resolve_qname(event.name().as_ref(), namespaces, true, name)?;
    if !known_word_element(&qname.0, &qname.1) {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!(
                "unknown semantic XML QName {{{}}}{} in {name}",
                qname.0, qname.1
            ),
        ));
    }
    let mut local_names = BTreeSet::new();
    for item in event.attributes() {
        let item = item.map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("invalid XML attribute: {error}"),
            )
        })?;
        let key = item.key.as_ref();
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let attribute = resolve_qname(key, namespaces, false, name)?;
        if !known_word_attribute(&qname.0, &qname.1, &attribute.0, &attribute.1) {
            return Err(failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!(
                    "unknown semantic XML attribute QName {{{}}}{} in {name}",
                    attribute.0, attribute.1
                ),
            ));
        }
        if !local_names.insert(attribute.1.clone()) {
            return Err(failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!(
                    "ambiguous duplicate XML attribute local name {} in {name}",
                    attribute.1
                ),
            ));
        }
    }
    Ok(())
}

fn resolve_qname(
    raw: &str,
    namespaces: &BTreeMap<String, String>,
    default_namespace_applies: bool,
    part: &str,
) -> Result<(String, String), WorkerFailure> {
    let mut parts = raw.split(':');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    if parts.next().is_some() || first.is_empty() || second.is_some_and(str::is_empty) {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("malformed XML QName {raw:?} in {part}"),
        ));
    }
    let (prefix, local) = match second {
        Some(local) => (Some(first), local),
        None => (None, first),
    };
    let namespace = match prefix {
        Some("xml") => XML_NS,
        Some(prefix) => namespaces.get(prefix).map(String::as_str).ok_or_else(|| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("unbound XML prefix {prefix} in QName {raw} in {part}"),
            )
        })?,
        None if default_namespace_applies => namespaces.get("").map(String::as_str).unwrap_or(""),
        None => "",
    };
    Ok((namespace.to_owned(), local.to_owned()))
}

fn known_word_element(namespace: &str, local: &str) -> bool {
    match namespace {
        WORD_NS => {
            known_word_element_local(local)
                && !matches!(
                    local,
                    "commentsEx"
                        | "commentEx"
                        | "paraId"
                        | "parentId"
                        | "done"
                        | "inline"
                        | "anchor"
                        | "extent"
                        | "docPr"
                        | "graphic"
                        | "graphicData"
                        | "pic"
                        | "blipFill"
                        | "blip"
                        | "srcRect"
                        | "stretch"
                        | "fillRect"
                        | "spPr"
                        | "xfrm"
                        | "off"
                        | "ext"
                        | "prstGeom"
                        | "noFill"
                        | "solidFill"
                        | "srgbClr"
                        | "schemeClr"
                        | "graphicFrame"
                        | "cNvGraphicFramePr"
                )
        }
        DRAWING_NS => matches!(
            local,
            "graphic"
                | "graphicData"
                | "blipFill"
                | "blip"
                | "srcRect"
                | "stretch"
                | "fillRect"
                | "spPr"
                | "xfrm"
                | "off"
                | "ext"
                | "prstGeom"
                | "avLst"
                | "noFill"
                | "solidFill"
                | "srgbClr"
                | "schemeClr"
                | "graphicFrame"
                | "cNvGraphicFramePr"
        ),
        DRAWING_PICTURE_NS => matches!(
            local,
            "pic" | "nvPicPr" | "cNvPr" | "cNvPicPr" | "blipFill" | "spPr"
        ),
        WORD_DRAWING_NS => matches!(local, "inline" | "anchor" | "extent" | "docPr"),
        WORD_2012_NS => matches!(local, "commentsEx" | "commentEx"),
        _ => false,
    }
}

fn known_word_attribute(
    element_namespace: &str,
    element_local: &str,
    attribute_namespace: &str,
    attribute_local: &str,
) -> bool {
    match element_namespace {
        WORD_NS => match attribute_namespace {
            WORD_NS => known_wordprocessing_attribute(element_local, attribute_local),
            OFFICE_RELATIONSHIPS_NS => {
                attribute_local == "id"
                    && matches!(
                        element_local,
                        "hyperlink" | "headerReference" | "footerReference"
                    )
            }
            WORD_2010_NS => attribute_local == "paraId" && matches!(element_local, "p" | "comment"),
            XML_NS => matches!(
                (element_local, attribute_local),
                ("t" | "delText" | "instrText", "space")
            ),
            _ => false,
        },
        WORD_2012_NS => {
            attribute_namespace == WORD_2012_NS
                && element_local == "commentEx"
                && matches!(attribute_local, "paraId" | "parentId" | "done")
        }
        DRAWING_NS => {
            if attribute_namespace == OFFICE_RELATIONSHIPS_NS {
                return element_local == "blip" && matches!(attribute_local, "embed" | "link");
            }
            attribute_namespace.is_empty()
                && known_drawing_attribute(element_local, attribute_local)
        }
        WORD_DRAWING_NS => {
            attribute_namespace.is_empty()
                && known_word_drawing_attribute(element_local, attribute_local)
        }
        DRAWING_PICTURE_NS => {
            attribute_namespace.is_empty()
                && element_local == "cNvPr"
                && matches!(attribute_local, "id" | "name")
        }
        _ => false,
    }
}

fn known_wordprocessing_attribute(element: &str, local: &str) -> bool {
    if element == "style" && local == "default" {
        return true;
    }
    if element == "pgMar" && matches!(local, "top" | "right" | "bottom" | "left") {
        return true;
    }
    const KNOWN: &[&str] = &[
        "id",
        "val",
        "w",
        "h",
        "orient",
        "type",
        "styleId",
        "numId",
        "ilvl",
        "abstractNumId",
        "ascii",
        "hAnsi",
        "eastAsia",
        "cs",
        "author",
        "date",
        "font",
        "rsidR",
        "rsidRPr",
        "rsidRDefault",
        "rsidP",
        "rsidSect",
        "rsidDel",
        "rsidTr",
        "dirty",
        "fldCharType",
        "color",
        "themeColor",
        "themeTint",
        "themeShade",
        "fill",
    ];
    KNOWN.contains(&local)
}

fn known_drawing_attribute(element: &str, local: &str) -> bool {
    matches!(
        (element, local),
        ("graphicData", "uri")
            | ("ext", "cx" | "cy")
            | ("off", "x" | "y")
            | ("xfrm", "rot")
            | ("prstGeom", "prst")
            | ("srgbClr" | "schemeClr", "val")
    )
}

fn known_word_drawing_attribute(element: &str, local: &str) -> bool {
    matches!(
        (element, local),
        ("extent", "cx" | "cy")
            | ("docPr", "id" | "name" | "descr" | "title")
            | (
                "anchor",
                "distT"
                    | "distB"
                    | "distL"
                    | "distR"
                    | "relativeHeight"
                    | "behindDoc"
                    | "locked"
                    | "layoutInCell"
                    | "allowOverlap"
                    | "simplePos"
                    | "anchorId"
                    | "editId"
            )
            | ("inline", "distT" | "distB" | "distL" | "distR")
    )
}

fn known_word_element_local(local: &str) -> bool {
    const KNOWN: &[&str] = &[
        "document",
        "body",
        "p",
        "pPr",
        "pStyle",
        "r",
        "rPr",
        "t",
        "delText",
        "tbl",
        "tblPr",
        "tblGrid",
        "gridCol",
        "tr",
        "trPr",
        "tc",
        "tcPr",
        "gridSpan",
        "vMerge",
        "hMerge",
        "hyperlink",
        "bookmarkStart",
        "bookmarkEnd",
        "numPr",
        "numId",
        "ilvl",
        "numbering",
        "abstractNum",
        "abstractNumId",
        "num",
        "lvl",
        "numFmt",
        "lvlText",
        "start",
        "lvlRestart",
        "isLgl",
        "styleLink",
        "numStyleLink",
        "multiLevelType",
        "sectPr",
        "pgSz",
        "pgMar",
        "cols",
        "col",
        "titlePg",
        "headerReference",
        "hdr",
        "footerReference",
        "ftr",
        "type",
        "footnoteReference",
        "endnoteReference",
        "footnotes",
        "footnote",
        "endnotes",
        "endnote",
        "separator",
        "continuationSeparator",
        "comments",
        "comment",
        "commentRangeStart",
        "commentRangeEnd",
        "commentReference",
        "commentsEx",
        "commentEx",
        "ins",
        "del",
        "moveFrom",
        "moveTo",
        "rPrChange",
        "pPrChange",
        "styles",
        "style",
        "name",
        "basedOn",
        "next",
        "link",
        "qFormat",
        "jc",
        "outlineLvl",
        "keepNext",
        "keepLines",
        "pageBreakBefore",
        "spacing",
        "ind",
        "tabs",
        "tab",
        "rFonts",
        "sz",
        "szCs",
        "b",
        "i",
        "u",
        "color",
        "highlight",
        "shd",
        "bdr",
        "vanish",
        "webHidden",
        "caps",
        "smallCaps",
        "strike",
        "dstrike",
        "vertAlign",
        "lang",
        "snapToGrid",
        "suppressAutoHyphens",
        "drawing",
        "inline",
        "anchor",
        "extent",
        "docPr",
        "graphic",
        "graphicData",
        "pic",
        "blipFill",
        "blip",
        "srcRect",
        "stretch",
        "fillRect",
        "spPr",
        "xfrm",
        "off",
        "ext",
        "prstGeom",
        "noFill",
        "solidFill",
        "srgbClr",
        "schemeClr",
        "graphicFrame",
        "cNvGraphicFramePr",
        "customXml",
        "paraId",
        "parentId",
        "done",
        "tblCaption",
        "tblDescription",
        "tblLook",
        "tblStyle",
        "tblW",
        "tblInd",
        "tblLayout",
        "tblCellMar",
        "tblCellSpacing",
        "cantSplit",
        "trHeight",
        "bidi",
        "rtlGutter",
        "docGrid",
        "paperSrc",
        "pBdr",
        "rBdr",
        "top",
        "left",
        "bottom",
        "right",
        "insideH",
        "insideV",
        "pageBorders",
        "pgBorders",
        "framePr",
        "sectPrChange",
        "lastRenderedPageBreak",
        "softHyphen",
        "noBreakHyphen",
        "br",
        "cr",
        "sym",
        "fldSimple",
        "fldChar",
        "instrText",
        "proofErr",
        "permStart",
        "permEnd",
        "delInstrText",
        "altName",
        "compat",
        "compatSetting",
    ];
    KNOWN.contains(&local)
}

fn attr_required(event: &BytesStart<'_>, name: &str) -> Result<String, WorkerFailure> {
    attr(event, name)?.ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("missing required XML attribute {name}"),
        )
    })
}

fn attr_required_unqualified(event: &BytesStart<'_>, name: &str) -> Result<String, WorkerFailure> {
    attr_unqualified(event, name)?.ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("missing required unqualified XML attribute {name}"),
        )
    })
}

fn attr(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, WorkerFailure> {
    let mut matched = None;
    for item in event.attributes() {
        let item = item.map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("invalid XML attribute: {error}"),
            )
        })?;
        let key = item.key.as_ref();
        let local = key.rsplit_once(':').map_or(key, |(_, local)| local);
        if local == name {
            if matched.is_some() {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("ambiguous XML attribute local name {name}"),
                ));
            }
            matched = Some(decode_xml(item.value.as_ref(), "XML attribute")?);
        }
    }
    Ok(matched)
}

fn attr_unqualified(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, WorkerFailure> {
    for item in event.attributes() {
        let item = item.map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("invalid XML attribute: {error}"),
            )
        })?;
        if item.key.as_ref() == name {
            return Ok(Some(decode_xml(item.value.as_ref(), "XML attribute")?));
        }
    }
    Ok(None)
}

fn decode_xml(text: &str, context: &str) -> Result<String, WorkerFailure> {
    quick_xml::escape::unescape(text)
        .map(|value| value.into_owned())
        .map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("{context} entity decode failed: {error}"),
            )
        })
}

fn parse_numbering(data: &[u8]) -> Result<BTreeMap<(u32, u8), NumberingLevel>, WorkerFailure> {
    if data.is_empty() {
        return Ok(BTreeMap::new());
    }
    let text = std::str::from_utf8(data).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "numbering XML is not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut formats = BTreeMap::new();
    let mut starts = BTreeMap::new();
    let mut level_texts = BTreeMap::new();
    let mut abstract_types = BTreeMap::new();
    let mut instances = BTreeMap::new();
    let mut abstract_ids = BTreeSet::new();
    let mut level_ids = BTreeSet::new();
    let mut number_ids = BTreeSet::new();
    let mut abstract_id: Option<u32> = None;
    let mut level: Option<u8> = None;
    let mut num_id: Option<u32> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "abstractNum" => {
                    if abstract_id.is_some() || num_id.is_some() {
                        return Err(numbering_structure_failure(
                            "nested or overlapping numbering definitions",
                        ));
                    }
                    let id = numbering_u32(&event, "abstractNumId")?;
                    if !abstract_ids.insert(id) {
                        return Err(numbering_structure_failure(
                            "duplicate abstract numbering identifier",
                        ));
                    }
                    abstract_id = Some(id);
                }
                "lvl" => {
                    if level.is_some() || num_id.is_some() {
                        return Err(numbering_structure_failure(
                            "numbering level is outside an abstract definition",
                        ));
                    }
                    if abstract_id.is_none() {
                        return Err(numbering_structure_failure(
                            "numbering level is outside an abstract definition",
                        ));
                    }
                    let abstract_number_id = active_abstract_id(abstract_id)?;
                    let level_index = numbering_level_index(&event)?;
                    if !level_ids.insert((abstract_number_id, level_index)) {
                        return Err(numbering_structure_failure(
                            "duplicate abstract numbering level",
                        ));
                    }
                    level = Some(level_index);
                }
                "num" => {
                    if abstract_id.is_some() || num_id.is_some() {
                        return Err(numbering_structure_failure(
                            "nested or overlapping numbering definitions",
                        ));
                    }
                    let id = numbering_u32(&event, "numId")?;
                    if !number_ids.insert(id) {
                        return Err(numbering_structure_failure(
                            "duplicate numbering instance identifier",
                        ));
                    }
                    num_id = Some(id);
                }
                "numFmt" => {
                    let key = active_numbering_level(abstract_id, level)?;
                    let value = attr_required(&event, "val")?;
                    if value.is_empty() || formats.insert(key, value).is_some() {
                        return Err(numbering_structure_failure(
                            "empty or duplicate numbering format",
                        ));
                    }
                }
                "start" => {
                    let key = active_numbering_level(abstract_id, level)?;
                    let value = numbering_u32(&event, "val")?;
                    if starts.insert(key, value).is_some() {
                        return Err(numbering_structure_failure(
                            "duplicate numbering start value",
                        ));
                    }
                }
                "lvlText" => {
                    let key = active_numbering_level(abstract_id, level)?;
                    let value = attr_required(&event, "val")?;
                    if level_texts.insert(key, value).is_some() {
                        return Err(numbering_structure_failure(
                            "duplicate numbering level text",
                        ));
                    }
                }
                "multiLevelType" => {
                    let id = active_abstract_id(abstract_id)?;
                    record_multi_level_type(&event, id, &mut abstract_types)?;
                }
                "lvlRestart" | "isLgl" | "styleLink" | "numStyleLink" => {
                    return Err(unsupported_numbering_control());
                }
                "abstractNumId" => {
                    let num = num_id.ok_or_else(|| {
                        numbering_structure_failure(
                            "numbering instance reference is outside a numbering instance",
                        )
                    })?;
                    let abstract_num = numbering_u32(&event, "val")?;
                    if instances.insert(num, abstract_num).is_some() {
                        return Err(numbering_structure_failure(
                            "duplicate abstract numbering reference",
                        ));
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(event)) => match event.local_name().as_ref() {
                "numFmt" => {
                    let key = active_numbering_level(abstract_id, level)?;
                    let value = attr_required(&event, "val")?;
                    if value.is_empty() || formats.insert(key, value).is_some() {
                        return Err(numbering_structure_failure(
                            "empty or duplicate numbering format",
                        ));
                    }
                }
                "start" => {
                    let key = active_numbering_level(abstract_id, level)?;
                    let value = numbering_u32(&event, "val")?;
                    if starts.insert(key, value).is_some() {
                        return Err(numbering_structure_failure(
                            "duplicate numbering start value",
                        ));
                    }
                }
                "lvlText" => {
                    let key = active_numbering_level(abstract_id, level)?;
                    let value = attr_required(&event, "val")?;
                    if level_texts.insert(key, value).is_some() {
                        return Err(numbering_structure_failure(
                            "duplicate numbering level text",
                        ));
                    }
                }
                "multiLevelType" => {
                    let id = active_abstract_id(abstract_id)?;
                    record_multi_level_type(&event, id, &mut abstract_types)?;
                }
                "lvlRestart" | "isLgl" | "styleLink" | "numStyleLink" => {
                    return Err(unsupported_numbering_control());
                }
                "abstractNumId" => {
                    let num = num_id.ok_or_else(|| {
                        numbering_structure_failure(
                            "numbering instance reference is outside a numbering instance",
                        )
                    })?;
                    let abstract_num = numbering_u32(&event, "val")?;
                    if instances.insert(num, abstract_num).is_some() {
                        return Err(numbering_structure_failure(
                            "duplicate abstract numbering reference",
                        ));
                    }
                }
                "abstractNum" => {
                    if abstract_id.is_some() || num_id.is_some() {
                        return Err(numbering_structure_failure(
                            "nested or overlapping numbering definitions",
                        ));
                    }
                    let id = numbering_u32(&event, "abstractNumId")?;
                    if !abstract_ids.insert(id) {
                        return Err(numbering_structure_failure(
                            "duplicate abstract numbering identifier",
                        ));
                    }
                }
                "num" => {
                    return Err(numbering_structure_failure(
                        "empty numbering instance has no abstract definition reference",
                    ));
                }
                "lvl" => {
                    let abstract_number_id = active_abstract_id(abstract_id)?;
                    let level = numbering_level_index(&event)?;
                    if !level_ids.insert((abstract_number_id, level)) {
                        return Err(numbering_structure_failure(
                            "duplicate abstract numbering level",
                        ));
                    }
                }
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "lvl" => {
                    if level.take().is_none() {
                        return Err(numbering_structure_failure(
                            "numbering level closed without an opening element",
                        ));
                    }
                }
                "abstractNum" => {
                    if abstract_id.take().is_none() || level.is_some() {
                        return Err(numbering_structure_failure(
                            "abstract numbering definition closed inconsistently",
                        ));
                    }
                }
                "num" => {
                    let number = num_id.take().ok_or_else(|| {
                        numbering_structure_failure(
                            "numbering instance closed without an opening element",
                        )
                    })?;
                    if !instances.contains_key(&number) {
                        return Err(numbering_structure_failure(
                            "numbering instance has no abstract definition reference",
                        ));
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("numbering parse failed: {error}"),
                ));
            }
        }
    }
    if abstract_id.is_some() || level.is_some() || num_id.is_some() {
        return Err(numbering_structure_failure(
            "numbering XML ended inside a definition",
        ));
    }
    let mut resolved = BTreeMap::new();
    for (num, abstract_id) in instances {
        for ((candidate, level), format) in &formats {
            if *candidate == abstract_id {
                resolved.insert(
                    (num, *level),
                    NumberingLevel {
                        format: format.clone(),
                        start: starts.get(&(*candidate, *level)).copied().unwrap_or(0),
                        level_text: level_texts.get(&(*candidate, *level)).cloned(),
                        multi_level_type: abstract_types.get(&abstract_id).cloned(),
                    },
                );
            }
        }
    }
    Ok(resolved)
}

fn unsupported_numbering_control() -> WorkerFailure {
    failure(
        WorkerFailureCode::UnsupportedSemanticConstruct,
        "numbering restart, legal display, or linked style is not projected",
    )
}

fn record_multi_level_type(
    event: &BytesStart<'_>,
    abstract_id: u32,
    types: &mut BTreeMap<u32, String>,
) -> Result<(), WorkerFailure> {
    let value = attr_required(event, "val")?;
    if !matches!(
        value.as_str(),
        "singleLevel" | "multilevel" | "hybridMultilevel"
    ) {
        return Err(unsupported_numbering_control());
    }
    if types.insert(abstract_id, value).is_some() {
        return Err(numbering_structure_failure(
            "duplicate numbering level type",
        ));
    }
    Ok(())
}

fn active_abstract_id(abstract_id: Option<u32>) -> Result<u32, WorkerFailure> {
    abstract_id.ok_or_else(|| {
        numbering_structure_failure("numbering level property is outside an abstract definition")
    })
}

fn active_numbering_level(
    abstract_id: Option<u32>,
    level: Option<u8>,
) -> Result<(u32, u8), WorkerFailure> {
    Ok((
        active_abstract_id(abstract_id)?,
        level.ok_or_else(|| {
            numbering_structure_failure("numbering level property is outside a level")
        })?,
    ))
}

fn numbering_u32(event: &BytesStart<'_>, name: &str) -> Result<u32, WorkerFailure> {
    let value = attr_required(event, name)?;
    value.parse::<u32>().map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("invalid numbering {name} value {value:?}"),
        )
    })
}

fn numbering_level_index(event: &BytesStart<'_>) -> Result<u8, WorkerFailure> {
    let value = attr_required(event, "ilvl")?;
    let level = value.parse::<u8>().map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("invalid numbering level index {value:?}"),
        )
    })?;
    if level > 8 {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unsupported numbering level index {level}"),
        ));
    }
    Ok(level)
}

fn numbering_level_reference(event: &BytesStart<'_>) -> Result<u8, WorkerFailure> {
    let Some(value) = attr(event, "val")? else {
        return Ok(0);
    };
    let level = value.parse::<u8>().map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("invalid numbering level reference {value:?}"),
        )
    })?;
    if level > 8 {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unsupported numbering level reference {level}"),
        ));
    }
    Ok(level)
}

fn numbering_structure_failure(message: &str) -> WorkerFailure {
    failure(WorkerFailureCode::SemanticExtractionFailed, message)
}

fn parse_document_projection(
    bytes: &[u8],
    source_part: &str,
    context: DocumentProjectionContext<'_>,
) -> Result<(Vec<String>, String), WorkerFailure> {
    let DocumentProjectionContext {
        package,
        numbering,
        numbering_instances,
        footnotes,
        endnotes,
        image_budget,
    } = context;
    let text = std::str::from_utf8(bytes).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "document XML is not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut tokens = Vec::new();
    let mut visible = Vec::new();
    let mut paragraph_text = String::new();
    let styles = &package.styles;
    let mut in_text = false;
    let mut in_instruction_text = false;
    let mut deleted_depth = 0usize;
    let mut formatting_revision_depth = 0usize;
    let mut level = 0u8;
    let mut complex_fields = Vec::new();
    let mut section_active = false;
    let mut section_type_seen = false;
    let mut paragraph_properties_depth = 0usize;
    let mut paragraph_outline_level = None;
    let mut drawing_depth = 0usize;
    let mut picture_depth = 0usize;
    let mut pictures = Vec::<PictureProjection>::new();
    let mut section_index = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let local = event.local_name();
                match local.as_ref() {
                    "pPrChange" | "rPrChange" | "sectPrChange" => {
                        formatting_revision_depth += 1;
                    }
                    "del" | "moveFrom" => deleted_depth += 1,
                    _ if deleted_depth > 0 || formatting_revision_depth > 0 => {}
                    "fldChar" => {
                        project_complex_field_char(&event, &mut complex_fields, &mut tokens)?;
                    }
                    "instrText" => {
                        if in_instruction_text
                            || complex_fields.last().is_none_or(|field| field.separated)
                        {
                            return Err(unsupported_field(
                                "field instruction text is outside an instruction phase",
                            ));
                        }
                        in_instruction_text = true;
                    }
                    "sectPr" => {
                        begin_section(&mut section_active)?;
                        section_index = section_index.saturating_add(1);
                        if paragraph_properties_depth > 0 {
                            tokens.push("section-boundary".into());
                        }
                    }
                    "titlePg" if section_active => {
                        if parse_on_off_element(&event, "val")? {
                            tokens.push("title-page-header:on".into());
                        }
                    }
                    "pPr" => paragraph_properties_depth += 1,
                    "type" if section_active => {
                        project_section_type(&event, &mut section_type_seen, &mut tokens)?;
                    }
                    "p" => {
                        reject_default_style_numbering(styles)?;
                        tokens.push("p+".into());
                        paragraph_text.clear();
                        level = 0;
                        paragraph_outline_level = styles.default_outline_level;
                    }
                    "tbl" => tokens.push("table+".into()),
                    "tr" => tokens.push("row+".into()),
                    "tc" => tokens.push("cell+".into()),
                    "hyperlink" => {
                        if let Some(id) = attr(&event, "id")? {
                            push_hyperlink(&id, source_part, package, &mut tokens)?;
                        }
                    }
                    "t" => in_text = true,
                    "pStyle" => {
                        push_style(&event, styles, &mut paragraph_outline_level, &mut tokens)?
                    }
                    "outlineLvl" if paragraph_properties_depth > 0 => {
                        let level = parse_outline_level(&event)?;
                        paragraph_outline_level = (level < 9).then_some(level);
                    }
                    "ilvl" => level = numbering_level_reference(&event)?,
                    "numId" => {
                        push_numbering(&event, level, numbering, numbering_instances, &mut tokens)?
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
                    "hMerge" => push_horizontal_merge(&event, &mut tokens)?,
                    "docPr" => push_image_alt_text(&event, &mut tokens)?,
                    "blip" => {
                        push_image(&event, source_part, package, image_budget, &mut tokens)?;
                        if let Some(picture) = pictures.last_mut() {
                            picture.referenced_image = true;
                        }
                    }
                    "drawing" => {
                        drawing_depth += 1;
                        tokens.push("drawing+".into());
                    }
                    "pic" => {
                        picture_depth += 1;
                        pictures.push(PictureProjection::default());
                        tokens.push("picture+".into());
                    }
                    "extent" if drawing_depth > 0 => {
                        push_image_extent(&event, "frame", &mut tokens)?;
                    }
                    "ext" if picture_depth > 0 => {
                        push_image_extent(&event, "shape", &mut tokens)?;
                    }
                    "off" if picture_depth > 0 => {
                        push_image_offset(&event, &mut tokens)?;
                    }
                    "xfrm" if picture_depth > 0 => {
                        push_image_rotation(&event, &mut tokens)?;
                    }
                    "prstGeom" if picture_depth > 0 => {
                        record_picture_geometry(&event, &mut pictures)?;
                    }
                    "pgSz" => push_page_size(&event, &mut tokens)?,
                    "footnoteReference" => {
                        push_note_reference(&event, "footnote", footnotes, &mut tokens)?;
                    }
                    "endnoteReference" => {
                        push_note_reference(&event, "endnote", endnotes, &mut tokens)?;
                    }
                    "headerReference" if source_part == "word/document.xml" => {
                        push_section_reference(
                            &event,
                            SectionReferenceKey {
                                kind: "header",
                                section_index,
                            },
                            DocumentProjectionContext {
                                package,
                                numbering,
                                numbering_instances,
                                footnotes,
                                endnotes,
                                image_budget,
                            },
                            &mut tokens,
                        )?;
                    }
                    "footerReference" if source_part == "word/document.xml" => {
                        push_section_reference(
                            &event,
                            SectionReferenceKey {
                                kind: "footer",
                                section_index,
                            },
                            DocumentProjectionContext {
                                package,
                                numbering,
                                numbering_instances,
                                footnotes,
                                endnotes,
                                image_budget,
                            },
                            &mut tokens,
                        )?;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(event)) if deleted_depth == 0 && formatting_revision_depth == 0 => {
                match event.local_name().as_ref() {
                    "p" => {
                        reject_default_style_numbering(styles)?;
                        tokens.push("p+".into());
                        if let Some(outline_level) = styles.default_outline_level {
                            tokens.push(format!("outline-level:{outline_level}"));
                        }
                        tokens.push("p-".into());
                        visible.push(String::new());
                        paragraph_text.clear();
                        paragraph_outline_level = None;
                        level = 0;
                    }
                    "fldChar" => {
                        project_complex_field_char(&event, &mut complex_fields, &mut tokens)?;
                    }
                    "sectPr" => {
                        project_empty_section(section_active)?;
                        section_index = section_index.saturating_add(1);
                        if paragraph_properties_depth > 0 {
                            tokens.push("section-boundary".into());
                        }
                    }
                    "titlePg" if section_active => {
                        if parse_on_off_element(&event, "val")? {
                            tokens.push("title-page-header:on".into());
                        }
                    }
                    "type" if section_active => {
                        project_section_type(&event, &mut section_type_seen, &mut tokens)?;
                    }
                    "pStyle" => {
                        push_style(&event, styles, &mut paragraph_outline_level, &mut tokens)?
                    }
                    "outlineLvl" if paragraph_properties_depth > 0 => {
                        let level = parse_outline_level(&event)?;
                        paragraph_outline_level = (level < 9).then_some(level);
                    }
                    "ilvl" => level = numbering_level_reference(&event)?,
                    "numId" => {
                        push_numbering(&event, level, numbering, numbering_instances, &mut tokens)?
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
                    "hMerge" => push_horizontal_merge(&event, &mut tokens)?,
                    "docPr" => push_image_alt_text(&event, &mut tokens)?,
                    "blip" => {
                        push_image(&event, source_part, package, image_budget, &mut tokens)?;
                        if let Some(picture) = pictures.last_mut() {
                            picture.referenced_image = true;
                        }
                    }
                    "extent" if drawing_depth > 0 => {
                        push_image_extent(&event, "frame", &mut tokens)?;
                    }
                    "ext" if picture_depth > 0 => {
                        push_image_extent(&event, "shape", &mut tokens)?;
                    }
                    "off" if picture_depth > 0 => {
                        push_image_offset(&event, &mut tokens)?;
                    }
                    "xfrm" if picture_depth > 0 => {
                        push_image_rotation(&event, &mut tokens)?;
                    }
                    "prstGeom" if picture_depth > 0 => {
                        record_picture_geometry(&event, &mut pictures)?;
                    }
                    "pgSz" => push_page_size(&event, &mut tokens)?,
                    "footnoteReference" => {
                        push_note_reference(&event, "footnote", footnotes, &mut tokens)?;
                    }
                    "endnoteReference" => {
                        push_note_reference(&event, "endnote", endnotes, &mut tokens)?;
                    }
                    "headerReference" if source_part == "word/document.xml" => {
                        push_section_reference(
                            &event,
                            SectionReferenceKey {
                                kind: "header",
                                section_index,
                            },
                            DocumentProjectionContext {
                                package,
                                numbering,
                                numbering_instances,
                                footnotes,
                                endnotes,
                                image_budget,
                            },
                            &mut tokens,
                        )?;
                    }
                    "footerReference" if source_part == "word/document.xml" => {
                        push_section_reference(
                            &event,
                            SectionReferenceKey {
                                kind: "footer",
                                section_index,
                            },
                            DocumentProjectionContext {
                                package,
                                numbering,
                                numbering_instances,
                                footnotes,
                                endnotes,
                                image_budget,
                            },
                            &mut tokens,
                        )?;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(value))
                if in_instruction_text && deleted_depth == 0 && formatting_revision_depth == 0 =>
            {
                let field = complex_fields
                    .last_mut()
                    .ok_or_else(|| unsupported_field("field instruction text has no open field"))?;
                if field.separated {
                    return Err(unsupported_field(
                        "field instruction text follows its result separator",
                    ));
                }
                field
                    .instruction
                    .push_str(&decode_xml(value.as_ref(), "DOCX field instruction")?);
            }
            Ok(Event::Text(value))
                if in_text && deleted_depth == 0 && formatting_revision_depth == 0 =>
            {
                let value = decode_xml(value.as_ref(), "document text")?;
                push_text_fragment(&mut tokens, &mut paragraph_text, &value);
            }
            Ok(Event::End(event)) => {
                let local = event.local_name();
                match local.as_ref() {
                    "pPrChange" | "rPrChange" | "sectPrChange" => {
                        formatting_revision_depth = formatting_revision_depth.saturating_sub(1);
                    }
                    "del" | "moveFrom" => {
                        deleted_depth = deleted_depth.saturating_sub(1);
                    }
                    _ if deleted_depth > 0 || formatting_revision_depth > 0 => {}
                    "instrText" => in_instruction_text = false,
                    "sectPr" => end_section(&mut section_active, &mut section_type_seen)?,
                    "pPr" => {
                        paragraph_properties_depth = paragraph_properties_depth.saturating_sub(1);
                    }
                    "t" => in_text = false,
                    "p" => {
                        if let Some(outline_level) = paragraph_outline_level.take() {
                            tokens.push(format!("outline-level:{outline_level}"));
                        }
                        tokens.push("p-".into());
                        visible.push(std::mem::take(&mut paragraph_text));
                    }
                    "pic" => {
                        picture_depth = picture_depth.saturating_sub(1);
                        if let Some(picture) = pictures.pop() {
                            reject_unsupported_referenced_picture_geometry(picture)?;
                        }
                        tokens.push("picture-".into());
                    }
                    "drawing" => {
                        drawing_depth = drawing_depth.saturating_sub(1);
                        tokens.push("drawing-".into());
                    }
                    "tbl" => tokens.push("table-".into()),
                    "tr" => tokens.push("row-".into()),
                    "tc" => tokens.push("cell-".into()),
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("document projection parse failed: {error}"),
                ));
            }
        }
    }
    if in_instruction_text || !complex_fields.is_empty() {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "DOCX ended inside a complex field",
        ));
    }
    if section_active {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "DOCX ended inside section properties",
        ));
    }
    Ok((tokens, normalize_text(&visible.join(" "))))
}

fn project_complex_field_char(
    event: &BytesStart<'_>,
    fields: &mut Vec<ComplexField>,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    match attr_required(event, "fldCharType")?.as_str() {
        "begin" => fields.push(ComplexField::default()),
        "separate" => {
            let field = fields
                .last_mut()
                .ok_or_else(|| unsupported_field("field result separator has no open field"))?;
            if field.separated {
                return Err(unsupported_field(
                    "complex field has multiple result separators",
                ));
            }
            tokens.extend(project_field_instruction(&field.instruction)?);
            field.separated = true;
        }
        "end" => {
            let field = fields
                .pop()
                .ok_or_else(|| unsupported_field("field end has no open field"))?;
            if !field.separated {
                return Err(unsupported_field("complex field has no result separator"));
            }
        }
        kind => {
            return Err(unsupported_field(format!(
                "unsupported complex field marker {kind}"
            )));
        }
    }
    Ok(())
}

fn project_field_instruction(instruction: &str) -> Result<Vec<String>, WorkerFailure> {
    let words = tokenize_field_instruction(instruction)?;
    if !words
        .first()
        .is_some_and(|command| command.eq_ignore_ascii_case("HYPERLINK"))
    {
        return Err(unsupported_field(
            "complex field instruction is not a supported HYPERLINK",
        ));
    }

    let mut target = None;
    let mut anchor = None;
    let mut index = 1;
    while index < words.len() {
        let word = &words[index];
        if word.eq_ignore_ascii_case("\\l") {
            index += 1;
            let Some(value) = words.get(index) else {
                return Err(unsupported_field(
                    "HYPERLINK local-anchor switch has no value",
                ));
            };
            if value.is_empty() || anchor.replace(value.clone()).is_some() {
                return Err(unsupported_field(
                    "HYPERLINK has an empty or duplicate local anchor",
                ));
            }
        } else if word.starts_with('\\') {
            return Err(unsupported_field(format!(
                "unsupported HYPERLINK switch {word}"
            )));
        } else if word.is_empty() || target.replace(word.clone()).is_some() {
            return Err(unsupported_field(
                "HYPERLINK has an empty or ambiguous target",
            ));
        }
        index += 1;
    }

    if target.is_none() && anchor.is_none() {
        return Err(unsupported_field("HYPERLINK has no destination"));
    }
    let mut tokens = Vec::new();
    if let Some(target) = target {
        tokens.push(format!("field-hyperlink-target:{target}"));
    }
    if let Some(anchor) = anchor {
        tokens.push(format!("field-hyperlink-anchor:{anchor}"));
    }
    Ok(tokens)
}

fn tokenize_field_instruction(instruction: &str) -> Result<Vec<String>, WorkerFailure> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut characters = instruction.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' => quoted = !quoted,
            '\\' if quoted && characters.peek() == Some(&'"') => {
                word.push(characters.next().expect("peeked quote"));
            }
            character if character.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(character),
        }
    }
    if quoted {
        return Err(unsupported_field(
            "HYPERLINK instruction contains an unclosed quoted value",
        ));
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

fn unsupported_field(message: impl Into<String>) -> WorkerFailure {
    failure(
        WorkerFailureCode::UnsupportedSemanticConstruct,
        message.into(),
    )
}

fn begin_section(active: &mut bool) -> Result<(), WorkerFailure> {
    if *active {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "nested section properties",
        ));
    }
    *active = true;
    Ok(())
}

fn project_empty_section(active: bool) -> Result<(), WorkerFailure> {
    if active {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "nested section properties",
        ));
    }
    Ok(())
}

fn project_section_type(
    event: &BytesStart<'_>,
    type_seen: &mut bool,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    if *type_seen {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "section properties contain multiple section types",
        ));
    }
    let section_type = attr(event, "val")?.unwrap_or_else(|| "nextPage".into());
    if !matches!(
        section_type.as_str(),
        "nextPage" | "nextColumn" | "continuous" | "evenPage" | "oddPage"
    ) {
        return Err(unsupported_field(format!(
            "unsupported section break type {section_type}"
        )));
    }
    if section_type != "nextPage" {
        tokens.push(format!("section-type:{section_type}"));
    }
    *type_seen = true;
    Ok(())
}

fn end_section(active: &mut bool, type_seen: &mut bool) -> Result<(), WorkerFailure> {
    if !*active {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "section properties closed without an opening element",
        ));
    }
    *active = false;
    *type_seen = false;
    Ok(())
}

fn push_horizontal_merge(
    event: &BytesStart<'_>,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let merge = attr(event, "val")?.unwrap_or_else(|| "continue".into());
    if !matches!(merge.as_str(), "restart" | "continue") {
        return Err(unsupported_field(format!(
            "unsupported horizontal-merge value {merge}"
        )));
    }
    tokens.push(format!("hmerge:{merge}"));
    Ok(())
}

fn push_text_fragment(tokens: &mut Vec<String>, paragraph_text: &mut String, value: &str) {
    if let Some(last) = tokens.last_mut()
        && let Some(last_text) = last.strip_prefix("text:")
    {
        let mut merged = String::with_capacity(last_text.len() + value.len() + 5);
        merged.push_str("text:");
        merged.push_str(last_text);
        merged.push_str(value);
        *last = merged;
    } else {
        tokens.push(format!("text:{value}"));
    }
    paragraph_text.push_str(value);
}

fn push_hyperlink(
    id: &str,
    source_part: &str,
    package: &PackageInspection,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let rel = package
        .relationships_by_source
        .get(source_part)
        .and_then(|relationships| relationships.get(id))
        .ok_or_else(|| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("hyperlink relationship {id} is missing from {source_part}"),
            )
        })?;
    if rel.kind.ends_with("/hyperlink") {
        tokens.push(format!("link:{}", rel.target));
        Ok(())
    } else {
        Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("relationship {id} from {source_part} is not a hyperlink"),
        ))
    }
}

fn push_note_reference(
    event: &BytesStart<'_>,
    kind: &str,
    entries: &BTreeMap<String, NoteEntry>,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let id = attr_required(event, "id")?;
    let entry = entries.get(&id).ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("{kind} reference {id} has no corresponding note"),
        )
    })?;
    if let Some(construct) = &entry.unsupported_construct {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("referenced {kind} {id} contains unsupported construct {construct}"),
        ));
    }
    tokens.push(format!("{kind}-ref:{}", entry.content));
    Ok(())
}

fn push_section_reference(
    event: &BytesStart<'_>,
    key: SectionReferenceKey<'_>,
    context: DocumentProjectionContext<'_>,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let kind = key.kind;
    let id = attr_required(event, "id")?;
    let reference_type = validated_reference_type(event, kind)?;
    if !context.package.selected_section_references.contains(&(
        key.section_index,
        kind.to_owned(),
        id.clone(),
    )) {
        return Ok(());
    }
    let rel = context
        .package
        .relationships_by_source
        .get("word/document.xml")
        .and_then(|relationships| relationships.get(&id))
        .ok_or_else(|| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("{kind} relationship {id} is missing from word/document.xml"),
            )
        })?;
    if !rel.kind.ends_with(&format!("/{kind}")) || rel.external {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("relationship {id} is not an internal {kind}"),
        ));
    }
    let path = resolve_word_target(&rel.target)?;
    let bytes = context.package.parts.get(&path).ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("{kind} target {path} is missing"),
        )
    })?;
    let (part_tokens, _) = parse_document_projection(bytes, &path, context)?;
    tokens.push(format!("section-{kind}:{reference_type}+"));
    tokens.extend(
        part_tokens
            .into_iter()
            .map(|token| format!("{kind}-{token}")),
    );
    tokens.push(format!("section-{kind}-"));
    Ok(())
}

fn push_style(
    event: &BytesStart<'_>,
    styles: &ParagraphStyles,
    outline_level: &mut Option<u8>,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let style = attr_required(event, "val")?;
    if styles.numbered_styles.contains(&style) {
        return Err(unsupported_style_numbering(&style));
    }
    *outline_level = match styles.outline_levels.get(&style) {
        Some(level) => *level,
        None => builtin_paragraph_outline_level(&style).ok_or_else(|| {
            failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!("paragraph references missing style {style}"),
            )
        })?,
    };
    if style != "Normal" {
        tokens.push(format!("style:{style}"));
    }
    Ok(())
}

fn reject_default_style_numbering(styles: &ParagraphStyles) -> Result<(), WorkerFailure> {
    if let Some(style_id) = styles.default_numbering_style.as_deref() {
        return Err(unsupported_style_numbering(style_id));
    }
    Ok(())
}

fn unsupported_style_numbering(style_id: &str) -> WorkerFailure {
    failure(
        WorkerFailureCode::UnsupportedSemanticConstruct,
        format!("paragraph style {style_id} defines numbering that is unsupported in v0"),
    )
}

fn builtin_paragraph_outline_level(style: &str) -> Option<Option<u8>> {
    match style {
        "Heading1" => Some(Some(0)),
        "Heading2" => Some(Some(1)),
        "Heading3" => Some(Some(2)),
        "Heading4" => Some(Some(3)),
        "Heading5" => Some(Some(4)),
        "Heading6" => Some(Some(5)),
        "Heading7" => Some(Some(6)),
        "Heading8" => Some(Some(7)),
        "Heading9" => Some(Some(8)),
        "Normal" | "Title" | "Subtitle" => Some(None),
        _ => None,
    }
}

fn push_image_alt_text(
    event: &BytesStart<'_>,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    for (attribute, label) in [("descr", "image-description"), ("title", "image-title")] {
        if let Some(value) = attr(event, attribute)? {
            let value = normalize_text(&value);
            if !value.is_empty() {
                tokens.push(format!("{label}:{value}"));
            }
        }
    }
    Ok(())
}

fn push_image_extent(
    event: &BytesStart<'_>,
    extent_kind: &str,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let parse_coordinate = |name: &str| -> Result<u64, WorkerFailure> {
        let value = attr_required(event, name)?;
        value.parse::<u64>().map_err(|_| {
            failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!("unsupported image {extent_kind} extent coordinate {name}={value}"),
            )
        })
    };
    let width = parse_coordinate("cx")?;
    let height = parse_coordinate("cy")?;
    tokens.push(format!("image-{extent_kind}-extent:{width}:{height}"));
    Ok(())
}

fn push_image_offset(
    event: &BytesStart<'_>,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let parse_coordinate = |name: &str| -> Result<i64, WorkerFailure> {
        let value = attr_required(event, name)?;
        value.parse::<i64>().map_err(|_| {
            failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!("unsupported image offset coordinate {name}={value}"),
            )
        })
    };
    let x = parse_coordinate("x")?;
    let y = parse_coordinate("y")?;
    if x != 0 || y != 0 {
        tokens.push(format!("image-offset:{x}:{y}"));
    }
    Ok(())
}

fn push_image_rotation(
    event: &BytesStart<'_>,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let Some(value) = attr(event, "rot")? else {
        return Ok(());
    };
    let rotation = value.parse::<i32>().map_err(|_| {
        failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unsupported image rotation {value}"),
        )
    })?;
    let normalized_rotation = rotation.rem_euclid(DRAWINGML_ANGLE_UNITS_PER_TURN);
    if normalized_rotation != 0 {
        tokens.push(format!("image-rotation:{normalized_rotation}"));
    }
    Ok(())
}

fn record_picture_geometry(
    event: &BytesStart<'_>,
    pictures: &mut [PictureProjection],
) -> Result<(), WorkerFailure> {
    let Some(picture) = pictures.last_mut() else {
        return Ok(());
    };
    let preset = attr_required(event, "prst")?;
    if preset != "rect" {
        picture.unsupported_geometry.get_or_insert(preset);
    }
    Ok(())
}

fn reject_unsupported_referenced_picture_geometry(
    picture: PictureProjection,
) -> Result<(), WorkerFailure> {
    if picture.referenced_image
        && let Some(preset) = picture.unsupported_geometry
    {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("referenced picture uses unsupported preset geometry {preset}"),
        ));
    }
    Ok(())
}

fn push_numbering(
    event: &BytesStart<'_>,
    level: u8,
    numbering: &BTreeMap<(u32, u8), NumberingLevel>,
    numbering_instances: &mut BTreeMap<u32, usize>,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let id = numbering_u32(event, "val")?;
    let definition = numbering.get(&(id, level)).ok_or_else(|| {
        failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("unresolved numbering definition {id} at level {level}"),
        )
    })?;
    let instance = match numbering_instances.get(&id) {
        Some(instance) => *instance,
        None => {
            let instance = numbering_instances.len() + 1;
            numbering_instances.insert(id, instance);
            instance
        }
    };
    tokens.push(format!("list-instance:{instance}"));
    let token = serde_json::to_string(&(
        &definition.format,
        level,
        definition.start,
        &definition.level_text,
        &definition.multi_level_type,
    ))
    .map_err(|error| {
        failure(
            WorkerFailureCode::InvalidWorkerResult,
            format!("DOCX list projection serialization failed: {error}"),
        )
    })?;
    tokens.push(format!("list:{token}"));
    Ok(())
}

fn push_page_size(event: &BytesStart<'_>, tokens: &mut Vec<String>) -> Result<(), WorkerFailure> {
    let width = attr(event, "w")?.unwrap_or_default();
    let height = attr(event, "h")?.unwrap_or_default();
    let orientation = attr(event, "orient")?.unwrap_or_else(|| "portrait".into());
    tokens.push(format!("page:{width}x{height}:{orientation}"));
    Ok(())
}

fn push_image(
    event: &BytesStart<'_>,
    source_part: &str,
    package: &PackageInspection,
    image_budget: &mut ImageBudget,
    tokens: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    let Some(id) = attr(event, "embed")? else {
        return Ok(());
    };
    let rel = package
        .relationships_by_source
        .get(source_part)
        .and_then(|relationships| relationships.get(&id))
        .ok_or_else(|| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("image relationship {id} is missing from {source_part}"),
            )
        })?;
    if !rel.kind.ends_with("/image") || rel.external {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("drawing relationship {id} is not an internal image"),
        ));
    }
    let path = resolve_word_target(&rel.target)?;
    let bytes = package.parts.get(&path).ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("image target {path} is missing"),
        )
    })?;
    tokens.push(format!(
        "image-sha256:{}",
        image_semantic_digest(bytes, image_budget)?
    ));
    Ok(())
}

fn resolve_word_target(target: &str) -> Result<String, WorkerFailure> {
    validate_internal_target(target)?;
    Ok(format!("word/{target}"))
}

fn texts_for_prefix(
    parts: &BTreeMap<String, Vec<u8>>,
    prefix: &str,
) -> Result<Vec<String>, WorkerFailure> {
    let mut texts = Vec::new();
    for (name, bytes) in parts {
        if name.starts_with(prefix) && name.ends_with(".xml") {
            let text = extract_final_text(bytes)?;
            if !text.is_empty() {
                texts.push(text);
            }
        }
    }
    Ok(texts)
}

fn texts_for_parts(
    parts: &BTreeMap<String, Vec<u8>>,
    names: &[String],
) -> Result<Vec<String>, WorkerFailure> {
    let mut texts = Vec::new();
    for name in names {
        let bytes = parts.get(name).ok_or_else(|| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("referenced section part {name} is missing"),
            )
        })?;
        let text = extract_final_text(bytes)?;
        if !text.is_empty() {
            texts.push(text);
        }
    }
    Ok(texts)
}

fn note_entries(
    data: Option<&Vec<u8>>,
    note_name: &str,
) -> Result<BTreeMap<String, NoteEntry>, WorkerFailure> {
    let Some(bytes) = data else {
        return Ok(BTreeMap::new());
    };
    let text = std::str::from_utf8(bytes).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "OOXML note part is not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut entries = BTreeMap::new();
    let mut seen_ids = BTreeSet::new();
    let mut current: Option<CurrentNote> = None;
    let mut in_text = false;
    let mut deleted_depth = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.local_name().as_ref() == note_name => {
                if current.is_some() {
                    return Err(failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("nested {note_name} element"),
                    ));
                }
                let id = attr_required(&event, "id")?;
                let note_type = attr(&event, "type")?;
                let meaningful_type = !matches!(
                    note_type.as_deref(),
                    Some("separator" | "continuationSeparator")
                );
                current = Some(CurrentNote {
                    id,
                    content: String::new(),
                    meaningful_type,
                    unsupported_construct: None,
                    paragraph_count: 0,
                });
            }
            Ok(Event::Empty(event)) if event.local_name().as_ref() == note_name => {
                let id = attr_required(&event, "id")?;
                if !seen_ids.insert(id.clone()) {
                    return Err(failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("duplicate {note_name} id {id}"),
                    ));
                }
                entries.insert(
                    id,
                    NoteEntry {
                        content: String::new(),
                        unsupported_construct: None,
                    },
                );
            }
            Ok(Event::Empty(event)) => {
                if let Some(note) = current.as_mut()
                    && deleted_depth == 0
                    && note.meaningful_type
                {
                    let local_name = event.local_name();
                    record_note_element(note, local_name.as_ref());
                }
            }
            Ok(Event::Start(event)) => {
                if let Some(note) = current.as_mut()
                    && deleted_depth == 0
                    && note.meaningful_type
                {
                    let local_name = event.local_name();
                    record_note_element(note, local_name.as_ref());
                }
                match event.local_name().as_ref() {
                    "del" | "moveFrom" if current.is_some() => deleted_depth += 1,
                    "t" if current.is_some() && deleted_depth == 0 => in_text = true,
                    _ => {}
                }
            }
            Ok(Event::Text(value)) if in_text && deleted_depth == 0 => {
                if let Some(note) = current.as_mut() {
                    note.content
                        .push_str(&decode_xml(value.as_ref(), "OOXML note text")?);
                }
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "del" | "moveFrom" => deleted_depth = deleted_depth.saturating_sub(1),
                "p" if current.is_some() => {
                    if let Some(note) = current.as_mut()
                        && note
                            .content
                            .chars()
                            .last()
                            .is_some_and(|character| !character.is_whitespace())
                    {
                        note.content.push(' ');
                    }
                }
                local if local == note_name => {
                    let Some(note) = current.take() else {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            format!("unexpected closing {note_name} element"),
                        ));
                    };
                    if !seen_ids.insert(note.id.clone()) {
                        return Err(failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            format!("duplicate {note_name} id {}", note.id),
                        ));
                    }
                    let content = if note.meaningful_type {
                        normalize_text(&note.content)
                    } else {
                        String::new()
                    };
                    entries.insert(
                        note.id,
                        NoteEntry {
                            content,
                            unsupported_construct: note.unsupported_construct,
                        },
                    );
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("OOXML note parse failed: {error}"),
                ));
            }
        }
    }
    if current.is_some() {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("{note_name} part ended inside a note"),
        ));
    }
    Ok(entries)
}

fn note_values(entries: &BTreeMap<String, NoteEntry>) -> Vec<String> {
    let mut values: Vec<_> = entries
        .values()
        .map(|entry| &entry.content)
        .filter(|text| !text.is_empty())
        .cloned()
        .collect();
    values.sort();
    values
}

fn record_note_element(note: &mut CurrentNote, local_name: &str) {
    if let Some(construct) = unsupported_note_construct(local_name) {
        note.unsupported_construct
            .get_or_insert_with(|| construct.to_owned());
    }
    if local_name == "p" {
        note.paragraph_count = note.paragraph_count.saturating_add(1);
        if note.paragraph_count > 1 {
            note.unsupported_construct
                .get_or_insert_with(|| "multiple paragraphs".into());
        }
    }
}

fn unsupported_note_construct(local_name: &str) -> Option<&'static str> {
    match local_name {
        "drawing" | "pict" | "object" | "oleObject" | "control" | "altChunk" | "blip" | "pic"
        | "graphicFrame" => Some("visual or embedded content"),
        "hyperlink" | "fldChar" | "instrText" => Some("hyperlink or field content"),
        "tbl" | "tr" | "tc" | "tblPr" | "tblGrid" | "gridSpan" | "vMerge" | "hMerge" => {
            Some("table structure")
        }
        "numPr" | "numId" | "ilvl" => Some("list structure"),
        "sectPr" | "pgSz" | "pgMar" | "cols" | "col" | "titlePg" | "headerReference"
        | "footerReference" | "type" => Some("section or page structure"),
        "pStyle" | "outlineLvl" => Some("paragraph outline structure"),
        "br" | "cr" | "tab" | "sym" | "noBreakHyphen" | "softHyphen" => {
            Some("non-plain-text run content")
        }
        "bookmarkStart" | "bookmarkEnd" => Some("bookmark structure"),
        "commentRangeStart" | "commentRangeEnd" | "commentReference" => Some("comment structure"),
        "footnoteReference" | "endnoteReference" => Some("nested note reference"),
        _ => None,
    }
}

fn extract_final_text(bytes: &[u8]) -> Result<String, WorkerFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "OOXML text part is not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut in_text = false;
    let mut deleted_depth = 0usize;
    let mut paragraph_text = String::new();
    let mut pieces = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "del" | "moveFrom" => deleted_depth += 1,
                "p" if deleted_depth == 0 => paragraph_text.clear(),
                "t" if deleted_depth == 0 => in_text = true,
                _ => {}
            },
            Ok(Event::Text(value)) if in_text && deleted_depth == 0 => {
                paragraph_text.push_str(&decode_xml(value.as_ref(), "OOXML text")?);
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "del" | "moveFrom" => deleted_depth = deleted_depth.saturating_sub(1),
                "p" if deleted_depth == 0 => {
                    pieces.push(std::mem::take(&mut paragraph_text));
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("OOXML text parse failed: {error}"),
                ));
            }
        }
    }
    Ok(normalize_text(&pieces.join(" ")))
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn image_semantic_digest(bytes: &[u8], budget: &mut ImageBudget) -> Result<String, PngError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    let image_count = budget
        .count
        .checked_add(1)
        .ok_or(PngError::InspectionResourceLimitExceeded)?;
    if image_count > MAX_DOCX_IMAGES {
        return Err(PngError::InspectionResourceLimitExceeded);
    }
    budget.count = image_count;

    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err(PngError::UnsupportedSemanticConstruct(
            "DOCX image is not a supported PNG".into(),
        ));
    }

    // Inspect the complete chunk stream before asking png to allocate image buffers.
    // This bounds dimensions and rejects chunks whose display semantics are not in v0.
    let header = preflight_png(bytes)?;
    let pixels = u64::from(header.width)
        .checked_mul(u64::from(header.height))
        .ok_or(PngError::InspectionResourceLimitExceeded)?;
    let total_pixels = budget
        .decoded_pixels
        .checked_add(pixels)
        .ok_or(PngError::InspectionResourceLimitExceeded)?;
    if total_pixels > MAX_DOCX_DECODED_PIXELS {
        return Err(PngError::InspectionResourceLimitExceeded);
    }
    let output_bound = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(PngError::InspectionResourceLimitExceeded)?;
    let total_output = budget
        .decoded_output_bytes
        .checked_add(output_bound)
        .ok_or(PngError::InspectionResourceLimitExceeded)?;
    if total_output > MAX_DOCX_DECODED_OUTPUT_BYTES {
        return Err(PngError::InspectionResourceLimitExceeded);
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
        return Err(PngError::SemanticExtractionFailed(
            "PNG decoder metadata differs from strict preflight".into(),
        ));
    }

    let (output_color_type, output_bit_depth) = reader.output_color_type();
    if output_bit_depth != png::BitDepth::Eight {
        return Err(PngError::UnsupportedSemanticConstruct(
            "PNG decoder produced a non-8-bit pixel format".into(),
        ));
    }
    let channels = match output_color_type {
        png::ColorType::Grayscale => 1usize,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => {
            return Err(PngError::UnsupportedSemanticConstruct(
                "PNG palette was not expanded by the decoder".into(),
            ));
        }
    };
    let expected_output_size = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or(PngError::InspectionResourceLimitExceeded)?;
    let output_size = reader
        .output_buffer_size()
        .ok_or(PngError::InspectionResourceLimitExceeded)?;
    if output_size != expected_output_size || output_size > output_bound {
        return Err(PngError::InspectionResourceLimitExceeded);
    }

    let mut decoded = Vec::new();
    decoded
        .try_reserve_exact(output_size)
        .map_err(|_| PngError::InspectionResourceLimitExceeded)?;
    decoded.resize(output_size, 0);
    let output_info = reader.next_frame(&mut decoded).map_err(png_decode_error)?;
    let decoded = &decoded[..output_info.buffer_size()];
    if output_info.width != header.width
        || output_info.height != header.height
        || output_info.color_type != output_color_type
        || output_info.bit_depth != png::BitDepth::Eight
        || decoded.len() != expected_output_size
    {
        return Err(PngError::SemanticExtractionFailed(
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
    Ok(hex_digest(&digest.finalize()))
}

fn preflight_png(bytes: &[u8]) -> Result<PngHeader, PngError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err(PngError::UnsupportedSemanticConstruct(
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
            return Err(PngError::SemanticExtractionFailed(
                "truncated PNG chunk".into(),
            ));
        }
        let length = usize::try_from(u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| PngError::SemanticExtractionFailed("invalid PNG length".into()))?,
        ))
        .map_err(|_| PngError::InspectionResourceLimitExceeded)?;
        let data_start = offset
            .checked_add(8)
            .ok_or(PngError::InspectionResourceLimitExceeded)?;
        let data_end = data_start
            .checked_add(length)
            .ok_or(PngError::InspectionResourceLimitExceeded)?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or(PngError::InspectionResourceLimitExceeded)?;
        if chunk_end > bytes.len() {
            return Err(PngError::SemanticExtractionFailed(
                "PNG chunk exceeds image bytes".into(),
            ));
        }
        let chunk_type = &bytes[offset + 4..offset + 8];
        let data = &bytes[data_start..data_end];
        if !chunk_type.iter().all(u8::is_ascii_alphabetic) || chunk_type[2].is_ascii_lowercase() {
            return Err(PngError::SemanticExtractionFailed(
                "invalid PNG chunk type".into(),
            ));
        }
        if header.is_none() && (chunk_type != b"IHDR" || offset != PNG_SIGNATURE.len()) {
            return Err(PngError::SemanticExtractionFailed(
                "PNG IHDR must be the first chunk".into(),
            ));
        }
        if header.is_some() && chunk_type == b"IHDR" {
            return Err(PngError::SemanticExtractionFailed(
                "PNG has multiple IHDR chunks".into(),
            ));
        }
        if saw_idat && chunk_type != b"IDAT" {
            ended_idat = true;
        }
        if chunk_type == b"IDAT" && ended_idat {
            return Err(PngError::SemanticExtractionFailed(
                "PNG IDAT chunks are not contiguous".into(),
            ));
        }

        match chunk_type {
            b"IHDR" => {
                if length != 13 {
                    return Err(PngError::SemanticExtractionFailed(
                        "PNG IHDR must contain 13 bytes".into(),
                    ));
                }
                let width =
                    u32::from_be_bytes(data[0..4].try_into().map_err(|_| {
                        PngError::SemanticExtractionFailed("invalid PNG width".into())
                    })?);
                let height = u32::from_be_bytes(data[4..8].try_into().map_err(|_| {
                    PngError::SemanticExtractionFailed("invalid PNG height".into())
                })?);
                let bit_depth = data[8];
                let color_type = data[9];
                if width == 0 || height == 0 || data[10] != 0 || data[11] != 0 || data[12] > 1 {
                    return Err(PngError::SemanticExtractionFailed(
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
                    return Err(PngError::UnsupportedSemanticConstruct(
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
                    PngError::SemanticExtractionFailed("PNG PLTE precedes IHDR".into())
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
                    return Err(PngError::SemanticExtractionFailed(
                        "invalid PNG palette chunk".into(),
                    ));
                }
                let palette_entries = length / 3;
                if image_header.color_type == 3
                    && palette_entries > (1usize << image_header.bit_depth)
                {
                    return Err(PngError::SemanticExtractionFailed(
                        "indexed PNG palette exceeds its bit-depth range".into(),
                    ));
                }
                saw_palette = true;
                image_header.palette_entries = palette_entries;
                let (palette, remainder) = data.as_chunks::<3>();
                if !remainder.is_empty() {
                    return Err(PngError::SemanticExtractionFailed(
                        "invalid PNG palette chunk".into(),
                    ));
                }
                image_header.palette = palette.to_vec();
            }
            b"IDAT" => {
                let image_header = header.as_ref().ok_or_else(|| {
                    PngError::SemanticExtractionFailed("PNG IDAT precedes IHDR".into())
                })?;
                if image_header.color_type == 3 && !saw_palette {
                    return Err(PngError::SemanticExtractionFailed(
                        "indexed PNG has no palette".into(),
                    ));
                }
                saw_idat = true;
            }
            b"IEND" => {
                if !saw_idat || length != 0 || chunk_end != bytes.len() {
                    return Err(PngError::SemanticExtractionFailed(
                        "invalid PNG IEND or trailing bytes".into(),
                    ));
                }
                saw_iend = true;
                offset = chunk_end;
                break;
            }
            b"tRNS" => {
                let image_header = header.as_ref().ok_or_else(|| {
                    PngError::SemanticExtractionFailed("PNG tRNS precedes IHDR".into())
                })?;
                let valid_length = match image_header.color_type {
                    0 => length == 2,
                    2 => length == 6,
                    3 => saw_palette && length > 0 && length <= image_header.palette_entries,
                    _ => false,
                };
                if saw_idat || saw_transparency || !valid_length {
                    return Err(PngError::SemanticExtractionFailed(
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
                    PngError::SemanticExtractionFailed("PNG bKGD precedes IHDR".into())
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
                    return Err(PngError::SemanticExtractionFailed(
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
                        for (channel, sample_bytes) in data.as_chunks::<2>().0.iter().enumerate() {
                            let sample = u16::from_be_bytes(*sample_bytes)
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
                            PngError::SemanticExtractionFailed(
                                "indexed PNG background is outside its palette".into(),
                            )
                        })?,
                    _ => {
                        return Err(PngError::SemanticExtractionFailed(
                            "invalid PNG background color type".into(),
                        ));
                    }
                };
                image_header.metadata.background = Some(background);
                saw_background = true;
            }
            b"pHYs" => {
                let image_header = header.as_mut().ok_or_else(|| {
                    PngError::SemanticExtractionFailed("PNG pHYs precedes IHDR".into())
                })?;
                if saw_idat || saw_physical_dimensions || length != 9 || data[8] > 1 {
                    return Err(PngError::SemanticExtractionFailed(
                        "invalid PNG physical-dimensions chunk".into(),
                    ));
                }
                let mut physical_dimensions: [u8; 9] = data.try_into().map_err(|_| {
                    PngError::SemanticExtractionFailed("invalid PNG physical dimensions".into())
                })?;
                let pixels_per_unit_x =
                    u32::from_be_bytes(physical_dimensions[..4].try_into().map_err(|_| {
                        PngError::SemanticExtractionFailed("invalid PNG pHYs X value".into())
                    })?);
                let pixels_per_unit_y =
                    u32::from_be_bytes(physical_dimensions[4..8].try_into().map_err(|_| {
                        PngError::SemanticExtractionFailed("invalid PNG pHYs Y value".into())
                    })?);
                if pixels_per_unit_x == 0 || pixels_per_unit_y == 0 {
                    return Err(PngError::SemanticExtractionFailed(
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
                return Err(PngError::SemanticExtractionFailed(
                    "invalid PNG modification-time chunk".into(),
                ));
            }
            b"acTL" | b"fcTL" | b"fdAT" => {
                return Err(PngError::UnsupportedSemanticConstruct(
                    "animated PNG is unsupported".into(),
                ));
            }
            b"gAMA" | b"cHRM" | b"iCCP" | b"sRGB" | b"sBIT" | b"cICP" | b"mDCV" | b"cLLI" => {
                return Err(PngError::UnsupportedSemanticConstruct(
                    "PNG color profile or HDR metadata is unsupported".into(),
                ));
            }
            b"eXIf" => {
                return Err(PngError::UnsupportedSemanticConstruct(
                    "PNG EXIF orientation metadata is unsupported".into(),
                ));
            }
            _ if chunk_type[0].is_ascii_uppercase() => {
                return Err(PngError::UnsupportedSemanticConstruct(
                    "unknown critical PNG chunk".into(),
                ));
            }
            _ => {
                return Err(PngError::UnsupportedSemanticConstruct(
                    "unknown ancillary PNG chunk".into(),
                ));
            }
        }
        offset = chunk_end;
    }

    if !saw_iend || offset != bytes.len() || !saw_idat {
        return Err(PngError::SemanticExtractionFailed(
            "PNG is missing a complete IDAT/IEND sequence".into(),
        ));
    }
    header.ok_or_else(|| PngError::SemanticExtractionFailed("PNG has no IHDR".into()))
}

fn validate_indexed_png_pixels(bytes: &[u8], header: &PngHeader) -> Result<(), PngError> {
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
        return Err(PngError::SemanticExtractionFailed(
            "PNG indexed preflight metadata differs from decoder".into(),
        ));
    }

    let width =
        usize::try_from(header.width).map_err(|_| PngError::InspectionResourceLimitExceeded)?;
    let height =
        usize::try_from(header.height).map_err(|_| PngError::InspectionResourceLimitExceeded)?;
    let bit_depth = usize::from(header.bit_depth);
    let samples_per_byte = 8 / bit_depth;
    let row_bytes = width
        .checked_add(samples_per_byte - 1)
        .and_then(|bits| bits.checked_mul(bit_depth))
        .and_then(|bits| bits.checked_div(8))
        .ok_or(PngError::InspectionResourceLimitExceeded)?;
    let expected_size = row_bytes
        .checked_mul(height)
        .ok_or(PngError::InspectionResourceLimitExceeded)?;
    if reader.output_buffer_size() != Some(expected_size) {
        return Err(PngError::InspectionResourceLimitExceeded);
    }

    let mut decoded = Vec::new();
    decoded
        .try_reserve_exact(expected_size)
        .map_err(|_| PngError::InspectionResourceLimitExceeded)?;
    decoded.resize(expected_size, 0);
    let output_info = reader.next_frame(&mut decoded).map_err(png_decode_error)?;
    if output_info.width != header.width
        || output_info.height != header.height
        || output_info.color_type != png::ColorType::Indexed
        || output_info.bit_depth as u8 != header.bit_depth
        || output_info.buffer_size() != expected_size
    {
        return Err(PngError::SemanticExtractionFailed(
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
                return Err(PngError::SemanticExtractionFailed(
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
) -> Result<Option<Vec<u8>>, PngError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !matches!(header.color_type, 0 | 2) {
        return Ok(None);
    }

    let mut offset = PNG_SIGNATURE.len();
    while offset < bytes.len() {
        let length = usize::try_from(u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| PngError::SemanticExtractionFailed("invalid PNG length".into()))?,
        ))
        .map_err(|_| PngError::InspectionResourceLimitExceeded)?;
        let data_start = offset
            .checked_add(8)
            .ok_or(PngError::InspectionResourceLimitExceeded)?;
        let data_end = data_start
            .checked_add(length)
            .ok_or(PngError::InspectionResourceLimitExceeded)?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or(PngError::InspectionResourceLimitExceeded)?;
        let chunk_type = &bytes[offset + 4..offset + 8];
        if chunk_type == b"tRNS" {
            let data = &bytes[data_start..data_end];
            let stored_crc = u32::from_be_bytes(
                bytes[data_end..chunk_end]
                    .try_into()
                    .map_err(|_| PngError::SemanticExtractionFailed("invalid PNG CRC".into()))?,
            );
            if png_chunk_crc32(chunk_type, data) != stored_crc {
                return Err(PngError::SemanticExtractionFailed(
                    "PNG transparency chunk checksum mismatch".into(),
                ));
            }

            let sample_mask = png_sample_mask(header.bit_depth);
            let mut normalized = Vec::with_capacity(data.len());
            for encoded_sample in data.as_chunks::<2>().0 {
                let sample = u16::from_be_bytes(*encoded_sample) & sample_mask;
                normalized.extend_from_slice(&sample.to_be_bytes());
            }
            if normalized == data {
                return Ok(None);
            }

            let mut output = Vec::new();
            output
                .try_reserve_exact(bytes.len())
                .map_err(|_| PngError::InspectionResourceLimitExceeded)?;
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
) -> Result<(), PngError> {
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
            return Err(PngError::UnsupportedSemanticConstruct(
                "indexed PNG pixels were not expanded".into(),
            ));
        }
    };
    if !pixels.len().is_multiple_of(source_channels) {
        return Err(PngError::SemanticExtractionFailed(
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
                return Err(PngError::SemanticExtractionFailed(
                    "unexpected RGBA branch during PNG canonicalization".into(),
                ));
            }
            png::ColorType::Indexed => {
                return Err(PngError::UnsupportedSemanticConstruct(
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
) -> Result<(), PngError> {
    digest.update(kind);
    let length =
        u64::try_from(data.len()).map_err(|_| PngError::InspectionResourceLimitExceeded)?;
    digest.update(length.to_be_bytes());
    digest.update(data);
    Ok(())
}

fn png_decode_error(error: png::DecodingError) -> PngError {
    PngError::SemanticExtractionFailed(format!("PNG decoding failed: {error}"))
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

fn parse_editorial_evidence(
    parts: &BTreeMap<String, Vec<u8>>,
    document: &[u8],
    selected_headers: &[String],
    selected_footers: &[String],
) -> Result<EditorialProvenance, WorkerFailure> {
    let mut tracked_changes = parse_tracked_changes(document, "word/document.xml")?;
    let mut tracked_parts = BTreeSet::from(["word/document.xml"]);
    for part_name in ["word/footnotes.xml", "word/endnotes.xml"] {
        if let Some(part) = parts.get(part_name) {
            tracked_changes.extend(parse_tracked_changes(part, part_name)?);
            tracked_parts.insert(part_name);
        }
    }
    for part_name in selected_headers.iter().chain(selected_footers) {
        if tracked_parts.insert(part_name.as_str()) {
            let part = parts.get(part_name).ok_or_else(|| {
                failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("referenced section part {part_name} is missing"),
                )
            })?;
            tracked_changes.extend(parse_tracked_changes(part, part_name)?);
        }
    }
    let anchors = parse_comment_anchor_paragraphs(document)?;
    let comments = parse_comments(
        parts.get("word/comments.xml").map(Vec::as_slice),
        parts.get("word/commentsExtended.xml").map(Vec::as_slice),
        &anchors,
    )?;
    let (mut author_labels, last_modified_by, modification_metadata) =
        parse_core_properties(parts.get("docProps/core.xml").map(Vec::as_slice))?;
    let mut labels: BTreeSet<String> = author_labels.into_iter().collect();
    for change in &tracked_changes {
        if let Some(author) = &change.author_label
            && !author.is_empty()
        {
            labels.insert(author.clone());
        }
    }
    for comment in &comments {
        if let Some(author) = &comment.author_label
            && !author.is_empty()
        {
            labels.insert(author.clone());
        }
    }
    if let Some(author) = &last_modified_by
        && !author.is_empty()
    {
        labels.insert(author.clone());
    }
    author_labels = labels.into_iter().collect();
    Ok(EditorialProvenance {
        tracked_changes,
        comments,
        document_author_labels: author_labels,
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

fn parse_tracked_changes(
    data: &[u8],
    source_part: &str,
) -> Result<Vec<TrackedChangeEvidence>, WorkerFailure> {
    let text = std::str::from_utf8(data).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "document XML is not UTF-8",
        )
    })?;
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
                        source_locator: id.map_or_else(
                            || format!("{source_part}#{kind}:{}", changes.len()),
                            |id| format!("{source_part}#{kind}:{id}"),
                        ),
                        unresolved: true,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("tracked-change XML parse failed: {error}"),
                ));
            }
        }
    }
    Ok(changes)
}

fn parse_comment_anchor_paragraphs(
    document: &[u8],
) -> Result<BTreeMap<String, String>, WorkerFailure> {
    let text = std::str::from_utf8(document).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "document XML is not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut current_para_id = None;
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
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("comment anchor parse failed: {error}"),
                ));
            }
        }
    }
    Ok(anchors)
}

fn parse_comments(
    comments_data: Option<&[u8]>,
    extended_data: Option<&[u8]>,
    anchor_paragraphs: &BTreeMap<String, String>,
) -> Result<Vec<CommentEvidence>, WorkerFailure> {
    let Some(data) = comments_data else {
        return Ok(Vec::new());
    };
    let resolved = parse_comment_resolution(extended_data)?;
    let text = std::str::from_utf8(data).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "comments XML is not UTF-8",
        )
    })?;
    #[derive(Default)]
    struct Pending {
        id: Option<String>,
        author: Option<String>,
        timestamp: Option<String>,
        para_id: Option<String>,
        text: Vec<String>,
    }
    let mut reader = Reader::from_str(text);
    let mut pending: Option<Pending> = None;
    let mut in_text = false;
    let mut comments = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                "comment" => {
                    pending = Some(Pending {
                        id: attr(&event, "id")?,
                        author: attr(&event, "author")?,
                        timestamp: attr(&event, "date")?,
                        ..Pending::default()
                    });
                }
                "p" => {
                    if let Some(comment) = pending.as_mut()
                        && comment.para_id.is_none()
                    {
                        comment.para_id = attr(&event, "paraId")?;
                    }
                }
                "t" if pending.is_some() => in_text = true,
                _ => {}
            },
            Ok(Event::Empty(event)) if event.local_name().as_ref() == "p" => {
                if let Some(comment) = pending.as_mut()
                    && comment.para_id.is_none()
                {
                    comment.para_id = attr(&event, "paraId")?;
                }
            }
            Ok(Event::Text(value)) if in_text => {
                if let Some(comment) = pending.as_mut() {
                    comment
                        .text
                        .push(decode_xml(value.as_ref(), "comment text")?);
                }
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "comment" => {
                    if let Some(comment) = pending.take() {
                        let id = comment
                            .id
                            .clone()
                            .unwrap_or_else(|| comments.len().to_string());
                        let para_id = comment
                            .para_id
                            .clone()
                            .or_else(|| anchor_paragraphs.get(&id).cloned());
                        let is_resolved = para_id
                            .as_ref()
                            .and_then(|para_id| resolved.get(para_id))
                            .copied()
                            .unwrap_or(false);
                        comments.push(CommentEvidence {
                            author_label: comment.author,
                            timestamp: comment.timestamp,
                            resolved_state: if is_resolved {
                                "resolved"
                            } else {
                                "unresolved"
                            }
                            .into(),
                            source_locator: format!("word/comments.xml#comment:{id}"),
                            content: normalize_text(&comment.text.join(" ")),
                        });
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("comments XML parse failed: {error}"),
                ));
            }
        }
    }
    if pending.is_some() {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "comments XML ended inside a comment",
        ));
    }
    Ok(comments)
}

fn parse_comment_resolution(data: Option<&[u8]>) -> Result<BTreeMap<String, bool>, WorkerFailure> {
    let Some(bytes) = data else {
        return Ok(BTreeMap::new());
    };
    let text = std::str::from_utf8(bytes).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "commentsExtended XML is not UTF-8",
        )
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
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("commentsExtended parse failed: {error}"),
                ));
            }
        }
    }
    Ok(result)
}

type CorePropertyEvidence = (Vec<String>, Option<String>, BTreeMap<String, String>);

fn parse_core_properties(data: Option<&[u8]>) -> Result<CorePropertyEvidence, WorkerFailure> {
    let Some(bytes) = data else {
        return Ok((Vec::new(), None, BTreeMap::new()));
    };
    let text = std::str::from_utf8(bytes).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "core properties are not UTF-8",
        )
    })?;
    let mut reader = Reader::from_str(text);
    let mut current = None;
    let mut values = BTreeMap::new();
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
                    values.insert(key.to_owned(), decode_xml(value.as_ref(), "core property")?);
                }
            }
            Ok(Event::End(_)) => current = None,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("core properties parse failed: {error}"),
                ));
            }
        }
    }
    let authors = values
        .get("creator")
        .filter(|creator| !creator.is_empty())
        .cloned()
        .into_iter()
        .collect();
    let last_modified_by = values.get("lastModifiedBy").cloned();
    let modification_metadata = values
        .into_iter()
        .filter(|(key, _)| matches!(key.as_str(), "created" | "modified"))
        .collect();
    Ok((authors, last_modified_by, modification_metadata))
}

#[cfg(test)]
mod parser_disagreement_privacy_tests {
    use super::parser_disagreement_failure;
    use crate::{WorkerFailureCode, write_failure};

    #[test]
    fn parser_disagreement_stderr_omits_document_text_and_is_bounded() {
        let marker = "SYNTHETIC_PRIVATE_DOCUMENT_BODY_MARKER";
        let failure = parser_disagreement_failure(marker, "different candidate text");
        assert_eq!(failure.code(), WorkerFailureCode::ParserDisagreement);
        let mut stderr = Vec::new();
        write_failure(&mut stderr, &failure);
        let stderr = String::from_utf8(stderr).expect("failure is UTF-8 JSON");
        assert!(!stderr.contains(marker));
        assert!(!stderr.contains("different candidate text"));
        assert!(stderr.len() < 512, "failure output must be bounded");
    }
}
