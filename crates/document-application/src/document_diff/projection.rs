use document_diff_core::{ChangeOperation, ContentVerdict, DiffCoverage, RelocationKind};
use uuid::Uuid;

use super::{DiffResult, SourceEvidence};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonRowState {
    Confirmed,
    Unverified,
    Ancillary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonRow {
    pub state: ComparisonRowState,
    pub facet: String,
    pub operation: Option<ChangeOperation>,
    pub relocation: Option<RelocationKind>,
    pub base: Option<SourceEvidence>,
    pub target: Option<SourceEvidence>,
    pub reason: String,
    pub navigation_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedComparisonTable {
    pub rows: Vec<ComparisonRow>,
    pub verdict: ContentVerdict,
    pub coverage: DiffCoverage,
    pub result_digest: [u8; 32],
    pub audit_event_id: Uuid,
}

/// Pure internal projection; disclosure must go through DocumentDiffService::comparison_table.
pub fn project_comparison_table(result: &DiffResult) -> Vec<ComparisonRow> {
    let mut rows = Vec::with_capacity(
        result.changes.len() + result.unverified_regions.len() + result.ancillary_changes.len(),
    );
    for change in &result.changes {
        rows.push(ComparisonRow {
            state: ComparisonRowState::Confirmed,
            facet: change.facet.clone(),
            operation: change.operation,
            relocation: change.relocation,
            base: change.base.clone(),
            target: change.target.clone(),
            reason: change.reason_code.clone(),
            navigation_hint: None,
        });
    }
    for region in &result.unverified_regions {
        rows.push(ComparisonRow {
            state: ComparisonRowState::Unverified,
            facet: "unverified_region".into(),
            operation: None,
            relocation: None,
            base: region.base.clone(),
            target: region.target.clone(),
            reason: region.reason.as_str().to_owned(),
            navigation_hint: region.navigation_hint.clone(),
        });
    }
    for ancillary in &result.ancillary_changes {
        rows.push(ComparisonRow {
            state: ComparisonRowState::Ancillary,
            facet: ancillary.kind.clone(),
            operation: None,
            relocation: None,
            base: None,
            target: None,
            reason: "version_specific_metadata_changed".into(),
            navigation_hint: None,
        });
    }
    rows
}
