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
    image_hashes: Vec<Vec<String>>,
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
                        format!("action:{:?}", action.action_type()).to_lowercase()
                    }
                } else {
                    "none".to_owned()
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

            let mut image_hashes = structural.image_hashes[page_index].clone();
            image_hashes.sort();
            total_images = total_images.saturating_add(image_hashes.len());

            semantic_pages.push(json!({
                "index": page_index,
                "text": text,
                "links": links,
                "images": image_hashes,
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
    let mut image_hashes = Vec::with_capacity(pages.len());
    let mut form_names = BTreeSet::new();

    for (page_number, page_id) in pages {
        let page_content = document
            .get_page_content_with_limit(page_id, MAX_DECOMPRESSED_STREAM)
            .map_err(|error| {
                map_lopdf_stream_error(error, &format!("page {page_number} content"))
            })?;
        reject_inline_images(&page_content, page_number)?;
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
) -> Result<Vec<String>, WorkerFailure> {
    let resources = inherited_page_resources(document, page_id, page_number)?;
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

fn collect_resource_images(
    document: &Document,
    resources: &lopdf::Dictionary,
    page_number: u32,
    depth: usize,
    active_forms: &mut BTreeSet<lopdf::ObjectId>,
    hashes: &mut Vec<String>,
) -> Result<(), WorkerFailure> {
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            format!("lopdf page {page_number} XObject tree exceeded the depth limit"),
        ));
    }

    let xobjects = match resources.get_deref(b"XObject", document) {
        Ok(xobjects) => xobjects,
        Err(lopdf::Error::DictKey(_)) => return Ok(()),
        Err(error) => {
            return Err(failure(
                WorkerFailureCode::ParserDisagreement,
                format!("lopdf page {page_number} XObject dictionary: {error}"),
            ));
        }
    };
    let xobjects = xobjects.as_dict().map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("lopdf page {page_number} XObject is not a dictionary: {error}"),
        )
    })?;

    for (_, object) in xobjects.iter() {
        let (object_id, resolved) = document.dereference(object).map_err(|error| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("lopdf page {page_number} XObject: {error}"),
            )
        })?;
        let stream = resolved.as_stream().map_err(|error| {
            failure(
                WorkerFailureCode::ParserDisagreement,
                format!("lopdf page {page_number} XObject is not a stream: {error}"),
            )
        })?;
        let subtype = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .ok()
            .map(|name| name.to_vec());

        match subtype.as_deref() {
            Some(b"Image") => hashes.push(image_semantic_hash(
                document,
                object_id,
                stream,
                resources,
                page_number,
            )?),
            Some(b"Form") => {
                let Some(form_id) = object_id else {
                    return Err(failure(
                        WorkerFailureCode::UnsupportedSemanticConstruct,
                        format!("lopdf page {page_number} Form XObject is not indirect"),
                    ));
                };
                let form_content = stream
                    .decompressed_content_with_limit(MAX_DECOMPRESSED_STREAM)
                    .map_err(|error| {
                        map_lopdf_stream_error(error, &format!("page {page_number} Form content"))
                    })?;
                reject_inline_images(&form_content, page_number)?;
                if !active_forms.insert(form_id) {
                    return Err(failure(
                        WorkerFailureCode::UnsupportedSemanticConstruct,
                        format!("lopdf page {page_number} has a cyclic Form XObject"),
                    ));
                }
                let form_resources = match stream.dict.get_deref(b"Resources", document) {
                    Ok(form_resources) => form_resources.as_dict().map_err(|error| {
                        failure(
                            WorkerFailureCode::ParserDisagreement,
                            format!("lopdf page {page_number} Form Resources: {error}"),
                        )
                    })?,
                    Err(lopdf::Error::DictKey(_)) => resources,
                    Err(error) => {
                        active_forms.remove(&form_id);
                        return Err(failure(
                            WorkerFailureCode::ParserDisagreement,
                            format!("lopdf page {page_number} Form Resources: {error}"),
                        ));
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
    page_number: u32,
) -> Result<String, WorkerFailure> {
    const IMAGE_SEMANTIC_KEYS: [&[u8]; 14] = [
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
        .decompressed_content_with_limit(MAX_DECOMPRESSED_STREAM)
        .map_err(|error| {
            map_lopdf_stream_error(error, &format!("page {page_number} image stream"))
        })?;
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

fn reject_inline_images(content: &[u8], page_number: u32) -> Result<(), WorkerFailure> {
    if !content.windows(2).any(|window| window == b"BI") {
        return Ok(());
    }
    let operations = Content::decode_strict(content).map_err(|error| {
        failure(
            WorkerFailureCode::ParserDisagreement,
            format!("lopdf page {page_number} content operations: {error}"),
        )
    })?;
    if operations
        .operations
        .iter()
        .any(|operation| operation.operator == "BI")
    {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            format!("lopdf page {page_number} inline images are unsupported"),
        ));
    }
    Ok(())
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
