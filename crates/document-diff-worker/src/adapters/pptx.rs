use std::collections::BTreeMap;

use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, FormatId, RelocationKind, SourceLocator,
    UnverifiedReason, WorkerAncillaryChange, WorkerChange, WorkerDiffRequest, WorkerDiffResponse,
    WorkerUnverifiedRegion,
};
use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, WorkerFailureCode};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::WorkerError;

const MAX_COMBINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_SLIDES: usize = 4_096;
const PARSER_PROVENANCE: &str =
    "document-diff-pptx-v0;dsi-pptx-v0;office_oxide=0.1.11;quick-xml=0.42.0;zip=8.6.0";

#[derive(Debug, Clone, Copy, Default)]
pub struct PptxComparator;

struct SlideAlignment {
    pairs: Vec<(usize, usize)>,
    additions: Vec<usize>,
    removals: Vec<usize>,
    ambiguous: bool,
}

impl PptxComparator {
    pub fn compare(
        request: &WorkerDiffRequest,
        base: &[u8],
        target: &[u8],
        budget: &mut ComparisonBudget,
    ) -> Result<WorkerDiffResponse, WorkerError> {
        if request.format != FormatId::Pptx {
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
        let (old_inspection, old_slides) =
            match PptxAdapter.inspect_with_projection(base, &AdapterProfile::default()) {
                Ok(value) => value,
                Err(error) => return Ok(unverified(request, inspection_reason(error.code()))),
            };
        let (new_inspection, new_slides) =
            match PptxAdapter.inspect_with_projection(target, &AdapterProfile::default()) {
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
        let alignment = match align_slides(&old_slides, &new_slides, budget) {
            Ok(value) => value,
            Err(reason) => return Ok(unverified(request, reason)),
        };
        let mut changes = Vec::new();
        let mut regions = Vec::new();
        for index in alignment.additions {
            if push(
                &mut changes,
                budget,
                Some(ChangeOperation::Added),
                None,
                "pptx_slide",
                None,
                Some(slide(index)),
                "slide_added",
            )
            .is_err()
            {
                regions.push(unverified_region(UnverifiedReason::ResourceLimit));
                break;
            }
        }
        for index in alignment.removals {
            if push(
                &mut changes,
                budget,
                Some(ChangeOperation::Removed),
                None,
                "pptx_slide",
                Some(slide(index)),
                None,
                "slide_removed",
            )
            .is_err()
            {
                regions.push(unverified_region(UnverifiedReason::ResourceLimit));
                break;
            }
        }
        for (old_index, new_index) in alignment.pairs {
            if old_index != new_index
                && push(
                    &mut changes,
                    budget,
                    None,
                    Some(RelocationKind::Reordered),
                    "pptx_slide",
                    Some(slide(old_index)),
                    Some(slide(new_index)),
                    "slide_reordered",
                )
                .is_err()
            {
                regions.push(unverified_region(UnverifiedReason::ResourceLimit));
                break;
            }
            if let Err(reason) = compare_slide(
                &old_slides[old_index],
                &new_slides[new_index],
                old_index,
                new_index,
                budget,
                &mut changes,
            ) {
                regions.push(unverified_region(reason));
            }
        }
        if alignment.ambiguous {
            regions.push(unverified_region(UnverifiedReason::AmbiguousAlignment));
        }
        if old_inspection.external_dependencies() != new_inspection.external_dependencies()
            && !changes.iter().any(|change| change.facet == "pptx_link")
        {
            if let Err(reason) = push(
                &mut changes,
                budget,
                Some(ChangeOperation::Modified),
                None,
                "pptx_link",
                Some(SourceLocator::ContentItem),
                Some(SourceLocator::ContentItem),
                "external_hyperlink_changed",
            ) {
                regions.push(unverified_region(reason));
            }
        }
        if changes.is_empty() {
            changes.push(WorkerChange {
                operation: Some(ChangeOperation::Modified),
                relocation: None,
                facet: "pptx_content".to_owned(),
                base: Some(SourceLocator::ContentItem),
                target: Some(SourceLocator::ContentItem),
                reason_code: "dsi_fingerprint_changed_location_unknown".to_owned(),
            });
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

fn align_slides(
    base: &[Value],
    target: &[Value],
    budget: &mut ComparisonBudget,
) -> Result<SlideAlignment, UnverifiedReason> {
    if base.len() > MAX_SLIDES || target.len() > MAX_SLIDES {
        return Err(UnverifiedReason::ResourceLimit);
    }
    let mut paired = vec![None; base.len()];
    let mut used_target = vec![false; target.len()];
    let mut old_names: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut new_names: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, value) in base.iter().enumerate() {
        if let Some(name) = value["name"].as_str().filter(|name| !name.is_empty()) {
            old_names.entry(name).or_default().push(index);
        }
    }
    for (index, value) in target.iter().enumerate() {
        if let Some(name) = value["name"].as_str().filter(|name| !name.is_empty()) {
            new_names.entry(name).or_default().push(index);
        }
    }
    for (name, old) in &old_names {
        if old.len() == 1
            && let Some(new) = new_names.get(name)
            && new.len() == 1
        {
            paired[old[0]] = Some(new[0]);
            used_target[new[0]] = true;
        }
    }
    let mut old_exact: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    let mut new_exact: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (index, value) in base.iter().enumerate() {
        if paired[index].is_none() {
            old_exact
                .entry(semantic_key(value))
                .or_default()
                .push(index);
        }
    }
    for (index, value) in target.iter().enumerate() {
        if !used_target[index] {
            new_exact
                .entry(semantic_key(value))
                .or_default()
                .push(index);
        }
    }
    for (key, old) in &old_exact {
        if let Some(new) = new_exact.get(key)
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
    let monotonic = anchors.windows(2).all(|pair| pair[0].1 < pair[1].1);
    if monotonic {
        let mut boundaries = vec![(None, None)];
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
    let pairs: Vec<_> = paired
        .iter()
        .enumerate()
        .filter_map(|(old, new)| new.map(|new| (old, new)))
        .collect();
    budget
        .consume_candidates(pairs.len() as u64)
        .map_err(|_| UnverifiedReason::ResourceLimit)?;
    let removals: Vec<_> = paired
        .iter()
        .enumerate()
        .filter_map(|(index, paired)| paired.is_none().then_some(index))
        .collect();
    let additions: Vec<_> = used_target
        .iter()
        .enumerate()
        .filter_map(|(index, used)| (!used).then_some(index))
        .collect();
    let one_sided = removals.is_empty() || additions.is_empty();
    let ambiguous = !one_sided || !monotonic && (!removals.is_empty() || !additions.is_empty());
    Ok(SlideAlignment {
        pairs,
        additions: if one_sided && monotonic {
            additions
        } else {
            vec![]
        },
        removals: if one_sided && monotonic {
            removals
        } else {
            vec![]
        },
        ambiguous,
    })
}

fn semantic_key(value: &Value) -> Vec<u8> {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("index");
    }
    serde_json::to_vec(&value).expect("qualified slide projection")
}

fn compare_slide(
    base: &Value,
    target: &Value,
    old_index: usize,
    new_index: usize,
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
) -> Result<(), UnverifiedReason> {
    if base == target || semantic_key(base) == semantic_key(target) {
        return Ok(());
    }
    let before = changes.len();
    let mut emit = |facet: &str, reason: &str| {
        push(
            changes,
            budget,
            Some(ChangeOperation::Modified),
            None,
            facet,
            Some(slide(old_index)),
            Some(slide(new_index)),
            reason,
        )
    };
    if base["name"] != target["name"] || base["hidden"] != target["hidden"] {
        emit("pptx_slide", "slide_metadata_changed")?;
    }
    if base["speaker_notes"] != target["speaker_notes"] {
        emit("pptx_notes", "speaker_notes_changed")?;
    }
    if base["shape_click_hyperlinks"] != target["shape_click_hyperlinks"] {
        emit("pptx_link", "shape_click_hyperlink_changed")?;
    }
    if base["connector_relationships"] != target["connector_relationships"] {
        emit("pptx_shape", "shape_relationship_changed")?;
    }
    if base["picture_transforms"] != target["picture_transforms"] {
        emit("pptx_layout", "picture_transform_changed")?;
    }
    for (kind, facet) in [("chart", "pptx_chart"), ("smartart", "pptx_smartart")] {
        if collect_kind_nodes(&base["raw_graphics"], kind)
            != collect_kind_nodes(&target["raw_graphics"], kind)
        {
            emit(facet, "graphic_semantics_changed")?;
        }
    }
    let old_shapes = &base["shapes"];
    let new_shapes = &target["shapes"];
    if old_shapes != new_shapes {
        let mut specific_shape_change = false;
        if collect_visible_text(old_shapes) != collect_visible_text(new_shapes) {
            emit("pptx_text", "visible_text_changed")?;
            specific_shape_change = true;
        }
        if collect_kind_nodes(old_shapes, "table") != collect_kind_nodes(new_shapes, "table") {
            emit("pptx_table", "table_changed")?;
            specific_shape_change = true;
        }
        if collect_kind_nodes(old_shapes, "group") != collect_kind_nodes(new_shapes, "group") {
            emit("pptx_group", "group_structure_changed")?;
            specific_shape_change = true;
        }
        if collect_key(old_shapes, "image_semantic_sha256")
            != collect_key(new_shapes, "image_semantic_sha256")
        {
            emit("pptx_image", "image_semantics_changed")?;
            specific_shape_change = true;
        }
        if collect_key(old_shapes, "hyperlink") != collect_key(new_shapes, "hyperlink") {
            emit("pptx_link", "shape_hyperlink_changed")?;
            specific_shape_change = true;
        }
        if collect_key_in_order(old_shapes, "position")
            != collect_key_in_order(new_shapes, "position")
        {
            emit("pptx_layout", "meaningful_shape_position_changed")?;
            specific_shape_change = true;
        }
        if collect_key(old_shapes, "kind") != collect_key(new_shapes, "kind")
            || collect_key(old_shapes, "alt_text") != collect_key(new_shapes, "alt_text")
            || !specific_shape_change
        {
            emit("pptx_shape", "shape_association_changed")?;
        }
        if duplicated_shape_identity(old_shapes) || duplicated_shape_identity(new_shapes) {
            return Err(UnverifiedReason::AmbiguousAlignment);
        }
    }
    if changes.len() == before {
        return Err(UnverifiedReason::UnsupportedSemanticConstruct);
    }
    Ok(())
}

fn duplicated_shape_identity(shapes: &Value) -> bool {
    let Some(shapes) = shapes.as_array() else {
        return true;
    };
    let mut seen = BTreeMap::new();
    for shape in shapes {
        let mut key = shape.clone();
        if let Some(object) = key.as_object_mut() {
            object.remove("position");
        }
        let bytes = serde_json::to_vec(&key).expect("qualified shape projection");
        if seen.insert(bytes, ()).is_some() {
            return true;
        }
    }
    false
}

fn collect_kind_nodes(value: &Value, kind: &str) -> Vec<Vec<u8>> {
    let mut values = Vec::new();
    let mut pending = vec![value];
    while let Some(node) = pending.pop() {
        if node["kind"].as_str() == Some(kind) {
            values.push(serde_json::to_vec(node).expect("qualified projection"));
        }
        match node {
            Value::Array(array) => pending.extend(array),
            Value::Object(object) => pending.extend(object.values()),
            _ => {}
        }
    }
    values.sort();
    values
}

fn collect_key(value: &Value, key: &str) -> Vec<Vec<u8>> {
    let mut values = collect_key_in_order(value, key);
    values.sort();
    values
}

fn collect_visible_text(value: &Value) -> Vec<String> {
    let mut values = Vec::new();
    let mut pending = vec![value];
    while let Some(node) = pending.pop() {
        match node {
            Value::Array(array) => pending.extend(array),
            Value::Object(object) => {
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    values.push(text.to_owned());
                }
                pending.extend(object.values());
            }
            _ => {}
        }
    }
    values.sort();
    values
}

fn collect_key_in_order(value: &Value, key: &str) -> Vec<Vec<u8>> {
    let mut values = Vec::new();
    let mut pending = vec![value];
    while let Some(node) = pending.pop() {
        match node {
            Value::Array(array) => pending.extend(array.iter().rev()),
            Value::Object(object) => {
                if let Some(value) = object.get(key) {
                    values.push(serde_json::to_vec(value).expect("qualified projection"));
                }
                pending.extend(object.values().rev());
            }
            _ => {}
        }
    }
    values
}

fn slide(index: usize) -> SourceLocator {
    SourceLocator::SlideObject {
        slide: (index + 1) as u32,
        object: None,
    }
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

fn editorial_change(
    base: &document_semantic_inspection_core::EditorialProvenance,
    target: &document_semantic_inspection_core::EditorialProvenance,
) -> Vec<WorkerAncillaryChange> {
    if base == target {
        return vec![];
    }
    vec![WorkerAncillaryChange {
        kind: "pptx_editorial".to_owned(),
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

#[cfg(test)]
mod tests {
    use document_diff_core::{ComparisonBudget, UnverifiedReason};
    use serde_json::json;

    use super::compare_slide;

    #[test]
    fn duplicate_shape_correspondence_marks_slide_unverified() {
        let shape = json!({"kind":"auto_shape","position":{"x":1},"text":{"text":"Same"}});
        let base = json!({"shapes":[shape.clone(),shape.clone()]});
        let target = json!({"shapes":[shape,json!({"kind":"auto_shape","position":{"x":2},"text":{"text":"Changed"}})]});
        let mut budget = ComparisonBudget::new(10, 10);
        let mut changes = vec![];
        assert_eq!(
            compare_slide(&base, &target, 0, 0, &mut budget, &mut changes),
            Err(UnverifiedReason::AmbiguousAlignment)
        );
        assert!(
            changes
                .iter()
                .all(|change| change.base.is_some() && change.target.is_some())
        );
    }
}
