use document_diff_core::{
    DiffCoverage, SourceLocator, UnverifiedReason, WorkerDiffRequest, WorkerDiffResponse,
    WorkerUnverifiedRegion,
};

use crate::WorkerError;

pub(crate) mod csv;
pub(crate) mod html;
pub(crate) mod text;

pub(crate) fn compare(
    request: &WorkerDiffRequest,
    base: &[u8],
    target: &[u8],
) -> Result<WorkerDiffResponse, WorkerError> {
    if request.format == document_diff_core::FormatId::Txt {
        let mut budget = document_diff_core::ComparisonBudget::new(100_000, 100_000);
        return text::TextComparator::compare(request, base, target, &mut budget);
    }
    if request.format == document_diff_core::FormatId::Csv {
        let mut budget = document_diff_core::ComparisonBudget::new(100_000, 100_000);
        return csv::CsvComparator::compare(request, base, target, &mut budget);
    }
    if request.format == document_diff_core::FormatId::Html {
        let mut budget = document_diff_core::ComparisonBudget::new(100_000, 100_000);
        return html::HtmlComparator::compare(request, base, target, &mut budget);
    }
    // A format is promoted only after its independent qualification task.
    Ok(WorkerDiffResponse {
        protocol_version: request.protocol_version,
        diff_profile_version: request.diff_profile_version,
        resource_profile_version: request.resource_profile_version,
        base_raw_sha256: request.base_raw_sha256,
        base_size_bytes: request.base_size_bytes,
        target_raw_sha256: request.target_raw_sha256,
        target_size_bytes: request.target_size_bytes,
        format: request.format,
        coverage: DiffCoverage::None,
        changes: Vec::new(),
        unverified_regions: vec![WorkerUnverifiedRegion {
            base: Some(SourceLocator::ContentItem),
            target: Some(SourceLocator::ContentItem),
            reason: UnverifiedReason::UnsupportedSemanticConstruct,
            navigation_hint: Some("原本の両側を確認してください".to_owned()),
        }],
        parser_provenance: "document-diff-unsupported-v0".to_owned(),
    })
}
