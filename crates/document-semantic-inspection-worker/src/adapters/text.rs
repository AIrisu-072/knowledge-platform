use document_semantic_inspection_core::FormatId;
use encoding_rs::UTF_8;
use unicode_normalization::UnicodeNormalization;

use crate::{WorkerFailure, WorkerFailureCode};

use super::{AdapterProfile, SemanticAdapter, SemanticAdapterOutput};

const PARSER_LIBRARIES: [(&str, &str); 2] = [
    ("encoding_rs", "0.8.41"),
    ("unicode-normalization", "0.1.25"),
];

#[derive(Debug, Clone, Copy, Default)]
pub struct TextAdapter;

impl SemanticAdapter for TextAdapter {
    fn format(&self) -> FormatId {
        FormatId::Txt
    }

    fn inspect(
        &self,
        input: &[u8],
        profile: &AdapterProfile,
    ) -> Result<SemanticAdapterOutput, WorkerFailure> {
        if let Some(encoding) = profile.text_encoding()
            && encoding != "utf-8"
        {
            return Err(WorkerFailure::new(
                WorkerFailureCode::SemanticExtractionFailed,
                format!("ambiguous or unsupported text encoding: {encoding}"),
            ));
        }

        let decoded = UTF_8
            .decode_without_bom_handling_and_without_replacement(input)
            .ok_or_else(|| {
                WorkerFailure::new(
                    WorkerFailureCode::SemanticExtractionFailed,
                    "invalid UTF-8 text",
                )
            })?;

        let line_normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
        let semantic: String = line_normalized.nfc().collect();
        Ok(SemanticAdapterOutput::from_projection(
            semantic.as_bytes(),
            &["reader_content"],
            "text",
            &PARSER_LIBRARIES,
        ))
    }
}
