use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use calamine::{Data as CalamineData, Reader, Xlsx, open_workbook_auto_from_rs};
use document_semantic_inspection_core::{
    CapabilityState, CommentEvidence, EditorialProvenance, ExternalDependency, FormatId,
};
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use rxls::{Cell, StyleLossKind, Workbook};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::shell::{MAX_STRUCTURED_RESULT_BYTES, serialized_json_len_bounded};
use crate::{WorkerFailure, WorkerFailureCode};

use super::{AdapterProfile, SemanticAdapter, SemanticAdapterOutput};

const MAX_SPREADSHEET_PART_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SPREADSHEET_SHEETS: usize = 1_024;
const MAX_SPREADSHEET_CELLS: usize = 1_000_000;
const MAX_SPREADSHEET_IMAGES: usize = 4_096;
const MAX_SPREADSHEET_EMBEDDED_OBJECTS: usize = 1_024;
// The five supported ODBC fields have a combined validated maximum below 1 KiB.
const MAX_QUALIFIED_ODBC_CONNECTION_BYTES: usize = 1_024;
const XLSX_MAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const XLSM_MAIN_CONTENT_TYPE: &str = "application/vnd.ms-excel.sheet.macroEnabled.main+xml";
const VBA_PROJECT_CONTENT_TYPE: &str = "application/vnd.ms-office.vbaProject";
const EXTERNAL_LINK_PATH_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLinkPath";
const SPREADSHEETML_NAMESPACE: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

const XLSX_PARSER_LIBRARIES: [(&str, &str); 4] = [
    ("rxls", "0.1.3"),
    ("calamine", "0.36.1"),
    ("quick-xml", "0.42.0"),
    ("zip", "8.6.0"),
];
const XLSM_PARSER_LIBRARIES: [(&str, &str); 8] = [
    ("rxls", "0.1.3"),
    ("calamine", "0.36.1"),
    ("quick-xml", "0.42.0"),
    ("zip", "8.6.0"),
    ("ovba", "0.7.1"),
    ("tree-sitter", "0.25.10"),
    ("tree-sitter-language", "0.1.8"),
    (
        "tree-sitter-vba",
        "c691f237b2a703732d4b6a1f01d5b4f73f94d41e",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpreadsheetAdapter {
    format: FormatId,
}

impl SpreadsheetAdapter {
    pub const XLSX: Self = Self {
        format: FormatId::Xlsx,
    };
    pub const XLSM: Self = Self {
        format: FormatId::Xlsm,
    };
}

impl SemanticAdapter for SpreadsheetAdapter {
    fn format(&self) -> FormatId {
        self.format
    }

    fn inspect(
        &self,
        input: &[u8],
        profile: &AdapterProfile,
    ) -> Result<SemanticAdapterOutput, WorkerFailure> {
        super::spreadsheet_package::preflight_spreadsheet_package(input, profile)?;
        enforce_embedded_object_limit(input)?;

        let observed = spreadsheet_format(input)?;
        if observed != Some(self.format) {
            let expected = format_name(self.format);
            let observed = observed.map_or("unknown", format_name);
            return Err(WorkerFailure::new(
                WorkerFailureCode::FormatMismatch,
                format!("expected {expected} package, observed {observed}"),
            ));
        }

        let workbook = Workbook::open(input).map_err(|_| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                "rxls could not parse the SpreadsheetML workbook",
            )
        })?;
        if workbook.text_truncated {
            return Err(failure(
                WorkerFailureCode::InspectionResourceLimitExceeded,
                "rxls text allocation limit was reached",
            ));
        }
        for sheet in &workbook.sheets {
            for loss in sheet.style_losses() {
                match loss.kind {
                    StyleLossKind::DrawingMetadataPartial => {
                        return Err(failure(
                            WorkerFailureCode::UnsupportedSemanticConstruct,
                            "rxls only partially retained SpreadsheetML drawing semantics",
                        ));
                    }
                    StyleLossKind::LimitExceeded => {
                        return Err(failure(
                            WorkerFailureCode::InspectionResourceLimitExceeded,
                            "rxls reached a SpreadsheetML representation limit",
                        ));
                    }
                    _ => {}
                }
            }
        }
        enforce_workbook_semantic_limits(&workbook)?;
        compare_calamine_oracle(input, &workbook)?;

        let mut formula_present = false;
        let mut hidden_present = false;
        let mut external_dependencies = Vec::new();
        let mut external_dependency_bytes = 0;
        let mut editorial = EditorialProvenance::default();
        let mut comment_evidence_bytes = 0;
        let mut sheets = Vec::with_capacity(workbook.sheets.len());

        for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
            let visibility = format!("{:?}", sheet.visible()).to_lowercase();
            hidden_present |= visibility != "visible";

            let mut cells = Vec::new();
            for (row, columns) in sheet.rows() {
                for (col, cell) in columns {
                    formula_present |= matches!(cell, Cell::Formula { .. });
                    cells.push(json!({
                        "row": row,
                        "col": col,
                        "value": cell_projection(cell),
                    }));
                }
            }

            let mut merged_ranges: Vec<_> = sheet.merged_ranges().to_vec();
            merged_ranges.sort();

            let mut hyperlinks: Vec<_> = sheet.hyperlinks().to_vec();
            hyperlinks.sort();
            for (row, col, target) in &hyperlinks {
                validate_external_target(target)?;
                push_bounded_dependency(
                    &mut external_dependencies,
                    &mut external_dependency_bytes,
                    ExternalDependency {
                        dependency_kind: "hyperlink".into(),
                        normalized_reference: target.clone(),
                        source_locator: format!("sheet[{sheet_index}]/R{}C{}", row + 1, col + 1),
                        version_significant: true,
                    },
                )?;
            }

            let mut tables: Vec<Value> = sheet
                .tables()
                .iter()
                .map(|table| {
                    json!({
                        "name": table.name(),
                        "range": table.range(),
                        "columns": table.columns(),
                    })
                })
                .collect();
            tables.sort_by_key(|value| value["name"].as_str().unwrap_or_default().to_owned());

            let images: Vec<Value> = sheet
                .images()
                .iter()
                .map(|image| {
                    // RXLS retains these bytes as-is. This adapter hashes encoded bytes and
                    // does not decode raster data, so the decoded-pixel budget is inapplicable.
                    json!({
                        "format": format!("{:?}", image.format).to_lowercase(),
                        "from": image.from,
                        "to": image.to,
                        "sha256": hex_digest(&image.data),
                    })
                })
                .collect();

            let charts: Vec<Value> = sheet
                .charts()
                .iter()
                .map(|chart| {
                    json!({
                        "kind": format!("{:?}", chart.kind).to_lowercase(),
                        "title": chart.title,
                        "series": chart.series.iter().map(|series| json!({
                            "name": series.name,
                            "categories": series.categories,
                            "values": series.values,
                            "bubble_sizes": series.bubble_sizes,
                        })).collect::<Vec<_>>(),
                        "legend": chart.legend,
                        "data_labels": chart.data_labels,
                        "x_axis_title": chart.x_axis_title,
                        "y_axis_title": chart.y_axis_title,
                        "from": chart.from,
                        "to": chart.to,
                    })
                })
                .collect();

            for comment in sheet.comments() {
                let source_locator = format!(
                    "sheet[{sheet_index}]/R{}C{}",
                    comment.row + 1,
                    comment.col + 1
                );
                enforce_comment_evidence_budget(
                    &mut comment_evidence_bytes,
                    &comment.text,
                    comment.author.as_deref(),
                    &source_locator,
                )?;
                editorial.comments.push(CommentEvidence {
                    author_label: comment.author.clone(),
                    timestamp: None,
                    resolved_state: "unknown".into(),
                    source_locator,
                    content: comment.text.clone(),
                });
            }

            sheets.push(json!({
                "index": sheet_index,
                "name": sheet.name,
                "visibility": visibility,
                "cells": cells,
                "merged_ranges": merged_ranges,
                "hyperlinks": hyperlinks,
                "tables": tables,
                "images": images,
                "charts": charts,
            }));
        }

        let mut defined_names = workbook.defined_names.clone();
        defined_names.sort();
        let mut local_defined_names: Vec<_> = workbook
            .local_defined_names
            .iter()
            .map(|name| {
                (
                    name.sheet.clone(),
                    name.name.clone(),
                    name.refers_to.clone(),
                )
            })
            .collect();
        local_defined_names.sort();

        for dependency in inspect_external_package_definitions(input)? {
            push_bounded_dependency(
                &mut external_dependencies,
                &mut external_dependency_bytes,
                dependency,
            )?;
        }
        external_dependencies.sort_by(|left, right| {
            (
                left.dependency_kind.as_str(),
                left.normalized_reference.as_str(),
                left.source_locator.as_str(),
            )
                .cmp(&(
                    right.dependency_kind.as_str(),
                    right.normalized_reference.as_str(),
                    right.source_locator.as_str(),
                ))
        });
        external_dependencies.dedup();
        let mut external_projection: Vec<_> = external_dependencies
            .iter()
            .map(|dependency| {
                (
                    dependency.dependency_kind.clone(),
                    dependency.normalized_reference.clone(),
                )
            })
            .collect();
        external_projection.sort();
        external_projection.dedup();

        let (vba_projection, vba_state) = if self.format == FormatId::Xlsm {
            let inspection = super::vba::inspect_xlsm(input)?;
            let state = if inspection.module_count == 0 {
                CapabilityState::Absent
            } else {
                CapabilityState::Present
            };
            (inspection.projection, state)
        } else {
            (Value::Null, CapabilityState::NotRepresentable)
        };

        let projection = json!({
            "format": format_name(self.format),
            "date1904": workbook.date1904,
            "defined_names": defined_names,
            "local_defined_names": local_defined_names,
            "external_dependencies": external_projection,
            "sheets": sheets,
            "vba": vba_projection,
        });
        let semantic_projection = super::canonical_json_bytes(&projection).map_err(|_| {
            failure(
                WorkerFailureCode::InvalidWorkerResult,
                "spreadsheet semantic projection could not be serialized",
            )
        })?;

        let parser_libraries = if self.format == FormatId::Xlsm {
            &XLSM_PARSER_LIBRARIES[..]
        } else {
            &XLSX_PARSER_LIBRARIES[..]
        };
        let output = SemanticAdapterOutput::from_projection(
            &semantic_projection,
            &[
                "reader_content",
                "workbook_structure",
                "formula_logic",
                "hidden_content",
                "vba_logic",
                "external_references",
            ],
            "spreadsheet",
            parser_libraries,
        )
        .with_capability_state("formula_logic", present_or_absent(formula_present))?
        .with_capability_state("hidden_content", present_or_absent(hidden_present))?
        .with_capability_state("vba_logic", vba_state)?
        .with_capability_state(
            "external_references",
            present_or_absent(!external_dependencies.is_empty()),
        )?
        .with_editorial_provenance(editorial)
        .with_external_dependencies(external_dependencies);

        Ok(output)
    }
}

fn present_or_absent(present: bool) -> CapabilityState {
    if present {
        CapabilityState::Present
    } else {
        CapabilityState::Absent
    }
}

fn enforce_workbook_semantic_limits(workbook: &Workbook) -> Result<(), WorkerFailure> {
    if workbook.sheets.len() > MAX_SPREADSHEET_SHEETS {
        return Err(resource_limit(
            "SpreadsheetML sheet count exceeds the limit",
        ));
    }
    enforce_hyperlink_oracle_budget(workbook.sheets.iter().flat_map(|sheet| {
        sheet
            .hyperlinks()
            .iter()
            .map(|(_, _, target)| target.as_str())
    }))?;

    let mut cell_count = 0usize;
    let mut image_count = 0usize;
    for sheet in &workbook.sheets {
        for _ in sheet.cells() {
            cell_count = cell_count
                .checked_add(1)
                .ok_or_else(|| resource_limit("SpreadsheetML cell count exceeds the limit"))?;
            if cell_count > MAX_SPREADSHEET_CELLS {
                return Err(resource_limit("SpreadsheetML cell count exceeds the limit"));
            }
        }
        image_count = image_count
            .checked_add(sheet.images().len())
            .ok_or_else(|| resource_limit("SpreadsheetML image count exceeds the limit"))?;
        if image_count > MAX_SPREADSHEET_IMAGES {
            return Err(resource_limit(
                "SpreadsheetML image count exceeds the limit",
            ));
        }
    }
    Ok(())
}

fn enforce_hyperlink_oracle_budget<'a>(
    targets: impl IntoIterator<Item = &'a str>,
) -> Result<(), WorkerFailure> {
    let mut observed = 0usize;
    for target in targets {
        // The fixed allowance covers the comparison tuple, locator, JSON keys, and
        // punctuation for each target before comparison Vecs are built.
        observed = observed
            .checked_add(target.len())
            .and_then(|bytes| bytes.checked_add(128))
            .ok_or_else(|| resource_limit("SpreadsheetML hyperlink evidence exceeds the limit"))?;
        if observed > MAX_STRUCTURED_RESULT_BYTES {
            return Err(resource_limit(
                "SpreadsheetML hyperlink evidence exceeds the limit",
            ));
        }
    }
    Ok(())
}

fn enforce_embedded_object_limit(input: &[u8]) -> Result<(), WorkerFailure> {
    let mut archive = ZipArchive::new(Cursor::new(input)).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "SpreadsheetML package could not be opened",
        )
    })?;
    let mut embedded_count = 0usize;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|_| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                "SpreadsheetML package entry could not be read",
            )
        })?;
        let path = entry.name();
        if path.split('/').any(|segment| segment == "embeddings") {
            embedded_count = embedded_count.checked_add(1).ok_or_else(|| {
                resource_limit("SpreadsheetML embedded object count exceeds the limit")
            })?;
            if embedded_count > MAX_SPREADSHEET_EMBEDDED_OBJECTS {
                return Err(resource_limit(
                    "SpreadsheetML embedded object count exceeds the limit",
                ));
            }
        }
    }
    Ok(())
}

fn format_name(format: FormatId) -> &'static str {
    match format {
        FormatId::Xlsx => "xlsx",
        FormatId::Xlsm => "xlsm",
        _ => "other",
    }
}

fn failure(code: WorkerFailureCode, message: &'static str) -> WorkerFailure {
    WorkerFailure::new(code, message)
}

fn resource_limit(message: &'static str) -> WorkerFailure {
    failure(WorkerFailureCode::InspectionResourceLimitExceeded, message)
}

fn spreadsheet_format(input: &[u8]) -> Result<Option<FormatId>, WorkerFailure> {
    let mut archive = ZipArchive::new(Cursor::new(input)).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "SpreadsheetML package could not be opened",
        )
    })?;
    let content_types = read_zip_part_limited(&mut archive, "[Content_Types].xml")?;
    let mut reader = quick_xml::Reader::from_reader(content_types.as_slice());
    let mut saw_xlsx = false;
    let mut saw_xlsm = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event) | Event::Empty(event))
                if matches!(event.local_name().as_ref(), "Override" | "Default") =>
            {
                for attribute in event.attributes() {
                    let attribute = attribute.map_err(|_| {
                        failure(
                            WorkerFailureCode::SemanticExtractionFailed,
                            "SpreadsheetML content-types XML is malformed",
                        )
                    })?;
                    if attribute.key.local_name().as_ref() != "ContentType" {
                        continue;
                    }
                    let value = attribute
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| {
                            failure(
                                WorkerFailureCode::SemanticExtractionFailed,
                                "SpreadsheetML content-types XML is malformed",
                            )
                        })?;
                    saw_xlsx |= value == XLSX_MAIN_CONTENT_TYPE;
                    saw_xlsm |=
                        value == XLSM_MAIN_CONTENT_TYPE || value == VBA_PROJECT_CONTENT_TYPE;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    "SpreadsheetML content-types XML is malformed",
                ));
            }
        }
    }

    Ok(if saw_xlsm {
        Some(FormatId::Xlsm)
    } else if saw_xlsx {
        Some(FormatId::Xlsx)
    } else {
        None
    })
}

fn cell_projection(cell: &Cell) -> Value {
    match cell {
        Cell::Text(value) => json!({"kind":"text","value":value}),
        Cell::Number(value) => json!({"kind":"number","bits":value.to_bits()}),
        Cell::Date(value) => json!({"kind":"date","bits":value.to_bits()}),
        Cell::Bool(value) => json!({"kind":"bool","value":value}),
        Cell::Error(value) => json!({"kind":"error","value":value}),
        Cell::Formula { formula, .. } => json!({"kind":"formula","source":formula}),
    }
}

fn compare_calamine_oracle(input: &[u8], workbook: &Workbook) -> Result<(), WorkerFailure> {
    let mut oracle = open_workbook_auto_from_rs(Cursor::new(input))
        .map_err(|_| parser_disagreement("Calamine could not open the workbook"))?;
    let mut xlsx_oracle: Xlsx<_> = Xlsx::new(Cursor::new(input))
        .map_err(|_| parser_disagreement("Calamine XLSX reader could not open the workbook"))?;

    let rxls_names: Vec<String> = workbook
        .sheets
        .iter()
        .map(|sheet| sheet.name.clone())
        .collect();
    let calamine_names = oracle.sheet_names();
    if rxls_names != calamine_names {
        return Err(parser_disagreement("sheet order or names disagree"));
    }

    let calamine_sheet_meta = oracle.sheets_metadata();
    if calamine_sheet_meta.len() != workbook.sheets.len() {
        return Err(parser_disagreement("sheet metadata counts disagree"));
    }
    for (sheet, oracle_sheet) in workbook.sheets.iter().zip(calamine_sheet_meta.iter()) {
        let rxls_visibility = format!("{:?}", sheet.visible()).to_lowercase();
        let calamine_visibility = format!("{:?}", oracle_sheet.visible).to_lowercase();
        let rxls_type = format!("{:?}", sheet.sheet_type()).to_lowercase();
        let calamine_type = format!("{:?}", oracle_sheet.typ).to_lowercase();
        if sheet.name != oracle_sheet.name
            || rxls_visibility != calamine_visibility
            || rxls_type != calamine_type
        {
            return Err(parser_disagreement("sheet metadata disagree"));
        }
    }

    let mut rxls_names = workbook.defined_names.clone();
    let mut calamine_defined_names = oracle.defined_names().to_vec();
    rxls_names.sort();
    calamine_defined_names.sort();
    if rxls_names != calamine_defined_names {
        return Err(parser_disagreement("defined names disagree"));
    }

    compare_cells_and_formulas(workbook, &mut oracle)?;
    compare_hyperlinks(workbook, &mut xlsx_oracle)?;

    Ok(())
}

fn compare_cells_and_formulas(
    workbook: &Workbook,
    oracle: &mut calamine::Sheets<Cursor<&[u8]>>,
) -> Result<(), WorkerFailure> {
    for sheet in &workbook.sheets {
        let rxls_formulas: BTreeMap<(u32, u16), String> = sheet
            .cells()
            .filter_map(|(row, col, cell)| match cell {
                Cell::Formula { formula, .. } => Some(((row, col), formula.clone())),
                _ => None,
            })
            .collect();
        let formula_coords: BTreeSet<(u32, u16)> = rxls_formulas.keys().copied().collect();

        let formula_range = oracle
            .worksheet_formula(&sheet.name)
            .map_err(|_| parser_disagreement("Calamine could not read worksheet formulas"))?;
        let formula_start = formula_range.start().unwrap_or((0, 0));
        let mut calamine_formulas = BTreeMap::new();
        for (row, col, formula) in formula_range.used_cells() {
            if formula.is_empty() {
                continue;
            }
            let position = calamine_position(formula_start, row, col)?;
            calamine_formulas.insert(position, formula.clone());
        }
        if rxls_formulas != calamine_formulas {
            return Err(parser_disagreement(
                "formula source differs between parsers",
            ));
        }

        let value_range = oracle
            .worksheet_range(&sheet.name)
            .map_err(|_| parser_disagreement("Calamine could not read worksheet values"))?;
        let value_start = value_range.start().unwrap_or((0, 0));

        let rxls_values: BTreeMap<(u32, u16), String> = sheet
            .cells()
            .filter_map(|(row, col, cell)| {
                if matches!(cell, Cell::Formula { .. }) {
                    None
                } else {
                    Some(((row, col), rxls_oracle_value(cell)))
                }
            })
            .collect();
        let mut calamine_values = BTreeMap::new();
        for (row, col, value) in value_range.used_cells() {
            let position = calamine_position(value_start, row, col)?;
            if formula_coords.contains(&position) {
                continue;
            }
            if let Some(value) = calamine_oracle_value(value) {
                calamine_values.insert(position, value);
            }
        }
        if rxls_values != calamine_values {
            return Err(parser_disagreement(
                "cell values or types differ between parsers",
            ));
        }
    }

    Ok(())
}

fn compare_hyperlinks(
    workbook: &Workbook,
    oracle: &mut Xlsx<Cursor<&[u8]>>,
) -> Result<(), WorkerFailure> {
    for sheet in &workbook.sheets {
        let mut rxls_links: Vec<_> = sheet
            .hyperlinks()
            .iter()
            .map(|(row, col, target)| {
                (
                    (*row, u32::from(*col), *row, u32::from(*col)),
                    format!("target={target};location="),
                )
            })
            .collect();
        rxls_links.sort();

        let oracle_links = oracle
            .hyperlinks_by_sheet_name(&sheet.name)
            .map_err(|_| parser_disagreement("Calamine could not read worksheet hyperlinks"))?;
        enforce_hyperlink_oracle_budget(oracle_links.iter().flat_map(|link| {
            [
                link.target.as_deref().unwrap_or(""),
                link.location.as_deref().unwrap_or(""),
            ]
        }))?;
        let mut calamine_links: Vec<_> = oracle_links
            .into_iter()
            .map(|link| {
                (
                    (
                        link.range.start.0,
                        link.range.start.1,
                        link.range.end.0,
                        link.range.end.1,
                    ),
                    format!(
                        "target={};location={}",
                        link.target.as_deref().unwrap_or(""),
                        link.location.as_deref().unwrap_or("")
                    ),
                )
            })
            .collect();
        calamine_links.sort();
        if rxls_links != calamine_links {
            return Err(parser_disagreement(
                "hyperlink targets or locations differ between parsers",
            ));
        }
    }
    Ok(())
}

fn calamine_position(
    start: (u32, u32),
    row: usize,
    col: usize,
) -> Result<(u32, u16), WorkerFailure> {
    let row = u32::try_from(row).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "row index overflow",
        )
    })?;
    let col = u32::try_from(col).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "column index overflow",
        )
    })?;
    let absolute_row = start.0.checked_add(row).ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "row coordinate overflow",
        )
    })?;
    let absolute_col = start.1.checked_add(col).ok_or_else(|| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "column coordinate overflow",
        )
    })?;
    let absolute_col = u16::try_from(absolute_col).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "column exceeds the XLSX range",
        )
    })?;
    Ok((absolute_row, absolute_col))
}

fn normalized_number_bits(value: f64) -> u64 {
    if value == 0.0 {
        0.0f64.to_bits()
    } else {
        value.to_bits()
    }
}

fn rxls_oracle_value(cell: &Cell) -> String {
    match cell {
        Cell::Text(value) => format!("text:{value}"),
        Cell::Number(value) => format!("number:{:016x}", normalized_number_bits(*value)),
        Cell::Date(value) => format!("date:{:016x}", normalized_number_bits(*value)),
        Cell::Bool(value) => format!("bool:{value}"),
        Cell::Error(value) => format!("error:{value}"),
        Cell::Formula { cached, .. } => rxls_oracle_value(cached),
    }
}

fn calamine_oracle_value(value: &CalamineData) -> Option<String> {
    match value {
        CalamineData::Int(value) => Some(format!(
            "number:{:016x}",
            normalized_number_bits(*value as f64)
        )),
        CalamineData::Float(value) => {
            Some(format!("number:{:016x}", normalized_number_bits(*value)))
        }
        CalamineData::String(value) => Some(format!("text:{value}")),
        CalamineData::Bool(value) => Some(format!("bool:{value}")),
        CalamineData::DateTime(value) => Some(format!(
            "date:{:016x}",
            normalized_number_bits(value.as_f64())
        )),
        CalamineData::DateTimeIso(value) => Some(format!("datetime_iso:{value}")),
        CalamineData::DurationIso(value) => Some(format!("duration_iso:{value}")),
        CalamineData::Error(value) => Some(format!("error:{value}")),
        CalamineData::Empty => None,
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn inspect_external_package_definitions(
    input: &[u8],
) -> Result<Vec<ExternalDependency>, WorkerFailure> {
    let mut archive = ZipArchive::new(Cursor::new(input)).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "SpreadsheetML package could not be opened",
        )
    })?;
    let mut names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|_| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                "SpreadsheetML package entry could not be read",
            )
        })?;
        names.push(entry.name().to_owned());
    }
    names.sort();

    let mut dependencies = Vec::new();
    let mut dependency_bytes = 0;
    for name in names.iter().filter(|name| name.ends_with(".rels")) {
        let data = read_zip_part_limited(&mut archive, name)?;
        for dependency in inspect_external_relationships(&data, name)? {
            push_bounded_dependency(&mut dependencies, &mut dependency_bytes, dependency)?;
        }
    }
    if names.iter().any(|name| name == "xl/connections.xml") {
        let data = read_zip_part_limited(&mut archive, "xl/connections.xml")?;
        for dependency in inspect_connection_definitions(&data)? {
            push_bounded_dependency(&mut dependencies, &mut dependency_bytes, dependency)?;
        }
    }
    Ok(dependencies)
}

fn inspect_external_relationships(
    data: &[u8],
    source_locator: &str,
) -> Result<Vec<ExternalDependency>, WorkerFailure> {
    let mut reader = quick_xml::Reader::from_reader(data);
    let mut dependencies = Vec::new();
    let mut dependency_bytes = 0;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event) | Event::Empty(event))
                if event.local_name().as_ref() == "Relationship" =>
            {
                let relationship_type = attribute_value(&event, "Type")?;
                if relationship_type.as_deref() != Some(EXTERNAL_LINK_PATH_RELATIONSHIP) {
                    continue;
                }
                let target_mode = attribute_value(&event, "TargetMode")?;
                if target_mode.as_deref() != Some("External") {
                    return Err(failure(
                        WorkerFailureCode::UnsupportedSemanticConstruct,
                        "external workbook relationship is not explicitly external",
                    ));
                }
                let target = attribute_value(&event, "Target")?.ok_or_else(|| {
                    failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        "external workbook relationship has no target",
                    )
                })?;
                validate_external_target(&target)?;
                push_bounded_dependency(
                    &mut dependencies,
                    &mut dependency_bytes,
                    ExternalDependency {
                        dependency_kind: "external_workbook".into(),
                        normalized_reference: target,
                        source_locator: source_locator.to_owned(),
                        version_significant: true,
                    },
                )?;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    "SpreadsheetML relationship XML is malformed",
                ));
            }
        }
    }
    Ok(dependencies)
}

fn inspect_connection_definitions(data: &[u8]) -> Result<Vec<ExternalDependency>, WorkerFailure> {
    let mut reader = NsReader::from_reader(data);
    let mut parser = ConnectionDefinitionParser::default();

    loop {
        match reader.read_resolved_event() {
            Ok((namespace, Event::Start(event))) => {
                parser.start_element(namespace, &event, false)?
            }
            Ok((namespace, Event::Empty(event))) => {
                parser.start_element(namespace, &event, true)?
            }
            Ok((namespace, Event::End(event))) => parser.end_element(namespace, &event)?,
            Ok((_, Event::Text(event))) if is_xml_whitespace(event.as_ref()) => {}
            Ok((_, Event::CData(event))) if is_xml_whitespace(event.as_ref()) => {}
            Ok((_, Event::Comment(_))) => {}
            Ok((_, Event::Decl(_))) if !parser.root_seen => {}
            Ok((_, Event::Eof)) => break,
            Ok(_) => return Err(unsupported_connection_construct()),
            Err(_) => {
                return Err(failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    "SpreadsheetML connection XML is malformed",
                ));
            }
        }
    }

    parser.finish()
}

fn push_bounded_dependency(
    dependencies: &mut Vec<ExternalDependency>,
    observed_bytes: &mut usize,
    dependency: ExternalDependency,
) -> Result<(), WorkerFailure> {
    let simple_size = dependency
        .dependency_kind
        .len()
        .saturating_add(dependency.normalized_reference.len())
        .saturating_add(dependency.source_locator.len());
    if simple_size > MAX_STRUCTURED_RESULT_BYTES {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "SpreadsheetML external evidence exceeds its byte bound",
        ));
    }
    let serialized_size = serde_json::to_vec(&dependency)
        .map_err(|_| {
            failure(
                WorkerFailureCode::InvalidWorkerResult,
                "SpreadsheetML external evidence could not be measured",
            )
        })?
        .len();
    let next = observed_bytes
        .checked_add(serialized_size)
        .and_then(|sum| sum.checked_add(1));
    if next.is_none_or(|bytes| bytes > MAX_STRUCTURED_RESULT_BYTES) {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "SpreadsheetML external evidence exceeds its byte bound",
        ));
    }
    *observed_bytes = next.expect("checked above");
    dependencies.push(dependency);
    Ok(())
}

#[derive(Serialize)]
struct BorrowedCommentEvidence<'a> {
    author_label: Option<&'a str>,
    timestamp: Option<&'a str>,
    resolved_state: &'a str,
    source_locator: &'a str,
    content: &'a str,
}

fn enforce_comment_evidence_budget(
    observed_bytes: &mut usize,
    content: &str,
    author: Option<&str>,
    source_locator: &str,
) -> Result<(), WorkerFailure> {
    let borrowed = BorrowedCommentEvidence {
        author_label: author,
        timestamp: None,
        resolved_state: "unknown",
        source_locator,
        content,
    };
    let serialized_size = serialized_json_len_bounded(&borrowed)?;
    let next = observed_bytes
        .checked_add(serialized_size)
        .and_then(|sum| sum.checked_add(1));
    if next.is_none_or(|bytes| bytes > MAX_STRUCTURED_RESULT_BYTES) {
        return Err(resource_limit(
            "SpreadsheetML comment evidence exceeds its byte bound",
        ));
    }
    *observed_bytes = next.expect("checked above");
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionXmlElement {
    Connections,
    Connection,
    DbPr,
}

#[derive(Default)]
struct ConnectionDefinitionParser {
    elements: Vec<ConnectionXmlElement>,
    root_seen: bool,
    root_closed: bool,
    expected_connection_count: Option<usize>,
    connection_count: usize,
    current_connection: Option<ConnectionDefinition>,
    dependencies: Vec<ExternalDependency>,
    dependency_bytes: usize,
}

struct ConnectionDefinition {
    index: usize,
    attributes: BTreeMap<String, String>,
    database_properties: Option<BTreeMap<String, String>>,
}

impl ConnectionDefinitionParser {
    fn start_element(
        &mut self,
        namespace: ResolveResult<'_>,
        event: &BytesStart<'_>,
        empty: bool,
    ) -> Result<(), WorkerFailure> {
        if self.root_closed {
            return Err(unsupported_connection_construct());
        }
        let element = connection_xml_element(namespace, event.local_name().as_ref())?;
        match (self.elements.last().copied(), element) {
            (None, ConnectionXmlElement::Connections) if !self.root_seen => {
                let attributes = checked_xml_attributes(event, &["count"])?;
                validate_attribute_values(&attributes)?;
                self.expected_connection_count = attributes
                    .get("count")
                    .map(|value| {
                        value.trim().parse::<usize>().map_err(|_| {
                            failure(
                                WorkerFailureCode::SemanticExtractionFailed,
                                "SpreadsheetML connection count is malformed",
                            )
                        })
                    })
                    .transpose()?;
                self.root_seen = true;
                if empty {
                    self.close_root()?;
                } else {
                    self.elements.push(ConnectionXmlElement::Connections);
                }
            }
            (Some(ConnectionXmlElement::Connections), ConnectionXmlElement::Connection) => {
                let attributes = checked_xml_attributes(
                    event,
                    &["id", "name", "type", "refreshedVersion", "background"],
                )?;
                validate_attribute_values(&attributes)?;
                validate_qualified_connection_header(&attributes)?;
                self.connection_count = self
                    .connection_count
                    .checked_add(1)
                    .ok_or_else(unsupported_connection_construct)?;
                self.current_connection = Some(ConnectionDefinition {
                    index: self.connection_count,
                    attributes,
                    database_properties: None,
                });
                if empty {
                    self.close_connection()?;
                } else {
                    self.elements.push(ConnectionXmlElement::Connection);
                }
            }
            (Some(ConnectionXmlElement::Connection), ConnectionXmlElement::DbPr) => {
                let attributes =
                    checked_xml_attributes(event, &["connection", "command", "commandType"])?;
                validate_attribute_values(&attributes)?;
                let connection = required_connection_attribute(&attributes, "connection")?;
                let current = self
                    .current_connection
                    .as_mut()
                    .ok_or_else(unsupported_connection_construct)?;
                let connection_type = required_connection_attribute(&current.attributes, "type")?;
                if connection_type != "1" {
                    return Err(unsupported_connection_construct());
                }
                parse_qualified_odbc_connection_string(connection)?;
                validate_qualified_dbpr(&attributes)?;
                if current.database_properties.replace(attributes).is_some() {
                    return Err(unsupported_connection_construct());
                }
                if !empty {
                    self.elements.push(ConnectionXmlElement::DbPr);
                }
            }
            _ => return Err(unsupported_connection_construct()),
        }
        Ok(())
    }

    fn end_element(
        &mut self,
        namespace: ResolveResult<'_>,
        event: &BytesEnd<'_>,
    ) -> Result<(), WorkerFailure> {
        let element = connection_xml_element(namespace, event.local_name().as_ref())?;
        match (self.elements.last().copied(), element) {
            (Some(ConnectionXmlElement::DbPr), ConnectionXmlElement::DbPr) => {
                self.elements.pop();
            }
            (Some(ConnectionXmlElement::Connection), ConnectionXmlElement::Connection) => {
                self.elements.pop();
                self.close_connection()?;
            }
            (Some(ConnectionXmlElement::Connections), ConnectionXmlElement::Connections) => {
                self.elements.pop();
                self.close_root()?;
            }
            _ => return Err(unsupported_connection_construct()),
        }
        Ok(())
    }

    fn close_connection(&mut self) -> Result<(), WorkerFailure> {
        let current = self
            .current_connection
            .take()
            .ok_or_else(unsupported_connection_construct)?;
        let database_properties = current
            .database_properties
            .ok_or_else(unsupported_connection_construct)?;
        validate_qualified_connection_header(&current.attributes)?;
        validate_qualified_dbpr(&database_properties)?;
        let id = required_connection_attribute(&current.attributes, "id")?
            .parse::<u64>()
            .map_err(|_| unsupported_connection_construct())?;
        let name = required_connection_attribute(&current.attributes, "name")?;
        let refreshed_version = current
            .attributes
            .get("refreshedVersion")
            .map(|value| {
                value
                    .parse::<u32>()
                    .map_err(|_| unsupported_connection_construct())
            })
            .transpose()?;
        let background = current
            .attributes
            .get("background")
            .map(|value| value == "1");
        let odbc = parse_qualified_odbc_connection_string(required_connection_attribute(
            &database_properties,
            "connection",
        )?)?;
        let command = normalize_qualified_select_command(required_connection_attribute(
            &database_properties,
            "command",
        )?)?;
        let connection = format!(
            "DRIVER={};SERVER={};PORT={};DATABASE={}",
            odbc.driver, odbc.server, odbc.port, odbc.database
        );
        let normalized_reference = serde_json::to_string(&json!({
            "id": id,
            "name": name,
            "type": 1,
            "refreshed_version": refreshed_version,
            "background": background,
            "connection": connection,
            "uid_sha256": hex_digest(odbc.uid.as_bytes()),
            "command": command,
            "command_type": 2,
        }))
        .map_err(|_| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                "SpreadsheetML connection definition could not be normalized",
            )
        })?;
        push_bounded_dependency(
            &mut self.dependencies,
            &mut self.dependency_bytes,
            ExternalDependency {
                dependency_kind: "odbc".into(),
                normalized_reference,
                source_locator: format!("xl/connections.xml/connection[{}]", current.index),
                version_significant: true,
            },
        )?;
        Ok(())
    }

    fn close_root(&mut self) -> Result<(), WorkerFailure> {
        if self
            .expected_connection_count
            .is_some_and(|expected| expected != self.connection_count)
        {
            return Err(unsupported_connection_construct());
        }
        self.root_closed = true;
        Ok(())
    }

    fn finish(self) -> Result<Vec<ExternalDependency>, WorkerFailure> {
        if !self.root_seen
            || !self.root_closed
            || !self.elements.is_empty()
            || self.current_connection.is_some()
        {
            return Err(unsupported_connection_construct());
        }
        Ok(self.dependencies)
    }
}

fn connection_xml_element(
    namespace: ResolveResult<'_>,
    local_name: &str,
) -> Result<ConnectionXmlElement, WorkerFailure> {
    if !matches!(namespace, ResolveResult::Bound(value) if value.as_ref() == SPREADSHEETML_NAMESPACE)
    {
        return Err(unsupported_connection_construct());
    }
    match local_name {
        "connections" => Ok(ConnectionXmlElement::Connections),
        "connection" => Ok(ConnectionXmlElement::Connection),
        "dbPr" => Ok(ConnectionXmlElement::DbPr),
        _ => Err(unsupported_connection_construct()),
    }
}

fn checked_xml_attributes(
    event: &BytesStart<'_>,
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, WorkerFailure> {
    let mut attributes = BTreeMap::new();
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|_| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                "SpreadsheetML XML attribute is malformed",
            )
        })?;
        let key = attribute.key.as_ref();
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        if !allowed.contains(&key) {
            return Err(unsupported_connection_construct());
        }
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|_| {
                failure(
                    WorkerFailureCode::SemanticExtractionFailed,
                    "SpreadsheetML XML attribute is malformed",
                )
            })?
            .into_owned();
        if attributes.insert(key.to_owned(), value).is_some() {
            return Err(unsupported_connection_construct());
        }
    }
    Ok(attributes)
}

fn required_connection_attribute<'a>(
    attributes: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, WorkerFailure> {
    attributes
        .get(name)
        .map(String::as_str)
        .ok_or_else(unsupported_connection_construct)
}

fn validate_attribute_values(attributes: &BTreeMap<String, String>) -> Result<(), WorkerFailure> {
    for value in attributes.values() {
        validate_credential_free_reference(value)?;
    }
    Ok(())
}

fn is_xml_whitespace(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_whitespace())
}

fn unsupported_connection_construct() -> WorkerFailure {
    failure(
        WorkerFailureCode::UnsupportedSemanticConstruct,
        "SpreadsheetML connection definition is outside the qualified inspection subset",
    )
}

fn validate_credential_free_reference(reference: &str) -> Result<(), WorkerFailure> {
    if uri_has_userinfo(reference) || contains_credential_parameter(reference) {
        return Err(failure(
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "SpreadsheetML external reference contains embedded credentials",
        ));
    }
    Ok(())
}

fn validate_external_target(target: &str) -> Result<(), WorkerFailure> {
    validate_credential_free_reference(target)?;
    // The qualified external-reference subset has no userinfo, query, or fragment.
    // Those fields may carry arbitrary credential aliases and must not enter evidence.
    if target.contains(['@', '?', '#']) {
        return Err(unsupported_connection_construct());
    }
    Ok(())
}

fn validate_qualified_connection_header(
    attributes: &BTreeMap<String, String>,
) -> Result<(), WorkerFailure> {
    let id = required_connection_attribute(attributes, "id")?
        .parse::<u64>()
        .map_err(|_| unsupported_connection_construct())?;
    if id == 0
        || !is_safe_identifier(required_connection_attribute(attributes, "name")?)
        || required_connection_attribute(attributes, "type")? != "1"
    {
        return Err(unsupported_connection_construct());
    }
    if attributes
        .get("refreshedVersion")
        .is_some_and(|value| value.parse::<u32>().is_err())
        || attributes
            .get("background")
            .is_some_and(|value| value != "0" && value != "1")
    {
        return Err(unsupported_connection_construct());
    }
    Ok(())
}

fn validate_qualified_dbpr(attributes: &BTreeMap<String, String>) -> Result<(), WorkerFailure> {
    if required_connection_attribute(attributes, "commandType")? != "2" {
        return Err(unsupported_connection_construct());
    }
    parse_qualified_odbc_connection_string(required_connection_attribute(
        attributes,
        "connection",
    )?)?;
    normalize_qualified_select_command(required_connection_attribute(attributes, "command")?)?;
    Ok(())
}

struct QualifiedOdbcConnection {
    driver: String,
    server: String,
    port: u16,
    database: String,
    uid: String,
}

fn parse_qualified_odbc_connection_string(
    value: &str,
) -> Result<QualifiedOdbcConnection, WorkerFailure> {
    if value.len() > MAX_QUALIFIED_ODBC_CONNECTION_BYTES {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "SpreadsheetML ODBC connection definition exceeds its byte bound",
        ));
    }
    validate_credential_free_reference(value)?;
    let mut fields = BTreeMap::new();
    for part in split_odbc_connection_string(value)? {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((key, value)) = part.split_once('=') else {
            return Err(unsupported_connection_construct());
        };
        let key = key.trim().to_ascii_uppercase();
        let value = value.trim();
        let maximum_value_bytes = match key.as_str() {
            "DRIVER" => 130,
            "SERVER" => 253,
            "PORT" => 5,
            "DATABASE" | "UID" => 128,
            _ => return Err(unsupported_connection_construct()),
        };
        if value.is_empty()
            || value.len() > maximum_value_bytes
            || fields.insert(key, value.to_owned()).is_some()
        {
            return Err(unsupported_connection_construct());
        }
    }
    if fields.len() != 5 {
        return Err(unsupported_connection_construct());
    }
    let driver = fields
        .remove("DRIVER")
        .ok_or_else(unsupported_connection_construct)?;
    let driver_label = driver
        .strip_prefix('{')
        .and_then(|inner| inner.strip_suffix('}'))
        .ok_or_else(unsupported_connection_construct)?;
    if driver_label.is_empty()
        || driver_label.len() > 128
        || !driver_label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'_' | b'-' | b'.'))
    {
        return Err(unsupported_connection_construct());
    }
    let server = fields
        .remove("SERVER")
        .ok_or_else(unsupported_connection_construct)?;
    if server.is_empty()
        || server.len() > 253
        || !server.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(unsupported_connection_construct());
    }
    let port = fields
        .remove("PORT")
        .ok_or_else(unsupported_connection_construct)?
        .parse::<u16>()
        .map_err(|_| unsupported_connection_construct())?;
    if port == 0 {
        return Err(unsupported_connection_construct());
    }
    let database = fields
        .remove("DATABASE")
        .ok_or_else(unsupported_connection_construct)?;
    let uid = fields
        .remove("UID")
        .ok_or_else(unsupported_connection_construct)?;
    if !is_safe_identifier(&database) || !is_safe_identifier(&uid) {
        return Err(unsupported_connection_construct());
    }
    Ok(QualifiedOdbcConnection {
        driver,
        server,
        port,
        database,
        uid,
    })
}

fn is_safe_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    value.len() <= 128
        && (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn is_sql_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    value.len() <= 128
        && (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn normalize_qualified_select_command(command: &str) -> Result<String, WorkerFailure> {
    let mut words = command.split_ascii_whitespace();
    let Some(select) = words.next() else {
        return Err(unsupported_connection_construct());
    };
    let columns = words.next().ok_or_else(unsupported_connection_construct)?;
    let Some(from) = words.next() else {
        return Err(unsupported_connection_construct());
    };
    let table = words.next().ok_or_else(unsupported_connection_construct)?;
    if !select.eq_ignore_ascii_case("SELECT")
        || !from.eq_ignore_ascii_case("FROM")
        || words.next().is_some()
        || !is_sql_identifier(table)
        || columns.split(',').count() > 64
        || !columns.split(',').all(is_sql_identifier)
    {
        return Err(unsupported_connection_construct());
    }
    Ok(format!("SELECT {columns} FROM {table}"))
}

fn split_odbc_connection_string(value: &str) -> Result<Vec<&str>, WorkerFailure> {
    let bytes = value.as_bytes();
    let mut parts = Vec::new();
    let mut part_start = 0;
    let mut in_braced_value = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' if !in_braced_value => in_braced_value = true,
            b'}' if in_braced_value && bytes.get(index + 1) == Some(&b'}') => index += 1,
            b'}' if in_braced_value => in_braced_value = false,
            b'}' => return Err(unsupported_connection_construct()),
            b';' if !in_braced_value => {
                if parts.len() >= 5 {
                    return Err(unsupported_connection_construct());
                }
                parts.push(&value[part_start..index]);
                part_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    if in_braced_value {
        return Err(unsupported_connection_construct());
    }
    if parts.len() >= 5 {
        return Err(unsupported_connection_construct());
    }
    parts.push(&value[part_start..]);
    Ok(parts)
}

fn uri_has_userinfo(value: &str) -> bool {
    let mut search_from = 0;
    while let Some(relative_offset) = value[search_from..].find("://") {
        let authority_start = search_from + relative_offset + 3;
        let authority_end = value[authority_start..]
            .find(['/', '?', '#'])
            .map_or(value.len(), |offset| authority_start + offset);
        if value[authority_start..authority_end].contains('@') {
            return true;
        }
        search_from = authority_start;
    }
    false
}

fn contains_credential_parameter(value: &str) -> bool {
    value
        .split([';', '&', '?'])
        .filter_map(|segment| segment.split_once('=').map(|(key, _)| key))
        .any(|key| {
            let normalized: String = key
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .map(|character| character.to_ascii_lowercase())
                .collect();
            normalized == "pwd"
                || normalized.contains("password")
                || normalized.contains("passwd")
                || normalized.contains("secret")
                || normalized.contains("token")
                || normalized.contains("apikey")
                || normalized == "authorization"
        })
}

fn attribute_value(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, WorkerFailure> {
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|_| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                "SpreadsheetML XML attribute is malformed",
            )
        })?;
        if attribute.key.local_name().as_ref() == name {
            let value = attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|_| {
                    failure(
                        WorkerFailureCode::SemanticExtractionFailed,
                        "SpreadsheetML XML attribute is malformed",
                    )
                })?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

fn read_zip_part_limited(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, WorkerFailure> {
    let entry = archive.by_name(name).map_err(|_| {
        failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "required SpreadsheetML package part is missing",
        )
    })?;
    let declared_size = entry.size();
    if declared_size > MAX_SPREADSHEET_PART_BYTES {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "SpreadsheetML package part exceeds the byte limit",
        ));
    }
    let capacity = usize::try_from(declared_size).map_err(|_| {
        failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "SpreadsheetML package part exceeds the byte limit",
        )
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    entry
        .take(MAX_SPREADSHEET_PART_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            failure(
                WorkerFailureCode::SemanticExtractionFailed,
                "SpreadsheetML package part could not be read",
            )
        })?;
    if bytes.len() as u64 > MAX_SPREADSHEET_PART_BYTES {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "SpreadsheetML package part exceeds the byte limit",
        ));
    }
    if bytes.len() as u64 != declared_size {
        return Err(failure(
            WorkerFailureCode::SemanticExtractionFailed,
            "SpreadsheetML package part has inconsistent size metadata",
        ));
    }
    Ok(bytes)
}

fn parser_disagreement(message: &'static str) -> WorkerFailure {
    failure(WorkerFailureCode::ParserDisagreement, message)
}

#[cfg(test)]
mod hyperlink_oracle_budget_tests {
    use super::*;

    #[test]
    fn hyperlink_budget_accepts_exact_bound_and_rejects_one_over_before_oracle_vectors() {
        let mut target = "a".repeat(MAX_STRUCTURED_RESULT_BYTES - 128);
        assert!(enforce_hyperlink_oracle_budget([target.as_str()]).is_ok());

        target.push('a');
        let failure = enforce_hyperlink_oracle_budget([target.as_str()])
            .expect_err("one byte over the derived budget must fail");
        assert_eq!(
            failure.code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
    }
}

#[cfg(test)]
mod comment_evidence_budget_tests {
    use super::*;

    #[test]
    fn comment_evidence_budget_accepts_exact_bound_and_rejects_one_over() {
        let source_locator = "sheet[0]/R1C1";
        let empty = BorrowedCommentEvidence {
            author_label: None,
            timestamp: None,
            resolved_state: "unknown",
            source_locator,
            content: "",
        };
        let overhead = serialized_json_len_bounded(&empty).expect("bounded empty comment") + 1;
        let mut content = "a".repeat(MAX_STRUCTURED_RESULT_BYTES - overhead);
        let mut observed = 0;
        assert!(
            enforce_comment_evidence_budget(&mut observed, &content, None, source_locator).is_ok()
        );
        assert_eq!(observed, MAX_STRUCTURED_RESULT_BYTES);

        content.push('a');
        observed = 0;
        let failure =
            enforce_comment_evidence_budget(&mut observed, &content, None, source_locator)
                .expect_err("one byte over the evidence budget must fail");
        assert_eq!(
            failure.code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
    }
}
