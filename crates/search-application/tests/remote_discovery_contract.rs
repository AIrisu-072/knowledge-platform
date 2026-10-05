//! P4-12: remote Sources in the one common Discovery path.

#[path = "support/remote.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
use support::*;
use time::OffsetDateTime;
use uuid::Uuid;

const TITLE: u128 = 61;
const DEPARTMENT: u128 = 62;

fn claim(value: u128) -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(value))
}

/// No durable generation for any Source; nothing is ever written.
struct Nothing;

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

struct AllowAll;
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
struct Lookup;
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
            }))
        })
    }
}

/// Snapshot verifier that issues a new single-response token per call.
struct Rotating(Mutex<u32>);
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
struct Port<'r> {
    remote: &'r Remote,
    visibility: &'r dyn CurrentSourceVisibilityPort,
    verifier: &'r dyn RemoteSnapshotVerifierPort,
    answers: BTreeMap<&'static str, Vec<UntrustedRemoteHit>>,
    reverse: bool,
    batches: Mutex<Vec<usize>>,
}

fn operation_name(kind: RemoteOperationKind) -> &'static str {
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
fn doc(id: &str) -> UntrustedRemoteHit {
    let field = if id == "doc-1" {
        ("catalog.department", "総務", Some("ev-department"))
    } else {
        ("catalog.title", "規程", Some("ev-title"))
    };
    hit(Some(id), Some("v1"), Some("d1"), &[field])
}

fn answers(
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

fn config(source: SourceId, max_initial: usize) -> DiscoveryConfig {
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

fn request(remote: &Remote) -> DiscoveryRequest {
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

fn id(remote: &Remote, native_id: &str) -> ResourceId {
    remote_resource_id(
        remote.registration.tenant(),
        remote.registration.source_id(),
        "synthetic",
        &native(native_id),
    )
    .unwrap()
}

fn has_gap(result: &DiscoveryResult, source: SourceId, code: &str) -> bool {
    let fact = format!("source:{}:{code}", source.as_uuid());
    result
        .unresolved_gaps
        .iter()
        .any(|gap| gap.required_fact == fact)
}

/// One evaluation over the remote fixture: returns the result, the provider
/// batch sizes and whether the sealed key still lacks a DirectAddress list.
async fn evaluate(
    remote: &Remote,
    verifier: &dyn RemoteSnapshotVerifierPort,
    answers: BTreeMap<&'static str, Vec<UntrustedRemoteHit>>,
    reverse: bool,
    max_initial: usize,
) -> (DiscoveryResult, Vec<usize>, bool) {
    let source = remote.registration.source_id();
    let visibility = remote.visibility();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    let clock = ManualClock::new();
    let nothing = Nothing;
    let selectors = RemoteClaimSelectors::new(vec![
        (claim(TITLE), "catalog.title".into(), None),
        (claim(DEPARTMENT), "catalog.department".into(), None),
    ]);
    let view = CompositeEvaluationReadView::new(
        &nothing,
        &nothing,
        &nothing,
        &nothing,
        &nothing,
        &selectors,
        &gate,
        clock.clone(),
    );
    let port = Port {
        remote,
        visibility: &visibility,
        verifier,
        answers,
        reverse,
        batches: Mutex::new(vec![]),
    };
    let mut registry = InMemorySourceRegistry::default();
    registry.insert(remote.visible.discoverable_source());
    let access = AllowAll;
    let config = config(source, max_initial);
    let routing = config.routing.clone();
    let service = DiscoveryService::new(
        config,
        DiscoveryPorts {
            sources: &registry,
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
            },
            selectors: &nothing,
            assertions: &nothing,
            evidence: &nothing,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    let result = service
        .discover_scoped(
            request(remote),
            ScopedDiscoveryExecution {
                content_scope: DiscoveryScope::Normal,
                binding: &remote.binding,
                visible: std::slice::from_ref(&remote.visible),
                routing,
                remote: vec![RemoteSourceExecution {
                    context: remote.context(&visibility).await,
                    port: &port,
                    lineage: RegisteredLineage::new(&remote.registration, vec![]).unwrap(),
                    provenance: &Lookup,
                    lease: RemoteLease {
                        absolute_deadline: clock.now() + Duration::from_secs(60),
                        idle_timeout: None,
                        provider_expiry: None,
                    },
                }],
                view: &view,
            },
        )
        .await
        .unwrap();
    let direct = RetrievalAction {
        source_id: source,
        retriever_id: format!("{}:{:?}", source.as_uuid(), RetrieverKind::DirectAddress),
        retriever: RetrieverKind::DirectAddress,
        stage: RouteStage::Initial,
        state: ActionState::Planned,
        cursor: RetrieverCursorState::initial(),
    };
    let unsealed_direct = match view.remote_key(source) {
        Some(key) => view.retrieve(&direct, key).await.is_err(),
        None => true,
    };
    let batches = port.batches.lock().unwrap().clone();
    (result, batches, unsealed_direct)
}

fn qualified(result: &DiscoveryResult) -> Vec<ResourceId> {
    result
        .qualified_resources
        .iter()
        .map(|resource| resource.resource_ref)
        .collect()
}

#[tokio::test]
async fn two_remote_actions_one_key_pass_common_federation_and_claims() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let verifier = Verifier::shared("snapshot-1");
    let (result, batches, unsealed_direct) = evaluate(
        &remote,
        &verifier,
        answers(Some("doc-2"), Some("doc-1")),
        false,
        2,
    )
    .await;
    // One provider batch of both actions, sealed under one key.
    assert_eq!(batches, vec![2]);
    assert!(!unsealed_direct);
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().all(|gap| !gap.blocking));
    assert_eq!(
        qualified(&result),
        vec![id(&remote, "doc-2"), id(&remote, "doc-1")]
    );
    for claim_id in [claim(TITLE), claim(DEPARTMENT)] {
        assert!(
            result
                .evidence_set
                .iter()
                .any(|claim| claim.claim_id == claim_id && claim.state == ClaimState::Supported)
        );
    }
    let source = remote.registration.source_id().as_uuid();
    for kind in ["RemoteQuery", "DirectAddress"] {
        assert!(
            result
                .retrieval_trace
                .contains(&format!("retriever:{source}:{kind}"))
        );
    }
}

#[tokio::test]
async fn required_remote_planned_but_failed_is_not_executed() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    let verifier = Verifier::shared("snapshot-1");
    // Both planned actions fail: nothing is sealed, nothing executed.
    let (result, batches, _) = evaluate(&remote, &verifier, answers(None, None), false, 2).await;
    assert_eq!(batches, vec![2]);
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(has_gap(&result, source, "required_source_not_executed"));
    assert!(result.qualified_resources.is_empty());
    assert!(
        !result
            .retrieval_trace
            .iter()
            .any(|entry| entry.starts_with("retriever:"))
    );
    // One failed action: the completed one executes, the other is a gap.
    let (result, _, _) = evaluate(&remote, &verifier, answers(None, Some("doc-1")), false, 2).await;
    assert!(has_gap(&result, source, "remote_action_unavailable"));
    assert!(!has_gap(&result, source, "required_source_not_executed"));
    assert_eq!(qualified(&result), vec![id(&remote, "doc-1")]);
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(
        !result
            .retrieval_trace
            .iter()
            .any(|entry| entry.ends_with(":RemoteQuery"))
    );
}

#[tokio::test]
async fn remote_expansion_after_seal_requires_new_evaluation() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    let verifier = Verifier::shared("snapshot-1");
    // Only the query is initial; the lookup is a later expansion.
    let (result, batches, unsealed_direct) = evaluate(
        &remote,
        &verifier,
        answers(Some("doc-2"), Some("doc-1")),
        false,
        1,
    )
    .await;
    assert_eq!(batches, vec![1]);
    assert!(unsealed_direct);
    assert!(has_gap(
        &result,
        source,
        "remote_expansion_requires_new_evaluation"
    ));
    assert_eq!(qualified(&result), vec![id(&remote, "doc-2")]);
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    // An incompatible extra response is a gap and never joins the seal.
    let rotating = Rotating(Mutex::new(0));
    let (result, batches, unsealed_direct) = evaluate(
        &remote,
        &rotating,
        answers(Some("doc-2"), Some("doc-1")),
        false,
        2,
    )
    .await;
    assert_eq!(batches, vec![2]);
    assert!(unsealed_direct);
    assert!(has_gap(&result, source, "remote_snapshot_incompatible"));
    assert!(has_gap(&result, source, "remote_action_unavailable"));
    assert_eq!(qualified(&result), vec![id(&remote, "doc-2")]);
}

#[tokio::test]
async fn local_discovery_and_s1_priority_unchanged() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    // Provider answer order never changes the plan's S1 priority.
    let verifier = Verifier::shared("snapshot-1");
    let (result, _, _) = evaluate(
        &remote,
        &verifier,
        answers(Some("doc-2"), Some("doc-1")),
        true,
        2,
    )
    .await;
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(
        qualified(&result),
        vec![id(&remote, "doc-2"), id(&remote, "doc-1")]
    );

    // The legacy local entrypoint routes no remote mode and calls no provider.
    let nothing = Nothing;
    let access = AllowAll;
    let mut registry = InMemorySourceRegistry::default();
    registry.insert(remote.visible.discoverable_source());
    let service = DiscoveryService::new(
        config(source, 2),
        DiscoveryPorts {
            sources: &registry,
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
            },
            selectors: &nothing,
            assertions: &nothing,
            evidence: &nothing,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    let local = service.discover(request(&remote)).await.unwrap();
    assert!(local.qualified_resources.is_empty());
    assert_ne!(local.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(
        local
            .unresolved_gaps
            .iter()
            .any(|gap| gap.required_fact.ends_with(":RuntimePortUnavailable"))
    );
    assert!(
        !local
            .retrieval_trace
            .iter()
            .any(|entry| entry.starts_with("retriever:"))
    );
}
