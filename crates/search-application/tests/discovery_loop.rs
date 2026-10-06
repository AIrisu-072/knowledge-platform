use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use search_application::discovery_service::{
    DiscoveryConfig, DiscoveryPorts, DiscoveryService, TemporalPolicy,
};
use search_application::materialization::{
    ProbeBudget, ProbeCapability, ProbeEvidence, ProbeResult, ResourceCostEstimate,
};
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort,
    ConceptRegistryPort, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
    CurrentProbeCapability, CurrentSourcePolicy, CurrentSourcePolicyPort, DirectoryRetrieverPort,
    EvidenceResolverPort, GraphRetrievalHit, GraphRetrievalResult, HyperGraphRetrieverPort,
    LexicalQuery, LexicalRetrieverPort, ProbeCapabilityCatalogPort, ProbePort,
    ProjectionGenerationStore, ResolvedAssertionEvidence, SourceRegistryPort,
    StructuredFacetFilter, StructuredFacetOutcome, StructuredRetrievalHit, StructuredRetrieverPort,
};
use search_application::retrieval::{RetrievalInputs, RetrieverProfile, RetrieverSupport};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::routing::RoutingConstraints;
use search_core::applicability::{
    ApplicabilityState, Discriminator, DiscriminatorImportance, MinimumFactEvidence,
};
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::authority::AuthorityResolution;
use search_core::binding::RepresentationBinding;
use search_core::discovery::{
    CandidateIdentityClass, DiscoveryNeed, DiscoveryRequest, FederatedCandidate, GapReason,
};
use search_core::evidence::{EvidenceRequirement, EvidenceRole, EvidenceSufficiency};
use search_core::fact::{Fact, FactOrigin, FactSet};
use search_core::graph::{
    GraphPathEvidence, GraphPathStepEvidence, GraphTraversalPlan, RelationPathPattern,
    TraversalBudget,
};
use search_core::id::{
    ClaimId, DiscoveryEvaluationId, NeedId, ProjectionGenerationId, ResourceId, SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::materialization::{
    ProbeCompletenessSemantics, ProbeExecutionLocation, ProbeQueryMode, ProbeReturnType,
    ProviderContentPermission,
};
use search_core::observation::Coverage;
use search_core::predicate::{ConceptResolver, Operand, PredicateExpr, TruthValue, TypedValue};
use search_core::profile::FacetState;
use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection, ProjectionGenerationKey,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_core::relation::{RelationNamespace, RelationParticipant};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_core::temporal::{TemporalDiscoveryProfile, TemporalEvaluationContext};
use time::OffsetDateTime;
use uuid::Uuid;

fn sid(n: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(n))
}
fn rid(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}
fn cid(n: u128) -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(n))
}
fn gid(n: u128) -> ProjectionGenerationId {
    ProjectionGenerationId::from_uuid(Uuid::from_u128(n))
}

fn manifest(n: u128) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: sid(1),
        generation_id: gid(n),
        projection_schema_version: "v1".into(),
        lens_version: 1,
        semantic_registry_version: "v1".into(),
        analyzer_version: None,
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: format!("snap-{n}"),
        resource_count: 1,
        relation_count: Some(0),
        coverage: Coverage::CompleteEnumeration,
        digest: format!("digest-{n}"),
        built_at: OffsetDateTime::UNIX_EPOCH,
    }
}

fn projection(
    manifest: ProjectionGenerationManifest,
    facet: Option<bool>,
) -> CompiledResourceProjection {
    let mut typed_facets = BTreeMap::new();
    if let Some(value) = facet {
        typed_facets.insert(
            "suitable".into(),
            FacetState::Known(TypedValue::Bool(value)),
        );
    }
    CompiledResourceProjection {
        manifest,
        retention_mode: RetentionMode::PersistentDiscoveryMetadata,
        directory: DirectoryProjection {
            resource_ref: rid(10),
            resource_version: None,
            kind: ResourceKind::Knowledge,
            canonical_name: "synthetic".into(),
            title: None,
            aliases: vec![],
        },
        structured: StructuredProjection {
            resource_ref: rid(10),
            concept_refs: vec![],
            high_signal_facets: BTreeMap::new(),
            typed_facets,
            assertions: vec![],
            authority_resolutions: BTreeMap::new(),
        },
        temporal: TemporalProjection {
            resource_ref: rid(10),
            valid_from: None,
            valid_to: None,
            profile: TemporalDiscoveryProfile::default(),
        },
        access: AccessProjection {
            resource_ref: rid(10),
            access_scope: None,
            source_access_model: None,
        },
        relations: vec![],
    }
}

fn candidate(name: &str) -> FederatedCandidate {
    let mut candidate = FederatedCandidate::new(
        name,
        CandidateIdentityClass::DurableResource,
        sid(1),
        "structured",
    );
    candidate.resource_ref = Some(rid(10));
    candidate.locator = Some(format!("private://{name}"));
    candidate
}

fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::from_unix_timestamp(100).unwrap();
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(2)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find evidence".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![ResourceKind::Knowledge],
            required_claims: vec![cid(3)],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![cid(3)]),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(4)),
            now,
            now,
            "Asia/Tokyo",
        ),
        access_context: "principal".into(),
    }
}

fn config() -> DiscoveryConfig {
    DiscoveryConfig {
        routing: RoutingConstraints {
            required_source_ids: vec![sid(1)],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
        retriever_profile: RetrieverProfile::Capability,
        retriever_support: RetrieverSupport {
            structured: true,
            directory: true,
            lexical: true,
            vector: true,
            ..RetrieverSupport::default()
        },
        retrieval_inputs: RetrievalInputs {
            lexical_query: Some("evidence".into()),
            vector_query_available: true,
            max_initial_retrievers_per_source: 2,
            ..RetrievalInputs::default()
        },
        structured_filters: vec![StructuredFacetFilter::eq(
            "suitable",
            TypedValue::Bool(true),
        )],
        discriminators: vec![Discriminator::new(
            "suitable",
            DiscriminatorImportance::Hard,
            PredicateExpr::Eq(
                Operand::Fact("suitable".into()),
                Operand::Value(TypedValue::Bool(true)),
            ),
        )],
        lexical_query: Some(LexicalQuery::new("evidence", 10)),
        temporal_policy: TemporalPolicy {
            require_effective_at_target: false,
            max_current_age_seconds: None,
        },
        probe_budget: ProbeBudget {
            max_content_bytes: 1000,
            max_latency_ms: 100,
            max_remote_calls: 1,
            max_monetary_cost_minor_units: 0,
            currency: "USD".into(),
        },
        max_actions: 10,
        evaluation_currency: "USD".into(),
    }
}

struct NoConcept;
impl ConceptResolver for NoConcept {
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

struct Fixture {
    source: DiscoverableSource,
    current: Mutex<ProjectionGenerationManifest>,
    projections: BTreeMap<ProjectionGenerationKey, CompiledResourceProjection>,
    secondary_projection: Option<CompiledResourceProjection>,
    calls: Mutex<Vec<String>>,
    structured_hits: Vec<StructuredRetrievalHit>,
    directory_hits: Vec<FederatedCandidate>,
    denied: BTreeSet<String>,
    deny_after_first: BTreeSet<String>,
    revoke_candidate_on_evidence: BTreeSet<String>,
    revoke_candidate_on_probe: BTreeSet<String>,
    evidence_read: AtomicBool,
    probe_completed: AtomicBool,
    candidate_access_calls: Mutex<BTreeMap<String, usize>>,
    graph_deny_after_first: BTreeSet<ResourceId>,
    revoke_graph_on_evidence: BTreeSet<ResourceId>,
    graph_access_calls: Mutex<BTreeMap<ResourceId, usize>>,
    assertion: Option<Assertion>,
    reverse_evidence_refs_each_read: bool,
    assertion_reads: AtomicUsize,
    evidence: Option<ResolvedAssertionEvidence>,
    probe_available: bool,
    probe_result: Option<ProbeResult>,
    publish_on_structured: Option<ProjectionGenerationManifest>,
    pinned_reads: Mutex<Vec<ProjectionGenerationKey>>,
    graph_result: Option<GraphRetrievalResult>,
}

impl Fixture {
    fn sufficient() -> Self {
        let current = manifest(5);
        let mut source = DiscoverableSource::new(
            sid(1),
            "synthetic",
            EnumerationSemantics::Complete,
            RetentionMode::PersistentDiscoveryMetadata,
        );
        source.resource_types = vec![ResourceKind::Knowledge];
        source.discovery_modes = vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
            DiscoveryMode::RemoteQuery,
        ];
        let mut assertion = Assertion::new(
            "resource-10",
            "supports",
            TypedValue::Bool(true),
            "native-1",
            AssertionOrigin::Declared,
            "scope",
            OffsetDateTime::UNIX_EPOCH,
        );
        assertion.evidence_refs = vec!["evidence-1".into()];
        let evidence = ResolvedAssertionEvidence {
            generation: current.key(),
            source_id: sid(1),
            resource_id: rid(10),
            evidence_ref: "evidence-1".into(),
            upstream_origin: "publisher-1".into(),
            role: EvidenceRole::Primary,
            citation_chain: vec![],
            content_digest: Some("sha256:real".into()),
            is_summary: false,
        };
        Self {
            source,
            current: Mutex::new(current.clone()),
            projections: BTreeMap::from([(current.key(), projection(current, Some(true)))]),
            secondary_projection: None,
            calls: Mutex::new(vec![]),
            structured_hits: vec![StructuredRetrievalHit {
                candidate: candidate("visible"),
                outcomes: vec![StructuredFacetOutcome::Match],
            }],
            directory_hits: vec![candidate("visible")],
            denied: BTreeSet::new(),
            deny_after_first: BTreeSet::new(),
            revoke_candidate_on_evidence: BTreeSet::new(),
            revoke_candidate_on_probe: BTreeSet::new(),
            evidence_read: AtomicBool::new(false),
            probe_completed: AtomicBool::new(false),
            candidate_access_calls: Mutex::new(BTreeMap::new()),
            graph_deny_after_first: BTreeSet::new(),
            revoke_graph_on_evidence: BTreeSet::new(),
            graph_access_calls: Mutex::new(BTreeMap::new()),
            assertion: Some(assertion),
            reverse_evidence_refs_each_read: false,
            assertion_reads: AtomicUsize::new(0),
            evidence: Some(evidence),
            probe_available: true,
            probe_result: None,
            publish_on_structured: None,
            pinned_reads: Mutex::new(vec![]),
            graph_result: None,
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn service(&self, config: DiscoveryConfig) -> DiscoveryService<'_> {
        DiscoveryService::new(
            config,
            DiscoveryPorts {
                sources: self,
                generations: self,
                concepts: self,
                retrieval: RetrievalExecutionPorts {
                    remote: None,
                    directory: Some(self),
                    structured: Some(self),
                    lexical: Some(self),
                    hypergraph: Some(self),
                    graph_resource_access: Some(self),
                    access: self,
                    vector: None,
                },
                selectors: self,
                assertions: self,
                evidence: self,
                probe: self.probe_available.then_some(self),
                probe_catalog: Some(self),
                source_policy: Some(self),
            },
        )
        .unwrap()
    }
}

impl SourceRegistryPort for Fixture {
    fn get_source<'a>(&'a self, source_id: SourceId) -> BoxFuture<'a, Option<DiscoverableSource>> {
        Box::pin(async move { Ok((source_id == sid(1)).then(|| self.source.clone())) })
    }
    fn list_sources<'a>(&'a self) -> BoxFuture<'a, Vec<DiscoverableSource>> {
        Box::pin(async move { Ok(vec![self.source.clone()]) })
    }
}

impl ProjectionGenerationStore for Fixture {
    fn begin_generation<'a>(
        &'a self,
        _: search_application::projection::PersistableGenerationManifest,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn begin_incremental_generation<'a>(
        &'a self,
        _: search_application::projection::PersistableGenerationManifest,
        _: ProjectionGenerationKey,
        _: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn stage_resource<'a>(
        &'a self,
        _: search_application::projection::PersistableResourceProjection,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn stage_concept_registry<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: search_application::ports::SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn validate_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn publish_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn fail_generation<'a>(&'a self, _: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn pin_current<'a>(
        &'a self,
        _: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        Box::pin(async move { Ok(Some(self.current.lock().unwrap().clone())) })
    }
    fn resource_at<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        resource: ResourceId,
    ) -> BoxFuture<'a, Option<CompiledResourceProjection>> {
        self.pinned_reads.lock().unwrap().push(key);
        self.calls
            .lock()
            .unwrap()
            .push(format!("resource:{:?}", key.generation_id));
        Box::pin(async move {
            Ok(if resource == rid(10) {
                self.projections.get(&key).cloned()
            } else if resource == rid(11) {
                self.secondary_projection.clone()
            } else {
                None
            })
        })
    }
}

impl ConceptRegistryPort for Fixture {
    fn pin_view<'a>(
        &'a self,
        key: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Arc<dyn ConceptResolver + Send + Sync>> {
        self.pinned_reads.lock().unwrap().push(key);
        Box::pin(async { Ok(Arc::new(NoConcept) as Arc<dyn ConceptResolver + Send + Sync>) })
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

impl StructuredRetrieverPort for Fixture {
    fn retrieve<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a DiscoveryRequest,
        _: &'a [StructuredFacetFilter],
    ) -> BoxFuture<'a, Vec<StructuredRetrievalHit>> {
        self.calls.lock().unwrap().push("structured".into());
        if let Some(next) = &self.publish_on_structured {
            *self.current.lock().unwrap() = next.clone();
        }
        Box::pin(async move { Ok(self.structured_hits.clone()) })
    }
}
impl DirectoryRetrieverPort for Fixture {
    fn retrieve<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        self.calls.lock().unwrap().push("directory".into());
        Box::pin(async move { Ok(self.directory_hits.clone()) })
    }
}
impl LexicalRetrieverPort for Fixture {
    fn retrieve<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a DiscoveryRequest,
        _: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        self.calls.lock().unwrap().push("lexical".into());
        Box::pin(async { Ok(vec![]) })
    }
}
impl HyperGraphRetrieverPort for Fixture {
    fn retrieve<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: &'a GraphTraversalPlan,
    ) -> BoxFuture<'a, GraphRetrievalResult> {
        self.calls.lock().unwrap().push("hypergraph".into());
        Box::pin(async move { Ok(self.graph_result.clone().unwrap()) })
    }
}

impl CurrentCandidateAccessEvaluatorPort for Fixture {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        _: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        let mut calls = self.candidate_access_calls.lock().unwrap();
        let seen = calls.entry(candidate.candidate_id.clone()).or_default();
        *seen += 1;
        let deny = self.denied.contains(&candidate.candidate_id)
            || (self.deny_after_first.contains(&candidate.candidate_id) && *seen > 1)
            || (self.evidence_read.load(Ordering::SeqCst)
                && self
                    .revoke_candidate_on_evidence
                    .contains(&candidate.candidate_id))
            || (self.probe_completed.load(Ordering::SeqCst)
                && self
                    .revoke_candidate_on_probe
                    .contains(&candidate.candidate_id));
        Box::pin(async move {
            Ok(if deny {
                AccessDecision::Denied
            } else {
                AccessDecision::Allowed
            })
        })
    }
}
impl CurrentAccessEvaluatorPort for Fixture {
    fn evaluate<'a>(&'a self, resource: ResourceId, _: &'a str) -> BoxFuture<'a, AccessDecision> {
        let mut calls = self.graph_access_calls.lock().unwrap();
        let seen = calls.entry(resource).or_default();
        *seen += 1;
        let deny = (self.graph_deny_after_first.contains(&resource) && *seen > 1)
            || (self.evidence_read.load(Ordering::SeqCst)
                && self.revoke_graph_on_evidence.contains(&resource));
        Box::pin(async move {
            Ok(if deny {
                AccessDecision::Denied
            } else {
                AccessDecision::Allowed
            })
        })
    }
}

impl ClaimSelectorPort for Fixture {
    fn selector_for<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        self.pinned_reads.lock().unwrap().push(key);
        Box::pin(async move {
            Ok((claim_id == cid(3)).then(|| ClaimSelector {
                claim_id,
                subject_ref: "resource-10".into(),
                predicate: "supports".into(),
                expected_value: Some(TypedValue::Bool(true)),
            }))
        })
    }
}
impl AssertionStorePort for Fixture {
    fn assertions_for<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        _: ResourceId,
        _: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        self.pinned_reads.lock().unwrap().push(key);
        Box::pin(async move {
            let mut assertions: Vec<_> = self.assertion.clone().into_iter().collect();
            if self.reverse_evidence_refs_each_read
                && self.assertion_reads.fetch_add(1, Ordering::SeqCst) % 2 == 1
            {
                for assertion in &mut assertions {
                    assertion.evidence_refs.reverse();
                }
            }
            Ok(assertions)
        })
    }
}
impl EvidenceResolverPort for Fixture {
    fn resolve<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        resource: ResourceId,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        self.pinned_reads.lock().unwrap().push(key);
        self.evidence_read.store(true, Ordering::SeqCst);
        Box::pin(async move {
            Ok(self.evidence.clone().map(|mut evidence| {
                evidence.resource_id = resource;
                evidence.evidence_ref = evidence_ref.to_owned();
                evidence
            }))
        })
    }
}

impl ProbeCapabilityCatalogPort for Fixture {
    fn for_candidate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        facet: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, Option<CurrentProbeCapability>> {
        Box::pin(async move {
            Ok(Some(CurrentProbeCapability {
                candidate_id: candidate.candidate_id.clone(),
                resource_ref: candidate.resource_ref,
                facet: facet.into(),
                capability: ProbeCapability {
                    source_ref: sid(1),
                    probe_type: "facet-query".into(),
                    supported_resource_types: vec![ResourceKind::Knowledge],
                    supported_facets: vec![facet.into()],
                    query_mode: ProbeQueryMode::FacetExact,
                    return_types: vec![ProbeReturnType::Facts],
                    completeness_semantics: ProbeCompletenessSemantics::Partial,
                    location: ProbeExecutionLocation::Local,
                    coverage: Coverage::QueryResult,
                    estimated_cost: ResourceCostEstimate {
                        content_bytes: Some(1),
                        latency_ms: Some(1),
                        remote_calls: Some(0),
                        monetary_cost_minor_units: Some(0),
                        currency: Some("USD".into()),
                    },
                },
            }))
        })
    }
}
impl CurrentSourcePolicyPort for Fixture {
    fn for_candidate<'a>(
        &'a self,
        _: &'a FederatedCandidate,
        _: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        Box::pin(async {
            Ok(Some(CurrentSourcePolicy {
                resource_kind: ResourceKind::Knowledge,
                provider_permission: ProviderContentPermission::FullContent,
                retention_mode: RetentionMode::PersistentDiscoveryMetadata,
                probe_allowed: true,
            }))
        })
    }
    fn for_resource<'a>(
        &'a self,
        _: SourceId,
        _: ResourceId,
        _: &'a RepresentationBinding,
        _: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        Box::pin(async { Ok(None) })
    }
}
impl ProbePort for Fixture {
    fn probe<'a>(
        &'a self,
        _: &'a search_application::materialization::ProbeRequest,
    ) -> BoxFuture<'a, ProbeResult> {
        self.calls.lock().unwrap().push("probe".into());
        self.probe_completed.store(true, Ordering::SeqCst);
        Box::pin(async move {
            Ok(self
                .probe_result
                .clone()
                .unwrap_or_else(|| ProbeResult::failed("unset")))
        })
    }
}

#[tokio::test]
async fn structured_claim_evidence_stops_before_later_retrievers() {
    let fixture = Fixture::sufficient();
    let result = fixture.service(config()).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(result.qualified_resources.len(), 1);
    assert!(fixture.calls().contains(&"structured".into()));
    assert!(
        !fixture
            .calls()
            .iter()
            .any(|call| matches!(call.as_str(), "directory" | "lexical" | "vector" | "probe"))
    );
}

#[tokio::test]
async fn sufficient_claims_leave_alternate_candidate_gaps_nonblocking() {
    let mut fixture = Fixture::sufficient();
    let mut alternate = candidate("alternate");
    alternate.resource_ref = Some(rid(11));
    fixture.structured_hits.push(StructuredRetrievalHit {
        candidate: alternate,
        outcomes: vec![StructuredFacetOutcome::Unknown],
    });
    let result = fixture.service(config()).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(result.qualified_resources.len(), 1);
    assert!(
        result
            .unresolved_gaps
            .iter()
            .any(|gap| gap.required_fact == "suitable" && !gap.blocking)
    );
    assert!(!result.unresolved_gaps.iter().any(|gap| gap.blocking));
    assert!(
        !fixture
            .calls()
            .iter()
            .any(|call| call == "probe" || call == "directory")
    );
}

#[tokio::test]
async fn failed_alternate_probe_does_not_block_later_sufficient_resource() {
    let mut fixture = Fixture::sufficient();
    let mut alternate = candidate("alternate");
    alternate.resource_ref = Some(rid(11));
    fixture.structured_hits = vec![StructuredRetrievalHit {
        candidate: alternate,
        outcomes: vec![StructuredFacetOutcome::Unknown],
    }];
    fixture.probe_available = false;
    let mut settings = config();
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert!(fixture.calls().iter().any(|call| call == "directory"));
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(!result.unresolved_gaps.iter().any(|gap| gap.blocking));
    assert!(
        result.unresolved_gaps.iter().any(|gap| {
            gap.required_fact == "probe_execution_port_unavailable" && !gap.blocking
        })
    );
}

fn probe_evidence() -> ProbeEvidence {
    ProbeEvidence {
        source_ref: sid(1),
        candidate_id: "visible".into(),
        resource_ref: Some(rid(10)),
        facet: "suitable".into(),
        probe_type: "facet-query".into(),
        query_mode: ProbeQueryMode::FacetExact,
        completeness_semantics: ProbeCompletenessSemantics::Partial,
        provenance: "source-owned-probe".into(),
        coverage: Coverage::QueryResult,
    }
}

#[tokio::test]
async fn unknown_hard_discriminator_uses_targeted_probe_then_requalifies() {
    let mut fixture = Fixture::sufficient();
    let pinned = fixture.current.lock().unwrap().clone();
    fixture
        .projections
        .insert(pinned.key(), projection(pinned, None));
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    fixture.probe_result = Some(ProbeResult::found(facts, probe_evidence()));
    let result = fixture.service(config()).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(result.qualified_resources.len(), 1);
    assert!(
        result
            .retrieval_trace
            .contains(&"probe:00000000-0000-0000-0000-000000000001:visible:suitable:found".into())
    );
    assert_eq!(
        fixture
            .calls()
            .iter()
            .filter(|call| call.as_str() == "probe")
            .count(),
        1
    );
}

#[tokio::test]
async fn projection_conflict_cannot_be_probed_into_a_hard_discriminator_match() {
    let mut fixture = Fixture::sufficient();
    let pinned = fixture.current.lock().unwrap().clone();
    let mut detail = projection(pinned.clone(), None);
    detail
        .structured
        .typed_facets
        .insert("suitable".into(), FacetState::Conflict);
    fixture.projections.insert(pinned.key(), detail);
    fixture.structured_hits[0].outcomes.clear();
    fixture.directory_hits = vec![candidate("directory-alias")];
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    fixture.probe_result = Some(ProbeResult::found(facts, probe_evidence()));
    let mut settings = config();
    settings.structured_filters.clear();
    settings.discriminators[0].facet = "support-rule".into();
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    settings.max_actions = 4;

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert!(result.qualified_resources.is_empty());
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "suitable" && gap.reason == GapReason::Conflict && gap.blocking
    }));
    assert!(!result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "support-rule" && gap.reason == GapReason::MissingFact
    }));
    assert!(fixture.calls().iter().any(|call| call == "directory"));
    assert!(!fixture.calls().iter().any(|call| call == "probe"));
    assert!(
        !result.rejected_candidates.iter().any(|item| {
            item.candidate_id == "visible" || item.candidate_id == "directory-alias"
        })
    );
}

#[tokio::test]
async fn structured_projection_and_port_conflicts_remain_information_gaps() {
    for projection_conflict in [true, false] {
        let mut fixture = Fixture::sufficient();
        if projection_conflict {
            let pinned = fixture.current.lock().unwrap().clone();
            let mut detail = projection(pinned.clone(), None);
            detail
                .structured
                .typed_facets
                .insert("suitable".into(), FacetState::Conflict);
            fixture.projections.insert(pinned.key(), detail);
        } else {
            fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Conflict];
        }
        let result = fixture.service(config()).discover(request()).await.unwrap();
        assert!(result.qualified_resources.is_empty());
        assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
        assert!(result.unresolved_gaps.iter().any(|gap| {
            gap.required_fact == "suitable" && gap.reason == GapReason::Conflict && gap.blocking
        }));
        assert!(!fixture.calls().iter().any(|call| call == "probe"));
        assert!(result.rejected_candidates.is_empty());
    }
}

#[tokio::test]
async fn missing_projection_preserves_structured_conflict_as_blocking_gap() {
    let mut fixture = Fixture::sufficient();
    fixture.projections.clear();
    fixture.directory_hits.clear();
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Conflict];

    let result = fixture.service(config()).discover(request()).await.unwrap();
    assert!(result.qualified_resources.is_empty());
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "suitable" && gap.reason == GapReason::Conflict && gap.blocking
    }));
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "resource_projection" && gap.reason == GapReason::Availability
    }));
    assert!(!fixture.calls().iter().any(|call| call == "probe"));
}

#[tokio::test]
async fn supported_resource_does_not_clear_conflict_without_projection_detail() {
    let mut fixture = Fixture::sufficient();
    fixture.projections.clear();
    fixture.directory_hits.clear();
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Conflict];
    let pinned = fixture.current.lock().unwrap().clone();
    let mut supported = projection(pinned, Some(true));
    supported.directory.resource_ref = rid(11);
    supported.structured.resource_ref = rid(11);
    supported.temporal.resource_ref = rid(11);
    supported.access.resource_ref = rid(11);
    fixture.secondary_projection = Some(supported);
    let mut alternate = candidate("independent");
    alternate.resource_ref = Some(rid(11));
    fixture.structured_hits.push(StructuredRetrievalHit {
        candidate: alternate,
        outcomes: vec![StructuredFacetOutcome::Match],
    });

    let result = fixture.service(config()).discover(request()).await.unwrap();
    assert_eq!(result.qualified_resources.len(), 1);
    assert_eq!(result.qualified_resources[0].resource_ref, rid(11));
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "suitable" && gap.reason == GapReason::Conflict && gap.blocking
    }));
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "resource_projection" && gap.reason == GapReason::Availability
    }));
    assert!(!fixture.calls().iter().any(|call| call == "probe"));
}

#[tokio::test]
async fn missing_projection_not_applicable_outcome_excludes_without_probe() {
    let mut fixture = Fixture::sufficient();
    fixture.projections.clear();
    fixture.directory_hits.clear();
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::NotApplicable];

    let result = fixture.service(config()).discover(request()).await.unwrap();
    assert!(result.qualified_resources.is_empty());
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.rejected_candidates.iter().any(|item| {
        item.candidate_id == "visible" && item.state == ApplicabilityState::Excluded
    }));
    assert!(!fixture.calls().iter().any(|call| call == "probe"));
}

#[tokio::test]
async fn missing_projection_with_no_structured_filters_keeps_only_availability_boundary() {
    let mut fixture = Fixture::sufficient();
    fixture.projections.clear();
    fixture.directory_hits.clear();
    fixture.structured_hits[0].outcomes.clear();
    let mut settings = config();
    settings.structured_filters.clear();
    settings.discriminators.clear();

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert!(result.qualified_resources.is_empty());
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "resource_projection" && gap.reason == GapReason::Availability
    }));
    assert!(
        !result
            .unresolved_gaps
            .iter()
            .any(|gap| { gap.required_fact == "suitable" && gap.reason == GapReason::Conflict })
    );
    assert!(!fixture.calls().iter().any(|call| call == "probe"));
}

#[tokio::test]
async fn not_applicable_discriminator_fact_cannot_be_replaced_by_probe() {
    let mut fixture = Fixture::sufficient();
    let pinned = fixture.current.lock().unwrap().clone();
    let mut detail = projection(pinned.clone(), None);
    detail
        .structured
        .typed_facets
        .insert("suitable".into(), FacetState::NotApplicable);
    fixture.projections.insert(pinned.key(), detail);
    fixture.structured_hits[0].outcomes.clear();
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    fixture.probe_result = Some(ProbeResult::found(facts, probe_evidence()));
    let mut settings = config();
    settings.structured_filters.clear();
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert!(result.qualified_resources.is_empty());
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.rejected_candidates.iter().any(|item| {
        item.candidate_id == "visible" && item.state == ApplicabilityState::Excluded
    }));
    assert!(!fixture.calls().iter().any(|call| call == "probe"));
}

#[tokio::test]
async fn conflicting_probe_leaves_hard_structured_filter_unresolved() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(false), FactOrigin::Observed),
    );
    fixture.probe_result = Some(ProbeResult::found(facts, probe_evidence()));
    let mut settings = config();
    settings.retriever_support = RetrieverSupport {
        structured: true,
        ..RetrieverSupport::default()
    };
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    settings.max_actions = 2;

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(
        fixture
            .calls()
            .iter()
            .filter(|call| *call == "probe")
            .count(),
        1
    );
    assert!(result.qualified_resources.is_empty());
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "suitable" && gap.reason == GapReason::Conflict && gap.blocking
    }));
    assert!(result.rejected_candidates.is_empty());
}

#[tokio::test]
async fn conflicting_probe_leaves_hard_discriminator_unresolved() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits[0].outcomes.clear();
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(false), FactOrigin::Authoritative),
    );
    fixture.probe_result = Some(ProbeResult::found(facts, probe_evidence()));
    let mut settings = config();
    settings.structured_filters.clear();
    settings.discriminators.insert(
        0,
        Discriminator::new(
            "other",
            DiscriminatorImportance::Hard,
            PredicateExpr::Eq(
                Operand::Value(TypedValue::Bool(true)),
                Operand::Value(TypedValue::Bool(true)),
            ),
        ),
    );
    settings.discriminators[1].minimum_fact_evidence = MinimumFactEvidence::AuthoritativeOrExplicit;
    settings.retriever_support = RetrieverSupport {
        structured: true,
        ..RetrieverSupport::default()
    };
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    settings.max_actions = 2;

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(
        fixture
            .calls()
            .iter()
            .filter(|call| *call == "probe")
            .count(),
        1
    );
    assert!(result.qualified_resources.is_empty());
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "suitable" && gap.reason == GapReason::Conflict && gap.blocking
    }));
    assert!(result.rejected_candidates.is_empty());
}

#[tokio::test]
async fn later_retriever_cannot_qualify_candidate_with_conflicting_probe() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
    fixture.directory_hits = vec![candidate("directory-alias")];
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(false), FactOrigin::Observed),
    );
    fixture.probe_result = Some(ProbeResult::found(facts, probe_evidence()));
    let mut settings = config();
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    settings.max_actions = 4;

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert!(fixture.calls().iter().any(|call| call == "directory"));
    assert!(result.qualified_resources.is_empty());
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "suitable" && gap.reason == GapReason::Conflict && gap.blocking
    }));
    assert!(result.rejected_candidates.is_empty());
    assert_eq!(
        fixture
            .calls()
            .iter()
            .filter(|call| *call == "probe")
            .count(),
        1
    );
}

#[tokio::test]
async fn conflict_gap_survives_an_independent_hard_exclusion_but_not_access_denial() {
    for denied in [false, true] {
        let mut fixture = Fixture::sufficient();
        let pinned = fixture.current.lock().unwrap().clone();
        let mut detail = projection(pinned.clone(), None);
        detail
            .structured
            .typed_facets
            .insert("suitable".into(), FacetState::Conflict);
        fixture.projections.insert(pinned.key(), detail);
        fixture.structured_hits[0].outcomes.clear();
        fixture.directory_hits.clear();
        if denied {
            fixture.denied.insert("visible".into());
        }
        let mut settings = config();
        settings.structured_filters.clear();
        settings.discriminators.push(Discriminator::new(
            "independent-mismatch",
            DiscriminatorImportance::Hard,
            PredicateExpr::Eq(
                Operand::Value(TypedValue::Bool(false)),
                Operand::Value(TypedValue::Bool(true)),
            ),
        ));
        let result = fixture.service(settings).discover(request()).await.unwrap();
        assert!(result.qualified_resources.is_empty());
        assert_eq!(
            result.unresolved_gaps.iter().any(|gap| {
                gap.required_fact == "suitable" && gap.reason == GapReason::Conflict
            }),
            !denied
        );
        if denied {
            assert!(!format!("{result:?}").contains("visible"));
        } else {
            assert!(result.rejected_candidates.iter().any(|item| {
                item.candidate_id == "visible" && item.state == ApplicabilityState::Excluded
            }));
        }
    }
}

#[tokio::test]
async fn independent_supported_resource_does_not_clear_default_blocking_conflict() {
    let mut fixture = Fixture::sufficient();
    let pinned = fixture.current.lock().unwrap().clone();
    let mut conflicted = projection(pinned.clone(), None);
    conflicted
        .structured
        .typed_facets
        .insert("suitable".into(), FacetState::Conflict);
    fixture.projections.insert(pinned.key(), conflicted);
    let mut supported = projection(pinned, Some(true));
    supported.directory.resource_ref = rid(11);
    supported.structured.resource_ref = rid(11);
    supported.temporal.resource_ref = rid(11);
    supported.access.resource_ref = rid(11);
    fixture.secondary_projection = Some(supported);
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
    let mut alternate = candidate("independent");
    alternate.resource_ref = Some(rid(11));
    fixture.structured_hits.push(StructuredRetrievalHit {
        candidate: alternate,
        outcomes: vec![StructuredFacetOutcome::Match],
    });

    let result = fixture.service(config()).discover(request()).await.unwrap();
    assert_eq!(result.qualified_resources.len(), 1);
    assert_eq!(result.qualified_resources[0].resource_ref, rid(11));
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().any(|gap| {
        gap.required_fact == "suitable" && gap.reason == GapReason::Conflict && gap.blocking
    }));
}

#[tokio::test]
async fn structured_conflict_gap_survives_resource_type_exclusion() {
    for projection_conflict in [true, false] {
        let mut fixture = Fixture::sufficient();
        let pinned = fixture.current.lock().unwrap().clone();
        let mut detail = projection(pinned.clone(), Some(true));
        detail.directory.kind = ResourceKind::Policy;
        if projection_conflict {
            detail
                .structured
                .typed_facets
                .insert("suitable".into(), FacetState::Conflict);
        }
        fixture.projections.insert(pinned.key(), detail);
        fixture.structured_hits[0].outcomes = vec![if projection_conflict {
            StructuredFacetOutcome::Unknown
        } else {
            StructuredFacetOutcome::Conflict
        }];
        fixture.directory_hits.clear();
        let mut settings = config();
        settings.discriminators.clear();

        let result = fixture.service(settings).discover(request()).await.unwrap();
        assert!(result.qualified_resources.is_empty());
        assert!(result.rejected_candidates.iter().any(|item| {
            item.candidate_id == "visible" && item.state == ApplicabilityState::Excluded
        }));
        assert!(
            result.unresolved_gaps.iter().any(|gap| {
                gap.required_fact == "suitable" && gap.reason == GapReason::Conflict
            })
        );
    }
}

#[tokio::test]
async fn predicate_type_error_remains_invalid_beside_a_factual_conflict() {
    let mut fixture = Fixture::sufficient();
    let pinned = fixture.current.lock().unwrap().clone();
    let mut detail = projection(pinned.clone(), None);
    detail
        .structured
        .typed_facets
        .insert("suitable".into(), FacetState::Conflict);
    fixture.projections.insert(pinned.key(), detail);
    fixture.structured_hits[0].outcomes.clear();
    fixture.directory_hits.clear();
    let mut settings = config();
    settings.structured_filters.clear();
    settings.discriminators.push(Discriminator::new(
        "malformed-rule",
        DiscriminatorImportance::Hard,
        PredicateExpr::Eq(
            Operand::Value(TypedValue::Integer(1)),
            Operand::Value(TypedValue::Bool(true)),
        ),
    ));

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert!(result.qualified_resources.is_empty());
    assert!(result.rejected_candidates.iter().any(|item| {
        item.candidate_id == "visible" && item.state == ApplicabilityState::Invalid
    }));
    assert!(
        result
            .unresolved_gaps
            .iter()
            .any(|gap| { gap.required_fact == "suitable" && gap.reason == GapReason::Conflict })
    );
    assert!(!fixture.calls().iter().any(|call| call == "probe"));
}

#[tokio::test]
async fn probe_fact_for_one_resource_does_not_change_another_resource() {
    let mut fixture = Fixture::sufficient();
    let mut alternate = candidate("alternate");
    alternate.resource_ref = Some(rid(11));
    fixture.structured_hits = vec![StructuredRetrievalHit {
        candidate: alternate,
        outcomes: vec![StructuredFacetOutcome::Unknown],
    }];
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(false), FactOrigin::Observed),
    );
    let mut evidence = probe_evidence();
    evidence.candidate_id = "alternate".into();
    evidence.resource_ref = Some(rid(11));
    fixture.probe_result = Some(ProbeResult::found(facts, evidence));
    let mut settings = config();
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    settings.max_actions = 4;

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert!(fixture.calls().iter().any(|call| call == "probe"));
    assert!(fixture.calls().iter().any(|call| call == "directory"));
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(result.qualified_resources.len(), 1);
    assert_eq!(result.qualified_resources[0].resource_ref, rid(10));
}

#[tokio::test]
async fn later_alias_inherits_probe_miss_outcome_without_a_negative_fact() {
    let mut fixture = Fixture::sufficient();
    let pinned = fixture.current.lock().unwrap().clone();
    fixture
        .projections
        .insert(pinned.key(), projection(pinned, None));
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
    fixture.directory_hits = vec![candidate("directory-alias")];
    fixture.probe_result = Some(ProbeResult::not_found_by_probe(probe_evidence()));
    let mut settings = config();
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    settings.max_actions = 3;

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert!(fixture.calls().iter().any(|call| call == "directory"));
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.qualified_resources.is_empty());
    assert_eq!(
        result
            .retrieval_trace
            .iter()
            .filter(|trace| trace.starts_with("probe:"))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "probe:00000000-0000-0000-0000-000000000001:visible:suitable:not_found_by_probe:partial"
        ]
    );
}

#[tokio::test]
async fn initial_directory_hit_cannot_launder_unknown_structured_gate() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
    fixture.directory_hits = vec![candidate("directory-alias")];
    let mut settings = config();
    settings.max_actions = 2;

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert!(fixture.calls().iter().any(|call| call == "directory"));
    assert!(result.qualified_resources.is_empty());
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(!fixture.calls().iter().any(|call| call == "probe"));
}

#[tokio::test]
async fn matching_probe_resolves_shared_unknown_after_initial_directory_hit() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
    fixture.directory_hits = vec![candidate("z-directory-alias")];
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    fixture.probe_result = Some(ProbeResult::found(facts, probe_evidence()));
    let mut settings = config();
    settings.max_actions = 3;

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(
        fixture
            .calls()
            .iter()
            .filter(|call| *call == "probe")
            .count(),
        1
    );
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(result.qualified_resources.len(), 1);
    assert_eq!(
        result
            .retrieval_trace
            .iter()
            .filter(|trace| trace.starts_with("probe:"))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["probe:00000000-0000-0000-0000-000000000001:visible:suitable:found"]
    );
}

#[tokio::test]
async fn shared_found_probe_state_is_removed_when_original_target_is_revoked() {
    for (projection_facet, revoke_during_evidence) in [
        (Some(true), false),
        (Some(true), true),
        (None, false),
        (None, true),
    ] {
        let mut fixture = Fixture::sufficient();
        let pinned = fixture.current.lock().unwrap().clone();
        fixture
            .projections
            .insert(pinned.key(), projection(pinned, projection_facet));
        fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
        fixture.directory_hits = vec![candidate("z-directory-alias")];
        if revoke_during_evidence {
            fixture
                .revoke_candidate_on_evidence
                .insert("visible".into());
        } else {
            fixture.revoke_candidate_on_probe.insert("visible".into());
        }
        let mut facts = FactSet::default();
        facts.insert(
            "suitable",
            Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
        );
        fixture.probe_result = Some(ProbeResult::found(facts, probe_evidence()));
        let mut settings = config();
        settings.max_actions = 3;

        let result = fixture.service(settings).discover(request()).await.unwrap();
        assert_eq!(
            result.evidence_sufficiency == EvidenceSufficiency::Sufficient,
            projection_facet.is_some()
        );
        assert_eq!(
            result.qualified_resources.len(),
            usize::from(projection_facet.is_some())
        );
        assert!(
            result
                .retrieval_trace
                .iter()
                .all(|trace| !trace.starts_with("probe:"))
        );
        assert!(!format!("{result:?}").contains("visible"));
        assert!(!format!("{result:?}").contains("private://visible"));
    }
}

#[tokio::test]
async fn initial_directory_hit_cannot_launder_explicit_structured_rejection() {
    for outcome in [
        StructuredFacetOutcome::Mismatch,
        StructuredFacetOutcome::Conflict,
    ] {
        let mut fixture = Fixture::sufficient();
        fixture.structured_hits[0].outcomes = vec![outcome];
        fixture.directory_hits = vec![candidate("directory-alias")];
        let mut settings = config();
        settings.max_actions = 2;

        let result = fixture.service(settings).discover(request()).await.unwrap();
        assert!(fixture.calls().iter().any(|call| call == "directory"));
        assert!(result.qualified_resources.is_empty());
        assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
        if outcome == StructuredFacetOutcome::Conflict {
            assert!(result.unresolved_gaps.iter().any(|gap| {
                gap.required_fact == "suitable" && gap.reason == GapReason::Conflict
            }));
            assert!(result.rejected_candidates.is_empty());
            assert!(!fixture.calls().iter().any(|call| call == "probe"));
        } else {
            assert!(
                result
                    .rejected_candidates
                    .iter()
                    .any(|candidate| candidate.candidate_id == "directory-alias")
            );
        }
    }
}

#[tokio::test]
async fn matching_probe_requalifies_known_projection_with_unknown_structured_outcome() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    fixture.probe_result = Some(ProbeResult::found(facts, probe_evidence()));
    let mut settings = config();
    settings.retriever_support = RetrieverSupport {
        structured: true,
        ..RetrieverSupport::default()
    };
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    settings.max_actions = 2;

    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(
        fixture
            .calls()
            .iter()
            .filter(|call| *call == "probe")
            .count(),
        1
    );
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(result.qualified_resources.len(), 1);
    assert!(result.rejected_candidates.is_empty());
}

#[tokio::test]
async fn probe_miss_never_becomes_a_negative_fact_and_is_not_retried_without_progress() {
    let mut fixture = Fixture::sufficient();
    let pinned = fixture.current.lock().unwrap().clone();
    fixture
        .projections
        .insert(pinned.key(), projection(pinned, None));
    fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
    fixture.probe_result = Some(ProbeResult::not_found_by_probe(probe_evidence()));
    let mut settings = config();
    settings.max_actions = 5;
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(result.qualified_resources.is_empty());
    assert_eq!(
        fixture
            .calls()
            .iter()
            .filter(|call| call.as_str() == "probe")
            .count(),
        1
    );
}

#[tokio::test]
async fn authorized_probe_outcomes_remain_distinct_without_exposing_provider_reasons() {
    let outcomes = [
        (
            ProbeResult::not_found_by_probe(probe_evidence()),
            "probe:00000000-0000-0000-0000-000000000001:visible:suitable:not_found_by_probe:partial",
        ),
        (
            ProbeResult::unsupported("private-provider-error"),
            "probe:00000000-0000-0000-0000-000000000001:visible:suitable:unsupported",
        ),
        (
            ProbeResult::failed("private-provider-error"),
            "probe:00000000-0000-0000-0000-000000000001:visible:suitable:failed",
        ),
    ];
    for (outcome, expected) in outcomes {
        let mut fixture = Fixture::sufficient();
        let pinned = fixture.current.lock().unwrap().clone();
        fixture
            .projections
            .insert(pinned.key(), projection(pinned, None));
        fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
        fixture.probe_result = Some(outcome);
        let mut settings = config();
        settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
        settings.max_actions = 2;
        let result = fixture.service(settings).discover(request()).await.unwrap();
        assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
        assert!(result.qualified_resources.is_empty());
        assert_eq!(
            result
                .retrieval_trace
                .iter()
                .filter(|trace| trace.starts_with("probe:"))
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![expected],
        );
        assert!(!format!("{result:?}").contains("private-provider-error"));
    }
}

#[tokio::test]
async fn failed_probe_outcome_does_not_block_later_sufficient_claim() {
    let mut fixture = Fixture::sufficient();
    let mut alternate = candidate("alternate");
    alternate.resource_ref = Some(rid(11));
    fixture.structured_hits = vec![StructuredRetrievalHit {
        candidate: alternate,
        outcomes: vec![StructuredFacetOutcome::Unknown],
    }];
    fixture.probe_result = Some(ProbeResult::failed("private-provider-error"));
    let mut settings = config();
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(result.qualified_resources.len(), 1);
    assert!(!result.unresolved_gaps.iter().any(|gap| gap.blocking));
    assert!(
        result.retrieval_trace.contains(
            &"probe:00000000-0000-0000-0000-000000000001:alternate:suitable:failed".into()
        )
    );
    assert!(!format!("{result:?}").contains("private-provider-error"));
}

#[tokio::test]
async fn no_hits_terminate_unresolved_with_finite_actions() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits.clear();
    fixture.directory_hits.clear();
    let mut settings = config();
    settings.max_actions = 2;
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(result.qualified_resources.is_empty());
    assert_eq!(
        fixture
            .calls()
            .iter()
            .filter(|call| matches!(call.as_str(), "structured" | "directory"))
            .count(),
        2
    );
}

#[tokio::test]
async fn required_remote_only_source_is_explicitly_unresolved() {
    let mut fixture = Fixture::sufficient();
    fixture.source.discovery_modes = vec![DiscoveryMode::RemoteQuery];
    let result = fixture.service(config()).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(
        result
            .unresolved_gaps
            .iter()
            .any(|gap| gap.blocking && gap.required_fact.contains("source:"))
    );
    assert!(
        result
            .unresolved_gaps
            .iter()
            .any(|gap| gap.required_fact.contains("RemoteQuery")
                || gap.required_fact.contains("RuntimePortUnavailable"))
    );
    assert!(!fixture.calls().iter().any(|call| call == "structured"));
}

#[tokio::test]
async fn denied_candidate_has_no_identity_or_rank_leak_in_result() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits[0].candidate = candidate("private");
    fixture.denied.insert("private".into());
    fixture.directory_hits.clear();
    let mut settings = config();
    settings.max_actions = 1;
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(result.qualified_resources.is_empty());
    assert!(result.rejected_candidates.is_empty());
    let printed = format!("{result:?}");
    assert!(!printed.contains("private"));
    assert!(!printed.contains("candidate:1"));
}

#[tokio::test]
async fn revocation_between_retrieval_and_detail_hides_hit_and_compacts_visible_rank() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits.insert(
        0,
        StructuredRetrievalHit {
            candidate: candidate("revoked"),
            outcomes: vec![StructuredFacetOutcome::Match],
        },
    );
    fixture.deny_after_first.insert("revoked".into());
    let mut settings = config();
    settings.max_actions = 1;
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    let printed = format!("{result:?}");
    assert!(!printed.contains("revoked"));
    assert!(
        result
            .retrieval_trace
            .iter()
            .any(|trace| trace.ends_with(":1:visible"))
    );
}

#[tokio::test]
async fn candidate_revoked_during_evidence_reads_leaves_no_claim_or_trace() {
    let mut fixture = Fixture::sufficient();
    fixture
        .revoke_candidate_on_evidence
        .insert("visible".into());
    let mut settings = config();
    settings.max_actions = 1;
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(result.evidence_set.is_empty());
    assert!(result.qualified_resources.is_empty());
    let printed = format!("{result:?}");
    assert!(!printed.contains("visible"));
    assert!(!printed.contains("evidence-1"));
}

#[tokio::test]
async fn revocation_during_evidence_reads_requalifies_a_surviving_hit() {
    let mut fixture = Fixture::sufficient();
    fixture.structured_hits.insert(
        0,
        StructuredRetrievalHit {
            candidate: candidate("revoked"),
            outcomes: vec![StructuredFacetOutcome::Match],
        },
    );
    fixture
        .revoke_candidate_on_evidence
        .insert("revoked".into());
    let mut settings = config();
    settings.max_actions = 1;
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(result.qualified_resources.len(), 1);
    assert_eq!(result.evidence_set.len(), 1);
    assert!(!format!("{result:?}").contains("revoked"));
    assert!(
        result
            .retrieval_trace
            .iter()
            .any(|trace| trace.ends_with(":1:visible"))
    );
}

#[tokio::test]
async fn probe_outcome_is_hidden_when_target_is_revoked_during_probe() {
    for outcome in [
        ProbeResult::not_found_by_probe(probe_evidence()),
        ProbeResult::unsupported("private-provider-error"),
        ProbeResult::failed("private-provider-error"),
    ] {
        let mut fixture = Fixture::sufficient();
        let pinned = fixture.current.lock().unwrap().clone();
        fixture
            .projections
            .insert(pinned.key(), projection(pinned, None));
        fixture.structured_hits[0].outcomes = vec![StructuredFacetOutcome::Unknown];
        fixture.probe_result = Some(outcome);
        fixture.revoke_candidate_on_probe.insert("visible".into());
        let mut settings = config();
        settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
        settings.max_actions = 2;
        let result = fixture.service(settings).discover(request()).await.unwrap();
        assert_eq!(
            fixture
                .calls()
                .iter()
                .filter(|call| *call == "probe")
                .count(),
            1
        );
        assert!(
            result
                .retrieval_trace
                .iter()
                .all(|trace| !trace.starts_with("probe:"))
        );
        assert!(!format!("{result:?}").contains("visible"));
        assert!(!format!("{result:?}").contains("private-provider-error"));
    }
}

#[tokio::test]
async fn probe_outcome_is_removed_when_evidence_reads_revoke_its_target() {
    let mut fixture = Fixture::sufficient();
    let mut alternate = candidate("alternate");
    alternate.resource_ref = Some(rid(11));
    fixture.structured_hits = vec![StructuredRetrievalHit {
        candidate: alternate,
        outcomes: vec![StructuredFacetOutcome::Unknown],
    }];
    fixture.probe_result = Some(ProbeResult::failed("private-provider-error"));
    fixture
        .revoke_candidate_on_evidence
        .insert("alternate".into());
    let mut settings = config();
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    let result = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(result.qualified_resources.len(), 1);
    assert!(!result.unresolved_gaps.iter().any(|gap| gap.blocking));
    assert!(
        result
            .retrieval_trace
            .iter()
            .all(|trace| !trace.starts_with("probe:"))
    );
    assert!(!format!("{result:?}").contains("alternate"));
    assert!(!format!("{result:?}").contains("private-provider-error"));
}

#[tokio::test]
async fn reordered_evidence_refs_do_not_reprobe_unchanged_known_state() {
    let mut fixture = Fixture::sufficient();
    fixture.assertion.as_mut().unwrap().evidence_refs =
        vec!["evidence-1".into(), "evidence-2".into()];
    fixture.reverse_evidence_refs_each_read = true;
    let mut alternate = candidate("alternate");
    alternate.resource_ref = Some(rid(11));
    fixture.structured_hits.push(StructuredRetrievalHit {
        candidate: alternate,
        outcomes: vec![StructuredFacetOutcome::Unknown],
    });
    fixture.probe_result = Some(ProbeResult::not_found_by_probe(ProbeEvidence {
        candidate_id: "alternate".into(),
        resource_ref: Some(rid(11)),
        ..probe_evidence()
    }));
    let mut needs = request();
    needs
        .need
        .completion_requirement
        .minimum_independent_sources = 2;
    let mut settings = config();
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    settings.max_actions = 4;
    let result = fixture.service(settings).discover(needs).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert_eq!(
        fixture
            .calls()
            .iter()
            .filter(|call| *call == "probe")
            .count(),
        1
    );
}

#[tokio::test]
async fn evaluation_keeps_n_after_n_plus_one_is_published_during_retrieval() {
    let mut fixture = Fixture::sufficient();
    fixture.publish_on_structured = Some(manifest(6));
    let result = fixture.service(config()).discover(request()).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(fixture.current.lock().unwrap().generation_id, gid(6));
    let reads = fixture.pinned_reads.lock().unwrap();
    assert!(!reads.is_empty());
    assert!(reads.iter().all(|key| key.generation_id == gid(5)));
}

#[tokio::test]
async fn temporal_gate_checks_the_intersection_of_identity_and_profile_ranges() {
    let target = request().temporal_context.temporal_target;
    for late_profile_start in [true, false] {
        let mut fixture = Fixture::sufficient();
        let key = fixture.current.lock().unwrap().key();
        let detail = fixture.projections.get_mut(&key).unwrap();
        if late_profile_start {
            detail.temporal.valid_from = Some(target - time::Duration::seconds(20));
            detail.temporal.profile.effective_from = Some(target + time::Duration::seconds(1));
        } else {
            detail.temporal.valid_to = Some(target + time::Duration::seconds(20));
            detail.temporal.profile.effective_to = Some(target - time::Duration::seconds(1));
        }
        let mut settings = config();
        settings.max_actions = 1;
        let result = fixture.service(settings).discover(request()).await.unwrap();
        assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
        assert!(result.qualified_resources.is_empty());
        assert!(
            result
                .rejected_candidates
                .iter()
                .any(|item| item.candidate_id == "visible")
        );
    }
}

#[tokio::test]
async fn typed_facet_does_not_claim_authority_without_matching_resolved_assertion() {
    let mut fixture = Fixture::sufficient();
    let mut settings = config();
    settings.discriminators[0].minimum_fact_evidence = MinimumFactEvidence::AuthoritativeOrExplicit;
    settings.max_actions = 1;
    let unresolved = fixture
        .service(settings.clone())
        .discover(request())
        .await
        .unwrap();
    assert_eq!(
        unresolved.evidence_sufficiency,
        EvidenceSufficiency::Unresolved
    );
    assert!(unresolved.qualified_resources.is_empty());

    let key = fixture.current.lock().unwrap().key();
    let detail = fixture.projections.get_mut(&key).unwrap();
    detail.structured.authority_resolutions.insert(
        "suitable".into(),
        AuthorityResolution::Resolved(TypedValue::Bool(true)),
    );
    detail.structured.assertions.push(Assertion::new(
        "resource-10",
        "suitable",
        TypedValue::Bool(true),
        "native-1",
        AssertionOrigin::Authoritative,
        "scope",
        OffsetDateTime::UNIX_EPOCH,
    ));
    let resolved = fixture.service(settings).discover(request()).await.unwrap();
    assert_eq!(
        resolved.evidence_sufficiency,
        EvidenceSufficiency::Sufficient
    );
}

#[tokio::test]
async fn unresolved_or_copied_claim_reference_cannot_complete_discovery() {
    for copied in [false, true] {
        let mut fixture = Fixture::sufficient();
        if copied {
            fixture.evidence.as_mut().unwrap().is_summary = true;
        } else {
            fixture.evidence = None;
        }
        let mut settings = config();
        settings.max_actions = 2;
        let result = fixture.service(settings).discover(request()).await.unwrap();
        assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    }
}

#[tokio::test]
async fn opaque_authority_and_freshness_requirements_remain_unresolved() {
    let fixture = Fixture::sufficient();
    let mut needs = request();
    needs
        .need
        .authority_requirements
        .push("policy-owner".into());
    needs.need.freshness_requirements.push("current".into());
    needs
        .need
        .completion_requirement
        .authority_requirements
        .push("policy-owner".into());
    needs
        .need
        .completion_requirement
        .freshness_requirements
        .push("current".into());
    let mut settings = config();
    settings.max_actions = 1;
    let result = fixture.service(settings).discover(needs).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(
        result
            .unresolved_gaps
            .iter()
            .any(|gap| gap.reason == search_core::discovery::GapReason::Authority)
    );
    assert!(
        result
            .unresolved_gaps
            .iter()
            .any(|gap| gap.reason == search_core::discovery::GapReason::Freshness)
    );
}

fn graph_plan(request: &DiscoveryRequest) -> GraphTraversalPlan {
    GraphTraversalPlan {
        seed_nodes: vec![rid(20)],
        path_patterns: vec![RelationPathPattern::new(
            RelationNamespace::Discovery,
            "connects",
            "from",
            "to",
        )],
        allowed_relation_types: vec!["connects".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: None,
        temporal_context: Some(request.temporal_context.clone()),
        access_context: request.access_context.clone(),
        expansion_budget: TraversalBudget {
            max_hops: 1,
            max_relations: 1,
            max_branching_per_node: 1,
            max_seed_nodes: 1,
            max_paths: 1,
        },
        stop_conditions: vec![],
    }
}

fn graph_setup() -> (Fixture, DiscoveryConfig, DiscoveryRequest) {
    let mut fixture = Fixture::sufficient();
    let mut graph_candidate = FederatedCandidate::new(
        "graph-only",
        CandidateIdentityClass::DurableResource,
        sid(1),
        "hypergraph",
    );
    graph_candidate.resource_ref = Some(rid(21));
    graph_candidate.matched_signals.push("relation".into());
    let path = GraphPathEvidence {
        resource_path: vec![rid(20), rid(21)],
        steps: vec![GraphPathStepEvidence {
            relation_id: search_core::id::RelationId::from_uuid(Uuid::from_u128(30)),
            namespace: RelationNamespace::Discovery,
            relation_type: "connects".into(),
            from_role: "from".into(),
            from_resource: rid(20),
            to_role: "to".into(),
            to_resource: rid(21),
            participants: vec![
                RelationParticipant::new("from", rid(20)),
                RelationParticipant::new("to", rid(21)),
            ],
            evidence_refs: vec!["graph-evidence".into()],
            provenance: Some("source".into()),
        }],
    };
    fixture.graph_result = Some(GraphRetrievalResult {
        generation: fixture.current.lock().unwrap().key(),
        hits: vec![GraphRetrievalHit {
            candidate: graph_candidate,
            paths: vec![path],
        }],
    });
    let request = request();
    let mut settings = config();
    settings.retriever_profile = RetrieverProfile::EvidenceInvestigation;
    settings.retriever_support = RetrieverSupport {
        hypergraph: true,
        ..RetrieverSupport::default()
    };
    settings
        .retrieval_inputs
        .graph_plans
        .insert(sid(1), graph_plan(&request));
    settings.retrieval_inputs.max_initial_retrievers_per_source = 1;
    settings.max_actions = 1;
    (fixture, settings, request)
}

#[tokio::test]
async fn graph_path_without_resource_claim_is_trace_only() {
    let (fixture, settings, request) = graph_setup();
    let result = fixture.service(settings).discover(request).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(result.qualified_resources.is_empty());
    assert!(result.evidence_set.is_empty());
    assert!(
        result
            .retrieval_trace
            .iter()
            .any(|item| item.starts_with("graph_path:"))
    );
}

#[tokio::test]
async fn graph_path_revocation_before_final_result_hides_the_whole_hit() {
    let (mut fixture, settings, request) = graph_setup();
    fixture.graph_deny_after_first.insert(rid(20));
    let result = fixture.service(settings).discover(request).await.unwrap();
    let printed = format!("{result:?}");
    assert!(!printed.contains("graph-only"));
    assert!(!printed.contains("graph_path:"));
    assert!(result.qualified_resources.is_empty());
}

#[tokio::test]
async fn graph_path_revoked_during_evidence_reads_leaves_no_claim_or_trace() {
    let (mut fixture, settings, request) = graph_setup();
    let graph_hit = &mut fixture.graph_result.as_mut().unwrap().hits[0];
    graph_hit.candidate.resource_ref = Some(rid(10));
    graph_hit.paths[0].resource_path[1] = rid(10);
    graph_hit.paths[0].steps[0].to_resource = rid(10);
    graph_hit.paths[0].steps[0].participants[1].resource_ref = rid(10);
    fixture.revoke_graph_on_evidence.insert(rid(20));
    let result = fixture.service(settings).discover(request).await.unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(result.evidence_set.is_empty());
    assert!(result.qualified_resources.is_empty());
    let printed = format!("{result:?}");
    assert!(!printed.contains("graph-only"));
    assert!(!printed.contains("graph_path:"));
}

#[tokio::test]
async fn request_must_not_silently_drop_need_claims_or_accept_empty_completion() {
    let fixture = Fixture::sufficient();
    let mut missing = request();
    missing.need.completion_requirement.required_claims.clear();
    assert!(fixture.service(config()).discover(missing).await.is_err());
    let mut different = request();
    different.need.required_claims = vec![cid(9)];
    assert!(fixture.service(config()).discover(different).await.is_err());
}
