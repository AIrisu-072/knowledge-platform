use search_application::retrieval_execution::RawRetrievalHit;
use search_application::routing::{
    RouteStage, RouteState, SourceRole, SourceRoute, SourceRoutePlan,
};
use search_core::applicability::ApplicabilityState;
use search_core::discovery::{
    CandidateIdentityClass, DiscoveryNeed, DiscoveryResult, FederatedCandidate, GapReason,
    InformationGap, QualifiedResource,
};
use search_core::evidence::{
    Claim, ClaimState, EvidenceReference, EvidenceRequirement, EvidenceRole, EvidenceSufficiency,
};
use search_core::graph::{GraphPathEvidence, GraphPathStepEvidence};
use search_core::id::{
    ClaimId, DiscoveryEvaluationId, NeedId, ProjectionGenerationId, RelationId, ResourceId,
    SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::projection::ProjectionGenerationKey;
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use search_core::source::{DiscoveryMode, EnumerationSemantics};
use search_evaluation::report::{EvaluationReport, FailureClass};
use search_evaluation::scenario::{
    EvaluationObservation, EvaluationScenario, EvidenceLocator, ObservedClaim, ScenarioCategory,
    SourceKnowledgeTruth, evaluate_scenario,
};
use uuid::Uuid;

fn source(id: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(id))
}

fn resource(id: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(id))
}

fn relation(id: u128) -> RelationId {
    RelationId::from_uuid(Uuid::from_u128(id))
}

fn claim(id: u128) -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(id))
}

fn present(category: ScenarioCategory) -> EvaluationScenario {
    let mut scenario = EvaluationScenario::new(category, SourceKnowledgeTruth::Present);
    scenario.required_sources.push(source(1));
    scenario.relevant_resources.push(resource(10));
    scenario
}

fn observation() -> EvaluationObservation {
    EvaluationObservation::new(EvidenceSufficiency::Unresolved)
}

fn valid_relation() -> TypedRelationInstance {
    TypedRelationInstance::new(
        relation(100),
        RelationNamespace::Discovery,
        "eligible_for",
        vec![
            RelationParticipant::new("resource", resource(10)),
            RelationParticipant::new("purpose", resource(20)),
            RelationParticipant::new("authority", resource(30)),
        ],
    )
}

fn alternate_relation() -> TypedRelationInstance {
    TypedRelationInstance::new(
        relation(101),
        RelationNamespace::Discovery,
        "eligible_for",
        vec![
            RelationParticipant::new("resource", resource(11)),
            RelationParticipant::new("purpose", resource(21)),
            RelationParticipant::new("authority", resource(31)),
        ],
    )
}

fn path(participants: Vec<RelationParticipant>) -> GraphPathEvidence {
    GraphPathEvidence {
        resource_path: vec![resource(10), resource(20)],
        steps: vec![GraphPathStepEvidence {
            relation_id: relation(100),
            namespace: RelationNamespace::Discovery,
            relation_type: "eligible_for".into(),
            from_role: "resource".into(),
            from_resource: resource(10),
            to_role: "purpose".into(),
            to_resource: resource(20),
            participants,
            evidence_refs: vec!["synthetic-evidence".into()],
            provenance: None,
        }],
    }
}

#[test]
fn coverage_absence_is_not_a_retrieval_miss() {
    let scenario = EvaluationScenario::new(
        ScenarioCategory::SourceCoverageAbsent,
        SourceKnowledgeTruth::Absent,
    );
    let outcome = evaluate_scenario(&scenario, &observation());
    assert_eq!(outcome.failure, Some(FailureClass::SourceKnowledgeAbsent));
    assert_eq!(outcome.metrics.retrieval.relevant_resources, 0);
    assert_eq!(outcome.metrics.retrieval.misses, 0);
}

#[test]
fn a_required_source_miss_is_attributed_before_retrieval() {
    let scenario = present(ScenarioCategory::RemoteRights);
    let outcome = evaluate_scenario(&scenario, &observation());
    assert_eq!(outcome.failure, Some(FailureClass::SourceRoutingMiss));
    assert_eq!(outcome.metrics.routing.required_sources, 1);
    assert_eq!(outcome.metrics.routing.misses, 1);
    assert_eq!(outcome.metrics.retrieval.misses, 0);
}

#[test]
fn confusable_hit_does_not_satisfy_relevant_resource_recall() {
    let scenario = present(ScenarioCategory::ConfusableResource);
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(11));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.failure, Some(FailureClass::RetrievalMiss));
    assert_eq!(outcome.metrics.retrieval.relevant_resources, 1);
    assert_eq!(outcome.metrics.retrieval.misses, 1);
}

#[test]
fn n_ary_false_composite_and_graph_path_miss_are_separate() {
    let mut scenario = present(ScenarioCategory::HyperGraph);
    scenario.graph_truth_complete = true;
    scenario.known_relations.push(valid_relation());
    scenario.known_relations.push(alternate_relation());
    scenario.required_relations.push(relation(100));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.graph_paths.push(path(vec![
        RelationParticipant::new("resource", resource(10)),
        RelationParticipant::new("purpose", resource(20)),
        RelationParticipant::new("authority", resource(31)),
    ]));

    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.failure, Some(FailureClass::GraphPathMiss));
    assert_eq!(outcome.metrics.graph.misses, 1);
    assert_eq!(outcome.safety.false_composite_paths, 1);
    assert_eq!(outcome.safety.hard_false_accepts, 0);
}

#[test]
fn a_valid_n_ary_path_uses_one_relation_identity() {
    let mut scenario = present(ScenarioCategory::HyperGraph);
    scenario.graph_truth_complete = true;
    let relation = valid_relation();
    let participants = relation.participants.clone();
    scenario.known_relations.push(relation);
    scenario
        .required_relations
        .push(RelationId::from_uuid(Uuid::from_u128(100)));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.graph_paths.push(path(participants));
    observed.qualified_resources.push(resource(10));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.failure, None);
    assert_eq!(outcome.metrics.graph.found, 1);
    assert_eq!(outcome.safety.false_composite_paths, 0);
}

#[test]
fn an_unverified_graph_truth_does_not_label_a_path_false_composite() {
    let scenario = present(ScenarioCategory::ConfusableResource);
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.qualified_resources.push(resource(10));
    observed.graph_paths.push(path(vec![
        RelationParticipant::new("resource", resource(10)),
        RelationParticipant::new("purpose", resource(20)),
        RelationParticipant::new("authority", resource(30)),
    ]));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.safety.false_composite_paths, 0);
    assert_eq!(outcome.failure, None);
}

#[test]
fn role_mismatch_is_graph_invalid_not_false_composite() {
    let mut scenario = present(ScenarioCategory::HyperGraph);
    scenario.graph_truth_complete = true;
    let truth = valid_relation();
    let participants = truth.participants.clone();
    scenario.known_relations.push(truth);
    scenario.required_relations.push(relation(100));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    let mut invalid = path(participants);
    invalid.steps[0].from_role = "wrong_role".into();
    observed.graph_paths.push(invalid);
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.safety.false_composite_paths, 0);
    assert_eq!(outcome.metrics.graph.invalid_paths, 1);
    assert_eq!(outcome.metrics.graph.misses, 1);
}

#[test]
fn empty_path_is_graph_invalid_not_false_composite() {
    let mut scenario = present(ScenarioCategory::HyperGraph);
    scenario.graph_truth_complete = true;
    scenario.known_relations.push(valid_relation());
    scenario.required_relations.push(relation(100));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.graph_paths.push(GraphPathEvidence {
        resource_path: vec![resource(10)],
        steps: vec![],
    });
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.safety.false_composite_paths, 0);
    assert_eq!(outcome.metrics.graph.invalid_paths, 1);
    assert_eq!(outcome.metrics.graph.misses, 1);
}

#[test]
fn temporal_hard_false_accept_is_not_a_graph_error() {
    let mut scenario = present(ScenarioCategory::Temporal);
    scenario.hard_ineligible_resources.push(resource(11));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.qualified_resources.push(resource(10));
    observed.qualified_resources.push(resource(11));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.failure, Some(FailureClass::ApplicabilityError));
    assert_eq!(outcome.safety.hard_false_accepts, 1);
    assert_eq!(outcome.safety.false_composite_paths, 0);
}

#[test]
fn retrieved_eligible_resource_rejected_by_applicability_is_a_false_reject() {
    let scenario = present(ScenarioCategory::Temporal);
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.failure, Some(FailureClass::ApplicabilityError));
    assert_eq!(outcome.metrics.applicability.eligible_retrieved, 1);
    assert_eq!(outcome.metrics.applicability.false_rejects, 1);
}

#[test]
fn retrieved_hard_ineligible_resource_is_not_an_eligible_false_reject() {
    let mut scenario = present(ScenarioCategory::Temporal);
    scenario.relevant_resources.push(resource(11));
    scenario.hard_ineligible_resources.push(resource(11));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed
        .retrieved_resources
        .extend([resource(10), resource(11)]);
    observed.qualified_resources.push(resource(10));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.metrics.retrieval.found, 2);
    assert_eq!(outcome.metrics.applicability.eligible_retrieved, 1);
    assert_eq!(outcome.metrics.applicability.false_rejects, 0);
    assert_eq!(outcome.failure, None);
}

#[test]
fn remote_query_miss_without_complete_coverage_is_not_absence() {
    let mut scenario = EvaluationScenario::new(
        ScenarioCategory::RemoteRights,
        SourceKnowledgeTruth::Unverified,
    );
    scenario.required_sources.push(source(1));
    scenario.expected_gaps.push(InformationGap::new(
        "remote coverage",
        GapReason::UnsupportedCoverage,
        false,
    ));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.gaps.push(InformationGap::new(
        "remote coverage",
        GapReason::UnsupportedCoverage,
        false,
    ));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.failure, None);
    assert_eq!(outcome.metrics.retrieval.misses, 0);
    assert_eq!(outcome.metrics.coverage.unverified_cases, 1);
}

#[test]
fn supported_claim_with_unresolvable_locator_is_evidence_error() {
    let mut scenario = present(ScenarioCategory::EvidenceConflict);
    scenario.required_claims.push(claim(50));
    scenario.expected_claim_locators.insert(
        claim(50),
        vec![EvidenceLocator::new(source(1), "known-ref")],
    );
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.qualified_resources.push(resource(10));
    observed.claims.push(ObservedClaim::new(
        claim(50),
        ClaimState::Supported,
        vec![EvidenceLocator::new(source(1), "wrong-ref")],
    ));
    observed.evidence_sufficiency = EvidenceSufficiency::Sufficient;
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.failure, Some(FailureClass::EvidenceLocatorError));
    assert_eq!(outcome.metrics.evidence.locator_errors, 1);
}

#[test]
fn a_locator_verified_for_claim_a_cannot_satisfy_claim_b() {
    let mut scenario = present(ScenarioCategory::EvidenceConflict);
    scenario.required_claims.extend([claim(50), claim(51)]);
    scenario.expected_claim_locators.insert(
        claim(50),
        vec![EvidenceLocator::new(source(1), "claim-a-ref")],
    );
    scenario.expected_claim_locators.insert(
        claim(51),
        vec![EvidenceLocator::new(source(1), "claim-b-ref")],
    );
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.qualified_resources.push(resource(10));
    observed.claims.push(ObservedClaim::new(
        claim(50),
        ClaimState::Supported,
        vec![EvidenceLocator::new(source(1), "claim-a-ref")],
    ));
    observed.claims.push(ObservedClaim::new(
        claim(51),
        ClaimState::Supported,
        vec![EvidenceLocator::new(source(1), "claim-a-ref")],
    ));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.metrics.evidence.located, 1);
    assert_eq!(outcome.metrics.evidence.locator_errors, 1);
    assert_eq!(outcome.failure, Some(FailureClass::EvidenceLocatorError));
}

#[test]
fn access_exposure_is_counted_even_when_a_valid_resource_is_found() {
    let mut scenario = present(ScenarioCategory::SecurityAccess);
    scenario.unauthorized_resources.push(resource(99));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed
        .retrieved_resources
        .extend([resource(10), resource(99)]);
    observed.qualified_resources.push(resource(10));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.failure, Some(FailureClass::SecurityExposure));
    assert_eq!(outcome.safety.unauthorized_exposures, 1);
}

#[test]
fn unauthorized_n_ary_participant_is_a_security_exposure_with_an_authorized_endpoint() {
    let mut scenario = present(ScenarioCategory::SecurityAccess);
    scenario.unauthorized_resources.push(resource(99));
    scenario.graph_truth_complete = true;
    let mut known_relation = valid_relation();
    known_relation.participants[2] = RelationParticipant::new("authority", resource(99));
    let participants = known_relation.participants.clone();
    scenario.known_relations.push(known_relation);
    scenario.required_relations.push(relation(100));

    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.qualified_resources.push(resource(10));
    observed.graph_paths.push(path(participants));

    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.metrics.graph.found, 1);
    assert_eq!(outcome.safety.unauthorized_exposures, 1);
    assert_eq!(outcome.failure, Some(FailureClass::SecurityExposure));
}

#[test]
fn unauthorized_graph_resource_path_member_is_a_security_exposure() {
    let mut scenario = present(ScenarioCategory::SecurityAccess);
    scenario.unauthorized_resources.push(resource(99));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.qualified_resources.push(resource(10));
    let mut graph_path = path(valid_relation().participants);
    graph_path.resource_path.insert(1, resource(99));
    observed.graph_paths.push(graph_path);

    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.safety.unauthorized_exposures, 1);
    assert_eq!(outcome.failure, Some(FailureClass::SecurityExposure));
}

#[test]
fn fault_gap_is_kept_distinct_from_verified_source_absence() {
    let mut scenario = EvaluationScenario::new(
        ScenarioCategory::FaultInjection,
        SourceKnowledgeTruth::Unverified,
    );
    scenario.required_sources.push(source(1));
    scenario.expected_gaps.push(InformationGap::new(
        "provider",
        GapReason::Availability,
        true,
    ));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.failure, Some(FailureClass::GapReportingError));
    assert_eq!(outcome.metrics.retrieval.misses, 0);
    assert_eq!(outcome.metrics.coverage.missing_expected_gaps, 1);
}

#[test]
fn one_observed_gap_cannot_satisfy_two_expected_gaps_with_the_same_reason() {
    let mut scenario = EvaluationScenario::new(
        ScenarioCategory::FaultInjection,
        SourceKnowledgeTruth::Unverified,
    );
    scenario.expected_gaps.extend([
        InformationGap::new("provider A", GapReason::Availability, true),
        InformationGap::new("provider B", GapReason::Availability, true),
    ]);
    let mut observed = observation();
    observed.gaps.push(InformationGap::new(
        "provider A",
        GapReason::Availability,
        true,
    ));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.metrics.coverage.expected_gaps, 2);
    assert_eq!(outcome.metrics.coverage.observed_expected_gaps, 1);
    assert_eq!(outcome.metrics.coverage.missing_expected_gaps, 1);
}

#[test]
fn same_reason_for_another_required_fact_does_not_match_the_expected_gap() {
    let mut scenario = EvaluationScenario::new(
        ScenarioCategory::FaultInjection,
        SourceKnowledgeTruth::Unverified,
    );
    scenario.expected_gaps.push(InformationGap::new(
        "required fact A",
        GapReason::MissingFact,
        true,
    ));
    let mut observed = observation();
    observed.gaps.push(InformationGap::new(
        "required fact B",
        GapReason::MissingFact,
        true,
    ));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.metrics.coverage.observed_expected_gaps, 0);
    assert_eq!(outcome.metrics.coverage.missing_expected_gaps, 1);
}

#[test]
fn blocking_availability_gap_prevents_completion() {
    let mut scenario = EvaluationScenario::new(
        ScenarioCategory::FaultInjection,
        SourceKnowledgeTruth::Unverified,
    );
    scenario.expected_completion = Some(false);
    let mut observed = EvaluationObservation::new(EvidenceSufficiency::Sufficient);
    observed.gaps.push(InformationGap::new(
        "provider",
        GapReason::Availability,
        true,
    ));
    let outcome = evaluate_scenario(&scenario, &observed);
    assert_eq!(outcome.metrics.completion.observed_complete_cases, 0);
    assert_eq!(outcome.metrics.completion.correct_cases, 1);
}

#[test]
fn aggregate_report_contains_counts_and_codes_without_fixture_content() {
    let mut scenario = present(ScenarioCategory::ConfusableResource);
    scenario.hard_ineligible_resources.push(resource(99));
    let mut observed = observation();
    observed.planned_sources.push(source(1));
    observed.retrieved_resources.push(resource(10));
    observed.qualified_resources.push(resource(99));
    let outcomes = [
        evaluate_scenario(&scenario, &observed),
        evaluate_scenario(
            &EvaluationScenario::new(
                ScenarioCategory::SourceCoverageAbsent,
                SourceKnowledgeTruth::Absent,
            ),
            &observation(),
        ),
    ];
    let report = EvaluationReport::from_outcomes(&outcomes);
    assert_eq!(report.scenario_count, 2);
    assert_eq!(report.safety.hard_false_accepts, 1);
    assert_eq!(report.safety.false_composite_paths, 0);
    assert_eq!(report.failure_count(FailureClass::SourceKnowledgeAbsent), 1);
    assert_eq!(report.failure_count(FailureClass::RetrievalMiss), 0);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("known-ref"));
    assert!(!json.contains("private://"));
    assert!(!json.contains(&resource(99).as_uuid().to_string()));
}

#[test]
fn typed_search_outputs_are_reduced_to_ids_and_verified_direct_locators() {
    let routes = SourceRoutePlan {
        routes: vec![SourceRoute {
            source_id: source(1),
            role: SourceRole::Required,
            stage: RouteStage::Initial,
            discovery_mode: Some(DiscoveryMode::LocalDirectory),
            discovery_modes: vec![DiscoveryMode::LocalDirectory],
            enumeration_semantics: Some(EnumerationSemantics::Complete),
            state: RouteState::Planned,
            required_claims: vec![claim(50)],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            unresolved_gaps: vec![],
        }],
    };
    let mut candidate = FederatedCandidate::new(
        "synthetic-candidate",
        CandidateIdentityClass::DurableResource,
        source(1),
        "directory",
    );
    candidate.resource_ref = Some(resource(10));
    candidate.locator = Some("private://must-not-report".into());
    let hits = vec![RawRetrievalHit {
        candidate,
        structured_outcomes: None,
        graph_paths: None,
        retriever_id: "directory".into(),
        generation: ProjectionGenerationKey {
            source_id: source(1),
            generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(2)),
        },
        rank: 1,
        unit_hit: None,
    }];
    let mut supported = Claim::new(claim(50), ClaimState::Supported);
    let mut direct = EvidenceReference::new(source(1), "synthetic-origin", EvidenceRole::Primary);
    direct.evidence_ref = Some("known-ref".into());
    let mut summary = EvidenceReference::new(source(1), "synthetic-origin", EvidenceRole::Primary);
    summary.evidence_ref = Some("summary-ref".into());
    summary.is_summary = true;
    supported.evidence_refs = vec![direct, summary];
    let result = DiscoveryResult {
        discovery_evaluation_id: DiscoveryEvaluationId::from_uuid(Uuid::from_u128(3)),
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(4)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "synthetic purpose".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![],
            required_claims: vec![claim(50)],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![claim(50)]),
        },
        qualified_resources: vec![QualifiedResource {
            resource_ref: resource(10),
            source_ref: None,
            usage_profile_ref: None,
            applicability: ApplicabilityState::Applicable,
            matched_conditions: vec![],
            resolved_discriminators: vec![],
            remaining_nonblocking_unknowns: vec![],
            contrast_resolution: None,
            evidence_refs: vec!["known-ref".into()],
            qualification_trace: vec![],
        }],
        evidence_set: vec![supported],
        evidence_sufficiency: EvidenceSufficiency::Sufficient,
        unresolved_gaps: vec![InformationGap::new(
            "synthetic coverage",
            GapReason::UnsupportedCoverage,
            false,
        )],
        rejected_candidates: vec![],
        source_trace: vec!["private://must-not-report".into()],
        retrieval_trace: vec!["private://must-not-report".into()],
        qualification_trace: vec!["private://must-not-report".into()],
    };
    let observed = EvaluationObservation::from_search(&routes, &hits, &result);
    assert_eq!(observed.planned_sources, vec![source(1)]);
    assert_eq!(observed.retrieved_resources, vec![resource(10)]);
    assert_eq!(
        observed.claims[0].locators,
        vec![EvidenceLocator::new(source(1), "known-ref")]
    );
    assert_eq!(
        observed.gaps,
        vec![InformationGap::new(
            "synthetic coverage",
            GapReason::UnsupportedCoverage,
            false,
        )]
    );
    let mut scenario = present(ScenarioCategory::EvidenceConflict);
    scenario.required_claims.push(claim(50));
    scenario.expected_claim_locators.insert(
        claim(50),
        vec![EvidenceLocator::new(source(1), "known-ref")],
    );
    scenario.expected_completion = Some(true);
    let report = EvaluationReport::from_outcomes(&[evaluate_scenario(&scenario, &observed)]);
    assert_eq!(report.failure_count(FailureClass::EvidenceLocatorError), 0);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("private://"));
    assert!(!json.contains("known-ref"));
    assert!(!json.contains("synthetic coverage"));
}

#[test]
fn synthetic_seven_category_report_emits_only_stage_counts() {
    let mut cases = Vec::new();

    let mut confusable = observation();
    confusable.planned_sources.push(source(1));
    confusable.retrieved_resources.push(resource(11));
    cases.push(evaluate_scenario(
        &present(ScenarioCategory::ConfusableResource),
        &confusable,
    ));

    let mut temporal_truth = present(ScenarioCategory::Temporal);
    temporal_truth.hard_ineligible_resources.push(resource(11));
    let mut temporal = observation();
    temporal.planned_sources.push(source(1));
    temporal.retrieved_resources.push(resource(10));
    temporal.qualified_resources.push(resource(10));
    temporal.qualified_resources.push(resource(11));
    cases.push(evaluate_scenario(&temporal_truth, &temporal));

    let mut graph_truth = present(ScenarioCategory::HyperGraph);
    graph_truth.graph_truth_complete = true;
    graph_truth.known_relations.push(valid_relation());
    graph_truth.known_relations.push(alternate_relation());
    graph_truth.required_relations.push(relation(100));
    let mut graph = observation();
    graph.planned_sources.push(source(1));
    graph.retrieved_resources.push(resource(10));
    graph.graph_paths.push(path(vec![
        RelationParticipant::new("resource", resource(10)),
        RelationParticipant::new("purpose", resource(20)),
        RelationParticipant::new("authority", resource(31)),
    ]));
    cases.push(evaluate_scenario(&graph_truth, &graph));

    let mut remote_truth = EvaluationScenario::new(
        ScenarioCategory::RemoteRights,
        SourceKnowledgeTruth::Unverified,
    );
    remote_truth.required_sources.push(source(1));
    remote_truth.expected_gaps.push(InformationGap::new(
        "remote coverage",
        GapReason::UnsupportedCoverage,
        false,
    ));
    let mut remote = observation();
    remote.planned_sources.push(source(1));
    remote.gaps.push(InformationGap::new(
        "remote coverage",
        GapReason::UnsupportedCoverage,
        false,
    ));
    cases.push(evaluate_scenario(&remote_truth, &remote));

    let mut evidence_truth = present(ScenarioCategory::EvidenceConflict);
    evidence_truth.required_claims.push(claim(50));
    evidence_truth.expected_claim_locators.insert(
        claim(50),
        vec![EvidenceLocator::new(source(1), "synthetic-valid")],
    );
    let mut evidence = observation();
    evidence.planned_sources.push(source(1));
    evidence.retrieved_resources.push(resource(10));
    evidence.qualified_resources.push(resource(10));
    evidence.claims.push(ObservedClaim::new(
        claim(50),
        ClaimState::Supported,
        vec![EvidenceLocator::new(source(1), "synthetic-invalid")],
    ));
    cases.push(evaluate_scenario(&evidence_truth, &evidence));

    let mut security_truth = present(ScenarioCategory::SecurityAccess);
    security_truth.unauthorized_resources.push(resource(99));
    let mut security = observation();
    security.planned_sources.push(source(1));
    security
        .retrieved_resources
        .extend([resource(10), resource(99)]);
    security.qualified_resources.push(resource(10));
    cases.push(evaluate_scenario(&security_truth, &security));

    cases.push(evaluate_scenario(
        &EvaluationScenario::new(
            ScenarioCategory::SourceCoverageAbsent,
            SourceKnowledgeTruth::Absent,
        ),
        &observation(),
    ));

    let report = EvaluationReport::from_outcomes(&cases);
    assert_eq!(report.scenario_count, 7);
    assert_eq!(report.categories.len(), 7);
    assert_eq!(report.safety.false_composite_paths, 1);
    assert_eq!(report.safety.hard_false_accepts, 1);
    assert_eq!(report.safety.unauthorized_exposures, 1);
    assert_eq!(report.failure_count(FailureClass::SourceKnowledgeAbsent), 1);
    assert_eq!(report.failure_count(FailureClass::RetrievalMiss), 1);
    assert_eq!(report.stage_metrics.retrieval.misses, 1);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("synthetic-valid"));
    assert!(!json.contains("synthetic-invalid"));
    assert!(!json.contains("remote coverage"));
    println!("SEARCH_EVALUATION_SYNTHETIC_REPORT={json}");
}
