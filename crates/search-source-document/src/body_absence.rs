//! P1-A04 finite Source-owned exact negative proof.
//!
//! The scan reads the pinned published manifest, never the lexical index: every
//! Unit of every visible authoritative item of the selector parent is checked
//! for the normalized literal within a finite budget. Only a complete scan of
//! `Completed + Supported` items, re-confirmed against the Source and the
//! caller's Read right before issue, yields `ProvenAbsent`.

use std::time::Instant;

use document_domain::DocumentVersionId;
use search_application::body_ports::{
    CONTAINS_EXACT_PREDICATE, ExactScanBudget, ExactTextAbsenceOutcome, ExactTextNegativeProof,
    ExactTextSelector, NegativeProofFields, PinnedBodyBundle, SourceExactTextAbsencePort,
};
use search_application::ports::BoxFuture;
use search_core::discovery::{DiscoveryRequest, GapReason, InformationGap};
use search_core::knowledge_unit::{KnowledgeUnit, normalize_unit_text, text_sha256};
use search_core::projection::ProjectionGenerationKey;
use search_extraction_core::{BodyCoverage, ItemOperationState};
use sha2::{Digest, Sha256};

use crate::body_evidence::{DocumentExactTextEvidenceCatalog, current_live};
use crate::body_manifest::{BodyItemEntry, version_ref};
use crate::model::AuthoritativeItemBinding;

fn unknown(code: &str, reason: GapReason) -> ExactTextAbsenceOutcome {
    ExactTextAbsenceOutcome::Unknown(InformationGap::new(code, reason, true))
}

const UNBOUND: &str = "document.body.absence_unbound";
const BUNDLE_CHANGED: &str = "document.body.absence_bundle_changed";
const UNVERIFIABLE: &str = "document.body.absence_unverifiable";
const COVERAGE: &str = "document.body.absence_coverage";
const BUDGET: &str = "document.body.absence_budget";
const UNAVAILABLE: &str = "document.body.absence_unavailable";

fn frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u32).to_be_bytes());
    hasher.update(bytes);
}

fn item_binding(hasher: &mut Sha256, item: &AuthoritativeItemBinding) {
    hasher.update(item.content_item_id.as_bytes());
    hasher.update(item.part.ordinal.to_be_bytes());
    frame(hasher, item.part.logical_path.as_bytes());
    frame(hasher, item.part.source_native_part_id.as_bytes());
    hasher.update(item.representation_id.as_bytes());
    hasher.update(item.raw.sha256);
    hasher.update(item.raw.size_bytes.to_be_bytes());
    frame(hasher, item.raw.media_type.as_bytes());
}

fn unit_binding(hasher: &mut Sha256, unit: &KnowledgeUnit) -> bool {
    let Ok(locator) = unit.locator.encode() else {
        return false;
    };
    frame(hasher, unit.unit_id.to_string().as_bytes());
    hasher.update(unit.ordinal.to_be_bytes());
    frame(hasher, &locator);
    hasher.update(unit.text_sha256);
    true
}

fn supported(entry: &BodyItemEntry) -> bool {
    entry.operation == ItemOperationState::Completed
        && entry.coverage == Some(BodyCoverage::Supported)
        && entry.profile.is_some()
}

impl DocumentExactTextEvidenceCatalog {
    async fn scan(
        &self,
        request: &DiscoveryRequest,
        pinned: &PinnedBodyBundle,
        selector: &ExactTextSelector,
        budget: ExactScanBudget,
    ) -> ExactTextAbsenceOutcome {
        let expected = &selector.expected_exact_text;
        if self.selectors.get(&selector.claim_id) != Some(selector)
            || !request.need.required_claims.contains(&selector.claim_id)
            || pinned.generation.source_id != self.source_id
            || selector.predicate != CONTAINS_EXACT_PREDICATE
            || expected.is_empty()
            || normalize_unit_text(expected) != *expected
        {
            return unknown(UNBOUND, GapReason::UnsupportedCoverage);
        }
        let body = match self.runtime.published_body(pinned.generation) {
            Ok(Some(body)) if body.receipt.composite_digest == pinned.composite_digest => body,
            Ok(_) => return unknown(BUNDLE_CHANGED, GapReason::Availability),
            Err(_) => return unknown(UNAVAILABLE, GapReason::Availability),
        };
        // Denied and undecidable parents look the same from outside.
        if !self.allowed(selector.parent_resource, request).await {
            return unknown(UNVERIFIABLE, GapReason::Availability);
        }
        let version = DocumentVersionId::from_uuid(selector.parent_resource.as_uuid());
        let record = match self.versions.load_version(version).await {
            Ok(Some(record)) if current_live(&record) => record,
            Ok(_) => return unknown(UNVERIFIABLE, GapReason::Availability),
            Err(_) => return unknown(UNAVAILABLE, GapReason::Availability),
        };
        let parent = version_ref(self.source_id, &record);
        let mut items: Vec<&AuthoritativeItemBinding> = record.authoritative_items.iter().collect();
        if items.len() as u64 > budget.max_visible_items {
            return unknown(BUDGET, GapReason::Availability);
        }
        items.sort_by(|left, right| {
            (
                left.part.ordinal,
                &left.part.logical_path,
                &left.part.source_native_part_id,
            )
                .cmp(&(
                    right.part.ordinal,
                    &right.part.logical_path,
                    &right.part.source_native_part_id,
                ))
        });
        // The pinned manifest must hold exactly the parent's visible items.
        let entries: Vec<&BodyItemEntry> = body
            .manifest
            .entries
            .iter()
            .filter(|entry| entry.version == parent)
            .collect();
        if entries.len() != items.len() {
            return unknown(COVERAGE, GapReason::UnsupportedCoverage);
        }
        let mut item_digest = Sha256::new();
        item_digest.update(b"body-absence-items:v1\0");
        let mut unit_digest = Sha256::new();
        unit_digest.update(b"body-absence-units:v1\0");
        let (mut scanned, mut bytes) = (0u64, 0u64);
        for item in &items {
            let Some(entry) = entries.iter().find(|entry| {
                entry.part == item.part
                    && entry.authoritative_representation_ref == item.representation_id.to_string()
                    && entry.raw == item.raw
            }) else {
                return unknown(COVERAGE, GapReason::UnsupportedCoverage);
            };
            let covered = body.coverage.items.iter().find(|covered| {
                covered.version == parent && covered.part == item.part && covered.raw == item.raw
            });
            let consistent = covered.is_some_and(|covered| {
                covered.operation == entry.operation
                    && covered.coverage == entry.coverage
                    && covered.unit_count as usize == entry.units.len()
            });
            // Partial, Unsupported or Failed items cannot support absence.
            if !supported(entry) || !consistent {
                return unknown(COVERAGE, GapReason::UnsupportedCoverage);
            }
            item_binding(&mut item_digest, item);
            let mut units: Vec<&KnowledgeUnit> = entry.units.iter().collect();
            units.sort_by_key(|unit| unit.ordinal);
            for unit in units {
                scanned += 1;
                bytes += unit.text.len() as u64;
                if scanned > budget.max_units
                    || bytes > budget.max_text_bytes
                    || Instant::now() >= budget.deadline
                {
                    return unknown(BUDGET, GapReason::Availability);
                }
                if unit.version != parent
                    || unit.part != item.part
                    || unit.provenance.authoritative_representation_ref
                        != entry.authoritative_representation_ref
                    || unit.provenance.raw != item.raw
                    || Some(&unit.provenance.profile) != entry.profile.as_ref()
                    || text_sha256(&unit.text) != unit.text_sha256
                    || !unit_binding(&mut unit_digest, unit)
                {
                    return unknown(COVERAGE, GapReason::UnsupportedCoverage);
                }
                if unit.text.contains(expected.as_str()) {
                    return ExactTextAbsenceOutcome::MatchFound;
                }
            }
        }
        // Source, Read and the pinned bundle are confirmed again before issue.
        if !self.allowed(selector.parent_resource, request).await {
            return unknown(UNVERIFIABLE, GapReason::Availability);
        }
        match self.versions.load_version(version).await {
            Ok(Some(again)) if again == record => {}
            Ok(_) => return unknown(UNVERIFIABLE, GapReason::Availability),
            Err(_) => return unknown(UNAVAILABLE, GapReason::Availability),
        }
        match self.runtime.published_body(pinned.generation) {
            Ok(Some(again)) if again.receipt == body.receipt => {}
            _ => return unknown(BUNDLE_CHANGED, GapReason::Availability),
        }
        ExactTextAbsenceOutcome::ProvenAbsent(Box::new(ExactTextNegativeProof::issue(
            NegativeProofFields {
                generation: pinned.generation,
                bundle_digest: body.receipt.composite_digest,
                source_snapshot: body.manifest.source_snapshot.clone(),
                claim_id: selector.claim_id,
                parent,
                document_revision: record.document_revision,
                access_revision: record.access_revision,
                exact_text_sha256: Sha256::digest(expected.as_bytes()).into(),
                visible_item_bindings_digest: item_digest.finalize().into(),
                scanned_unit_bindings_digest: unit_digest.finalize().into(),
                visible_item_count: items.len() as u64,
                scanned_unit_count: scanned,
            },
        )))
    }
}

impl SourceExactTextAbsencePort for DocumentExactTextEvidenceCatalog {
    fn pin_body<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Option<PinnedBodyBundle>> {
        Box::pin(async move {
            if generation.source_id != self.source_id {
                return Ok(None);
            }
            Ok(self
                .runtime
                .published_body(generation)?
                .map(|body| PinnedBodyBundle {
                    generation,
                    composite_digest: body.receipt.composite_digest,
                }))
        })
    }

    fn verify_absence<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        pinned: &'a PinnedBodyBundle,
        selector: &'a ExactTextSelector,
        budget: ExactScanBudget,
    ) -> BoxFuture<'a, ExactTextAbsenceOutcome> {
        Box::pin(async move { Ok(self.scan(request, pinned, selector, budget).await) })
    }
}
