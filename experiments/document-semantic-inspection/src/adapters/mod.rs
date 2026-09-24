mod csv;
mod docx;
mod html;
mod pdf;
mod pptx;
mod spreadsheet;
mod signatures;
mod vba;
mod text;

pub use csv::CsvAdapter;
pub use docx::DocxAdapter;
pub(crate) use docx::is_docx_package;
pub use html::HtmlAdapter;
pub use pdf::PdfAdapter;
pub use pptx::PptxAdapter;
pub(crate) use pptx::is_pptx_package;
pub use spreadsheet::SpreadsheetAdapter;
pub use signatures::{SignatureInspector, SignatureTrustContext};
pub(crate) use spreadsheet::spreadsheet_format;
pub use vba::VbaAdapter;
pub use text::TextAdapter;

use crate::{AdapterOutput, FormatId, InspectionAdapter, InspectionProfile, PocError};

#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysSuccessAdapter;

impl InspectionAdapter for AlwaysSuccessAdapter {
    fn format(&self) -> FormatId {
        FormatId::Txt
    }

    fn inspect(
        &self,
        input: &[u8],
        _profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError> {
        Ok(AdapterOutput::projection_only(input.to_vec()))
    }
}
