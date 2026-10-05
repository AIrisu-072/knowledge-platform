//! Evidence-driven, generation-pinned Discovery over Source-owned ports.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use search_core::applicability::{ApplicabilityState, Discriminator, DiscriminatorImportance};
use search_core::assertion::AssertionOrigin;
use search_core::authority::AuthorityResolution;
use search_core::discovery::{
    CandidateIdentityClass, DiscoveryRequest, DiscoveryResult, FederatedCandidate, GapReason,
    InformationGap,
};
use search_core::evidence::{
    Claim, ClaimState, EvidenceReference, EvidenceRole, EvidenceSufficiency,
    claim_values_semantically_equal,
};
use search_core::fact::{Fact, FactOrigin, FactSet};
use search_core::id::{ClaimId, ResourceId, SourceId};
use search_core::knowledge_unit::normalize_unit_text;
use search_core::materialization::{ProbeCompletenessSemantics, ProbeOutcome};
use search_core::predicate::{ConceptResolver, Operand, PredicateExpr, TypedValue};
use search_core::profile::FacetState;
use search_core::projection::{CompiledResourceProjection, ProjectionGenerationKey};
use sha2::{Digest, Sha256};

use crate::action_selection::{
    ActionCandidate, ActionCostEstimate, ActionPriority, KnownStateDigest, NoProgressHistory,
    NoProgressKey, Selection, select_next_action,
};
use crate::body_ports::{
    BodyCoverageGapPort, CONTAINS_EXACT_PREDICATE, ExactScanBudget, ExactTextAbsenceOutcome,
    ExactTextEvidencePort, ExactTextSelector, KnowledgeUnitHitRef, PinnedBodyBundle,
    SourceExactTextAbsencePort,
};
use crate::candidate::{
    CandidateHardGates, HardGateEvaluation, RankedCandidateHit, RetrieverRankList,
};
use crate::content_scope::{BodySearchSpec, DiscoveryScope};
use crate::error::SearchError;
use crate::federation::{CandidateFederator, FusionStrategy};
use crate::materialization::ProbeBudget;
use crate::ports::{
    AccessDecision, AssertionStorePort, ClaimSelectorPort, ConceptRegistryPort,
    CurrentSourcePolicyPort, EvidenceResolverPort, GenerationReadPort, LexicalQuery,
    ProbeCapabilityCatalogPort, ProbeExecutionInput, ProbeExecutionService, ProbePort,
    SourceRegistryPort, StructuredFacetFilter, StructuredFacetOutcome, assemble_resource_claims,
    assemble_verified_unit_text_claim, assess_claim_evidence,
};
use crate::retrieval::{
    ActionState, RetrievalAction, RetrievalInputs, RetrieverKind, RetrieverPlanner,
    RetrieverProfile, RetrieverSupport,
};
use crate::retrieval_execution::{
    RawRetrievalHit, RetrievalExecutionInput, RetrievalExecutionPorts, RetrievalExecutor,
};
use crate::routing::{RoutingConstraints, SourceRole, SourceRoutePlan, SourceRouter};

/// A trusted caller resolves temporal policy before creating the service.
/// Opaque `DiscoveryNeed.freshness_requirements` are never guessed as durations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TemporalPolicy {
    pub require_effective_at_target: bool,
    pub max_current_age_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    pub routing: RoutingConstraints,
    pub retriever_profile: RetrieverProfile,
    pub retriever_support: RetrieverSupport,
    pub retrieval_inputs: RetrievalInputs,
    pub structured_filters: Vec<StructuredFacetFilter>,
    pub discriminators: Vec<Discriminator>,
    pub lexical_query: Option<LexicalQuery>,
    pub temporal_policy: TemporalPolicy,
    pub probe_budget: ProbeBudget,
    pub max_actions: usize,
    pub evaluation_currency: String,
}

pub struct DiscoveryPorts<'a> {
    pub sources: &'a dyn SourceRegistryPort,
    pub generations: &'a dyn GenerationReadPort,
    pub concepts: &'a dyn ConceptRegistryPort,
    pub retrieval: RetrievalExecutionPorts<'a>,
    pub selectors: &'a dyn ClaimSelectorPort,
    pub assertions: &'a dyn AssertionStorePort,
    pub evidence: &'a dyn EvidenceResolverPort,
    pub probe: Option<&'a dyn ProbePort>,
    pub probe_catalog: Option<&'a dyn ProbeCapabilityCatalogPort>,
    pub source_policy: Option<&'a dyn CurrentSourcePolicyPort>,
}

pub struct DiscoveryService<'a> {
    config: DiscoveryConfig,
    ports: DiscoveryPorts<'a>,
    exact_text: Option<&'a dyn ExactTextEvidencePort>,
    body_coverage: Option<&'a dyn BodyCoverageGapPort>,
    absence: Option<&'a dyn SourceExactTextAbsencePort>,
}

struct PinnedSource {
    key: ProjectionGenerationKey,
    concepts: Arc<dyn ConceptResolver + Send + Sync>,
}

struct HitRecord {
    raw: RawRetrievalHit,
    projection: Option<CompiledResourceProjection>,
    probe_facts: BTreeMap<String, Fact>,
    probe_outcomes: BTreeMap<String, ProbeObservation>,
    probe_origins: BTreeMap<String, ProbeOrigin>,
}

#[derive(Clone)]
struct ProbeOrigin {
    candidate: FederatedCandidate,
    generation: ProjectionGenerationKey,
}

#[derive(Clone, Copy)]
enum ProbeObservation {
    Found,
    NotFoundByProbe(ProbeCompletenessSemantics),
    Unsupported,
    Failed,
}

impl ProbeObservation {
    const fn code(self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::NotFoundByProbe(ProbeCompletenessSemantics::CompleteForQuery) => {
                "not_found_by_probe:complete_for_query"
            }
            Self::NotFoundByProbe(ProbeCompletenessSemantics::Partial) => {
                "not_found_by_probe:partial"
            }
            Self::NotFoundByProbe(ProbeCompletenessSemantics::Unknown) => {
                "not_found_by_probe:unknown"
            }
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone)]
struct ProbeTarget {
    candidate: FederatedCandidate,
    generation: ProjectionGenerationKey,
    gap: InformationGap,
}

struct Evaluation {
    result: DiscoveryResult,
    probes: Vec<ProbeTarget>,
}

enum NextAction {
    Retrieval(RetrievalAction),
    Probe(Box<ProbeTarget>),
}

impl<'a> DiscoveryService<'a> {
    pub fn new(config: DiscoveryConfig, ports: DiscoveryPorts<'a>) -> Result<Self, SearchError> {
        if config.max_actions == 0 || config.evaluation_currency.trim().is_empty() {
            return Err(SearchError::InvalidRequest(
                "Discovery requires a positive action limit and evaluation currency".into(),
            ));
        }
        if config.probe_budget.currency != config.evaluation_currency {
            return Err(SearchError::InvalidRequest(
                "Probe budget currency differs from evaluation currency".into(),
            ));
        }
        Ok(Self {
            config,
            ports,
            exact_text: None,
            body_coverage: None,
            absence: None,
        })
    }

    /// Source-owned exact-text selectors and Unit verification for body scope.
    pub fn with_exact_text_evidence(mut self, port: &'a dyn ExactTextEvidencePort) -> Self {
        self.exact_text = Some(port);
        self
    }

    /// Source-owned finite negative proof for an exact-text Claim.
    pub fn with_exact_text_absence(mut self, port: &'a dyn SourceExactTextAbsencePort) -> Self {
        self.absence = Some(port);
        self
    }

    /// Source-owned, access-filtered body coverage for body scope. Without it a
    /// body result never claims a complete corpus.
    pub fn with_body_coverage(mut self, port: &'a dyn BodyCoverageGapPort) -> Self {
        self.body_coverage = Some(port);
        self
    }

    pub async fn discover(
        &self,
        request: DiscoveryRequest,
    ) -> Result<DiscoveryResult, SearchError> {
        self.discover_with_content_scope(request, DiscoveryScope::Normal)
            .await
    }

    /// One shared evaluation loop for every content scope. `BodyRequired` runs
    /// only BodyOnly lexical actions and qualifies only verified Unit hits.
    pub async fn discover_with_content_scope(
        &self,
        request: DiscoveryRequest,
        scope: DiscoveryScope,
    ) -> Result<DiscoveryResult, SearchError> {
        validate_request(&request)?;
        let body = scope.body();
        if let Some(spec) = body {
            spec.validate()?;
            // An exact-text selector may bind only a Claim this request requires.
            if spec.exact_text_claim.is_some_and(|claim| {
                !request.need.required_claims.contains(&claim)
                    || !request
                        .need
                        .completion_requirement
                        .required_claims
                        .contains(&claim)
            }) {
                return Err(SearchError::InvalidRequest(
                    "exact-text claim is not a required claim of this request".into(),
                ));
            }
        }
        let sources = self.ports.sources.list_sources().await?;
        let routes = SourceRouter::plan(&request.need, &sources, &self.config.routing);
        // Directory, Structured, Graph, Vector, remote and probes cannot create a
        // body match, so a body scope plans only Lexical with the request query.
        let body_plan_inputs = body.map(|spec| {
            (
                RetrieverSupport {
                    lexical: self.config.retriever_support.lexical,
                    ..RetrieverSupport::default()
                },
                RetrievalInputs {
                    lexical_query: Some(spec.query.text.clone()),
                    max_initial_retrievers_per_source: self
                        .config
                        .retrieval_inputs
                        .max_initial_retrievers_per_source,
                    ..RetrievalInputs::default()
                },
            )
        });
        let (support, inputs) = match &body_plan_inputs {
            Some((support, inputs)) => (support, inputs),
            None => (
                &self.config.retriever_support,
                &self.config.retrieval_inputs,
            ),
        };
        let plan = RetrieverPlanner::plan(self.config.retriever_profile, &routes, support, inputs);
        let mut pins = BTreeMap::new();
        let mut route_gaps = Vec::new();
        for route in &routes.routes {
            route_gaps.extend(
                route
                    .unresolved_gaps
                    .iter()
                    .filter(|gap| {
                        !matches!(gap.reason, GapReason::Authority | GapReason::Freshness)
                    })
                    .cloned(),
            );
            if !plan
                .initial_actions
                .iter()
                .chain(&plan.expansion_actions)
                .any(|action| {
                    action.source_id == route.source_id && action.state == ActionState::Planned
                })
            {
                continue;
            }
            let Some(manifest) = self.ports.generations.pin_current(route.source_id).await? else {
                route_gaps.push(source_gap(
                    route.source_id,
                    "projection_generation_unavailable",
                    route.role == SourceRole::Required,
                ));
                continue;
            };
            if manifest.source_id != route.source_id {
                return Err(SearchError::OperationFailed(
                    "pinned generation belongs to another Source".into(),
                ));
            }
            let key = manifest.key();
            let concepts = self.ports.concepts.pin_view(key).await?;
            pins.insert(route.source_id, PinnedSource { key, concepts });
        }

        let mut records = Vec::new();
        let mut attempted_trace = Vec::new();
        let mut action_gaps = Vec::new();
        let mut executed_retrievers = BTreeSet::new();
        let mut attempted_retrievers = BTreeSet::new();
        let mut no_progress = NoProgressHistory::default();
        let mut initial: VecDeque<_> = plan
            .initial_actions
            .iter()
            .filter(|action| action.state == ActionState::Planned)
            .cloned()
            .collect();
        let mut performed = 0;
        let mut evaluation = self
            .evaluate(
                &request,
                &routes,
                &plan,
                &pins,
                &mut records,
                &executed_retrievers,
                &attempted_trace,
                &route_gaps,
                &action_gaps,
                body,
            )
            .await?;

        while !complete(&evaluation.result) && performed < self.config.max_actions {
            let selected = if let Some(action) = initial.pop_front() {
                Some((NextAction::Retrieval(action), None))
            } else {
                self.select_expansion(
                    &evaluation,
                    &plan,
                    &pins,
                    &attempted_retrievers,
                    &no_progress,
                    &records,
                )?
            };
            let Some((next, no_progress_key)) = selected else {
                break;
            };
            let before = known_state_digest(&records, &evaluation.result.evidence_set)?;
            match next {
                NextAction::Retrieval(action) => {
                    attempted_retrievers.insert(action.retriever_id.clone());
                    if let Some(pin) = pins.get(&action.source_id) {
                        if matches!(
                            action.retriever,
                            RetrieverKind::Vector
                                | RetrieverKind::RemoteEnumeration
                                | RetrieverKind::RemoteQuery
                                | RetrieverKind::DirectAddress
                                | RetrieverKind::LiveOnly
                        ) {
                            action_gaps.push(source_gap(
                                action.source_id,
                                &format!("{:?}_execution_port_unavailable", action.retriever),
                                false,
                            ));
                        } else {
                            let executed = RetrievalExecutor::execute(
                                &self.ports.retrieval,
                                RetrievalExecutionInput {
                                    action: &action,
                                    generation: pin.key,
                                    request: &request,
                                    structured_filters: &self.config.structured_filters,
                                    lexical_query: if body.is_some() {
                                        None
                                    } else {
                                        self.config.lexical_query.as_ref()
                                    },
                                    body_query: body.map(|spec| &spec.query),
                                    graph_plan: self
                                        .config
                                        .retrieval_inputs
                                        .graph_plans
                                        .get(&action.source_id),
                                },
                            )
                            .await;
                            let result = match executed {
                                Ok(result) => Some(result),
                                // A missing port, unpublished body bundle or refusal is a
                                // blocking body gap, never a partial body success.
                                Err(_) if body.is_some() => {
                                    action_gaps.push(InformationGap::new(
                                        "document.body.retrieval_unavailable",
                                        GapReason::Availability,
                                        true,
                                    ));
                                    None
                                }
                                Err(error) => return Err(error),
                            };
                            if let Some(result) = result {
                                // The executor already checks current access. Recheck the
                                // candidate and every Graph path resource before detail I/O.
                                for raw in result.hits {
                                    // Body scope counts only hits that carry a literal Unit span.
                                    if body.is_some() && raw.unit_hit.is_none() {
                                        continue;
                                    }
                                    if !self.currently_allowed(&raw.candidate, &request).await
                                        || !self.graph_paths_currently_allowed(&raw, &request).await
                                    {
                                        continue;
                                    }
                                    let projection =
                                        if let Some(resource) = raw.candidate.resource_ref {
                                            self.ports
                                                .generations
                                                .resource_at(pin.key, resource)
                                                .await?
                                        } else {
                                            None
                                        };
                                    if let Some(ref detail) = projection {
                                        validate_detail(detail, pin.key, resource_of(&raw)?)?;
                                    }
                                    // Probe observations belong to the pinned durable Resource,
                                    // even when retrievers use distinct candidates or locators.
                                    let (probe_facts, probe_outcomes, probe_origins) = records
                                        .iter()
                                        .find(|record| {
                                            same_probe_binding(
                                                &record.raw.candidate,
                                                record.raw.generation,
                                                &raw.candidate,
                                                raw.generation,
                                            )
                                        })
                                        .map(|record| {
                                            (
                                                record.probe_facts.clone(),
                                                record.probe_outcomes.clone(),
                                                record.probe_origins.clone(),
                                            )
                                        })
                                        .unwrap_or_default();
                                    records.push(HitRecord {
                                        raw,
                                        projection,
                                        probe_facts,
                                        probe_outcomes,
                                        probe_origins,
                                    });
                                }
                                executed_retrievers.insert(action.retriever_id.clone());
                                attempted_trace.push(format!("retriever:{}", action.retriever_id));
                            }
                        }
                    } else {
                        action_gaps.push(source_gap(
                            action.source_id,
                            "projection_generation_unavailable",
                            source_required(&routes, action.source_id),
                        ));
                    }
                }
                NextAction::Probe(target) => {
                    if let (Some(probe), Some(catalog), Some(policy)) = (
                        self.ports.probe,
                        self.ports.probe_catalog,
                        self.ports.source_policy,
                    ) {
                        let input = ProbeExecutionInput {
                            candidate: target.candidate.clone(),
                            gap: target.gap.clone(),
                            facet: target.gap.required_fact.clone(),
                            access_context: request.access_context.clone(),
                            budget: self.config.probe_budget.clone(),
                        };
                        match ProbeExecutionService::execute(
                            probe,
                            self.ports.retrieval.access,
                            catalog,
                            policy,
                            &input,
                        )
                        .await
                        {
                            Ok(result) => {
                                let fact = result
                                    .facts()
                                    .and_then(|facts| facts.get(&target.gap.required_fact));
                                let observation = match result.outcome() {
                                    ProbeOutcome::Found => Some(ProbeObservation::Found),
                                    ProbeOutcome::NotFoundByProbe => {
                                        result.evidence().map(|evidence| {
                                            ProbeObservation::NotFoundByProbe(
                                                evidence.completeness_semantics,
                                            )
                                        })
                                    }
                                    ProbeOutcome::Unsupported => {
                                        Some(ProbeObservation::Unsupported)
                                    }
                                    ProbeOutcome::Failed => Some(ProbeObservation::Failed),
                                };
                                for record in &mut records {
                                    if same_probe_binding(
                                        &record.raw.candidate,
                                        record.raw.generation,
                                        &target.candidate,
                                        target.generation,
                                    ) {
                                        if let Some(fact) = fact {
                                            record.probe_facts.insert(
                                                target.gap.required_fact.clone(),
                                                fact.clone(),
                                            );
                                        }
                                        if let Some(observation) = observation {
                                            record.probe_outcomes.insert(
                                                target.gap.required_fact.clone(),
                                                observation,
                                            );
                                        }
                                        if fact.is_some() || observation.is_some() {
                                            record.probe_origins.insert(
                                                target.gap.required_fact.clone(),
                                                ProbeOrigin {
                                                    candidate: target.candidate.clone(),
                                                    generation: target.generation,
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                            Err(SearchError::InvalidRequest(_))
                            | Err(SearchError::SourceUnavailable(_)) => {
                                // A denied target is removed by the final visibility pass.
                                action_gaps.push(InformationGap::new(
                                    "probe_execution_unavailable",
                                    GapReason::Availability,
                                    true,
                                ));
                            }
                            Err(error) => return Err(error),
                        }
                    } else {
                        action_gaps.push(InformationGap::new(
                            "probe_execution_port_unavailable",
                            GapReason::Availability,
                            true,
                        ));
                    }
                }
            }
            performed += 1;
            evaluation = self
                .evaluate(
                    &request,
                    &routes,
                    &plan,
                    &pins,
                    &mut records,
                    &executed_retrievers,
                    &attempted_trace,
                    &route_gaps,
                    &action_gaps,
                    body,
                )
                .await?;
            if let Some(key) = no_progress_key
                && known_state_digest(&records, &evaluation.result.evidence_set)? == before
            {
                no_progress.mark(key);
            }
        }

        if !complete(&evaluation.result) {
            for action in plan.initial_actions.iter().chain(&plan.expansion_actions) {
                if action.state != ActionState::Planned {
                    let reason = format!(
                        "source:{:?}:retriever:{:?}:{:?}",
                        action.source_id, action.retriever, action.state
                    );
                    push_gap(
                        &mut evaluation.result.unresolved_gaps,
                        InformationGap::new(
                            reason,
                            GapReason::UnsupportedCoverage,
                            source_required(&routes, action.source_id),
                        ),
                    );
                }
            }
            if performed >= self.config.max_actions {
                push_gap(
                    &mut evaluation.result.unresolved_gaps,
                    InformationGap::new(
                        "max_discovery_actions_reached",
                        GapReason::Availability,
                        true,
                    ),
                );
            }
            if !matches!(
                evaluation.result.evidence_sufficiency,
                EvidenceSufficiency::Conflicted | EvidenceSufficiency::Invalid
            ) {
                evaluation.result.evidence_sufficiency = EvidenceSufficiency::Unresolved;
            }
        }
        if let Some(spec) = body {
            restrict_to_body_hits(&mut evaluation.result, &records);
            if let Some(claim_id) = spec.exact_text_claim {
                self.exact_absence(
                    &mut evaluation.result,
                    &request,
                    &pins,
                    &records,
                    spec,
                    claim_id,
                )
                .await?;
            }
            for pin in pins.values() {
                let gaps = match self.body_coverage {
                    Some(port) => {
                        port.coverage_gaps(&request, pin.key)
                            .await
                            .unwrap_or_else(|_| {
                                vec![InformationGap::new(
                                    "document.body.coverage_unavailable",
                                    GapReason::Availability,
                                    true,
                                )]
                            })
                    }
                    None => vec![InformationGap::new(
                        "document.body.coverage_unverified",
                        GapReason::UnsupportedCoverage,
                        true,
                    )],
                };
                for gap in gaps {
                    push_gap(&mut evaluation.result.unresolved_gaps, gap);
                }
            }
            // A verified positive Claim may coexist with these gaps; the corpus
            // as a whole is not complete while any of them blocks.
            if evaluation.result.evidence_sufficiency == EvidenceSufficiency::Sufficient
                && evaluation
                    .result
                    .unresolved_gaps
                    .iter()
                    .any(|gap| gap.blocking)
            {
                evaluation.result.evidence_sufficiency = EvidenceSufficiency::Unresolved;
            }
        }
        Ok(evaluation.result)
    }

    /// A finite Source-owned scan may turn the still-Unknown exact Claim into
    /// `Absent`, but only for the selector parent, only when no Unit of that
    /// parent was hit, and only with a proof bound to this Claim and literal.
    async fn exact_absence(
        &self,
        result: &mut DiscoveryResult,
        request: &DiscoveryRequest,
        pins: &BTreeMap<SourceId, PinnedSource>,
        records: &[HitRecord],
        spec: &BodySearchSpec,
        claim_id: ClaimId,
    ) -> Result<(), SearchError> {
        let (Some(absence), Some(exact)) = (self.absence, self.exact_text) else {
            return Ok(());
        };
        if result
            .evidence_set
            .iter()
            .any(|claim| claim.claim_id == claim_id && claim.state != ClaimState::Unknown)
        {
            return Ok(());
        }
        let literal = normalize_unit_text(&spec.query.text);
        for pin in pins.values() {
            let Ok(Some(selector)) = exact.selector_for(pin.key, claim_id).await else {
                continue;
            };
            if selector.claim_id != claim_id
                || selector.predicate != CONTAINS_EXACT_PREDICATE
                || selector.expected_exact_text != literal
                || records.iter().any(|record| {
                    record
                        .raw
                        .unit_hit
                        .as_ref()
                        .is_some_and(|unit| unit.parent_resource == selector.parent_resource)
                })
            {
                continue;
            }
            let pinned = match absence.pin_body(pin.key).await {
                Ok(Some(pinned)) if pinned.generation == pin.key => pinned,
                _ => {
                    push_gap(
                        &mut result.unresolved_gaps,
                        InformationGap::new(
                            "document.body.absence_unavailable",
                            GapReason::Availability,
                            true,
                        ),
                    );
                    continue;
                }
            };
            let outcome = absence
                .verify_absence(
                    request,
                    &pinned,
                    &selector,
                    ExactScanBudget::initial(std::time::Instant::now()),
                )
                .await;
            let gap = match outcome {
                Ok(ExactTextAbsenceOutcome::ProvenAbsent(proof))
                    if proof_bound(&proof, claim_id, &selector, &pinned) =>
                {
                    result
                        .evidence_set
                        .retain(|claim| claim.claim_id != claim_id);
                    result.evidence_set.push(absent_claim(&proof, &selector));
                    result.evidence_sufficiency = assess_claim_evidence(
                        &request.need.completion_requirement,
                        &result.evidence_set,
                    )?;
                    return Ok(());
                }
                Ok(ExactTextAbsenceOutcome::ProvenAbsent(_)) => InformationGap::new(
                    "document.body.absence_unbound",
                    GapReason::Availability,
                    true,
                ),
                // A literal the index did not return: an integrity/recall signal only.
                Ok(ExactTextAbsenceOutcome::MatchFound) => InformationGap::new(
                    "document.body.recall_mismatch",
                    GapReason::Availability,
                    true,
                ),
                Ok(ExactTextAbsenceOutcome::Unknown(mut gap)) => {
                    gap.blocking = true;
                    gap
                }
                Err(_) => InformationGap::new(
                    "document.body.absence_unavailable",
                    GapReason::Availability,
                    true,
                ),
            };
            push_gap(&mut result.unresolved_gaps, gap);
        }
        Ok(())
    }

    /// Exact-text Claim for one qualified parent: the trusted selector must name
    /// this parent and the request literal, and the owning Source must verify the
    /// retained Unit hit. Every failure leaves the Claim to the Unknown fallback.
    async fn exact_text_claim(
        &self,
        generation: ProjectionGenerationKey,
        resource: ResourceId,
        claim_id: ClaimId,
        spec: &BodySearchSpec,
        request: &DiscoveryRequest,
        records: &[HitRecord],
    ) -> Option<Claim> {
        let port = self.exact_text?;
        let hit: &KnowledgeUnitHitRef = records
            .iter()
            .filter_map(|record| record.raw.unit_hit.as_ref())
            .find(|unit| unit.generation == generation && unit.parent_resource == resource)?;
        let selector = port.selector_for(generation, claim_id).await.ok()??;
        if selector.claim_id != claim_id
            || selector.parent_resource != resource
            || selector.predicate != CONTAINS_EXACT_PREDICATE
            || normalize_unit_text(&spec.query.text) != selector.expected_exact_text
        {
            return None;
        }
        let verified = port.resolve_hit(request, hit, &selector).await.ok()??;
        let claim = assemble_verified_unit_text_claim(claim_id, &selector, &verified);
        (claim.state == ClaimState::Supported).then_some(claim)
    }

    async fn currently_allowed(
        &self,
        candidate: &search_core::discovery::FederatedCandidate,
        request: &DiscoveryRequest,
    ) -> bool {
        matches!(
            self.ports
                .retrieval
                .access
                .evaluate(candidate, &request.access_context)
                .await,
            Ok(AccessDecision::Allowed)
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn evaluate(
        &self,
        request: &DiscoveryRequest,
        routes: &SourceRoutePlan,
        plan: &crate::retrieval::RetrievalPlan,
        pins: &BTreeMap<SourceId, PinnedSource>,
        records: &mut Vec<HitRecord>,
        executed: &BTreeSet<String>,
        attempted: &[String],
        route_gaps: &[InformationGap],
        action_gaps: &[InformationGap],
        body: Option<&BodySearchSpec>,
    ) -> Result<Evaluation, SearchError> {
        // The exact-text Claim is assembled only from a verified Unit span; the
        // stored-Assertion path keeps every other required Claim.
        let exact_claim = body.and_then(|spec| spec.exact_text_claim);
        let generic_requirement = match exact_claim {
            None => Some(request.need.completion_requirement.clone()),
            Some(exact) => {
                let mut requirement = request.need.completion_requirement.clone();
                requirement.required_claims.retain(|claim| *claim != exact);
                (!requirement.required_claims.is_empty()).then_some(requirement)
            }
        };
        loop {
            let mut visible = Vec::new();
            for record in records.drain(..) {
                if self.currently_allowed(&record.raw.candidate, request).await
                    && self
                        .graph_paths_currently_allowed(&record.raw, request)
                        .await
                {
                    visible.push(record);
                }
            }
            *records = visible;
            retain_visible_probe_origins(records);

            let ordered_actions: Vec<_> = plan
                .initial_actions
                .iter()
                .chain(&plan.expansion_actions)
                .collect();
            let mut hard_gates = records
                .iter()
                .map(|record| {
                    let pin = pins.get(&record.raw.candidate.source_ref).ok_or_else(|| {
                        SearchError::OperationFailed("retrieval hit has no pinned Source".into())
                    })?;
                    Ok(self.hard_gates(record, pin, request))
                })
                .collect::<Result<Vec<_>, SearchError>>()?;
            reconcile_durable_hard_gates(records, &mut hard_gates);
            let mut lists = Vec::new();
            let mut retrieval_trace = attempted.to_vec();
            for action in ordered_actions {
                let Some(pin) = pins.get(&action.source_id) else {
                    continue;
                };
                let hits: Vec<_> = records
                    .iter()
                    .zip(&hard_gates)
                    .filter(|(record, _)| record.raw.retriever_id == action.retriever_id)
                    .enumerate()
                    .map(|(visible_index, (record, gates))| {
                        if let Some(paths) = &record.raw.graph_paths {
                            retrieval_trace.push(format!(
                                "graph_path:{}:{}",
                                record.raw.candidate.candidate_id,
                                serde_json::to_string(paths).unwrap_or_default()
                            ));
                        }
                        retrieval_trace.push(format!(
                            "candidate:{}:{}:{}",
                            action.retriever_id,
                            visible_index + 1,
                            record.raw.candidate.candidate_id
                        ));
                        RankedCandidateHit {
                            candidate: record.raw.candidate.clone(),
                            hard_gates: gates.clone(),
                            identity_evidence: vec![],
                            raw_score: None,
                            evidence_refs: vec![],
                        }
                    })
                    .collect();
                if !hits.is_empty() {
                    lists.push(RetrieverRankList {
                        retriever_id: action.retriever_id.clone(),
                        generation: pin.key,
                        hits,
                    });
                }
            }
            let federation = CandidateFederator::merge(&lists, FusionStrategy::PriorityConcat)
                .map_err(|error| SearchError::OperationFailed(error.to_string()))?;

            let mut claims = Vec::new();
            let mut qualified = Vec::new();
            let mut seen_resources = BTreeSet::new();
            let mut qualification_trace = Vec::new();
            for group in &federation.ranked {
                for hit in &group.hits {
                    let candidate = &hit.candidate;
                    let Some(resource) = candidate.resource_ref else {
                        continue;
                    };
                    let Some(mut qualified_resource) =
                        hit.hard_gates.applicability.qualify(candidate)
                    else {
                        continue;
                    };
                    let Some(pin) = pins.get(&candidate.source_ref) else {
                        continue;
                    };
                    if !self.currently_allowed(candidate, request).await {
                        continue;
                    }
                    if !seen_resources.insert((pin.key, resource)) {
                        continue;
                    }
                    let mut resource_claims = match &generic_requirement {
                        Some(requirement) => {
                            assemble_resource_claims(
                                pin.key,
                                resource,
                                requirement,
                                self.ports.selectors,
                                self.ports.assertions,
                                self.ports.evidence,
                            )
                            .await?
                        }
                        None => Vec::new(),
                    };
                    if let (Some(exact), Some(spec)) = (exact_claim, body)
                        && let Some(claim) = self
                            .exact_text_claim(pin.key, resource, exact, spec, request, records)
                            .await
                    {
                        resource_claims.push(claim);
                    }
                    qualified_resource.evidence_refs = resource_claims
                        .iter()
                        .flat_map(|claim| {
                            claim
                                .evidence_refs
                                .iter()
                                .filter_map(|evidence| evidence.evidence_ref.clone())
                        })
                        .collect();
                    claims.extend(resource_claims);
                    qualification_trace.push(format!("qualified:{}", candidate.candidate_id));
                    qualified.push(qualified_resource);
                }
            }
            // Source-owned evidence reads above are async. Access can change while
            // they run, so no Claim, rank, path, or qualification trace may leave
            // this evaluation until every contributing hit is visible again.
            let mut revoked = Vec::new();
            for record in records.iter() {
                if !self.currently_allowed(&record.raw.candidate, request).await
                    || !self
                        .graph_paths_currently_allowed(&record.raw, request)
                        .await
                {
                    revoked.push(record.raw.candidate.clone());
                }
            }
            if !revoked.is_empty() {
                records.retain(|record| {
                    !revoked
                        .iter()
                        .any(|candidate| same_candidate(&record.raw.candidate, candidate))
                });
                continue;
            }
            // Probe outcomes are emitted only after the final current-access
            // pass. Provider reason strings never enter the result.
            let mut probe_trace = BTreeSet::new();
            for record in records.iter() {
                for (facet, outcome) in &record.probe_outcomes {
                    if let Some(origin) = record.probe_origins.get(facet) {
                        probe_trace.insert(format!(
                            "probe:{}:{}:{}:{}",
                            origin.candidate.source_ref.as_uuid(),
                            origin.candidate.candidate_id,
                            facet,
                            outcome.code()
                        ));
                    }
                }
            }
            retrieval_trace.extend(probe_trace);
            if let Some(exact) = exact_claim
                && !claims.iter().any(|claim| claim.claim_id == exact)
            {
                // No verified span: the Claim stays Unknown without saying why.
                claims.push(Claim::new(exact, ClaimState::Unknown));
            }
            let sufficiency = assess_claim_evidence(&request.need.completion_requirement, &claims)?;
            let mut gaps = route_gaps.to_vec();
            // A visible factual conflict remains an InformationGap even when an
            // independent hard gate rejects that Resource. Federation exposes
            // rejected reasons but does not carry their gaps into the result.
            for gates in &hard_gates {
                for gap in gates
                    .applicability
                    .gaps
                    .iter()
                    .chain(&gates.structured.gaps)
                    .filter(|gap| gap.reason == GapReason::Conflict)
                {
                    push_gap(&mut gaps, gap.clone());
                }
            }
            for gap in action_gaps {
                let mut result_gap = gap.clone();
                if sufficiency == EvidenceSufficiency::Sufficient
                    && matches!(
                        gap.required_fact.as_str(),
                        "probe_execution_unavailable" | "probe_execution_port_unavailable"
                    )
                {
                    result_gap.blocking = false;
                }
                push_gap(&mut gaps, result_gap);
            }
            for route in &routes.routes {
                if route.role == SourceRole::Required
                    && !executed
                        .iter()
                        .any(|id| id.starts_with(&format!("{}:", route.source_id.as_uuid())))
                {
                    push_gap(
                        &mut gaps,
                        source_gap(route.source_id, "required_source_not_executed", true),
                    );
                }
            }
            let mut probes = Vec::new();
            for pending in &federation.pending {
                for hit in &pending.hits {
                    for gap in &hit.gaps {
                        let mut result_gap = gap.clone();
                        if sufficiency == EvidenceSufficiency::Sufficient
                            && gap.reason != GapReason::Conflict
                        {
                            // An alternate candidate's unresolved hard gate does not
                            // block evidence already supplied by a qualified one.
                            result_gap.blocking = false;
                        }
                        push_gap(&mut gaps, result_gap.clone());
                        if result_gap.blocking
                            && result_gap.reason != GapReason::Conflict
                            && self.config.discriminators.iter().any(|rule| {
                                rule.importance == DiscriminatorImportance::Hard
                                    && rule.facet == gap.required_fact
                            })
                        {
                            probes.push(ProbeTarget {
                                candidate: hit.hit.candidate.clone(),
                                generation: hit.hit.trace.generation,
                                gap: gap.clone(),
                            });
                        }
                    }
                    qualification_trace.push(format!("pending:{}", hit.hit.candidate.candidate_id));
                }
            }
            for rejected in &federation.rejected {
                qualification_trace.push(format!("rejected:{}", rejected.rejection.candidate_id));
            }
            if sufficiency != EvidenceSufficiency::Sufficient {
                for claim in &request.need.completion_requirement.required_claims {
                    push_gap(
                        &mut gaps,
                        InformationGap::new(
                            format!("claim:{}", claim.as_uuid()),
                            if sufficiency == EvidenceSufficiency::Conflicted {
                                GapReason::Conflict
                            } else {
                                GapReason::MissingFact
                            },
                            true,
                        ),
                    );
                }
            }
            for item in request
                .need
                .authority_requirements
                .iter()
                .chain(&request.need.completion_requirement.authority_requirements)
            {
                push_gap(
                    &mut gaps,
                    InformationGap::new(format!("authority:{item}"), GapReason::Authority, true),
                );
            }
            for item in request
                .need
                .freshness_requirements
                .iter()
                .chain(&request.need.completion_requirement.freshness_requirements)
            {
                push_gap(
                    &mut gaps,
                    InformationGap::new(format!("freshness:{item}"), GapReason::Freshness, true),
                );
            }
            for constraint in &request.need.constraints {
                push_gap(
                    &mut gaps,
                    InformationGap::new(
                        format!("constraint:{constraint}"),
                        GapReason::MissingFact,
                        true,
                    ),
                );
            }
            let source_trace = routes
                .routes
                .iter()
                .map(|route| {
                    format!(
                        "source:{:?}:{:?}:{:?}",
                        route.source_id, route.role, route.state
                    )
                })
                .collect();
            return Ok(Evaluation {
                result: DiscoveryResult {
                    discovery_evaluation_id: request.temporal_context.evaluation_id,
                    need: request.need.clone(),
                    qualified_resources: qualified,
                    evidence_set: claims,
                    evidence_sufficiency: sufficiency,
                    unresolved_gaps: gaps,
                    rejected_candidates: federation
                        .rejected
                        .into_iter()
                        .map(|item| item.rejection)
                        .collect(),
                    source_trace,
                    retrieval_trace,
                    qualification_trace,
                },
                probes,
            });
        }
    }

    fn hard_gates(
        &self,
        record: &HitRecord,
        pin: &PinnedSource,
        request: &DiscoveryRequest,
    ) -> CandidateHardGates {
        // Conflicting source observations cannot become a single qualifying Fact.
        let conflicts = factual_conflict_facets(record, &self.config.structured_filters);
        let facts = facts_for(record);
        let mut applicability = crate::qualification::QualificationService::qualify_candidate(
            &record.raw.candidate,
            &facts,
            &self.config.discriminators,
            pin.concepts.as_ref(),
        );
        let mut structured = structured_gate(
            record,
            &facts,
            &self.config.structured_filters,
            &request.need.required_resource_types,
        );
        for facet in conflicts {
            let reason = format!("{facet}: factual conflict");
            let affected_rules: Vec<_> = self
                .config
                .discriminators
                .iter()
                .filter(|rule| {
                    rule.importance == DiscriminatorImportance::Hard
                        && (rule.facet == facet || predicate_uses_fact(&rule.predicate, &facet))
                })
                .collect();
            if !affected_rules.is_empty() {
                applicability.gaps.retain(|gap| {
                    gap.reason == GapReason::Conflict
                        || !affected_rules
                            .iter()
                            .any(|rule| rule.facet == gap.required_fact)
                });
                push_gap(
                    &mut applicability.gaps,
                    InformationGap::new(facet.clone(), GapReason::Conflict, true),
                );
                applicability.state =
                    combine_state(applicability.state, ApplicabilityState::Unresolved);
                applicability.reasons.push(reason.clone());
            }
            if self
                .config
                .structured_filters
                .iter()
                .any(|filter| filter.facet == facet)
            {
                structured
                    .gaps
                    .retain(|gap| gap.required_fact != facet || gap.reason == GapReason::Conflict);
                push_gap(
                    &mut structured.gaps,
                    InformationGap::new(facet.clone(), GapReason::Conflict, true),
                );
                structured.state = combine_state(structured.state, ApplicabilityState::Unresolved);
                structured.reasons.push(reason);
            }
        }
        if let Some(detail) = &record.projection {
            for (facet, state) in &detail.structured.typed_facets {
                if !matches!(state, FacetState::NotApplicable)
                    || !self.config.discriminators.iter().any(|rule| {
                        rule.importance == DiscriminatorImportance::Hard
                            && (rule.facet == *facet || predicate_uses_fact(&rule.predicate, facet))
                    })
                {
                    continue;
                }
                applicability.gaps.retain(|gap| gap.required_fact != *facet);
                applicability.state =
                    combine_state(applicability.state, ApplicabilityState::Excluded);
                applicability
                    .reasons
                    .push(format!("{facet}: not applicable"));
            }
        }
        let temporal = temporal_gate(
            record.projection.as_ref(),
            request,
            self.config.temporal_policy,
        );
        CandidateHardGates {
            applicability,
            structured,
            access: gate(ApplicabilityState::Applicable, vec![], vec![]),
            temporal,
        }
    }

    async fn graph_paths_currently_allowed(
        &self,
        raw: &RawRetrievalHit,
        request: &DiscoveryRequest,
    ) -> bool {
        let Some(paths) = &raw.graph_paths else {
            return true;
        };
        let Some(port) = self.ports.retrieval.graph_resource_access else {
            return false;
        };
        let mut resources = BTreeSet::new();
        for path in paths {
            resources.extend(path.resource_path.iter().copied());
            for step in &path.steps {
                resources.insert(step.from_resource);
                resources.insert(step.to_resource);
                resources.extend(
                    step.participants
                        .iter()
                        .map(|participant| participant.resource_ref),
                );
            }
        }
        for resource in resources {
            if !matches!(
                port.evaluate(resource, &request.access_context).await,
                Ok(AccessDecision::Allowed)
            ) {
                return false;
            }
        }
        true
    }

    fn select_expansion(
        &self,
        evaluation: &Evaluation,
        plan: &crate::retrieval::RetrievalPlan,
        pins: &BTreeMap<SourceId, PinnedSource>,
        attempted: &BTreeSet<String>,
        history: &NoProgressHistory,
        records: &[HitRecord],
    ) -> Result<Option<(NextAction, Option<NoProgressKey>)>, SearchError> {
        let mut choices: BTreeMap<String, NextAction> = BTreeMap::new();
        let mut actions = Vec::new();
        let gap = evaluation
            .result
            .unresolved_gaps
            .iter()
            .find(|gap| gap.blocking)
            .cloned()
            .unwrap_or_else(|| InformationGap::new("evidence", GapReason::MissingFact, true));
        for action in &plan.expansion_actions {
            if action.state != ActionState::Planned
                || attempted.contains(&action.retriever_id)
                || !pins.contains_key(&action.source_id)
            {
                continue;
            }
            let id = action.retriever_id.clone();
            let candidate = ActionCandidate::new(
                id.clone(),
                &gap,
                ActionPriority {
                    authority_or_freshness_necessity: matches!(
                        gap.reason,
                        GapReason::Authority | GapReason::Freshness
                    ),
                    expected_gap_resolution: 1,
                    critical_need_impact: 1,
                    cost: unknown_cost(),
                },
            );
            choices.insert(id, NextAction::Retrieval(action.clone()));
            actions.push(candidate);
        }
        for probe in &evaluation.probes {
            let id = format!(
                "probe:{}:{}:{}",
                probe.candidate.source_ref.as_uuid(),
                probe.candidate.candidate_id,
                probe.gap.required_fact
            );
            if choices.contains_key(&id) {
                continue;
            }
            let candidate = ActionCandidate::new(
                id.clone(),
                &probe.gap,
                ActionPriority {
                    authority_or_freshness_necessity: false,
                    expected_gap_resolution: 3,
                    critical_need_impact: 2,
                    cost: unknown_cost(),
                },
            );
            choices.insert(id, NextAction::Probe(Box::new(probe.clone())));
            actions.push(candidate);
        }
        let digest = known_state_digest(records, &evaluation.result.evidence_set)?;
        match select_next_action(&actions, digest, history, &self.config.evaluation_currency)
            .map_err(|error| {
                SearchError::InvalidRequest(format!("invalid Discovery action set: {error:?}"))
            })? {
            Selection::Exhausted => Ok(None),
            Selection::Selected(candidate) => {
                let key = NoProgressKey::for_action(candidate, digest);
                let action = choices
                    .remove(candidate.action_id())
                    .expect("selected action exists");
                Ok(Some((action, Some(key))))
            }
        }
    }
}

fn validate_request(request: &DiscoveryRequest) -> Result<(), SearchError> {
    if request.access_context.trim().is_empty()
        || request
            .need
            .completion_requirement
            .required_claims
            .is_empty()
        || request.need.required_claims.is_empty()
        || request
            .need
            .required_claims
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            != request
                .need
                .completion_requirement
                .required_claims
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
    {
        return Err(SearchError::InvalidRequest(
            "Discovery requires current access and the same nonempty required Claims in Need and completion".into(),
        ));
    }
    Ok(())
}

fn validate_detail(
    detail: &CompiledResourceProjection,
    key: ProjectionGenerationKey,
    resource: ResourceId,
) -> Result<(), SearchError> {
    if detail.manifest.key() != key
        || detail.directory.resource_ref != resource
        || detail.structured.resource_ref != resource
        || detail.temporal.resource_ref != resource
        || detail.access.resource_ref != resource
    {
        return Err(SearchError::OperationFailed(
            "projection detail does not match pinned Resource and generation".into(),
        ));
    }
    Ok(())
}

fn resource_of(hit: &RawRetrievalHit) -> Result<ResourceId, SearchError> {
    hit.candidate.resource_ref.ok_or_else(|| {
        SearchError::OperationFailed(
            "projection detail was returned for a reference-only candidate".into(),
        )
    })
}

fn facts_for(record: &HitRecord) -> FactSet {
    let mut facts = FactSet::default();
    let conflicts = conflicting_probe_facets(record);
    if let Some(detail) = &record.projection {
        for (facet, state) in &detail.structured.typed_facets {
            if let FacetState::Known(value) = state
                && !conflicts.contains(facet)
            {
                facts.insert(
                    facet.clone(),
                    Fact::new(value.clone(), facet_origin(detail, facet, value)),
                );
            }
        }
    }
    for (facet, fact) in &record.probe_facts {
        let definitive_nonfact = record.projection.as_ref().is_some_and(|detail| {
            matches!(
                detail.structured.typed_facets.get(facet),
                Some(FacetState::Conflict | FacetState::NotApplicable)
            )
        });
        if !definitive_nonfact && !conflicts.contains(facet) && !facts.contains(facet) {
            facts.insert(facet.clone(), fact.clone());
        }
    }
    facts
}

fn factual_conflict_facets(
    record: &HitRecord,
    filters: &[StructuredFacetFilter],
) -> BTreeSet<String> {
    let mut conflicts: BTreeSet<_> = record
        .projection
        .as_ref()
        .into_iter()
        .flat_map(|detail| &detail.structured.typed_facets)
        .filter_map(|(facet, state)| matches!(state, FacetState::Conflict).then_some(facet.clone()))
        .collect();
    if let Some(outcomes) = &record.raw.structured_outcomes {
        conflicts.extend(
            filters
                .iter()
                .zip(outcomes)
                .filter_map(|(filter, outcome)| {
                    matches!(outcome, StructuredFacetOutcome::Conflict)
                        .then_some(filter.facet.clone())
                }),
        );
    }
    conflicts.extend(conflicting_probe_facets(record));
    conflicts
}

fn conflicting_probe_facets(record: &HitRecord) -> Vec<String> {
    let Some(detail) = &record.projection else {
        return Vec::new();
    };
    record
        .probe_facts
        .iter()
        .filter_map(
            |(facet, probe)| match detail.structured.typed_facets.get(facet) {
                Some(FacetState::Known(projected))
                    if !claim_values_semantically_equal(projected, &probe.value) =>
                {
                    Some(facet.clone())
                }
                _ => None,
            },
        )
        .collect()
}

fn predicate_uses_fact(predicate: &PredicateExpr, facet: &str) -> bool {
    let mut pending = vec![predicate];
    while let Some(item) = pending.pop() {
        match item {
            PredicateExpr::And(items) | PredicateExpr::Or(items) => pending.extend(items),
            PredicateExpr::Not(inner) => pending.push(inner),
            PredicateExpr::Exists(key) | PredicateExpr::Missing(key) if key == facet => {
                return true;
            }
            PredicateExpr::Eq(left, right)
            | PredicateExpr::Ne(left, right)
            | PredicateExpr::Lt(left, right)
            | PredicateExpr::Lte(left, right)
            | PredicateExpr::Gt(left, right)
            | PredicateExpr::Gte(left, right)
            | PredicateExpr::In(left, right)
            | PredicateExpr::Contains(left, right)
            | PredicateExpr::Intersects(left, right)
            | PredicateExpr::Subset(left, right)
            | PredicateExpr::SameConcept(left, right)
            | PredicateExpr::IsA(left, right)
            | PredicateExpr::DescendantOf(left, right)
                if matches!(left, Operand::Fact(key) if key == facet)
                    || matches!(right, Operand::Fact(key) if key == facet) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn facet_origin(
    detail: &CompiledResourceProjection,
    facet: &str,
    value: &TypedValue,
) -> FactOrigin {
    let resolved = detail
        .structured
        .authority_resolutions
        .get(facet)
        .is_some_and(|resolution| {
            matches!(resolution, AuthorityResolution::Resolved(selected)
            if claim_values_semantically_equal(selected, value))
        });
    if !resolved {
        return FactOrigin::Derived;
    }
    detail
        .structured
        .assertions
        .iter()
        .filter(|assertion| {
            assertion.predicate == facet
                && !assertion.source_ref.trim().is_empty()
                && !assertion.authority_scope.trim().is_empty()
                && claim_values_semantically_equal(&assertion.value, value)
        })
        .map(|assertion| match assertion.origin {
            AssertionOrigin::Authoritative => FactOrigin::Authoritative,
            AssertionOrigin::Declared | AssertionOrigin::Curated => FactOrigin::Explicit,
            AssertionOrigin::Observed | AssertionOrigin::Extracted => FactOrigin::Observed,
            AssertionOrigin::Derived => FactOrigin::Derived,
            AssertionOrigin::Inferred => FactOrigin::Inferred,
        })
        .max_by_key(|origin| match origin {
            FactOrigin::Authoritative => 5,
            FactOrigin::Explicit => 4,
            FactOrigin::Observed => 3,
            FactOrigin::Derived => 2,
            FactOrigin::Inferred => 1,
        })
        .unwrap_or(FactOrigin::Derived)
}

fn structured_gate(
    record: &HitRecord,
    facts: &FactSet,
    filters: &[StructuredFacetFilter],
    required_types: &[search_core::resource::ResourceKind],
) -> HardGateEvaluation {
    let Some(detail) = &record.projection else {
        let mut state = ApplicabilityState::Unresolved;
        let mut reasons = vec!["projection detail unavailable".into()];
        let mut gaps = vec![InformationGap::new(
            "resource_projection",
            GapReason::Availability,
            true,
        )];
        if let Some(outcomes) = &record.raw.structured_outcomes {
            for (filter, outcome) in filters.iter().zip(outcomes) {
                match outcome {
                    StructuredFacetOutcome::Conflict => {
                        gaps.push(InformationGap::new(
                            filter.facet.clone(),
                            GapReason::Conflict,
                            true,
                        ));
                        reasons.push(format!("{}: factual conflict", filter.facet));
                    }
                    StructuredFacetOutcome::Mismatch | StructuredFacetOutcome::NotApplicable => {
                        state = ApplicabilityState::Excluded;
                        reasons.push(format!("{}: {state:?}", filter.facet));
                    }
                    StructuredFacetOutcome::Match | StructuredFacetOutcome::Unknown => {}
                }
            }
        }
        return gate(state, reasons, gaps);
    };
    if !required_types.is_empty() && !required_types.contains(&detail.directory.kind) {
        let conflicts = filters
            .iter()
            .enumerate()
            .filter(|(index, filter)| {
                matches!(
                    detail.structured.typed_facets.get(&filter.facet),
                    Some(FacetState::Conflict)
                ) || matches!(
                    record
                        .raw
                        .structured_outcomes
                        .as_ref()
                        .and_then(|outcomes| outcomes.get(*index)),
                    Some(StructuredFacetOutcome::Conflict)
                )
            })
            .map(|(_, filter)| InformationGap::new(filter.facet.clone(), GapReason::Conflict, true))
            .collect();
        return gate(
            ApplicabilityState::Excluded,
            vec!["resource type mismatch".into()],
            conflicts,
        );
    }
    let mut reasons = Vec::new();
    let mut gaps = Vec::new();
    let mut state = ApplicabilityState::Applicable;
    for (index, filter) in filters.iter().enumerate() {
        let projected = detail.structured.typed_facets.get(&filter.facet);
        let fact = facts.get(&filter.facet);
        let from_projection = match projected {
            Some(FacetState::Known(_)) => match fact {
                Some(fact) if claim_values_semantically_equal(&fact.value, &filter.expected) => {
                    ApplicabilityState::Applicable
                }
                Some(_) => ApplicabilityState::Excluded,
                None => ApplicabilityState::Unresolved,
            },
            Some(FacetState::NotApplicable) => ApplicabilityState::Excluded,
            Some(FacetState::Conflict) => ApplicabilityState::Unresolved,
            Some(FacetState::Unknown) | None => match fact {
                Some(fact) if claim_values_semantically_equal(&fact.value, &filter.expected) => {
                    ApplicabilityState::Applicable
                }
                Some(_) => ApplicabilityState::Excluded,
                None => ApplicabilityState::Unresolved,
            },
        };
        let from_port = record
            .raw
            .structured_outcomes
            .as_ref()
            .and_then(|outcomes| outcomes.get(index));
        let checked = match from_port {
            Some(StructuredFacetOutcome::Mismatch | StructuredFacetOutcome::NotApplicable) => {
                ApplicabilityState::Excluded
            }
            Some(StructuredFacetOutcome::Conflict) => ApplicabilityState::Unresolved,
            Some(StructuredFacetOutcome::Unknown)
                if from_projection == ApplicabilityState::Applicable
                    && !record.probe_facts.contains_key(&filter.facet) =>
            {
                ApplicabilityState::Unresolved
            }
            _ => from_projection,
        };
        state = combine_state(state, checked);
        let conflict = matches!(projected, Some(FacetState::Conflict))
            || matches!(from_port, Some(StructuredFacetOutcome::Conflict));
        if conflict {
            gaps.push(InformationGap::new(
                filter.facet.clone(),
                GapReason::Conflict,
                true,
            ));
        } else if checked == ApplicabilityState::Unresolved {
            gaps.push(InformationGap::new(
                filter.facet.clone(),
                GapReason::MissingFact,
                true,
            ));
        }
        if checked != ApplicabilityState::Applicable {
            reasons.push(format!("{}: {checked:?}", filter.facet));
        }
    }
    gate(state, reasons, gaps)
}

fn temporal_gate(
    projection: Option<&CompiledResourceProjection>,
    request: &DiscoveryRequest,
    policy: TemporalPolicy,
) -> HardGateEvaluation {
    let Some(detail) = projection else {
        return gate(
            ApplicabilityState::Unresolved,
            vec!["temporal projection unavailable".into()],
            vec![InformationGap::new(
                "temporal_projection",
                GapReason::Availability,
                true,
            )],
        );
    };
    let target = request.temporal_context.temporal_target;
    let temporal = &detail.temporal;
    let from = [temporal.valid_from, temporal.profile.effective_from]
        .into_iter()
        .flatten()
        .max();
    let to = [temporal.valid_to, temporal.profile.effective_to]
        .into_iter()
        .flatten()
        .min();
    if from.is_some_and(|value| target < value) || to.is_some_and(|value| target >= value) {
        return gate(
            ApplicabilityState::Excluded,
            vec!["outside effective interval".into()],
            vec![],
        );
    }
    let mut gaps = Vec::new();
    if policy.require_effective_at_target && from.is_none() && to.is_none() {
        gaps.push(InformationGap::new(
            "effective_at_target",
            GapReason::MissingFact,
            true,
        ));
    }
    if let Some(max_age_seconds) = policy.max_current_age_seconds {
        let anchor = temporal.profile.freshness_anchor_at;
        let evaluated = request.temporal_context.evaluated_at.unix_timestamp_nanos();
        match anchor {
            Some(anchor) if anchor.unix_timestamp_nanos() <= evaluated => {
                let age = evaluated - anchor.unix_timestamp_nanos();
                if age > i128::from(max_age_seconds) * 1_000_000_000 {
                    return gate(
                        ApplicabilityState::Excluded,
                        vec!["stale temporal profile".into()],
                        vec![],
                    );
                }
            }
            _ => gaps.push(InformationGap::new(
                "current_freshness_anchor",
                GapReason::Freshness,
                true,
            )),
        }
    }
    if gaps.is_empty() {
        gate(ApplicabilityState::Applicable, vec![], gaps)
    } else {
        gate(
            ApplicabilityState::Unresolved,
            vec!["temporal requirement unresolved".into()],
            gaps,
        )
    }
}

fn gate(
    state: ApplicabilityState,
    reasons: Vec<String>,
    gaps: Vec<InformationGap>,
) -> HardGateEvaluation {
    HardGateEvaluation {
        state,
        reasons,
        gaps,
    }
}

fn combine_state(left: ApplicabilityState, right: ApplicabilityState) -> ApplicabilityState {
    use ApplicabilityState::{Applicable, Excluded, Invalid, Unresolved};
    match (left, right) {
        (Invalid, _) | (_, Invalid) => Invalid,
        (Excluded, _) | (_, Excluded) => Excluded,
        (Unresolved, _) | (_, Unresolved) => Unresolved,
        _ => Applicable,
    }
}

fn proof_bound(
    proof: &crate::body_ports::ExactTextNegativeProof,
    claim_id: ClaimId,
    selector: &ExactTextSelector,
    pinned: &PinnedBodyBundle,
) -> bool {
    let expected: [u8; 32] = Sha256::digest(selector.expected_exact_text.as_bytes()).into();
    proof.claim_id() == claim_id
        && proof.parent().resource_id == selector.parent_resource
        && proof.parent().source_id == pinned.generation.source_id
        && proof.generation() == pinned.generation
        && proof.bundle_digest() == pinned.composite_digest
        && proof.exact_text_sha256() == expected
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn absent_claim(
    proof: &crate::body_ports::ExactTextNegativeProof,
    selector: &ExactTextSelector,
) -> Claim {
    let parent = proof.parent().resource_id.as_uuid();
    let generation = proof.generation();
    let (items, units) = proof.binding_digests();
    let mut claim = Claim::new(proof.claim_id(), ClaimState::Absent);
    claim.subject = Some(parent.to_string());
    claim.predicate = Some(proof.predicate().into());
    claim.value = Some(TypedValue::String(selector.expected_exact_text.clone()));
    let mut evidence = EvidenceReference::new(
        generation.source_id,
        format!(
            "body-absence:v1:{}:{}:{parent}",
            generation.source_id.as_uuid(),
            generation.generation_id.as_uuid()
        ),
        EvidenceRole::Primary,
    );
    evidence.evidence_ref = Some(format!("body-absence-receipt:v1:{}", hex(&units)));
    evidence.content_digest = Some(format!("sha256:{}", hex(&items)));
    claim.evidence_refs.push(evidence);
    claim
}

/// Body scope counts only Resources reached through a verified Unit hit. Title
/// Assertions or Graph paths never make a body result complete on their own.
fn restrict_to_body_hits(result: &mut DiscoveryResult, records: &[HitRecord]) {
    let parents: BTreeSet<ResourceId> = records
        .iter()
        .filter_map(|record| record.raw.unit_hit.as_ref())
        .map(|unit| unit.parent_resource)
        .collect();
    result
        .qualified_resources
        .retain(|qualified| parents.contains(&qualified.resource_ref));
    if result.qualified_resources.is_empty() {
        push_gap(
            &mut result.unresolved_gaps,
            InformationGap::new(
                "document.body.match_unproven",
                GapReason::UnsupportedCoverage,
                true,
            ),
        );
        if result.evidence_sufficiency == EvidenceSufficiency::Sufficient {
            result.evidence_sufficiency = EvidenceSufficiency::Unresolved;
        }
    }
    result.qualification_trace.push(format!(
        "content_scope:body_required:unit_hits:{}",
        records
            .iter()
            .filter(|record| record.raw.unit_hit.is_some())
            .count()
    ));
}

fn complete(result: &DiscoveryResult) -> bool {
    result.evidence_sufficiency == EvidenceSufficiency::Sufficient
        && !result.unresolved_gaps.iter().any(|gap| gap.blocking)
}

fn source_required(routes: &SourceRoutePlan, source: SourceId) -> bool {
    routes
        .routes
        .iter()
        .any(|route| route.source_id == source && route.role == SourceRole::Required)
}

fn source_gap(source: SourceId, detail: &str, blocking: bool) -> InformationGap {
    InformationGap::new(
        format!("source:{}:{detail}", source.as_uuid()),
        GapReason::Availability,
        blocking,
    )
}

fn push_gap(gaps: &mut Vec<InformationGap>, gap: InformationGap) {
    if !gaps.iter().any(|known| {
        known.required_fact == gap.required_fact
            && known.reason == gap.reason
            && known.blocking == gap.blocking
    }) {
        gaps.push(gap);
    }
}

fn retain_visible_probe_origins(records: &mut [HitRecord]) {
    // These hits just passed current access and Graph path checks. A Probe
    // observation may be shared with another locator only while the exact
    // candidate that was probed remains visible at its pinned generation.
    let visible_targets: Vec<_> = records
        .iter()
        .map(|record| (record.raw.candidate.clone(), record.raw.generation))
        .collect();
    for record in records {
        record.probe_origins.retain(|_, origin| {
            visible_targets.iter().any(|(candidate, generation)| {
                *generation == origin.generation && same_candidate(candidate, &origin.candidate)
            })
        });
        record
            .probe_facts
            .retain(|facet, _| record.probe_origins.contains_key(facet));
        record
            .probe_outcomes
            .retain(|facet, _| record.probe_origins.contains_key(facet));
    }
}

fn durable_resource_key(
    candidate: &FederatedCandidate,
    generation: ProjectionGenerationKey,
) -> Option<(ProjectionGenerationKey, ResourceId)> {
    (candidate.identity_class == CandidateIdentityClass::DurableResource
        && candidate.source_ref == generation.source_id)
        .then_some(candidate.resource_ref)
        .flatten()
        .map(|resource| (generation, resource))
}

fn same_probe_binding(
    left: &FederatedCandidate,
    left_generation: ProjectionGenerationKey,
    right: &FederatedCandidate,
    right_generation: ProjectionGenerationKey,
) -> bool {
    if left_generation != right_generation {
        return false;
    }
    match (
        durable_resource_key(left, left_generation),
        durable_resource_key(right, right_generation),
    ) {
        (Some(left_key), Some(right_key)) => left_key == right_key,
        _ => same_candidate(left, right),
    }
}

fn extend_unique<T: Clone + PartialEq>(target: &mut Vec<T>, source: &[T]) {
    for item in source {
        if !target.contains(item) {
            target.push(item.clone());
        }
    }
}

fn reconcile_durable_hard_gates(records: &[HitRecord], gates: &mut [CandidateHardGates]) {
    let mut resource_hits = BTreeMap::<(ProjectionGenerationKey, ResourceId), Vec<usize>>::new();
    for (index, record) in records.iter().enumerate() {
        if let Some(key) = durable_resource_key(&record.raw.candidate, record.raw.generation) {
            resource_hits.entry(key).or_default().push(index);
        }
    }
    for indexes in resource_hits.values() {
        let mut applicability_state = ApplicabilityState::Applicable;
        let mut structured_state = ApplicabilityState::Applicable;
        let mut applicability_reasons = Vec::new();
        let mut applicability_gaps = Vec::new();
        let mut structured_reasons = Vec::new();
        let mut structured_gaps = Vec::new();
        for &index in indexes {
            let hit = &gates[index];
            applicability_state = combine_state(applicability_state, hit.applicability.state);
            structured_state = combine_state(structured_state, hit.structured.state);
            extend_unique(&mut applicability_reasons, &hit.applicability.reasons);
            extend_unique(&mut applicability_gaps, &hit.applicability.gaps);
            extend_unique(&mut structured_reasons, &hit.structured.reasons);
            extend_unique(&mut structured_gaps, &hit.structured.gaps);
        }
        for &index in indexes {
            let hit = &mut gates[index];
            hit.applicability.state = applicability_state;
            hit.applicability.reasons.clone_from(&applicability_reasons);
            hit.applicability.gaps.clone_from(&applicability_gaps);
            hit.structured.state = structured_state;
            hit.structured.reasons.clone_from(&structured_reasons);
            hit.structured.gaps.clone_from(&structured_gaps);
        }
    }
}

fn same_candidate(left: &FederatedCandidate, right: &FederatedCandidate) -> bool {
    left.source_ref == right.source_ref
        && left.candidate_id == right.candidate_id
        && left.resource_ref == right.resource_ref
        && left.identity_class == right.identity_class
        && left.locator == right.locator
}

fn unknown_cost() -> ActionCostEstimate {
    ActionCostEstimate {
        remote_calls: None,
        monetary_cost_minor_units: None,
        currency: None,
        latency_ms: None,
        content_bytes: None,
    }
}

fn known_state_digest(
    records: &[HitRecord],
    claims: &[Claim],
) -> Result<KnownStateDigest, SearchError> {
    let mut known = records
        .iter()
        .map(|record| {
            // Exclude rank, locator, transient trace refs, elapsed time and attempt
            // history. Only stable target identity plus known Facts and Claim
            // evidence can distinguish a progressed evaluation.
            serde_json::to_string(&(
                record.raw.generation,
                record.raw.candidate.source_ref,
                record.raw.candidate.resource_ref,
                &record.raw.candidate.candidate_id,
                facts_for(record),
            ))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
    for claim in claims {
        let mut canonical = claim.clone();
        let mut evidence_refs = claim
            .evidence_refs
            .iter()
            .map(|reference| serde_json::to_string(reference).map(|key| (key, reference.clone())))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
        evidence_refs.sort_by(|left, right| left.0.cmp(&right.0));
        canonical.evidence_refs = evidence_refs
            .into_iter()
            .map(|(_, reference)| reference)
            .collect();
        known.push(
            serde_json::to_string(&canonical)
                .map_err(|error| SearchError::OperationFailed(error.to_string()))?,
        );
    }
    known.sort();
    known.dedup();
    let mut hasher = Sha256::new();
    for item in known {
        hasher.update(item.len().to_le_bytes());
        hasher.update(item.as_bytes());
    }
    Ok(KnownStateDigest::from_bytes(hasher.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_core::id::ProjectionGenerationId;
    use uuid::Uuid;

    #[test]
    fn probe_binding_is_scoped_to_pinned_source_resource_and_generation() {
        let source = SourceId::from_uuid(Uuid::from_u128(1));
        let other_source = SourceId::from_uuid(Uuid::from_u128(2));
        let resource = ResourceId::from_uuid(Uuid::from_u128(10));
        let other_resource = ResourceId::from_uuid(Uuid::from_u128(11));
        let generation = ProjectionGenerationKey {
            source_id: source,
            generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(5)),
        };
        let next_generation = ProjectionGenerationKey {
            generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(6)),
            ..generation
        };
        let mut first = FederatedCandidate::new(
            "structured-hit",
            CandidateIdentityClass::DurableResource,
            source,
            "structured",
        );
        first.resource_ref = Some(resource);
        first.locator = Some("private://first".into());
        let mut second = first.clone();
        second.candidate_id = "directory-hit".into();
        second.locator = Some("private://second".into());

        assert!(same_probe_binding(&first, generation, &second, generation));
        second.resource_ref = Some(other_resource);
        assert!(!same_probe_binding(&first, generation, &second, generation));
        second.resource_ref = Some(resource);
        assert!(!same_probe_binding(
            &first,
            generation,
            &second,
            next_generation
        ));
        second.source_ref = other_source;
        let other_source_generation = ProjectionGenerationKey {
            source_id: other_source,
            generation_id: generation.generation_id,
        };
        assert!(!same_probe_binding(
            &first,
            generation,
            &second,
            other_source_generation
        ));
        second.source_ref = source;
        second.identity_class = CandidateIdentityClass::RemoteStableReference;
        assert!(!same_probe_binding(&first, generation, &second, generation));
    }
}
