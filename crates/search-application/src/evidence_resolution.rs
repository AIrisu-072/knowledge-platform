//! Assemble Claim evidence only from generation-bound Assertions and resolved provenance.

use std::collections::BTreeSet;

use search_core::assertion::AssertionOrigin;
use search_core::evidence::{
    Claim, ClaimState, EvidenceReference, EvidenceRequirement, EvidenceRole, EvidenceSufficiency,
    claim_values_semantically_equal, evaluate_evidence_sufficiency,
    is_verified_direct_claim_evidence,
};
use search_core::id::{ClaimId, ResourceId};
use search_core::knowledge_unit::normalize_unit_text;
use search_core::predicate::TypedValue;
use search_core::projection::ProjectionGenerationKey;
use search_core::relation::RelationTemporalScope;

use crate::body_ports::{
    CONTAINS_EXACT_PREDICATE, ExactTextSelector, VerifiedExtractedTextEvidence,
};
use crate::error::SearchError;

use super::{AssertionStorePort, ClaimSelectorPort, EvidenceResolverPort};

/// Build resource-local claims. Callers may append claims from other resources
/// before assessing the complete requirement. Unresolved refs remain Unknown
/// unless another verified ref establishes a contradiction.
pub async fn assemble_resource_claims(
    generation: ProjectionGenerationKey,
    resource_ref: ResourceId,
    requirement: &EvidenceRequirement,
    selectors: &dyn ClaimSelectorPort,
    assertions: &dyn AssertionStorePort,
    resolver: &dyn EvidenceResolverPort,
) -> Result<Vec<Claim>, SearchError> {
    validate_requirement(requirement)?;
    let mut claims = Vec::new();
    let mut seen_claim_ids = BTreeSet::new();

    for claim_id in &requirement.required_claims {
        if !seen_claim_ids.insert(*claim_id) {
            continue;
        }
        let Some(selector) = selectors.selector_for(generation, *claim_id).await? else {
            // The Source cannot evaluate this Claim: its Resources are outside
            // the Claim's judgement, not evidence that it is unknown. Without
            // any evaluating Source the Claim still stays unresolved.
            continue;
        };
        if selector.claim_id != *claim_id
            || selector.subject_ref.trim().is_empty()
            || selector.predicate.trim().is_empty()
        {
            return Err(SearchError::OperationFailed(
                "trusted ClaimSelector did not match the required claim".into(),
            ));
        }

        let stored = assertions
            .assertions_for(generation, resource_ref, &selector.predicate)
            .await?;
        if stored.is_empty() {
            // A resource with no Assertion contributes no Claim; the required
            // Claim stays unresolved unless another resource supports it.
            continue;
        }
        let mut matched = false;
        for assertion in stored {
            if assertion.subject_ref != selector.subject_ref
                || assertion.predicate != selector.predicate
            {
                continue;
            }
            matched = true;
            let mut claim = Claim::new(*claim_id, ClaimState::Unknown);
            claim.subject = Some(selector.subject_ref.clone());
            claim.predicate = Some(selector.predicate.clone());
            claim.value = Some(assertion.value.clone());
            claim.temporal_scope = RelationTemporalScope {
                valid_from: assertion.effective_from,
                valid_to: assertion.effective_to,
            };

            let mut unresolved = assertion.evidence_refs.is_empty();
            let mut has_verified_direct_evidence = false;
            let mut has_verified_contradiction = false;
            let mut seen_refs = BTreeSet::new();
            for opaque_ref in &assertion.evidence_refs {
                if !seen_refs.insert(opaque_ref) {
                    continue;
                }
                if opaque_ref.trim().is_empty() {
                    unresolved = true;
                    continue;
                }
                let Some(record) = resolver
                    .resolve(generation, resource_ref, opaque_ref)
                    .await?
                else {
                    unresolved = true;
                    continue;
                };
                if record.generation != generation
                    || record.source_id != generation.source_id
                    || record.resource_id != resource_ref
                    || record.evidence_ref != *opaque_ref
                {
                    unresolved = true;
                    continue;
                }
                let is_direct_evidence = is_verified_direct_claim_evidence(
                    record.role,
                    record.is_summary,
                    &record.upstream_origin,
                );
                if matches!(
                    record.role,
                    EvidenceRole::Primary | EvidenceRole::Contradicting
                ) && !is_direct_evidence
                {
                    unresolved = true;
                }
                has_verified_direct_evidence |= is_direct_evidence;
                has_verified_contradiction |=
                    is_direct_evidence && record.role == EvidenceRole::Contradicting;
                let mut evidence =
                    EvidenceReference::new(record.source_id, record.upstream_origin, record.role);
                evidence.evidence_ref = Some(record.evidence_ref);
                evidence.citation_chain = record.citation_chain;
                evidence.content_digest = record.content_digest;
                evidence.is_summary = record.is_summary;
                claim.evidence_refs.push(evidence);
            }

            claim.state = if has_verified_contradiction
                || (has_verified_direct_evidence
                    && selector.expected_value.as_ref().is_some_and(|expected| {
                        !claim_values_semantically_equal(expected, &assertion.value)
                    })) {
                // Keep verified contradictions and their provenance even if another ref is missing.
                ClaimState::Conflicted
            } else if unresolved || !has_verified_direct_evidence {
                ClaimState::Unknown
            } else {
                ClaimState::Supported
            };
            claims.push(claim);
        }
        // Assertions about another subject say nothing about this Claim; the
        // Resource is outside its judgement rather than evidence of Unknown.
        let _ = matched;
    }
    Ok(claims)
}

/// The only route from a body Unit to a Claim. The trusted selector for
/// `required` and the Source-verified span must agree on the Claim, parent,
/// predicate and normalized literal; anything else stays `Unknown`. A generic
/// body hit, a similarity score or a title Assertion never reaches here.
pub fn assemble_verified_unit_text_claim(
    required: ClaimId,
    selector: &ExactTextSelector,
    verified: &VerifiedExtractedTextEvidence,
) -> Claim {
    let expected = &selector.expected_exact_text;
    let assertion = &verified.assertion;
    let resolved = &verified.resolved;
    let mut claim = Claim::new(required, ClaimState::Unknown);
    claim.subject = Some(assertion.subject_ref.clone());
    claim.predicate = Some(selector.predicate.clone());
    claim.value = Some(TypedValue::String(expected.clone()));
    let span_len = verified
        .matched_span
        .end_byte
        .checked_sub(verified.matched_span.start_byte)
        .map(|len| len as usize);
    let bound = selector.claim_id == required
        && selector.predicate == CONTAINS_EXACT_PREDICATE
        && !expected.is_empty()
        && normalize_unit_text(expected) == *expected
        && span_len == Some(expected.len())
        && assertion.origin == AssertionOrigin::Extracted
        && !assertion.subject_ref.trim().is_empty()
        && assertion.predicate == selector.predicate
        && assertion.value == TypedValue::String(expected.clone())
        && assertion.evidence_refs == [resolved.evidence_ref.clone()]
        && resolved.resource_id == selector.parent_resource
        && resolved.source_id == resolved.generation.source_id
        && resolved.role == EvidenceRole::Primary
        && resolved.content_digest.is_some()
        && is_verified_direct_claim_evidence(
            resolved.role,
            resolved.is_summary,
            &resolved.upstream_origin,
        );
    if !bound {
        return claim;
    }
    let mut evidence = EvidenceReference::new(
        resolved.source_id,
        resolved.upstream_origin.clone(),
        EvidenceRole::Primary,
    );
    evidence.evidence_ref = Some(resolved.evidence_ref.clone());
    evidence.citation_chain = resolved.citation_chain.clone();
    evidence.content_digest = resolved.content_digest.clone();
    evidence.is_summary = false;
    claim.evidence_refs.push(evidence);
    claim.state = ClaimState::Supported;
    claim
}

/// C8 completion gate. The core evaluator owns sufficiency semantics.
pub fn assess_claim_evidence(
    requirement: &EvidenceRequirement,
    claims: &[Claim],
) -> Result<EvidenceSufficiency, SearchError> {
    validate_requirement(requirement)?;
    Ok(evaluate_evidence_sufficiency(requirement, claims))
}

fn validate_requirement(requirement: &EvidenceRequirement) -> Result<(), SearchError> {
    if requirement.required_claims.is_empty() {
        return Err(SearchError::InvalidRequest(
            "evidence completion requires at least one required claim".into(),
        ));
    }
    Ok(())
}
