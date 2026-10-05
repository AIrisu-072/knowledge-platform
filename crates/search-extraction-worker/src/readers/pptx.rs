//! PPTX slides in presentation order with physical shape-tree locators.
//! Notes slides are known package omissions; any other unlocatable shape makes
//! the whole item unsupported.

use std::collections::{BTreeMap, BTreeSet};

use search_core::knowledge_unit::{NativeLocator, PptxTextSlot, UnitKind};
use search_extraction_core::{BudgetMeter, CoverageReason};

use super::ooxml::{
    Node, check_package, declared_content_type, opc_relationships, opc_target, parse_xml, part,
    safe_name, zip_parts,
};
use super::{Body, ReadResult, corrupt, resource_limit, structure};

const MAIN: &str = "ppt/presentation.xml";
const MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const NOTES_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
const NOTES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";

pub(super) fn read(raw: &[u8], meter: &mut BudgetMeter, depth: u64) -> ReadResult<Body> {
    let parts = zip_parts(raw, meter, depth)?;
    let mut omitted_parts = Vec::new();
    for name in parts.keys() {
        if matches!(
            name.as_str(),
            "[Content_Types].xml" | "_rels/.rels" | MAIN | "ppt/_rels/presentation.xml.rels"
        ) || (name.starts_with("ppt/slides/")
            && name.ends_with(".xml")
            && !name.contains("/_rels/"))
            || (name.starts_with("ppt/slides/_rels/") && name.ends_with(".xml.rels"))
        {
            continue;
        }
        if name.starts_with("ppt/notesSlides/") || name.starts_with("ppt/charts/") {
            omitted_parts.push(name.clone());
        } else {
            return Err(structure());
        }
    }
    let types = check_package(&parts, MAIN, MAIN_TYPE, meter)?;
    let presentation = parse_xml(part(&parts, MAIN)?, meter)?;
    let relationships = parse_xml(part(&parts, "ppt/_rels/presentation.xml.rels")?, meter)?;
    let targets: BTreeMap<_, _> = relationships
        .children("Relationship")
        .filter_map(|node| Some((node.attr("Id")?.to_owned(), node.attr("Target")?.to_owned())))
        .collect();
    let mut body = Body::default();
    let mut unlocated = false;
    let mut referenced_slides = BTreeSet::new();
    let mut referenced_rels = BTreeSet::new();
    let slides = presentation.child("sldIdLst").ok_or_else(corrupt)?;
    for (slide_index, slide) in slides.children("sldId").enumerate() {
        let slide_ordinal = u32::try_from(slide_index).map_err(|_| resource_limit())?;
        let target = targets
            .get(slide.attr("r:id").ok_or_else(corrupt)?)
            .ok_or_else(corrupt)?;
        let path = format!("ppt/{}", target.trim_start_matches('/'));
        if !safe_name(&path) || !referenced_slides.insert(path.clone()) {
            return Err(structure());
        }
        let (directory, filename) = path.rsplit_once('/').ok_or_else(corrupt)?;
        let rel_path = format!("{directory}/_rels/{filename}.rels");
        if parts.contains_key(&rel_path) {
            referenced_rels.insert(rel_path.clone());
            for relationship in opc_relationships(&parts, &rel_path, meter)? {
                if relationship.kind != NOTES_RELATIONSHIP {
                    return Err(structure());
                }
                let note_path = opc_target(&path, &relationship.target)?;
                if !note_path.starts_with("ppt/notesSlides/")
                    || declared_content_type(&types, &note_path) != Some(NOTES_TYPE)
                {
                    return Err(structure());
                }
                if parse_xml(part(&parts, &note_path)?, meter)?.local() != "notes" {
                    return Err(corrupt());
                }
                if !omitted_parts.contains(&note_path) {
                    return Err(structure());
                }
            }
        }
        let xml = parse_xml(part(&parts, &path)?, meter)?;
        let tree = xml
            .child("cSld")
            .and_then(|node| node.child("spTree"))
            .ok_or_else(corrupt)?;
        shapes(tree, slide_ordinal, &[], &mut body, &mut unlocated, meter)?;
    }
    if parts.keys().any(|name| {
        (name.starts_with("ppt/slides/")
            && !name.contains("/_rels/")
            && !referenced_slides.contains(name))
            || (name.starts_with("ppt/slides/_rels/") && !referenced_rels.contains(name))
    }) {
        return Err(structure());
    }
    if unlocated || (!omitted_parts.is_empty() && body.units.is_empty()) {
        return Err(structure());
    }
    for name in omitted_parts {
        body.omit(Some(name), Vec::new(), CoverageReason::UnsupportedStructure);
    }
    Ok(body)
}

fn paragraphs(
    text_body: &Node,
    locate: impl Fn(u32) -> NativeLocator,
    body: &mut Body,
    unlocated: &mut bool,
    meter: &mut BudgetMeter,
) -> ReadResult<()> {
    for (paragraph_index, paragraph) in text_body.children("p").enumerate() {
        if paragraph.children.iter().any(|node| node.local() != "r") {
            *unlocated = true;
            continue;
        }
        let paragraph = u32::try_from(paragraph_index).map_err(|_| resource_limit())?;
        body.unit(
            meter,
            UnitKind::SlideText,
            &paragraph_text(text_body, paragraph as usize),
            locate(paragraph),
        )?;
    }
    Ok(())
}

fn paragraph_text(text_body: &Node, index: usize) -> String {
    text_body
        .children("p")
        .nth(index)
        .map(|paragraph| paragraph.desc_text("t"))
        .unwrap_or_default()
}

fn shapes(
    tree: &Node,
    slide_ordinal: u32,
    prefix: &[u32],
    body: &mut Body,
    unlocated: &mut bool,
    meter: &mut BudgetMeter,
) -> ReadResult<()> {
    for (shape_index, shape) in tree.children.iter().enumerate() {
        body.visit(1)?;
        let mut shape_path = prefix.to_vec();
        shape_path.push(u32::try_from(shape_index).map_err(|_| resource_limit())?);
        match shape.local() {
            "grpSp" => shapes(shape, slide_ordinal, &shape_path, body, unlocated, meter)?,
            "sp" => {
                if let Some(text_body) = shape.child("txBody") {
                    let path = shape_path.clone();
                    paragraphs(
                        text_body,
                        |paragraph| NativeLocator::Pptx {
                            slide_ordinal,
                            shape_path: path.clone(),
                            text_slot: PptxTextSlot::ShapeParagraph { paragraph },
                        },
                        body,
                        unlocated,
                        meter,
                    )?;
                }
            }
            "graphicFrame" => {
                let table = shape
                    .child("graphic")
                    .and_then(|node| node.child("graphicData"))
                    .and_then(|node| node.child("tbl"));
                let Some(table) = table else {
                    *unlocated = true;
                    continue;
                };
                for (row_index, row_node) in table.children("tr").enumerate() {
                    let row = u32::try_from(row_index).map_err(|_| resource_limit())?;
                    for (col_index, cell) in row_node.children("tc").enumerate() {
                        let col = u32::try_from(col_index).map_err(|_| resource_limit())?;
                        if let Some(text_body) = cell.child("txBody") {
                            let path = shape_path.clone();
                            paragraphs(
                                text_body,
                                |paragraph| NativeLocator::Pptx {
                                    slide_ordinal,
                                    shape_path: path.clone(),
                                    text_slot: PptxTextSlot::TableCellParagraph {
                                        row,
                                        col,
                                        paragraph,
                                    },
                                },
                                body,
                                unlocated,
                                meter,
                            )?;
                        }
                    }
                }
            }
            _ => *unlocated = true,
        }
    }
    Ok(())
}
