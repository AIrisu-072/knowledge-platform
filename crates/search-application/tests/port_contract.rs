use search_application::SearchError;
use search_application::ports::{
    BoxFuture, GraphRetrievalHit, GraphRetrievalResult, HyperGraphRetrieverPort, SourceRegistryPort,
};
use search_application::qualification::QualificationService;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::discovery::{CandidateIdentityClass, FederatedCandidate};
use search_core::graph::{
    GraphPathEvidence, GraphPathStepEvidence, GraphTraversalPlan, RelationPathPattern,
    TraversalBudget,
};
use search_core::id::{
    DiscoveryEvaluationId, ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
use search_core::projection::ProjectionGenerationKey;
use search_core::relation::{RelationNamespace, RelationParticipant};
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_core::temporal::TemporalEvaluationContext;
use time::OffsetDateTime;
use uuid::Uuid;

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(1))
}

#[tokio::test]
async fn registry_returns_capabilities_without_source_body() {
    let mut registry = InMemorySourceRegistry::default();
    let mut source = DiscoverableSource::new(
        source_id(),
        "remote-policy",
        EnumerationSemantics::QueryOnly,
        RetentionMode::NoRetention,
    );
    source.discovery_modes.push(DiscoveryMode::RemoteQuery);
    registry.insert(source);
    let resolved = registry.get_source(source_id()).await.unwrap().unwrap();
    assert!(resolved.supports(DiscoveryMode::RemoteQuery));
    assert_eq!(resolved.retention_mode, RetentionMode::NoRetention);
}

struct FakeGraph(GraphRetrievalResult);
impl HyperGraphRetrieverPort for FakeGraph {
    fn retrieve<'a>(
        &'a self,
        _generation: ProjectionGenerationKey,
        _plan: &'a GraphTraversalPlan,
    ) -> BoxFuture<'a, GraphRetrievalResult> {
        let result = self.0.clone();
        Box::pin(async move { Ok(result) })
    }
}

fn graph_plan() -> GraphTraversalPlan {
    let now = OffsetDateTime::from_unix_timestamp(100).unwrap();
    GraphTraversalPlan {
        seed_nodes: vec![ResourceId::from_uuid(Uuid::from_u128(2))],
        path_patterns: vec![RelationPathPattern::new(
            RelationNamespace::Discovery,
            "loan",
            "borrower",
            "product",
        )],
        allowed_relation_types: vec!["loan".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: None,
        temporal_context: Some(TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(3)),
            now,
            now,
            "Asia/Tokyo",
        )),
        access_context: "principal-1".into(),
        expansion_budget: TraversalBudget {
            max_hops: 1,
            max_relations: 10,
            max_branching_per_node: 3,
            max_seed_nodes: 1,
            max_paths: 3,
        },
        stop_conditions: vec![],
    }
}

fn generation_key() -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(4)),
    }
}

fn graph_result(generation: ProjectionGenerationKey) -> GraphRetrievalResult {
    let seed = ResourceId::from_uuid(Uuid::from_u128(2));
    let target = ResourceId::from_uuid(Uuid::from_u128(5));
    let participants = vec![
        RelationParticipant::new("borrower", seed),
        RelationParticipant::new("product", target),
    ];
    let mut candidate = FederatedCandidate::new(
        "graph-candidate",
        CandidateIdentityClass::DurableResource,
        generation.source_id,
        "hypergraph",
    );
    candidate.resource_ref = Some(target);
    GraphRetrievalResult {
        generation,
        hits: vec![GraphRetrievalHit {
            candidate,
            paths: vec![GraphPathEvidence {
                resource_path: vec![seed, target],
                steps: vec![GraphPathStepEvidence {
                    relation_id: RelationId::from_uuid(Uuid::from_u128(6)),
                    namespace: RelationNamespace::Discovery,
                    relation_type: "loan".into(),
                    from_role: "borrower".into(),
                    from_resource: seed,
                    to_role: "product".into(),
                    to_resource: target,
                    participants,
                    evidence_refs: vec!["evidence-1".into()],
                    provenance: None,
                }],
            }],
        }],
    }
}

#[tokio::test]
async fn application_accepts_hypergraph_candidates_without_backend_types() {
    let generation = generation_key();
    let results = QualificationService::candidates_from_graph(
        &FakeGraph(graph_result(generation)),
        generation,
        &graph_plan(),
    )
    .await
    .unwrap();
    assert_eq!(results.generation, generation);
    assert_eq!(results.hits[0].candidate.candidate_id, "graph-candidate");
}

#[tokio::test]
async fn application_rejects_mismatched_graph_generation() {
    let generation = generation_key();
    let mut result = graph_result(generation);
    result.generation.generation_id = ProjectionGenerationId::from_uuid(Uuid::from_u128(7));
    assert!(matches!(
        QualificationService::candidates_from_graph(&FakeGraph(result), generation, &graph_plan())
            .await,
        Err(SearchError::OperationFailed(_))
    ));
}

#[tokio::test]
async fn application_rejects_graph_hit_from_another_source() {
    let generation = generation_key();
    let mut result = graph_result(generation);
    result.hits[0].candidate.source_ref = SourceId::from_uuid(Uuid::from_u128(8));
    assert!(matches!(
        QualificationService::candidates_from_graph(&FakeGraph(result), generation, &graph_plan())
            .await,
        Err(SearchError::OperationFailed(_))
    ));
}

#[tokio::test]
async fn application_rejects_graph_hit_without_path_evidence() {
    let generation = generation_key();
    let mut result = graph_result(generation);
    result.hits[0].paths.clear();
    assert!(matches!(
        QualificationService::candidates_from_graph(&FakeGraph(result), generation, &graph_plan())
            .await,
        Err(SearchError::OperationFailed(_))
    ));
}

#[tokio::test]
async fn application_rejects_empty_graph_path() {
    let generation = generation_key();
    let mut result = graph_result(generation);
    result.hits[0].paths[0].steps.clear();
    assert!(matches!(
        QualificationService::candidates_from_graph(&FakeGraph(result), generation, &graph_plan())
            .await,
        Err(SearchError::OperationFailed(_))
    ));
}

fn _port_error_is_application_error(_: SearchError) {}
