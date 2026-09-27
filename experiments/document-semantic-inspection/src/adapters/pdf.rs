use crate::{
    AdapterOutput, CapabilityEvidence, CommentEvidence, Diagnostic, EditorialEvidence,
    ExternalDependency, FormatId, InspectionAdapter, InspectionProfile, PocError,
    canonical_json_bytes,
};
use lopdf::content::{Content, Operation};
use lopdf::{Document, LoadOptions, Object};
use pdfium_render::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::sync::OnceLock;

const MAX_DECOMPRESSED_STREAM: usize = 64 * 1024 * 1024;
const MAX_PDF_OBJECT_DEPTH: usize = 64;

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default)]
pub struct PdfAdapter;

#[derive(Debug)]
struct LopdfFacts {
    page_count: usize,
    annotation_counts: Vec<usize>,
    link_counts: Vec<usize>,
    form_field_count: usize,
    image_hashes: Vec<Vec<Value>>,
    paint_orders: Vec<Vec<Value>>,
}

impl InspectionAdapter for PdfAdapter {
    fn format(&self) -> FormatId {
        FormatId::Pdf
    }

    fn inspect(
        &self,
        input: &[u8],
        _profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError> {
        let lopdf = load_lopdf(input)?;
        if lopdf.is_encrypted() || lopdf.was_encrypted() {
            return Err(PocError::EncryptedContentUnsupported);
        }
        let structural = extract_lopdf_facts(&lopdf)?;
        let pdfium = pdfium()?;
        let document = pdfium
            .load_pdf_from_byte_slice(input, None)
            .map_err(|error| map_pdfium_open_error(error))?;

        let pages = document.pages();
        let page_count = pages.len() as usize;
        if page_count != structural.page_count {
            return Err(PocError::ParserDisagreement(format!(
                "page count mismatch: pdfium={page_count}, lopdf={}",
                structural.page_count
            )));
        }

        let form_values: BTreeMap<String, Option<String>> = document
            .form()
            .map(|form| form.field_values(&pages).into_iter().collect())
            .unwrap_or_default();

        if form_values.len() != structural.form_field_count {
            return Err(PocError::ParserDisagreement(format!(
                "form field count mismatch: pdfium={}, lopdf={}",
                form_values.len(),
                structural.form_field_count
            )));
        }

        let mut semantic_pages = Vec::with_capacity(page_count);
        let mut external_dependencies = Vec::new();
        let mut editorial = EditorialEvidence::default();
        let mut any_text = false;
        let mut total_images = 0usize;

        for (page_index, page) in pages.iter().enumerate() {
            validate_pdfium_read_order(&page, page_index)?;

            let text = page
                .text()
                .map_err(|error| {
                    PocError::SemanticExtractionFailed(format!(
                        "PDFium text extraction on page {page_index}: {error}"
                    ))
                })?
                .all();
            if !text.trim().is_empty() {
                any_text = true;
            }

            let annotation_count = page.annotations().len() as usize;
            for (annotation_index, annotation) in page.annotations().iter().enumerate() {
                let annotation_type = annotation.annotation_type();
                if !matches!(
                    annotation_type,
                    PdfPageAnnotationType::Link
                        | PdfPageAnnotationType::Widget
                        | PdfPageAnnotationType::XfaWidget
                ) {
                    editorial.comments_present = true;
                    editorial.comments.push(CommentEvidence {
                        author_label: None,
                        timestamp: None,
                        resolved_state: "unknown".into(),
                        source_locator: format!(
                            "page[{page_index}]/annotation[{annotation_index}]"
                        ),
                        content: format!(
                            "{:?}:{}",
                            annotation_type,
                            annotation.contents().unwrap_or_default()
                        ),
                    });
                }
            }
            if annotation_count != structural.annotation_counts[page_index] {
                return Err(PocError::ParserDisagreement(format!(
                    "annotation count mismatch on page {}: pdfium={}, lopdf={}",
                    page_index + 1,
                    annotation_count,
                    structural.annotation_counts[page_index]
                )));
            }

            let mut links = Vec::new();
            for link in page.links().iter() {
                let target = if let Some(action) = link.action() {
                    if let Some(uri) = action.as_uri_action() {
                        let uri = uri.uri().map_err(|error| {
                            PocError::SemanticExtractionFailed(format!(
                                "PDFium URI link on page {page_index}: {error}"
                            ))
                        })?;
                        external_dependencies.push(ExternalDependency {
                            kind: "uri".into(),
                            definition: uri.clone(),
                        });
                        format!("uri:{uri}")
                    } else if let Some(local) = action.as_local_destination_action() {
                        let destination = local.destination().map_err(|error| {
                            PocError::SemanticExtractionFailed(format!(
                                "PDFium local destination on page {page_index}: {error}"
                            ))
                        })?;
                        let page = destination.page_index().map_err(|error| {
                            PocError::SemanticExtractionFailed(format!(
                                "PDFium local destination page index: {error}"
                            ))
                        })?;
                        format!("page:{page}")
                    } else {
                        return Err(PocError::UnsupportedSemanticConstruct(format!(
                            "unsupported PDF link action on page {page_index}: {:?}",
                            action.action_type()
                        )));
                    }
                } else if let Some(destination) = link.destination() {
                    let page = destination.page_index().map_err(|error| {
                        PocError::SemanticExtractionFailed(format!(
                            "PDFium link destination page index: {error}"
                        ))
                    })?;
                    format!("page:{page}")
                } else {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "PDF link has no supported target on page {page_index}"
                    )));
                };
                links.push(target);
            }
            links.sort();
            if links.len() != structural.link_counts[page_index] {
                return Err(PocError::ParserDisagreement(format!(
                    "link count mismatch on page {}: pdfium={}, lopdf={}",
                    page_index + 1,
                    links.len(),
                    structural.link_counts[page_index]
                )));
            }

            let image_hashes = structural.image_hashes[page_index].clone();
            let paint_order = structural.paint_orders[page_index].clone();
            total_images += image_hashes.len();

            semantic_pages.push(json!({
                "index": page_index,
                "text": text,
                "links": links,
                "images": image_hashes,
                "paint_order": paint_order,
            }));
        }

        if !any_text && total_images > 0 {
            return Err(PocError::RequiresOcr);
        }

        external_dependencies.sort_by(|left, right| {
            (left.kind.as_str(), left.definition.as_str())
                .cmp(&(right.kind.as_str(), right.definition.as_str()))
        });
        external_dependencies
            .dedup_by(|left, right| left.kind == right.kind && left.definition == right.definition);

        let projection = json!({
            "pages": semantic_pages,
            "form_values": form_values,
        });
        let semantic_projection = canonical_json_bytes(&projection)
            .map_err(|error| PocError::InvalidWorkerResult(format!("PDF projection: {error}")))?;
        let semantic_equivalence = hex::encode(crate::fingerprint(&semantic_projection));

        Ok(AdapterOutput {
            semantic_projection,
            capabilities: vec![
                CapabilityEvidence::binary(
                    "reader_content",
                    any_text,
                    true,
                    Some(semantic_equivalence.clone()),
                ),
                CapabilityEvidence::binary(
                    "form_fields",
                    structural.form_field_count > 0,
                    true,
                    Some(semantic_equivalence.clone()),
                ),
                CapabilityEvidence::binary(
                    "annotations",
                    structural.annotation_counts.iter().any(|count| *count > 0),
                    false,
                    None,
                ),
                CapabilityEvidence::binary(
                    "visual_content",
                    total_images > 0,
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
                CapabilityEvidence::new(
                    "hidden_content",
                    crate::CapabilityState::NotVerifiable,
                    true,
                    None,
                ),
            ],
            editorial,
            external_dependencies,
            signatures: Vec::new(),
            diagnostics: vec![Diagnostic {
                code: "extractor_provenance".into(),
                message: format!(
                    "pdfium=151.0.7881.0;release=chromium/7881;sha256={};lopdf=0.45.0",
                    pdfium_artifact_sha256()
                ),
            }],
        })
    }
}

fn pdfium() -> Result<&'static Pdfium, PocError> {
    match PDFIUM.get_or_init(|| {
        let directory = env::var("PDFIUM_DYNAMIC_LIB_PATH")
            .map_err(|_| "PDFIUM_DYNAMIC_LIB_PATH is not set".to_owned())?;
        let path = Pdfium::pdfium_platform_library_name_at_path(&directory);
        let bindings = Pdfium::bind_to_library(path).map_err(|error| error.to_string())?;
        Ok(Pdfium::new(bindings))
    }) {
        Ok(pdfium) => Ok(pdfium),
        Err(message) => Err(PocError::ExtractorUnavailable(format!(
            "PDFium 7881: {message}"
        ))),
    }
}

fn load_lopdf(input: &[u8]) -> Result<Document, PocError> {
    let options = LoadOptions {
        strict: true,
        max_decompressed_size: Some(MAX_DECOMPRESSED_STREAM),
        ..Default::default()
    };
    Document::load_mem_with_options(input, options).map_err(map_lopdf_open_error)
}

fn map_lopdf_open_error(error: lopdf::Error) -> PocError {
    match error {
        lopdf::Error::InvalidPassword
        | lopdf::Error::UnsupportedSecurityHandler(_)
        | lopdf::Error::Decryption(_) => PocError::EncryptedContentUnsupported,
        other => PocError::SemanticExtractionFailed(format!("lopdf strict load: {other}")),
    }
}

fn map_pdfium_open_error(error: PdfiumError) -> PocError {
    PocError::SemanticExtractionFailed(format!("PDFium open: {error}"))
}

fn validate_pdfium_read_order(page: &PdfPage<'_>, page_index: usize) -> Result<(), PocError> {
    let mut text_regions = Vec::new();

    for object in page.objects().iter() {
        let Some(text_object) = object.as_text_object() else {
            continue;
        };
        let text = text_object.text();
        if text.trim().is_empty() {
            continue;
        }
        let bounds = object
            .bounds()
            .map_err(|error| {
                PocError::SemanticExtractionFailed(format!(
                    "PDFium text bounds on page {page_index}: {error}"
                ))
            })?
            .to_rect();
        text_regions.push((text, bounds));
    }

    for left in 0..text_regions.len() {
        for right in (left + 1)..text_regions.len() {
            let (left_text, left_bounds) = &text_regions[left];
            let (right_text, right_bounds) = &text_regions[right];
            if left_text != right_text && left_bounds.does_overlap(right_bounds) {
                return Err(PocError::UnsupportedSemanticConstruct(format!(
                    "ambiguous PDF text/read order on page {}",
                    page_index + 1
                )));
            }
        }
    }

    Ok(())
}

fn extract_lopdf_facts(document: &Document) -> Result<LopdfFacts, PocError> {
    let pages = document.get_pages();
    let mut annotation_counts = Vec::with_capacity(pages.len());
    let mut link_counts = Vec::with_capacity(pages.len());
    let mut image_hashes = Vec::with_capacity(pages.len());
    let mut paint_orders = Vec::with_capacity(pages.len());
    let mut form_names = BTreeSet::new();

    for (page_number, page_id) in pages {
        let page = document.get_dictionary(page_id).map_err(|error| {
            PocError::SemanticExtractionFailed(format!("lopdf page {page_number}: {error}"))
        })?;

        let mut annotation_count = 0usize;
        let mut link_count = 0usize;

        if let Ok(annots) = page.get_deref(b"Annots", document) {
            let annots = annots.as_array().map_err(|error| {
                PocError::ParserDisagreement(format!(
                    "lopdf page {page_number} Annots is not an array: {error}"
                ))
            })?;
            annotation_count = annots.len();

            for annotation in annots {
                let (_, resolved) = document.dereference(annotation).map_err(|error| {
                    PocError::ParserDisagreement(format!(
                        "lopdf page {page_number} annotation cannot be resolved: {error}"
                    ))
                })?;
                let dict = resolved.as_dict().map_err(|error| {
                    PocError::ParserDisagreement(format!(
                        "lopdf page {page_number} annotation is not a dictionary: {error}"
                    ))
                })?;
                let subtype = dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .map(String::from_utf8_lossy)
                    .map(|value| value.into_owned())
                    .unwrap_or_default();

                if subtype == "Link" {
                    link_count += 1;
                } else if subtype == "Widget" {
                    let field = match dict.get(b"Parent") {
                        Ok(parent) => document
                            .dereference(parent)
                            .map_err(|error| {
                                PocError::ParserDisagreement(format!(
                                    "lopdf form parent cannot be resolved: {error}"
                                ))
                            })?
                            .1
                            .as_dict()
                            .map_err(|error| {
                                PocError::ParserDisagreement(format!(
                                    "lopdf form parent is not a dictionary: {error}"
                                ))
                            })?,
                        Err(_) => dict,
                    };
                    if let Ok(name) = field.get(b"T").and_then(Object::as_str) {
                        form_names.insert(String::from_utf8_lossy(name).into_owned());
                    }
                }
            }
        }

        annotation_counts.push(annotation_count);
        link_counts.push(link_count);
        let (page_images, paint_order) = page_paint_events(document, page_id, page_number)?;
        image_hashes.push(page_images);
        paint_orders.push(paint_order);
    }

    Ok(LopdfFacts {
        page_count: annotation_counts.len(),
        annotation_counts,
        link_counts,
        form_field_count: form_names.len(),
        image_hashes,
        paint_orders,
    })
}

#[derive(Clone, Copy)]
struct PdfCtm([f64; 6]);

impl PdfCtm {
    const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn concat(self, other: [f64; 6], page_number: u32) -> Result<Self, PocError> {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = other;
        let matrix = Self([
            g * a + h * c,
            g * b + h * d,
            i * a + j * c,
            i * b + j * d,
            k * a + l * c + e,
            k * b + l * d + f,
        ]);
        if matrix.0.iter().any(|value| !value.is_finite()) {
            return Err(PocError::UnsupportedSemanticConstruct(format!(
                "non-finite PDF transform on page {page_number}"
            )));
        }
        Ok(matrix)
    }

    fn projection(self) -> Value {
        json!(
            self.0
                .into_iter()
                .map(|value| format!(
                    "f64:{:016x}",
                    if value == 0.0 { 0.0f64 } else { value }.to_bits()
                ))
                .collect::<Vec<_>>()
        )
    }
}

#[derive(Default)]
struct PdfContentBudget {
    decompressed: usize,
}

fn page_paint_events(
    document: &Document,
    page_id: lopdf::ObjectId,
    page_number: u32,
) -> Result<(Vec<Value>, Vec<Value>), PocError> {
    let mut current_id = page_id;
    let mut visited = BTreeSet::new();
    let mut resources = None;
    for depth in 0..MAX_PDF_OBJECT_DEPTH {
        if !visited.insert(current_id) {
            return Err(PocError::ParserDisagreement(format!(
                "lopdf page {page_number} has a cyclic page tree"
            )));
        }
        let node = document.get_dictionary(current_id).map_err(|error| {
            PocError::ParserDisagreement(format!(
                "lopdf page {page_number} page-tree node: {error}"
            ))
        })?;
        match node.get_deref(b"Resources", document) {
            Ok(value) => {
                resources = Some(value.as_dict().map_err(|error| {
                    PocError::ParserDisagreement(format!(
                        "lopdf page {page_number} Resources: {error}"
                    ))
                })?);
                break;
            }
            Err(lopdf::Error::DictKey(_)) => {}
            Err(error) => {
                return Err(PocError::ParserDisagreement(format!(
                    "lopdf page {page_number} Resources: {error}"
                )));
            }
        }
        match node.get(b"Parent") {
            Ok(Object::Reference(parent_id)) => current_id = *parent_id,
            Err(lopdf::Error::DictKey(_)) => break,
            Ok(_) | Err(_) => {
                return Err(PocError::ParserDisagreement(format!(
                    "lopdf page {page_number} Parent is invalid"
                )));
            }
        }
        if depth + 1 == MAX_PDF_OBJECT_DEPTH {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
    }
    let mut budget = PdfContentBudget::default();
    if document
        .get_dictionary(page_id)
        .is_ok_and(|page| page.has(b"Group"))
    {
        return Err(PocError::UnsupportedSemanticConstruct(format!(
            "unsupported PDF page transparency group on page {page_number}"
        )));
    }
    let content = page_content_bytes(document, page_id, page_number, &mut budget)?;
    let content = Content::decode_strict(&content).map_err(|error| {
        PocError::ParserDisagreement(format!(
            "lopdf page {page_number} strict content decode: {error}"
        ))
    })?;
    let empty_resources = lopdf::Dictionary::new();
    let resources = resources.unwrap_or(&empty_resources);
    let mut events = Vec::new();
    let mut paint_order = Vec::new();
    let mut active_forms = BTreeSet::new();
    let mut clips = Vec::new();
    collect_painted_images(
        document,
        &content.operations,
        resources,
        page_number,
        0,
        PdfCtm::IDENTITY,
        &mut budget,
        &mut active_forms,
        &mut clips,
        &mut events,
        &mut paint_order,
    )?;
    Ok((events, paint_order))
}

fn page_content_bytes(
    document: &Document,
    page_id: lopdf::ObjectId,
    page_number: u32,
    budget: &mut PdfContentBudget,
) -> Result<Vec<u8>, PocError> {
    let page = document.get_dictionary(page_id).map_err(|error| {
        PocError::ParserDisagreement(format!("lopdf page {page_number}: {error}"))
    })?;
    let contents = match page.get(b"Contents") {
        Ok(contents) => contents,
        Err(lopdf::Error::DictKey(_)) => return Ok(Vec::new()),
        Err(error) => {
            return Err(PocError::ParserDisagreement(format!(
                "lopdf page {page_number} Contents: {error}"
            )));
        }
    };
    let (_, contents) = document.dereference(contents).map_err(|error| {
        PocError::ParserDisagreement(format!(
            "lopdf page {page_number} Contents cannot be resolved: {error}"
        ))
    })?;
    let streams: Vec<&lopdf::Stream> = match contents {
        Object::Stream(stream) => vec![stream],
        Object::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let (_, resolved) = document.dereference(item).map_err(|error| {
                    PocError::ParserDisagreement(format!(
                        "lopdf page {page_number} Contents[{index}] cannot be resolved: {error}"
                    ))
                })?;
                resolved.as_stream().map_err(|error| {
                    PocError::ParserDisagreement(format!(
                        "lopdf page {page_number} Contents[{index}] is not a stream: {error}"
                    ))
                })
            })
            .collect::<Result<_, _>>()?,
        other => {
            return Err(PocError::ParserDisagreement(format!(
                "lopdf page {page_number} Contents is not a stream or array: {other:?}"
            )));
        }
    };

    let mut content = Vec::new();
    for (index, stream) in streams.iter().enumerate() {
        let decoded =
            decode_pdf_content_stream(stream, page_number, &format!("Contents[{index}]"), budget)?;
        if decoded.len().saturating_add(1)
            > MAX_DECOMPRESSED_STREAM.saturating_sub(budget.decompressed)
        {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
        content.extend_from_slice(&decoded);
        content.push(b'\n');
        budget.decompressed += 1;
    }
    Ok(content)
}

fn decode_pdf_content_stream(
    stream: &lopdf::Stream,
    page_number: u32,
    label: &str,
    budget: &mut PdfContentBudget,
) -> Result<Vec<u8>, PocError> {
    let remaining = MAX_DECOMPRESSED_STREAM.saturating_sub(budget.decompressed);
    let decoded =
        stream
            .decompressed_content_with_limit(remaining)
            .map_err(|error| match error {
                lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded {
                    ..
                }) => PocError::InspectionResourceLimitExceeded,
                other => PocError::ParserDisagreement(format!(
                    "lopdf page {page_number} {label} cannot be decoded: {other}"
                )),
            })?;
    budget.decompressed = budget
        .decompressed
        .checked_add(decoded.len())
        .ok_or(PocError::InspectionResourceLimitExceeded)?;
    Ok(decoded)
}

fn collect_painted_images(
    document: &Document,
    operations: &[Operation],
    resources: &lopdf::Dictionary,
    page_number: u32,
    depth: usize,
    mut ctm: PdfCtm,
    budget: &mut PdfContentBudget,
    active_forms: &mut BTreeSet<lopdf::ObjectId>,
    clips: &mut Vec<Value>,
    events: &mut Vec<Value>,
    paint_order: &mut Vec<Value>,
) -> Result<(), PocError> {
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(PocError::InspectionResourceLimitExceeded);
    }
    let mut graphics_stack = Vec::new();
    for operation in operations {
        match operation.operator.as_str() {
            "q" if operation.operands.is_empty() => graphics_stack.push(ctm),
            "Q" if operation.operands.is_empty() => {
                ctm = graphics_stack.pop().ok_or_else(|| {
                    PocError::UnsupportedSemanticConstruct(format!(
                        "unbalanced PDF Q operator on page {page_number}"
                    ))
                })?;
            }
            "cm" => {
                ctm = ctm.concat(
                    pdf_matrix_operands(&operation.operands, page_number, "cm")?,
                    page_number,
                )?;
            }
            "Do" => {
                let name = match operation.operands.as_slice() {
                    [Object::Name(name)] => name,
                    _ => {
                        return Err(PocError::ParserDisagreement(format!(
                            "lopdf page {page_number} Do requires one name operand"
                        )));
                    }
                };
                invoke_pdf_xobject(
                    document,
                    name,
                    resources,
                    page_number,
                    depth,
                    ctm,
                    budget,
                    active_forms,
                    clips,
                    events,
                    paint_order,
                )?;
            }
            "Tf" => ensure_supported_font_selection(
                document,
                resources,
                &operation.operands,
                page_number,
            )?,
            "Tj" | "TJ" | "'" | "\"" => {
                if text_show_contains_nonempty_string(operation, page_number)?
                    && paint_order.last().and_then(Value::as_str) != Some("text")
                {
                    paint_order.push(json!("text"));
                }
            }
            "BI" | "sh" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "W" | "W*"
            | "gs" | "ri" => {
                return Err(PocError::UnsupportedSemanticConstruct(format!(
                    "unsupported PDF visual operator {} on page {page_number}",
                    operation.operator
                )));
            }
            "Tr" => {
                if operation.operands.len() != 1
                    || pdf_number(&operation.operands[0]).ok() != Some(0.0)
                {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "unsupported PDF text rendering mode on page {page_number}"
                    )));
                }
            }
            "BDC" => {
                if matches!(
                    operation.operands.first(),
                    Some(Object::Name(tag)) if tag.as_slice() == b"OC"
                ) {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "unsupported optional-content visibility on page {page_number}"
                    )));
                }
            }
            operator if known_non_painting_operator(operator) => {}
            operator => {
                return Err(PocError::UnsupportedSemanticConstruct(format!(
                    "unsupported PDF content operator {operator} on page {page_number}"
                )));
            }
        }
    }

    if !graphics_stack.is_empty() {
        return Err(PocError::UnsupportedSemanticConstruct(format!(
            "unbalanced PDF q/Q operators on page {page_number}"
        )));
    }
    Ok(())
}

fn ensure_supported_font_selection(
    document: &Document,
    resources: &lopdf::Dictionary,
    operands: &[Object],
    page_number: u32,
) -> Result<(), PocError> {
    let (font_name, font_size) = match operands {
        [Object::Name(font_name), font_size] => (font_name, font_size),
        _ => {
            return Err(PocError::ParserDisagreement(format!(
                "lopdf page {page_number} Tf requires a font name and size"
            )));
        }
    };
    pdf_number(font_size).map_err(|_| {
        PocError::ParserDisagreement(format!(
            "lopdf page {page_number} Tf has an invalid font size"
        ))
    })?;

    let fonts = resources
        .get_deref(b"Font", document)
        .and_then(Object::as_dict)
        .map_err(|_| {
            PocError::ParserDisagreement(format!(
                "lopdf page {page_number} Font resources cannot be resolved"
            ))
        })?;
    let font = fonts
        .get_deref(font_name, document)
        .and_then(Object::as_dict)
        .map_err(|_| {
            PocError::ParserDisagreement(format!(
                "lopdf page {page_number} selected font cannot be resolved"
            ))
        })?;
    let subtype = font
        .get(b"Subtype")
        .and_then(Object::as_name)
        .map_err(|_| {
            PocError::ParserDisagreement(format!(
                "lopdf page {page_number} selected font has no valid Subtype"
            ))
        })?;
    match subtype {
        b"Type0" | b"Type1" | b"MMType1" | b"TrueType" => Ok(()),
        b"Type3" => Err(PocError::UnsupportedSemanticConstruct(format!(
            "Type 3 glyph drawing is unsupported on page {page_number}"
        ))),
        _ => Err(PocError::UnsupportedSemanticConstruct(format!(
            "selected PDF font subtype is unsupported on page {page_number}"
        ))),
    }
}

fn text_show_contains_nonempty_string(
    operation: &Operation,
    page_number: u32,
) -> Result<bool, PocError> {
    let malformed = || {
        PocError::ParserDisagreement(format!(
            "lopdf page {page_number} {} has invalid text-show operands",
            operation.operator
        ))
    };
    let string_is_nonempty = |object: &Object| match object {
        Object::String(bytes, _) => Ok(!bytes.is_empty()),
        _ => Err(malformed()),
    };

    match operation.operator.as_str() {
        "Tj" | "'" => match operation.operands.as_slice() {
            [text] => string_is_nonempty(text),
            _ => Err(malformed()),
        },
        "TJ" => match operation.operands.as_slice() {
            [Object::Array(items)] => {
                let mut contains_text = false;
                for item in items {
                    match item {
                        Object::String(bytes, _) => contains_text |= !bytes.is_empty(),
                        Object::Integer(_) | Object::Real(_) => {}
                        _ => return Err(malformed()),
                    }
                }
                Ok(contains_text)
            }
            _ => Err(malformed()),
        },
        "\"" => match operation.operands.as_slice() {
            [word_spacing, char_spacing, text] => {
                pdf_number(word_spacing).map_err(|_| malformed())?;
                pdf_number(char_spacing).map_err(|_| malformed())?;
                string_is_nonempty(text)
            }
            _ => Err(malformed()),
        },
        _ => unreachable!("only PDF text-show operators are inspected"),
    }
}

fn known_non_painting_operator(operator: &str) -> bool {
    matches!(
        operator,
        "w" | "J"
            | "j"
            | "M"
            | "d"
            | "i"
            | "m"
            | "l"
            | "c"
            | "v"
            | "y"
            | "h"
            | "re"
            | "n"
            | "CS"
            | "cs"
            | "SC"
            | "SCN"
            | "sc"
            | "scn"
            | "G"
            | "g"
            | "RG"
            | "rg"
            | "K"
            | "k"
            | "BT"
            | "ET"
            | "Tc"
            | "Tw"
            | "Tz"
            | "TL"
            | "Ts"
            | "Td"
            | "TD"
            | "Tm"
            | "T*"
            | "MP"
            | "DP"
            | "BMC"
            | "EMC"
            | "BX"
            | "EX"
    )
}

fn pdf_matrix_operands(
    operands: &[Object],
    page_number: u32,
    operator: &str,
) -> Result<[f64; 6], PocError> {
    let values = operands
        .iter()
        .map(pdf_number)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            PocError::ParserDisagreement(format!(
                "lopdf page {page_number} {operator} operands: {error}"
            ))
        })?;
    values.try_into().map_err(|values: Vec<f64>| {
        PocError::ParserDisagreement(format!(
            "lopdf page {page_number} {operator} requires six operands, got {}",
            values.len()
        ))
    })
}

fn pdf_number(object: &Object) -> Result<f64, String> {
    let value = match object {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => *value as f64,
        _ => return Err("expected a PDF number".into()),
    };
    if value.is_finite() {
        Ok(value)
    } else {
        Err("PDF number is not finite".into())
    }
}

fn invoke_pdf_xobject(
    document: &Document,
    name: &[u8],
    resources: &lopdf::Dictionary,
    page_number: u32,
    depth: usize,
    ctm: PdfCtm,
    budget: &mut PdfContentBudget,
    active_forms: &mut BTreeSet<lopdf::ObjectId>,
    clips: &mut Vec<Value>,
    events: &mut Vec<Value>,
    paint_order: &mut Vec<Value>,
) -> Result<(), PocError> {
    let xobjects = resources
        .get_deref(b"XObject", document)
        .and_then(Object::as_dict)
        .map_err(|error| {
            PocError::UnsupportedSemanticConstruct(format!(
                "invoked PDF XObject dictionary on page {page_number}: {error}"
            ))
        })?;
    let reference = xobjects.get(name).map_err(|error| {
        PocError::UnsupportedSemanticConstruct(format!(
            "invoked PDF XObject /{} is missing on page {page_number}: {error}",
            String::from_utf8_lossy(name)
        ))
    })?;
    let (object_id, resolved) = document.dereference(reference).map_err(|error| {
        PocError::ParserDisagreement(format!(
            "lopdf page {page_number} XObject /{} cannot be resolved: {error}",
            String::from_utf8_lossy(name)
        ))
    })?;
    let stream = resolved.as_stream().map_err(|error| {
        PocError::ParserDisagreement(format!(
            "lopdf page {page_number} XObject /{} is not a stream: {error}",
            String::from_utf8_lossy(name)
        ))
    })?;
    match stream.dict.get(b"Subtype").and_then(Object::as_name) {
        Ok(b"Image") => {
            if stream.dict.has(b"OC") {
                return Err(PocError::UnsupportedSemanticConstruct(format!(
                    "Image XObject optional-content visibility is unsupported on page {page_number}"
                )));
            }
            let image_hash = image_semantic_hash(document, object_id, stream, resources)?;
            events.push(json!({
                "image_sha256": image_hash,
                "ctm": ctm.projection(),
                "form_clips": clips.as_slice(),
            }));
            paint_order.push(json!("image"));
            Ok(())
        }
        Ok(b"Form") => {
            let form_id = object_id.ok_or_else(|| {
                PocError::UnsupportedSemanticConstruct(format!(
                    "lopdf page {page_number} Form XObject is not indirect"
                ))
            })?;
            if !active_forms.insert(form_id) {
                return Err(PocError::UnsupportedSemanticConstruct(format!(
                    "lopdf page {page_number} has a cyclic Form XObject"
                )));
            }
            let clip_count = clips.len();
            let result = (|| {
                if stream.dict.has(b"Group") || stream.dict.has(b"OC") {
                    return Err(PocError::UnsupportedSemanticConstruct(format!(
                        "unsupported Form XObject transparency or optional content on page {page_number}"
                    )));
                }
                let form_ctm = ctm.concat(
                    form_matrix(document, &stream.dict, page_number)?,
                    page_number,
                )?;
                let bbox = form_bbox(document, &stream.dict, page_number)?;
                clips.push(json!({"bbox": bbox, "ctm": form_ctm.projection()}));
                let form_resources = match stream.dict.get_deref(b"Resources", document) {
                    Ok(value) => value.as_dict().map_err(|error| {
                        PocError::ParserDisagreement(format!(
                            "lopdf page {page_number} Form Resources: {error}"
                        ))
                    })?,
                    Err(lopdf::Error::DictKey(_)) => resources,
                    Err(error) => {
                        return Err(PocError::ParserDisagreement(format!(
                            "lopdf page {page_number} Form Resources: {error}"
                        )));
                    }
                };
                let content =
                    decode_pdf_content_stream(stream, page_number, "Form XObject", budget)?;
                let content = Content::decode_strict(&content).map_err(|error| {
                    PocError::ParserDisagreement(format!(
                        "lopdf page {page_number} Form strict content decode: {error}"
                    ))
                })?;
                collect_painted_images(
                    document,
                    &content.operations,
                    form_resources,
                    page_number,
                    depth + 1,
                    form_ctm,
                    budget,
                    active_forms,
                    clips,
                    events,
                    paint_order,
                )
            })();
            clips.truncate(clip_count);
            active_forms.remove(&form_id);
            result
        }
        Ok(subtype) => Err(PocError::UnsupportedSemanticConstruct(format!(
            "unsupported invoked PDF XObject subtype /{} on page {page_number}",
            String::from_utf8_lossy(subtype)
        ))),
        Err(error) => Err(PocError::ParserDisagreement(format!(
            "lopdf page {page_number} XObject /{} has no valid Subtype: {error}",
            String::from_utf8_lossy(name)
        ))),
    }
}

fn form_matrix(
    document: &Document,
    dictionary: &lopdf::Dictionary,
    page_number: u32,
) -> Result<[f64; 6], PocError> {
    let matrix = match dictionary.get_deref(b"Matrix", document) {
        Ok(value) => value.as_array().map_err(|error| {
            PocError::ParserDisagreement(format!("lopdf page {page_number} Form Matrix: {error}"))
        })?,
        Err(lopdf::Error::DictKey(_)) => return Ok(PdfCtm::IDENTITY.0),
        Err(error) => {
            return Err(PocError::ParserDisagreement(format!(
                "lopdf page {page_number} Form Matrix: {error}"
            )));
        }
    };
    pdf_matrix_operands(matrix, page_number, "Form Matrix")
}

fn form_bbox(
    document: &Document,
    dictionary: &lopdf::Dictionary,
    page_number: u32,
) -> Result<Value, PocError> {
    let bbox = dictionary
        .get_deref(b"BBox", document)
        .and_then(Object::as_array)
        .map_err(|error| {
            PocError::ParserDisagreement(format!("lopdf page {page_number} Form BBox: {error}"))
        })?;
    let values = bbox
        .iter()
        .map(pdf_number)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            PocError::ParserDisagreement(format!("lopdf page {page_number} Form BBox: {error}"))
        })?;
    if values.len() != 4 {
        return Err(PocError::ParserDisagreement(format!(
            "lopdf page {page_number} Form BBox requires four numbers"
        )));
    }
    Ok(json!(
        values
            .into_iter()
            .map(|value| format!(
                "f64:{:016x}",
                if value == 0.0 { 0.0f64 } else { value }.to_bits()
            ))
            .collect::<Vec<_>>()
    ))
}

fn image_semantic_hash(
    document: &Document,
    image_id: Option<lopdf::ObjectId>,
    stream: &lopdf::Stream,
    resources: &lopdf::Dictionary,
) -> Result<String, PocError> {
    const IMAGE_KEYS: [&[u8]; 14] = [
        b"Width",
        b"Height",
        b"ColorSpace",
        b"BitsPerComponent",
        b"Decode",
        b"ImageMask",
        b"Interpolate",
        b"Intent",
        b"Mask",
        b"SMask",
        b"SMaskInData",
        b"Matte",
        b"Alternates",
        b"OC",
    ];
    let mut dictionary = serde_json::Map::new();
    let mut active_references = BTreeSet::new();
    if let Some(image_id) = image_id {
        active_references.insert(image_id);
    }
    for key in IMAGE_KEYS {
        if let Ok(value) = stream.dict.get(key) {
            dictionary.insert(
                String::from_utf8_lossy(key).into_owned(),
                normalized_pdf_object(document, value, 0, &mut active_references)?,
            );
        }
    }
    if let Ok(Object::Name(name)) = stream.dict.get(b"ColorSpace") {
        if !matches!(
            name.as_slice(),
            b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK"
        ) {
            let color_spaces = resources
                .get_deref(b"ColorSpace", document)
                .and_then(Object::as_dict)
                .map_err(|error| {
                    PocError::UnsupportedSemanticConstruct(format!(
                        "named PDF image ColorSpace cannot be resolved: {error}"
                    ))
                })?;
            let definition = color_spaces.get_deref(name, document).map_err(|error| {
                PocError::UnsupportedSemanticConstruct(format!(
                    "named PDF image ColorSpace definition: {error}"
                ))
            })?;
            dictionary.insert(
                "ResolvedColorSpace".into(),
                normalized_pdf_object(document, definition, 0, &mut active_references)?,
            );
        }
    }
    let samples = stream
        .decompressed_content_with_limit(MAX_DECOMPRESSED_STREAM)
        .map_err(pdf_stream_error)?;
    let projection = canonical_json_bytes(&json!({
        "dictionary": dictionary,
        "samples_sha256": hex::encode(Sha256::digest(samples)),
    }))
    .map_err(|error| PocError::InvalidWorkerResult(error.to_string()))?;
    Ok(hex::encode(Sha256::digest(projection)))
}

fn normalized_pdf_object(
    document: &Document,
    object: &Object,
    depth: usize,
    active_references: &mut BTreeSet<lopdf::ObjectId>,
) -> Result<Value, PocError> {
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(PocError::InspectionResourceLimitExceeded);
    }
    match object {
        Object::Null => Ok(Value::Null),
        Object::Boolean(value) => Ok(json!(value)),
        Object::Integer(value) => Ok(json!(value)),
        Object::Real(value) if value.is_finite() => {
            Ok(Value::String(format!("f32:{:08x}", value.to_bits())))
        }
        Object::Real(_) => Err(PocError::UnsupportedSemanticConstruct(
            "non-finite PDF image number".into(),
        )),
        Object::Name(value) => Ok(json!({"name_hex": hex::encode(value)})),
        Object::String(value, _) => Ok(json!({"string_hex": hex::encode(value)})),
        Object::Reference(id) => {
            if !active_references.insert(*id) {
                return Err(PocError::UnsupportedSemanticConstruct(
                    "PDF image reference cycle".into(),
                ));
            }
            let result = document
                .get_object(*id)
                .map_err(|error| PocError::ParserDisagreement(error.to_string()))
                .and_then(|value| {
                    normalized_pdf_object(document, value, depth + 1, active_references)
                });
            active_references.remove(id);
            result
        }
        Object::Array(values) => values
            .iter()
            .map(|value| normalized_pdf_object(document, value, depth + 1, active_references))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Object::Dictionary(dictionary) => {
            normalized_pdf_dictionary(document, dictionary, depth + 1, active_references, false)
        }
        Object::Stream(stream) => {
            let dictionary = normalized_pdf_dictionary(
                document,
                &stream.dict,
                depth + 1,
                active_references,
                true,
            )?;
            let samples = stream
                .decompressed_content_with_limit(MAX_DECOMPRESSED_STREAM)
                .map_err(pdf_stream_error)?;
            Ok(json!({
                "dictionary": dictionary,
                "samples_sha256": hex::encode(Sha256::digest(samples)),
            }))
        }
    }
}

fn normalized_pdf_dictionary(
    document: &Document,
    dictionary: &lopdf::Dictionary,
    depth: usize,
    active_references: &mut BTreeSet<lopdf::ObjectId>,
    omit_stream_encoding: bool,
) -> Result<Value, PocError> {
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(PocError::InspectionResourceLimitExceeded);
    }
    let mut normalized = serde_json::Map::new();
    for (key, value) in dictionary.iter() {
        if omit_stream_encoding && matches!(key.as_slice(), b"Length" | b"Filter" | b"DecodeParms")
        {
            continue;
        }
        normalized.insert(
            hex::encode(key),
            normalized_pdf_object(document, value, depth + 1, active_references)?,
        );
    }
    Ok(Value::Object(normalized))
}

fn pdf_stream_error(error: lopdf::Error) -> PocError {
    match error {
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. }) => {
            PocError::InspectionResourceLimitExceeded
        }
        other => PocError::SemanticExtractionFailed(other.to_string()),
    }
}

fn pdfium_artifact_sha256() -> &'static str {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b"
    } else {
        "unsupported-platform"
    }
}
