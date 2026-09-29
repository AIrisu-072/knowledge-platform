use std::collections::{BTreeMap, BTreeSet};

use search_application::SearchError;
use search_application::ports::{
    AccessDecision, BoxFuture, GraphRetrievalHit, GraphRetrievalResult, HyperGraphRetrieverPort,
};
use search_core::discovery::{CandidateIdentityClass, FederatedCandidate};
use search_core::graph::{GraphPathEvidence, GraphPathStepEvidence, GraphTraversalPlan};
use search_core::id::ResourceId;
use search_core::projection::ProjectionGenerationKey;

use crate::index::{GenerationIndex, MemoryGraphRetriever};

#[derive(Clone)]
struct PartialPath {
    current: ResourceId,
    resource_path: Vec<ResourceId>,
    steps: Vec<GraphPathStepEvidence>,
}

impl MemoryGraphRetriever {
    async fn visible(
        &self,
        index: &GenerationIndex,
        id: ResourceId,
        plan: &GraphTraversalPlan,
        cache: &mut BTreeMap<ResourceId, bool>,
    ) -> Result<bool, SearchError> {
        if let Some(visible) = cache.get(&id) {
            return Ok(*visible);
        }
        let Some(resource) = index.resources.get(&id) else {
            cache.insert(id, false);
            return Ok(false);
        };
        let target = plan
            .temporal_context
            .as_ref()
            .expect("validated traversal has temporal context")
            .temporal_target;
        let temporal = &resource.temporal;
        let temporally_valid = temporal.valid_from.is_none_or(|from| target >= from)
            && temporal.valid_to.is_none_or(|to| target < to)
            && temporal
                .profile
                .effective_from
                .is_none_or(|from| target >= from)
            && temporal.profile.effective_to.is_none_or(|to| target < to);
        // AccessProjection.access_scope is a scope label, while access_context
        // identifies the requester. Comparing the strings would reject valid
        // grants. CurrentAccessEvaluatorPort owns the policy decision.
        if !temporally_valid {
            cache.insert(id, false);
            return Ok(false);
        }
        let visible = matches!(
            self.access.evaluate(id, &plan.access_context).await,
            Ok(AccessDecision::Allowed)
        );
        cache.insert(id, visible);
        Ok(visible)
    }

    async fn traverse(
        &self,
        generation: ProjectionGenerationKey,
        index: &GenerationIndex,
        plan: &GraphTraversalPlan,
    ) -> Result<GraphRetrievalResult, SearchError> {
        plan.validate()
            .map_err(|reason| SearchError::InvalidRequest(reason.into()))?;
        if plan.path_patterns.len() > plan.expansion_budget.max_hops {
            return Err(SearchError::InvalidRequest(
                "graph path exceeds hop budget".into(),
            ));
        }
        let mut visible = BTreeMap::new();
        let mut frontier = Vec::new();
        for seed in plan.seed_nodes.iter().copied().collect::<BTreeSet<_>>() {
            if self.visible(index, seed, plan, &mut visible).await? {
                if frontier
                    .len()
                    .checked_add(1)
                    .is_none_or(|count| count > plan.expansion_budget.max_paths)
                {
                    return Err(SearchError::InvalidRequest(
                        "graph path budget exceeded".into(),
                    ));
                }
                frontier.push(PartialPath {
                    current: seed,
                    resource_path: vec![seed],
                    steps: vec![],
                });
            }
        }
        let mut expanded_relations = 0usize;
        for pattern in &plan.path_patterns {
            let mut next = Vec::new();
            for path in frontier {
                let mut branches = Vec::new();
                let mut relation_expansions = 0usize;
                let key = (path.current, pattern.from_role.clone());
                for relation_id in index.incidence.get(&key).into_iter().flatten() {
                    let relation = index
                        .relations
                        .get(relation_id)
                        .expect("incidence relation exists");
                    if !plan.allows(relation)
                        || !pattern.matches_relation(relation)
                        || pattern.from_resource.is_some_and(|id| id != path.current)
                        || !relation.has_participant(&pattern.from_role, Some(path.current))
                    {
                        continue;
                    }
                    // A n-ary relation is invisible unless every participant is
                    // currently allowed. This also excludes unknown metadata.
                    let mut all_visible = true;
                    for participant in &relation.participants {
                        if !self
                            .visible(index, participant.resource_ref, plan, &mut visible)
                            .await?
                        {
                            all_visible = false;
                            break;
                        }
                    }
                    if !all_visible {
                        continue;
                    }
                    let before_relation = branches.len();
                    for participant in relation.participants.iter().filter(|participant| {
                        participant.role == pattern.to_role
                            && pattern
                                .to_resource
                                .is_none_or(|id| id == participant.resource_ref)
                    }) {
                        let branch_count = branches.len().checked_add(1).ok_or_else(|| {
                            SearchError::InvalidRequest("graph path budget exceeded".into())
                        })?;
                        if branch_count > plan.expansion_budget.max_branching_per_node {
                            return Err(SearchError::InvalidRequest(
                                "graph branching budget exceeded".into(),
                            ));
                        }
                        if next
                            .len()
                            .checked_add(branch_count)
                            .is_none_or(|count| count > plan.expansion_budget.max_paths)
                        {
                            return Err(SearchError::InvalidRequest(
                                "graph path budget exceeded".into(),
                            ));
                        }
                        let target = participant.resource_ref;
                        let mut next_path = path.clone();
                        next_path.current = target;
                        next_path.resource_path.push(target);
                        next_path.steps.push(GraphPathStepEvidence {
                            relation_id: *relation_id,
                            namespace: relation.namespace,
                            relation_type: relation.relation_type.clone(),
                            from_role: pattern.from_role.clone(),
                            from_resource: path.current,
                            to_role: pattern.to_role.clone(),
                            to_resource: target,
                            participants: relation.participants.clone(),
                            evidence_refs: relation.evidence_refs.clone(),
                            provenance: relation.provenance.clone(),
                        });
                        branches.push(next_path);
                    }
                    if branches.len() > before_relation {
                        relation_expansions =
                            relation_expansions.checked_add(1).ok_or_else(|| {
                                SearchError::InvalidRequest("graph relation budget exceeded".into())
                            })?;
                        if expanded_relations
                            .checked_add(relation_expansions)
                            .is_none_or(|total| total > plan.expansion_budget.max_relations)
                        {
                            return Err(SearchError::InvalidRequest(
                                "graph relation budget exceeded".into(),
                            ));
                        }
                    }
                }
                expanded_relations = expanded_relations
                    .checked_add(relation_expansions)
                    .ok_or_else(|| SearchError::InvalidRequest("graph budget exceeded".into()))?;
                next.extend(branches);
            }
            frontier = next;
        }
        let mut grouped: BTreeMap<ResourceId, Vec<GraphPathEvidence>> = BTreeMap::new();
        for path in frontier {
            grouped
                .entry(path.current)
                .or_default()
                .push(GraphPathEvidence {
                    resource_path: path.resource_path,
                    steps: path.steps,
                });
        }
        let hits = grouped
            .into_iter()
            .map(|(id, mut paths)| {
                paths.sort();
                paths.dedup();
                let mut candidate = FederatedCandidate::new(
                    format!("{}:{}", generation.source_id.as_uuid(), id.as_uuid()),
                    CandidateIdentityClass::DurableResource,
                    generation.source_id,
                    "hypergraph",
                );
                candidate.resource_ref = Some(id);
                candidate.retrieval_trace_ref = Some(format!(
                    "{}:{}",
                    generation.source_id.as_uuid(),
                    generation.generation_id.as_uuid()
                ));
                GraphRetrievalHit { candidate, paths }
            })
            .collect();
        Ok(GraphRetrievalResult { generation, hits })
    }
}

impl HyperGraphRetrieverPort for MemoryGraphRetriever {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        plan: &'a GraphTraversalPlan,
    ) -> BoxFuture<'a, GraphRetrievalResult> {
        Box::pin(async move {
            let index = self
                .generations
                .read()
                .map_err(|_| SearchError::OperationFailed("graph index lock is poisoned".into()))?
                .get(&generation)
                .cloned()
                .ok_or_else(|| SearchError::InvalidRequest("unknown graph generation".into()))?;
            self.traverse(generation, &index, plan).await
        })
    }
}
