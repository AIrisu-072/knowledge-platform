//! Shared synthetic remote Discovery wiring for the P4-12/P4-13 contract
//! tests: no durable generation, allow-all candidate access, a direct
//! provenance lookup and a provider behind the checked observation adapter.
#![allow(dead_code, unused_imports)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::support::*;
use search_application::SearchError;
use search_application::content_scope::DiscoveryScope;
use search_application::discovery_service::{
    DiscoveryConfig, DiscoveryPorts, DiscoveryService, RemoteSourceExecution,
    ScopedDiscoveryExecution, TemporalPolicy,
};
use search_application::materialization::ProbeBudget;
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort,
    ConceptRegistryPort, CurrentCandidateAccessEvaluatorPort, CurrentSourcePolicy,
    EvidenceResolverPort, ProjectionGenerationStore, ResolvedAssertionEvidence,
    SealedRemoteRetrieverPort, SemanticRegistrySnapshot,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_application::remote::{
    PinnedRemoteTarget, PlannedRemoteAction, RemoteAccessTarget, RemoteActionOutcome,
    RemoteIdentity, RemoteOperationKind, RemotePage, RemoteReadOutcome, RemoteResponseInput,
    RemoteResponseStatus, RemoteSourcePort, RemoteUnknownReason, TrustedRemoteContext,
    UntrustedRemoteHit,
};
use search_application::remote_evidence::{
    RegisteredLineage, RemoteProvenanceLookupPort, VerifiedSourceProvenance,
};
use search_application::remote_identity::remote_resource_id;
use search_application::remote_lease::{LeaseClock, RemoteLease, ScopedOwnerGate};
use search_application::remote_observation::{
    CheckedRemoteObservationAdapter, RemoteSnapshotVerifierPort, SnapshotAttestation,
    SnapshotExtent,
};
use search_application::remote_read_view::{CompositeEvaluationReadView, RemoteClaimSelectors};
use search_application::retrieval::{
    ActionState, RemoteQueryInput, RetrievalAction, RetrievalInputs, RetrieverCursorState,
    RetrieverKind, RetrieverProfile, RetrieverSupport,
};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::routing::{RouteStage, RoutingConstraints};
use search_application::scoped::{AuthorizedSourceScope, CurrentSourceVisibilityPort};
use search_application::source_registry::InMemorySourceRegistry;
use search_core::assertion::Assertion;
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest, DiscoveryResult};
use search_core::evidence::{ClaimState, EvidenceRequirement, EvidenceSufficiency};
use search_core::id::{ClaimId, NeedId, ResourceId, SourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::materialization::MaterializationState;
use search_core::predicate::{ConceptResolver, TruthValue};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::resource::ResourceKind;
use search_core::source::RetentionMode;
use search_core::temporal::TemporalEvaluationContext;
use time::OffsetDateTime;
use uuid::Uuid;

pub const TITLE: u128 = 61;
pub const DEPARTMENT: u128 = 62;

pub fn claim(value: u128) -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(value))
}

/// No durable generation for any Source; nothing is ever written.
pub struct Nothing;

impl ProjectionGenerationStore for Nothing {
    fn begin_generation<'a>(&'a self, _: PersistableGenerationManifest) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(SearchError::OperationFailed("no durable write".into())) })
    }
    fn begin_incremental_generation<'a>(
        &'a self,
        _: PersistableGenerationManifest,
        _: ProjectionGenerationKey,
        _: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(SearchError::OperationFailed("no durable write".into())) })
    }
    fn stage_resource<'a>(&'a self, _: PersistableResourceProjection) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(SearchError::OperationFailed("no durable write".into())) })
    }
    fn stage_concept_registry<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(SearchError::OperationFailed("no durable write".into())) })
    }
    fn validate_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(SearchError::OperationFailed("no durable write".into())) })
    }
    fn publish_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(SearchError::OperationFailed("no durable write".into())) })
    }
    fn fail_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(SearchError::OperationFailed("no durable write".into())) })
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

pub struct Unknown;
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

pub struct AllowAll;
impl CurrentCandidateAccessEvaluatorPort for AllowAll {
    fn evaluate<'a>(
        &'a self,
        _: &'a search_core::discovery::FederatedCandidate,
        _: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async { Ok(AccessDecision::Allowed) })
    }
}

/// The fixed Source's provenance protocol: a direct record of the pinned
/// version for every evidence ref.
pub struct Lookup;
impl RemoteProvenanceLookupPort for Lookup {
    fn lookup<'a>(
        &'a self,
        _: &'a AuthorizedSourceScope,
        target: &'a PinnedRemoteTarget,
        _: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedSourceProvenance>> {
        Box::pin(async move {
            Ok(Some(VerifiedSourceProvenance {
                direct: true,
                summary: false,
                version: target.version().map(str::to_owned),
                digest: target.digest().map(str::to_owned),
                lineage_label: "catalog".into(),
                predicate: "catalog.title".into(),
                citation_chain: vec![],
                stance: Default::default(),
            }))
        })
    }
}

/// Snapshot verifier that issues a new single-response token per call.
pub struct Rotating(pub Mutex<u32>);
impl RemoteSnapshotVerifierPort for Rotating {
    fn verify<'a>(
        &'a self,
        _: &'a TrustedRemoteContext,
        _: &'a PlannedRemoteAction,
        _: &'a RemoteResponseInput,
    ) -> BoxFuture<'a, Option<SnapshotAttestation>> {
        Box::pin(async move {
            let mut next = self.0.lock().unwrap();
            *next += 1;
            Ok(Some(SnapshotAttestation::new(
                format!("snapshot-{next}"),
                SnapshotExtent::SingleResponse,
                OffsetDateTime::now_utc(),
                vec![],
            )?))
        })
    }
}

/// A synthetic provider behind the checked observation adapter. A missing
/// answer is an `Unknown` outcome; batches are recorded by size.
pub struct Port<'r> {
    pub remote: &'r Remote,
    pub visibility: &'r dyn CurrentSourceVisibilityPort,
    pub verifier: &'r dyn RemoteSnapshotVerifierPort,
    pub answers: BTreeMap<&'static str, Vec<UntrustedRemoteHit>>,
    pub reverse: bool,
    pub batches: Mutex<Vec<usize>>,
}

pub fn operation_name(kind: RemoteOperationKind) -> &'static str {
    match kind {
        RemoteOperationKind::Enumerate => "enumerate",
        RemoteOperationKind::Query => "query",
        RemoteOperationKind::Lookup => "lookup",
        RemoteOperationKind::Live => "live",
    }
}

impl RemoteSourcePort for Port<'_> {
    fn execute_batch<'a>(
        &'a self,
        context: &'a TrustedRemoteContext,
        actions: &'a [PlannedRemoteAction],
    ) -> BoxFuture<'a, Vec<RemoteActionOutcome>> {
        Box::pin(async move {
            self.batches.lock().unwrap().push(actions.len());
            let adapter = CheckedRemoteObservationAdapter::new(
                &self.remote.registration,
                &self.remote.authority,
                self.visibility,
                self.verifier,
            );
            let mut outcomes = Vec::new();
            for action in actions {
                let kind = action.operation().kind();
                let outcome = match self.answers.get(operation_name(kind)) {
                    Some(hits) => {
                        let input = RemoteResponseInput::new(
                            RemoteResponseStatus::Success,
                            RemotePage::Unpaged,
                            hits.clone(),
                            None,
                        )?;
                        adapter.observe(context, action, input).await?
                    }
                    None => RemoteActionOutcome::Unknown {
                        retriever_id: action.retriever_id().into(),
                        operation: kind,
                        reason: RemoteUnknownReason::Unavailable,
                    },
                };
                outcomes.push(outcome);
            }
            if self.reverse {
                outcomes.reverse();
            }
            Ok(outcomes)
        })
    }
    fn current_access<'a>(
        &'a self,
        _: &'a TrustedRemoteContext,
        _: &'a RemoteAccessTarget,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async { Err(SearchError::SourceUnavailable("not wired".into())) })
    }
    fn current_policy<'a>(
        &'a self,
        _: &'a TrustedRemoteContext,
        _: &'a RemoteIdentity,
    ) -> BoxFuture<'a, CurrentSourcePolicy> {
        Box::pin(async { Err(SearchError::SourceUnavailable("not wired".into())) })
    }
    fn probe_or_materialize<'a>(
        &'a self,
        _: &'a TrustedRemoteContext,
        _: &'a PinnedRemoteTarget,
        _: MaterializationState,
    ) -> BoxFuture<'a, RemoteReadOutcome> {
        Box::pin(async { Err(SearchError::SourceUnavailable("not wired".into())) })
    }
}

/// doc-1 carries a verified department, doc-2 a verified title: the
/// evaluation needs both lists before its two Claims are Supported.
pub fn doc(id: &str) -> UntrustedRemoteHit {
    let field = if id == "doc-1" {
        ("catalog.department", "総務", Some("ev-department"))
    } else {
        ("catalog.title", "規程", Some("ev-title"))
    };
    hit(Some(id), Some("v1"), Some("d1"), &[field])
}

pub fn answers(
    query: Option<&str>,
    lookup: Option<&str>,
) -> BTreeMap<&'static str, Vec<UntrustedRemoteHit>> {
    let mut answers = BTreeMap::new();
    if let Some(id) = query {
        answers.insert("query", vec![doc(id)]);
    }
    if let Some(id) = lookup {
        answers.insert("lookup", vec![doc(id)]);
    }
    answers
}

pub fn config(source: SourceId, max_initial: usize) -> DiscoveryConfig {
    let mut retrieval_inputs = RetrievalInputs {
        max_initial_retrievers_per_source: max_initial,
        ..RetrievalInputs::default()
    };
    retrieval_inputs.remote_queries.insert(
        source,
        RemoteQueryInput::new("規程", vec![], 10, &[]).unwrap(),
    );
    retrieval_inputs.native_ids.insert(source, native("doc-1"));
    DiscoveryConfig {
        routing: RoutingConstraints {
            required_source_ids: vec![source],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
        retriever_profile: RetrieverProfile::Capability,
        retriever_support: RetrieverSupport {
            remote_query: true,
            direct_address: true,
            ..RetrieverSupport::default()
        },
        retrieval_inputs,
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

pub fn request(remote: &Remote) -> DiscoveryRequest {
    let now = OffsetDateTime::now_utc();
    let claims = vec![claim(TITLE), claim(DEPARTMENT)];
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
            remote.evaluation(),
            now,
            now,
            "Asia/Tokyo",
        ),
        access_context: "reader".into(),
    }
}

pub fn id(remote: &Remote, native_id: &str) -> ResourceId {
    remote_resource_id(
        remote.registration.tenant(),
        remote.registration.source_id(),
        "synthetic",
        &native(native_id),
    )
    .unwrap()
}

pub fn has_gap(result: &DiscoveryResult, source: SourceId, code: &str) -> bool {
    let fact = format!("source:{}:{code}", source.as_uuid());
    result
        .unresolved_gaps
        .iter()
        .any(|gap| gap.required_fact == fact)
}

pub fn service<'a>(
    registry: &'a InMemorySourceRegistry,
    nothing: &'a Nothing,
    access: &'a AllowAll,
    config: DiscoveryConfig,
) -> DiscoveryService<'a> {
    DiscoveryService::new(
        config,
        DiscoveryPorts {
            sources: registry,
            generations: nothing,
            concepts: nothing,
            retrieval: RetrievalExecutionPorts {
                directory: None,
                structured: None,
                lexical: None,
                hypergraph: None,
                graph_resource_access: None,
                remote: None,
                access,
                vector: None,
            },
            selectors: nothing,
            assertions: nothing,
            evidence: nothing,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap()
}

pub fn selectors() -> RemoteClaimSelectors {
    RemoteClaimSelectors::new(vec![
        (claim(TITLE), "catalog.title".into(), None),
        (claim(DEPARTMENT), "catalog.department".into(), None),
    ])
}
