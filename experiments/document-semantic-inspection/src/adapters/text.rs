use crate::{AdapterOutput, FormatId, InspectionAdapter, InspectionProfile, PocError};
use encoding_rs::UTF_8;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, Default)]
pub struct TextAdapter;

impl InspectionAdapter for TextAdapter {
    fn format(&self) -> FormatId {
        FormatId::Txt
    }

    fn inspect(
        &self,
        input: &[u8],
        profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError> {
        if let Some(encoding) = profile.text_encoding.as_deref() {
            if encoding != "utf-8" {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "ambiguous or unsupported text encoding: {encoding}"
                )));
            }
        }

        let decoded = UTF_8
            .decode_without_bom_handling_and_without_replacement(input)
            .ok_or_else(|| PocError::SemanticExtractionFailed("invalid UTF-8".into()))?;

        let line_normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
        let semantic: String = line_normalized.nfc().collect();
        Ok(AdapterOutput::projection_only(semantic.into_bytes()))
    }
}
