//! Synthetic ground truth and observations over existing Search pipeline types.

use std::collections::{BTreeMap, BTreeSet};

use search_application::retrieval_execution::RawRetrievalHit;
use search_application::routing::{RouteState, SourceRoutePlan};
use search_core::discovery::{DiscoveryResult, InformationGap};
use search_core::evidence::{ClaimState, EvidenceSufficiency, is_verified_direct_claim_evidence};
use search_core::graph::{GraphPathEvidence, GraphPathStepEvidence};
use search_core::id::{ClaimId, RelationId, ResourceId, SourceId};
use search_core::relation::TypedRelationInstance;
use serde::Serialize;

use crate::report::{FailureClass, SafetyMetrics, ScenarioOutcome, StageMetrics};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioCategory {
    ConfusableResource,
    Temporal,
    HyperGraph,
    RemoteRights,
    EvidenceConflict,
    SecurityAccess,
    SourceCoverageAbsent,
    FaultInjection,
    PromptInjection,
}

/// Hidden Source truth must be independently established by a fixture/owner.
/// A remote query miss alone is `Unverified`, never `Absent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKnowledgeTruth {
    Present,
    Absent,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceLocator {
    pub source_ref: SourceId,
    pub opaque_ref: String,
}

impl EvidenceLocator {
    pub fn new(source_ref: SourceId, opaque_ref: impl Into<String>) -> Self {
        Self {
            source_ref,
            opaque_ref: opaque_ref.into(),
        }
    }
}

/// Truth fields contain IDs and opaque locators only; caller-owned instructions
/// and Source content are deliberately excluded from the report boundary.
#[derive(Debug, Clone)]
pub struct EvaluationScenario {
    pub category: ScenarioCategory,
    pub source_knowledge: SourceKnowledgeTruth,
    pub required_sources: Vec<SourceId>,
    pub relevant_resources: Vec<ResourceId>,
    pub hard_ineligible_resources: Vec<ResourceId>,
    pub unauthorized_resources: Vec<ResourceId>,
    pub required_relations: Vec<RelationId>,
    pub known_relations: Vec<TypedRelationInstance>,
    /// False-composite claims are possible only against independently complete truth.
    pub graph_truth_complete: bool,
    pub required_claims: Vec<ClaimId>,
    pub expected_claim_locators: BTreeMap<ClaimId, Vec<EvidenceLocator>>,
    pub expected_gaps: Vec<InformationGap>,
    pub expected_completion: Option<bool>,
}

impl EvaluationScenario {
    pub fn new(category: ScenarioCategory, source_knowledge: SourceKnowledgeTruth) -> Self {
        Self {
            category,
            source_knowledge,
            required_sources: Vec::new(),
            relevant_resources: Vec::new(),
            hard_ineligible_resources: Vec::new(),
            unauthorized_resources: Vec::new(),
            required_relations: Vec::new(),
            known_relations: Vec::new(),
            graph_truth_complete: false,
            required_claims: Vec::new(),
            expected_claim_locators: BTreeMap::new(),
            expected_gaps: Vec::new(),
            expected_completion: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedClaim {
    pub claim_id: ClaimId,
    pub state: ClaimState,
    pub locators: Vec<EvidenceLocator>,
}

impl ObservedClaim {
    pub fn new(claim_id: ClaimId, state: ClaimState, locators: Vec<EvidenceLocator>) -> Self {
        Self {
            claim_id,
            state,
            locators,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvaluationObservation {
    pub planned_sources: Vec<SourceId>,
    pub retrieved_resources: Vec<ResourceId>,
    pub graph_paths: Vec<GraphPathEvidence>,
    pub qualified_resources: Vec<ResourceId>,
    pub claims: Vec<ObservedClaim>,
    pub evidence_sufficiency: EvidenceSufficiency,
    pub gaps: Vec<InformationGap>,
}

impl EvaluationObservation {
    pub fn new(evidence_sufficiency: EvidenceSufficiency) -> Self {
        Self {
            planned_sources: Vec::new(),
            retrieved_resources: Vec::new(),
            graph_paths: Vec::new(),
            qualified_resources: Vec::new(),
            claims: Vec::new(),
            evidence_sufficiency,
            gaps: Vec::new(),
        }
    }

    /// Convert typed Search outputs without parsing free-form trace strings.
    /// Only direct verified claim evidence contributes a locator.
    pub fn from_search(
        routes: &SourceRoutePlan,
        hits: &[RawRetrievalHit],
        result: &DiscoveryResult,
    ) -> Self {
        let mut observed = Self::new(result.evidence_sufficiency);
        observed.planned_sources = routes
            .routes
            .iter()
            .filter(|route| route.state == RouteState::Planned)
            .map(|route| route.source_id)
            .collect();
        for hit in hits {
            if let Some(resource) = hit.candidate.resource_ref {
                observed.retrieved_resources.push(resource);
            }
            if let Some(paths) = &hit.graph_paths {
                observed.graph_paths.extend(paths.iter().cloned());
            }
        }
        observed.qualified_resources = result
            .qualified_resources
            .iter()
            .map(|resource| resource.resource_ref)
            .collect();
        observed.claims = result
            .evidence_set
            .iter()
            .map(|claim| {
                ObservedClaim::new(
                    claim.claim_id,
                    claim.state,
                    claim
                        .evidence_refs
                        .iter()
                        .filter(|reference| {
                            is_verified_direct_claim_evidence(
                                reference.role,
                                reference.is_summary,
                                &reference.upstream_origin,
                            )
                        })
                        .filter_map(|reference| {
                            reference.evidence_ref.as_ref().map(|opaque_ref| {
                                EvidenceLocator::new(reference.source_ref, opaque_ref)
                            })
                        })
                        .collect(),
                )
            })
            .collect();
        observed.gaps = result.unresolved_gaps.clone();
        observed
    }
}

pub fn evaluate_scenario(
    scenario: &EvaluationScenario,
    observed: &EvaluationObservation,
) -> ScenarioOutcome {
    let mut metrics = StageMetrics::default();
    let mut safety = SafetyMetrics::default();
    let required_sources: BTreeSet<_> = scenario.required_sources.iter().copied().collect();
    let planned_sources: BTreeSet<_> = observed.planned_sources.iter().copied().collect();
    metrics.routing.required_sources = required_sources.len();
    metrics.routing.planned_sources = required_sources.intersection(&planned_sources).count();
    metrics.routing.misses = metrics.routing.required_sources - metrics.routing.planned_sources;

    let relevant_resources: BTreeSet<_> = scenario.relevant_resources.iter().copied().collect();
    let retrieved_resources: BTreeSet<_> = observed.retrieved_resources.iter().copied().collect();
    if scenario.source_knowledge == SourceKnowledgeTruth::Present && metrics.routing.misses == 0 {
        metrics.retrieval.relevant_resources = relevant_resources.len();
        metrics.retrieval.found = relevant_resources
            .intersection(&retrieved_resources)
            .count();
        metrics.retrieval.misses = metrics.retrieval.relevant_resources - metrics.retrieval.found;
    }

    let required_relations: BTreeSet<_> = scenario.required_relations.iter().copied().collect();
    let mut valid_relations = BTreeSet::new();
    if scenario.graph_truth_complete {
        for path in &observed.graph_paths {
            match classify_path(path, &scenario.known_relations) {
                GraphPathClass::Valid => {
                    valid_relations.extend(path.steps.iter().map(|step| step.relation_id));
                }
                GraphPathClass::FalseComposite => safety.false_composite_paths += 1,
                GraphPathClass::Invalid => metrics.graph.invalid_paths += 1,
            }
        }
    }
    if scenario.graph_truth_complete
        && scenario.source_knowledge == SourceKnowledgeTruth::Present
        && metrics.routing.misses == 0
        && metrics.retrieval.misses == 0
    {
        metrics.graph.required_relations = required_relations.len();
        metrics.graph.found = required_relations.intersection(&valid_relations).count();
        metrics.graph.misses = metrics.graph.required_relations - metrics.graph.found;
    }

    let qualified: BTreeSet<_> = observed.qualified_resources.iter().copied().collect();
    let hard_ineligible: BTreeSet<_> = scenario.hard_ineligible_resources.iter().copied().collect();
    safety.hard_false_accepts = hard_ineligible.intersection(&qualified).count();
    let unauthorized: BTreeSet<_> = scenario.unauthorized_resources.iter().copied().collect();
    let mut exposed_resources = retrieved_resources.clone();
    exposed_resources.extend(qualified.iter().copied());
    for path in &observed.graph_paths {
        exposed_resources.extend(path.resource_path.iter().copied());
        for step in &path.steps {
            exposed_resources.extend([step.from_resource, step.to_resource]);
            exposed_resources.extend(
                step.participants
                    .iter()
                    .map(|participant| participant.resource_ref),
            );
        }
    }
    safety.unauthorized_exposures = unauthorized.intersection(&exposed_resources).count();
    let eligible_resources: BTreeSet<_> = relevant_resources
        .difference(&hard_ineligible)
        .copied()
        .filter(|resource| !unauthorized.contains(resource))
        .collect();
    if scenario.source_knowledge == SourceKnowledgeTruth::Present
        && metrics.routing.misses == 0
        && metrics.retrieval.misses == 0
        && metrics.graph.misses == 0
    {
        metrics.applicability.eligible_retrieved = eligible_resources
            .intersection(&retrieved_resources)
            .count();
        metrics.applicability.qualified_eligible = eligible_resources
            .intersection(&retrieved_resources)
            .filter(|resource| qualified.contains(resource))
            .count();
        metrics.applicability.false_rejects =
            metrics.applicability.eligible_retrieved - metrics.applicability.qualified_eligible;
    }

    let required_claims: BTreeSet<_> = scenario.required_claims.iter().copied().collect();
    if scenario.source_knowledge == SourceKnowledgeTruth::Present
        && metrics.routing.misses == 0
        && metrics.retrieval.misses == 0
        && metrics.graph.misses == 0
        && metrics.applicability.false_rejects == 0
    {
        metrics.evidence.required_claims = required_claims.len();
        for claim_id in required_claims {
            let located = observed.claims.iter().any(|claim| {
                claim.claim_id == claim_id
                    && claim.state == ClaimState::Supported
                    && scenario
                        .expected_claim_locators
                        .get(&claim_id)
                        .is_some_and(|expected| {
                            claim
                                .locators
                                .iter()
                                .any(|locator| expected.contains(locator))
                        })
            });
            if located {
                metrics.evidence.located += 1;
            } else {
                metrics.evidence.locator_errors += 1;
            }
        }
    }

    metrics.coverage.known_absent_cases =
        usize::from(scenario.source_knowledge == SourceKnowledgeTruth::Absent);
    metrics.coverage.unverified_cases =
        usize::from(scenario.source_knowledge == SourceKnowledgeTruth::Unverified);
    let mut unmatched_observed = observed.gaps.iter().collect::<Vec<_>>();
    for gap in &scenario.expected_gaps {
        metrics.coverage.expected_gaps += 1;
        if let Some(index) = unmatched_observed
            .iter()
            .position(|observed| gap_matches(gap, observed))
        {
            unmatched_observed.swap_remove(index);
            metrics.coverage.observed_expected_gaps += 1;
        } else {
            metrics.coverage.missing_expected_gaps += 1;
        }
    }
    if let Some(expected) = scenario.expected_completion {
        metrics.completion.expected_complete_cases = usize::from(expected);
        let complete = observed.evidence_sufficiency == EvidenceSufficiency::Sufficient
            && !observed.gaps.iter().any(|gap| gap.blocking);
        metrics.completion.observed_complete_cases = usize::from(complete);
        metrics.completion.correct_cases = usize::from(expected == complete);
    }

    let failure = if safety.unauthorized_exposures > 0 {
        Some(FailureClass::SecurityExposure)
    } else if scenario.source_knowledge == SourceKnowledgeTruth::Absent {
        Some(FailureClass::SourceKnowledgeAbsent)
    } else if metrics.routing.misses > 0 {
        Some(FailureClass::SourceRoutingMiss)
    } else if metrics.retrieval.misses > 0 {
        Some(FailureClass::RetrievalMiss)
    } else if metrics.graph.misses > 0
        || metrics.graph.invalid_paths > 0
        || safety.false_composite_paths > 0
    {
        Some(FailureClass::GraphPathMiss)
    } else if safety.hard_false_accepts > 0 || metrics.applicability.false_rejects > 0 {
        Some(FailureClass::ApplicabilityError)
    } else if metrics.evidence.locator_errors > 0 {
        Some(FailureClass::EvidenceLocatorError)
    } else if metrics.coverage.missing_expected_gaps > 0 {
        Some(FailureClass::GapReportingError)
    } else if scenario.expected_completion.is_some() && metrics.completion.correct_cases == 0 {
        Some(FailureClass::CompletionError)
    } else {
        None
    };

    ScenarioOutcome {
        category: scenario.category,
        failure,
        metrics,
        safety,
    }
}

fn gap_matches(expected: &InformationGap, observed: &InformationGap) -> bool {
    expected.gap_id == observed.gap_id
        && expected.required_fact == observed.required_fact
        && expected.reason == observed.reason
        && expected.blocking == observed.blocking
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphPathClass {
    Valid,
    FalseComposite,
    Invalid,
}

fn classify_path(
    path: &GraphPathEvidence,
    known_relations: &[TypedRelationInstance],
) -> GraphPathClass {
    if path.steps.is_empty() || path.resource_path.len() != path.steps.len() + 1 {
        return GraphPathClass::Invalid;
    }
    let mut has_composite_step = false;
    for (index, step) in path.steps.iter().enumerate() {
        if step.from_resource != path.resource_path[index]
            || step.to_resource != path.resource_path[index + 1]
        {
            return GraphPathClass::Invalid;
        }
        if known_relations
            .iter()
            .any(|relation| step_matches_relation(step, relation))
        {
            continue;
        }
        if is_proven_nary_composite(step, known_relations) {
            has_composite_step = true;
        } else {
            return GraphPathClass::Invalid;
        }
    }
    if has_composite_step {
        GraphPathClass::FalseComposite
    } else {
        GraphPathClass::Valid
    }
}

fn step_matches_relation(step: &GraphPathStepEvidence, relation: &TypedRelationInstance) -> bool {
    let mut step_participants = step.participants.clone();
    step_participants.sort();
    let mut known_participants = relation.participants.clone();
    known_participants.sort();
    step.relation_id == relation.relation_id
        && step.namespace == relation.namespace
        && step.relation_type == relation.relation_type
        && step_participants == known_participants
        && relation.has_participant(&step.from_role, Some(step.from_resource))
        && relation.has_participant(&step.to_role, Some(step.to_resource))
}

fn is_proven_nary_composite(
    step: &GraphPathStepEvidence,
    known_relations: &[TypedRelationInstance],
) -> bool {
    let Some(base) = known_relations.iter().find(|relation| {
        relation.relation_id == step.relation_id
            && relation.namespace == step.namespace
            && relation.relation_type == step.relation_type
    }) else {
        return false;
    };
    if step.participants.len() < 3
        || step.participants.len() != base.participants.len()
        || !base.has_participant(&step.from_role, Some(step.from_resource))
        || !base.has_participant(&step.to_role, Some(step.to_resource))
    {
        return false;
    }
    let mut claimed_roles: Vec<_> = step.participants.iter().map(|p| &p.role).collect();
    let mut base_roles: Vec<_> = base.participants.iter().map(|p| &p.role).collect();
    claimed_roles.sort();
    base_roles.sort();
    if claimed_roles != base_roles
        || known_relations.iter().any(|relation| {
            relation.namespace == step.namespace
                && relation.relation_type == step.relation_type
                && sorted_participants(&relation.participants)
                    == sorted_participants(&step.participants)
        })
    {
        return false;
    }
    let mut from_base = false;
    let mut from_other = false;
    for participant in &step.participants {
        if base.participants.contains(participant) {
            from_base = true;
        } else if known_relations.iter().any(|relation| {
            relation.relation_id != base.relation_id
                && relation.namespace == step.namespace
                && relation.relation_type == step.relation_type
                && relation.participants.contains(participant)
        }) {
            from_other = true;
        } else {
            return false;
        }
    }
    from_base && from_other
}

fn sorted_participants(
    participants: &[search_core::relation::RelationParticipant],
) -> Vec<search_core::relation::RelationParticipant> {
    let mut sorted = participants.to_vec();
    sorted.sort();
    sorted
}
