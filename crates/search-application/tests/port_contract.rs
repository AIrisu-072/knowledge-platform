use search_application::SearchError;
use search_application::ports::{BoxFuture, HyperGraphRetrieverPort, SourceRegistryPort};
use search_application::qualification::QualificationService;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::discovery::{CandidateIdentityClass, FederatedCandidate};
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{DiscoveryEvaluationId, ResourceId, SourceId};
use search_core::relation::RelationNamespace;
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

struct FakeGraph;
impl HyperGraphRetrieverPort for FakeGraph {
    fn retrieve<'a>(
        &'a self,
        _plan: &'a GraphTraversalPlan,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        Box::pin(async move {
            Ok(vec![FederatedCandidate::new(
                "graph-candidate",
                CandidateIdentityClass::DurableResource,
                source_id(),
                "hypergraph",
            )])
        })
    }
}

#[tokio::test]
async fn application_accepts_hypergraph_candidates_without_backend_types() {
    let now = OffsetDateTime::from_unix_timestamp(100).unwrap();
    let plan = GraphTraversalPlan {
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
        },
        stop_conditions: vec![],
    };
    let results = QualificationService::candidates_from_graph(&FakeGraph, &plan)
        .await
        .unwrap();
    assert_eq!(results[0].candidate_id, "graph-candidate");
}

fn _port_error_is_application_error(_: SearchError) {}
