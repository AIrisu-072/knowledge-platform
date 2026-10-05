//! DOCX main body with physical `BodyBlock → Row → Cell → CellBlock` locators.
//! Headers, footers and footnotes are known package omissions.

use search_core::knowledge_unit::{DocxStep, NativeLocator, UnitKind};
use search_extraction_core::{BudgetMeter, CoverageReason};

use super::ooxml::{
    Node, check_package, declared_content_type, opc_relationships, opc_target, parse_xml, part,
    zip_parts,
};
use super::{Body, ReadResult, corrupt, resource_limit, structure};

const MAIN: &str = "word/document.xml";
const MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const HEADER_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const HEADER_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";

pub(super) fn read(raw: &[u8], meter: &mut BudgetMeter, depth: u64) -> ReadResult<Body> {
    let parts = zip_parts(raw, meter, depth)?;
    let mut body = Body::default();
    let mut omitted_parts = Vec::new();
    for name in parts.keys() {
        if matches!(
            name.as_str(),
            "[Content_Types].xml" | "_rels/.rels" | MAIN | "word/_rels/document.xml.rels"
        ) {
            continue;
        }
        if name.starts_with("word/header")
            || name.starts_with("word/footer")
            || name.starts_with("word/footnotes")
        {
            omitted_parts.push(name.clone());
        } else {
            return Err(structure());
        }
    }
    let types = check_package(&parts, MAIN, MAIN_TYPE, meter)?;
    let document = parse_xml(part(&parts, MAIN)?, meter)?;
    if document.local() != "document" || document.attr("xmlns:w").is_none() {
        return Err(corrupt());
    }
    let main_body = document.child("body").ok_or_else(corrupt)?;
    let section = main_body
        .children
        .last()
        .filter(|node| node.local() == "sectPr");
    if main_body
        .children
        .iter()
        .take(main_body.children.len().saturating_sub(1))
        .any(|node| node.local() == "sectPr")
    {
        return Err(structure());
    }
    if parts.contains_key("word/_rels/document.xml.rels") {
        let relationships = opc_relationships(&parts, "word/_rels/document.xml.rels", meter)?;
        let section = section.ok_or_else(structure)?;
        if section.children.is_empty()
            || section
                .children
                .iter()
                .any(|node| node.local() != "headerReference")
        {
            return Err(structure());
        }
        let mut used = std::collections::BTreeSet::new();
        for reference in &section.children {
            let id = reference.attr("r:id").ok_or_else(corrupt)?;
            if !used.insert(id) {
                return Err(structure());
            }
            let relationship = relationships
                .iter()
                .find(|candidate| candidate.id == id)
                .ok_or_else(corrupt)?;
            if relationship.kind != HEADER_RELATIONSHIP {
                return Err(structure());
            }
            let path = opc_target(MAIN, &relationship.target)?;
            if !path.starts_with("word/header") || !omitted_parts.contains(&path) {
                return Err(structure());
            }
            if declared_content_type(&types, &path) != Some(HEADER_TYPE) {
                return Err(structure());
            }
            if parse_xml(part(&parts, &path)?, meter)?.local() != "hdr" {
                return Err(corrupt());
            }
        }
        if used.len() != relationships.len() {
            return Err(structure());
        }
    } else if section.is_some() {
        return Err(structure());
    }
    walk_blocks(main_body, &[], false, &mut body, meter)?;
    for path in omitted_parts {
        body.omit(Some(path), Vec::new(), CoverageReason::UnsupportedStructure);
    }
    Ok(body)
}

fn index(value: usize) -> ReadResult<u32> {
    u32::try_from(value).map_err(|_| resource_limit())
}

fn walk_blocks(
    parent: &Node,
    steps: &[DocxStep],
    in_cell: bool,
    body: &mut Body,
    meter: &mut BudgetMeter,
) -> ReadResult<()> {
    let block_count = parent.children.len();
    for (block_index, block) in parent.children.iter().enumerate() {
        body.visit(1)?;
        let mut path = steps.to_vec();
        path.push(if steps.is_empty() {
            DocxStep::BodyBlock(index(block_index)?)
        } else {
            DocxStep::CellBlock(index(block_index)?)
        });
        match block.local() {
            "sectPr" if steps.is_empty() && block_index + 1 == block_count => {}
            "p" => {
                for child in &block.children {
                    match child.local() {
                        "pPr" => {}
                        "r" if child.children.iter().all(|run| run.local() == "t") => {}
                        _ => return Err(structure()),
                    }
                }
                let heading = block
                    .child("pPr")
                    .and_then(|properties| properties.child("pStyle"))
                    .and_then(|style| style.attr("w:val"))
                    .is_some_and(|style| style.starts_with("Heading"));
                let kind = if in_cell {
                    UnitKind::TableCell
                } else if heading {
                    UnitKind::Heading
                } else {
                    UnitKind::Paragraph
                };
                body.unit(
                    meter,
                    kind,
                    &block.desc_text("t"),
                    NativeLocator::Docx { steps: path },
                )?;
            }
            "tbl" => {
                if block.children.iter().any(|row| row.local() != "tr") {
                    return Err(structure());
                }
                for (row_index, row) in block.children("tr").enumerate() {
                    if row.children.iter().any(|cell| cell.local() != "tc") {
                        return Err(structure());
                    }
                    for (cell_index, cell) in row.children("tc").enumerate() {
                        let mut cell_path = path.clone();
                        cell_path.push(DocxStep::Row(index(row_index)?));
                        cell_path.push(DocxStep::Cell(index(cell_index)?));
                        walk_blocks(cell, &cell_path, true, body, meter)?;
                    }
                }
            }
            _ => return Err(structure()),
        }
    }
    Ok(())
}
