use search_application::candidate::{
    CandidateHardGates, HardGateEvaluation, RankedCandidateHit, RetrieverRankList,
};
use search_application::federation::{CandidateFederator, FederationError, FusionStrategy};
use search_core::applicability::{ApplicabilityEvaluation, ApplicabilityState};
use search_core::discovery::{
    CandidateIdentityClass, FederatedCandidate, GapReason, InformationGap,
};
use search_core::id::{LogicalResourceId, ProjectionGenerationId, ResourceId, SourceId};
use search_core::identity::{IdentityEvidence, IdentityEvidenceKind};
use search_core::projection::ProjectionGenerationKey;
use uuid::Uuid;

fn source(value: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(value))
}

fn resource(value: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(value))
}

fn logical(value: u128) -> LogicalResourceId {
    LogicalResourceId::from_uuid(Uuid::from_u128(value))
}

fn generation(source_id: SourceId, value: u128) -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(value)),
    }
}

fn applicable() -> ApplicabilityEvaluation {
    ApplicabilityEvaluation {
        state: ApplicabilityState::Applicable,
        matched_conditions: vec!["intent".into()],
        resolved_discriminators: vec![],
        remaining_nonblocking_unknowns: vec![],
        gaps: vec![],
        reasons: vec![],
    }
}

fn gate(state: ApplicabilityState, reason: &str) -> HardGateEvaluation {
    HardGateEvaluation {
        state,
        reasons: if reason.is_empty() {
            vec![]
        } else {
            vec![reason.into()]
        },
        gaps: vec![],
    }
}

fn candidate(id: &str, source_id: SourceId, method: &str) -> FederatedCandidate {
    let mut candidate = FederatedCandidate::new(
        id,
        CandidateIdentityClass::DurableResource,
        source_id,
        method,
    );
    candidate.locator = Some(format!("{method}://{id}"));
    candidate.retrieval_trace_ref = Some(format!("trace:{method}:{id}"));
    candidate
}

fn hit(candidate: FederatedCandidate, raw_score: Option<f64>) -> RankedCandidateHit {
    RankedCandidateHit {
        candidate,
        hard_gates: CandidateHardGates {
            applicability: applicable(),
            structured: gate(ApplicabilityState::Applicable, ""),
            access: gate(ApplicabilityState::Applicable, ""),
            temporal: gate(ApplicabilityState::Applicable, ""),
        },
        identity_evidence: vec![],
        raw_score,
        evidence_refs: vec![],
    }
}

fn identity_hit(
    id: &str,
    method: &str,
    logical_id: u128,
    supports_same_resource: bool,
) -> RankedCandidateHit {
    let mut candidate = candidate(id, source(1), method);
    candidate.resource_ref = Some(resource(10));
    candidate.logical_resource_ref = Some(logical(logical_id));
    let mut hit = hit(candidate, None);
    hit.identity_evidence = vec![IdentityEvidence::new(
        IdentityEvidenceKind::ExplicitDeclaration,
        supports_same_resource,
    )];
    hit
}

fn list(
    retriever_id: &str,
    source_id: SourceId,
    hits: Vec<RankedCandidateHit>,
) -> RetrieverRankList {
    list_at(retriever_id, generation(source_id, 1), hits)
}

fn list_at(
    retriever_id: &str,
    generation: ProjectionGenerationKey,
    hits: Vec<RankedCandidateHit>,
) -> RetrieverRankList {
    RetrieverRankList {
        retriever_id: retriever_id.into(),
        generation,
        hits,
    }
}

#[test]
fn list_source_mismatch_is_rejected_before_fusion() {
    let wrong_source = hit(candidate("wrong-source", source(2), "lexical"), None);
    let error = CandidateFederator::merge(
        &[list_at(
            "lexical",
            generation(source(1), 10),
            vec![wrong_source],
        )],
        FusionStrategy::PriorityConcat,
    )
    .unwrap_err();
    assert_eq!(
        error,
        FederationError::HitSourceMismatch {
            retriever_id: "lexical".into(),
            list_source: source(1),
            hit_source: source(2),
        }
    );
}

#[test]
fn one_source_cannot_mix_generation_n_and_n_plus_one() {
    let error = CandidateFederator::merge(
        &[
            list_at(
                "lexical",
                generation(source(1), 10),
                vec![hit(candidate("old", source(1), "lexical"), None)],
            ),
            list_at(
                "graph",
                generation(source(1), 11),
                vec![hit(candidate("new", source(1), "graph"), None)],
            ),
        ],
        FusionStrategy::PriorityConcat,
    )
    .unwrap_err();
    assert_eq!(
        error,
        FederationError::MixedSourceGenerations {
            source_id: source(1),
            first: generation(source(1), 10).generation_id,
            second: generation(source(1), 11).generation_id,
        }
    );
}

#[test]
fn validated_generation_stays_in_hit_trace() {
    let key = generation(source(1), 10);
    let result = CandidateFederator::merge(
        &[list_at(
            "lexical",
            key,
            vec![hit(candidate("kept", source(1), "lexical"), None)],
        )],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");
    assert_eq!(result.ranked[0].hits[0].trace.generation, key);
}

#[test]
fn priority_concat_keeps_route_order_graph_only_hit_and_all_duplicate_traces() {
    let mut lexical_a = candidate("lexical-a", source(1), "lexical");
    lexical_a.resource_ref = Some(resource(10));
    let mut lexical_c = candidate("lexical-c", source(1), "lexical");
    lexical_c.resource_ref = Some(resource(30));
    let mut graph_a = candidate("graph-a", source(1), "graph");
    graph_a.resource_ref = Some(resource(10));
    let mut graph_b = candidate("graph-b", source(1), "graph");
    graph_b.resource_ref = Some(resource(20));
    let mut graph_a_hit = hit(graph_a, Some(-20.0));
    graph_a_hit.evidence_refs = vec!["relation:42".into()];

    let result = CandidateFederator::merge(
        &[
            list(
                "lexical",
                source(1),
                vec![hit(lexical_a, Some(0.01)), hit(lexical_c, Some(500.0))],
            ),
            list(
                "graph",
                source(1),
                vec![hit(graph_b, Some(1000.0)), graph_a_hit],
            ),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    let ids = result
        .ranked
        .iter()
        .map(|group| group.hits[0].candidate.resource_ref)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![Some(resource(10)), Some(resource(30)), Some(resource(20))]
    );
    assert_eq!(result.ranked[0].hits.len(), 2);
    assert_eq!(result.ranked[0].hits[0].trace.retriever_id, "lexical");
    assert_eq!(result.ranked[0].hits[0].trace.rank, 1);
    assert_eq!(result.ranked[0].hits[0].trace.raw_score, Some(0.01));
    assert_eq!(result.ranked[0].hits[1].trace.retriever_id, "graph");
    assert_eq!(result.ranked[0].hits[1].trace.rank, 2);
    assert_eq!(result.ranked[0].hits[1].trace.raw_score, Some(-20.0));
    assert_eq!(
        result.ranked[0].hits[1].trace.evidence_refs,
        vec!["relation:42"]
    );
    assert_eq!(
        result.ranked[0].hits[1]
            .candidate
            .retrieval_trace_ref
            .as_deref(),
        Some("trace:graph:graph-a")
    );
}

#[test]
fn cross_source_logical_group_requires_resolved_identity_for_each_representation() {
    let mut mcp = candidate("mcp-ref", source(1), "mcp");
    mcp.resource_ref = Some(resource(10));
    mcp.logical_resource_ref = Some(logical(40));
    let mut rest = candidate("rest-ref", source(2), "rest");
    rest.resource_ref = Some(resource(20));
    rest.logical_resource_ref = Some(logical(40));

    let mut mcp_hit = hit(mcp.clone(), None);
    mcp_hit.identity_evidence = vec![IdentityEvidence::new(
        IdentityEvidenceKind::ExplicitDeclaration,
        true,
    )];
    mcp_hit.evidence_refs = vec!["declaration:mcp".into()];
    let mut rest_hit = hit(rest.clone(), None);
    rest_hit.identity_evidence = vec![IdentityEvidence::new(
        IdentityEvidenceKind::StableProviderId,
        true,
    )];
    rest_hit.evidence_refs = vec!["provider:rest".into()];

    let resolved = CandidateFederator::merge(
        &[
            list("mcp", source(1), vec![mcp_hit.clone()]),
            list("rest", source(2), vec![rest_hit.clone()]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");
    assert_eq!(resolved.ranked.len(), 1);
    assert_eq!(resolved.ranked[0].logical_resource_ref, Some(logical(40)));
    assert_eq!(resolved.ranked[0].hits.len(), 2);
    assert_eq!(
        resolved.ranked[0].hits[0].candidate.locator.as_deref(),
        Some("mcp://mcp-ref")
    );
    assert_eq!(
        resolved.ranked[0].hits[1].candidate.locator.as_deref(),
        Some("rest://rest-ref")
    );
    assert_eq!(
        resolved.ranked[0].hits[1].trace.evidence_refs,
        vec!["provider:rest"]
    );

    rest_hit.identity_evidence = vec![IdentityEvidence::new(
        IdentityEvidenceKind::NameSimilarity,
        true,
    )];
    let weak = CandidateFederator::merge(
        &[
            list("mcp", source(1), vec![mcp_hit]),
            list("rest", source(2), vec![rest_hit]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");
    assert_eq!(weak.ranked.len(), 2);
    assert_eq!(weak.ranked[0].logical_resource_ref, Some(logical(40)));
    assert_eq!(weak.ranked[1].logical_resource_ref, None);
}

#[test]
fn conflicting_identity_on_same_local_resource_prevents_logical_promotion() {
    let mut conflict = candidate("conflict", source(1), "lexical");
    conflict.resource_ref = Some(resource(10));
    conflict.logical_resource_ref = Some(logical(40));
    let mut positive = candidate("positive", source(1), "graph");
    positive.resource_ref = Some(resource(10));
    positive.logical_resource_ref = Some(logical(40));

    let mut conflict_hit = hit(conflict, None);
    conflict_hit.identity_evidence = vec![
        IdentityEvidence::new(IdentityEvidenceKind::ExplicitDeclaration, true),
        IdentityEvidence::new(IdentityEvidenceKind::CanonicalUri, false),
    ];
    let mut positive_hit = hit(positive, None);
    positive_hit.identity_evidence = vec![IdentityEvidence::new(
        IdentityEvidenceKind::StableProviderId,
        true,
    )];

    let result = CandidateFederator::merge(
        &[
            list("lexical", source(1), vec![conflict_hit]),
            list("graph", source(1), vec![positive_hit]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");
    assert_eq!(result.ranked.len(), 1);
    assert_eq!(result.ranked[0].logical_resource_ref, None);
    assert_eq!(result.ranked[0].hits.len(), 2);
    assert_eq!(result.ranked[0].hits[0].candidate.candidate_id, "conflict");
    assert_eq!(result.ranked[0].hits[1].candidate.candidate_id, "positive");
}

#[test]
fn identity_conflict_outside_ranked_hits_still_blocks_local_logical_promotion() {
    for gate_state in [ApplicabilityState::Unresolved, ApplicabilityState::Invalid] {
        let mut conflict = candidate("conflict", source(1), "lexical");
        conflict.resource_ref = Some(resource(10));
        conflict.logical_resource_ref = Some(logical(40));
        let mut positive = candidate("positive", source(1), "graph");
        positive.resource_ref = Some(resource(10));
        positive.logical_resource_ref = Some(logical(40));

        let mut conflict_hit = hit(conflict, None);
        conflict_hit.identity_evidence = vec![
            IdentityEvidence::new(IdentityEvidenceKind::ExplicitDeclaration, true),
            IdentityEvidence::new(IdentityEvidenceKind::CanonicalUri, false),
        ];
        conflict_hit.hard_gates.access = gate(gate_state, "access undecided or invalid");
        let mut positive_hit = hit(positive, None);
        positive_hit.identity_evidence = vec![IdentityEvidence::new(
            IdentityEvidenceKind::StableProviderId,
            true,
        )];

        let result = CandidateFederator::merge(
            &[
                list("lexical", source(1), vec![conflict_hit]),
                list("graph", source(1), vec![positive_hit]),
            ],
            FusionStrategy::PriorityConcat,
        )
        .expect("valid source and generation");
        assert_eq!(result.ranked.len(), 1);
        assert_eq!(result.ranked[0].logical_resource_ref, None);
    }
}

#[test]
fn eligible_a_and_pending_b_conflict_without_mixing_hard_gate_buckets() {
    let eligible_a = identity_hit("eligible-a", "lexical", 40, true);
    let mut pending_b = identity_hit("pending-b", "graph", 50, true);
    pending_b.hard_gates.access = gate(ApplicabilityState::Unresolved, "access unknown");

    let result = CandidateFederator::merge(
        &[
            list("lexical", source(1), vec![eligible_a]),
            list("graph", source(1), vec![pending_b]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert_eq!(result.ranked.len(), 1);
    assert_eq!(result.ranked[0].logical_resource_ref, None);
    assert_eq!(result.ranked[0].hits[0].trace.retriever_id, "lexical");
    assert_eq!(result.pending.len(), 1);
    assert_eq!(result.pending[0].logical_resource_ref, None);
    assert_eq!(result.pending[0].hits[0].hit.trace.retriever_id, "graph");
    assert_eq!(
        result.pending[0].hits[0].gaps[0].required_fact,
        "current_access"
    );
    assert!(result.rejected.is_empty());
}

#[test]
fn eligible_a_and_rejected_b_conflict_without_promoting_rejected_hit() {
    let eligible_a = identity_hit("eligible-a", "lexical", 40, true);
    let mut rejected_b = identity_hit("rejected-b", "graph", 50, true);
    rejected_b.hard_gates.structured = gate(ApplicabilityState::Excluded, "facet mismatch");

    let result = CandidateFederator::merge(
        &[
            list("lexical", source(1), vec![eligible_a]),
            list("graph", source(1), vec![rejected_b]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert_eq!(result.ranked.len(), 1);
    assert_eq!(result.ranked[0].logical_resource_ref, None);
    assert!(result.pending.is_empty());
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(
        result.rejected[0].rejection.state,
        ApplicabilityState::Excluded
    );
    assert_eq!(result.rejected[0].hit.candidate.candidate_id, "rejected-b");
    assert_eq!(result.rejected[0].hit.trace.retriever_id, "graph");
}

#[test]
fn pending_a_and_rejected_b_conflict_without_mixing_hard_gate_buckets() {
    let mut pending_a = identity_hit("pending-a", "lexical", 40, true);
    pending_a.hard_gates.access = gate(ApplicabilityState::Unresolved, "access unknown");
    let mut rejected_b = identity_hit("rejected-b", "graph", 50, true);
    rejected_b.hard_gates.access = gate(ApplicabilityState::Invalid, "access invalid");

    let result = CandidateFederator::merge(
        &[
            list("lexical", source(1), vec![pending_a]),
            list("graph", source(1), vec![rejected_b]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert!(result.ranked.is_empty());
    assert_eq!(result.pending.len(), 1);
    assert_eq!(result.pending[0].logical_resource_ref, None);
    assert_eq!(
        result.pending[0].hits[0].gaps[0].required_fact,
        "current_access"
    );
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(
        result.rejected[0].rejection.state,
        ApplicabilityState::Invalid
    );
    assert_eq!(result.rejected[0].hit.candidate.candidate_id, "rejected-b");
}

#[test]
fn positive_a_and_strong_negative_a_in_ranked_hits_conflict() {
    let positive_a = identity_hit("positive-a", "lexical", 40, true);
    let negative_a = identity_hit("negative-a", "graph", 40, false);

    let result = CandidateFederator::merge(
        &[
            list("lexical", source(1), vec![positive_a]),
            list("graph", source(1), vec![negative_a]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert_eq!(result.ranked.len(), 1);
    assert_eq!(result.ranked[0].logical_resource_ref, None);
    assert_eq!(result.ranked[0].hits.len(), 2);
    assert_eq!(result.ranked[0].hits[0].trace.retriever_id, "lexical");
    assert_eq!(result.ranked[0].hits[1].trace.retriever_id, "graph");
}

#[test]
fn same_source_id_or_name_without_identity_does_not_group_ephemeral_candidates() {
    let mut first = candidate("same-name", source(1), "lexical");
    first.identity_class = CandidateIdentityClass::EphemeralCandidate;
    let mut second = candidate("same-name", source(1), "graph");
    second.identity_class = CandidateIdentityClass::EphemeralCandidate;
    let result = CandidateFederator::merge(
        &[
            list("lexical", source(1), vec![hit(first, None)]),
            list("graph", source(1), vec![hit(second, None)]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");
    assert_eq!(result.ranked.len(), 2);
}

#[test]
fn hard_gates_remove_excluded_and_invalid_hits_and_route_unknown_to_probe() {
    let mut unknown_applicability =
        hit(candidate("missing-fact", source(1), "lexical"), Some(100.0));
    unknown_applicability.hard_gates.applicability.state = ApplicabilityState::Unresolved;
    unknown_applicability.hard_gates.applicability.gaps = vec![InformationGap::new(
        "requires_approval",
        GapReason::MissingFact,
        true,
    )];
    unknown_applicability.hard_gates.applicability.reasons = vec!["hard condition unknown".into()];

    let mut structured_mismatch = hit(candidate("wrong-facet", source(1), "lexical"), Some(200.0));
    structured_mismatch.hard_gates.structured =
        gate(ApplicabilityState::Excluded, "facet mismatch");

    let mut access_unknown = hit(
        candidate("access-unknown", source(1), "lexical"),
        Some(300.0),
    );
    access_unknown.hard_gates.access =
        gate(ApplicabilityState::Unresolved, "current access unknown");
    access_unknown.hard_gates.access.gaps = vec![InformationGap::new(
        "current_access",
        GapReason::Availability,
        true,
    )];

    let mut access_invalid = hit(
        candidate("access-invalid", source(1), "lexical"),
        Some(400.0),
    );
    access_invalid.hard_gates.access =
        gate(ApplicabilityState::Invalid, "access evaluation failed");

    let mut eligible = hit(candidate("eligible", source(1), "lexical"), Some(-100.0));
    eligible.candidate.resource_ref = Some(resource(90));

    let result = CandidateFederator::merge(
        &[list(
            "lexical",
            source(1),
            vec![
                unknown_applicability,
                structured_mismatch,
                access_unknown,
                access_invalid,
                eligible,
            ],
        )],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");
    assert_eq!(result.ranked.len(), 1);
    assert_eq!(result.ranked[0].hits[0].candidate.candidate_id, "eligible");
    assert_eq!(result.ranked[0].hits[0].trace.rank, 5);
    assert_eq!(result.pending.len(), 2);
    assert_eq!(
        result.pending[0].hits[0].hit.candidate.candidate_id,
        "missing-fact"
    );
    assert_eq!(
        result.pending[0].hits[0].gaps[0].required_fact,
        "requires_approval"
    );
    assert_eq!(
        result.pending[1].hits[0].hit.candidate.candidate_id,
        "access-unknown"
    );
    assert_eq!(
        result.pending[1].hits[0].gaps[0].required_fact,
        "current_access"
    );
    assert_eq!(result.rejected.len(), 2);
    assert_eq!(result.rejected[0].rejection.candidate_id, "wrong-facet");
    assert_eq!(
        result.rejected[0].rejection.state,
        ApplicabilityState::Excluded
    );
    assert_eq!(result.rejected[0].hit.trace.rank, 2);
    assert_eq!(
        result.rejected[0]
            .hit
            .candidate
            .retrieval_trace_ref
            .as_deref(),
        Some("trace:lexical:wrong-facet")
    );
    assert!(
        result.rejected[0]
            .rejection
            .reason_trace
            .contains(&"structured: facet mismatch".into())
    );
    assert_eq!(result.rejected[1].rejection.candidate_id, "access-invalid");
    assert_eq!(
        result.rejected[1].rejection.state,
        ApplicabilityState::Invalid
    );
}

#[test]
fn rejected_hits_with_the_same_candidate_id_keep_source_and_original_trace() {
    let mut first = hit(candidate("same-id", source(1), "lexical"), Some(0.25));
    first.hard_gates.access = gate(ApplicabilityState::Invalid, "access failed");
    first.evidence_refs = vec!["first:evidence".into()];

    let mut second = hit(candidate("same-id", source(2), "graph"), Some(-7.0));
    second.hard_gates.structured = gate(ApplicabilityState::Excluded, "facet mismatch");
    second.evidence_refs = vec!["second:evidence".into()];

    let result = CandidateFederator::merge(
        &[
            list("lexical", source(1), vec![first]),
            list("graph", source(2), vec![second]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");
    assert_eq!(result.rejected.len(), 2);
    assert_eq!(result.rejected[0].rejection.candidate_id, "same-id");
    assert_eq!(result.rejected[1].rejection.candidate_id, "same-id");
    assert_eq!(result.rejected[0].hit.candidate.source_ref, source(1));
    assert_eq!(result.rejected[1].hit.candidate.source_ref, source(2));
    assert_eq!(result.rejected[0].hit.trace.retriever_id, "lexical");
    assert_eq!(result.rejected[0].hit.trace.rank, 1);
    assert_eq!(result.rejected[0].hit.trace.raw_score, Some(0.25));
    assert_eq!(
        result.rejected[0].hit.trace.evidence_refs,
        vec!["first:evidence"]
    );
    assert_eq!(
        result.rejected[0]
            .hit
            .candidate
            .retrieval_trace_ref
            .as_deref(),
        Some("trace:lexical:same-id")
    );
    assert_eq!(result.rejected[1].hit.trace.retriever_id, "graph");
    assert_eq!(result.rejected[1].hit.trace.raw_score, Some(-7.0));
    assert_eq!(
        result.rejected[1].hit.trace.evidence_refs,
        vec!["second:evidence"]
    );
}

#[test]
fn unresolved_access_without_a_supplied_gap_still_exposes_a_blocking_gap() {
    let mut unknown = hit(candidate("access-undetermined", source(1), "graph"), None);
    unknown.hard_gates.access = gate(
        ApplicabilityState::Unresolved,
        "current decision unavailable",
    );

    let result = CandidateFederator::merge(
        &[list("graph", source(1), vec![unknown])],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert!(result.ranked.is_empty());
    assert_eq!(result.pending.len(), 1);
    assert_eq!(result.pending[0].hits[0].gaps.len(), 1);
    assert_eq!(
        result.pending[0].hits[0].gaps[0].required_fact,
        "current_access"
    );
    assert!(result.pending[0].hits[0].gaps[0].blocking);
}

#[test]
fn duplicate_unresolved_hits_group_before_probe_without_losing_each_gap_or_rank() {
    let mut lexical = candidate("lexical-ref", source(1), "lexical");
    lexical.resource_ref = Some(resource(10));
    let mut graph = candidate("graph-ref", source(1), "graph");
    graph.resource_ref = Some(resource(10));

    let mut lexical_hit = hit(lexical, None);
    lexical_hit.hard_gates.structured = gate(ApplicabilityState::Unresolved, "facet unknown");
    lexical_hit.hard_gates.structured.gaps = vec![InformationGap::new(
        "required_facet",
        GapReason::MissingFact,
        true,
    )];
    let mut graph_hit = hit(graph, None);
    graph_hit.hard_gates.access = gate(ApplicabilityState::Unresolved, "access unknown");
    graph_hit.hard_gates.access.gaps = vec![InformationGap::new(
        "current_access",
        GapReason::Availability,
        true,
    )];

    let result = CandidateFederator::merge(
        &[
            list("lexical", source(1), vec![lexical_hit]),
            list("graph", source(1), vec![graph_hit]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert!(result.ranked.is_empty());
    assert_eq!(result.pending.len(), 1);
    assert_eq!(result.pending[0].hits.len(), 2);
    assert_eq!(
        result.pending[0].hits[0].gaps[0].required_fact,
        "required_facet"
    );
    assert_eq!(
        result.pending[0].hits[1].gaps[0].required_fact,
        "current_access"
    );
    assert_eq!(result.pending[0].hits[0].hit.trace.retriever_id, "lexical");
    assert_eq!(result.pending[0].hits[1].hit.trace.retriever_id, "graph");
}

#[test]
fn known_temporal_period_exclusion_is_rejected_before_priority_concat() {
    let mut out_of_period = hit(
        candidate("out-of-period", source(1), "lexical"),
        Some(1000.0),
    );
    out_of_period.hard_gates.temporal = gate(
        ApplicabilityState::Excluded,
        "effective period excludes the as-of target",
    );
    let current = hit(candidate("current", source(1), "lexical"), Some(-10.0));
    let graph = hit(candidate("graph-only", source(1), "graph"), Some(20.0));

    let result = CandidateFederator::merge(
        &[
            list("lexical", source(1), vec![out_of_period, current]),
            list("graph", source(1), vec![graph]),
        ],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert_eq!(result.ranked.len(), 2);
    assert_eq!(result.ranked[0].hits[0].candidate.candidate_id, "current");
    assert_eq!(result.ranked[0].hits[0].trace.rank, 2);
    assert_eq!(
        result.ranked[1].hits[0].candidate.candidate_id,
        "graph-only"
    );
    assert!(result.pending.is_empty());
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(result.rejected[0].rejection.candidate_id, "out-of-period");
    assert_eq!(
        result.rejected[0].rejection.state,
        ApplicabilityState::Excluded
    );
    assert_eq!(
        result.rejected[0].rejection.reason_trace,
        vec!["temporal: effective period excludes the as-of target"]
    );
    assert_eq!(result.rejected[0].hit.trace.raw_score, Some(1000.0));
}

#[test]
fn unknown_required_freshness_remains_pending_with_its_gap_and_reason() {
    let mut freshness_unknown = hit(
        candidate("freshness-unknown", source(1), "lexical"),
        Some(1000.0),
    );
    freshness_unknown.hard_gates.temporal = gate(
        ApplicabilityState::Unresolved,
        "required freshness cannot be established",
    );
    let mut freshness_gap = InformationGap::new("required_freshness", GapReason::Freshness, false);
    freshness_gap.acceptable_evidence = vec!["recent_source_observation".into()];
    freshness_unknown.hard_gates.temporal.gaps = vec![freshness_gap];
    let current = hit(candidate("current", source(1), "lexical"), Some(-10.0));

    let result = CandidateFederator::merge(
        &[list("lexical", source(1), vec![freshness_unknown, current])],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert_eq!(result.ranked.len(), 1);
    assert_eq!(result.ranked[0].hits[0].candidate.candidate_id, "current");
    assert_eq!(result.pending.len(), 1);
    assert_eq!(result.pending[0].hits[0].hit.trace.rank, 1);
    assert_eq!(result.pending[0].hits[0].gaps.len(), 1);
    assert_eq!(
        result.pending[0].hits[0].gaps[0].required_fact,
        "required_freshness"
    );
    assert_eq!(
        result.pending[0].hits[0].gaps[0].reason,
        GapReason::Freshness
    );
    assert!(result.pending[0].hits[0].gaps[0].blocking);
    assert_eq!(
        result.pending[0].hits[0].gaps[0].acceptable_evidence,
        vec!["recent_source_observation"]
    );
    assert!(!result.pending[0].hits[0].hit.hard_gates.temporal.gaps[0].blocking);
    assert_eq!(
        result.pending[0].hits[0].reason_trace,
        vec!["temporal: required freshness cannot be established"]
    );
    assert!(result.rejected.is_empty());
}

#[test]
fn unknown_required_period_normalizes_supplied_gap_without_promoting_optional_gap() {
    let mut period_unknown = hit(
        candidate("period-unknown", source(1), "lexical"),
        Some(1000.0),
    );
    period_unknown.hard_gates.structured.gaps = vec![InformationGap::new(
        "optional_display_label",
        GapReason::MissingFact,
        false,
    )];
    period_unknown.hard_gates.temporal = gate(
        ApplicabilityState::Unresolved,
        "effective period cannot be established",
    );
    let mut period_gap = InformationGap::new("effective_period", GapReason::MissingFact, false);
    period_gap.acceptable_evidence = vec!["effective_from".into(), "effective_to".into()];
    period_unknown.hard_gates.temporal.gaps = vec![period_gap];
    let current = hit(candidate("current", source(1), "lexical"), Some(-10.0));

    let result = CandidateFederator::merge(
        &[list("lexical", source(1), vec![period_unknown, current])],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert_eq!(result.ranked.len(), 1);
    assert_eq!(result.ranked[0].hits[0].candidate.candidate_id, "current");
    assert_eq!(result.ranked[0].hits[0].trace.rank, 2);
    assert_eq!(result.pending.len(), 1);
    let pending = &result.pending[0].hits[0];
    assert_eq!(pending.hit.candidate.candidate_id, "period-unknown");
    assert_eq!(pending.hit.trace.rank, 1);
    assert_eq!(pending.gaps.len(), 2);
    assert_eq!(pending.gaps[0].required_fact, "optional_display_label");
    assert!(!pending.gaps[0].blocking);
    assert_eq!(pending.gaps[1].required_fact, "effective_period");
    assert_eq!(pending.gaps[1].reason, GapReason::MissingFact);
    assert!(pending.gaps[1].blocking);
    assert_eq!(
        pending.gaps[1].acceptable_evidence,
        vec!["effective_from", "effective_to"]
    );
    assert!(result.rejected.is_empty());
}

#[test]
fn unresolved_temporal_gate_without_supplied_gap_gets_a_blocking_gap() {
    let mut unknown = hit(candidate("temporal-unknown", source(1), "graph"), None);
    unknown.hard_gates.temporal = gate(ApplicabilityState::Unresolved, "");

    let result = CandidateFederator::merge(
        &[list("graph", source(1), vec![unknown])],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert!(result.ranked.is_empty());
    assert_eq!(result.pending.len(), 1);
    assert_eq!(result.pending[0].hits[0].gaps.len(), 1);
    assert_eq!(
        result.pending[0].hits[0].gaps[0].required_fact,
        "temporal_hard_gate"
    );
    assert_eq!(
        result.pending[0].hits[0].gaps[0].reason,
        GapReason::MissingFact
    );
    assert!(result.pending[0].hits[0].gaps[0].blocking);
    assert_eq!(
        result.pending[0].hits[0].reason_trace,
        vec!["temporal: Unresolved"]
    );
}

#[test]
fn invalid_temporal_gate_is_rejected_even_when_another_gate_is_unresolved() {
    let mut invalid = hit(candidate("invalid-time", source(1), "lexical"), None);
    invalid.hard_gates.temporal = gate(ApplicabilityState::Invalid, "invalid period bounds");
    invalid.hard_gates.structured = gate(ApplicabilityState::Unresolved, "facet unknown");

    let result = CandidateFederator::merge(
        &[list("lexical", source(1), vec![invalid])],
        FusionStrategy::PriorityConcat,
    )
    .expect("valid source and generation");

    assert!(result.ranked.is_empty());
    assert!(result.pending.is_empty());
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(
        result.rejected[0].rejection.state,
        ApplicabilityState::Invalid
    );
    assert_eq!(
        result.rejected[0].rejection.reason_trace,
        vec![
            "structured: facet unknown",
            "temporal: invalid period bounds"
        ]
    );
}
