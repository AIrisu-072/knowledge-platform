use std::collections::{HashMap, HashSet};

use csv::ReaderBuilder;
use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, FormatId, RelocationKind, SourceLocator,
    UnverifiedReason, WorkerChange, WorkerDiffRequest, WorkerDiffResponse, WorkerUnverifiedRegion,
};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::WorkerError;

const MAX_CSV_COMBINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_ROWS: usize = 100_000;
const MAX_CELLS: usize = 1_000_000;
const MAX_CHANGES: usize = 100_000;
const DELIMITERS: [u8; 4] = *b",;\t|";
const PARSER_PROVENANCE: &str = "document-diff-csv-v0;csv=1.4.0;unicode-normalization=0.1.25";

#[derive(Debug, Clone, Copy, Default)]
pub struct CsvComparator;

#[derive(Debug)]
struct ParsedCsv {
    rows: Vec<Vec<String>>,
}

impl CsvComparator {
    pub fn compare(
        request: &WorkerDiffRequest,
        base: &[u8],
        target: &[u8],
        budget: &mut ComparisonBudget,
    ) -> Result<WorkerDiffResponse, WorkerError> {
        if request.format != FormatId::Csv {
            return Err(WorkerError::InvalidRequest);
        }
        request
            .validate()
            .map_err(|_| WorkerError::ResourceLimit("source bytes"))?;
        if base.len() as u64 != request.base_size_bytes
            || target.len() as u64 != request.target_size_bytes
            || <[u8; 32]>::from(Sha256::digest(base)) != request.base_raw_sha256
            || <[u8; 32]>::from(Sha256::digest(target)) != request.target_raw_sha256
        {
            return Err(WorkerError::RawBindingMismatch);
        }
        if base.len().saturating_add(target.len()) > MAX_CSV_COMBINED_BYTES {
            return Ok(unverified(request, UnverifiedReason::ResourceLimit));
        }
        let base = match detect_and_parse(base) {
            Ok(parsed) => parsed,
            Err(reason) => return Ok(unverified(request, reason)),
        };
        let target = match detect_and_parse(target) {
            Ok(parsed) => parsed,
            Err(reason) => return Ok(unverified(request, reason)),
        };
        match compare_rows(&base.rows, &target.rows, budget) {
            Ok(changes) => Ok(response(request, DiffCoverage::Full, changes, Vec::new())),
            Err(reason) => Ok(unverified(request, reason)),
        }
    }
}

fn detect_and_parse(input: &[u8]) -> Result<ParsedCsv, UnverifiedReason> {
    let outside_quotes = unquoted_bytes(input)?;
    let mut selected = None;
    for delimiter in DELIMITERS {
        if !outside_quotes.contains(&delimiter) {
            continue;
        }
        let rows = match parse_rows(input, delimiter) {
            Ok(rows) => rows,
            Err(UnverifiedReason::ResourceLimit) => return Err(UnverifiedReason::ResourceLimit),
            Err(_) => continue,
        };
        if rows.is_empty() || rows[0].len() < 2 {
            continue;
        }
        if selected.is_some() {
            return Err(UnverifiedReason::UnsupportedSemanticConstruct);
        }
        selected = Some(ParsedCsv { rows });
    }
    selected.ok_or(UnverifiedReason::CorruptedSource)
}

fn unquoted_bytes(input: &[u8]) -> Result<HashSet<u8>, UnverifiedReason> {
    let mut outside = HashSet::new();
    let mut quoted = false;
    let mut position = 0;
    while position < input.len() {
        if input[position] == b'"' {
            if quoted && input.get(position + 1).copied() == Some(b'"') {
                position += 2;
                continue;
            }
            quoted = !quoted;
        } else if !quoted {
            outside.insert(input[position]);
        }
        position += 1;
    }
    if quoted {
        return Err(UnverifiedReason::CorruptedSource);
    }
    Ok(outside)
}

fn parse_rows(input: &[u8], delimiter: u8) -> Result<Vec<Vec<String>>, UnverifiedReason> {
    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(false)
        .delimiter(delimiter)
        .from_reader(input);
    let mut rows = Vec::new();
    let mut cell_count = 0;
    for record in reader.records() {
        let record = record.map_err(|_| UnverifiedReason::CorruptedSource)?;
        if rows.len() >= MAX_ROWS {
            return Err(UnverifiedReason::ResourceLimit);
        }
        cell_count += record.len();
        if cell_count > MAX_CELLS {
            return Err(UnverifiedReason::ResourceLimit);
        }
        rows.push(record.iter().map(|cell| cell.nfc().collect()).collect());
    }
    Ok(rows)
}

fn compare_rows(
    base: &[Vec<String>],
    target: &[Vec<String>],
    budget: &mut ComparisonBudget,
) -> Result<Vec<WorkerChange>, UnverifiedReason> {
    budget
        .consume_candidates((base.len() + target.len()) as u64)
        .map_err(|_| UnverifiedReason::ResourceLimit)?;
    if base == target {
        return Ok(Vec::new());
    }

    if base.len() == target.len() {
        let mut positions = HashMap::new();
        let unique = base
            .iter()
            .enumerate()
            .all(|(index, row)| positions.insert(row, index).is_none());
        let same_rows = unique
            && !has_duplicate_rows(target)
            && target.iter().all(|row| positions.contains_key(row))
            && positions.len() == target.len();
        if same_rows {
            let mut changes = Vec::new();
            for (target_index, row) in target.iter().enumerate() {
                let base_index = positions[row];
                if base_index != target_index {
                    add_change(
                        &mut changes,
                        budget,
                        WorkerChange {
                            operation: None,
                            relocation: Some(RelocationKind::Reordered),
                            facet: "csv_row".to_owned(),
                            base: Some(cell_locator(base_index, 0)?),
                            target: Some(cell_locator(target_index, 0)?),
                            reason_code: "unique_row_reordered".to_owned(),
                        },
                    )?;
                }
            }
            return Ok(changes);
        }
        if has_duplicate_rows(base) || has_duplicate_rows(target) {
            return Err(UnverifiedReason::AmbiguousAlignment);
        }
        if let Some(changes) = compare_added_or_removed_columns(base, target, budget)? {
            return Ok(changes);
        }
    }

    let mut prefix = 0;
    while prefix < base.len() && prefix < target.len() && base[prefix] == target[prefix] {
        budget
            .consume_candidates(1)
            .map_err(|_| UnverifiedReason::ResourceLimit)?;
        prefix += 1;
    }
    let mut base_end = base.len();
    let mut target_end = target.len();
    while base_end > prefix && target_end > prefix && base[base_end - 1] == target[target_end - 1] {
        budget
            .consume_candidates(1)
            .map_err(|_| UnverifiedReason::ResourceLimit)?;
        base_end -= 1;
        target_end -= 1;
    }
    let old = &base[prefix..base_end];
    let new = &target[prefix..target_end];
    let mut changes = Vec::new();
    if old.len() == 1 && new.len() == 1 && old[0].len() == new[0].len() {
        for column in 0..old[0].len() {
            budget
                .consume_candidates(1)
                .map_err(|_| UnverifiedReason::ResourceLimit)?;
            if old[0][column] != new[0][column] {
                add_change(
                    &mut changes,
                    budget,
                    WorkerChange {
                        operation: Some(ChangeOperation::Modified),
                        relocation: None,
                        facet: "csv_cell".to_owned(),
                        base: Some(cell_locator(prefix, column)?),
                        target: Some(cell_locator(prefix, column)?),
                        reason_code: "cell_value_changed".to_owned(),
                    },
                )?;
            }
        }
    } else if old.is_empty() {
        for row in prefix..target_end {
            add_change(
                &mut changes,
                budget,
                WorkerChange {
                    operation: Some(ChangeOperation::Added),
                    relocation: None,
                    facet: "csv_row".to_owned(),
                    base: None,
                    target: Some(cell_locator(row, 0)?),
                    reason_code: "row_added".to_owned(),
                },
            )?;
        }
    } else if new.is_empty() {
        for row in prefix..base_end {
            add_change(
                &mut changes,
                budget,
                WorkerChange {
                    operation: Some(ChangeOperation::Removed),
                    relocation: None,
                    facet: "csv_row".to_owned(),
                    base: Some(cell_locator(row, 0)?),
                    target: None,
                    reason_code: "row_removed".to_owned(),
                },
            )?;
        }
    } else {
        return Err(UnverifiedReason::AmbiguousAlignment);
    }
    Ok(changes)
}

fn has_duplicate_rows(rows: &[Vec<String>]) -> bool {
    let mut seen = HashSet::new();
    rows.iter().any(|row| !seen.insert(row))
}

fn compare_added_or_removed_columns(
    base: &[Vec<String>],
    target: &[Vec<String>],
    budget: &mut ComparisonBudget,
) -> Result<Option<Vec<WorkerChange>>, UnverifiedReason> {
    let Some((base_width, target_width)) = base
        .first()
        .zip(target.first())
        .map(|(b, t)| (b.len(), t.len()))
    else {
        return Ok(None);
    };
    if base_width == target_width || base_width == 0 || target_width == 0 {
        return Ok(None);
    }
    let common = base_width.min(target_width);
    if !base
        .iter()
        .zip(target)
        .all(|(b, t)| b[..common] == t[..common])
    {
        return Ok(None);
    }
    let mut changes = Vec::new();
    for row in 0..base.len() {
        for column in common..base_width.max(target_width) {
            let added = target_width > base_width;
            add_change(
                &mut changes,
                budget,
                WorkerChange {
                    operation: Some(if added {
                        ChangeOperation::Added
                    } else {
                        ChangeOperation::Removed
                    }),
                    relocation: None,
                    facet: "csv_column".to_owned(),
                    base: if added {
                        None
                    } else {
                        Some(cell_locator(row, column)?)
                    },
                    target: if added {
                        Some(cell_locator(row, column)?)
                    } else {
                        None
                    },
                    reason_code: if added {
                        "column_added"
                    } else {
                        "column_removed"
                    }
                    .to_owned(),
                },
            )?;
        }
    }
    Ok(Some(changes))
}

fn add_change(
    changes: &mut Vec<WorkerChange>,
    budget: &mut ComparisonBudget,
    change: WorkerChange,
) -> Result<(), UnverifiedReason> {
    if changes.len() >= MAX_CHANGES || budget.consume_changes(1).is_err() {
        return Err(UnverifiedReason::ResourceLimit);
    }
    changes.push(change);
    Ok(())
}

fn cell_locator(row: usize, column: usize) -> Result<SourceLocator, UnverifiedReason> {
    let row = u32::try_from(row + 1).map_err(|_| UnverifiedReason::ResourceLimit)?;
    let column = u32::try_from(column + 1).map_err(|_| UnverifiedReason::ResourceLimit)?;
    Ok(SourceLocator::CsvCell { row, column })
}

fn unverified(request: &WorkerDiffRequest, reason: UnverifiedReason) -> WorkerDiffResponse {
    response(
        request,
        DiffCoverage::None,
        Vec::new(),
        vec![WorkerUnverifiedRegion {
            base: Some(SourceLocator::ContentItem),
            target: Some(SourceLocator::ContentItem),
            reason,
            navigation_hint: Some("原本の両側を確認してください".to_owned()),
        }],
    )
}

fn response(
    request: &WorkerDiffRequest,
    coverage: DiffCoverage,
    changes: Vec<WorkerChange>,
    unverified_regions: Vec<WorkerUnverifiedRegion>,
) -> WorkerDiffResponse {
    WorkerDiffResponse {
        protocol_version: request.protocol_version,
        diff_profile_version: request.diff_profile_version,
        resource_profile_version: request.resource_profile_version,
        base_raw_sha256: request.base_raw_sha256,
        base_size_bytes: request.base_size_bytes,
        target_raw_sha256: request.target_raw_sha256,
        target_size_bytes: request.target_size_bytes,
        format: request.format,
        coverage,
        changes,
        unverified_regions,
        parser_provenance: PARSER_PROVENANCE.to_owned(),
    }
}
