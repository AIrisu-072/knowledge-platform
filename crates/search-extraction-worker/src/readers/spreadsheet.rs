//! XLSX/XLSM sheet cells in workbook order with absolute `(row, col)` locators.
//! Formula cells never become Units; their physical position is a known
//! omission. VBA is never executed and is outside the body scope.

use std::collections::{BTreeMap, BTreeSet};

use search_core::knowledge_unit::{NativeLocator, UnitKind};
use search_extraction_core::{BudgetMeter, CoverageReason};

use super::ooxml::{check_package, declared_content_type, parse_xml, part, safe_name, zip_parts};
use super::{Body, ReadResult, corrupt, resource_limit, structure};

const WORKSHEET_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const SHARED_STRINGS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
const MAX_SHARED_STRINGS: usize = 100_000;

pub(super) fn read(
    raw: &[u8],
    macro_enabled: bool,
    meter: &mut BudgetMeter,
    depth: u64,
) -> ReadResult<Body> {
    let parts = zip_parts(raw, meter, depth)?;
    let mut package_omissions = Vec::new();
    for name in parts.keys() {
        if matches!(
            name.as_str(),
            "[Content_Types].xml"
                | "_rels/.rels"
                | "xl/workbook.xml"
                | "xl/_rels/workbook.xml.rels"
                | "xl/sharedStrings.xml"
        ) || (name.starts_with("xl/worksheets/")
            && name.ends_with(".xml")
            && !name.contains("/_rels/"))
        {
            continue;
        }
        if (macro_enabled && name == "xl/vbaProject.bin")
            || name.starts_with("xl/charts/")
            || name.starts_with("xl/drawings/")
            || name.starts_with("xl/comments")
        {
            package_omissions.push(name.clone());
        } else {
            return Err(structure());
        }
    }
    let types = check_package(
        &parts,
        "xl/workbook.xml",
        if macro_enabled {
            "application/vnd.ms-excel.sheet.macroEnabled.main+xml"
        } else {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
        },
        meter,
    )?;
    let workbook = parse_xml(part(&parts, "xl/workbook.xml")?, meter)?;
    if workbook.local() != "workbook" {
        return Err(corrupt());
    }
    if workbook
        .children
        .iter()
        .any(|node| node.local() != "sheets")
    {
        return Err(structure());
    }
    let relationships = parse_xml(part(&parts, "xl/_rels/workbook.xml.rels")?, meter)?;
    let targets: BTreeMap<_, _> = relationships
        .children("Relationship")
        .filter_map(|node| Some((node.attr("Id")?.to_owned(), node.attr("Target")?.to_owned())))
        .collect();
    let shared = match parts.get("xl/sharedStrings.xml") {
        Some(bytes) => {
            if declared_content_type(&types, "xl/sharedStrings.xml") != Some(SHARED_STRINGS_TYPE) {
                return Err(structure());
            }
            Some(shared_strings(bytes, meter)?)
        }
        None => None,
    };
    let mut body = Body::default();
    let mut referenced = BTreeSet::new();
    let sheets = workbook.child("sheets").ok_or_else(corrupt)?;
    for (sheet_index, sheet) in sheets.children("sheet").enumerate() {
        let sheet_ordinal = u32::try_from(sheet_index).map_err(|_| resource_limit())?;
        let target = targets
            .get(sheet.attr("r:id").ok_or_else(corrupt)?)
            .ok_or_else(corrupt)?;
        let path = format!("xl/{}", target.trim_start_matches('/'));
        if !safe_name(&path) || !referenced.insert(path.clone()) {
            return Err(structure());
        }
        if declared_content_type(&types, &path) != Some(WORKSHEET_TYPE) {
            return Err(structure());
        }
        let worksheet = parse_xml(part(&parts, &path)?, meter)?;
        if worksheet.local() != "worksheet" || worksheet.child("sheetData").is_none() {
            return Err(corrupt());
        }
        if !matches!(sheet.attr("state"), None | Some("visible")) {
            body.omit(Some(path), Vec::new(), CoverageReason::UnsupportedStructure);
            continue;
        }
        if worksheet
            .children
            .iter()
            .any(|node| node.local() != "sheetData")
        {
            return Err(structure());
        }
        let data = worksheet.child("sheetData").ok_or_else(corrupt)?;
        if data.children.iter().any(|node| node.local() != "row") {
            return Err(structure());
        }
        for (row_index, row) in data.children.iter().enumerate() {
            if row.children.iter().any(|node| node.local() != "c") {
                return Err(structure());
            }
            for (cell_index, cell) in row.children.iter().enumerate() {
                body.visit(1)?;
                if cell
                    .children
                    .iter()
                    .any(|node| !matches!(node.local(), "f" | "v" | "is"))
                {
                    return Err(structure());
                }
                if !matches!(cell.attr("t"), None | Some("n" | "inlineStr" | "s")) {
                    return Err(structure());
                }
                let (row_number, col_number) = cell_reference(cell.attr("r").ok_or_else(corrupt)?)?;
                let value = cell.child("v");
                if cell.child("f").is_some() {
                    // Cached formula values have unknown freshness; never claim them.
                    let reason = if value.is_none_or(|value| value.text.is_empty()) {
                        CoverageReason::MissingFormulaCache
                    } else {
                        CoverageReason::UnsupportedStructure
                    };
                    body.omit(
                        Some(path.clone()),
                        vec![
                            u32::try_from(row_index).map_err(|_| resource_limit())?,
                            u32::try_from(cell_index).map_err(|_| resource_limit())?,
                        ],
                        reason,
                    );
                    continue;
                }
                let text = match cell.attr("t") {
                    Some("inlineStr") => cell.desc_text("t"),
                    Some("s") => {
                        let index: usize = value
                            .ok_or_else(corrupt)?
                            .text
                            .parse()
                            .map_err(|_| corrupt())?;
                        shared
                            .as_ref()
                            .ok_or_else(corrupt)?
                            .get(index)
                            .ok_or_else(corrupt)?
                            .clone()
                    }
                    _ => value.map(|value| value.text.clone()).unwrap_or_default(),
                };
                body.unit(
                    meter,
                    UnitKind::SpreadsheetCell,
                    &text,
                    NativeLocator::Spreadsheet {
                        sheet_ordinal,
                        row: row_number - 1,
                        col: col_number - 1,
                    },
                )?;
            }
        }
    }
    if parts.keys().any(|name| {
        name.starts_with("xl/worksheets/")
            && !name.contains("/_rels/")
            && !referenced.contains(name)
    }) {
        return Err(structure());
    }
    for name in package_omissions {
        body.omit(Some(name), Vec::new(), CoverageReason::UnsupportedStructure);
    }
    Ok(body)
}

fn shared_strings(bytes: &[u8], meter: &mut BudgetMeter) -> ReadResult<Vec<String>> {
    let table = parse_xml(bytes, meter)?;
    if table.local() != "sst" {
        return Err(corrupt());
    }
    let mut strings = Vec::new();
    for item in table.children("si") {
        if strings.len() >= MAX_SHARED_STRINGS {
            return Err(resource_limit());
        }
        let value = if item.children.len() == 1 && item.children[0].local() == "t" {
            item.children[0].text.clone()
        } else if item.children.iter().all(|node| node.local() == "r") {
            let mut out = String::new();
            for run in &item.children {
                if run
                    .children
                    .iter()
                    .any(|node| !matches!(node.local(), "rPr" | "t"))
                {
                    return Err(structure());
                }
                out.push_str(&run.child("t").ok_or_else(corrupt)?.text);
            }
            out
        } else {
            return Err(structure());
        };
        strings.push(value);
    }
    Ok(strings)
}

/// `A1` style reference → one-based `(row, col)`.
fn cell_reference(reference: &str) -> ReadResult<(u32, u32)> {
    let mut chars = reference.chars().peekable();
    let mut col = 0u32;
    while let Some(letter) = chars.next_if(char::is_ascii_uppercase) {
        col = col
            .checked_mul(26)
            .and_then(|value| value.checked_add(letter as u32 - 64))
            .ok_or_else(resource_limit)?;
    }
    let row: u32 = chars.collect::<String>().parse().map_err(|_| corrupt())?;
    if row == 0 || col == 0 {
        return Err(corrupt());
    }
    Ok((row, col))
}
