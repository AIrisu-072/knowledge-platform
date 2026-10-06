//! P5-04: the Discover route over the actor-visible catalog.
//!
//! The route binds one server-issued Discovery evaluation, binds each
//! required Claim through the actor-visible Claim catalog (an unbound Claim
//! stays required and can never be satisfied), and evaluates through
//! `ScopedDiscoveryService` with request-owned selectors: durable Claims via
//! `VisibleClaimSelectors`, remote Claims only for their own Source. Without
//! a Claim catalog the route refuses instead of evaluating partially.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use search_core::discovery::{DiscoveryNeed, DiscoveryRequest, DiscoveryResult};
use search_core::evidence::EvidenceRequirement;
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{ClaimId, DiscoveryEvaluationId, NeedId, ResourceId, SourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::relation::RelationNamespace;
use search_core::resource::ResourceKind;
use search_core::temporal::TemporalEvaluationContext;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::SearchError;
use crate::api_scope::{ApiError, SearchOperationContext};
use crate::content_scope::{BodySearchSpec, DiscoveryScope};
use crate::discovery_service::{DiscoveryConfig, DiscoveryPorts, DiscoveryService};
use crate::ports::{
    AssertionStorePort, ConceptRegistryPort, CurrentSourcePolicyPort, EvidenceResolverPort,
    GenerationReadPort, LexicalQuery, ProbeCapabilityCatalogPort, ProbePort, SourceRegistryPort,
};
use crate::remote_disclosure::{RemoteWiring, ScopedDiscoveryService, TransientDisclosure};
use crate::remote_lease::LeaseClock;
use crate::retrieval_execution::RetrievalExecutionPorts;
use crate::routing::RoutingConstraints;
use crate::scoped::{
    AccessContextAuthorityPort, CurrentSourceVisibilityPort, ScopedSourceRegistryPort,
    VisibleCatalogSnapshot,
};
use crate::search_query::{MAX_QUERY_BYTES, SearchCoverage};
use crate::source_registration::SourceKind;
use crate::visible_claim::{ActorVisibleClaimCatalogPort, VisibleClaimSelectors};

pub const MAX_PURPOSE_BYTES: usize = 512;
pub const MAX_OPTIONAL_INITIAL_SOURCES: usize = 8;

/// The closed public Discover input: no actor, tenant, selector, routing,
/// provider URL or access context.
#[derive(Clone, PartialEq, Eq)]
pub struct DiscoverInput {
    pub purpose: String,
    pub required_resource_types: Vec<ResourceKind>,
    pub required_claim_ids: Vec<ClaimId>,
    pub temporal_target: Option<OffsetDateTime>,
    pub business_timezone: Option<String>,
    pub query: Option<String>,
    pub coverage: SearchCoverage,
    /// B7: one public Graph path request, or none.
    pub graph: Option<DiscoverGraphInput>,
}

/// Seed Resources and one relation path. The server adds the namespace,
/// the actor's access context, the evaluation's temporal context and fixed
/// traversal budgets; a caller never names them.
#[derive(Clone, PartialEq, Eq)]
pub struct DiscoverGraphInput {
    pub seed_resource_ids: Vec<ResourceId>,
    pub relation_type: String,
    pub from_role: String,
    pub to_role: String,
    pub max_hops: usize,
}

pub const MAX_GRAPH_SEEDS: usize = 8;
pub const MAX_GRAPH_HOPS: usize = 3;

/// A relation type or role: a short lowercase token.
pub fn graph_token_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(byte))
}

impl DiscoverGraphInput {
    pub fn validate(&self) -> Result<(), ApiError> {
        let mut seeds = self.seed_resource_ids.clone();
        seeds.sort();
        seeds.dedup();
        if self.seed_resource_ids.is_empty()
            || self.seed_resource_ids.len() > MAX_GRAPH_SEEDS
            || seeds.len() != self.seed_resource_ids.len()
            || !graph_token_ok(&self.relation_type)
            || !graph_token_ok(&self.from_role)
            || !graph_token_ok(&self.to_role)
            || !(1..=MAX_GRAPH_HOPS).contains(&self.max_hops)
        {
            return Err(ApiError::ValidationFailed);
        }
        Ok(())
    }

    fn plan(
        &self,
        access_context: String,
        temporal_context: TemporalEvaluationContext,
    ) -> GraphTraversalPlan {
        GraphTraversalPlan {
            seed_nodes: self.seed_resource_ids.clone(),
            path_patterns: vec![RelationPathPattern {
                namespace: RelationNamespace::Discovery,
                relation_type: self.relation_type.clone(),
                from_role: self.from_role.clone(),
                to_role: self.to_role.clone(),
                from_resource: None,
                to_resource: None,
                required_participants: vec![],
            }],
            allowed_relation_types: vec![self.relation_type.clone()],
            allowed_namespaces: vec![RelationNamespace::Discovery],
            authority_requirement: None,
            temporal_context: Some(temporal_context),
            access_context,
            expansion_budget: TraversalBudget {
                max_hops: self.max_hops,
                max_relations: 256,
                max_branching_per_node: 32,
                max_seed_nodes: MAX_GRAPH_SEEDS,
                max_paths: 64,
            },
            stop_conditions: vec![],
        }
    }
}

impl fmt::Debug for DiscoverInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DiscoverInput(<redacted>)")
    }
}

impl DiscoverInput {
    pub fn validate(&self) -> Result<(), ApiError> {
        if let Some(graph) = &self.graph {
            graph.validate()?;
        }
        let mut claims = self.required_claim_ids.clone();
        claims.sort();
        claims.dedup();
        if self.purpose.trim().is_empty()
            || self.purpose.len() > MAX_PURPOSE_BYTES
            || self.required_resource_types.is_empty()
            || self.required_resource_types.len() > 8
            || self.required_claim_ids.is_empty()
            || self.required_claim_ids.len() > 16
            || claims.len() != self.required_claim_ids.len()
            || self
                .query
                .as_ref()
                .is_some_and(|query| query.trim().is_empty() || query.len() > MAX_QUERY_BYTES)
            || self
                .business_timezone
                .as_ref()
                .is_some_and(|zone| zone.is_empty() || zone.len() > 255)
            || (self.coverage == SearchCoverage::BodyRequired && self.query.is_none())
        {
            return Err(ApiError::ValidationFailed);
        }
        Ok(())
    }
}

/// Everything the route wires per request, all server-owned.
pub struct DiscoverRouteWiring<'a> {
    pub config: DiscoveryConfig,
    pub sources: &'a dyn SourceRegistryPort,
    pub generations: &'a dyn GenerationReadPort,
    pub concepts: &'a dyn ConceptRegistryPort,
    pub retrieval: RetrievalExecutionPorts<'a>,
    pub assertions: &'a dyn AssertionStorePort,
    pub evidence: &'a dyn EvidenceResolverPort,
    pub probe: Option<&'a dyn ProbePort>,
    pub probe_catalog: Option<&'a dyn ProbeCapabilityCatalogPort>,
    pub source_policy: Option<&'a dyn CurrentSourcePolicyPort>,
    pub authority: &'a dyn AccessContextAuthorityPort,
    pub visibility: &'a dyn CurrentSourceVisibilityPort,
    pub registry: &'a dyn ScopedSourceRegistryPort,
    pub claims: Option<&'a dyn ActorVisibleClaimCatalogPort>,
    pub remote: Vec<(SourceId, RemoteWiring<'a>)>,
    pub clock: Arc<dyn LeaseClock>,
    pub disclosure_ttl: Duration,
}

pub struct DiscoverRouteService<'a> {
    wiring: DiscoverRouteWiring<'a>,
}

impl fmt::Debug for DiscoverRouteService<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DiscoverRouteService(<wired>)")
    }
}

fn route_error(error: SearchError) -> ApiError {
    match error {
        SearchError::SourceUnavailable(_) => ApiError::DependencyUnavailable,
        _ => ApiError::ServiceUnavailable,
    }
}

impl<'a> DiscoverRouteService<'a> {
    pub fn new(wiring: DiscoverRouteWiring<'a>) -> Self {
        Self { wiring }
    }

    pub async fn discover(
        &self,
        context: &SearchOperationContext,
        snapshot: &VisibleCatalogSnapshot,
        input: DiscoverInput,
    ) -> Result<TransientDisclosure<DiscoveryResult>, ApiError> {
        input.validate()?;
        context.check_live()?;
        let wiring = &self.wiring;
        let catalog = wiring.claims.ok_or(ApiError::DependencyUnavailable)?;
        let actor = context.actor();
        let binding = wiring
            .authority
            .bind_discovery(actor, DiscoveryEvaluationId::from_uuid(Uuid::now_v7()))
            .await
            .map_err(|_| ApiError::IdentityUnavailable)?
            .ok_or(ApiError::AuthenticationRequired)?;
        let scopes: Vec<_> = snapshot
            .entries()
            .iter()
            .map(|entry| entry.scope().clone())
            .collect();
        let selectors =
            VisibleClaimSelectors::bind_all(catalog, actor, &scopes, &input.required_claim_ids)
                .await
                .map_err(|_| ApiError::DependencyUnavailable)?;
        let remote_sources: Vec<SourceId> = snapshot
            .entries()
            .iter()
            .filter(|entry| entry.registration().kind() == SourceKind::Remote)
            .map(|entry| entry.scope().source_id())
            .collect();
        let remote_selectors = selectors.remote_selectors(&remote_sources);
        let now = OffsetDateTime::now_utc();
        let temporal_context = TemporalEvaluationContext::new(
            binding.evaluation(),
            input.temporal_target.unwrap_or(now),
            now,
            input.business_timezone.as_deref().unwrap_or("UTC"),
        );
        let mut config = wiring.config.clone();
        if let Some(graph) = &input.graph {
            // One plan per visible Document Source; Remote Sources expose
            // no Graph. The plan carries this actor's access context.
            config.retriever_support.hypergraph = true;
            let access_context = actor.access_handle().to_opaque_string();
            for entry in snapshot
                .entries()
                .iter()
                .filter(|entry| entry.registration().kind() == SourceKind::Document)
            {
                config.retrieval_inputs.graph_plans.insert(
                    entry.scope().source_id(),
                    graph.plan(access_context.clone(), temporal_context.clone()),
                );
            }
        }
        let service = DiscoveryService::new(
            config,
            DiscoveryPorts {
                sources: wiring.sources,
                generations: wiring.generations,
                concepts: wiring.concepts,
                retrieval: wiring.retrieval,
                selectors: &selectors,
                assertions: wiring.assertions,
                evidence: wiring.evidence,
                probe: wiring.probe,
                probe_catalog: wiring.probe_catalog,
                source_policy: wiring.source_policy,
            },
        )
        .map_err(|_| ApiError::ServiceUnavailable)?;
        let scoped = ScopedDiscoveryService::new(
            &service,
            wiring.authority,
            wiring.visibility,
            wiring.registry,
            &remote_selectors,
            wiring.remote.clone(),
            wiring.clock.clone(),
            wiring.disclosure_ttl,
        )
        .map_err(|_| ApiError::ServiceUnavailable)?;
        let request = DiscoveryRequest {
            need: DiscoveryNeed {
                need_id: NeedId::from_uuid(Uuid::now_v7()),
                intent_signature: IntentSignature::new(IntentFact::new(
                    input.purpose.clone(),
                    IntentFactOrigin::Explicit,
                )),
                required_resource_types: input.required_resource_types.clone(),
                // Every requested Claim stays required, bound or not.
                required_claims: input.required_claim_ids.clone(),
                authority_requirements: vec![],
                freshness_requirements: vec![],
                constraints: vec![],
                completion_requirement: EvidenceRequirement::new(input.required_claim_ids.clone()),
            },
            temporal_context,
            access_context: String::new(),
        };
        let visible_ids: Vec<SourceId> = scopes.iter().map(|scope| scope.source_id()).collect();
        let routing = RoutingConstraints {
            required_source_ids: vec![],
            preferred_source_ids: visible_ids.clone(),
            max_initial_optional_sources: visible_ids.len().min(MAX_OPTIONAL_INITIAL_SOURCES),
        };
        let content_scope = match (input.coverage, &input.query) {
            (SearchCoverage::BodyRequired, Some(query)) => {
                DiscoveryScope::BodyRequired(BodySearchSpec {
                    query: LexicalQuery::body_only(query.clone(), 50),
                    exact_text_claim: None,
                })
            }
            _ => DiscoveryScope::Normal,
        };
        let disclosure = scoped
            .discover_with_scope(&binding, request, routing, content_scope)
            .await
            .map_err(route_error)?;
        context.check_live()?;
        Ok(disclosure)
    }
}
