use std::collections::BTreeMap;
use std::sync::Mutex;

use search_application::SearchError;
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
    DirectoryRetrieverPort, GraphRetrievalHit, GraphRetrievalResult, HyperGraphRetrieverPort,
    LexicalQuery, LexicalRetrieverPort, StructuredFacetFilter, StructuredFacetOutcome,
    StructuredRetrievalHit, StructuredRetrieverPort,
};
use search_application::retrieval::{
    ActionState, RetrievalAction, RetrieverCursorState, RetrieverKind,
};
use search_application::retrieval_execution::{
    RetrievalExecutionInput, RetrievalExecutionPorts, RetrievalExecutionResult, RetrievalExecutor,
};
use search_application::routing::RouteStage;
use search_core::discovery::{
    CandidateIdentityClass, DiscoveryNeed, DiscoveryRequest, FederatedCandidate,
};
use search_core::evidence::EvidenceRequirement;
use search_core::graph::{
    GraphPathEvidence, GraphPathStepEvidence, GraphTraversalPlan, RelationPathPattern,
    TraversalBudget,
};
use search_core::id::{
    DiscoveryEvaluationId, NeedId, ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::predicate::TypedValue;
use search_core::projection::ProjectionGenerationKey;
use search_core::relation::{RelationNamespace, RelationParticipant};
use search_core::temporal::TemporalEvaluationContext;
use time::OffsetDateTime;
use uuid::Uuid;

fn source(value: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(value))
}

fn resource(value: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(value))
}

fn generation() -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: source(1),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(2)),
    }
}

fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::from_unix_timestamp(100).unwrap();
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(3)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find evidence".into(),
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
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(4)),
            now,
            now,
            "Asia/Tokyo",
        ),
        access_context: "principal-1".into(),
    }
}

fn action(kind: RetrieverKind) -> RetrievalAction {
    RetrievalAction {
        source_id: source(1),
        retriever_id: format!("source-1:{kind:?}"),
        retriever: kind,
        stage: RouteStage::Initial,
        state: ActionState::Planned,
        cursor: RetrieverCursorState::initial(),
    }
}

fn candidate(id: &str) -> FederatedCandidate {
    let mut hit = FederatedCandidate::new(
        id,
        CandidateIdentityClass::DurableResource,
        source(1),
        "retriever",
    );
    hit.resource_ref = Some(resource(10));
    hit.locator = Some(format!("private://{id}"));
    hit.retrieval_trace_ref = Some(format!("trace-{id}"));
    hit
}

fn graph_candidate(id: &str, endpoint: u128) -> FederatedCandidate {
    let mut hit = candidate(id);
    hit.resource_ref = Some(resource(endpoint));
    hit
}

fn graph_plan() -> GraphTraversalPlan {
    GraphTraversalPlan {
        seed_nodes: vec![resource(10)],
        path_patterns: vec![RelationPathPattern::new(
            RelationNamespace::Discovery,
            "loan",
            "borrower",
            "product",
        )],
        allowed_relation_types: vec!["loan".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: None,
        temporal_context: Some(request().temporal_context),
        access_context: "principal-1".into(),
        expansion_budget: TraversalBudget {
            max_hops: 1,
            max_relations: 2,
            max_branching_per_node: 2,
            max_seed_nodes: 1,
            max_paths: 2,
        },
        stop_conditions: vec![],
    }
}

fn graph_path() -> GraphPathEvidence {
    let participants = vec![
        RelationParticipant::new("borrower", resource(10)),
        RelationParticipant::new("product", resource(11)),
    ];
    GraphPathEvidence {
        resource_path: vec![resource(10), resource(11)],
        steps: vec![GraphPathStepEvidence {
            relation_id: RelationId::from_uuid(Uuid::from_u128(12)),
            namespace: RelationNamespace::Discovery,
            relation_type: "loan".into(),
            from_role: "borrower".into(),
            from_resource: resource(10),
            to_role: "product".into(),
            to_resource: resource(11),
            participants,
            evidence_refs: vec!["relation-evidence".into()],
            provenance: Some("source-observation".into()),
        }],
    }
}

fn graph_retrievers(hits: Vec<GraphRetrievalHit>) -> Retrievers {
    Retrievers {
        graph: Some(GraphRetrievalResult {
            generation: generation(),
            hits,
        }),
        ..Default::default()
    }
}

fn assert_invalid_graph_path(result: Result<RetrievalExecutionResult, SearchError>, case: &str) {
    match result {
        Err(SearchError::OperationFailed(message)) => {
            assert_eq!(
                message, "graph retriever returned invalid path evidence",
                "{case}"
            );
        }
        other => panic!("{case}: expected generic invalid-path failure, got {other:?}"),
    }
}

async fn assert_graph_hits_invalid(
    case: &str,
    plan: &GraphTraversalPlan,
    hits: Vec<GraphRetrievalHit>,
) {
    let retrievers = graph_retrievers(hits);
    let access = CurrentAccess::allowed();
    let action = action(RetrieverKind::HyperGraph);
    let request = request();
    let mut args = input(&action, &request);
    args.graph_plan = Some(plan);
    assert_invalid_graph_path(
        RetrievalExecutor::execute(&ports(&retrievers, &access), args).await,
        case,
    );
}

struct AllowGraphResources;

impl CurrentAccessEvaluatorPort for AllowGraphResources {
    fn evaluate<'a>(
        &'a self,
        _resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        assert_eq!(access_context, "principal-1");
        Box::pin(async { Ok(AccessDecision::Allowed) })
    }
}

static ALLOW_GRAPH_RESOURCES: AllowGraphResources = AllowGraphResources;

struct GraphResourceAccess {
    decisions: BTreeMap<ResourceId, AccessResult>,
    checked: Mutex<Vec<ResourceId>>,
}

impl CurrentAccessEvaluatorPort for GraphResourceAccess {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        assert_eq!(access_context, "principal-1");
        self.checked.lock().unwrap().push(resource_ref);
        let decision = self
            .decisions
            .get(&resource_ref)
            .copied()
            .unwrap_or(AccessResult::Decision(AccessDecision::Allowed));
        Box::pin(async move {
            match decision {
                AccessResult::Decision(value) => Ok(value),
                AccessResult::Error => Err(SearchError::SourceUnavailable("secret detail".into())),
            }
        })
    }
}

#[derive(Default)]
struct Retrievers {
    directory: Vec<FederatedCandidate>,
    structured: Vec<StructuredRetrievalHit>,
    lexical: Vec<FederatedCandidate>,
    graph: Option<GraphRetrievalResult>,
    calls: Mutex<Vec<&'static str>>,
}

impl DirectoryRetrieverPort for Retrievers {
    fn retrieve<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        assert_eq!(key, generation());
        assert_eq!(request.access_context, "principal-1");
        self.calls.lock().unwrap().push("directory");
        Box::pin(async move { Ok(self.directory.clone()) })
    }
}

impl StructuredRetrieverPort for Retrievers {
    fn retrieve<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        hard_filters: &'a [StructuredFacetFilter],
    ) -> BoxFuture<'a, Vec<StructuredRetrievalHit>> {
        assert_eq!(key, generation());
        assert_eq!(request.access_context, "principal-1");
        assert_eq!(hard_filters.len(), 2);
        self.calls.lock().unwrap().push("structured");
        Box::pin(async move { Ok(self.structured.clone()) })
    }
}

impl LexicalRetrieverPort for Retrievers {
    fn retrieve<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        assert_eq!(key, generation());
        assert_eq!(request.access_context, "principal-1");
        assert_eq!(query, &LexicalQuery::new("evidence", 3));
        self.calls.lock().unwrap().push("lexical");
        Box::pin(async move { Ok(self.lexical.clone()) })
    }
}

impl HyperGraphRetrieverPort for Retrievers {
    fn retrieve<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        plan: &'a GraphTraversalPlan,
    ) -> BoxFuture<'a, GraphRetrievalResult> {
        assert_eq!(key, generation());
        assert_eq!(plan.access_context, "principal-1");
        self.calls.lock().unwrap().push("graph");
        let result = self.graph.clone().unwrap();
        Box::pin(async move { Ok(result) })
    }
}

#[derive(Clone, Copy)]
enum AccessResult {
    Decision(AccessDecision),
    Error,
}

struct CurrentAccess {
    decisions: BTreeMap<String, AccessResult>,
    checked: Mutex<Vec<String>>,
}

impl CurrentAccess {
    fn allowed() -> Self {
        Self {
            decisions: BTreeMap::new(),
            checked: Mutex::new(vec![]),
        }
    }
}

impl CurrentCandidateAccessEvaluatorPort for CurrentAccess {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        assert_eq!(access_context, "principal-1");
        self.checked
            .lock()
            .unwrap()
            .push(candidate.candidate_id.clone());
        let decision = self
            .decisions
            .get(&candidate.candidate_id)
            .copied()
            .unwrap_or(AccessResult::Decision(AccessDecision::Allowed));
        Box::pin(async move {
            match decision {
                AccessResult::Decision(value) => Ok(value),
                AccessResult::Error => Err(SearchError::SourceUnavailable("secret detail".into())),
            }
        })
    }
}

fn ports<'a>(retrievers: &'a Retrievers, access: &'a CurrentAccess) -> RetrievalExecutionPorts<'a> {
    RetrievalExecutionPorts {
        directory: Some(retrievers),
        structured: Some(retrievers),
        lexical: Some(retrievers),
        hypergraph: Some(retrievers),
        graph_resource_access: Some(&ALLOW_GRAPH_RESOURCES),
        access,
    }
}

fn input<'a>(
    action: &'a RetrievalAction,
    request: &'a DiscoveryRequest,
) -> RetrievalExecutionInput<'a> {
    RetrievalExecutionInput {
        action,
        generation: generation(),
        request,
        structured_filters: &[],
        lexical_query: None,
        graph_plan: None,
        body_query: None,
    }
}

#[tokio::test]
async fn directory_uses_one_port_and_hides_all_non_allowed_hits_before_returning_trace() {
    let retrievers = Retrievers {
        directory: vec![
            candidate("denied"),
            candidate("unknown"),
            candidate("error"),
            candidate("allowed"),
        ],
        ..Default::default()
    };
    let access = CurrentAccess {
        decisions: BTreeMap::from([
            (
                "denied".into(),
                AccessResult::Decision(AccessDecision::Denied),
            ),
            (
                "unknown".into(),
                AccessResult::Decision(AccessDecision::Unknown),
            ),
            ("error".into(), AccessResult::Error),
        ]),
        checked: Mutex::new(vec![]),
    };
    let action = action(RetrieverKind::Directory);
    let request = request();
    let result = RetrievalExecutor::execute(&ports(&retrievers, &access), input(&action, &request))
        .await
        .unwrap();
    assert_eq!(*retrievers.calls.lock().unwrap(), ["directory"]);
    assert_eq!(access.checked.lock().unwrap().len(), 4);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].candidate, candidate("allowed"));
    assert_eq!(result.hits[0].retriever_id, action.retriever_id);
    assert_eq!(result.hits[0].generation, generation());
    assert_eq!(result.hits[0].rank, 1);
    assert!(result.hits[0].structured_outcomes.is_none());
    assert!(result.hits[0].graph_paths.is_none());
    let output = format!("{result:?}");
    for private in ["denied", "unknown", "error", "secret detail"] {
        assert!(!output.contains(private), "hidden hit leaked {private}");
    }
}

#[tokio::test]
async fn structured_preserves_ordered_outcomes_and_rejects_wrong_count() {
    let filters = [
        StructuredFacetFilter::eq("type", TypedValue::String("policy".into())),
        StructuredFacetFilter::eq("status", TypedValue::String("active".into())),
    ];
    let retrievers = Retrievers {
        structured: vec![StructuredRetrievalHit {
            candidate: candidate("structured"),
            outcomes: vec![
                StructuredFacetOutcome::Match,
                StructuredFacetOutcome::Unknown,
            ],
        }],
        ..Default::default()
    };
    let access = CurrentAccess::allowed();
    let action = action(RetrieverKind::Structured);
    let request = request();
    let mut args = input(&action, &request);
    args.structured_filters = &filters;
    let result = RetrievalExecutor::execute(&ports(&retrievers, &access), args)
        .await
        .unwrap();
    assert_eq!(*retrievers.calls.lock().unwrap(), ["structured"]);
    assert_eq!(
        result.hits[0].structured_outcomes,
        Some(vec![
            StructuredFacetOutcome::Match,
            StructuredFacetOutcome::Unknown,
        ])
    );
    assert_eq!(result.hits[0].rank, 1);

    let malformed = Retrievers {
        structured: vec![StructuredRetrievalHit {
            candidate: candidate("structured"),
            outcomes: vec![StructuredFacetOutcome::Match],
        }],
        ..Default::default()
    };
    let mut args = input(&action, &request);
    args.structured_filters = &filters;
    assert!(matches!(
        RetrievalExecutor::execute(&ports(&malformed, &access), args).await,
        Err(SearchError::OperationFailed(_))
    ));
}

#[tokio::test]
async fn lexical_uses_explicit_query_and_keeps_retriever_rank() {
    let retrievers = Retrievers {
        lexical: vec![candidate("first"), candidate("second")],
        ..Default::default()
    };
    let access = CurrentAccess::allowed();
    let action = action(RetrieverKind::Lexical);
    let request = request();
    let query = LexicalQuery::new("evidence", 3);
    let mut args = input(&action, &request);
    args.lexical_query = Some(&query);
    let result = RetrievalExecutor::execute(&ports(&retrievers, &access), args)
        .await
        .unwrap();
    assert_eq!(*retrievers.calls.lock().unwrap(), ["lexical"]);
    assert_eq!(
        result.hits.iter().map(|hit| hit.rank).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(result.hits[1].candidate.candidate_id, "second");
}

#[tokio::test]
async fn graph_keeps_typed_path_and_rejects_invalid_generation_or_path() {
    let path = graph_path();
    let graph = GraphRetrievalResult {
        generation: generation(),
        hits: vec![GraphRetrievalHit {
            candidate: graph_candidate("graph", 11),
            paths: vec![path.clone()],
        }],
    };
    let retrievers = Retrievers {
        graph: Some(graph.clone()),
        ..Default::default()
    };
    let access = CurrentAccess::allowed();
    let action = action(RetrieverKind::HyperGraph);
    let request = request();
    let plan = graph_plan();
    let mut args = input(&action, &request);
    args.graph_plan = Some(&plan);
    let result = RetrievalExecutor::execute(&ports(&retrievers, &access), args)
        .await
        .unwrap();
    assert_eq!(*retrievers.calls.lock().unwrap(), ["graph"]);
    assert_eq!(result.hits[0].graph_paths, Some(vec![path]));

    let mut wrong_generation = graph.clone();
    wrong_generation.generation.generation_id =
        ProjectionGenerationId::from_uuid(Uuid::from_u128(99));
    let malformed = Retrievers {
        graph: Some(wrong_generation),
        ..Default::default()
    };
    let mut args = input(&action, &request);
    args.graph_plan = Some(&plan);
    assert!(matches!(
        RetrievalExecutor::execute(&ports(&malformed, &access), args).await,
        Err(SearchError::OperationFailed(_))
    ));

    let mut wrong_source = graph.clone();
    wrong_source.hits[0].candidate.source_ref = source(99);
    let malformed = Retrievers {
        graph: Some(wrong_source),
        ..Default::default()
    };
    let mut args = input(&action, &request);
    args.graph_plan = Some(&plan);
    assert!(matches!(
        RetrievalExecutor::execute(&ports(&malformed, &access), args).await,
        Err(SearchError::OperationFailed(_))
    ));

    let mut empty_step = graph.clone();
    empty_step.hits[0].paths[0].steps.clear();
    let malformed = Retrievers {
        graph: Some(empty_step),
        ..Default::default()
    };
    let mut args = input(&action, &request);
    args.graph_plan = Some(&plan);
    assert!(matches!(
        RetrievalExecutor::execute(&ports(&malformed, &access), args).await,
        Err(SearchError::OperationFailed(_))
    ));

    let mut no_path = graph;
    no_path.hits[0].paths.clear();
    let malformed = Retrievers {
        graph: Some(no_path),
        ..Default::default()
    };
    let mut args = input(&action, &request);
    args.graph_plan = Some(&plan);
    assert!(matches!(
        RetrievalExecutor::execute(&ports(&malformed, &access), args).await,
        Err(SearchError::OperationFailed(_))
    ));
}

async fn assert_hidden_graph_hit_matches_no_hit(
    hit: GraphRetrievalHit,
    returned_generation: ProjectionGenerationKey,
) {
    let action = action(RetrieverKind::HyperGraph);
    let request = request();
    let plan = graph_plan();
    let empty = graph_retrievers(vec![]);
    let allowed = CurrentAccess::allowed();
    let mut args = input(&action, &request);
    args.graph_plan = Some(&plan);
    let no_hit = RetrievalExecutor::execute(&ports(&empty, &allowed), args)
        .await
        .unwrap();

    let mut hidden = graph_retrievers(vec![hit]);
    hidden.graph.as_mut().unwrap().generation = returned_generation;
    let access = CurrentAccess {
        decisions: BTreeMap::from([(
            "hidden".into(),
            AccessResult::Decision(AccessDecision::Denied),
        )]),
        checked: Mutex::new(vec![]),
    };
    let mut args = input(&action, &request);
    args.graph_plan = Some(&plan);
    let hidden_result = RetrievalExecutor::execute(&ports(&hidden, &access), args)
        .await
        .unwrap();
    assert_eq!(hidden_result, no_hit);
}

#[tokio::test]
async fn graph_hidden_hit_from_another_source_has_no_existence_signal() {
    let mut candidate = graph_candidate("hidden", 11);
    candidate.source_ref = source(99);
    assert_hidden_graph_hit_matches_no_hit(
        GraphRetrievalHit {
            candidate,
            paths: vec![graph_path()],
        },
        generation(),
    )
    .await;
}

#[tokio::test]
async fn graph_hidden_hit_without_path_has_no_existence_signal() {
    assert_hidden_graph_hit_matches_no_hit(
        GraphRetrievalHit {
            candidate: graph_candidate("hidden", 11),
            paths: vec![],
        },
        generation(),
    )
    .await;
}

#[tokio::test]
async fn graph_hidden_hit_from_another_generation_has_no_existence_signal() {
    let mut wrong_generation = generation();
    wrong_generation.generation_id = ProjectionGenerationId::from_uuid(Uuid::from_u128(99));
    assert_hidden_graph_hit_matches_no_hit(
        GraphRetrievalHit {
            candidate: graph_candidate("hidden", 11),
            paths: vec![graph_path()],
        },
        wrong_generation,
    )
    .await;
}

#[tokio::test]
async fn graph_hides_entire_hit_if_an_intermediate_participant_loses_current_access() {
    let mut path = graph_path();
    path.steps[0]
        .participants
        .push(RelationParticipant::new("approver", resource(13)));
    let retrievers = Retrievers {
        graph: Some(GraphRetrievalResult {
            generation: generation(),
            hits: vec![GraphRetrievalHit {
                candidate: graph_candidate("graph-private-relation", 11),
                paths: vec![path],
            }],
        }),
        ..Default::default()
    };
    let access = CurrentAccess::allowed();
    let action = action(RetrieverKind::HyperGraph);
    let request = request();
    let plan = graph_plan();
    for decision in [
        AccessResult::Decision(AccessDecision::Denied),
        AccessResult::Decision(AccessDecision::Unknown),
        AccessResult::Error,
    ] {
        let resource_access = GraphResourceAccess {
            decisions: BTreeMap::from([(resource(13), decision)]),
            checked: Mutex::new(vec![]),
        };
        let mut graph_ports = ports(&retrievers, &access);
        graph_ports.graph_resource_access = Some(&resource_access);
        let mut args = input(&action, &request);
        args.graph_plan = Some(&plan);
        let result = RetrievalExecutor::execute(&graph_ports, args)
            .await
            .unwrap();
        assert!(result.hits.is_empty());
        assert!(
            resource_access
                .checked
                .lock()
                .unwrap()
                .contains(&resource(13))
        );
    }
}

#[tokio::test]
async fn graph_path_access_drops_do_not_leave_visible_rank_gaps() {
    let mut hidden_path = graph_path();
    hidden_path.steps[0]
        .participants
        .push(RelationParticipant::new("approver", resource(13)));
    let retrievers = graph_retrievers(vec![
        GraphRetrievalHit {
            candidate: graph_candidate("hidden", 11),
            paths: vec![hidden_path],
        },
        GraphRetrievalHit {
            candidate: graph_candidate("visible", 11),
            paths: vec![graph_path()],
        },
    ]);
    let candidate_access = CurrentAccess::allowed();
    let action = action(RetrieverKind::HyperGraph);
    let request = request();
    let plan = graph_plan();
    for decision in [
        AccessResult::Decision(AccessDecision::Denied),
        AccessResult::Decision(AccessDecision::Unknown),
        AccessResult::Error,
    ] {
        let resource_access = GraphResourceAccess {
            decisions: BTreeMap::from([(resource(13), decision)]),
            checked: Mutex::new(vec![]),
        };
        let mut graph_ports = ports(&retrievers, &candidate_access);
        graph_ports.graph_resource_access = Some(&resource_access);
        let mut args = input(&action, &request);
        args.graph_plan = Some(&plan);
        let result = RetrievalExecutor::execute(&graph_ports, args)
            .await
            .unwrap();
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].candidate.candidate_id, "visible");
        assert_eq!(result.hits[0].rank, 1);
        assert!(!format!("{result:?}").contains("hidden"));
    }
}

#[tokio::test]
async fn graph_rejects_candidate_resource_that_is_not_the_path_endpoint() {
    let action = action(RetrieverKind::HyperGraph);
    let request = request();
    let plan = graph_plan();
    for (case, candidate) in [
        ("wrong endpoint", graph_candidate("private-candidate", 99)),
        ("missing endpoint", {
            let mut candidate = graph_candidate("private-candidate", 11);
            candidate.resource_ref = None;
            candidate
        }),
    ] {
        let retrievers = graph_retrievers(vec![GraphRetrievalHit {
            candidate,
            paths: vec![graph_path()],
        }]);
        let access = CurrentAccess::allowed();
        let mut args = input(&action, &request);
        args.graph_plan = Some(&plan);
        assert_invalid_graph_path(
            RetrievalExecutor::execute(&ports(&retrievers, &access), args).await,
            case,
        );
    }
}

#[tokio::test]
async fn graph_rejects_relation_outside_the_planned_type() {
    let mut path = graph_path();
    path.steps[0].relation_type = "deposit".into();
    let retrievers = graph_retrievers(vec![GraphRetrievalHit {
        candidate: graph_candidate("private-relation", 11),
        paths: vec![path],
    }]);
    let access = CurrentAccess::allowed();
    let action = action(RetrieverKind::HyperGraph);
    let request = request();
    let plan = graph_plan();
    let mut args = input(&action, &request);
    args.graph_plan = Some(&plan);
    assert_invalid_graph_path(
        RetrievalExecutor::execute(&ports(&retrievers, &access), args).await,
        "unplanned relation type",
    );
}

#[tokio::test]
async fn graph_rejects_path_shape_and_relation_participants_outside_the_plan() {
    let original = graph_path();
    let base_plan = graph_plan();
    let mut cases = Vec::new();

    let mut path = original.clone();
    path.resource_path[0] = resource(99);
    cases.push(("unplanned seed", base_plan.clone(), path));

    let mut path = original.clone();
    path.resource_path.push(resource(99));
    cases.push(("resource and step counts differ", base_plan.clone(), path));

    let mut path = original.clone();
    path.steps[0].from_resource = resource(99);
    cases.push(("disconnected from endpoint", base_plan.clone(), path));

    let mut path = original.clone();
    path.steps[0].participants.pop();
    cases.push(("missing to participant", base_plan.clone(), path));

    let mut path = original.clone();
    path.steps[0].participants[1].role = "seller".into();
    cases.push(("wrong participant role", base_plan.clone(), path));

    let mut path = original.clone();
    path.steps[0].namespace = RelationNamespace::Evidence;
    cases.push(("unplanned namespace", base_plan.clone(), path));

    let mut path = original.clone();
    path.steps[0].from_role = "lender".into();
    cases.push(("unplanned from role", base_plan.clone(), path));

    let mut plan = base_plan.clone();
    plan.path_patterns[0]
        .required_participants
        .push(RelationParticipant::new("approver", resource(13)));
    cases.push(("missing required participant", plan, original.clone()));

    let mut plan = base_plan.clone();
    plan.path_patterns[0].to_resource = Some(resource(99));
    cases.push(("unplanned endpoint constraint", plan, original.clone()));

    let mut plan = base_plan.clone();
    plan.allowed_relation_types = vec!["deposit".into()];
    cases.push(("relation outside allowed types", plan, original.clone()));

    let mut plan = base_plan;
    plan.allowed_namespaces = vec![RelationNamespace::Evidence];
    cases.push(("namespace outside allowed set", plan, original));

    for (case, plan, path) in cases {
        assert_graph_hits_invalid(
            case,
            &plan,
            vec![GraphRetrievalHit {
                candidate: graph_candidate("private-path", 11),
                paths: vec![path],
            }],
        )
        .await;
    }
}

#[tokio::test]
async fn graph_rejects_disconnected_second_hop() {
    let mut plan = graph_plan();
    plan.path_patterns.push(RelationPathPattern::new(
        RelationNamespace::Discovery,
        "loan",
        "borrower",
        "product",
    ));
    plan.expansion_budget.max_hops = 2;
    let mut valid_path = graph_path();
    valid_path.resource_path.push(resource(14));
    valid_path.steps.push(GraphPathStepEvidence {
        relation_id: RelationId::from_uuid(Uuid::from_u128(15)),
        namespace: RelationNamespace::Discovery,
        relation_type: "loan".into(),
        from_role: "borrower".into(),
        from_resource: resource(11),
        to_role: "product".into(),
        to_resource: resource(14),
        participants: vec![
            RelationParticipant::new("borrower", resource(11)),
            RelationParticipant::new("product", resource(14)),
        ],
        evidence_refs: vec![],
        provenance: None,
    });
    let retrievers = graph_retrievers(vec![GraphRetrievalHit {
        candidate: graph_candidate("valid-two-hop", 14),
        paths: vec![valid_path.clone()],
    }]);
    let access = CurrentAccess::allowed();
    let action = action(RetrieverKind::HyperGraph);
    let request = request();
    let mut args = input(&action, &request);
    args.graph_plan = Some(&plan);
    let result = RetrievalExecutor::execute(&ports(&retrievers, &access), args)
        .await
        .unwrap();
    assert_eq!(result.hits[0].graph_paths, Some(vec![valid_path.clone()]));

    let mut path = valid_path;
    path.steps[1].from_resource = resource(99);
    path.steps[1].participants[0].resource_ref = resource(99);
    assert_graph_hits_invalid(
        "disconnected second hop",
        &plan,
        vec![GraphRetrievalHit {
            candidate: graph_candidate("private-path", 14),
            paths: vec![path],
        }],
    )
    .await;
}

#[tokio::test]
async fn graph_rejects_excess_returned_paths_relations_and_branches() {
    let mut first_branch = graph_path();
    first_branch.steps[0]
        .participants
        .push(RelationParticipant::new("product", resource(14)));
    first_branch.steps[0]
        .evidence_refs
        .push("second-ref".into());
    let mut second_branch = first_branch.clone();
    second_branch.resource_path[1] = resource(14);
    second_branch.steps[0].to_resource = resource(14);
    second_branch.steps[0].participants.reverse();
    second_branch.steps[0].evidence_refs.reverse();
    let mut single_relation_plan = graph_plan();
    single_relation_plan.expansion_budget.max_relations = 1;
    let retrievers = graph_retrievers(vec![
        GraphRetrievalHit {
            candidate: graph_candidate("branch-1", 11),
            paths: vec![first_branch],
        },
        GraphRetrievalHit {
            candidate: graph_candidate("branch-2", 14),
            paths: vec![second_branch],
        },
    ]);
    let access = CurrentAccess::allowed();
    let action = action(RetrieverKind::HyperGraph);
    let request = request();
    let mut args = input(&action, &request);
    args.graph_plan = Some(&single_relation_plan);
    assert_eq!(
        RetrievalExecutor::execute(&ports(&retrievers, &access), args)
            .await
            .unwrap()
            .hits
            .len(),
        2
    );

    let first = graph_path();
    let mut second = graph_path();
    second.steps[0].relation_id = RelationId::from_uuid(Uuid::from_u128(13));
    let paths = vec![first, second];
    for (case, adjust) in [
        ("path budget", 0),
        ("relation budget", 1),
        ("branching budget", 2),
    ] {
        let mut plan = graph_plan();
        match adjust {
            0 => plan.expansion_budget.max_paths = 1,
            1 => plan.expansion_budget.max_relations = 1,
            _ => plan.expansion_budget.max_branching_per_node = 1,
        }
        assert_graph_hits_invalid(
            case,
            &plan,
            vec![GraphRetrievalHit {
                candidate: graph_candidate("private-path", 11),
                paths: paths.clone(),
            }],
        )
        .await;
    }
}

#[tokio::test]
async fn graph_rejects_conflicting_metadata_for_one_relation_id_across_paths() {
    let first = graph_path();
    for (case, alter) in [("participants", 0), ("evidence refs", 1), ("provenance", 2)] {
        let mut second = first.clone();
        match alter {
            0 => second.steps[0]
                .participants
                .push(RelationParticipant::new("approver", resource(14))),
            1 => second.steps[0].evidence_refs.push("another-ref".into()),
            _ => second.steps[0].provenance = Some("another-origin".into()),
        }
        assert_graph_hits_invalid(
            case,
            &graph_plan(),
            vec![GraphRetrievalHit {
                candidate: graph_candidate("private-relation", 11),
                paths: vec![first.clone(), second],
            }],
        )
        .await;
    }
}

#[tokio::test]
async fn graph_rejects_reused_relation_id_with_different_namespace_or_type() {
    for (case, second_namespace, second_type) in [
        ("namespace", RelationNamespace::Evidence, "loan"),
        ("type", RelationNamespace::Discovery, "deposit"),
    ] {
        let mut plan = graph_plan();
        plan.path_patterns.push(RelationPathPattern::new(
            second_namespace,
            second_type,
            "product",
            "document",
        ));
        plan.allowed_namespaces.push(second_namespace);
        plan.allowed_relation_types.push(second_type.into());
        plan.expansion_budget.max_hops = 2;
        let mut path = graph_path();
        path.resource_path.push(resource(14));
        path.steps[0]
            .participants
            .push(RelationParticipant::new("document", resource(14)));
        let first_step = path.steps[0].clone();
        path.steps.push(GraphPathStepEvidence {
            relation_id: first_step.relation_id,
            namespace: second_namespace,
            relation_type: second_type.into(),
            from_role: "product".into(),
            from_resource: resource(11),
            to_role: "document".into(),
            to_resource: resource(14),
            participants: first_step.participants,
            evidence_refs: first_step.evidence_refs,
            provenance: first_step.provenance,
        });
        assert_graph_hits_invalid(
            case,
            &plan,
            vec![GraphRetrievalHit {
                candidate: graph_candidate("private-relation", 14),
                paths: vec![path],
            }],
        )
        .await;
    }
}

#[tokio::test]
async fn mismatched_source_or_unplanned_action_never_calls_a_port() {
    let retrievers = Retrievers::default();
    let access = CurrentAccess::allowed();
    let request = request();
    let mut action = action(RetrieverKind::Directory);
    action.source_id = source(9);
    assert!(matches!(
        RetrievalExecutor::execute(&ports(&retrievers, &access), input(&action, &request)).await,
        Err(SearchError::InvalidRequest(_))
    ));
    action.source_id = source(1);
    action.state =
        ActionState::Unsupported(search_application::retrieval::ActionIssue::NoExecutionPort);
    assert!(matches!(
        RetrievalExecutor::execute(&ports(&retrievers, &access), input(&action, &request)).await,
        Err(SearchError::InvalidRequest(_))
    ));
    assert!(retrievers.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn wrong_source_hit_is_rejected_and_vector_or_remote_is_unsupported() {
    let mut other_source = candidate("wrong-source");
    other_source.source_ref = source(9);
    let retrievers = Retrievers {
        directory: vec![other_source],
        ..Default::default()
    };
    let access = CurrentAccess::allowed();
    let request = request();
    let directory_action = action(RetrieverKind::Directory);
    assert!(matches!(
        RetrievalExecutor::execute(
            &ports(&retrievers, &access),
            input(&directory_action, &request)
        )
        .await,
        Err(SearchError::OperationFailed(_))
    ));

    let empty = Retrievers::default();
    for kind in [
        RetrieverKind::Vector,
        RetrieverKind::RemoteQuery,
        RetrieverKind::RemoteEnumeration,
    ] {
        let action = action(kind);
        assert!(matches!(
            RetrievalExecutor::execute(&ports(&empty, &access), input(&action, &request)).await,
            Err(SearchError::InvalidRequest(_))
        ));
    }
    assert!(empty.calls.lock().unwrap().is_empty());
}
