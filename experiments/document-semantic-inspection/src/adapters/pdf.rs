use crate::{
    AdapterOutput, CapabilityEvidence, CommentEvidence, Diagnostic, EditorialEvidence,
    ExternalDependency, FormatId, InspectionAdapter, InspectionProfile, PocError,
    canonical_json_bytes,
};
use lopdf::content::Content;
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
    image_hashes: Vec<Vec<String>>,
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
                        format!("action:{:?}", action.action_type()).to_lowercase()
                    }
                } else {
                    "none".to_owned()
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

            let mut image_hashes = structural.image_hashes[page_index].clone();
            image_hashes.sort();
            total_images += image_hashes.len();

            semantic_pages.push(json!({
                "index": page_index,
                "text": text,
                "links": links,
                "images": image_hashes,
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
    let mut form_names = BTreeSet::new();

    for (page_number, page_id) in pages {
        let page_content = document
            .get_page_content_with_limit(page_id, MAX_DECOMPRESSED_STREAM)
            .map_err(pdf_stream_error)?;
        reject_inline_images(&page_content, page_number)?;
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
        image_hashes.push(page_image_hashes(document, page_id, page_number)?);
    }

    Ok(LopdfFacts {
        page_count: annotation_counts.len(),
        annotation_counts,
        link_counts,
        form_field_count: form_names.len(),
        image_hashes,
    })
}

fn page_image_hashes(
    document: &Document,
    page_id: lopdf::ObjectId,
    page_number: u32,
) -> Result<Vec<String>, PocError> {
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
    let Some(resources) = resources else {
        return Ok(Vec::new());
    };
    let mut hashes = Vec::new();
    let mut active_forms = BTreeSet::new();
    collect_resource_images(
        document,
        resources,
        page_number,
        0,
        &mut active_forms,
        &mut hashes,
    )?;
    hashes.sort();
    Ok(hashes)
}

fn collect_resource_images(
    document: &Document,
    resources: &lopdf::Dictionary,
    page_number: u32,
    depth: usize,
    active_forms: &mut BTreeSet<lopdf::ObjectId>,
    hashes: &mut Vec<String>,
) -> Result<(), PocError> {
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(PocError::InspectionResourceLimitExceeded);
    }
    let xobjects = match resources.get_deref(b"XObject", document) {
        Ok(value) => value.as_dict().map_err(|error| {
            PocError::ParserDisagreement(format!(
                "lopdf page {page_number} XObject dictionary: {error}"
            ))
        })?,
        Err(lopdf::Error::DictKey(_)) => return Ok(()),
        Err(error) => {
            return Err(PocError::ParserDisagreement(format!(
                "lopdf page {page_number} XObject dictionary: {error}"
            )));
        }
    };
    for (_, object) in xobjects.iter() {
        let (object_id, resolved) = document.dereference(object).map_err(|error| {
            PocError::SemanticExtractionFailed(format!("lopdf page {page_number} XObject: {error}"))
        })?;
        let stream = resolved.as_stream().map_err(|error| {
            PocError::ParserDisagreement(format!(
                "lopdf page {page_number} XObject stream: {error}"
            ))
        })?;
        let subtype = stream.dict.get(b"Subtype").and_then(Object::as_name).ok();
        match subtype {
            Some(b"Image") => {
                hashes.push(image_semantic_hash(document, object_id, stream, resources)?)
            }
            Some(b"Form") => {
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
                let form_content = stream
                    .decompressed_content_with_limit(MAX_DECOMPRESSED_STREAM)
                    .map_err(pdf_stream_error)?;
                reject_inline_images(&form_content, page_number)?;
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
                let result = collect_resource_images(
                    document,
                    form_resources,
                    page_number,
                    depth + 1,
                    active_forms,
                    hashes,
                );
                active_forms.remove(&form_id);
                result?;
            }
            _ => {}
        }
    }
    Ok(())
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

fn reject_inline_images(content: &[u8], page_number: u32) -> Result<(), PocError> {
    if !content.windows(2).any(|window| window == b"BI") {
        return Ok(());
    }
    let operations = Content::decode_strict(content).map_err(|error| {
        PocError::ParserDisagreement(format!(
            "lopdf page {page_number} content operations: {error}"
        ))
    })?;
    if operations
        .operations
        .iter()
        .any(|operation| operation.operator == "BI")
    {
        return Err(PocError::UnsupportedSemanticConstruct(format!(
            "lopdf page {page_number} inline images are unsupported"
        )));
    }
    Ok(())
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
