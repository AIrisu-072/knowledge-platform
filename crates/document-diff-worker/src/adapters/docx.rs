use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};

use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, FormatId, RelocationKind, SourceLocator,
    UnverifiedReason, WorkerAncillaryChange, WorkerChange, WorkerDiffRequest, WorkerDiffResponse,
    WorkerUnverifiedRegion,
};
use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, SemanticAdapter, WorkerFailureCode,
};
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::WorkerError;

const MAX_COMBINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 256;
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
const MAX_PARAGRAPHS: usize = 100_000;
const PARSER_PROVENANCE: &str =
    "document-diff-docx-v0;dsi-docx-v0;office_oxide=0.1.11;quick-xml=0.42.0;zip=8.6.0";

#[derive(Debug, Clone, Copy, Default)]
pub struct DocxComparator;

#[derive(Debug)]
struct Package {
    parts: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
struct Paragraph {
    text: String,
    style: Option<String>,
    number: Option<String>,
    stable_id: Option<String>,
    in_table: bool,
    path: String,
}

#[derive(Debug, Default)]
struct DocumentStructure {
    paragraphs: Vec<Paragraph>,
    table_markers: Vec<String>,
    section_markers: Vec<String>,
}

#[derive(Debug, Default)]
struct PendingParagraph {
    text: String,
    style: Option<String>,
    number: Option<String>,
    stable_id: Option<String>,
    in_table: bool,
    path: String,
}

impl DocxComparator {
    pub fn compare(
        request: &WorkerDiffRequest,
        base: &[u8],
        target: &[u8],
        budget: &mut ComparisonBudget,
    ) -> Result<WorkerDiffResponse, WorkerError> {
        if request.format != FormatId::Docx {
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
        let base_inspection = match DocxAdapter.inspect(base, &AdapterProfile::default()) {
            Ok(value) => value,
            Err(error) => return Ok(unverified(request, inspection_reason(error.code()))),
        };
        let target_inspection = match DocxAdapter.inspect(target, &AdapterProfile::default()) {
            Ok(value) => value,
            Err(error) => return Ok(unverified(request, inspection_reason(error.code()))),
        };
        let ancillary_changes = editorial_change(
            base_inspection.editorial_provenance(),
            target_inspection.editorial_provenance(),
        );
        if base_inspection.semantic_fingerprint() == target_inspection.semantic_fingerprint() {
            return Ok(response(
                request,
                DiffCoverage::Full,
                Vec::new(),
                Vec::new(),
                ancillary_changes,
            ));
        }
        let base_package = match read_package(base) {
            Ok(value) => value,
            Err(reason) => return Ok(unverified(request, reason)),
        };
        let target_package = match read_package(target) {
            Ok(value) => value,
            Err(reason) => return Ok(unverified(request, reason)),
        };
        let mut changes = Vec::new();
        let mut unresolved = false;
        let part_names: BTreeSet<_> = base_package
            .parts
            .keys()
            .chain(target_package.parts.keys())
            .collect();
        for part in part_names {
            let old = base_package.parts.get(part);
            let new = target_package.parts.get(part);
            if old == new {
                continue;
            }
            let result = if part == "word/document.xml" {
                compare_document(old, new, budget, &mut changes)
            } else if part.starts_with("word/header") && part.ends_with(".xml") {
                compare_text_part(part, "docx_header", old, new, budget, &mut changes)
            } else if part.starts_with("word/footer") && part.ends_with(".xml") {
                compare_text_part(part, "docx_footer", old, new, budget, &mut changes)
            } else if part == "word/footnotes.xml" {
                compare_text_part(part, "docx_footnote", old, new, budget, &mut changes)
            } else if part == "word/endnotes.xml" {
                compare_text_part(part, "docx_endnote", old, new, budget, &mut changes)
            } else if part.ends_with(".rels") && part.starts_with("word/_rels/") {
                compare_links(part, old, new, budget, &mut changes)
            } else if part.starts_with("word/media/") {
                add_part_change(part, "docx_image", old, new, budget, &mut changes)
            } else if part == "word/numbering.xml" {
                add_part_change(part, "docx_list", old, new, budget, &mut changes)
            } else if part == "word/styles.xml" {
                add_part_change(part, "docx_heading", old, new, budget, &mut changes)
            } else if part == "word/comments.xml" || part == "word/commentsExtended.xml" {
                Ok(())
            } else {
                unresolved = true;
                Ok(())
            };
            if let Err(reason) = result {
                if reason == UnverifiedReason::ResourceLimit {
                    return Ok(unverified(request, reason));
                }
                unresolved = true;
            }
        }
        if changes.is_empty() {
            changes.push(WorkerChange {
                operation: Some(ChangeOperation::Modified),
                relocation: None,
                facet: "docx_content".to_owned(),
                base: Some(SourceLocator::ContentItem),
                target: Some(SourceLocator::ContentItem),
                reason_code: "dsi_fingerprint_changed_location_unknown".to_owned(),
            });
            unresolved = true;
        }
        let regions = if unresolved {
            vec![unverified_region()]
        } else {
            Vec::new()
        };
        let coverage = if unresolved {
            DiffCoverage::Partial
        } else {
            DiffCoverage::Full
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

fn editorial_change(
    base: &document_semantic_inspection_core::EditorialProvenance,
    target: &document_semantic_inspection_core::EditorialProvenance,
) -> Vec<WorkerAncillaryChange> {
    if base == target {
        return Vec::new();
    }
    let old: [u8; 32] =
        Sha256::digest(serde_json::to_vec(base).expect("typed editorial evidence")).into();
    let new: [u8; 32] =
        Sha256::digest(serde_json::to_vec(target).expect("typed editorial evidence")).into();
    vec![WorkerAncillaryChange {
        kind: "docx_editorial".to_owned(),
        base_digest: Some(old),
        target_digest: Some(new),
    }]
}

fn read_package(input: &[u8]) -> Result<Package, UnverifiedReason> {
    let mut archive =
        ZipArchive::new(Cursor::new(input)).map_err(|_| UnverifiedReason::CorruptedSource)?;
    if archive.len() > MAX_ENTRIES {
        return Err(UnverifiedReason::ResourceLimit);
    }
    let mut parts = BTreeMap::new();
    let mut total = 0usize;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| UnverifiedReason::CorruptedSource)?;
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(UnverifiedReason::ResourceLimit);
        }
        let name = entry.name().to_owned();
        if !name.starts_with("word/") || name.ends_with('/') {
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| UnverifiedReason::CorruptedSource)?;
        if bytes.len() as u64 > MAX_ENTRY_BYTES {
            return Err(UnverifiedReason::ResourceLimit);
        }
        total = total.saturating_add(bytes.len());
        if total > MAX_TOTAL_BYTES {
            return Err(UnverifiedReason::ResourceLimit);
        }
        if parts.insert(name, bytes).is_some() {
            return Err(UnverifiedReason::CorruptedSource);
        }
    }
    Ok(Package { parts })
}

fn compare_document(
    base: Option<&Vec<u8>>,
    target: Option<&Vec<u8>>,
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
) -> Result<(), UnverifiedReason> {
    let (Some(base), Some(target)) = (base, target) else {
        return Err(UnverifiedReason::CorruptedSource);
    };
    let old = parse_document_structure(base)?;
    let new = parse_document_structure(target)?;
    if old.table_markers != new.table_markers {
        let path = "word/document.xml#table".to_owned();
        push_change(
            changes,
            budget,
            Some(ChangeOperation::Modified),
            "docx_table",
            Some(&path),
            Some(&path),
            "table_structure_changed",
        )?;
    }
    if old.section_markers != new.section_markers {
        let path = "word/document.xml#section".to_owned();
        push_change(
            changes,
            budget,
            Some(ChangeOperation::Modified),
            "docx_section",
            Some(&path),
            Some(&path),
            "section_structure_changed",
        )?;
    }
    let alignment = align_paragraphs(&old.paragraphs, &new.paragraphs, budget)?;
    for (base_index, target_index) in alignment.pairs {
        let base = &old.paragraphs[base_index];
        let target = &new.paragraphs[target_index];
        let relocation = (base_index != target_index).then_some(RelocationKind::Reordered);
        let text_changed = base.text != target.text;
        if text_changed || relocation.is_some() {
            push_paragraph_change(
                changes,
                budget,
                text_changed.then_some(ChangeOperation::Modified),
                relocation,
                if base.in_table || target.in_table {
                    "docx_table"
                } else {
                    "docx_paragraph"
                },
                base,
                target,
                if text_changed {
                    "visible_text_changed"
                } else {
                    "paragraph_reordered"
                },
            )?;
        }
        for (changed, facet, reason) in [
            (
                base.style != target.style,
                "docx_heading",
                "paragraph_style_changed",
            ),
            (
                base.number != target.number,
                "docx_list",
                "list_numbering_changed",
            ),
            (
                base.in_table != target.in_table,
                "docx_table",
                "table_membership_changed",
            ),
        ] {
            if changed {
                push_paragraph_change(
                    changes,
                    budget,
                    Some(ChangeOperation::Modified),
                    None,
                    facet,
                    base,
                    target,
                    reason,
                )?;
            }
        }
    }
    if alignment.unmatched {
        return Err(UnverifiedReason::AmbiguousAlignment);
    }
    Ok(())
}

struct ParagraphAlignment {
    pairs: Vec<(usize, usize)>,
    unmatched: bool,
}

fn align_paragraphs(
    base: &[Paragraph],
    target: &[Paragraph],
    budget: &mut ComparisonBudget,
) -> Result<ParagraphAlignment, UnverifiedReason> {
    let mut paired = vec![None; base.len()];
    let mut used_target = vec![false; target.len()];
    let mut base_ids: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut target_ids: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, paragraph) in base.iter().enumerate() {
        if let Some(id) = paragraph.stable_id.as_deref() {
            base_ids.entry(id).or_default().push(index);
        }
    }
    for (index, paragraph) in target.iter().enumerate() {
        if let Some(id) = paragraph.stable_id.as_deref() {
            target_ids.entry(id).or_default().push(index);
        }
    }
    for (id, old) in &base_ids {
        if old.len() == 1
            && let Some(new) = target_ids.get(id)
            && new.len() == 1
        {
            paired[old[0]] = Some(new[0]);
            used_target[new[0]] = true;
        }
    }
    let mut base_text = BTreeMap::new();
    let mut target_text = BTreeMap::new();
    for (index, paragraph) in base.iter().enumerate() {
        if paired[index].is_none() && !paragraph.text.is_empty() {
            base_text
                .entry((paragraph.text.as_str(), paragraph.in_table))
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    for (index, paragraph) in target.iter().enumerate() {
        if !used_target[index] && !paragraph.text.is_empty() {
            target_text
                .entry((paragraph.text.as_str(), paragraph.in_table))
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    for (key, old) in &base_text {
        if old.len() == 1
            && let Some(new) = target_text.get(key)
            && new.len() == 1
        {
            paired[old[0]] = Some(new[0]);
            used_target[new[0]] = true;
        }
    }
    let mut base_exact = BTreeMap::new();
    let mut target_exact = BTreeMap::new();
    for (index, paragraph) in base.iter().enumerate() {
        if paired[index].is_none() {
            base_exact
                .entry(paragraph_key(paragraph))
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    for (index, paragraph) in target.iter().enumerate() {
        if !used_target[index] {
            target_exact
                .entry(paragraph_key(paragraph))
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    for (key, old) in &base_exact {
        if let Some(new) = target_exact.get(key)
            && old.len() == new.len()
        {
            for (&old, &new) in old.iter().zip(new) {
                paired[old] = Some(new);
                used_target[new] = true;
            }
        }
    }
    let anchors: Vec<_> = paired
        .iter()
        .enumerate()
        .filter_map(|(old, new)| new.map(|new| (old, new)))
        .collect();
    if anchors.windows(2).all(|pair| pair[0].1 < pair[1].1) {
        let mut boundaries = Vec::with_capacity(anchors.len() + 2);
        boundaries.push((None, None));
        boundaries.extend(anchors.iter().map(|(old, new)| (Some(*old), Some(*new))));
        boundaries.push((Some(base.len()), Some(target.len())));
        for window in boundaries.windows(2) {
            let old_start = window[0].0.map_or(0, |index| index + 1);
            let new_start = window[0].1.map_or(0, |index| index + 1);
            let old_end = window[1].0.expect("end boundary");
            let new_end = window[1].1.expect("end boundary");
            if old_end == old_start + 1 && new_end == new_start + 1 {
                paired[old_start] = Some(new_start);
                used_target[new_start] = true;
            }
        }
    }
    let mut pairs = Vec::new();
    for (old, new) in paired.iter().enumerate() {
        if let Some(new) = new {
            budget
                .consume_candidates(1)
                .map_err(|_| UnverifiedReason::ResourceLimit)?;
            pairs.push((old, *new));
        }
    }
    Ok(ParagraphAlignment {
        pairs,
        unmatched: paired.iter().any(Option::is_none) || used_target.iter().any(|used| !used),
    })
}

fn paragraph_key(paragraph: &Paragraph) -> (&str, Option<&str>, Option<&str>, bool) {
    (
        &paragraph.text,
        paragraph.style.as_deref(),
        paragraph.number.as_deref(),
        paragraph.in_table,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_paragraph_change(
    changes: &mut Vec<WorkerChange>,
    budget: &mut ComparisonBudget,
    operation: Option<ChangeOperation>,
    relocation: Option<RelocationKind>,
    facet: &str,
    base: &Paragraph,
    target: &Paragraph,
    reason: &str,
) -> Result<(), UnverifiedReason> {
    push_change(
        changes,
        budget,
        operation,
        facet,
        Some(&base.path),
        Some(&target.path),
        reason,
    )?;
    changes.last_mut().expect("just pushed").relocation = relocation;
    Ok(())
}

fn parse_document_structure(bytes: &[u8]) -> Result<DocumentStructure, UnverifiedReason> {
    let text = std::str::from_utf8(bytes).map_err(|_| UnverifiedReason::CorruptedSource)?;
    let mut reader = Reader::from_str(text);
    let mut result = DocumentStructure::default();
    let mut current: Option<PendingParagraph> = None;
    let mut in_text = false;
    let mut deletion_depth = 0usize;
    let mut table_depth = 0usize;
    let mut table_index = 0usize;
    let mut row_index = 0usize;
    let mut cell_index = 0usize;
    let mut cell_paragraph_index = 0usize;
    let mut node_count = 0usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| UnverifiedReason::CorruptedSource)?;
        node_count += 1;
        if node_count > 2_000_000 {
            return Err(UnverifiedReason::ResourceLimit);
        }
        match event {
            Event::Start(event) => {
                let local = event.local_name();
                match local.as_ref() {
                    "tbl" => {
                        if table_depth > 0 {
                            return Err(UnverifiedReason::UnsupportedSemanticConstruct);
                        }
                        table_depth += 1;
                        table_index += 1;
                        row_index = 0;
                        result.table_markers.push("table".to_owned());
                    }
                    "tr" if table_depth > 0 => {
                        row_index += 1;
                        cell_index = 0;
                        result.table_markers.push("tr".to_owned());
                    }
                    "tc" if table_depth > 0 => {
                        cell_index += 1;
                        cell_paragraph_index = 0;
                        result.table_markers.push("tc".to_owned());
                    }
                    "gridSpan" | "vMerge" if table_depth > 0 => result.table_markers.push(format!(
                        "{}:{:?}",
                        local.as_ref(),
                        attr_value(&event, "val")?
                    )),
                    "pgSz" | "type" | "cols" | "textDirection" => {
                        result.section_markers.push(marker(&event)?)
                    }
                    "p" => {
                        if result.paragraphs.len() >= MAX_PARAGRAPHS {
                            return Err(UnverifiedReason::ResourceLimit);
                        }
                        let path = if table_depth > 0 {
                            if row_index == 0 || cell_index == 0 {
                                return Err(UnverifiedReason::UnsupportedSemanticConstruct);
                            }
                            cell_paragraph_index += 1;
                            format!(
                                "word/document.xml#table[{table_index}]/row[{row_index}]/cell[{cell_index}]/p[{cell_paragraph_index}]"
                            )
                        } else {
                            format!("word/document.xml#p[{}]", result.paragraphs.len() + 1)
                        };
                        current = Some(PendingParagraph {
                            stable_id: attr_value(&event, "paraId")?,
                            in_table: table_depth > 0,
                            path,
                            ..Default::default()
                        });
                    }
                    "pStyle" => {
                        if let Some(p) = current.as_mut() {
                            p.style = attr_value(&event, "val")?;
                        }
                    }
                    "numId" => {
                        if let Some(p) = current.as_mut() {
                            p.number = attr_value(&event, "val")?;
                        }
                    }
                    "del" | "moveFrom" => deletion_depth += 1,
                    "t" if deletion_depth == 0 => in_text = true,
                    _ => {}
                }
            }
            Event::Empty(event) => {
                let local = event.local_name();
                match local.as_ref() {
                    "tr" | "tc" if table_depth > 0 => {
                        result.table_markers.push(local.as_ref().to_owned())
                    }
                    "gridSpan" | "vMerge" if table_depth > 0 => result.table_markers.push(format!(
                        "{}:{:?}",
                        local.as_ref(),
                        attr_value(&event, "val")?
                    )),
                    "pgSz" | "type" | "cols" | "textDirection" => {
                        result.section_markers.push(marker(&event)?)
                    }
                    "pStyle" => {
                        if let Some(p) = current.as_mut() {
                            p.style = attr_value(&event, "val")?;
                        }
                    }
                    "numId" => {
                        if let Some(p) = current.as_mut() {
                            p.number = attr_value(&event, "val")?;
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(value) if in_text && deletion_depth == 0 => {
                if let Some(p) = current.as_mut() {
                    p.text.push_str(&decode_xml(value.as_ref())?);
                }
            }
            Event::End(event) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "del" | "moveFrom" => deletion_depth = deletion_depth.saturating_sub(1),
                "tbl" => table_depth = table_depth.saturating_sub(1),
                "p" => {
                    if let Some(p) = current.take() {
                        result.paragraphs.push(Paragraph {
                            text: normalize_text(&p.text),
                            style: p.style,
                            number: p.number,
                            stable_id: p.stable_id,
                            in_table: p.in_table,
                            path: p.path,
                        });
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}

fn attr_value(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, UnverifiedReason> {
    for attr in event.attributes() {
        let attr = attr.map_err(|_| UnverifiedReason::CorruptedSource)?;
        if attr.key.local_name().as_ref() == name {
            return Ok(Some(decode_xml(attr.value.as_ref())?));
        }
    }
    Ok(None)
}

fn marker(event: &BytesStart<'_>) -> Result<String, UnverifiedReason> {
    let mut attrs = Vec::new();
    for attr in event.attributes() {
        let attr = attr.map_err(|_| UnverifiedReason::CorruptedSource)?;
        let name = attr.key.local_name().as_ref().to_owned();
        attrs.push((name, decode_xml(attr.value.as_ref())?));
    }
    attrs.sort();
    Ok(format!("{}:{attrs:?}", event.local_name().as_ref()))
}

fn decode_xml(value: &str) -> Result<String, UnverifiedReason> {
    quick_xml::escape::unescape(value)
        .map(|v| v.into_owned())
        .map_err(|_| UnverifiedReason::CorruptedSource)
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compare_text_part(
    part: &str,
    facet: &str,
    base: Option<&Vec<u8>>,
    target: Option<&Vec<u8>>,
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
) -> Result<(), UnverifiedReason> {
    let old = base.map(|bytes| extract_text(bytes)).transpose()?;
    let new = target.map(|bytes| extract_text(bytes)).transpose()?;
    if old == new {
        return Ok(());
    }
    push_change(
        changes,
        budget,
        Some(operation(base, target)),
        facet,
        base.map(|_| part),
        target.map(|_| part),
        "visible_part_changed",
    )
}

fn extract_text(bytes: &[u8]) -> Result<String, UnverifiedReason> {
    let text = std::str::from_utf8(bytes).map_err(|_| UnverifiedReason::CorruptedSource)?;
    let mut reader = Reader::from_str(text);
    let mut output = String::new();
    let mut in_text = false;
    let mut deleted_depth = 0usize;
    let mut nodes = 0usize;
    loop {
        nodes += 1;
        if nodes > 2_000_000 {
            return Err(UnverifiedReason::ResourceLimit);
        }
        match reader
            .read_event()
            .map_err(|_| UnverifiedReason::CorruptedSource)?
        {
            Event::Start(event) => match event.local_name().as_ref() {
                "del" | "moveFrom" => deleted_depth += 1,
                "t" if deleted_depth == 0 => in_text = true,
                _ => {}
            },
            Event::Text(value) if in_text && deleted_depth == 0 => {
                output.push_str(&decode_xml(value.as_ref())?);
                output.push(' ');
            }
            Event::End(event) => match event.local_name().as_ref() {
                "t" => in_text = false,
                "del" | "moveFrom" => deleted_depth = deleted_depth.saturating_sub(1),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(normalize_text(&output))
}

fn compare_links(
    part: &str,
    base: Option<&Vec<u8>>,
    target: Option<&Vec<u8>>,
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
) -> Result<(), UnverifiedReason> {
    let old = base.map(|bytes| hyperlink_targets(bytes)).transpose()?;
    let new = target.map(|bytes| hyperlink_targets(bytes)).transpose()?;
    if old == new {
        return Ok(());
    }
    push_change(
        changes,
        budget,
        Some(operation(base, target)),
        "docx_link",
        base.map(|_| part),
        target.map(|_| part),
        "hyperlink_target_changed",
    )
}

fn hyperlink_targets(bytes: &[u8]) -> Result<Vec<String>, UnverifiedReason> {
    let text = std::str::from_utf8(bytes).map_err(|_| UnverifiedReason::CorruptedSource)?;
    let mut reader = Reader::from_str(text);
    let mut targets = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|_| UnverifiedReason::CorruptedSource)?
        {
            Event::Start(event) | Event::Empty(event)
                if event.local_name().as_ref() == "Relationship" =>
            {
                let kind = attr_value(&event, "Type")?;
                if kind
                    .as_deref()
                    .is_some_and(|value| value.ends_with("/hyperlink"))
                    && let Some(target) = attr_value(&event, "Target")?
                {
                    targets.push(target);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    targets.sort();
    Ok(targets)
}

fn add_part_change(
    part: &str,
    facet: &str,
    base: Option<&Vec<u8>>,
    target: Option<&Vec<u8>>,
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
) -> Result<(), UnverifiedReason> {
    push_change(
        changes,
        budget,
        Some(operation(base, target)),
        facet,
        base.map(|_| part),
        target.map(|_| part),
        "semantic_part_changed",
    )
}

fn operation(base: Option<&Vec<u8>>, target: Option<&Vec<u8>>) -> ChangeOperation {
    match (base, target) {
        (None, Some(_)) => ChangeOperation::Added,
        (Some(_), None) => ChangeOperation::Removed,
        _ => ChangeOperation::Modified,
    }
}

fn push_change(
    changes: &mut Vec<WorkerChange>,
    budget: &mut ComparisonBudget,
    operation: Option<ChangeOperation>,
    facet: &str,
    base: Option<&str>,
    target: Option<&str>,
    reason: &str,
) -> Result<(), UnverifiedReason> {
    budget
        .consume_changes(1)
        .map_err(|_| UnverifiedReason::ResourceLimit)?;
    changes.push(WorkerChange {
        operation,
        relocation: None,
        facet: facet.to_owned(),
        base: base.map(|path| SourceLocator::OfficePath {
            path: path.to_owned(),
        }),
        target: target.map(|path| SourceLocator::OfficePath {
            path: path.to_owned(),
        }),
        reason_code: reason.to_owned(),
    });
    Ok(())
}

fn unverified_region() -> WorkerUnverifiedRegion {
    WorkerUnverifiedRegion {
        base: Some(SourceLocator::ContentItem),
        target: Some(SourceLocator::ContentItem),
        reason: UnverifiedReason::UnsupportedSemanticConstruct,
        navigation_hint: Some("未比較範囲を両原本で確認してください".to_owned()),
    }
}

fn unverified(request: &WorkerDiffRequest, reason: UnverifiedReason) -> WorkerDiffResponse {
    let mut region = unverified_region();
    region.reason = reason;
    response(
        request,
        DiffCoverage::None,
        Vec::new(),
        vec![region],
        Vec::new(),
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
