//! P4-11: explicit-key composite read view and the remote executor arms.

#[path = "support/remote.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use search_application::SearchError;
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort,
    ConceptRegistryPort, CurrentCandidateAccessEvaluatorPort, EvidenceResolverPort,
    GenerationReadPort, ProjectionGenerationStore, ResolvedAssertionEvidence,
    SealedRemoteRetrieverPort, SemanticRegistrySnapshot,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_application::remote::{EvaluationLeaseId, PinnedRemoteTarget};
use search_application::remote_evidence::{
    RegisteredLineage, RemoteProvenanceLookupPort, VerifiedSourceProvenance,
};
use search_application::remote_generation::{
    REMOTE_CLAIM_SUBJECT, RemoteEvaluationGeneration, RemoteGenerationBuilder,
};
use search_application::remote_lease::{LeaseClock, RemoteLease, ScopedOwnerGate};
use search_application::remote_read_view::{CompositeEvaluationReadView, RemoteClaimSelectors};
use search_application::retrieval::{
    ActionState, RetrievalAction, RetrieverCursorState, RetrieverKind,
};
use search_application::retrieval_execution::{
    RetrievalExecutionInput, RetrievalExecutionPorts, RetrievalExecutor,
};
use search_application::routing::RouteStage;
use search_application::scoped::AuthorizedSourceScope;
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::discovery::{
    CandidateIdentityClass, DiscoveryNeed, DiscoveryRequest, FederatedCandidate,
};
use search_core::evidence::{EvidenceRequirement, EvidenceRole};
use search_core::id::{ClaimId, NeedId, ProjectionGenerationId, ResourceId, SourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::predicate::{ConceptResolver, TruthValue, TypedValue};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::source::RetentionMode;
use search_core::temporal::TemporalEvaluationContext;
use support::*;
use time::OffsetDateTime;
use uuid::Uuid;

const CLAIM: u128 = 51;

/// Durable side: records every call, owns nothing for the remote key, and
/// fails the test on any write.
#[derive(Default)]
struct Durable {
    pinned: Option<ProjectionGenerationManifest>,
    resources: BTreeMap<(ProjectionGenerationKey, ResourceId), CompiledResourceProjection>,
    reads: Mutex<Vec<ProjectionGenerationKey>>,
    writes: Mutex<usize>,
}

impl Durable {
    fn wrote(&self) -> BoxFuture<'_, ()> {
        *self.writes.lock().unwrap() += 1;
        Box::pin(async { Err(SearchError::OperationFailed("no durable write".into())) })
    }
    fn read(&self, key: ProjectionGenerationKey) {
        self.reads.lock().unwrap().push(key);
    }
}

impl ProjectionGenerationStore for Durable {
    fn begin_generation<'a>(&'a self, _: PersistableGenerationManifest) -> BoxFuture<'a, ()> {
        self.wrote()
    }
    fn begin_incremental_generation<'a>(
        &'a self,
        _: PersistableGenerationManifest,
        _: ProjectionGenerationKey,
        _: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()> {
        self.wrote()
    }
    fn stage_resource<'a>(&'a self, _: PersistableResourceProjection) -> BoxFuture<'a, ()> {
        self.wrote()
    }
    fn stage_concept_registry<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        self.wrote()
    }
    fn validate_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        self.wrote()
    }
    fn publish_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        self.wrote()
    }
    fn fail_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        self.wrote()
    }
    fn pin_current<'a>(
        &'a self,
        _: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        Box::pin(async move { Ok(self.pinned.clone()) })
    }
    fn resource_at<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        id: ResourceId,
    ) -> BoxFuture<'a, Option<CompiledResourceProjection>> {
        self.read(key);
        Box::pin(async move { Ok(self.resources.get(&(key, id)).cloned()) })
    }
}

impl ClaimSelectorPort for Durable {
    fn selector_for<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        self.read(key);
        Box::pin(async move {
            Ok(Some(ClaimSelector {
                claim_id,
                subject_ref: "durable".into(),
                predicate: "catalog.title".into(),
                expected_value: None,
            }))
        })
    }
}

impl AssertionStorePort for Durable {
    fn assertions_for<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        _: ResourceId,
        predicate: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        self.read(key);
        Box::pin(async move {
            Ok(vec![Assertion::new(
                "durable",
                predicate,
                TypedValue::String("durable".into()),
                "durable",
                AssertionOrigin::Observed,
                "durable",
                OffsetDateTime::now_utc(),
            )])
        })
    }
}

impl EvidenceResolverPort for Durable {
    fn resolve<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        resource_id: ResourceId,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        self.read(key);
        Box::pin(async move {
            Ok(Some(ResolvedAssertionEvidence {
                generation: key,
                source_id: key.source_id,
                resource_id,
                evidence_ref: evidence_ref.into(),
                upstream_origin: "durable".into(),
                role: EvidenceRole::Primary,
                citation_chain: vec![],
                content_digest: None,
                is_summary: false,
            }))
        })
    }
}

struct Proven;
impl ConceptResolver for Proven {
    fn same_concept(&self, _: &str, _: &str) -> TruthValue {
        TruthValue::True
    }
    fn is_a(&self, _: &str, _: &str) -> TruthValue {
        TruthValue::True
    }
    fn descendant_of(&self, _: &str, _: &str) -> TruthValue {
        TruthValue::True
    }
}

impl ConceptRegistryPort for Durable {
    fn pin_view<'a>(
        &'a self,
        key: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Arc<dyn ConceptResolver + Send + Sync>> {
        self.read(key);
        Box::pin(async move {
            let resolver: Arc<dyn ConceptResolver + Send + Sync> = Arc::new(Proven);
            Ok(resolver)
        })
    }
    fn same_concept<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        self.read(key);
        Box::pin(async { Ok(TruthValue::True) })
    }
    fn is_a<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        self.read(key);
        Box::pin(async { Ok(TruthValue::True) })
    }
}

/// The fixed Source's provenance protocol: every ref is a direct record of
/// the pinned version.
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
                stance: Default::default(),
            }))
        })
    }
}

/// Current candidate access: a resource set the test revokes by hand.
#[derive(Default)]
struct Access(Mutex<BTreeSet<ResourceId>>);
impl Access {
    fn deny(&self, id: ResourceId) {
        self.0.lock().unwrap().insert(id);
    }
}
impl CurrentCandidateAccessEvaluatorPort for Access {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        _: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            Ok(match candidate.resource_ref {
                Some(id) if self.0.lock().unwrap().contains(&id) => AccessDecision::Denied,
                _ => AccessDecision::Allowed,
            })
        })
    }
}

fn lease(clock: &ManualClock) -> RemoteLease {
    RemoteLease {
        absolute_deadline: clock.now() + Duration::from_secs(60),
        idle_timeout: None,
        provider_expiry: None,
    }
}

fn selectors() -> RemoteClaimSelectors {
    RemoteClaimSelectors::new(vec![(
        ClaimId::from_uuid(Uuid::from_u128(CLAIM)),
        "catalog.title".into(),
        None,
    )])
}

/// One sealed batch: `search` lists every document, `lookup` the first.
async fn sealed(remote: &Remote, ids: &[&str]) -> RemoteEvaluationGeneration {
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let verifier = Verifier::shared("snapshot-1");
    let hits: Vec<_> = ids
        .iter()
        .map(|id| {
            hit(
                Some(id),
                Some("v1"),
                Some("d1"),
                &[("catalog.title", "規程", Some("ev-1"))],
            )
        })
        .collect();
    let searched = observe(
        remote,
        &visibility,
        &verifier,
        &context,
        "search",
        query(),
        hits.clone(),
    )
    .await;
    let looked = observe(
        remote,
        &visibility,
        &verifier,
        &context,
        "lookup",
        lookup(ids[0]),
        vec![hits[0].clone()],
    )
    .await;
    let mut batch =
        RemoteGenerationBuilder::new(context, remote.evaluation(), EvaluationLeaseId::new())
            .unwrap();
    batch.stage(searched).unwrap();
    batch.stage(looked).unwrap();
    let lineage = RegisteredLineage::new(&remote.registration, vec![]).unwrap();
    batch.verify_evidence(&lineage, &Lookup).await.unwrap();
    batch.seal().unwrap()
}

fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::now_utc();
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::now_v7()),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find a rule".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![],
            required_claims: vec![],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![]),
        },
        temporal_context: TemporalEvaluationContext::new(
            search_core::id::DiscoveryEvaluationId::from_uuid(Uuid::now_v7()),
            now,
            now,
            "Asia/Tokyo",
        ),
        access_context: "reader".into(),
    }
}

fn action(source: SourceId, retriever: &str, kind: RetrieverKind) -> RetrievalAction {
    RetrievalAction {
        source_id: source,
        retriever_id: retriever.into(),
        retriever: kind,
        stage: RouteStage::Initial,
        state: ActionState::Planned,
        cursor: RetrieverCursorState::initial(),
    }
}

fn ports<'a>(
    remote: Option<&'a dyn SealedRemoteRetrieverPort>,
    access: &'a dyn CurrentCandidateAccessEvaluatorPort,
) -> RetrievalExecutionPorts<'a> {
    RetrievalExecutionPorts {
        directory: None,
        structured: None,
        lexical: None,
        hypergraph: None,
        graph_resource_access: None,
        remote,
        access,
        vector: None,
    }
}

async fn execute(
    view: &dyn SealedRemoteRetrieverPort,
    access: &dyn CurrentCandidateAccessEvaluatorPort,
    action: &RetrievalAction,
    key: ProjectionGenerationKey,
) -> Result<Vec<(ResourceId, usize, ProjectionGenerationKey)>, SearchError> {
    let request = request();
    let result = RetrievalExecutor::execute(
        &ports(Some(view), access),
        RetrievalExecutionInput {
            action,
            generation: key,
            request: &request,
            structured_filters: &[],
            lexical_query: None,
            body_query: None,
            graph_plan: None,
            vector_query: None,
            defer_access_to_caller: false,
        },
    )
    .await?;
    Ok(result
        .hits
        .into_iter()
        .map(|hit| {
            (
                hit.candidate.resource_ref.unwrap(),
                hit.rank,
                hit.generation,
            )
        })
        .collect())
}

macro_rules! view {
    ($durable:expr, $selectors:expr, $gate:expr, $clock:expr) => {
        CompositeEvaluationReadView::new(
            $durable, $durable, $durable, $durable, $durable, $selectors, $gate, $clock,
        )
    };
}

#[tokio::test]
async fn sealed_key_reads_all_five_ports_without_durable_write() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    let clock = ManualClock::new();
    let durable = Durable::default();
    let selectors = selectors();
    let view = view!(&durable, &selectors, &gate, clock.clone());

    let generation = sealed(&remote, &["doc-1"]).await;
    let id = generation.resource_ids().into_iter().next().unwrap();
    let key = view
        .register_remote(generation, lease(&clock))
        .await
        .unwrap();

    // 1. projection read port.
    let manifest = GenerationReadPort::pin_current(&view, key.source_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(manifest.key(), key);
    let projection = GenerationReadPort::resource_at(&view, key, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(projection.directory.resource_ref, id);
    // 2. server-owned selector about the remote subject.
    let claim = ClaimId::from_uuid(Uuid::from_u128(CLAIM));
    let selector = view.selector_for(key, claim).await.unwrap().unwrap();
    assert_eq!(selector.subject_ref, REMOTE_CLAIM_SUBJECT);
    assert!(
        view.selector_for(key, ClaimId::from_uuid(Uuid::from_u128(99)))
            .await
            .unwrap()
            .is_none()
    );
    // 3. assertion with the verified evidence ref.
    let assertions = view.assertions_for(key, id, "catalog.title").await.unwrap();
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].subject_ref, REMOTE_CLAIM_SUBJECT);
    assert_eq!(assertions[0].evidence_refs, vec!["ev-1".to_owned()]);
    assert_eq!(assertions[0].origin, AssertionOrigin::Authoritative);
    // 4. evidence resolved only from the verified record.
    let evidence = view.resolve(key, id, "ev-1").await.unwrap().unwrap();
    assert_eq!((evidence.generation, evidence.resource_id), (key, id));
    assert_eq!(evidence.role, EvidenceRole::Primary);
    assert!(
        view.resolve(key, id, "ev-unverified")
            .await
            .unwrap()
            .is_none()
    );
    // 5. non-sensitive concept view: nothing beyond identity is proven.
    let concepts = view.pin_view(key).await.unwrap();
    assert_eq!(concepts.is_a("規程", "文書"), TruthValue::Unknown);
    assert_eq!(
        view.same_concept(key, "a", "b").await.unwrap(),
        TruthValue::Unknown
    );
    assert_eq!(view.is_a(key, "a", "b").await.unwrap(), TruthValue::Unknown);

    assert_eq!(*durable.writes.lock().unwrap(), 0);
    assert!(durable.reads.lock().unwrap().is_empty());

    // The evaluation ends: the lease closes and nothing is readable.
    view.close();
    assert!(
        GenerationReadPort::resource_at(&view, key, id)
            .await
            .is_err()
    );
    assert!(view.assertions_for(key, id, "catalog.title").await.is_err());
}

#[tokio::test]
async fn unknown_key_never_falls_back_by_uuid() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    let clock = ManualClock::new();
    let generation = sealed(&remote, &["doc-1"]).await;
    let id = generation.resource_ids().into_iter().next().unwrap();
    let remote_key = generation.key();

    // The durable store holds a resource under the same generation UUID for
    // another Source, and the same Source under another UUID.
    let other_source = ProjectionGenerationKey {
        source_id: SourceId::from_uuid(Uuid::from_u128(9_001)),
        generation_id: remote_key.generation_id,
    };
    let other_generation = ProjectionGenerationKey {
        source_id: remote_key.source_id,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::now_v7()),
    };
    // A real compiled projection, copied out of another evaluation's view.
    let planted = {
        let probe = Durable::default();
        let selectors = selectors();
        let view = view!(&probe, &selectors, &gate, clock.clone());
        let key = view
            .register_remote(sealed(&remote, &["doc-1"]).await, lease(&clock))
            .await
            .unwrap();
        GenerationReadPort::resource_at(&view, key, id)
            .await
            .unwrap()
            .unwrap()
    };
    let mut durable = Durable::default();
    for key in [other_source, other_generation] {
        durable.resources.insert((key, id), planted.clone());
    }
    let selectors = selectors();
    let view = view!(&durable, &selectors, &gate, clock.clone());
    view.register_remote(generation, lease(&clock))
        .await
        .unwrap();

    let claim = ClaimId::from_uuid(Uuid::from_u128(CLAIM));
    for key in [other_source, other_generation] {
        assert!(
            GenerationReadPort::resource_at(&view, key, id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(view.selector_for(key, claim).await.unwrap().is_none());
        assert!(
            view.assertions_for(key, id, "catalog.title")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(view.resolve(key, id, "ev-1").await.unwrap().is_none());
        assert_eq!(view.is_a(key, "a", "b").await.unwrap(), TruthValue::Unknown);
    }
    assert!(durable.reads.lock().unwrap().is_empty());

    // A durable key becomes readable only once this evaluation pinned it.
    let pinned_source = SourceId::from_uuid(Uuid::from_u128(9_002));
    let durable_key = ProjectionGenerationKey {
        source_id: pinned_source,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::now_v7()),
    };
    let mut manifest = planted.manifest.clone();
    manifest.source_id = durable_key.source_id;
    manifest.generation_id = durable_key.generation_id;
    let mut durable = Durable {
        pinned: Some(manifest),
        ..Durable::default()
    };
    durable.resources.insert((durable_key, id), planted);
    let view = view!(&durable, &selectors, &gate, clock.clone());
    assert!(
        GenerationReadPort::resource_at(&view, durable_key, id)
            .await
            .unwrap()
            .is_none()
    );
    GenerationReadPort::pin_current(&view, pinned_source)
        .await
        .unwrap()
        .unwrap();
    assert!(
        GenerationReadPort::resource_at(&view, durable_key, id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        view.is_a(durable_key, "a", "b").await.unwrap(),
        TruthValue::True
    );
    // One Source keeps one generation domain per evaluation.
    let mut durable_pinned_remote = Durable {
        pinned: Some({
            let mut m = durable.pinned.clone().unwrap();
            m.source_id = remote.registration.source_id();
            m
        }),
        ..Durable::default()
    };
    durable_pinned_remote.resources.clear();
    let view = view!(&durable_pinned_remote, &selectors, &gate, clock.clone());
    GenerationReadPort::pin_current(&view, remote.registration.source_id())
        .await
        .unwrap()
        .unwrap();
    assert!(
        view.register_remote(sealed(&remote, &["doc-1"]).await, lease(&clock))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn remote_executor_ranks_only_accessible_hits() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    let clock = ManualClock::new();
    let durable = Durable::default();
    let selectors = selectors();
    let view = view!(&durable, &selectors, &gate, clock.clone());
    let generation = sealed(&remote, &["doc-1", "doc-2", "doc-3"]).await;
    let ordered: Vec<ResourceId> = generation
        .candidates("search")
        .unwrap()
        .iter()
        .map(|candidate| candidate.resource_ref.unwrap())
        .collect();
    let key = view
        .register_remote(generation, lease(&clock))
        .await
        .unwrap();
    let access = Access::default();
    access.deny(ordered[1]);

    let hits = execute(
        &view,
        &access,
        &action(key.source_id, "search", RetrieverKind::RemoteQuery),
        key,
    )
    .await
    .unwrap();
    // The hidden second position is not disclosed by a rank gap.
    assert_eq!(hits, vec![(ordered[0], 1, key), (ordered[2], 2, key)],);
    // Without the sealed port a remote action is unsupported, never empty.
    let request = request();
    let missing = RetrievalExecutor::execute(
        &ports(None, &access),
        RetrievalExecutionInput {
            action: &action(key.source_id, "search", RetrieverKind::RemoteQuery),
            generation: key,
            request: &request,
            structured_filters: &[],
            lexical_query: None,
            body_query: None,
            graph_plan: None,
            vector_query: None,
            defer_access_to_caller: false,
        },
    )
    .await;
    assert!(missing.is_err());
}

/// A port that answers with a list from another generation.
struct Stale(Vec<FederatedCandidate>);
impl SealedRemoteRetrieverPort for Stale {
    fn retrieve<'a>(
        &'a self,
        _: &'a RetrievalAction,
        _: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        Box::pin(async move { Ok(self.0.clone()) })
    }
}

#[tokio::test]
async fn all_remote_hit_and_list_keys_match_pin() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    let clock = ManualClock::new();
    let durable = Durable::default();
    let selectors = selectors();
    let view = view!(&durable, &selectors, &gate, clock.clone());
    let generation = sealed(&remote, &["doc-1", "doc-2"]).await;
    let key = view
        .register_remote(generation, lease(&clock))
        .await
        .unwrap();
    let access = Access::default();

    for (retriever, kind) in [
        ("search", RetrieverKind::RemoteQuery),
        ("search", RetrieverKind::RemoteEnumeration),
        ("lookup", RetrieverKind::DirectAddress),
        ("lookup", RetrieverKind::LiveOnly),
    ] {
        let hits = execute(&view, &access, &action(key.source_id, retriever, kind), key)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|(_, _, generation)| *generation == key));
    }
    // An action the seal never completed is an error, not an empty list.
    assert!(
        execute(
            &view,
            &access,
            &action(key.source_id, "never-run", RetrieverKind::RemoteQuery),
            key,
        )
        .await
        .is_err()
    );
    // Another generation UUID for the same Source is not this evaluation's pin.
    let stale_key = ProjectionGenerationKey {
        source_id: key.source_id,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::now_v7()),
    };
    assert!(
        execute(
            &view,
            &access,
            &action(key.source_id, "search", RetrieverKind::RemoteQuery),
            stale_key,
        )
        .await
        .is_err()
    );
    // A list whose trace names another generation is refused by the executor.
    let mut foreign = FederatedCandidate::new(
        "foreign",
        CandidateIdentityClass::DurableResource,
        key.source_id,
        "search",
    );
    foreign.resource_ref = Some(ResourceId::from_uuid(Uuid::now_v7()));
    foreign.retrieval_trace_ref = Some(format!(
        "{}:{}",
        key.source_id.as_uuid(),
        stale_key.generation_id.as_uuid()
    ));
    assert!(
        execute(
            &Stale(vec![foreign]),
            &access,
            &action(key.source_id, "search", RetrieverKind::RemoteQuery),
            key,
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn remote_graph_participant_access_is_current() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    let clock = ManualClock::new();
    let durable = Durable::default();
    let selectors = selectors();
    let view = view!(&durable, &selectors, &gate, clock.clone());
    let generation = sealed(&remote, &["doc-1", "doc-2"]).await;
    let ordered: Vec<ResourceId> = generation
        .candidates("search")
        .unwrap()
        .iter()
        .map(|candidate| candidate.resource_ref.unwrap())
        .collect();
    let key = view
        .register_remote(generation, lease(&clock))
        .await
        .unwrap();
    let access = Access::default();
    let search = action(key.source_id, "search", RetrieverKind::RemoteQuery);

    // Access granted at seal time is not reused: a revocation after the seal
    // removes the participant on the next execution.
    assert_eq!(
        execute(&view, &access, &search, key).await.unwrap().len(),
        2
    );
    access.deny(ordered[0]);
    assert_eq!(
        execute(&view, &access, &search, key).await.unwrap(),
        vec![(ordered[1], 1, key)]
    );
    // The sealed projection carries no provider relation or concept edge.
    let projection = GenerationReadPort::resource_at(&view, key, ordered[1])
        .await
        .unwrap()
        .unwrap();
    assert!(projection.relations.is_empty());
    assert_eq!(
        view.pin_view(key)
            .await
            .unwrap()
            .descendant_of("規程", "文書"),
        TruthValue::Unknown
    );
    // Revoking the Source ends every read of its generation at once.
    view.revoke_source(key.source_id);
    assert!(execute(&view, &access, &search, key).await.is_err());
    assert!(
        GenerationReadPort::resource_at(&view, key, ordered[1])
            .await
            .is_err()
    );
}
