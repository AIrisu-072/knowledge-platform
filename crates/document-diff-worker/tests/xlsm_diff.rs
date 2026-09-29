use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest,
    WorkerProtocolVersion,
};
use document_diff_worker::SpreadsheetComparator;
use sha2::{Digest, Sha256};

const BASE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsm/calamine-vba.xlsm"
);
const LOGIC: &[u8] =
    include_bytes!("../../../experiments/document-diff/fixtures/xlsm/logic-change.xlsm");
const PROCEDURE: &[u8] =
    include_bytes!("../../../experiments/document-diff/fixtures/xlsm/procedure-name-change.xlsm");
const WHITESPACE: &[u8] =
    include_bytes!("../../../experiments/document-diff/fixtures/xlsm/whitespace-noise.xlsm");
const CASE: &[u8] =
    include_bytes!("../../../experiments/document-diff/fixtures/xlsm/case-noise.xlsm");
const COMMENT: &[u8] = include_bytes!(
    "../../document-semantic-inspection-worker/tests/fixtures/xlsm_vba_comment_noise.xlsm"
);
const INVALID: &[u8] =
    include_bytes!("../../../experiments/document-diff/fixtures/xlsm/invalid-syntax.xlsm");
const WORKSHEET: &[u8] =
    include_bytes!("../../../experiments/document-diff/fixtures/xlsm/worksheet-cell-change.xlsm");

fn compare(base: &[u8], target: &[u8]) -> document_diff_core::WorkerDiffResponse {
    let request = WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Xlsm,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    };
    let mut budget = ComparisonBudget::new(8_000_000, 100_000);
    let result = SpreadsheetComparator::xlsm(&request, base, target, &mut budget).unwrap();
    result.validate_against(&request).unwrap();
    result
}

#[test]
fn procedure_logic_and_declaration_changes_are_located_to_the_vba_module() {
    let logic = compare(BASE, LOGIC);
    assert_eq!(logic.coverage, DiffCoverage::Full);
    assert!(logic.changes.iter().any(|change| {
        change.facet == "xlsm_vba_procedure"
            && change.operation == Some(ChangeOperation::Modified)
            && matches!(change.base, Some(SourceLocator::VbaModule { ref module, procedure: Some(ref name) }) if module == "testVBA" && name.eq_ignore_ascii_case("test"))
    }));
    let declaration = compare(BASE, PROCEDURE);
    assert!(
        declaration
            .changes
            .iter()
            .any(|change| change.facet == "xlsm_vba_module"
                || change.facet == "xlsm_vba_declaration")
    );
}

#[test]
fn whitespace_comment_and_keyword_case_are_not_vba_content_changes() {
    for target in [WHITESPACE, COMMENT, CASE] {
        let result = compare(BASE, target);
        assert_eq!(result.coverage, DiffCoverage::Full);
        assert!(result.changes.is_empty());
    }
}

#[test]
fn worksheet_change_is_not_mislabeled_as_vba_change() {
    let result = compare(BASE, WORKSHEET);
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert!(
        result
            .changes
            .iter()
            .any(|change| change.facet == "xlsx_cell_value")
    );
    assert!(
        !result
            .changes
            .iter()
            .any(|change| change.facet.starts_with("xlsm_vba"))
    );
}

#[test]
fn invalid_vba_syntax_is_unverified_without_running_macros() {
    let result = compare(BASE, INVALID);
    assert_eq!(result.coverage, DiffCoverage::None);
    assert!(result.changes.is_empty());
    assert_eq!(
        result.unverified_regions[0].reason,
        UnverifiedReason::CorruptedSource
    );
    assert_eq!(
        result.unverified_regions[0].base,
        Some(SourceLocator::ContentItem)
    );
}
