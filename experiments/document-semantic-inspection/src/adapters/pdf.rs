use crate::{
    canonical_json_bytes, AdapterOutput, CapabilityEvidence, Diagnostic, EditorialEvidence,
    ExternalDependency, FormatId, InspectionAdapter, InspectionProfile, PocError,
};
use lopdf::{Document, LoadOptions, Object};
use pdfium_render::prelude::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::sync::OnceLock;

const MAX_DECOMPRESSED_STREAM: usize = 64 * 1024 * 1024;

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
        let mut any_text = false;
        let mut total_images = 0usize;

        for (page_index, page) in pages.iter().enumerate() {
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

            let mut annotations = Vec::new();
            for annotation in page.annotations().iter() {
                annotations.push(json!({
                    "type": format!("{:?}", annotation.annotation_type()).to_lowercase(),
                    "contents": annotation.contents(),
                }));
            }
            let annotation_count = annotations.len();
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
                "annotations": annotations,
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
        external_dependencies.dedup_by(|left, right| {
            left.kind == right.kind && left.definition == right.definition
        });

        let projection = json!({
            "pages": semantic_pages,
            "form_values": form_values,
        });

        Ok(AdapterOutput {
            semantic_projection: canonical_json_bytes(&projection).map_err(|error| {
                PocError::InvalidWorkerResult(format!("PDF projection: {error}"))
            })?,
            capabilities: vec![
                CapabilityEvidence {
                    capability: "native_text".into(),
                    present: any_text,
                },
                CapabilityEvidence {
                    capability: "form_fields".into(),
                    present: structural.form_field_count > 0,
                },
                CapabilityEvidence {
                    capability: "annotations".into(),
                    present: structural.annotation_counts.iter().any(|count| *count > 0),
                },
                CapabilityEvidence {
                    capability: "images".into(),
                    present: total_images > 0,
                },
            ],
            editorial: EditorialEvidence::default(),
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

fn extract_lopdf_facts(document: &Document) -> Result<LopdfFacts, PocError> {
    let pages = document.get_pages();
    let mut annotation_counts = Vec::with_capacity(pages.len());
    let mut link_counts = Vec::with_capacity(pages.len());
    let mut image_hashes = Vec::with_capacity(pages.len());
    let mut form_names = BTreeSet::new();

    for (page_number, page_id) in pages {
        let page = document
            .get_dictionary(page_id)
            .map_err(|error| PocError::SemanticExtractionFailed(format!(
                "lopdf page {page_number}: {error}"
            )))?;

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
        image_hashes.push(page_image_hashes(document, page, page_number)?);
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
    page: &lopdf::Dictionary,
    page_number: u32,
) -> Result<Vec<String>, PocError> {
    let Ok(resources) = page.get_deref(b"Resources", document) else {
        return Ok(Vec::new());
    };
    let resources = resources.as_dict().map_err(|error| {
        PocError::SemanticExtractionFailed(format!(
            "lopdf page {page_number} resources: {error}"
        ))
    })?;
    let Ok(xobjects) = resources.get_deref(b"XObject", document) else {
        return Ok(Vec::new());
    };
    let xobjects = xobjects.as_dict().map_err(|error| {
        PocError::SemanticExtractionFailed(format!(
            "lopdf page {page_number} XObject dictionary: {error}"
        ))
    })?;

    let mut hashes = Vec::new();
    for (_, object) in xobjects.iter() {
        let (_, resolved) = document.dereference(object).map_err(|error| {
            PocError::SemanticExtractionFailed(format!(
                "lopdf page {page_number} XObject: {error}"
            ))
        })?;
        let Ok(stream) = resolved.as_stream() else {
            continue;
        };
        let is_image = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|name| name == b"Image");
        if !is_image {
            continue;
        }
        let bytes = stream.decompressed_content().map_err(|error| {
            PocError::SemanticExtractionFailed(format!(
                "lopdf page {page_number} image decode: {error}"
            ))
        })?;
        hashes.push(hex::encode(Sha256::digest(bytes)));
    }
    hashes.sort();
    Ok(hashes)
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
