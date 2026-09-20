mod csv;
mod html;
mod text;

pub use csv::CsvAdapter;
pub use html::HtmlAdapter;
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
