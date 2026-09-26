use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::OnceLock;

use document_semantic_inspection_core::{
    CapabilityState, CommentEvidence, EditorialProvenance, ExternalDependency, FormatId,
    NativeDependencyIdentity,
};
use lopdf::content::Content;
use lopdf::{Document, LoadOptions, Object};
use pdfium_render::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{WorkerFailure, WorkerFailureCode};

use super::{AdapterProfile, SemanticAdapter, SemanticAdapterOutput, canonical_json_bytes};

const PDFIUM_RELEASE: &str = "151.0.7881.0";
const MAX_DECOMPRESSED_STREAM: usize = 64 * 1024 * 1024;
const MAX_PDF_OBJECT_DEPTH: usize = 64;
const MAX_PDF_CONTENT_OPERATIONS: usize = 1_000_000;
const MAX_PDF_PAINT_EVENTS: usize = 100_000;

const PARSER_LIBRARIES: [(&str, &str); 2] = [("pdfium-render", "0.9.4"), ("lopdf", "0.45.0")];

static PDFIUM: OnceLock<Result<PdfiumRuntime, String>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default)]
pub struct PdfAdapter;

struct PdfiumRuntime {
    engine: Pdfium,
    sha256: [u8; 32],
}

#[derive(Debug)]
struct LopdfFacts {
    page_count: usize,
    annotation_counts: Vec<usize>,
    link_counts: Vec<usize>,
    form_field_count: usize,
    image_paints: Vec<Vec<Value>>,
    paint_orders: Vec<Vec<&'static str>>,
}

impl SemanticAdapter for PdfAdapter {
    fn format(&self) -> FormatId {
        FormatId::Pdf
    }

    fn inspect(
        &self,
        input: &[u8],
        _profile: &AdapterProfile,
    ) -> Result<SemanticAdapterOutput, WorkerFailure> {
        let lopdf = load_lopdf(input)?;
        if lopdf.is_encrypted() || lopdf.was_encrypted() {
            return Err(failure(
                WorkerFailureCode::EncryptedContentUnsupported,
                "encrypted PDF content is unsupported",
            ));
        }
        let structural = extract_lopdf_facts(&lopdf)?;
        let pdfium = pdfium()?;
        let document = pdfium
            .engine
            .load_pdf_from_byte_slice(input, None)
            .map_err(map_pdfium_open_error)?;

        let pages = document.pages();
        let page_count = pages.len() as usize;
        if page_count != structural.page_count {
            return Err(failure(
                WorkerFailureCode::ParserDisagreement,
                format!(
                    "page count mismatch: pdfium={page_count}, lopdf={}",
                    structural.page_count
                ),
            ));
        }

        let form_values: BTreeMap<String, Option<String>> = document
            .form()
            .map(|form| form.field_values(pages).into_iter().collect())
            .unwrap_or_default();
        if form_values.len() != structural.form_field_count {
            return Err(failure(
                WorkerFailureCode::ParserDisagreement,
                format!(
                    "form field count mismatch: pdfium={}, lopdf={}",
                    form_values.len(),
                    structural.form_field_count
                ),
            ));
        }

        let mut semantic_pages = Vec::with_capacity(page_count);
        let mut external_dependencies = Vec::new();
        let mut editorial = EditorialProvenance::default();
        let mut any_text = false;
        let mut total_images = 0usize;

        for (page_index, page) in pages.iter().enumerate() {
            validate_pdfium_read_order(&page, page_index)?;

            let text = page
                .text()
                .map_err(|error| {
                    failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        format!("PDFium text extraction on page {page_index}: {error}"),
                    )
                })?
                .all();
            if !text.trim().is_empty() {
                any_text = true;
            }

            let annotations = page.annotations();
            let annotation_count = annotations.len();
            for (annotation_index, annotation) in annotations.iter().enumerate() {
                let annotation_type = annotation.annotation_type();
                if !matches!(
                    annotation_type,
                    PdfPageAnnotationType::Link
                        | PdfPageAnnotationType::Widget
                        | PdfPageAnnotationType::XfaWidget
                ) {
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
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!(
                        "annotation count mismatch on page {}: pdfium={annotation_count}, lopdf={}",
                        page_index + 1,
                        structural.annotation_counts[page_index]
                    ),
                ));
            }

            let mut links = Vec::new();
            for (link_index, link) in page.links().iter().enumerate() {
                let target = if let Some(action) = link.action() {
                    if let Some(uri) = action.as_uri_action() {
                        let uri = uri.uri().map_err(|error| {
                            failure(
                                WorkerFailureCode::SemanticExtractionFailed,
                                format!("PDFium URI link on page {page_index}: {error}"),
                            )
                        })?;
                        external_dependencies.push(ExternalDependency {
                            dependency_kind: "uri".into(),
                            normalized_reference: uri.clone(),
                            source_locator: format!("page[{page_index}]/link[{link_index}]"),
                            version_significant: true,
                        });
                        format!("uri:{uri}")
                    } else if let Some(local) = action.as_local_destination_action() {
                        let destination = local.destination().map_err(|error| {
                            failure(
                                WorkerFailureCode::SemanticExtractionFailed,
                                format!("PDFium local destination on page {page_index}: {error}"),
                            )
                        })?;
                        let destination_page = destination.page_index().map_err(|error| {
                            failure(
                                WorkerFailureCode::SemanticExtractionFailed,
                                format!("PDFium local destination page index: {error}"),
                            )
                        })?;
                        format!("page:{destination_page}")
                    } else {
                        return Err(failure(
                            WorkerFailureCode::UnsupportedSemanticConstruct,
                            format!(
                                "unsupported PDF link action on page {}: {:?}",
                                page_index + 1,
                                action.action_type()
                            ),
                        ));
                    }
                } else if let Some(destination) = link.destination() {
                    let destination_page = destination.page_index().map_err(|error| {
                        failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            format!("PDFium link destination page index: {error}"),
                        )
                    })?;
                    format!("page:{destination_page}")
                } else {
                    return Err(failure(
                        WorkerFailureCode::UnsupportedSemanticConstruct,
                        format!(
                            "PDF link has no supported target on page {}",
                            page_index + 1
                        ),
                    ));
                };
                links.push(target);
            }
            links.sort();
            if links.len() != structural.link_counts[page_index] {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!(
                        "link count mismatch on page {}: pdfium={}, lopdf={}",
                        page_index + 1,
                        links.len(),
                        structural.link_counts[page_index]
                    ),
                ));
            }

            let image_paints = structural.image_paints[page_index].clone();
            total_images = total_images.saturating_add(image_paints.len());

            semantic_pages.push(json!({
                "index": page_index,
                "text": text,
                "links": links,
                "images": image_paints,
                "paint_order": structural.paint_orders[page_index],
            }));
        }

        if !any_text && total_images > 0 {
            return Err(failure(
                WorkerFailureCode::RequiresOcr,
                "PDF contains images but no extractable text",
            ));
        }

        external_dependencies.sort_by(|left, right| {
            (
                left.dependency_kind.as_str(),
                left.normalized_reference.as_str(),
                left.source_locator.as_str(),
            )
                .cmp(&(
                    right.dependency_kind.as_str(),
                    right.normalized_reference.as_str(),
                    right.source_locator.as_str(),
                ))
        });
        external_dependencies.dedup_by(|left, right| {
            left.dependency_kind == right.dependency_kind
                && left.normalized_reference == right.normalized_reference
        });

        let semantic_projection = canonical_json_bytes(&json!({
            "pages": semantic_pages,
            "form_values": form_values,
        }))
        .map_err(|error| {
            failure(
                WorkerFailureCode::InvalidWorkerResult,
                format!("PDF semantic projection: {error}"),
            )
        })?;

        let mut output = SemanticAdapterOutput::from_projection(
            &semantic_projection,
            &[
                "reader_content",
                "form_fields",
                "annotations",
                "visual_content",
                "formula_logic",
                "vba_logic",
                "hidden_content",
            ],
            "pdf",
            &PARSER_LIBRARIES,
        )
        .with_editorial_provenance(editorial);
        output = output.with_external_dependencies(external_dependencies);

        if !any_text {
            output = output.with_capability_state("reader_content", CapabilityState::Absent)?;
        }
        if structural.form_field_count == 0 {
            output = output.with_capability_state("form_fields", CapabilityState::Absent)?;
        }
        if !structural.annotation_counts.iter().any(|count| *count > 0) {
            output = output.with_capability_state("annotations", CapabilityState::Absent)?;
        }
        if total_images == 0 {
            output = output.with_capability_state("visual_content", CapabilityState::Absent)?;
        }
        output =
            output.with_capability_state("formula_logic", CapabilityState::NotRepresentable)?;
        output = output.with_capability_state("vba_logic", CapabilityState::NotRepresentable)?;
        output = output.with_capability_state("hidden_content", CapabilityState::NotVerifiable)?;
        if let Some(annotations) = output
            .semantic_capabilities
            .iter_mut()
            .find(|capability| capability.capability_id == "annotations")
        {
            annotations.version_significant = false;
            annotations.equivalence_fingerprint = None;
        }

        output
            .extractor_provenance
            .native_dependency_identity
            .push(NativeDependencyIdentity {
                name: "pdfium".into(),
                version: Some(PDFIUM_RELEASE.into()),
                sha256: Some(pdfium.sha256),
            });

        Ok(output)
    }
}

fn pdfium() -> Result<&'static PdfiumRuntime, WorkerFailure> {
    match PDFIUM.get_or_init(|| {
        let directory = env::var("PDFIUM_DYNAMIC_LIB_PATH")
            .map_err(|_| "PDFIUM_DYNAMIC_LIB_PATH is not set".to_owned())?;
        let path = Pdfium::pdfium_platform_library_name_at_path(Path::new(&directory));
        let observed_hash = sha256_file(&path).map_err(|error| {
            format!("unable to verify the PDFium native binary: {}", error.kind())
        })?;
        let expected_hash = expected_pdfium_sha256().ok_or_else(|| {
            "no qualified PDFium native binary exists for this target platform".to_owned()
        })?;
        let observed_hex = format_sha256(&observed_hash);
        if observed_hex != expected_hash {
            return Err(format!(
                "PDFium native binary SHA-256 mismatch: expected {expected_hash}, observed {observed_hex}"
            ));
        }

        let bindings = Pdfium::bind_to_library(&path).map_err(|error| {
            format!("unable to bind the verified PDFium native binary: {error}")
        })?;
        Ok(PdfiumRuntime {
            engine: Pdfium::new(bindings),
            sha256: observed_hash,
        })
    }) {
        Ok(runtime) => Ok(runtime),
        Err(message) => Err(failure(
            WorkerFailureCode::ExtractorUnavailable,
            format!("PDFium {PDFIUM_RELEASE}: {message}"),
        )),
    }
}

fn expected_pdfium_sha256() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("4eaad6c3e8d786cf6f66a45d7d014edf5c65f372f98c3070e66595ebb50e43d9")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7")
    } else {
        None
    }
}

fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().into())
}

fn format_sha256(hash: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in hash {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn load_lopdf(input: &[u8]) -> Result<Document, WorkerFailure> {
    let options = LoadOptions {
        strict: true,
        max_decompressed_size: Some(MAX_DECOMPRESSED_STREAM),
        ..Default::default()
    };
    Document::load_mem_with_options(input, options).map_err(map_lopdf_open_error)
}

fn map_lopdf_open_error(error: lopdf::Error) -> WorkerFailure {
    match error {
        lopdf::Error::InvalidPassword
        | lopdf::Error::UnsupportedSecurityHandler(_)
        | lopdf::Error::Decryption(_) => failure(
            WorkerFailureCode::EncryptedContentUnsupported,
            "encrypted PDF content is unsupported",
        ),
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. }) => failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "PDF stream exceeded the configured 64 MiB decompression limit",
        ),
        other => failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("lopdf strict load: {other}"),
        ),
    }
}

fn map_lopdf_stream_error(error: lopdf::Error, context: &str) -> WorkerFailure {
    match error {
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. }) => failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!("PDF {context} exceeded the configured 64 MiB decompression limit"),
        ),
        other => failure(
            WorkerFailureCode::SemanticExtractionFailed,
            format!("PDF {context}: {other}"),
        ),
    }
}

fn map_pdfium_open_error(error: PdfiumError) -> WorkerFailure {
    failure(
        WorkerFailureCode::SemanticExtractionFailed,
        format!("PDFium open: {error}"),
    )
}

fn validate_pdfium_read_order(page: &PdfPage<'_>, page_index: usize) -> Result<(), WorkerFailure> {
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
                failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("PDFium text bounds on page {page_index}: {error}"),
                )
            })?
            .to_rect();
        text_regions.push((text, bounds));
    }

    for left in 0..text_regions.len() {
        for right in (left + 1)..text_regions.len() {
            let (left_text, left_bounds) = &text_regions[left];
            let (right_text, right_bounds) = &text_regions[right];
            if left_text != right_text && left_bounds.does_overlap(right_bounds) {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("ambiguous PDF text/read order on page {}", page_index + 1),
                ));
            }
        }
    }
    Ok(())
}

fn extract_lopdf_facts(document: &Document) -> Result<LopdfFacts, WorkerFailure> {
    let pages = document.get_pages();
    let mut annotation_counts = Vec::with_capacity(pages.len());
    let mut link_counts = Vec::with_capacity(pages.len());
    let mut image_paints = Vec::with_capacity(pages.len());
    let mut paint_orders = Vec::with_capacity(pages.len());
    let mut form_names = BTreeSet::new();

    for (page_number, page_id) in pages {
        let mut decode_budget = PdfDecodeBudget::default();
        let page_content = decode_page_content(document, page_id, page_number, &mut decode_budget)?;
        let operations = decode_content_operations(&page_content, page_number, "page")?;
        let resources = inherited_page_resources(document, page_id, page_number)?;
        let mut paint_context = PdfPaintContext {
            document,
            page_number,
            active_forms: BTreeSet::new(),
            paint_events: Vec::new(),
            paint_order: Vec::new(),
            operations_seen: 0,
            decode_budget,
        };
        collect_content_paints(
            &mut paint_context,
            &operations,
            resources,
            0,
            &mut PdfGraphicsState::default(),
        )?;
        let page = document.get_dictionary(page_id).map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("lopdf page {page_number}: {error}"),
            )
        })?;
        let mut annotation_count = 0usize;
        let mut link_count = 0usize;

        match page.get_deref(b"Annots", document) {
            Ok(annots) => {
                let annots = annots.as_array().map_err(|error| {
                    failure(
                        WorkerFailureCode::ParserDisagreement,
                        format!("lopdf page {page_number} Annots is not an array: {error}"),
                    )
                })?;
                annotation_count = annots.len();

                for annotation in annots {
                    let (_, resolved) = document.dereference(annotation).map_err(|error| {
                        failure(
                            WorkerFailureCode::ParserDisagreement,
                            format!(
                                "lopdf page {page_number} annotation cannot be resolved: {error}"
                            ),
                        )
                    })?;
                    let dict = resolved.as_dict().map_err(|error| {
                        failure(
                            WorkerFailureCode::ParserDisagreement,
                            format!(
                                "lopdf page {page_number} annotation is not a dictionary: {error}"
                            ),
                        )
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
                                    failure(
                                        WorkerFailureCode::ParserDisagreement,
                                        format!("lopdf form parent cannot be resolved: {error}"),
                                    )
                                })?
                                .1
                                .as_dict()
                                .map_err(|error| {
                                    failure(
                                        WorkerFailureCode::ParserDisagreement,
                                        format!("lopdf form parent is not a dictionary: {error}"),
                                    )
                                })?,
                            Err(_) => dict,
                        };
                        if let Ok(name) = field.get(b"T").and_then(Object::as_str) {
                            form_names.insert(String::from_utf8_lossy(name).into_owned());
                        }
                    }
                }
            }
            Err(lopdf::Error::DictKey(_)) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!("lopdf page {page_number} Annots cannot be read: {error}"),
                ));
            }
        }

        annotation_counts.push(annotation_count);
        link_counts.push(link_count);
        image_paints.push(paint_context.paint_events);
        paint_orders.push(paint_context.paint_order);
    }

    Ok(LopdfFacts {
        page_count: annotation_counts.len(),
        annotation_counts,
        link_counts,
        form_field_count: form_names.len(),
        image_paints,
        paint_orders,
    })
}

#[derive(Debug, Default)]
struct PdfDecodeBudget {
    used: usize,
}

impl PdfDecodeBudget {
    fn remaining(&self) -> usize {
        MAX_DECOMPRESSED_STREAM.saturating_sub(self.used)
    }

    fn charge(&mut self, amount: usize, context: &str) -> Result<(), WorkerFailure> {
        if amount > self.remaining() {
            return Err(failure(
                WorkerFailureCode::InspectionResourceLimitExceeded,
                format!("PDF {context} exceeded the configured 64 MiB decode budget"),
            ));
        }
        self.used += amount;
        Ok(())
    }
}

#[derive(Clone)]
struct PdfGraphicsState {
    ctm: [f64; 6],
    clips: Vec<PdfClip>,
    text_render_mode: i64,
}

impl Default for PdfGraphicsState {
    fn default() -> Self {
        Self {
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            clips: Vec::new(),
            text_render_mode: 0,
        }
    }
}

#[derive(Clone)]
struct PdfClip {
    bbox: [f64; 4],
    ctm: [f64; 6],
}

struct PdfPaintContext<'a> {
    document: &'a Document,
    page_number: u32,
    active_forms: BTreeSet<lopdf::ObjectId>,
    paint_events: Vec<Value>,
    paint_order: Vec<&'static str>,
    operations_seen: usize,
    decode_budget: PdfDecodeBudget,
}

fn decode_page_content(
    document: &Document,
    page_id: lopdf::ObjectId,
    page_number: u32,
    budget: &mut PdfDecodeBudget,
) -> Result<Vec<u8>, WorkerFailure> {
    let page = document.get_dictionary(page_id).map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("lopdf page {page_number} dictionary: {error}"),
        )
    })?;
    let contents = match page.get(b"Contents") {
        Ok(contents) => contents,
        Err(lopdf::Error::DictKey(_)) => return Ok(Vec::new()),
        Err(error) => {
            return Err(failure(
                WorkerFailureCode::ParserDisagreement,
                format!("lopdf page {page_number} Contents cannot be read: {error}"),
            ));
        }
    };

    let mut stream_ids = Vec::new();
    let mut active_references = BTreeSet::new();
    collect_content_stream_ids(
        document,
        contents,
        page_number,
        0,
        &mut active_references,
        &mut stream_ids,
    )?;

    let mut content = Vec::new();
    for stream_id in stream_ids {
        let stream = document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .map_err(|error| {
                failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!("lopdf page {page_number} Contents stream: {error}"),
                )
            })?;
        let decoded = stream
            .decompressed_content_with_limit(budget.remaining())
            .map_err(|error| map_required_content_error(error, page_number, "Contents"))?;
        budget.charge(
            decoded.len().saturating_add(1),
            &format!("page {page_number} Contents"),
        )?;
        content.extend_from_slice(&decoded);
        content.push(b'\n');
    }
    Ok(content)
}

fn collect_content_stream_ids(
    document: &Document,
    object: &Object,
    page_number: u32,
    depth: usize,
    active_references: &mut BTreeSet<lopdf::ObjectId>,
    streams: &mut Vec<lopdf::ObjectId>,
) -> Result<(), WorkerFailure> {
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!("lopdf page {page_number} Contents exceeded the object depth limit"),
        ));
    }

    match object {
        Object::Reference(object_id) => {
            if !active_references.insert(*object_id) {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!("lopdf page {page_number} Contents has a reference cycle"),
                ));
            }
            let referenced = document.get_object(*object_id).map_err(|error| {
                failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!("lopdf page {page_number} Contents reference: {error}"),
                )
            });
            let result = match referenced {
                Ok(Object::Stream(_)) => {
                    streams.push(*object_id);
                    Ok(())
                }
                Ok(referenced) => collect_content_stream_ids(
                    document,
                    referenced,
                    page_number,
                    depth + 1,
                    active_references,
                    streams,
                ),
                Err(error) => Err(error),
            };
            active_references.remove(object_id);
            result
        }
        Object::Array(values) => {
            for value in values {
                collect_content_stream_ids(
                    document,
                    value,
                    page_number,
                    depth + 1,
                    active_references,
                    streams,
                )?;
            }
            Ok(())
        }
        _ => Err(failure(
            WorkerFailureCode::ParserDisagreement,
            format!("lopdf page {page_number} Contents value is not a stream or stream array"),
        )),
    }
}

fn map_required_content_error(
    error: lopdf::Error,
    page_number: u32,
    context: &str,
) -> WorkerFailure {
    match error {
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. }) => failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!(
                "PDF page {page_number} {context} exceeded the configured 64 MiB decode budget"
            ),
        ),
        other => failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} {context} cannot be decoded strictly: {other}"),
        ),
    }
}

fn decode_content_operations(
    bytes: &[u8],
    page_number: u32,
    context: &str,
) -> Result<Vec<lopdf::content::Operation>, WorkerFailure> {
    let content = Content::decode_strict(bytes).map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("lopdf page {page_number} {context} operations: {error}"),
        )
    })?;
    enforce_operation_limit(content.operations.len(), page_number, context)?;
    Ok(content.operations)
}

fn enforce_operation_limit(
    count: usize,
    page_number: u32,
    context: &str,
) -> Result<(), WorkerFailure> {
    if count > MAX_PDF_CONTENT_OPERATIONS {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!("PDF page {page_number} {context} exceeded the operation limit"),
        ));
    }
    Ok(())
}

fn ensure_image_paint_slot(count: usize, page_number: u32) -> Result<(), WorkerFailure> {
    if count >= MAX_PDF_PAINT_EVENTS {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!("PDF page {page_number} exceeded the image paint-event limit"),
        ));
    }
    Ok(())
}

fn collect_content_paints(
    context: &mut PdfPaintContext<'_>,
    operations: &[lopdf::content::Operation],
    resources: Option<&lopdf::Dictionary>,
    depth: usize,
    state: &mut PdfGraphicsState,
) -> Result<(), WorkerFailure> {
    let page_number = context.page_number;
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!("PDF page {page_number} Form XObjects exceeded the depth limit"),
        ));
    }

    let mut saved_states = Vec::new();
    for operation in operations {
        context.operations_seen = context.operations_seen.saturating_add(1);
        enforce_operation_limit(context.operations_seen, page_number, "operation traversal")?;

        match operation.operator.as_str() {
            "q" if operation.operands.is_empty() => {
                if saved_states.len() >= MAX_PDF_OBJECT_DEPTH {
                    return Err(failure(
                        WorkerFailureCode::InspectionResourceLimitExceeded,
                        format!(
                            "PDF page {page_number} graphics-state stack exceeded the depth limit"
                        ),
                    ));
                }
                saved_states.push(state.clone());
            }
            "Q" if operation.operands.is_empty() => {
                *state = saved_states.pop().ok_or_else(|| {
                    failure(
                        WorkerFailureCode::ParserDisagreement,
                        format!("PDF page {page_number} has an unmatched Q operator"),
                    )
                })?;
            }
            "cm" => {
                let matrix = parse_pdf_matrix(&operation.operands, page_number)?;
                state.ctm = multiply_pdf_matrices(matrix, state.ctm);
                validate_finite_matrix(&state.ctm, page_number)?;
            }
            "Do" => {
                let [Object::Name(name)] = operation.operands.as_slice() else {
                    return Err(failure(
                        WorkerFailureCode::ParserDisagreement,
                        format!("PDF page {page_number} Do operator has invalid operands"),
                    ));
                };
                collect_xobject_paint(context, resources, name, depth, state)?;
            }
            "Tr" => {
                state.text_render_mode = parse_text_render_mode(operation, page_number)?;
            }
            "Tf" => {
                validate_selected_font(context.document, resources, operation, page_number)?;
            }
            "Tj" | "TJ" | "'" | "\"" => {
                if text_show_has_bytes(operation, page_number)?
                    && state.text_render_mode != 3
                    && context.paint_order.last().copied() != Some("text")
                {
                    context.paint_order.push("text");
                }
            }
            "BT" | "ET" | "Tc" | "Tw" | "Tz" | "TL" | "Ts" | "Td" | "TD" | "Tm" | "T*" => {}
            operator => {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("unsupported PDF page {page_number} content operator: {operator}"),
                ));
            }
        }
    }
    if !saved_states.is_empty() {
        return Err(failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} has an unmatched q operator"),
        ));
    }
    Ok(())
}

fn parse_text_render_mode(
    operation: &lopdf::content::Operation,
    page_number: u32,
) -> Result<i64, WorkerFailure> {
    let [mode] = operation.operands.as_slice() else {
        return Err(failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} Tr operator has invalid operands"),
        ));
    };
    let mode = mode.as_i64().map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} Tr mode is invalid: {error}"),
        )
    })?;
    if !(0..=3).contains(&mode) {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("PDF page {page_number} text clipping mode is unsupported"),
        ));
    }
    Ok(mode)
}

fn validate_selected_font(
    document: &Document,
    resources: Option<&lopdf::Dictionary>,
    operation: &lopdf::content::Operation,
    page_number: u32,
) -> Result<(), WorkerFailure> {
    let [Object::Name(name), size] = operation.operands.as_slice() else {
        return Err(failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} Tf operator has invalid operands"),
        ));
    };
    numeric_values(std::slice::from_ref(size), 1, page_number, "font size")?;
    let resources = resources.ok_or_else(|| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} selected font has no scoped Resources"),
        )
    })?;
    let fonts = resources
        .get_deref(b"Font", document)
        .and_then(Object::as_dict)
        .map_err(|error| {
            failure(
                WorkerFailureCode::ParserDisagreement,
                format!("PDF page {page_number} Font resources cannot be resolved: {error}"),
            )
        })?;
    let font = fonts.get(name).map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} selected font cannot be resolved: {error}"),
        )
    })?;
    let (_, resolved) = document.dereference(font).map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} selected font reference is invalid: {error}"),
        )
    })?;
    let font = resolved.as_dict().map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} selected font is not a dictionary: {error}"),
        )
    })?;
    let subtype = font
        .get_deref(b"Subtype", document)
        .and_then(Object::as_name)
        .map_err(|error| {
            failure(
                WorkerFailureCode::ParserDisagreement,
                format!("PDF page {page_number} selected font has no valid Subtype: {error}"),
            )
        })?;
    match subtype {
        b"Type0" | b"Type1" | b"MMType1" | b"TrueType" => Ok(()),
        b"Type3" => Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("PDF page {page_number} Type 3 glyph drawing is unsupported"),
        )),
        _ => Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("PDF page {page_number} selected font subtype is unsupported"),
        )),
    }
}

fn text_show_has_bytes(
    operation: &lopdf::content::Operation,
    page_number: u32,
) -> Result<bool, WorkerFailure> {
    let malformed = || {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!(
                "PDF page {page_number} {} operator has invalid operands",
                operation.operator
            ),
        )
    };
    match (operation.operator.as_str(), operation.operands.as_slice()) {
        ("Tj" | "'", [Object::String(bytes, _)]) => Ok(!bytes.is_empty()),
        (
            "\"",
            [
                Object::Integer(_) | Object::Real(_),
                Object::Integer(_) | Object::Real(_),
                Object::String(bytes, _),
            ],
        ) => {
            numeric_values(&operation.operands[..2], 2, page_number, "text spacing")?;
            Ok(!bytes.is_empty())
        }
        ("TJ", [Object::Array(parts)]) => {
            let mut has_bytes = false;
            for part in parts {
                match part {
                    Object::String(bytes, _) => has_bytes |= !bytes.is_empty(),
                    Object::Integer(_) => {}
                    Object::Real(value) if value.is_finite() => {}
                    _ => return Err(malformed()),
                }
            }
            Ok(has_bytes)
        }
        _ => Err(malformed()),
    }
}

fn collect_xobject_paint(
    context: &mut PdfPaintContext<'_>,
    resources: Option<&lopdf::Dictionary>,
    name: &[u8],
    depth: usize,
    state: &PdfGraphicsState,
) -> Result<(), WorkerFailure> {
    let document = context.document;
    let page_number = context.page_number;
    let resources = resources.ok_or_else(|| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} Do operator has no scoped Resources"),
        )
    })?;
    let xobjects = resources
        .get_deref(b"XObject", document)
        .and_then(Object::as_dict)
        .map_err(|error| {
            failure(
                WorkerFailureCode::ParserDisagreement,
                format!("PDF page {page_number} XObject resources: {error}"),
            )
        })?;
    let object = xobjects.get(name).map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!(
                "PDF page {page_number} Do resource /{} cannot be resolved: {error}",
                String::from_utf8_lossy(name)
            ),
        )
    })?;
    let (object_id, resolved) = document.dereference(object).map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} Do XObject reference: {error}"),
        )
    })?;
    let object_id = object_id.ok_or_else(|| {
        failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("PDF page {page_number} Do XObject must be an indirect stream"),
        )
    })?;
    let stream = resolved.as_stream().map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} Do XObject is not a stream: {error}"),
        )
    })?;
    let subtype = stream
        .dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .map_err(|error| {
            failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!("PDF page {page_number} Do XObject has no supported Subtype: {error}"),
            )
        })?;

    match subtype {
        b"Image" => {
            if stream.dict.has(b"OC") {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!(
                        "PDF page {page_number} Image XObject optional-content visibility is unsupported"
                    ),
                ));
            }
            ensure_image_paint_slot(context.paint_events.len(), page_number)?;
            let image_hash = image_semantic_hash(
                document,
                Some(object_id),
                stream,
                resources,
                page_number,
                &mut context.decode_budget,
            )?;
            let ctm = normalize_matrix(state.ctm);
            let clips = state
                .clips
                .iter()
                .map(|clip| {
                    json!({
                        "bbox": clip.bbox,
                        "ctm": normalize_matrix(clip.ctm),
                    })
                })
                .collect::<Vec<_>>();
            context.paint_events.push(json!({
                "image_sha256": image_hash,
                "ctm": ctm,
                "clips": clips,
            }));
            context.paint_order.push("image");
            Ok(())
        }
        b"Form" => {
            if stream.dict.has(b"Group") || stream.dict.has(b"OC") {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!(
                        "PDF page {page_number} Form XObject group/visibility semantics are unsupported"
                    ),
                ));
            }
            if !context.active_forms.insert(object_id) {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("PDF page {page_number} has a cyclic Form XObject"),
                ));
            }
            let result = (|| {
                let bbox = form_bbox(document, stream, page_number)?;
                let form_matrix = form_matrix(document, stream, page_number)?;
                let mut form_state = state.clone();
                form_state.ctm = multiply_pdf_matrices(form_matrix, form_state.ctm);
                validate_finite_matrix(&form_state.ctm, page_number)?;
                form_state.clips.push(PdfClip {
                    bbox,
                    ctm: form_state.ctm,
                });

                let form_resources = match stream.dict.get_deref(b"Resources", document) {
                    Ok(resources) => Some(resources.as_dict().map_err(|error| {
                        failure(
                            WorkerFailureCode::ParserDisagreement,
                            format!("PDF page {page_number} Form Resources: {error}"),
                        )
                    })?),
                    Err(lopdf::Error::DictKey(_)) => Some(resources),
                    Err(error) => {
                        return Err(failure(
                            WorkerFailureCode::ParserDisagreement,
                            format!("PDF page {page_number} Form Resources: {error}"),
                        ));
                    }
                };
                let decoded = stream
                    .decompressed_content_with_limit(context.decode_budget.remaining())
                    .map_err(|error| map_required_content_error(error, page_number, "Form"))?;
                context
                    .decode_budget
                    .charge(decoded.len(), &format!("page {page_number} Form"))?;
                let operations = decode_content_operations(&decoded, page_number, "Form")?;
                collect_content_paints(
                    context,
                    &operations,
                    form_resources,
                    depth + 1,
                    &mut form_state,
                )
            })();
            context.active_forms.remove(&object_id);
            result
        }
        other => Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!(
                "unsupported PDF page {page_number} Do XObject subtype: {}",
                String::from_utf8_lossy(other)
            ),
        )),
    }
}

fn form_bbox(
    document: &Document,
    stream: &lopdf::Stream,
    page_number: u32,
) -> Result<[f64; 4], WorkerFailure> {
    let bbox = stream.dict.get_deref(b"BBox", document).map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} Form BBox is missing: {error}"),
        )
    })?;
    let values = numeric_array(bbox, 4, page_number, "Form BBox")?;
    let bbox = [values[0], values[1], values[2], values[3]];
    if bbox[0] > bbox[2] || bbox[1] > bbox[3] {
        return Err(failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} Form BBox bounds are reversed"),
        ));
    }
    Ok(bbox)
}

fn form_matrix(
    document: &Document,
    stream: &lopdf::Stream,
    page_number: u32,
) -> Result<[f64; 6], WorkerFailure> {
    let matrix = match stream.dict.get_deref(b"Matrix", document) {
        Ok(matrix) => matrix,
        Err(lopdf::Error::DictKey(_)) => return Ok(PdfGraphicsState::default().ctm),
        Err(error) => {
            return Err(failure(
                WorkerFailureCode::ParserDisagreement,
                format!("PDF page {page_number} Form Matrix cannot be resolved: {error}"),
            ));
        }
    };
    let values = numeric_array(matrix, 6, page_number, "Form Matrix")?;
    let matrix = [
        values[0], values[1], values[2], values[3], values[4], values[5],
    ];
    validate_finite_matrix(&matrix, page_number)?;
    Ok(matrix)
}

fn parse_pdf_matrix(operands: &[Object], page_number: u32) -> Result<[f64; 6], WorkerFailure> {
    let values = numeric_values(operands, 6, page_number, "cm")?;
    let matrix = [
        values[0], values[1], values[2], values[3], values[4], values[5],
    ];
    validate_finite_matrix(&matrix, page_number)?;
    Ok(matrix)
}

fn numeric_array(
    object: &Object,
    expected_len: usize,
    page_number: u32,
    context: &str,
) -> Result<Vec<f64>, WorkerFailure> {
    let Object::Array(values) = object else {
        return Err(failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} {context} is not an array"),
        ));
    };
    numeric_values(values, expected_len, page_number, context)
}

fn numeric_values(
    values: &[Object],
    expected_len: usize,
    page_number: u32,
    context: &str,
) -> Result<Vec<f64>, WorkerFailure> {
    if values.len() != expected_len {
        return Err(failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} {context} has an invalid element count"),
        ));
    }
    values
        .iter()
        .map(|value| {
            let number = match value {
                Object::Integer(value) => *value as f64,
                Object::Real(value) => f64::from(*value),
                _ => {
                    return Err(failure(
                        WorkerFailureCode::ParserDisagreement,
                        format!("PDF page {page_number} {context} contains a non-number"),
                    ));
                }
            };
            if !number.is_finite() {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!("PDF page {page_number} {context} contains a non-finite number"),
                ));
            }
            Ok(number)
        })
        .collect()
}

fn multiply_pdf_matrices(left: [f64; 6], right: [f64; 6]) -> [f64; 6] {
    [
        left[0] * right[0] + left[1] * right[2],
        left[0] * right[1] + left[1] * right[3],
        left[2] * right[0] + left[3] * right[2],
        left[2] * right[1] + left[3] * right[3],
        left[4] * right[0] + left[5] * right[2] + right[4],
        left[4] * right[1] + left[5] * right[3] + right[5],
    ]
}

fn validate_finite_matrix(matrix: &[f64; 6], page_number: u32) -> Result<(), WorkerFailure> {
    if matrix.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(failure(
            WorkerFailureCode::ParserDisagreement,
            format!("PDF page {page_number} effective matrix is non-finite"),
        ))
    }
}

fn normalize_matrix(mut matrix: [f64; 6]) -> [f64; 6] {
    for value in &mut matrix {
        if *value == 0.0 {
            *value = 0.0;
        }
    }
    matrix
}

fn inherited_page_resources(
    document: &Document,
    page_id: lopdf::ObjectId,
    page_number: u32,
) -> Result<Option<&lopdf::Dictionary>, WorkerFailure> {
    let mut current_id = page_id;
    let mut visited = BTreeSet::new();
    for depth in 0..MAX_PDF_OBJECT_DEPTH {
        if !visited.insert(current_id) {
            return Err(failure(
                WorkerFailureCode::ParserDisagreement,
                format!("lopdf page {page_number} has a cyclic page tree"),
            ));
        }
        let node = document.get_dictionary(current_id).map_err(|error| {
            failure(
                WorkerFailureCode::ParserDisagreement,
                format!("lopdf page {page_number} page-tree node: {error}"),
            )
        })?;

        match node.get_deref(b"Resources", document) {
            Ok(resources) => {
                let dictionary = resources.as_dict().map_err(|error| {
                    failure(
                        WorkerFailureCode::ParserDisagreement,
                        format!("lopdf page {page_number} Resources is not a dictionary: {error}"),
                    )
                })?;
                return Ok(Some(dictionary));
            }
            Err(lopdf::Error::DictKey(_)) => {}
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!("lopdf page {page_number} Resources cannot be resolved: {error}"),
                ));
            }
        }

        match node.get(b"Parent") {
            Ok(Object::Reference(parent_id)) => current_id = *parent_id,
            Ok(_) => {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!("lopdf page {page_number} Parent is not an indirect reference"),
                ));
            }
            Err(lopdf::Error::DictKey(_)) => return Ok(None),
            Err(error) => {
                return Err(failure(
                    WorkerFailureCode::ParserDisagreement,
                    format!("lopdf page {page_number} Parent cannot be read: {error}"),
                ));
            }
        }

        if depth + 1 == MAX_PDF_OBJECT_DEPTH {
            return Err(failure(
                WorkerFailureCode::InspectionResourceLimitExceeded,
                format!("lopdf page {page_number} page tree exceeded the depth limit"),
            ));
        }
    }
    Err(failure(
        WorkerFailureCode::InspectionResourceLimitExceeded,
        format!("lopdf page {page_number} page tree exceeded the depth limit"),
    ))
}

fn image_semantic_hash(
    document: &Document,
    image_id: Option<lopdf::ObjectId>,
    stream: &lopdf::Stream,
    resources: &lopdf::Dictionary,
    page_number: u32,
    decode_budget: &mut PdfDecodeBudget,
) -> Result<String, WorkerFailure> {
    const IMAGE_SEMANTIC_KEYS: [&[u8]; 13] = [
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
    ];
    let mut image_dictionary = serde_json::Map::new();
    let mut active_references = BTreeSet::new();
    if let Some(image_id) = image_id {
        active_references.insert(image_id);
    }
    for key in IMAGE_SEMANTIC_KEYS {
        let Ok(value) = stream.dict.get(key) else {
            continue;
        };
        let normalized =
            normalize_pdf_object(document, value, page_number, 0, &mut active_references)?;
        image_dictionary.insert(String::from_utf8_lossy(key).into_owned(), normalized);
    }
    if let Ok(Object::Name(name)) = stream.dict.get(b"ColorSpace")
        && !matches!(
            name.as_slice(),
            b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK"
        )
    {
        let color_spaces = resources
            .get_deref(b"ColorSpace", document)
            .and_then(Object::as_dict)
            .map_err(|error| {
                failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!(
                        "lopdf page {page_number} named ColorSpace cannot be resolved: {error}"
                    ),
                )
            })?;
        let definition = color_spaces.get_deref(name, document).map_err(|error| {
            failure(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                format!("lopdf page {page_number} named ColorSpace definition: {error}"),
            )
        })?;
        image_dictionary.insert(
            "ResolvedColorSpace".into(),
            normalize_pdf_object(document, definition, page_number, 0, &mut active_references)?,
        );
    }

    let samples = stream
        .decompressed_content_with_limit(decode_budget.remaining())
        .map_err(|error| {
            map_lopdf_stream_error(error, &format!("page {page_number} image stream"))
        })?;
    decode_budget.charge(samples.len(), &format!("page {page_number} image samples"))?;
    let sample_hash: [u8; 32] = Sha256::digest(samples).into();
    let projection = canonical_json_bytes(&json!({
        "image_dictionary": image_dictionary,
        "samples_sha256": format_sha256(&sample_hash),
    }))
    .map_err(|error| {
        failure(
            WorkerFailureCode::InvalidWorkerResult,
            format!("PDF image semantic projection: {error}"),
        )
    })?;
    let semantic_hash: [u8; 32] = Sha256::digest(projection).into();
    Ok(format_sha256(&semantic_hash))
}

fn normalize_pdf_object(
    document: &Document,
    object: &Object,
    page_number: u32,
    depth: usize,
    active_references: &mut BTreeSet<lopdf::ObjectId>,
) -> Result<Value, WorkerFailure> {
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!("lopdf page {page_number} image dictionary exceeded the depth limit"),
        ));
    }

    match object {
        Object::Null => Ok(Value::Null),
        Object::Boolean(value) => Ok(json!(value)),
        Object::Integer(value) => Ok(json!(value)),
        Object::Real(value) if value.is_finite() => {
            Ok(Value::String(format!("f32:{:08x}", value.to_bits())))
        }
        Object::Real(_) => Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("lopdf page {page_number} image dictionary has a non-finite number"),
        )),
        Object::Name(value) => Ok(json!({ "name_hex": format_bytes(value) })),
        Object::String(value, _) => Ok(json!({ "string_hex": format_bytes(value) })),
        Object::Reference(object_id) => {
            if !active_references.insert(*object_id) {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    format!("lopdf page {page_number} image dictionary has a reference cycle"),
                ));
            }
            let result = document
                .get_object(*object_id)
                .map_err(|error| {
                    failure(
                        WorkerFailureCode::ParserDisagreement,
                        format!("lopdf page {page_number} image reference: {error}"),
                    )
                })
                .and_then(|object| {
                    normalize_pdf_object(
                        document,
                        object,
                        page_number,
                        depth + 1,
                        active_references,
                    )
                });
            active_references.remove(object_id);
            result
        }
        Object::Array(values) => values
            .iter()
            .map(|value| {
                normalize_pdf_object(document, value, page_number, depth + 1, active_references)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Object::Dictionary(dictionary) => normalize_pdf_dictionary(
            document,
            dictionary,
            page_number,
            depth,
            active_references,
            false,
        ),
        Object::Stream(stream) => {
            let dictionary = normalize_pdf_dictionary(
                document,
                &stream.dict,
                page_number,
                depth + 1,
                active_references,
                true,
            )?;
            let bytes = stream
                .decompressed_content_with_limit(MAX_DECOMPRESSED_STREAM)
                .map_err(|error| {
                    map_lopdf_stream_error(error, &format!("page {page_number} image metadata"))
                })?;
            let stream_hash: [u8; 32] = Sha256::digest(bytes).into();
            Ok(json!({
                "stream_dictionary": dictionary,
                "stream_sha256": format_sha256(&stream_hash),
            }))
        }
    }
}

fn normalize_pdf_dictionary(
    document: &Document,
    dictionary: &lopdf::Dictionary,
    page_number: u32,
    depth: usize,
    active_references: &mut BTreeSet<lopdf::ObjectId>,
    omit_stream_encoding: bool,
) -> Result<Value, WorkerFailure> {
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!("lopdf page {page_number} image dictionary exceeded the depth limit"),
        ));
    }
    let mut normalized = serde_json::Map::new();
    for (key, value) in dictionary.iter() {
        if omit_stream_encoding && matches!(key.as_slice(), b"Length" | b"Filter" | b"DecodeParms")
        {
            continue;
        }
        let normalized_value =
            normalize_pdf_object(document, value, page_number, depth + 1, active_references)?;
        normalized.insert(format_bytes(key), normalized_value);
    }
    Ok(Value::Object(normalized))
}

fn format_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn failure(code: WorkerFailureCode, message: impl Into<String>) -> WorkerFailure {
    WorkerFailure::new(code, message)
}

#[cfg(test)]
mod resource_limit_tests {
    use super::{
        MAX_PDF_CONTENT_OPERATIONS, MAX_PDF_PAINT_EVENTS, decode_content_operations,
        enforce_operation_limit, ensure_image_paint_slot, text_show_has_bytes,
    };
    use crate::WorkerFailureCode;
    use lopdf::content::Operation;
    use lopdf::{Object, StringFormat};

    #[test]
    fn operation_limit_accepts_boundary_and_rejects_one_over() {
        assert!(enforce_operation_limit(MAX_PDF_CONTENT_OPERATIONS, 1, "page").is_ok());
        assert!(enforce_operation_limit(MAX_PDF_CONTENT_OPERATIONS, 1, "traversal").is_ok());
        assert_eq!(
            enforce_operation_limit(MAX_PDF_CONTENT_OPERATIONS + 1, 1, "page")
                .unwrap_err()
                .code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
        assert_eq!(
            enforce_operation_limit(MAX_PDF_CONTENT_OPERATIONS + 1, 1, "traversal")
                .unwrap_err()
                .code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
    }

    #[test]
    fn image_paint_limit_accepts_last_slot_and_rejects_next() {
        assert!(ensure_image_paint_slot(MAX_PDF_PAINT_EVENTS - 1, 1).is_ok());
        assert_eq!(
            ensure_image_paint_slot(MAX_PDF_PAINT_EVENTS, 1)
                .unwrap_err()
                .code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
    }

    #[test]
    fn decoded_operations_accept_limit_and_reject_one_over() {
        let exact = b"BT\n".repeat(MAX_PDF_CONTENT_OPERATIONS);
        assert_eq!(
            decode_content_operations(&exact, 1, "page")
                .expect("exact operation limit is accepted")
                .len(),
            MAX_PDF_CONTENT_OPERATIONS
        );
        let content = b"BT\n".repeat(MAX_PDF_CONTENT_OPERATIONS + 1);
        assert_eq!(
            decode_content_operations(&content, 1, "page")
                .unwrap_err()
                .code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
    }

    #[test]
    fn malformed_text_show_array_fails_closed() {
        let operation = Operation::new(
            "TJ",
            vec![Object::Array(vec![
                Object::String(b"visible".to_vec(), StringFormat::Literal),
                Object::Boolean(true),
            ])],
        );
        assert_eq!(
            text_show_has_bytes(&operation, 1).unwrap_err().code(),
            WorkerFailureCode::ParserDisagreement
        );
    }
}
