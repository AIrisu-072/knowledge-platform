//! The scoped Discovery path over HTTP adapters: no durable generation,
//! per-Source current item access from each adapter's `/authorize`, and a
//! disclosure through the final gate copied into a plain `Shape`.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use search_application::SearchError;
use search_application::discovery_service::{
    DiscoveryConfig, DiscoveryPorts, DiscoveryService, TemporalPolicy,
};
use search_application::materialization::ProbeBudget;
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort,
    ConceptRegistryPort, CurrentCandidateAccessEvaluatorPort, EvidenceResolverPort,
    ProjectionGenerationStore, ResolvedAssertionEvidence, SemanticRegistrySnapshot,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_application::remote_disclosure::{
    RemoteWiring, ScopedDisclosureGate, ScopedDiscoveryService, TransientDisclosure,
};
use search_application::remote_evidence::RegisteredLineage;
use search_application::remote_lease::SystemLeaseClock;
use search_application::remote_read_view::RemoteClaimSelectors;
use search_application::retrieval::{
    LiveInput, OpaqueNativeId, RemoteQueryInput, RetrievalInputs, RetrieverProfile,
    RetrieverSupport,
};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::routing::RoutingConstraints;
use search_application::scoped::{SyntheticVisibilityAdapter, TrustedDiscoveryBinding};
use search_application::source_registration::TrustedVisibleRegistry;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::assertion::Assertion;
use search_core::discovery::{
    DiscoveryNeed, DiscoveryRequest, DiscoveryResult, FederatedCandidate, GapReason,
};
use search_core::evidence::{ClaimState, EvidenceRequirement, EvidenceSufficiency};
use search_core::id::{ClaimId, NeedId, ResourceId, SourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::predicate::{ConceptResolver, TruthValue};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::resource::ResourceKind;
use search_core::temporal::TemporalEvaluationContext;
use search_source_http::adapter::HttpRemoteSourceAdapter;
use time::OffsetDateTime;
use uuid::Uuid;

use super::World;

pub const TITLE: u128 = 81;

pub fn title_claim() -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(TITLE))
}

/// No durable generation for any Source; nothing is ever written.
pub struct Nothing;

fn no_write<'a>() -> BoxFuture<'a, ()> {
    Box::pin(async { Err(SearchError::OperationFailed("no durable write".into())) })
}

impl ProjectionGenerationStore for Nothing {
    fn begin_generation<'a>(&'a self, _: PersistableGenerationManifest) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn begin_incremental_generation<'a>(
        &'a self,
        _: PersistableGenerationManifest,
        _: ProjectionGenerationKey,
        _: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn stage_resource<'a>(&'a self, _: PersistableResourceProjection) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn stage_concept_registry<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn validate_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn publish_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn fail_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        no_write()
    }
    fn pin_current<'a>(
        &'a self,
        _: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        Box::pin(async { Ok(None) })
    }
    fn resource_at<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: ResourceId,
    ) -> BoxFuture<'a, Option<CompiledResourceProjection>> {
        Box::pin(async { Ok(None) })
    }
}

impl ClaimSelectorPort for Nothing {
    fn selector_for<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async { Ok(None) })
    }
}

impl AssertionStorePort for Nothing {
    fn assertions_for<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: ResourceId,
        _: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        Box::pin(async { Ok(vec![]) })
    }
}

impl EvidenceResolverPort for Nothing {
    fn resolve<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: ResourceId,
        _: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        Box::pin(async { Ok(None) })
    }
}

struct Unknown;
impl ConceptResolver for Unknown {
    fn same_concept(&self, _: &str, _: &str) -> TruthValue {
        TruthValue::Unknown
    }
    fn is_a(&self, _: &str, _: &str) -> TruthValue {
        TruthValue::Unknown
    }
    fn descendant_of(&self, _: &str, _: &str) -> TruthValue {
        TruthValue::Unknown
    }
}

impl ConceptRegistryPort for Nothing {
    fn pin_view<'a>(
        &'a self,
        _: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Arc<dyn ConceptResolver + Send + Sync>> {
        Box::pin(async {
            let resolver: Arc<dyn ConceptResolver + Send + Sync> = Arc::new(Unknown);
            Ok(resolver)
        })
    }
    fn same_concept<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        Box::pin(async { Ok(TruthValue::Unknown) })
    }
    fn is_a<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        Box::pin(async { Ok(TruthValue::Unknown) })
    }
}

/// Dispatches current candidate access to the owning Source's adapter.
pub struct BySource<'a>(pub Vec<&'a HttpRemoteSourceAdapter<'a>>);

impl CurrentCandidateAccessEvaluatorPort for BySource<'_> {
    fn evaluate<'b>(
        &'b self,
        candidate: &'b FederatedCandidate,
        access_context: &'b str,
    ) -> BoxFuture<'b, AccessDecision> {
        Box::pin(async move {
            match self
                .0
                .iter()
                .find(|adapter| adapter.registration().source_id() == candidate.source_ref)
            {
                Some(adapter) => adapter.evaluate(candidate, access_context).await,
                None => Ok(AccessDecision::Unknown),
            }
        })
    }
}

/// One planned remote mode with its trusted, Source-local input.
#[derive(Clone)]
pub enum Mode {
    Enumerate,
    Query(&'static str),
    Lookup(&'static str),
    Live(&'static str),
}

pub fn config(
    required: &[SourceId],
    preferred: &[SourceId],
    modes: &[(SourceId, Mode)],
    max_initial: usize,
) -> DiscoveryConfig {
    let mut support = RetrieverSupport::default();
    let mut inputs = RetrievalInputs {
        max_initial_retrievers_per_source: max_initial,
        ..RetrievalInputs::default()
    };
    for (source, mode) in modes {
        match mode {
            Mode::Enumerate => support.remote_enumeration = true,
            Mode::Query(text) => {
                support.remote_query = true;
                inputs.remote_queries.insert(
                    *source,
                    RemoteQueryInput::new(*text, vec![], 10, &[]).unwrap(),
                );
            }
            Mode::Lookup(id) => {
                support.direct_address = true;
                inputs
                    .native_ids
                    .insert(*source, OpaqueNativeId::new(*id).unwrap());
            }
            Mode::Live(id) => {
                support.live_only = true;
                inputs.live_inputs.insert(
                    *source,
                    LiveInput::lookup(OpaqueNativeId::new(*id).unwrap()),
                );
            }
        }
    }
    DiscoveryConfig {
        routing: RoutingConstraints {
            required_source_ids: required.to_vec(),
            preferred_source_ids: preferred.to_vec(),
            max_initial_optional_sources: preferred.len(),
        },
        retriever_profile: RetrieverProfile::Capability,
        retriever_support: support,
        retrieval_inputs: inputs,
        structured_filters: vec![],
        discriminators: vec![],
        lexical_query: None,
        temporal_policy: TemporalPolicy::default(),
        probe_budget: ProbeBudget {
            max_content_bytes: 0,
            max_latency_ms: 0,
            max_remote_calls: 0,
            max_monetary_cost_minor_units: 0,
            currency: "USD".into(),
        },
        max_actions: 8,
        evaluation_currency: "USD".into(),
    }
}

pub fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::now_utc();
    let claims = vec![title_claim()];
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::now_v7()),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find a rule".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![ResourceKind::Knowledge],
            required_claims: claims.clone(),
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(claims),
        },
        temporal_context: TemporalEvaluationContext::new(
            search_core::id::DiscoveryEvaluationId::from_uuid(Uuid::now_v7()),
            now,
            now,
            "Asia/Tokyo",
        ),
        access_context: "replaced-by-the-binding".into(),
    }
}

/// Everything a disclosure reveals, copied out inside the callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub sufficiency: EvidenceSufficiency,
    pub qualified: Vec<ResourceId>,
    pub claims: Vec<(ClaimId, ClaimState)>,
    pub gaps: Vec<(String, GapReason, bool)>,
    pub trace: Vec<String>,
}

impl Shape {
    pub fn has_gap(&self, fragment: &str) -> bool {
        self.gaps.iter().any(|(fact, _, _)| fact.contains(fragment))
    }
    pub fn supported(&self) -> bool {
        self.claims
            .iter()
            .any(|(_, state)| *state == ClaimState::Supported)
    }
}

/// One scoped Discovery: returns the still-closed transient disclosure.
pub async fn discover_scoped(
    world: &World,
    visibility: &SyntheticVisibilityAdapter<'_>,
    adapters: &[&HttpRemoteSourceAdapter<'_>],
    binding: &TrustedDiscoveryBinding,
    config: DiscoveryConfig,
) -> Result<TransientDisclosure<DiscoveryResult>, SearchError> {
    let sources = InMemorySourceRegistry::default();
    let nothing = Nothing;
    let access = BySource(adapters.to_vec());
    let routing = config.routing.clone();
    let service = DiscoveryService::new(
        config,
        DiscoveryPorts {
            sources: &sources,
            generations: &nothing,
            concepts: &nothing,
            retrieval: RetrievalExecutionPorts {
                directory: None,
                structured: None,
                lexical: None,
                hypergraph: None,
                graph_resource_access: None,
                remote: None,
                access: &access,
                vector: None,
            },
            selectors: &nothing,
            assertions: &nothing,
            evidence: &nothing,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )?;
    let selectors = RemoteClaimSelectors::new(vec![(title_claim(), "catalog.title".into(), None)]);
    let registry = TrustedVisibleRegistry::new(&world.authority, visibility, &world.catalog);
    let wiring = adapters
        .iter()
        .map(|adapter| {
            (
                adapter.registration().source_id(),
                RemoteWiring {
                    port: *adapter,
                    lineage: RegisteredLineage::new(adapter.registration(), vec![]).unwrap(),
                    provenance: *adapter,
                    evaluation_ttl: Duration::from_secs(5),
                    idle_timeout: None,
                },
            )
        })
        .collect();
    let scoped = ScopedDiscoveryService::new(
        &service,
        &world.authority,
        visibility,
        &registry,
        &selectors,
        wiring,
        Arc::new(SystemLeaseClock),
        Duration::from_secs(30),
    )?;
    scoped.discover(binding, request(), routing).await
}

pub async fn disclose(
    world: &World,
    visibility: &SyntheticVisibilityAdapter<'_>,
    disclosure: &mut TransientDisclosure<DiscoveryResult>,
) -> Result<Shape, SearchError> {
    let gate = ScopedDisclosureGate::new(&world.authority, visibility);
    let mut shape = None;
    disclosure
        .with_disclosure(&gate, |view| {
            shape = Some(Shape {
                sufficiency: view.sufficiency(),
                qualified: view.qualified_resources().collect(),
                claims: view.claims().collect(),
                gaps: view
                    .gaps()
                    .map(|(fact, reason, blocking)| (fact.to_owned(), reason, blocking))
                    .collect(),
                trace: view.trace().map(str::to_owned).collect(),
            });
            Ok(())
        })
        .await?;
    Ok(shape.unwrap())
}

/// Discover and disclose in one step.
pub async fn evaluate(
    world: &World,
    visibility: &SyntheticVisibilityAdapter<'_>,
    adapters: &[&HttpRemoteSourceAdapter<'_>],
    binding: &TrustedDiscoveryBinding,
    config: DiscoveryConfig,
) -> Shape {
    let mut disclosure = discover_scoped(world, visibility, adapters, binding, config)
        .await
        .unwrap();
    disclose(world, visibility, &mut disclosure).await.unwrap()
}
