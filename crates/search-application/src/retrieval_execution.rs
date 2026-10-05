//! Execute one planned, source-local retrieval action without qualifying its hits.

use std::collections::{BTreeMap, BTreeSet};

use search_core::discovery::{DiscoveryRequest, FederatedCandidate};
use search_core::graph::{
    GraphPathEvidence, GraphPathStepEvidence, GraphTraversalPlan, TraversalBudget,
};
use search_core::id::{RelationId, ResourceId};
use search_core::projection::ProjectionGenerationKey;
use search_core::relation::{RelationNamespace, RelationParticipant};

use crate::body_ports::KnowledgeUnitHitRef;
use crate::error::SearchError;
use crate::ports::{
    AccessDecision, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
    DirectoryRetrieverPort, HyperGraphRetrieverPort, LexicalFieldScope, LexicalQuery,
    LexicalRetrieverPort, StructuredFacetFilter, StructuredFacetOutcome, StructuredRetrieverPort,
};
use crate::qualification::QualificationService;
use crate::retrieval::{ActionState, RetrievalAction, RetrieverKind};

/// The caller installs only adapters available for this evaluation. Access is
/// always current and Source-owned, rather than inferred from a projection.
pub struct RetrievalExecutionPorts<'a> {
    pub directory: Option<&'a dyn DirectoryRetrieverPort>,
    pub structured: Option<&'a dyn StructuredRetrieverPort>,
    pub lexical: Option<&'a dyn LexicalRetrieverPort>,
    /// The trusted Graph adapter must enforce plan authority and temporal
    /// constraints, authorize relation metadata and provenance, and apply the
    /// traversal work budgets. GraphPathEvidence does not carry enough fields
    /// to independently recheck those decisions here. This executor checks
    /// returned path shape/bounds and current Source-owned access to every
    /// resource named anywhere in each path.
    pub hypergraph: Option<&'a dyn HyperGraphRetrieverPort>,
    /// Required for Graph execution. No path leaves this boundary without
    /// current Source-owned access to every path node and relation participant.
    pub graph_resource_access: Option<&'a dyn CurrentAccessEvaluatorPort>,
    pub access: &'a dyn CurrentCandidateAccessEvaluatorPort,
}

pub struct RetrievalExecutionInput<'a> {
    pub action: &'a RetrievalAction,
    pub generation: ProjectionGenerationKey,
    pub request: &'a DiscoveryRequest,
    pub structured_filters: &'a [StructuredFacetFilter],
    pub lexical_query: Option<&'a LexicalQuery>,
    /// Request-scoped BodyOnly query; when present the Lexical arm searches
    /// only Source-owned Units and keeps each Unit hit reference.
    pub body_query: Option<&'a LexicalQuery>,
    pub graph_plan: Option<&'a GraphTraversalPlan>,
}

/// Authorized retriever output, before applicability, evidence, or Primary
/// selection. Rank is one-based among hits visible after current access and
/// Graph path checks; hidden retriever positions must not be disclosed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRetrievalHit {
    pub candidate: FederatedCandidate,
    pub structured_outcomes: Option<Vec<StructuredFacetOutcome>>,
    pub graph_paths: Option<Vec<GraphPathEvidence>>,
    pub retriever_id: String,
    pub generation: ProjectionGenerationKey,
    pub rank: usize,
    pub unit_hit: Option<KnowledgeUnitHitRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalExecutionResult {
    pub hits: Vec<RawRetrievalHit>,
    /// For a BodyOnly action: whether every matching Unit was seen.
    pub body_exhausted: Option<bool>,
}

struct PortHit {
    candidate: FederatedCandidate,
    structured_outcomes: Option<Vec<StructuredFacetOutcome>>,
    graph_paths: Option<Vec<GraphPathEvidence>>,
    graph_generation: Option<ProjectionGenerationKey>,
    unit_hit: Option<KnowledgeUnitHitRef>,
}

pub struct RetrievalExecutor;

impl RetrievalExecutor {
    pub async fn execute(
        ports: &RetrievalExecutionPorts<'_>,
        input: RetrievalExecutionInput<'_>,
    ) -> Result<RetrievalExecutionResult, SearchError> {
        if input.action.state != ActionState::Planned
            || input.action.source_id != input.generation.source_id
            || input.action.retriever_id.is_empty()
            || input.request.access_context.is_empty()
        {
            return Err(SearchError::InvalidRequest(
                "retrieval action is not executable for this Source and request".into(),
            ));
        }

        let mut body_exhausted = None;
        let port_hits: Vec<PortHit> = match input.action.retriever {
            RetrieverKind::Lexical if input.body_query.is_some() => {
                let query = input.body_query.expect("guarded body query");
                if query.field_scope != LexicalFieldScope::BodyOnly
                    || query.text.trim().is_empty()
                    || query.limit == 0
                {
                    return Err(SearchError::InvalidRequest(
                        "body retrieval requires nonempty text and a positive limit".into(),
                    ));
                }
                let port = ports.lexical.ok_or_else(|| unsupported("lexical"))?;
                let batch = port
                    .retrieve_body(input.generation, input.request, query)
                    .await?;
                body_exhausted = Some(batch.exhausted_matching_units);
                let mut body_hits = Vec::with_capacity(batch.hits.len());
                for hit in batch.hits {
                    if let Some(unit) = &hit.unit_hit
                        && (unit.generation != input.generation
                            || hit.candidate.resource_ref != Some(unit.parent_resource))
                    {
                        return Err(SearchError::OperationFailed(
                            "body retriever returned a Unit outside the pinned parent".into(),
                        ));
                    }
                    body_hits.push(PortHit {
                        candidate: hit.candidate,
                        structured_outcomes: None,
                        graph_paths: None,
                        graph_generation: None,
                        unit_hit: hit.unit_hit,
                    });
                }
                body_hits
            }
            RetrieverKind::Directory => {
                let port = ports.directory.ok_or_else(|| unsupported("directory"))?;
                port.retrieve(input.generation, input.request)
                    .await?
                    .into_iter()
                    .map(|candidate| PortHit {
                        candidate,
                        structured_outcomes: None,
                        graph_paths: None,
                        graph_generation: None,
                        unit_hit: None,
                    })
                    .collect()
            }
            RetrieverKind::Structured => {
                let port = ports.structured.ok_or_else(|| unsupported("structured"))?;
                port.retrieve(input.generation, input.request, input.structured_filters)
                    .await?
                    .into_iter()
                    .map(|hit| PortHit {
                        candidate: hit.candidate,
                        structured_outcomes: Some(hit.outcomes),
                        graph_paths: None,
                        graph_generation: None,
                        unit_hit: None,
                    })
                    .collect()
            }
            RetrieverKind::Lexical => {
                let query = input.lexical_query.ok_or_else(|| {
                    SearchError::InvalidRequest("lexical retrieval requires a query".into())
                })?;
                if query.field_scope != LexicalFieldScope::ExistingFields
                    || query.text.trim().is_empty()
                    || query.limit == 0
                {
                    return Err(SearchError::InvalidRequest(
                        "lexical retrieval requires nonempty text and a positive limit".into(),
                    ));
                }
                let port = ports.lexical.ok_or_else(|| unsupported("lexical"))?;
                port.retrieve(input.generation, input.request, query)
                    .await?
                    .into_iter()
                    .map(|candidate| PortHit {
                        candidate,
                        structured_outcomes: None,
                        graph_paths: None,
                        graph_generation: None,
                        unit_hit: None,
                    })
                    .collect()
            }
            RetrieverKind::HyperGraph => {
                let plan = input.graph_plan.ok_or_else(|| {
                    SearchError::InvalidRequest("graph retrieval requires a plan".into())
                })?;
                if plan.access_context != input.request.access_context
                    || plan.temporal_context.as_ref() != Some(&input.request.temporal_context)
                {
                    return Err(SearchError::InvalidRequest(
                        "graph plan is not bound to the current request".into(),
                    ));
                }
                let port = ports.hypergraph.ok_or_else(|| unsupported("hypergraph"))?;
                if ports.graph_resource_access.is_none() {
                    return Err(unsupported("graph path access"));
                }
                let result =
                    QualificationService::candidates_from_graph(port, input.generation, plan)
                        .await?;
                let returned_generation = result.generation;
                result
                    .hits
                    .into_iter()
                    .map(|hit| PortHit {
                        candidate: hit.candidate,
                        structured_outcomes: None,
                        graph_paths: Some(hit.paths),
                        graph_generation: Some(returned_generation),
                        unit_hit: None,
                    })
                    .collect()
            }
            RetrieverKind::Vector
            | RetrieverKind::RemoteEnumeration
            | RetrieverKind::RemoteQuery
            | RetrieverKind::DirectAddress
            | RetrieverKind::LiveOnly => {
                return Err(unsupported("retriever"));
            }
        };

        let mut hits = Vec::new();
        let mut graph_budget = ReturnedGraphBudget::default();
        for hit in port_hits {
            // Denied, Unknown, and evaluator errors have the same silent result:
            // none of the candidate identity, locator, or trace leaves here.
            if !matches!(
                ports
                    .access
                    .evaluate(&hit.candidate, &input.request.access_context)
                    .await,
                Ok(AccessDecision::Allowed)
            ) {
                continue;
            }
            if let Some(paths) = &hit.graph_paths {
                if !graph_paths_currently_allowed(
                    ports
                        .graph_resource_access
                        .expect("graph execution checked for path access port"),
                    paths,
                    &input.request.access_context,
                )
                .await
                {
                    continue;
                }
                let plan = input
                    .graph_plan
                    .expect("graph execution checked for traversal plan");
                if !graph_paths_match_plan(&hit.candidate, paths, plan, &mut graph_budget) {
                    return Err(SearchError::OperationFailed(
                        "graph retriever returned invalid path evidence".into(),
                    ));
                }
            }
            if hit.candidate.source_ref != input.generation.source_id {
                return Err(SearchError::OperationFailed(
                    "retriever returned another Source".into(),
                ));
            }
            if hit
                .graph_generation
                .is_some_and(|returned| returned != input.generation)
            {
                return Err(SearchError::OperationFailed(
                    "graph retriever returned another generation".into(),
                ));
            }
            if hit
                .structured_outcomes
                .as_ref()
                .is_some_and(|outcomes| outcomes.len() != input.structured_filters.len())
            {
                return Err(SearchError::OperationFailed(
                    "structured retriever returned a different outcome count".into(),
                ));
            }
            hits.push(RawRetrievalHit {
                candidate: hit.candidate,
                structured_outcomes: hit.structured_outcomes,
                graph_paths: hit.graph_paths,
                retriever_id: input.action.retriever_id.clone(),
                generation: input.generation,
                rank: hits.len() + 1,
                unit_hit: hit.unit_hit,
            });
        }
        Ok(RetrievalExecutionResult {
            hits,
            body_exhausted,
        })
    }
}

type TraversalPrefix = (ResourceId, Vec<(RelationId, ResourceId)>);
type ReturnedBranch = (RelationId, ResourceId);

#[derive(Default)]
struct ReturnedGraphBudget {
    paths: usize,
    relations: BTreeMap<RelationId, ReturnedRelationMetadata>,
    relation_expansions: BTreeSet<(TraversalPrefix, RelationId)>,
    branches: BTreeMap<TraversalPrefix, BTreeSet<ReturnedBranch>>,
}

#[derive(PartialEq, Eq)]
struct ReturnedRelationMetadata {
    namespace: RelationNamespace,
    relation_type: String,
    participants: Vec<RelationParticipant>,
    evidence_refs: Vec<String>,
    provenance: Option<String>,
}

impl ReturnedRelationMetadata {
    fn from_step(step: &GraphPathStepEvidence) -> Self {
        let mut participants = step.participants.clone();
        participants.sort();
        let mut evidence_refs = step.evidence_refs.clone();
        evidence_refs.sort();
        Self {
            namespace: step.namespace,
            relation_type: step.relation_type.clone(),
            participants,
            evidence_refs,
            provenance: step.provenance.clone(),
        }
    }
}

fn graph_paths_match_plan(
    candidate: &FederatedCandidate,
    paths: &[GraphPathEvidence],
    plan: &GraphTraversalPlan,
    returned: &mut ReturnedGraphBudget,
) -> bool {
    let budget = plan.expansion_budget;
    let Some(endpoint) = candidate.resource_ref else {
        return false;
    };
    let Some(path_count) = returned.paths.checked_add(paths.len()) else {
        return false;
    };
    if paths.is_empty() || path_count > budget.max_paths {
        return false;
    }
    for path in paths {
        let Some(resource_count) = path.steps.len().checked_add(1) else {
            return false;
        };
        if path.steps.is_empty()
            || path.steps.len() != plan.path_patterns.len()
            || path.steps.len() > budget.max_hops
            || path.resource_path.len() != resource_count
            || !plan.seed_nodes.contains(&path.resource_path[0])
            || path.resource_path.last() != Some(&endpoint)
        {
            return false;
        }
        for (index, step) in path.steps.iter().enumerate() {
            let pattern = &plan.path_patterns[index];
            if step.from_resource != path.resource_path[index]
                || step.to_resource != path.resource_path[index + 1]
                || step.namespace != pattern.namespace
                || step.relation_type != pattern.relation_type
                || step.from_role != pattern.from_role
                || step.to_role != pattern.to_role
                || step.relation_type.is_empty()
                || step.from_role.is_empty()
                || step.to_role.is_empty()
                || !plan.allowed_namespaces.contains(&step.namespace)
                || !plan.allowed_relation_types.contains(&step.relation_type)
                || pattern
                    .from_resource
                    .is_some_and(|required| required != step.from_resource)
                || pattern
                    .to_resource
                    .is_some_and(|required| required != step.to_resource)
                || step.participants.len() < 2
                || step
                    .participants
                    .iter()
                    .any(|participant| participant.role.is_empty())
                || !step.participants.iter().any(|participant| {
                    participant.role == step.from_role
                        && participant.resource_ref == step.from_resource
                })
                || !step.participants.iter().any(|participant| {
                    participant.role == step.to_role && participant.resource_ref == step.to_resource
                })
                || !pattern.required_participants.iter().all(|required| {
                    step.participants
                        .iter()
                        .any(|participant| participant == required)
                })
                || !returned.record_step(path, index, &budget)
            {
                return false;
            }
        }
    }
    returned.paths = path_count;
    true
}

impl ReturnedGraphBudget {
    fn record_step(
        &mut self,
        path: &GraphPathEvidence,
        index: usize,
        budget: &TraversalBudget,
    ) -> bool {
        let prefix = (
            path.resource_path[0],
            path.steps[..index]
                .iter()
                .map(|prior| (prior.relation_id, prior.to_resource))
                .collect(),
        );
        let step = &path.steps[index];
        let metadata = ReturnedRelationMetadata::from_step(step);
        if let Some(existing) = self.relations.get(&step.relation_id) {
            if existing != &metadata {
                return false;
            }
        } else {
            self.relations.insert(step.relation_id, metadata);
        }
        self.relation_expansions
            .insert((prefix.clone(), step.relation_id));
        if self.relation_expansions.len() > budget.max_relations {
            return false;
        }
        let branches = self.branches.entry(prefix).or_default();
        branches.insert((step.relation_id, step.to_resource));
        branches.len() <= budget.max_branching_per_node
    }
}

async fn graph_paths_currently_allowed(
    access: &dyn CurrentAccessEvaluatorPort,
    paths: &[GraphPathEvidence],
    access_context: &str,
) -> bool {
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
            access.evaluate(resource, access_context).await,
            Ok(AccessDecision::Allowed)
        ) {
            return false;
        }
    }
    true
}

fn unsupported(kind: &str) -> SearchError {
    SearchError::InvalidRequest(format!("{kind} retrieval has no execution port"))
}
