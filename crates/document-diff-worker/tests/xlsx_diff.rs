use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId, RelocationKind,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest,
    WorkerProtocolVersion,
};
use document_diff_worker::SpreadsheetComparator;
use sha2::{Digest, Sha256};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/xlsx/base.xlsx");
const CELL: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/cell-value-change.xlsx"
);
const FORMULA: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/formula-source-change-same-cache.xlsx"
);
const CACHE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/cached-result-only.xlsx"
);
const STYLE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/style-only.xlsx"
);
const XML_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/xml-order-noise.xlsx"
);
const SHEET_ADDED: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/sheet-add.xlsx"
);
const SHEET_REORDERED: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/sheet-order-change.xlsx"
);
const VERY_HIDDEN: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/very-hidden-change.xlsx"
);
const DEFINED_NAME: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/defined-name-change.xlsx"
);
const MERGED: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/merged-change.xlsx"
);
const TABLE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/table-add.xlsx"
);
const CHART: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/chart-add.xlsx"
);
const EXTERNAL: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/external-reference-add.xlsx"
);
const LINK: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/hyperlink-change.xlsx"
);
const IMAGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/image-add.xlsx"
);
const UNKNOWN: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/unknown-semantic-part.xlsx"
);

fn compare(base: &[u8], target: &[u8]) -> document_diff_core::WorkerDiffResponse {
    let request = WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Xlsx,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    };
    let mut budget = ComparisonBudget::new(100_000, 100_000);
    let result = SpreadsheetComparator::xlsx(&request, base, target, &mut budget).unwrap();
    result.validate_against(&request).unwrap();
    result
}

#[test]
fn cell_value_and_formula_are_separate_located_facets() {
    for (target, facet, cell) in [
        (CELL, "xlsx_cell_value", "A1"),
        (FORMULA, "xlsx_formula", "B1"),
    ] {
        let result = compare(BASE, target);
        assert_eq!(result.coverage, DiffCoverage::Full, "{facet}");
        assert!(result.changes.iter().any(|change| {
            change.facet == facet
                && change.operation == Some(ChangeOperation::Modified)
                && matches!(change.base, Some(SourceLocator::SheetCell { ref sheet, cell: ref c }) if sheet == "Sheet1" && c == cell)
        }), "{facet}");
    }
}

#[test]
fn calculation_cache_and_decoration_noise_are_equivalent() {
    for target in [CACHE, STYLE, XML_NOISE] {
        let result = compare(BASE, target);
        assert_eq!(result.coverage, DiffCoverage::Full);
        assert!(result.changes.is_empty());
    }
}

#[test]
fn sheet_add_order_visibility_and_named_range_are_distinct() {
    let added = compare(BASE, SHEET_ADDED);
    assert!(
        added
            .changes
            .iter()
            .any(|change| change.facet == "xlsx_sheet"
                && change.operation == Some(ChangeOperation::Added))
    );
    let reordered = compare(SHEET_ADDED, SHEET_REORDERED);
    assert!(
        reordered
            .changes
            .iter()
            .any(|change| change.facet == "xlsx_sheet"
                && change.relocation == Some(RelocationKind::Reordered))
    );
    let hidden = compare(BASE, VERY_HIDDEN);
    assert!(
        hidden
            .changes
            .iter()
            .any(|change| change.facet == "xlsx_sheet_visibility")
    );
    let named = compare(BASE, DEFINED_NAME);
    assert!(
        named
            .changes
            .iter()
            .any(|change| change.facet == "xlsx_named_range")
    );
}

#[test]
fn workbook_structure_and_external_dependencies_are_explicit() {
    for (target, facet) in [
        (MERGED, "xlsx_merge"),
        (TABLE, "xlsx_table"),
        (CHART, "xlsx_chart"),
        (EXTERNAL, "xlsx_external_reference"),
        (LINK, "xlsx_link"),
        (IMAGE, "xlsx_image"),
    ] {
        let result = compare(BASE, target);
        assert_ne!(result.coverage, DiffCoverage::None, "{facet}");
        assert!(
            result.changes.iter().any(|change| change.facet == facet),
            "{facet}"
        );
    }
}

#[test]
fn unknown_spreadsheet_semantics_fail_closed() {
    let result = compare(BASE, UNKNOWN);
    assert_eq!(result.coverage, DiffCoverage::None);
    assert!(result.changes.is_empty());
    assert_eq!(
        result.unverified_regions[0].reason,
        UnverifiedReason::UnsupportedSemanticConstruct
    );
    assert_eq!(
        result.unverified_regions[0].base,
        Some(SourceLocator::ContentItem)
    );
}
