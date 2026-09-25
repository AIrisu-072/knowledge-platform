use csv::ReaderBuilder;
use document_semantic_inspection_core::FormatId;
use unicode_normalization::UnicodeNormalization;

use crate::{WorkerFailure, WorkerFailureCode};

use super::{AdapterProfile, SemanticAdapter, SemanticAdapterOutput};

const PARSER_LIBRARIES: [(&str, &str); 2] = [("csv", "1.4.0"), ("unicode-normalization", "0.1.25")];

#[derive(Debug, Clone, Copy, Default)]
pub struct CsvAdapter;

impl SemanticAdapter for CsvAdapter {
    fn format(&self) -> FormatId {
        FormatId::Csv
    }

    fn inspect(
        &self,
        input: &[u8],
        profile: &AdapterProfile,
    ) -> Result<SemanticAdapterOutput, WorkerFailure> {
        let delimiter = profile.csv_delimiter().ok_or_else(|| {
            WorkerFailure::new(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "CSV delimiter must be explicit; guessing is prohibited",
            )
        })?;
        if matches!(delimiter, b'\r' | b'\n' | b'"') {
            return Err(WorkerFailure::new(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "CSV delimiter conflicts with record or quote syntax",
            ));
        }

        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .flexible(false)
            .delimiter(delimiter)
            .from_reader(input);
        let mut rows: Vec<Vec<String>> = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|error| {
                WorkerFailure::new(
                    WorkerFailureCode::SemanticExtractionFailed,
                    format!("invalid CSV structure: {error}"),
                )
            })?;
            rows.push(
                record
                    .iter()
                    .map(|cell| cell.nfc().collect::<String>())
                    .collect(),
            );
        }

        let semantic_projection = serde_json::to_vec(&rows).map_err(|error| {
            WorkerFailure::new(
                WorkerFailureCode::InvalidWorkerResult,
                format!("CSV semantic projection could not be serialized: {error}"),
            )
        })?;
        Ok(SemanticAdapterOutput::from_projection(
            &semantic_projection,
            &["reader_content", "table_structure"],
            "csv",
            &PARSER_LIBRARIES,
        ))
    }
}
