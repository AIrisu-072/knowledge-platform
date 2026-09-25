use crate::{
    canonical_json_bytes, AdapterOutput, CapabilityEvidence, FormatId, InspectionAdapter, InspectionProfile, PocError,
};
use ::csv::ReaderBuilder;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, Default)]
pub struct CsvAdapter;

impl InspectionAdapter for CsvAdapter {
    fn format(&self) -> FormatId {
        FormatId::Csv
    }

    fn inspect(
        &self,
        input: &[u8],
        profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError> {
        let delimiter = profile.csv_delimiter.ok_or_else(|| {
            PocError::UnsupportedSemanticConstruct(
                "CSV delimiter must be explicit; guessing is prohibited".into(),
            )
        })?;

        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .flexible(false)
            .delimiter(delimiter)
            .from_reader(input);

        let mut rows: Vec<Vec<String>> = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|error| {
                PocError::SemanticExtractionFailed(format!("invalid CSV structure: {error}"))
            })?;
            rows.push(
                record
                    .iter()
                    .map(|cell| cell.nfc().collect::<String>())
                    .collect(),
            );
        }

        let semantic_projection = canonical_json_bytes(&rows)
            .map_err(|error| PocError::InvalidWorkerResult(error.to_string()))?;
        let equivalence = hex::encode(crate::fingerprint(&semantic_projection));
        Ok(AdapterOutput {
            semantic_projection,
            capabilities: vec![
                CapabilityEvidence::binary("reader_content", true, true, Some(equivalence.clone())),
                CapabilityEvidence::binary("table_structure", true, true, Some(equivalence)),
            ],
            editorial: Default::default(),
            external_dependencies: Vec::new(),
            signatures: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}
