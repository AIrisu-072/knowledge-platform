use std::collections::{BTreeMap, BTreeSet};

use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, FormatId, RelocationKind, SourceLocator,
    UnverifiedReason, WorkerAncillaryChange, WorkerChange, WorkerDiffRequest, WorkerDiffResponse,
    WorkerUnverifiedRegion,
};
use document_semantic_inspection_worker::{
    AdapterProfile, SemanticAdapter, SpreadsheetAdapter, WorkerFailureCode,
};
use rxls::{Cell, Workbook};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::WorkerError;

const MAX_COMBINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_SHEETS: usize = 1_024;
const MAX_CELLS: usize = 1_000_000;
const PARSER_PROVENANCE: &str =
    "document-diff-xlsx-v0;dsi-spreadsheet-v0;rxls=0.1.3;calamine=0.36.1;quick-xml=0.42.0";

#[derive(Debug, Clone, Copy, Default)]
pub struct SpreadsheetComparator;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CellMeaning {
    value: Option<String>,
    formula: Option<String>,
}

impl SpreadsheetComparator {
    pub fn xlsx(
        request: &WorkerDiffRequest,
        base: &[u8],
        target: &[u8],
        budget: &mut ComparisonBudget,
    ) -> Result<WorkerDiffResponse, WorkerError> {
        if request.format != FormatId::Xlsx {
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
        if base.len().saturating_add(target.len()) > MAX_COMBINED_BYTES {
            return Ok(unverified(request, UnverifiedReason::ResourceLimit));
        }
        let old_inspection =
            match SpreadsheetAdapter::XLSX.inspect(base, &AdapterProfile::default()) {
                Ok(value) => value,
                Err(error) => return Ok(unverified(request, inspection_reason(error.code()))),
            };
        let new_inspection =
            match SpreadsheetAdapter::XLSX.inspect(target, &AdapterProfile::default()) {
                Ok(value) => value,
                Err(error) => return Ok(unverified(request, inspection_reason(error.code()))),
            };
        let ancillary_changes = editorial_change(
            old_inspection.editorial_provenance(),
            new_inspection.editorial_provenance(),
        );
        if old_inspection.semantic_fingerprint() == new_inspection.semantic_fingerprint() {
            return Ok(response(
                request,
                DiffCoverage::Full,
                vec![],
                vec![],
                ancillary_changes,
            ));
        }
        let old = match Workbook::open(base) {
            Ok(value) => value,
            Err(_) => return Ok(unverified(request, UnverifiedReason::CorruptedSource)),
        };
        let new = match Workbook::open(target) {
            Ok(value) => value,
            Err(_) => return Ok(unverified(request, UnverifiedReason::CorruptedSource)),
        };
        let mut changes = Vec::new();
        let result = compare_workbooks(
            &old,
            &new,
            old_inspection.external_dependencies(),
            new_inspection.external_dependencies(),
            budget,
            &mut changes,
        );
        let mut regions = Vec::new();
        if let Err(reason) = result {
            regions.push(unverified_region(reason));
        }
        if changes.is_empty() {
            push_content_change(&mut changes);
            regions.push(unverified_region(
                UnverifiedReason::UnsupportedSemanticConstruct,
            ));
        }
        let coverage = if regions.is_empty() {
            DiffCoverage::Full
        } else {
            DiffCoverage::Partial
        };
        Ok(response(
            request,
            coverage,
            changes,
            regions,
            ancillary_changes,
        ))
    }
}

fn compare_workbooks(
    base: &Workbook,
    target: &Workbook,
    base_external: &[document_semantic_inspection_core::ExternalDependency],
    target_external: &[document_semantic_inspection_core::ExternalDependency],
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
) -> Result<(), UnverifiedReason> {
    if base.sheets.len() > MAX_SHEETS || target.sheets.len() > MAX_SHEETS {
        return Err(UnverifiedReason::ResourceLimit);
    }
    let old_names: BTreeMap<_, _> = base
        .sheets
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();
    let new_names: BTreeMap<_, _> = target
        .sheets
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();
    if old_names.len() != base.sheets.len() || new_names.len() != target.sheets.len() {
        return Err(UnverifiedReason::AmbiguousAlignment);
    }
    let names: BTreeSet<_> = old_names.keys().chain(new_names.keys()).copied().collect();
    for name in names {
        let old_index = old_names.get(name).copied();
        let new_index = new_names.get(name).copied();
        let path = sheet_path(name);
        match (old_index, new_index) {
            (None, Some(_)) => push(
                changes,
                budget,
                Some(ChangeOperation::Added),
                None,
                "xlsx_sheet",
                None,
                Some(office(&path)),
                "sheet_added",
            )?,
            (Some(_), None) => push(
                changes,
                budget,
                Some(ChangeOperation::Removed),
                None,
                "xlsx_sheet",
                Some(office(&path)),
                None,
                "sheet_removed",
            )?,
            (Some(old_index), Some(new_index)) => {
                if old_index != new_index {
                    push(
                        changes,
                        budget,
                        None,
                        Some(RelocationKind::Reordered),
                        "xlsx_sheet",
                        Some(office(&path)),
                        Some(office(&path)),
                        "sheet_order_changed",
                    )?;
                }
                let old_sheet = &base.sheets[old_index];
                let new_sheet = &target.sheets[new_index];
                if format!("{:?}", old_sheet.visible()) != format!("{:?}", new_sheet.visible()) {
                    push(
                        changes,
                        budget,
                        Some(ChangeOperation::Modified),
                        None,
                        "xlsx_sheet_visibility",
                        Some(office(&path)),
                        Some(office(&path)),
                        "sheet_visibility_changed",
                    )?;
                }
                compare_sheet(name, old_sheet, new_sheet, budget, changes)?;
            }
            (None, None) => unreachable!(),
        }
    }
    if base.date1904 != target.date1904 {
        let locator = office("xl/workbook.xml");
        push(
            changes,
            budget,
            Some(ChangeOperation::Modified),
            None,
            "xlsx_date_system",
            Some(locator.clone()),
            Some(locator),
            "date_system_changed",
        )?;
    }
    let mut old_names = base.defined_names.clone();
    let mut new_names = target.defined_names.clone();
    old_names.sort();
    new_names.sort();
    let mut old_local: Vec<_> = base
        .local_defined_names
        .iter()
        .map(|v| format!("{}:{}:{}", v.sheet, v.name, v.refers_to))
        .collect();
    let mut new_local: Vec<_> = target
        .local_defined_names
        .iter()
        .map(|v| format!("{}:{}:{}", v.sheet, v.name, v.refers_to))
        .collect();
    old_local.sort();
    new_local.sort();
    if old_names != new_names || old_local != new_local {
        let locator = office("xl/workbook.xml#definedNames");
        push(
            changes,
            budget,
            Some(ChangeOperation::Modified),
            None,
            "xlsx_named_range",
            Some(locator.clone()),
            Some(locator),
            "defined_name_changed",
        )?;
    }
    let external = |items: &[document_semantic_inspection_core::ExternalDependency]| {
        items
            .iter()
            .filter(|v| v.dependency_kind != "hyperlink")
            .map(|v| (v.dependency_kind.clone(), v.normalized_reference.clone()))
            .collect::<BTreeSet<_>>()
    };
    if external(base_external) != external(target_external) {
        push(
            changes,
            budget,
            Some(ChangeOperation::Modified),
            None,
            "xlsx_external_reference",
            Some(SourceLocator::ContentItem),
            Some(SourceLocator::ContentItem),
            "external_dependency_changed",
        )?;
    }
    Ok(())
}

fn compare_sheet(
    name: &str,
    base: &rxls::Sheet,
    target: &rxls::Sheet,
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
) -> Result<(), UnverifiedReason> {
    let old_cells: BTreeMap<_, _> = base
        .cells()
        .map(|(row, col, cell)| ((row, col), cell))
        .collect();
    let new_cells: BTreeMap<_, _> = target
        .cells()
        .map(|(row, col, cell)| ((row, col), cell))
        .collect();
    if old_cells.len() > MAX_CELLS || new_cells.len() > MAX_CELLS {
        return Err(UnverifiedReason::ResourceLimit);
    }
    let positions: BTreeSet<_> = old_cells.keys().chain(new_cells.keys()).copied().collect();
    let row_reorder = compare_row_alignment(&old_cells, &new_cells)?;
    if let Some(moves) = &row_reorder {
        for (old_row, new_row) in moves {
            push(
                changes,
                budget,
                None,
                Some(RelocationKind::Reordered),
                "xlsx_row",
                Some(row_locator(name, *old_row)),
                Some(row_locator(name, *new_row)),
                "unique_row_reordered",
            )?;
        }
    }
    for (row, col) in positions {
        if row_reorder.is_some() {
            break;
        }
        budget
            .consume_candidates(1)
            .map_err(|_| UnverifiedReason::ResourceLimit)?;
        let old = old_cells.get(&(row, col)).map(|cell| cell_meaning(cell));
        let new = new_cells.get(&(row, col)).map(|cell| cell_meaning(cell));
        if old == new {
            continue;
        }
        let locator = SourceLocator::SheetCell {
            sheet: name.to_owned(),
            cell: cell_address(row, col),
        };
        let operation = match (&old, &new) {
            (None, Some(_)) => ChangeOperation::Added,
            (Some(_), None) => ChangeOperation::Removed,
            _ => ChangeOperation::Modified,
        };
        let facet = if old.as_ref().and_then(|v| v.formula.as_ref())
            != new.as_ref().and_then(|v| v.formula.as_ref())
        {
            "xlsx_formula"
        } else {
            "xlsx_cell_value"
        };
        push(
            changes,
            budget,
            Some(operation),
            None,
            facet,
            old.map(|_| locator.clone()),
            new.map(|_| locator),
            "cell_semantics_changed",
        )?;
    }
    let path = sheet_path(name);
    let mut old_merges = base.merged_ranges().to_vec();
    let mut new_merges = target.merged_ranges().to_vec();
    old_merges.sort();
    new_merges.sort();
    if old_merges != new_merges {
        push_sheet_facet(changes, budget, &path, "xlsx_merge", "merged_range_changed")?;
    }
    let mut old_links = base.hyperlinks().to_vec();
    let mut new_links = target.hyperlinks().to_vec();
    old_links.sort();
    new_links.sort();
    if old_links != new_links {
        push_sheet_facet(changes, budget, &path, "xlsx_link", "hyperlink_changed")?;
    }
    if table_projection(base) != table_projection(target) {
        push_sheet_facet(
            changes,
            budget,
            &path,
            "xlsx_table",
            "table_structure_changed",
        )?;
    }
    if chart_projection(base) != chart_projection(target) {
        push_sheet_facet(
            changes,
            budget,
            &path,
            "xlsx_chart",
            "chart_source_or_data_changed",
        )?;
    }
    if image_projection(base) != image_projection(target) {
        push_sheet_facet(changes, budget, &path, "xlsx_image", "image_changed")?;
    }
    Ok(())
}

type CellMap<'a> = BTreeMap<(u32, u16), &'a Cell>;
type RowSignature = Vec<(u16, CellMeaning)>;

fn compare_row_alignment(
    base: &CellMap<'_>,
    target: &CellMap<'_>,
) -> Result<Option<Vec<(u32, u32)>>, UnverifiedReason> {
    let old_rows = row_signatures(base);
    let new_rows = row_signatures(target);
    if old_rows == new_rows {
        return Ok(None);
    }
    let mut old_values: Vec<_> = old_rows.values().cloned().collect();
    let mut new_values: Vec<_> = new_rows.values().cloned().collect();
    old_values.sort();
    new_values.sort();
    if old_values == new_values {
        if old_values.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(UnverifiedReason::AmbiguousAlignment);
        }
        let old_index: BTreeMap<_, _> = old_rows
            .into_iter()
            .map(|(row, value)| (value, row))
            .collect();
        let moves = new_rows
            .into_iter()
            .filter_map(|(new_row, value)| {
                let old_row = old_index[&value];
                (old_row != new_row).then_some((old_row, new_row))
            })
            .collect();
        return Ok(Some(moves));
    }
    let old_unique: BTreeMap<_, _> = old_rows.iter().map(|(row, value)| (value, row)).collect();
    for (new_row, value) in &new_rows {
        if let Some(old_row) = old_unique.get(value)
            && *old_row != new_row
        {
            return Err(UnverifiedReason::AmbiguousAlignment);
        }
    }
    Ok(None)
}

fn row_signatures(cells: &CellMap<'_>) -> BTreeMap<u32, RowSignature> {
    let mut rows: BTreeMap<u32, RowSignature> = BTreeMap::new();
    for (&(row, column), cell) in cells {
        rows.entry(row)
            .or_default()
            .push((column, cell_meaning(cell)));
    }
    rows
}

fn row_locator(sheet: &str, row: u32) -> SourceLocator {
    SourceLocator::SheetCell {
        sheet: sheet.to_owned(),
        cell: format!("A{}:XFD{}", row + 1, row + 1),
    }
}

fn cell_meaning(cell: &Cell) -> CellMeaning {
    match cell {
        Cell::Formula { formula, .. } => CellMeaning {
            value: None,
            formula: Some(formula.clone()),
        },
        Cell::Text(value) => CellMeaning {
            value: Some(format!("text:{value}")),
            formula: None,
        },
        Cell::Number(value) => CellMeaning {
            value: Some(format!("number:{}", value.to_bits())),
            formula: None,
        },
        Cell::Date(value) => CellMeaning {
            value: Some(format!("date:{}", value.to_bits())),
            formula: None,
        },
        Cell::Bool(value) => CellMeaning {
            value: Some(format!("bool:{value}")),
            formula: None,
        },
        Cell::Error(value) => CellMeaning {
            value: Some(format!("error:{value}")),
            formula: None,
        },
    }
}

fn table_projection(sheet: &rxls::Sheet) -> Vec<Value> {
    let mut values: Vec<_> = sheet
        .tables()
        .iter()
        .map(|table| json!({"name":table.name(), "range":table.range(), "columns":table.columns()}))
        .collect();
    values.sort_by_key(|value| value["name"].as_str().unwrap_or_default().to_owned());
    values
}

fn chart_projection(sheet: &rxls::Sheet) -> Vec<Value> {
    sheet
        .charts()
        .iter()
        .map(|chart| {
            json!({
                "kind": format!("{:?}", chart.kind).to_lowercase(),
                "title": chart.title,
                "series": chart.series.iter().map(|series| json!({
                    "name":series.name, "categories":series.categories, "values":series.values,
                    "bubble_sizes":series.bubble_sizes,
                })).collect::<Vec<_>>(),
                "legend":chart.legend, "data_labels":chart.data_labels,
                "x_axis_title":chart.x_axis_title, "y_axis_title":chart.y_axis_title,
                "from":chart.from, "to":chart.to,
            })
        })
        .collect()
}

fn image_projection(sheet: &rxls::Sheet) -> Vec<Value> {
    sheet
        .images()
        .iter()
        .map(|image| {
            json!({
                "format":format!("{:?}", image.format).to_lowercase(),
                "from":image.from, "to":image.to,
                "sha256":Sha256::digest(&image.data).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            })
        })
        .collect()
}

fn cell_address(row: u32, column: u16) -> String {
    let mut n = usize::from(column) + 1;
    let mut label = String::new();
    while n > 0 {
        n -= 1;
        label.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    format!("{label}{}", row + 1)
}

fn sheet_path(name: &str) -> String {
    format!("xl/workbook.xml#sheet[{name}]")
}

fn office(path: &str) -> SourceLocator {
    SourceLocator::OfficePath {
        path: path.to_owned(),
    }
}

fn push_sheet_facet(
    changes: &mut Vec<WorkerChange>,
    budget: &mut ComparisonBudget,
    path: &str,
    facet: &str,
    reason: &str,
) -> Result<(), UnverifiedReason> {
    let locator = office(path);
    push(
        changes,
        budget,
        Some(ChangeOperation::Modified),
        None,
        facet,
        Some(locator.clone()),
        Some(locator),
        reason,
    )
}

#[allow(clippy::too_many_arguments)]
fn push(
    changes: &mut Vec<WorkerChange>,
    budget: &mut ComparisonBudget,
    operation: Option<ChangeOperation>,
    relocation: Option<RelocationKind>,
    facet: &str,
    base: Option<SourceLocator>,
    target: Option<SourceLocator>,
    reason: &str,
) -> Result<(), UnverifiedReason> {
    budget
        .consume_changes(1)
        .map_err(|_| UnverifiedReason::ResourceLimit)?;
    changes.push(WorkerChange {
        operation,
        relocation,
        facet: facet.to_owned(),
        base,
        target,
        reason_code: reason.to_owned(),
    });
    Ok(())
}

fn push_content_change(changes: &mut Vec<WorkerChange>) {
    changes.push(WorkerChange {
        operation: Some(ChangeOperation::Modified),
        relocation: None,
        facet: "xlsx_content".to_owned(),
        base: Some(SourceLocator::ContentItem),
        target: Some(SourceLocator::ContentItem),
        reason_code: "dsi_fingerprint_changed_location_unknown".to_owned(),
    });
}

fn editorial_change(
    base: &document_semantic_inspection_core::EditorialProvenance,
    target: &document_semantic_inspection_core::EditorialProvenance,
) -> Vec<WorkerAncillaryChange> {
    if base == target {
        return vec![];
    }
    vec![WorkerAncillaryChange {
        kind: "xlsx_editorial".to_owned(),
        base_digest: Some(
            Sha256::digest(serde_json::to_vec(base).expect("typed editorial evidence")).into(),
        ),
        target_digest: Some(
            Sha256::digest(serde_json::to_vec(target).expect("typed editorial evidence")).into(),
        ),
    }]
}

fn inspection_reason(code: WorkerFailureCode) -> UnverifiedReason {
    match code {
        WorkerFailureCode::InspectionResourceLimitExceeded
        | WorkerFailureCode::InspectionTimeout => UnverifiedReason::ResourceLimit,
        WorkerFailureCode::SemanticExtractionFailed | WorkerFailureCode::ParserDisagreement => {
            UnverifiedReason::CorruptedSource
        }
        _ => UnverifiedReason::UnsupportedSemanticConstruct,
    }
}

fn unverified_region(reason: UnverifiedReason) -> WorkerUnverifiedRegion {
    WorkerUnverifiedRegion {
        base: Some(SourceLocator::ContentItem),
        target: Some(SourceLocator::ContentItem),
        reason,
        navigation_hint: Some("未比較範囲を両原本で確認してください".to_owned()),
    }
}

fn unverified(request: &WorkerDiffRequest, reason: UnverifiedReason) -> WorkerDiffResponse {
    response(
        request,
        DiffCoverage::None,
        vec![],
        vec![unverified_region(reason)],
        vec![],
    )
}

fn response(
    request: &WorkerDiffRequest,
    coverage: DiffCoverage,
    changes: Vec<WorkerChange>,
    unverified_regions: Vec<WorkerUnverifiedRegion>,
    ancillary_changes: Vec<WorkerAncillaryChange>,
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
        ancillary_changes,
        parser_provenance: PARSER_PROVENANCE.to_owned(),
    }
}
