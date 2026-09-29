use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId, RelocationKind,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest,
    WorkerProtocolVersion,
};
use document_diff_worker::CsvComparator;
use sha2::{Digest, Sha256};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/csv/base.csv");
const QUOTE_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/csv/quote-noise.csv"
);
const CELL_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/csv/cell-change.csv"
);
const ROW_CHANGE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/csv/row-change.csv");
const INCONSISTENT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/csv/inconsistent.csv"
);
const AMBIGUOUS: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/csv/delimiter-ambiguous.csv"
);

fn request(base: &[u8], target: &[u8]) -> WorkerDiffRequest {
    WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Csv,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    }
}

fn compare(base: &[u8], target: &[u8]) -> document_diff_core::WorkerDiffResponse {
    let request = request(base, target);
    let mut budget = ComparisonBudget::new(100_000, 100_000);
    let result = CsvComparator::compare(&request, base, target, &mut budget).unwrap();
    result.validate_against(&request).unwrap();
    result
}

#[test]
fn quote_syntax_noise_is_semantically_equal() {
    let result = compare(BASE, QUOTE_NOISE);
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert!(result.changes.is_empty());
}

#[test]
fn cell_and_row_changes_point_to_old_and_new_csv_positions() {
    let cell = compare(BASE, CELL_CHANGE);
    assert_eq!(cell.coverage, DiffCoverage::Full);
    assert_eq!(cell.changes.len(), 1);
    assert_eq!(cell.changes[0].operation, Some(ChangeOperation::Modified));
    assert_eq!(
        cell.changes[0].base,
        Some(SourceLocator::CsvCell { row: 2, column: 2 })
    );
    assert_eq!(
        cell.changes[0].target,
        Some(SourceLocator::CsvCell { row: 2, column: 2 })
    );

    let row = compare(BASE, ROW_CHANGE);
    assert_eq!(row.coverage, DiffCoverage::Full);
    assert_eq!(row.changes.len(), 1);
    assert_eq!(row.changes[0].operation, Some(ChangeOperation::Added));
    assert!(row.changes[0].base.is_none());
    assert_eq!(
        row.changes[0].target,
        Some(SourceLocator::CsvCell { row: 4, column: 1 })
    );
}

#[test]
fn added_column_is_located_in_each_authoritative_row() {
    let result = compare(b"id,value\n1,a\n", b"id,value,flag\n1,a,yes\n");
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert_eq!(result.changes.len(), 2);
    assert!(result.changes.iter().all(|change| {
        change.operation == Some(ChangeOperation::Added)
            && change.base.is_none()
            && matches!(
                change.target,
                Some(SourceLocator::CsvCell { column: 3, .. })
            )
    }));
}

#[test]
fn unique_row_reorder_is_confirmed_but_duplicate_row_alignment_is_not() {
    let reordered = compare(b"id,val\na,1\nb,2\n", b"id,val\nb,2\na,1\n");
    assert_eq!(reordered.coverage, DiffCoverage::Full);
    assert!(reordered.changes.iter().any(|change| {
        change.relocation == Some(RelocationKind::Reordered)
            && change.base.is_some()
            && change.target.is_some()
    }));

    let duplicate = compare(b"id,val\na,1\na,1\nb,2\n", b"id,val\na,1\nb,2\na,1\n");
    assert_eq!(duplicate.coverage, DiffCoverage::None);
    assert!(duplicate.changes.is_empty());
    assert_eq!(
        duplicate.unverified_regions[0].reason,
        UnverifiedReason::AmbiguousAlignment
    );

    let duplicated_target = compare(b"id,val\na,1\nb,2\n", b"id,val\na,1\na,1\n");
    assert_eq!(duplicated_target.coverage, DiffCoverage::None);
    assert!(duplicated_target.changes.is_empty());
}

#[test]
fn inconsistent_structure_and_ambiguous_delimiter_are_unverified() {
    for target in [INCONSISTENT, AMBIGUOUS] {
        let result = compare(BASE, target);
        assert_eq!(result.coverage, DiffCoverage::None);
        assert!(result.changes.is_empty());
        assert!(!result.unverified_regions.is_empty());
    }
}

#[test]
fn row_limit_is_a_resource_unverified_region() {
    let many_rows = b"a,b\n".repeat(100_001);
    let result = compare(BASE, &many_rows);
    assert_eq!(result.coverage, DiffCoverage::None);
    assert_eq!(
        result.unverified_regions[0].reason,
        UnverifiedReason::ResourceLimit
    );
}
