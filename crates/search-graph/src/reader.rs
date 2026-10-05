//! P3-G07: pinned Graph read. The server-issued lease is verified before a
//! `REPEATABLE READ READ ONLY` snapshot and again, in a new DB-clock
//! transaction, before anything is returned; the lease struct alone is never
//! authority. Traversal keeps the memory oracle's `GraphTraversalPlan`
//! semantics: every participant must be visible before a relation counts
//! against the visible relation, branch and path budgets, so hidden relations
//! cannot change a public result. An access error or `Unknown` decision fails
//! the whole read rather than returning partial hits. The incidence scan has
//! no physical cap of its own; hidden-degree timing remains a residual risk
//! for the P5 rate and concurrency limits.

use std::collections::{BTreeMap, BTreeSet};

use search_application::SearchError;
use search_application::graph_generation::{
    GenerationScopedGraphAccessPort, GraphLeaseVerifierPort, GraphReadLease,
    PinnedGraphRetrievalPort,
};
use search_application::ports::{
    AccessDecision, BoxFuture, GraphRetrievalHit, GraphRetrievalResult,
};
use search_application::scoped::{AuthorizedSourceScope, TrustedDiscoveryBinding};
use search_core::discovery::{CandidateIdentityClass, FederatedCandidate};
use search_core::graph::{GraphPathEvidence, GraphPathStepEvidence, GraphTraversalPlan};
use search_core::id::{RelationId, ResourceId};
use search_core::projection::{ProjectionGenerationKey, TemporalProjection};
use search_core::relation::TypedRelationInstance;
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

use crate::GraphError;
use crate::store::{relation_from_row, resource_columns, resource_from_row};

#[derive(Clone)]
struct PartialPath {
    current: ResourceId,
    resource_path: Vec<ResourceId>,
    steps: Vec<GraphPathStepEvidence>,
}

fn invalid(reason: &str) -> SearchError {
    SearchError::InvalidRequest(reason.into())
}

fn unavailable() -> SearchError {
    SearchError::SourceUnavailable("graph read unavailable".into())
}

/// Rows of one READY generation read inside a single snapshot.
struct Snapshot<'c> {
    connection: &'c mut PgConnection,
    key: ProjectionGenerationKey,
    relations: BTreeMap<RelationId, TypedRelationInstance>,
    temporal: BTreeMap<ResourceId, Option<TemporalProjection>>,
}

impl Snapshot<'_> {
    /// Relation IDs incident to each resource in `role`, in relation-ID order.
    async fn incidence(
        &mut self,
        resources: &BTreeSet<ResourceId>,
        role: &str,
    ) -> Result<BTreeMap<ResourceId, Vec<RelationId>>, GraphError> {
        let ids: Vec<Uuid> = resources.iter().map(|id| id.as_uuid()).collect();
        let rows = sqlx::query(
            "SELECT DISTINCT resource_id, relation_id FROM search_graph.participant \
             WHERE source_id=$1 AND generation_id=$2 AND role=$3 AND resource_id=ANY($4) \
             ORDER BY resource_id, relation_id",
        )
        .bind(self.key.source_id.as_uuid())
        .bind(self.key.generation_id.as_uuid())
        .bind(role)
        .bind(&ids)
        .fetch_all(&mut *self.connection)
        .await?;
        let mut incidence: BTreeMap<ResourceId, Vec<RelationId>> = BTreeMap::new();
        for row in rows {
            incidence
                .entry(ResourceId::from_uuid(row.try_get("resource_id")?))
                .or_default()
                .push(RelationId::from_uuid(row.try_get("relation_id")?));
        }
        Ok(incidence)
    }

    /// Loads relations not yet cached, checking payload digest and that the
    /// participant rows are exactly the payload participants.
    async fn load_relations(&mut self, wanted: &BTreeSet<RelationId>) -> Result<(), GraphError> {
        let ids: Vec<Uuid> = wanted
            .iter()
            .filter(|id| !self.relations.contains_key(id))
            .map(|id| id.as_uuid())
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        let rows = sqlx::query(
            "SELECT relation_id, payload, canonical_digest FROM search_graph.relation \
             WHERE source_id=$1 AND generation_id=$2 AND relation_id=ANY($3)",
        )
        .bind(self.key.source_id.as_uuid())
        .bind(self.key.generation_id.as_uuid())
        .bind(&ids)
        .fetch_all(&mut *self.connection)
        .await?;
        let participant_rows = sqlx::query(
            "SELECT relation_id, role, resource_id FROM search_graph.participant \
             WHERE source_id=$1 AND generation_id=$2 AND relation_id=ANY($3) \
             ORDER BY relation_id, ordinal",
        )
        .bind(self.key.source_id.as_uuid())
        .bind(self.key.generation_id.as_uuid())
        .bind(&ids)
        .fetch_all(&mut *self.connection)
        .await?;
        if rows.len() != ids.len() {
            return Err(GraphError::Integrity("incident relation row"));
        }
        let mut stored: BTreeMap<Uuid, Vec<(String, Uuid)>> = BTreeMap::new();
        for row in participant_rows {
            stored
                .entry(row.try_get("relation_id")?)
                .or_default()
                .push((row.try_get("role")?, row.try_get("resource_id")?));
        }
        for row in &rows {
            let mut relation = relation_from_row(row)?;
            let expected: Vec<(String, Uuid)> = relation
                .participants
                .iter()
                .map(|participant| (participant.role.clone(), participant.resource_ref.as_uuid()))
                .collect();
            if stored.remove(&relation.relation_id.as_uuid()) != Some(expected) {
                return Err(GraphError::Integrity("participant incidence"));
            }
            // The same normalization as the memory oracle's build step.
            relation.participants.sort();
            relation.evidence_refs.sort();
            relation.evidence_refs.dedup();
            self.relations.insert(relation.relation_id, relation);
        }
        Ok(())
    }

    /// Loads temporal metadata for resources not yet cached; a resource that
    /// is not part of this generation is recorded as absent.
    async fn load_resources(&mut self, wanted: &BTreeSet<ResourceId>) -> Result<(), GraphError> {
        let ids: Vec<Uuid> = wanted
            .iter()
            .filter(|id| !self.temporal.contains_key(id))
            .map(|id| id.as_uuid())
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        let rows = sqlx::query(concat!(
            "SELECT ",
            resource_columns!(),
            " FROM search_graph.resource \
             WHERE source_id=$1 AND generation_id=$2 AND resource_id=ANY($3)"
        ))
        .bind(self.key.source_id.as_uuid())
        .bind(self.key.generation_id.as_uuid())
        .bind(&ids)
        .fetch_all(&mut *self.connection)
        .await?;
        for id in &ids {
            self.temporal.insert(ResourceId::from_uuid(*id), None);
        }
        for row in &rows {
            let record = resource_from_row(row, Vec::new())?;
            self.temporal
                .insert(record.resource_ref, Some(record.temporal));
        }
        Ok(())
    }
}

/// Reads one pinned READY generation. Holds borrowed ports: the P7 lease
/// verifier and the generation-scoped current access gate.
pub struct PostgresGraphReader<'r> {
    pool: PgPool,
    access: &'r dyn GenerationScopedGraphAccessPort,
    verifier: &'r dyn GraphLeaseVerifierPort,
}

struct Request<'a> {
    key: ProjectionGenerationKey,
    plan: &'a GraphTraversalPlan,
    binding: &'a TrustedDiscoveryBinding,
    scope: &'a AuthorizedSourceScope,
}

impl<'r> PostgresGraphReader<'r> {
    pub fn new(
        pool: PgPool,
        access: &'r dyn GenerationScopedGraphAccessPort,
        verifier: &'r dyn GraphLeaseVerifierPort,
    ) -> Self {
        Self {
            pool,
            access,
            verifier,
        }
    }

    async fn visible(
        &self,
        snapshot: &mut Snapshot<'_>,
        request: &Request<'_>,
        id: ResourceId,
        cache: &mut BTreeMap<ResourceId, bool>,
    ) -> Result<bool, SearchError> {
        if let Some(visible) = cache.get(&id) {
            return Ok(*visible);
        }
        snapshot.load_resources(&BTreeSet::from([id])).await?;
        let Some(Some(temporal)) = snapshot.temporal.get(&id) else {
            cache.insert(id, false);
            return Ok(false);
        };
        let target = request
            .plan
            .temporal_context
            .as_ref()
            .ok_or_else(|| invalid("traversal requires access and temporal context"))?
            .temporal_target;
        let temporally_valid = temporal.valid_from.is_none_or(|from| target >= from)
            && temporal.valid_to.is_none_or(|to| target < to)
            && temporal
                .profile
                .effective_from
                .is_none_or(|from| target >= from)
            && temporal.profile.effective_to.is_none_or(|to| target < to);
        if !temporally_valid {
            cache.insert(id, false);
            return Ok(false);
        }
        let visible = match self
            .access
            .evaluate(&request.key, id, request.binding, request.scope)
            .await?
        {
            AccessDecision::Allowed => true,
            AccessDecision::Denied => false,
            AccessDecision::Unknown => return Err(unavailable()),
        };
        cache.insert(id, visible);
        Ok(visible)
    }

    async fn traverse(
        &self,
        snapshot: &mut Snapshot<'_>,
        request: &Request<'_>,
    ) -> Result<Vec<PartialPath>, SearchError> {
        let plan = request.plan;
        let mut visible = BTreeMap::new();
        let mut frontier = Vec::new();
        let seeds: BTreeSet<ResourceId> = plan.seed_nodes.iter().copied().collect();
        snapshot.load_resources(&seeds).await?;
        for seed in seeds {
            if self.visible(snapshot, request, seed, &mut visible).await? {
                if frontier
                    .len()
                    .checked_add(1)
                    .is_none_or(|count| count > plan.expansion_budget.max_paths)
                {
                    return Err(invalid("graph path budget exceeded"));
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
            let currents: BTreeSet<ResourceId> = frontier.iter().map(|path| path.current).collect();
            let incidence = snapshot.incidence(&currents, &pattern.from_role).await?;
            let wanted: BTreeSet<RelationId> = incidence.values().flatten().copied().collect();
            snapshot.load_relations(&wanted).await?;
            let members: BTreeSet<ResourceId> = wanted
                .iter()
                .filter_map(|id| snapshot.relations.get(id))
                .filter(|relation| plan.allows(relation) && pattern.matches_relation(relation))
                .flat_map(|relation| relation.participants.iter().map(|p| p.resource_ref))
                .collect();
            snapshot.load_resources(&members).await?;

            let mut next = Vec::new();
            for path in frontier {
                let mut branches = Vec::new();
                let mut relation_expansions = 0usize;
                for relation_id in incidence.get(&path.current).into_iter().flatten() {
                    let relation = snapshot
                        .relations
                        .get(relation_id)
                        .cloned()
                        .ok_or_else(unavailable)?;
                    if !plan.allows(&relation)
                        || !pattern.matches_relation(&relation)
                        || pattern.from_resource.is_some_and(|id| id != path.current)
                        || !relation.has_participant(&pattern.from_role, Some(path.current))
                    {
                        continue;
                    }
                    // An n-ary relation is invisible unless every participant
                    // is currently allowed; it then consumes no budget.
                    let mut all_visible = true;
                    for participant in &relation.participants {
                        if !self
                            .visible(snapshot, request, participant.resource_ref, &mut visible)
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
                        let branch_count = branches
                            .len()
                            .checked_add(1)
                            .ok_or_else(|| invalid("graph path budget exceeded"))?;
                        if branch_count > plan.expansion_budget.max_branching_per_node {
                            return Err(invalid("graph branching budget exceeded"));
                        }
                        if next
                            .len()
                            .checked_add(branch_count)
                            .is_none_or(|count| count > plan.expansion_budget.max_paths)
                        {
                            return Err(invalid("graph path budget exceeded"));
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
                        relation_expansions = relation_expansions
                            .checked_add(1)
                            .ok_or_else(|| invalid("graph relation budget exceeded"))?;
                        if expanded_relations
                            .checked_add(relation_expansions)
                            .is_none_or(|total| total > plan.expansion_budget.max_relations)
                        {
                            return Err(invalid("graph relation budget exceeded"));
                        }
                    }
                }
                expanded_relations = expanded_relations
                    .checked_add(relation_expansions)
                    .ok_or_else(|| invalid("graph budget exceeded"))?;
                next.extend(branches);
            }
            frontier = next;
        }
        Ok(frontier)
    }

    async fn read_snapshot(&self, request: &Request<'_>) -> Result<Vec<PartialPath>, SearchError> {
        let mut tx = self.pool.begin().await.map_err(GraphError::from)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(GraphError::from)?;
        let state: Option<String> = sqlx::query_scalar(
            "SELECT state FROM search_graph.generation WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(request.key.source_id.as_uuid())
        .bind(request.key.generation_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(GraphError::from)?;
        if state.as_deref() != Some("READY") {
            return Err(unavailable());
        }
        let mut snapshot = Snapshot {
            connection: &mut tx,
            key: request.key,
            relations: BTreeMap::new(),
            temporal: BTreeMap::new(),
        };
        let paths = self.traverse(&mut snapshot, request).await?;
        tx.rollback().await.map_err(GraphError::from)?;
        Ok(paths)
    }

    pub async fn retrieve(
        &self,
        lease: &GraphReadLease,
        plan: &GraphTraversalPlan,
        binding: &TrustedDiscoveryBinding,
        scope: &AuthorizedSourceScope,
    ) -> Result<GraphRetrievalResult, SearchError> {
        plan.validate().map_err(invalid)?;
        if plan.path_patterns.len() > plan.expansion_budget.max_hops {
            return Err(invalid("graph path exceeds hop budget"));
        }
        let key = lease.key();
        if key.source_id != scope.source_id()
            || lease.evaluation_id() != binding.evaluation()
            || binding.actor() != scope.actor()
        {
            return Err(unavailable());
        }
        self.verifier.verify(lease, binding, scope).await?;
        let request = Request {
            key,
            plan,
            binding,
            scope,
        };
        let frontier = self.read_snapshot(&request).await?;
        // A new DB-clock transaction: an expired or revoked pin discards all hits.
        self.verifier.verify(lease, binding, scope).await?;

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
                    format!("{}:{}", key.source_id.as_uuid(), id.as_uuid()),
                    CandidateIdentityClass::DurableResource,
                    key.source_id,
                    "hypergraph",
                );
                candidate.resource_ref = Some(id);
                candidate.retrieval_trace_ref = Some(format!(
                    "{}:{}",
                    key.source_id.as_uuid(),
                    key.generation_id.as_uuid()
                ));
                GraphRetrievalHit { candidate, paths }
            })
            .collect();
        Ok(GraphRetrievalResult {
            generation: key,
            hits,
        })
    }
}

impl PinnedGraphRetrievalPort for PostgresGraphReader<'_> {
    fn retrieve_pinned<'a>(
        &'a self,
        lease: &'a GraphReadLease,
        plan: &'a GraphTraversalPlan,
        binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, GraphRetrievalResult> {
        Box::pin(self.retrieve(lease, plan, binding, scope))
    }
}
