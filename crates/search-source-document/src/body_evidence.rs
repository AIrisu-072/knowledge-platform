//! P1-A03 Source-owned exact-text evidence and access-filtered body coverage.
//!
//! A positive `document.body.contains_exact` Claim needs more than a lexical
//! hit: the Unit must exist in the pinned published manifest, the parent must
//! still be the current Live Version the caller may Read, and the same raw
//! bytes must rebuild the same Unit and literal span. Any change, uncertainty or
//! mismatch yields `None`.

use std::collections::BTreeMap;
use std::sync::Arc;

use document_domain::{DocumentVersionId, LifecycleState};
use search_application::SearchError;
use search_application::body_ports::{
    BodyCoverageGapPort, CONTAINS_EXACT_PREDICATE, ExactTextEvidencePort, ExactTextSelector,
    KnowledgeUnitHitRef, VerifiedExtractedTextEvidence,
};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, ResolvedAssertionEvidence,
};
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::discovery::{DiscoveryRequest, GapReason, InformationGap};
use search_core::evidence::EvidenceRole;
use search_core::id::{ClaimId, ResourceId, SourceId};
use search_core::knowledge_unit::{KnowledgeUnit, normalize_unit_text, text_sha256};
use search_core::predicate::TypedValue;
use search_core::projection::ProjectionGenerationKey;
use search_extraction_core::{BodyCoverage, ItemOperationState};
use time::OffsetDateTime;

use crate::extraction::BodyItemExtractor;
use crate::model::AuthoritativeItemBinding;
use crate::outbox::MemoryDocumentIndexRuntime;
use crate::postgres::{
    DocumentSnapshotReader, PostgresDocumentSnapshotReader, VersionSnapshotRecord,
};

/// Object-safe read of one Document Version from the authoritative Source.
pub trait CurrentVersionReader: Send + Sync {
    fn load_version<'a>(
        &'a self,
        version: DocumentVersionId,
    ) -> BoxFuture<'a, Option<VersionSnapshotRecord>>;
}

impl CurrentVersionReader for PostgresDocumentSnapshotReader {
    fn load_version<'a>(
        &'a self,
        version: DocumentVersionId,
    ) -> BoxFuture<'a, Option<VersionSnapshotRecord>> {
        Box::pin(async move {
            self.load_document_version(version)
                .await
                .map_err(|error| SearchError::SourceUnavailable(error.to_string()))
        })
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Only completed Supported or Partial items furnish searchable Units.
fn searchable(operation: &ItemOperationState, coverage: Option<&BodyCoverage>) -> bool {
    matches!(operation, ItemOperationState::Completed)
        && matches!(
            coverage,
            Some(BodyCoverage::Supported | BodyCoverage::Partial { .. })
        )
}

pub(crate) fn current_live(record: &VersionSnapshotRecord) -> bool {
    let snapshot = &record.snapshot;
    snapshot.current_version_id == Some(snapshot.document_version_id)
        && snapshot.lifecycle_state == LifecycleState::Published
        && snapshot.publication_end.is_none()
}

fn same_unit(unit: &KnowledgeUnit, hit: &KnowledgeUnitHitRef) -> bool {
    unit.unit_id == hit.unit_id
        && unit.version == hit.version
        && unit.part == hit.part
        && unit.text_sha256 == hit.text_sha256
        && unit.provenance.profile == hit.profile
        && unit.provenance.raw == hit.raw
        && unit.provenance.authoritative_representation_ref == hit.authoritative_representation_ref
        && unit
            .locator
            .encode()
            .is_ok_and(|locator| hex(&locator) == hit.opaque_locator)
}

fn same_item(item: &AuthoritativeItemBinding, hit: &KnowledgeUnitHitRef) -> bool {
    item.part == hit.part
        && item.representation_id.to_string() == hit.authoritative_representation_ref
        && item.raw == hit.raw
}

fn literal_at(unit: &KnowledgeUnit, hit: &KnowledgeUnitHitRef, expected: &str) -> bool {
    unit.text
        .get(hit.span.start_byte as usize..hit.span.end_byte as usize)
        == Some(expected)
}

/// Trusted exact-text selectors and the Source-owned resolver for Unit hits.
pub struct DocumentExactTextEvidenceCatalog {
    pub(crate) source_id: SourceId,
    pub(crate) runtime: MemoryDocumentIndexRuntime,
    pub(crate) versions: Arc<dyn CurrentVersionReader>,
    pub(crate) access: Arc<dyn CurrentAccessEvaluatorPort>,
    extractor: Arc<dyn BodyItemExtractor>,
    pub(crate) selectors: BTreeMap<ClaimId, ExactTextSelector>,
}

impl DocumentExactTextEvidenceCatalog {
    pub fn new(
        source_id: SourceId,
        runtime: MemoryDocumentIndexRuntime,
        versions: Arc<dyn CurrentVersionReader>,
        access: Arc<dyn CurrentAccessEvaluatorPort>,
        extractor: Arc<dyn BodyItemExtractor>,
    ) -> Self {
        Self {
            source_id,
            runtime,
            versions,
            access,
            extractor,
            selectors: BTreeMap::new(),
        }
    }

    /// Trusted query preparation registers the selector a request may use.
    pub fn register(&mut self, selector: ExactTextSelector) -> Result<(), SearchError> {
        let text = &selector.expected_exact_text;
        if selector.predicate != CONTAINS_EXACT_PREDICATE
            || text.is_empty()
            || normalize_unit_text(text) != *text
        {
            return Err(SearchError::InvalidRequest(
                "exact-text selector needs contains_exact and a normalized nonempty literal".into(),
            ));
        }
        self.selectors.insert(selector.claim_id, selector);
        Ok(())
    }

    pub(crate) async fn allowed(&self, resource: ResourceId, request: &DiscoveryRequest) -> bool {
        matches!(
            self.access
                .evaluate(resource, &request.access_context)
                .await,
            Ok(AccessDecision::Allowed)
        )
    }

    async fn verify(
        &self,
        request: &DiscoveryRequest,
        hit: &KnowledgeUnitHitRef,
        selector: &ExactTextSelector,
    ) -> Result<Option<VerifiedExtractedTextEvidence>, SearchError> {
        let expected = &selector.expected_exact_text;
        if self.selectors.get(&selector.claim_id) != Some(selector)
            || !request.need.required_claims.contains(&selector.claim_id)
            || selector.parent_resource != hit.parent_resource
            || hit.generation.source_id != self.source_id
            || hit.version.source_id != self.source_id
            || hit.version.resource_id != hit.parent_resource
        {
            return Ok(None);
        }
        // The Unit must be in the pinned, published manifest with this span.
        let Some(body) = self.runtime.published_body(hit.generation)? else {
            return Ok(None);
        };
        let Some(entry) = body.manifest.entries.iter().find(|entry| {
            entry.version == hit.version
                && entry.part == hit.part
                && entry.authoritative_representation_ref == hit.authoritative_representation_ref
                && entry.raw == hit.raw
                && entry.profile.as_ref() == Some(&hit.profile)
                && searchable(&entry.operation, entry.coverage.as_ref())
        }) else {
            return Ok(None);
        };
        let Some(pinned) = entry.units.iter().find(|unit| same_unit(unit, hit)) else {
            return Ok(None);
        };
        if text_sha256(&pinned.text) != pinned.text_sha256 || !literal_at(pinned, hit, expected) {
            return Ok(None);
        }
        // Current Version, Part binding and Read, then the same raw bytes again.
        if !self.allowed(hit.parent_resource, request).await {
            return Ok(None);
        }
        let version = DocumentVersionId::from_uuid(hit.parent_resource.as_uuid());
        let Some(record) = self.versions.load_version(version).await? else {
            return Ok(None);
        };
        if !current_live(&record) {
            return Ok(None);
        }
        let Some(item) = record
            .authoritative_items
            .iter()
            .find(|item| same_item(item, hit))
        else {
            return Ok(None);
        };
        let reread = self.extractor.extract(&record, item).await?;
        if !searchable(&reread.operation, reread.coverage.as_ref())
            || reread.profile.as_ref() != Some(&hit.profile)
        {
            return Ok(None);
        }
        let Some(rebuilt) = reread.units.iter().find(|unit| same_unit(unit, hit)) else {
            return Ok(None);
        };
        if rebuilt.text != pinned.text || !literal_at(rebuilt, hit, expected) {
            return Ok(None);
        }
        // Authority is checked once more immediately before disclosure.
        if !self.allowed(hit.parent_resource, request).await {
            return Ok(None);
        }
        match self.versions.load_version(version).await? {
            Some(again) if again == record => {}
            _ => return Ok(None),
        }
        Ok(Some(self.evidence(hit, selector, &record)))
    }

    fn evidence(
        &self,
        hit: &KnowledgeUnitHitRef,
        selector: &ExactTextSelector,
        record: &VersionSnapshotRecord,
    ) -> VerifiedExtractedTextEvidence {
        let parent = hit.parent_resource.as_uuid();
        let document = record.snapshot.document_id.as_uuid();
        let evidence_ref = format!(
            "document-body:v1:{}:{}:{parent}:{}:{}-{}",
            self.source_id.as_uuid(),
            hit.generation.generation_id.as_uuid(),
            hit.unit_id,
            hit.span.start_byte,
            hit.span.end_byte,
        );
        let mut assertion = Assertion::new(
            format!("document-version:{parent}"),
            CONTAINS_EXACT_PREDICATE,
            TypedValue::String(selector.expected_exact_text.clone()),
            self.source_id.as_uuid().to_string(),
            AssertionOrigin::Extracted,
            format!("document:{document}"),
            OffsetDateTime::now_utc(),
        );
        assertion.evidence_refs = vec![evidence_ref.clone()];
        VerifiedExtractedTextEvidence {
            assertion,
            resolved: ResolvedAssertionEvidence {
                generation: hit.generation,
                source_id: self.source_id,
                resource_id: hit.parent_resource,
                evidence_ref,
                upstream_origin: format!(
                    "document:{document}:version:{parent}:part:{}",
                    hit.part.source_native_part_id
                ),
                role: EvidenceRole::Primary,
                citation_chain: vec![
                    format!("unit:{}", hit.unit_id),
                    format!("raw:sha256:{}", hex(&hit.raw.sha256)),
                ],
                content_digest: Some(format!("sha256:{}", hex(&hit.text_sha256))),
                is_summary: false,
            },
            matched_span: hit.span,
        }
    }
}

impl ExactTextEvidencePort for DocumentExactTextEvidenceCatalog {
    fn selector_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ExactTextSelector>> {
        Box::pin(async move {
            Ok((generation.source_id == self.source_id)
                .then(|| self.selectors.get(&claim_id).cloned())
                .flatten())
        })
    }

    fn resolve_hit<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        hit: &'a KnowledgeUnitHitRef,
        selector: &'a ExactTextSelector,
    ) -> BoxFuture<'a, Option<VerifiedExtractedTextEvidence>> {
        Box::pin(self.verify(request, hit, selector))
    }
}

/// Body coverage gaps of a published generation for the caller's current Read.
pub struct DocumentBodyCoverageGaps {
    runtime: MemoryDocumentIndexRuntime,
    access: Arc<dyn CurrentAccessEvaluatorPort>,
}

impl DocumentBodyCoverageGaps {
    pub fn new(
        runtime: MemoryDocumentIndexRuntime,
        access: Arc<dyn CurrentAccessEvaluatorPort>,
    ) -> Self {
        Self { runtime, access }
    }

    async fn decide(
        &self,
        resource: ResourceId,
        request: &DiscoveryRequest,
        cache: &mut BTreeMap<ResourceId, AccessDecision>,
    ) -> AccessDecision {
        if let Some(decision) = cache.get(&resource) {
            return *decision;
        }
        let decision = self
            .access
            .evaluate(resource, &request.access_context)
            .await
            .unwrap_or(AccessDecision::Unknown);
        cache.insert(resource, decision);
        decision
    }
}

impl BodyCoverageGapPort for DocumentBodyCoverageGaps {
    fn coverage_gaps<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        generation: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Vec<InformationGap>> {
        Box::pin(async move {
            let Some(body) = self.runtime.published_body(generation)? else {
                return Ok(vec![InformationGap::new(
                    "document.body.bundle_unavailable",
                    GapReason::Availability,
                    true,
                )]);
            };
            let mut decisions = BTreeMap::new();
            let mut visible = Vec::new();
            let mut undetermined = false;
            for item in &body.coverage.items {
                let code = match (&item.operation, &item.coverage) {
                    (ItemOperationState::Completed, Some(BodyCoverage::Supported)) => continue,
                    (ItemOperationState::Completed, Some(BodyCoverage::Partial { .. })) => {
                        "partial"
                    }
                    (ItemOperationState::Completed, _) => "unsupported",
                    _ => "failed",
                };
                let resource = item.version.resource_id;
                match self.decide(resource, request, &mut decisions).await {
                    AccessDecision::Allowed => visible.push((resource, item.part.ordinal, code)),
                    // A Denied item leaves no ID, count or reason behind.
                    AccessDecision::Denied => {}
                    AccessDecision::Unknown => undetermined = true,
                }
            }
            // Re-check Read right before the gaps leave the Source.
            let mut gaps = Vec::new();
            let mut rechecked = BTreeMap::new();
            for (resource, ordinal, code) in visible {
                match self.decide(resource, request, &mut rechecked).await {
                    AccessDecision::Allowed => gaps.push(InformationGap::new(
                        format!(
                            "document.body.coverage:{}:{ordinal}:{code}",
                            resource.as_uuid()
                        ),
                        GapReason::UnsupportedCoverage,
                        true,
                    )),
                    AccessDecision::Denied => {}
                    AccessDecision::Unknown => undetermined = true,
                }
            }
            if undetermined {
                gaps.push(InformationGap::new(
                    "document.body.coverage_undetermined",
                    GapReason::UnsupportedCoverage,
                    true,
                ));
            }
            Ok(gaps)
        })
    }
}
