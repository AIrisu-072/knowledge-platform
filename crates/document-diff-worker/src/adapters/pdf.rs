use std::collections::BTreeMap;

use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, FormatId, RelocationKind, SourceLocator,
    UnverifiedReason, WorkerAncillaryChange, WorkerChange, WorkerDiffRequest, WorkerDiffResponse,
    WorkerUnverifiedRegion,
};
use document_semantic_inspection_worker::{AdapterProfile, PdfAdapter, WorkerFailureCode};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::WorkerError;

const MAX_COMBINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_PAGES: usize = 4_096;
const PARSER_PROVENANCE: &str =
    "document-diff-pdf-v0;dsi-pdf-v0;pdfium-render=0.9.4;pdfium=151.0.7881.0;lopdf=0.45.0";

#[derive(Debug, Clone, Copy, Default)]
pub struct PdfComparator;

#[derive(Default)]
struct PageAlignment {
    pairs: Vec<(usize, usize)>,
    additions: Vec<usize>,
    removals: Vec<usize>,
    ambiguous: bool,
}

impl PdfComparator {
    pub fn compare(
        request: &WorkerDiffRequest,
        base: &[u8],
        target: &[u8],
        budget: &mut ComparisonBudget,
    ) -> Result<WorkerDiffResponse, WorkerError> {
        if request.format != FormatId::Pdf {
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
        let (old_inspection, old_projection) =
            match PdfAdapter.inspect_with_projection(base, &AdapterProfile::default()) {
                Ok(value) => value,
                Err(error) => return Ok(unverified(request, inspection_reason(error.code()))),
            };
        let (new_inspection, new_projection) =
            match PdfAdapter.inspect_with_projection(target, &AdapterProfile::default()) {
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
        let (Some(old_pages), Some(new_pages)) = (
            old_projection["pages"].as_array(),
            new_projection["pages"].as_array(),
        ) else {
            return Ok(unverified(
                request,
                UnverifiedReason::UnsupportedSemanticConstruct,
            ));
        };
        let alignment = match align_pages(old_pages, new_pages, budget) {
            Ok(alignment) => alignment,
            Err(reason) => return Ok(unverified(request, reason)),
        };
        let mut changes = Vec::new();
        let mut regions = Vec::new();
        for index in alignment.additions {
            if let Err(reason) = push(
                &mut changes,
                budget,
                Some(ChangeOperation::Added),
                None,
                "pdf_page",
                None,
                Some(page(index)),
                "page_added",
            ) {
                regions.push(unverified_region(reason));
                break;
            }
        }
        for index in alignment.removals {
            if let Err(reason) = push(
                &mut changes,
                budget,
                Some(ChangeOperation::Removed),
                None,
                "pdf_page",
                Some(page(index)),
                None,
                "page_removed",
            ) {
                regions.push(unverified_region(reason));
                break;
            }
        }
        for (old_index, new_index) in alignment.pairs {
            if old_index != new_index
                && let Err(reason) = push(
                    &mut changes,
                    budget,
                    None,
                    Some(RelocationKind::Reordered),
                    "pdf_page",
                    Some(page(old_index)),
                    Some(page(new_index)),
                    "page_reordered",
                )
            {
                regions.push(unverified_region(reason));
                break;
            }
            if let Err(reason) = compare_page(
                &old_pages[old_index],
                &new_pages[new_index],
                old_index,
                new_index,
                budget,
                &mut changes,
                &mut regions,
            ) {
                regions.push(unverified_region(reason));
                break;
            }
        }
        if alignment.ambiguous {
            regions.push(unverified_region(UnverifiedReason::AmbiguousAlignment));
        }
        if old_projection["form_values"] != new_projection["form_values"] {
            match push(
                &mut changes,
                budget,
                Some(ChangeOperation::Modified),
                None,
                "pdf_form",
                Some(SourceLocator::ContentItem),
                Some(SourceLocator::ContentItem),
                "form_value_changed_page_unknown",
            ) {
                Ok(()) => regions.push(unverified_region(
                    UnverifiedReason::UnsupportedSemanticConstruct,
                )),
                Err(reason) => regions.push(unverified_region(reason)),
            }
        }
        if changes.is_empty() && regions.is_empty() {
            regions.push(unverified_region(
                UnverifiedReason::UnsupportedSemanticConstruct,
            ));
        }
        let coverage = if regions.is_empty() {
            DiffCoverage::Full
        } else if changes.is_empty() {
            DiffCoverage::None
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

fn align_pages(
    base: &[Value],
    target: &[Value],
    budget: &mut ComparisonBudget,
) -> Result<PageAlignment, UnverifiedReason> {
    if base.len() > MAX_PAGES || target.len() > MAX_PAGES {
        return Err(UnverifiedReason::ResourceLimit);
    }
    let mut old_keys: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    let mut new_keys: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (index, value) in base.iter().enumerate() {
        old_keys.entry(semantic_key(value)).or_default().push(index);
    }
    for (index, value) in target.iter().enumerate() {
        new_keys.entry(semantic_key(value)).or_default().push(index);
    }
    budget
        .consume_candidates((base.len() + target.len()) as u64)
        .map_err(|_| UnverifiedReason::ResourceLimit)?;
    let mut paired = vec![None; base.len()];
    let mut used_target = vec![false; target.len()];
    for (key, old) in &old_keys {
        if old.len() == 1
            && let Some(new) = new_keys.get(key)
            && new.len() == 1
        {
            paired[old[0]] = Some(new[0]);
            used_target[new[0]] = true;
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
        .filter_map(|(index, new)| new.is_none().then_some(index))
        .collect();
    let additions: Vec<_> = used_target
        .iter()
        .enumerate()
        .filter_map(|(index, used)| (!used).then_some(index))
        .collect();
    let one_sided = removals.is_empty() || additions.is_empty();
    let ambiguous = !one_sided || !monotonic && (!removals.is_empty() || !additions.is_empty());
    Ok(PageAlignment {
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
    serde_json::to_vec(&value).expect("qualified PDF projection")
}

fn compare_page(
    base: &Value,
    target: &Value,
    old_index: usize,
    new_index: usize,
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
    regions: &mut Vec<WorkerUnverifiedRegion>,
) -> Result<(), UnverifiedReason> {
    if semantic_key(base) == semantic_key(target) {
        return Ok(());
    }
    for (field, facet, reason) in [
        ("text", "pdf_text", "native_text_changed"),
        ("links", "pdf_link", "page_link_changed"),
        ("images", "pdf_visual", "image_region_changed"),
        ("paint_order", "pdf_visual", "paint_order_changed"),
    ] {
        if base[field] != target[field] {
            push(
                changes,
                budget,
                Some(ChangeOperation::Modified),
                None,
                facet,
                Some(page(old_index)),
                Some(page(new_index)),
                reason,
            )?;
        }
    }
    if base["images"] != target["images"] || base["paint_order"] != target["paint_order"] {
        regions.push(WorkerUnverifiedRegion {
            base: Some(page(old_index)),
            target: Some(page(new_index)),
            reason: UnverifiedReason::UnsupportedSemanticConstruct,
            navigation_hint: Some(
                "視覚差の矩形範囲は未確定です。両原本の該当ページを確認してください".to_owned(),
            ),
        });
    }
    Ok(())
}

fn page(index: usize) -> SourceLocator {
    SourceLocator::PdfPage {
        page: (index + 1) as u32,
        region: None,
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

fn editorial_change(
    base: &document_semantic_inspection_core::EditorialProvenance,
    target: &document_semantic_inspection_core::EditorialProvenance,
) -> Vec<WorkerAncillaryChange> {
    if base == target {
        return vec![];
    }
    vec![WorkerAncillaryChange {
        kind: "pdf_editorial".to_owned(),
        base_digest: Some(
            Sha256::digest(serde_json::to_vec(base).expect("typed editorial evidence")).into(),
        ),
        target_digest: Some(
            Sha256::digest(serde_json::to_vec(target).expect("typed editorial evidence")).into(),
        ),
    }]
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
