use super::VbaAdapter;
use crate::{
    canonical_json_bytes, AdapterOutput, CapabilityEvidence, CommentEvidence, EditorialEvidence,
    ExternalDependency, FormatId, InspectionAdapter, InspectionProfile, PocError,
};
use calamine::{open_workbook_auto_from_rs, Reader};
use quick_xml::events::Event;
use rxls::{Cell, Workbook};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{Cursor, Read};
use zip::ZipArchive;

const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_TOTAL_UNCOMPRESSED: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpreadsheetAdapter {
    format: FormatId,
}

impl SpreadsheetAdapter {
    pub const XLSX: Self = Self { format: FormatId::Xlsx };
    pub const XLSM: Self = Self { format: FormatId::Xlsm };
}

impl InspectionAdapter for SpreadsheetAdapter {
    fn format(&self) -> FormatId {
        self.format
    }

    fn inspect(
        &self,
        input: &[u8],
        _profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError> {
        let observed = spreadsheet_format(input).ok_or_else(|| {
            PocError::SemanticExtractionFailed("not a recognized SpreadsheetML package".into())
        })?;
        if observed != self.format {
            return Err(PocError::FormatMismatch {
                expected: self.format,
                observed: Some(observed),
            });
        }

        inspect_package_coverage(input)?;
        let workbook = Workbook::open(input)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("rxls: {error}")))?;
        compare_calamine_oracle(input, &workbook)?;

        let mut formula_present = false;
        let mut hidden_present = false;
        let mut external_dependencies = Vec::new();
        let mut editorial = EditorialEvidence::default();
        let mut sheets = Vec::with_capacity(workbook.sheets.len());

        for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
            let visible = format!("{:?}", sheet.visible()).to_lowercase();
            if visible != "visible" {
                hidden_present = true;
            }

            let mut cells = Vec::new();
            for (row, columns) in sheet.rows() {
                for (col, cell) in columns {
                    if matches!(cell, Cell::Formula { .. }) {
                        formula_present = true;
                    }
                    cells.push(json!({
                        "row": row,
                        "col": col,
                        "value": cell_projection(cell),
                    }));
                }
            }

            let mut merges: Vec<_> = sheet.merged_ranges().to_vec();
            merges.sort();

            let mut hyperlinks: Vec<_> = sheet.hyperlinks().to_vec();
            hyperlinks.sort();
            for (row, col, target) in &hyperlinks {
                external_dependencies.push(ExternalDependency {
                    kind: "hyperlink".into(),
                    definition: format!("{}!R{}C{}={target}", sheet.name, row + 1, col + 1),
                });
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
                    json!({
                        "format": format!("{:?}", image.format).to_lowercase(),
                        "from": image.from,
                        "to": image.to,
                        "sha256": hex::encode(Sha256::digest(&image.data)),
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

            if !sheet.comments().is_empty() {
                editorial.comments_present = true;
                for (comment_index, comment) in sheet.comments().iter().enumerate() {
                    editorial.comments.push(CommentEvidence {
                        author_label: None,
                        timestamp: None,
                        resolved_state: "unknown".into(),
                        source_locator: format!("sheet[{sheet_index}]/comment[{comment_index}]"),
                        content: format!("{comment:?}"),
                    });
                }
            }

            sheets.push(json!({
                "index": sheet_index,
                "name": sheet.name,
                "visibility": visible,
                "cells": cells,
                "merged_ranges": merges,
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
            .map(|name| (name.sheet.clone(), name.name.clone(), name.refers_to.clone()))
            .collect();
        local_defined_names.sort();

        external_dependencies.sort_by(|left, right| {
            (left.kind.as_str(), left.definition.as_str())
                .cmp(&(right.kind.as_str(), right.definition.as_str()))
        });

        let mut vba_projection = Value::Null;
        let mut vba_present = false;
        if self.format == FormatId::Xlsm {
            let inspection = VbaAdapter::inspect_xlsm(input)?;
            vba_present = inspection.module_count > 0;
            vba_projection = inspection.projection;
        }

        let projection = json!({
            "format": match self.format { FormatId::Xlsm => "xlsm", _ => "xlsx" },
            "date1904": workbook.date1904,
            "defined_names": defined_names,
            "local_defined_names": local_defined_names,
            "sheets": sheets,
            "vba": vba_projection,
        });

        Ok(AdapterOutput {
            semantic_projection: canonical_json_bytes(&projection)
                .map_err(|error| PocError::InvalidWorkerResult(format!("spreadsheet projection: {error}")))?,
            capabilities: vec![
                CapabilityEvidence { capability: "formula_logic".into(), present: formula_present },
                CapabilityEvidence { capability: "hidden_content".into(), present: hidden_present },
                CapabilityEvidence { capability: "vba_logic".into(), present: vba_present },
            ],
            editorial,
            external_dependencies,
            signatures: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

pub(crate) fn spreadsheet_format(input: &[u8]) -> Option<FormatId> {
    let mut archive = ZipArchive::new(Cursor::new(input)).ok()?;
    let mut content_types = String::new();
    archive
        .by_name("[Content_Types].xml")
        .ok()?
        .read_to_string(&mut content_types)
        .ok()?;
    if content_types.contains("application/vnd.ms-excel.sheet.macroEnabled.main+xml")
        || content_types.contains("application/vnd.ms-office.vbaProject")
    {
        Some(FormatId::Xlsm)
    } else if content_types.contains(
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
    ) {
        Some(FormatId::Xlsx)
    } else {
        None
    }
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

fn compare_calamine_oracle(input: &[u8], workbook: &Workbook) -> Result<(), PocError> {
    let mut oracle = open_workbook_auto_from_rs(Cursor::new(input.to_vec()))
        .map_err(|error| PocError::ParserDisagreement(format!("Calamine open: {error}")))?;

    let rxls_names: Vec<String> = workbook.sheets.iter().map(|sheet| sheet.name.clone()).collect();
    let calamine_names = oracle.sheet_names();
    if rxls_names != calamine_names {
        return Err(PocError::ParserDisagreement(format!(
            "sheet order/name mismatch: rxls={rxls_names:?}, calamine={calamine_names:?}"
        )));
    }

    let mut rxls_names_def = workbook.defined_names.clone();
    let mut calamine_names_def = oracle.defined_names().to_vec();
    rxls_names_def.sort();
    calamine_names_def.sort();
    if rxls_names_def != calamine_names_def {
        return Err(PocError::ParserDisagreement(format!(
            "defined-name mismatch: rxls={rxls_names_def:?}, calamine={calamine_names_def:?}"
        )));
    }

    for sheet in &workbook.sheets {
        let mut rxls_formulas: Vec<String> = sheet
            .cells()
            .filter_map(|(_, _, cell)| match cell {
                Cell::Formula { formula, .. } => Some(formula.clone()),
                _ => None,
            })
            .collect();
        rxls_formulas.sort();

        let formula_range = oracle
            .worksheet_formula(&sheet.name)
            .map_err(|error| PocError::ParserDisagreement(format!(
                "Calamine formula range {}: {error}",
                sheet.name
            )))?;
        let mut calamine_formulas: Vec<String> = formula_range
            .rows()
            .flat_map(|row| row.iter())
            .filter(|formula| !formula.is_empty())
            .cloned()
            .collect();
        calamine_formulas.sort();
        if rxls_formulas != calamine_formulas {
            return Err(PocError::ParserDisagreement(format!(
                "formula-source mismatch on {}: rxls={rxls_formulas:?}, calamine={calamine_formulas:?}",
                sheet.name
            )));
        }

        let value_range = oracle
            .worksheet_range(&sheet.name)
            .map_err(|error| PocError::ParserDisagreement(format!(
                "Calamine value range {}: {error}",
                sheet.name
            )))?;
        let rxls_cell_count = sheet.rows().map(|(_, columns)| columns.len()).sum::<usize>();
        let calamine_cell_count = value_range.used_cells().count();
        if rxls_cell_count != calamine_cell_count {
            return Err(PocError::ParserDisagreement(format!(
                "used-cell count mismatch on {}: rxls={rxls_cell_count}, calamine={calamine_cell_count}",
                sheet.name
            )));
        }
    }

    Ok(())
}

fn inspect_package_coverage(input: &[u8]) -> Result<(), PocError> {
    let mut archive = ZipArchive::new(Cursor::new(input))
        .map_err(|error| PocError::SemanticExtractionFailed(format!("spreadsheet ZIP: {error}")))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(PocError::InspectionResourceLimitExceeded);
    }

    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("ZIP entry: {error}")))?;
        let name = file.name().replace('\\', "/");
        if name.starts_with('/') || name.split('/').any(|part| part == "..") {
            return Err(PocError::UnsupportedSemanticConstruct(
                "spreadsheet package path traversal".into(),
            ));
        }
        if !names.insert(name) {
            return Err(PocError::UnsupportedSemanticConstruct(
                "duplicate spreadsheet package entry".into(),
            ));
        }
        total = total.saturating_add(file.size());
        if total > MAX_TOTAL_UNCOMPRESSED {
            return Err(PocError::InspectionResourceLimitExceeded);
        }
    }

    let content_types = read_zip_part(&mut archive, "[Content_Types].xml")?;
    validate_content_types(&content_types)?;

    let rel_names: Vec<String> = names
        .iter()
        .filter(|name| name.ends_with(".rels"))
        .cloned()
        .collect();
    for name in rel_names {
        let data = read_zip_part(&mut archive, &name)?;
        validate_relationships(&data)?;
    }
    Ok(())
}

fn read_zip_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, PocError> {
    let mut data = Vec::new();
    archive
        .by_name(name)
        .map_err(|_| PocError::SemanticExtractionFailed(format!("missing package part {name}")))?
        .read_to_end(&mut data)
        .map_err(|error| PocError::SemanticExtractionFailed(format!("read {name}: {error}")))?;
    Ok(data)
}

fn validate_content_types(data: &[u8]) -> Result<(), PocError> {
    let mut reader = quick_xml::Reader::from_reader(data);
    loop {
        match reader
            .read_event()
            .map_err(|error| PocError::SemanticExtractionFailed(format!("content-types XML: {error}")))?
        {
            Event::Start(event) | Event::Empty(event)
                if matches!(event.local_name().as_ref(), "Default" | "Override") =>
            {
                for attribute in event.attributes() {
                    let attribute = attribute.map_err(|error| {
                        PocError::SemanticExtractionFailed(format!("content-type attribute: {error}"))
                    })?;
                    if attribute.key.local_name().as_ref() == "ContentType" {
                        let value = attribute.value.as_ref();
                        if !known_content_type(&value) {
                            return Err(PocError::UnsupportedSemanticConstruct(format!(
                                "unknown spreadsheet content type {value}"
                            )));
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn known_content_type(value: &str) -> bool {
    matches!(
        value,
        "application/vnd.openxmlformats-package.relationships+xml"
            | "application/xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
            | "application/vnd.ms-excel.sheet.macroEnabled.main+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.externalLink+xml"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml"
            | "application/vnd.openxmlformats-officedocument.drawing+xml"
            | "application/vnd.openxmlformats-officedocument.drawingml.chart+xml"
            | "application/vnd.openxmlformats-officedocument.theme+xml"
            | "application/vnd.openxmlformats-package.core-properties+xml"
            | "application/vnd.openxmlformats-officedocument.extended-properties+xml"
            | "application/vnd.ms-office.vbaProject"
            | "application/vnd.ms-excel.controlproperties+xml"
            | "application/vnd.ms-excel.printerSettings"
            | "image/png"
            | "image/jpeg"
    )
}

fn validate_relationships(data: &[u8]) -> Result<(), PocError> {
    let mut reader = quick_xml::Reader::from_reader(data);
    loop {
        match reader
            .read_event()
            .map_err(|error| PocError::SemanticExtractionFailed(format!("relationships XML: {error}")))?
        {
            Event::Start(event) | Event::Empty(event)
                if event.local_name().as_ref() == "Relationship" =>
            {
                for attribute in event.attributes() {
                    let attribute = attribute.map_err(|error| {
                        PocError::SemanticExtractionFailed(format!("relationship attribute: {error}"))
                    })?;
                    if attribute.key.local_name().as_ref() == "Type" {
                        let value = attribute.value.as_ref();
                        if !known_relationship_type(&value) {
                            return Err(PocError::UnsupportedSemanticConstruct(format!(
                                "unknown spreadsheet relationship type {value}"
                            )));
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn known_relationship_type(value: &str) -> bool {
    const SUFFIXES: &[&str] = &[
        "officeDocument", "worksheet", "styles", "sharedStrings", "theme", "hyperlink",
        "drawing", "image", "table", "comments", "vbaProject", "calcChain", "externalLink",
        "connections", "printerSettings", "pivotCacheDefinition", "pivotCacheRecords",
        "pivotTable", "control", "ctrlProp", "legacyDrawing",
    ];
    if value == "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"
        || value == "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties"
    {
        return true;
    }
    let prefix = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| SUFFIXES.contains(&suffix))
}
