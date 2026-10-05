//! PDF text from the pinned PDFium native character index, guarded by an
//! independent lopdf content-operator pass. Page-local ranges cannot locate an
//! unresolved global reading order, so multi-page text is not `Partial`.

use std::sync::OnceLock;

use pdfium_render::prelude::Pdfium;
use search_core::knowledge_unit::{BudgetKey, NativeLocator, UnitKind};
use search_extraction_core::{BudgetMeter, CoverageReason};

use super::{Body, ReadResult, corrupt, extraction_failed, resource_limit, structure, unsupported};

const MAX_PAGE_CONTENT_BYTES: usize = 67_108_864;

static PDFIUM: OnceLock<Result<Pdfium, ()>> = OnceLock::new();

/// The pinned PDFium library could not be bound in this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfiumUnavailable;

/// Bind the already pin-verified PDFium library. The worker calls this before
/// the sandbox is sealed because the seal removes access to library paths.
pub fn warm_up_pdfium() -> Result<(), PdfiumUnavailable> {
    PDFIUM
        .get_or_init(|| {
            let directory = std::env::var("PDFIUM_DYNAMIC_LIB_PATH").map_err(|_| ())?;
            let path = Pdfium::pdfium_platform_library_name_at_path(&directory);
            let bindings = Pdfium::bind_to_library(path).map_err(|_| ())?;
            Ok(Pdfium::new(bindings))
        })
        .as_ref()
        .map(|_| ())
        .map_err(|_| PdfiumUnavailable)
}

pub(super) fn read(raw: &[u8], meter: &mut BudgetMeter) -> ReadResult<Body> {
    let structure_document = lopdf::Document::load_mem(raw).map_err(|_| corrupt())?;
    if structure_document.is_encrypted() {
        return Err(unsupported(CoverageReason::Encrypted));
    }
    let pages = structure_document.get_pages();
    meter.charge(BudgetKey::PdfPages, pages.len() as u64)?;
    let mut has_image = false;
    for page_id in pages.values().copied() {
        let content = structure_document
            .get_page_content_with_limit(page_id, MAX_PAGE_CONTENT_BYTES)
            .map_err(|_| resource_limit())?;
        let decoded = lopdf::content::Content::decode(&content).map_err(|_| corrupt())?;
        meter.charge(BudgetKey::PdfOperations, decoded.operations.len() as u64)?;
        for operation in decoded.operations {
            match operation.operator.as_str() {
                "BT" | "Tf" | "Tm" | "Tj" | "ET" | "q" | "cm" | "Q" => {}
                "Do" => has_image = true,
                _ => return Err(structure()),
            }
        }
    }
    let engine = PDFIUM
        .get()
        .and_then(|engine| engine.as_ref().ok())
        .ok_or_else(extraction_failed)?;
    let document = engine
        .load_pdf_from_byte_slice(raw, None)
        .map_err(|_| corrupt())?;
    if usize::try_from(document.pages().len()).map_err(|_| corrupt())? != pages.len() {
        return Err(corrupt());
    }
    let mut body = Body::default();
    for (page_index, page) in document.pages().iter().enumerate() {
        body.visit(1)?;
        let text = page.text().map_err(|_| extraction_failed())?;
        let chars = text.chars();
        let mut content = String::new();
        let mut start = None;
        let mut end = 0usize;
        for index in 0..chars.len() {
            let Some(character) = chars
                .get(index)
                .map_err(|_| extraction_failed())?
                .unicode_char()
            else {
                continue;
            };
            if !character.is_whitespace() {
                start.get_or_insert(index);
                end = index + 1;
            }
            content.push(character);
        }
        if let Some(start) = start {
            body.unit(
                meter,
                UnitKind::PdfText,
                content.trim(),
                NativeLocator::Pdf {
                    page_index: u32::try_from(page_index).map_err(|_| resource_limit())?,
                    char_start: u32::try_from(start).map_err(|_| resource_limit())?,
                    char_end: u32::try_from(end).map_err(|_| resource_limit())?,
                },
            )?;
        }
    }
    if body.units.is_empty() {
        Err(unsupported(CoverageReason::RequiresOcr))
    } else if has_image {
        Err(structure())
    } else if body.units.len() > 1 {
        Err(unsupported(CoverageReason::AmbiguousReadingOrder))
    } else {
        Ok(body)
    }
}
