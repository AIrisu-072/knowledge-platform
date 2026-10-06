//! One synthetic Source snapshot through real projection, lexical, graph, and Discovery adapters.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use search_application::discovery_service::{
    DiscoveryConfig, DiscoveryPorts, DiscoveryService, TemporalPolicy,
};
use search_application::materialization::ProbeBudget;
use search_application::ports::{
    AccessDecision, BoxFuture, ClaimSelector, ClaimSelectorPort, CurrentAccessEvaluatorPort,
    CurrentCandidateAccessEvaluatorPort, EvidenceResolverPort, HyperGraphRetrieverPort,
    LexicalQuery, LexicalRetrieverPort, ProjectionGenerationStore, ResolvedAssertionEvidence,
    SemanticRegistrySnapshot, SourceRegistryPort, StructuredFacetFilter,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection, ProjectionCompiler,
    ProjectionInput,
};
use search_application::retrieval::{RetrievalInputs, RetrieverProfile, RetrieverSupport};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::routing::RoutingConstraints;
use search_core::applicability::{ApplicabilityState, Discriminator, DiscriminatorImportance};
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest, FederatedCandidate};
use search_core::evidence::{ClaimState, EvidenceRequirement, EvidenceRole, EvidenceSufficiency};
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{
    ClaimId, DiscoveryEvaluationId, NeedId, ProjectionGenerationId, RelationId, ResourceId,
    SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::observation::Coverage;
use search_core::predicate::{Operand, PredicateExpr, TypedValue};
use search_core::profile::{DiscoveryLens, DiscoveryProfile, FacetState};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use search_core::resource::{DiscoverableResource, ResourceBody, ResourceIdentity, ResourceKind};
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_core::temporal::TemporalEvaluationContext;
use search_graph_memory::MemoryGraphRetriever;
use search_projection_memory::{MemoryProjectionStore, generation_digest};
use search_tantivy::{LexicalBuildInput, LexicalDocument, TantivyLexicalIndex};
use time::OffsetDateTime;
use uuid::Uuid;

const SNAPSHOT: &str = "synthetic-source-snapshot-c10";
const ACCESS_CONTEXT: &str = "synthetic-principal";
const EVIDENCE_REF: &str = "source-primary-11";

fn sid(n: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(n))
}
fn rid(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}
fn cid(n: u128) -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(n))
}
fn at(n: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(n).unwrap()
}

struct SyntheticSource {
    source: DiscoverableSource,
    generation: ProjectionGenerationKey,
    allowed_resources: BTreeSet<ResourceId>,
    deny_target: AtomicBool,
}

impl SyntheticSource {
    fn permitted(&self, resource: ResourceId, context: &str) -> AccessDecision {
        if context == ACCESS_CONTEXT
            && self.allowed_resources.contains(&resource)
            && !(resource == rid(11) && self.deny_target.load(Ordering::SeqCst))
        {
            AccessDecision::Allowed
        } else {
            AccessDecision::Denied
        }
    }
}

impl SourceRegistryPort for SyntheticSource {
    fn get_source<'a>(&'a self, source_id: SourceId) -> BoxFuture<'a, Option<DiscoverableSource>> {
        Box::pin(
            async move { Ok((source_id == self.source.source_id).then(|| self.source.clone())) },
        )
    }

    fn list_sources<'a>(&'a self) -> BoxFuture<'a, Vec<DiscoverableSource>> {
        Box::pin(async move { Ok(vec![self.source.clone()]) })
    }
}

impl CurrentAccessEvaluatorPort for SyntheticSource {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move { Ok(self.permitted(resource_ref, access_context)) })
    }
}

impl CurrentCandidateAccessEvaluatorPort for SyntheticSource {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            Ok(candidate
                .resource_ref
                .filter(|_| candidate.source_ref == self.source.source_id)
                .map_or(AccessDecision::Denied, |id| {
                    self.permitted(id, access_context)
                }))
        })
    }
}

impl ClaimSelectorPort for SyntheticSource {
    fn selector_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async move {
            Ok(
                (generation == self.generation && claim_id == cid(40)).then(|| ClaimSelector {
                    claim_id,
                    subject_ref: "resource-11".into(),
                    predicate: "supports".into(),
                    expected_value: Some(TypedValue::Bool(true)),
                }),
            )
        })
    }
}

impl EvidenceResolverPort for SyntheticSource {
    fn resolve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        Box::pin(async move {
            Ok((generation == self.generation
                && resource_ref == rid(11)
                && evidence_ref == EVIDENCE_REF)
                .then(|| ResolvedAssertionEvidence {
                    generation,
                    source_id: self.source.source_id,
                    resource_id: resource_ref,
                    evidence_ref: evidence_ref.into(),
                    upstream_origin: "synthetic-source-record-11".into(),
                    role: EvidenceRole::Primary,
                    citation_chain: vec![],
                    content_digest: Some("sha256:synthetic-primary-11".into()),
                    is_summary: false,
                }))
        })
    }
}

fn source() -> DiscoverableSource {
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
    ];
    source.provenance = Some("synthetic-source".into());
    source
}

fn manifest() -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: sid(1),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(2)),
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-1".into(),
        analyzer_version: Some("tantivy-default-0.26.2".into()),
        embedding_model_version: None,
        graph_schema_version: Some("typed-nary-v1".into()),
        source_snapshot: SNAPSHOT.into(),
        resource_count: 5,
        relation_count: Some(1),
        coverage: Coverage::CompleteEnumeration,
        digest: String::new(),
        built_at: at(100),
    }
}

fn projection_input(
    source: &DiscoverableSource,
    id: ResourceId,
    name: &str,
    suitable: bool,
    relations: Vec<TypedRelationInstance>,
    assertions: Vec<Assertion>,
) -> ProjectionInput {
    let mut resource = DiscoverableResource::new(
        ResourceIdentity::new(id, ResourceKind::Knowledge, source.source_id),
        ResourceBody::Knowledge,
        DiscoveryProfile::new(name),
    );
    resource.relation_ids = relations
        .iter()
        .map(|relation| relation.relation_id)
        .collect();
    ProjectionInput {
        resource,
        source: source.clone(),
        lens: DiscoveryLens {
            lens_id: "synthetic-knowledge".into(),
            lens_version: 1,
            resource_type: ResourceKind::Knowledge,
            domain_scope: None,
            source_scope: Some(source.source_id),
            identity_fields: vec![],
            high_signal_facets: vec![],
            searchable_fields: vec![],
            applicability_fields: vec!["suitable".into()],
            temporal_fields: vec![],
            relation_fields: vec!["links".into()],
            extraction_policy: None,
            projection_policy: None,
        },
        source_snapshot: SNAPSHOT.into(),
        projection_schema_version: "schema-1".into(),
        semantic_registry_version: "registry-1".into(),
        title: None,
        typed_facets: BTreeMap::from([(
            "suitable".into(),
            FacetState::Known(TypedValue::Bool(suitable)),
        )]),
        assertions,
        authority_resolutions: BTreeMap::new(),
        relations,
    }
}

fn snapshot_inputs(source: &DiscoverableSource) -> Vec<ProjectionInput> {
    let mut relation = TypedRelationInstance::new(
        RelationId::from_uuid(Uuid::from_u128(50)),
        RelationNamespace::Discovery,
        "links",
        vec![
            RelationParticipant::new("seed", rid(20)),
            RelationParticipant::new("result", rid(11)),
            RelationParticipant::new("context", rid(30)),
        ],
    );
    relation.authority = Some("synthetic-authority".into());
    relation.provenance = Some("synthetic-source-relation".into());
    relation.evidence_refs = vec!["source-relation-50".into()];

    let mut assertion = Assertion::new(
        "resource-11",
        "supports",
        TypedValue::Bool(true),
        "synthetic-native-11",
        AssertionOrigin::Authoritative,
        "synthetic-authority",
        at(100),
    );
    assertion.evidence_refs = vec![EVIDENCE_REF.into()];

    vec![
        projection_input(source, rid(10), "needle", false, vec![], vec![]),
        projection_input(
            source,
            rid(11),
            "graph result",
            true,
            vec![],
            vec![assertion],
        ),
        projection_input(source, rid(12), "needle eligible", true, vec![], vec![]),
        projection_input(source, rid(20), "seed", false, vec![relation], vec![]),
        projection_input(source, rid(30), "context", false, vec![], vec![]),
    ]
}

fn request() -> DiscoveryRequest {
    let now = at(100);
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(3)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find a supported resource".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![ResourceKind::Knowledge],
            required_claims: vec![cid(40)],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![cid(40)]),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(4)),
            now,
            now,
            "UTC",
        ),
        access_context: ACCESS_CONTEXT.into(),
    }
}

fn graph_plan(request: &DiscoveryRequest) -> GraphTraversalPlan {
    GraphTraversalPlan {
        seed_nodes: vec![rid(20)],
        path_patterns: vec![
            RelationPathPattern::new(RelationNamespace::Discovery, "links", "seed", "result")
                .with_participant("context", rid(30)),
        ],
        allowed_relation_types: vec!["links".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: Some("synthetic-authority".into()),
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

fn config(plan: GraphTraversalPlan) -> DiscoveryConfig {
    DiscoveryConfig {
        routing: RoutingConstraints {
            required_source_ids: vec![sid(1)],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
        retriever_profile: RetrieverProfile::Exploratory,
        retriever_support: RetrieverSupport {
            lexical: true,
            hypergraph: true,
            ..RetrieverSupport::default()
        },
        retrieval_inputs: RetrievalInputs {
            lexical_query: Some("needle".into()),
            graph_plans: BTreeMap::from([(sid(1), plan)]),
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
        lexical_query: Some(LexicalQuery::new("needle", 10)),
        temporal_policy: TemporalPolicy::default(),
        probe_budget: ProbeBudget {
            max_content_bytes: 0,
            max_latency_ms: 0,
            max_remote_calls: 0,
            max_monetary_cost_minor_units: 0,
            currency: "USD".into(),
        },
        max_actions: 2,
        evaluation_currency: "USD".into(),
    }
}

fn discovery_ports<'a>(
    source_adapter: &'a SyntheticSource,
    projection_store: &'a MemoryProjectionStore,
    lexical: &'a TantivyLexicalIndex,
    graph: &'a MemoryGraphRetriever,
) -> DiscoveryPorts<'a> {
    DiscoveryPorts {
        sources: source_adapter,
        generations: projection_store,
        concepts: projection_store,
        retrieval: RetrievalExecutionPorts {
            remote: None,
            directory: Some(projection_store),
            structured: Some(projection_store),
            lexical: Some(lexical),
            hypergraph: Some(graph),
            graph_resource_access: Some(source_adapter),
            access: source_adapter,
            vector: None,
        },
        selectors: source_adapter,
        assertions: projection_store,
        evidence: source_adapter,
        probe: None,
        probe_catalog: None,
        source_policy: None,
    }
}

#[tokio::test]
async fn source_snapshot_reaches_evidence_sufficient_discovery_through_real_retrievers() {
    let source = source();
    let mut manifest = manifest();
    let inputs = snapshot_inputs(&source);
    let registry = SemanticRegistrySnapshot::new("registry-1");
    let mut projections: Vec<CompiledResourceProjection> = inputs
        .iter()
        .map(|input| ProjectionCompiler::compile_resource(&manifest, input).unwrap())
        .collect();
    manifest.digest = generation_digest(source.source_id, &projections, &registry).unwrap();
    for projection in &mut projections {
        projection.manifest = manifest.clone();
    }
    let generation = manifest.key();

    let projection_store = MemoryProjectionStore::new();
    projection_store
        .begin_generation(
            PersistableGenerationManifest::try_from((manifest.clone(), &source)).unwrap(),
        )
        .await
        .unwrap();
    projection_store
        .stage_concept_registry(generation, registry)
        .await
        .unwrap();
    for projection in &projections {
        projection_store
            .stage_resource(PersistableResourceProjection::try_from(projection.clone()).unwrap())
            .await
            .unwrap();
    }
    projection_store
        .validate_generation(generation)
        .await
        .unwrap();
    projection_store
        .publish_generation(generation)
        .await
        .unwrap();
    assert_eq!(
        projection_store
            .pin_current(source.source_id)
            .await
            .unwrap(),
        Some(manifest.clone())
    );

    let lexical = TantivyLexicalIndex::new();
    let lexical_documents = projections
        .iter()
        .map(|projection| LexicalDocument {
            resource_ref: projection.directory.resource_ref,
            kind: projection.directory.kind,
            canonical_name: projection.directory.canonical_name.clone(),
            title: projection.directory.title.clone(),
            aliases: projection.directory.aliases.clone(),
            high_signal_text: None,
            body: None,
            locator: Some(format!(
                "source://synthetic/{}",
                projection.directory.resource_ref.as_uuid()
            )),
        })
        .collect::<Vec<_>>();
    assert_eq!(lexical_documents.len() as u64, manifest.resource_count);
    lexical
        .build_generation(
            manifest.clone(),
            &source,
            LexicalBuildInput::new(
                source.source_id,
                SNAPSHOT,
                &manifest.projection_schema_version,
                manifest.lens_version,
                lexical_documents,
            ),
        )
        .unwrap();

    let source_adapter = Arc::new(SyntheticSource {
        source: source.clone(),
        generation,
        allowed_resources: [rid(10), rid(11), rid(12), rid(20), rid(30)].into(),
        deny_target: AtomicBool::new(false),
    });
    let graph = MemoryGraphRetriever::new(source_adapter.clone());
    graph
        .build_generation(manifest.clone(), &source, projections)
        .unwrap();

    let request = request();
    let plan = graph_plan(&request);
    let lexical_hits = lexical
        .retrieve(generation, &request, &LexicalQuery::new("needle", 10))
        .await
        .unwrap();
    assert_eq!(
        lexical_hits
            .iter()
            .map(|hit| hit.resource_ref)
            .collect::<Vec<_>>(),
        vec![Some(rid(10)), Some(rid(12))]
    );
    assert!(
        !lexical_hits
            .iter()
            .any(|hit| hit.resource_ref == Some(rid(11)))
    );
    let graph_hits = graph.retrieve(generation, &plan).await.unwrap();
    assert_eq!(graph_hits.generation, generation);
    assert_eq!(graph_hits.hits.len(), 1);
    assert_eq!(graph_hits.hits[0].candidate.resource_ref, Some(rid(11)));
    assert_eq!(graph_hits.hits[0].paths[0].steps[0].participants.len(), 3);

    let mut lexical_only_config = config(plan.clone());
    lexical_only_config.retriever_support.hypergraph = false;
    lexical_only_config.retrieval_inputs.graph_plans.clear();
    let lexical_only = DiscoveryService::new(
        lexical_only_config,
        discovery_ports(source_adapter.as_ref(), &projection_store, &lexical, &graph),
    )
    .unwrap()
    .discover(request.clone())
    .await
    .unwrap();
    assert_eq!(
        lexical_only.evidence_sufficiency,
        EvidenceSufficiency::Unresolved
    );
    assert_eq!(
        lexical_only
            .qualified_resources
            .iter()
            .map(|resource| resource.resource_ref)
            .collect::<Vec<_>>(),
        vec![rid(12)]
    );
    assert!(
        lexical_only
            .evidence_set
            .iter()
            .all(|claim| claim.state != ClaimState::Supported)
    );

    let service = DiscoveryService::new(
        config(plan),
        discovery_ports(source_adapter.as_ref(), &projection_store, &lexical, &graph),
    )
    .unwrap();
    let result = service.discover(request.clone()).await.unwrap();
    assert_eq!(
        result.discovery_evaluation_id,
        request.temporal_context.evaluation_id
    );
    assert_eq!(
        result
            .qualified_resources
            .iter()
            .map(|resource| resource.resource_ref)
            .collect::<Vec<_>>(),
        vec![rid(12), rid(11)]
    );
    assert_eq!(
        result.qualified_resources[1].applicability,
        ApplicabilityState::Applicable
    );
    assert_eq!(
        result.qualified_resources[1].matched_conditions,
        vec!["suitable"]
    );
    assert_eq!(
        result.qualified_resources[1].evidence_refs,
        vec![EVIDENCE_REF]
    );
    assert_eq!(
        result.qualified_resources[0].evidence_refs,
        Vec::<String>::new()
    );
    let supported_claim = result
        .evidence_set
        .iter()
        .find(|claim| claim.claim_id == cid(40) && claim.state == ClaimState::Supported)
        .unwrap();
    assert_eq!(supported_claim.evidence_refs.len(), 1);
    assert_eq!(supported_claim.evidence_refs[0].role, EvidenceRole::Primary);
    assert_eq!(
        supported_claim.evidence_refs[0].evidence_ref.as_deref(),
        Some(EVIDENCE_REF)
    );
    assert!(
        result
            .source_trace
            .iter()
            .any(|trace| trace.contains("Planned"))
    );
    let lexical_trace = format!("retriever:{}:Lexical", source.source_id.as_uuid());
    let graph_trace = format!("retriever:{}:HyperGraph", source.source_id.as_uuid());
    assert!(result.retrieval_trace.contains(&lexical_trace));
    assert!(result.retrieval_trace.contains(&graph_trace));
    let lexical_position = result
        .retrieval_trace
        .iter()
        .position(|trace| trace == &lexical_trace)
        .unwrap();
    let graph_position = result
        .retrieval_trace
        .iter()
        .position(|trace| trace == &graph_trace)
        .unwrap();
    assert!(lexical_position < graph_position);
    assert!(
        result
            .retrieval_trace
            .iter()
            .any(|trace| trace.starts_with("graph_path:") && trace.contains("source-relation-50"))
    );
    assert!(result.rejected_candidates.iter().any(|candidate| {
        candidate.candidate_id == format!("{}:{}", source.source_id.as_uuid(), rid(10).as_uuid())
            && candidate.state == ApplicabilityState::Excluded
    }));

    // A current Source denial must also hide the graph path and its Claim on a later evaluation.
    source_adapter.deny_target.store(true, Ordering::SeqCst);
    let denied = service.discover(request).await.unwrap();
    assert_eq!(denied.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert_eq!(
        denied
            .qualified_resources
            .iter()
            .map(|resource| resource.resource_ref)
            .collect::<Vec<_>>(),
        vec![rid(12)]
    );
    assert!(
        denied
            .evidence_set
            .iter()
            .all(|claim| claim.state != ClaimState::Supported && claim.evidence_refs.is_empty())
    );
    let denied_candidate_id = format!("{}:{}", source.source_id.as_uuid(), rid(11).as_uuid());
    assert!(denied.rejected_candidates.iter().all(|candidate| {
        candidate.candidate_id != denied_candidate_id
            && candidate.reason_trace.iter().all(|reason| {
                !reason.contains(&denied_candidate_id) && !reason.contains(EVIDENCE_REF)
            })
    }));
    assert!(
        denied.qualification_trace.iter().all(|trace| {
            !trace.contains(&denied_candidate_id) && !trace.contains(EVIDENCE_REF)
        })
    );
    assert!(
        denied.retrieval_trace.iter().all(|trace| {
            !trace.contains(&denied_candidate_id) && !trace.contains(EVIDENCE_REF)
        })
    );
    let denied_dump = format!("{denied:?}");
    assert!(!denied_dump.contains(&rid(11).as_uuid().to_string()));
    assert!(!denied_dump.contains(EVIDENCE_REF));
    assert!(
        !denied
            .retrieval_trace
            .iter()
            .any(|trace| trace.starts_with("graph_path:"))
    );
    assert_eq!(
        result.evidence_sufficiency,
        EvidenceSufficiency::Sufficient,
        "claim states after Graph: {:?}",
        result
            .evidence_set
            .iter()
            .map(|claim| claim.state)
            .collect::<Vec<_>>()
    );
    assert!(result.unresolved_gaps.iter().all(|gap| !gap.blocking));
}
