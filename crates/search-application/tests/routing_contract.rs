use std::collections::BTreeMap;

use search_application::retrieval::{
    ActionIssue, ActionState, CursorExecution, PresenceAssessment, RetrievalInputs,
    RetrievalObservation, RetrieverKind, RetrieverPlanner, RetrieverProfile, RetrieverSupport,
};
use search_application::routing::{
    RouteStage, RouteState, RoutingConstraints, SourceRole, SourceRouter,
};
use search_application::{retrieval, routing};
use search_core::discovery::{DiscoveryNeed, GapReason};
use search_core::evidence::EvidenceRequirement;
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{DiscoveryEvaluationId, NeedId, ResourceId, SourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::relation::RelationNamespace;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_core::temporal::TemporalEvaluationContext;
use time::OffsetDateTime;
use uuid::Uuid;

fn source_id(value: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(value))
}

fn valid_graph_plan() -> GraphTraversalPlan {
    let now = OffsetDateTime::from_unix_timestamp(100).unwrap();
    GraphTraversalPlan {
        seed_nodes: vec![ResourceId::from_uuid(Uuid::from_u128(101))],
        path_patterns: vec![RelationPathPattern::new(
            RelationNamespace::Discovery,
            "relation",
            "from",
            "to",
        )],
        allowed_relation_types: vec!["relation".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: None,
        temporal_context: Some(TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(102)),
            now,
            now,
            "Asia/Tokyo",
        )),
        access_context: "principal".into(),
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

fn need(kinds: Vec<ResourceKind>) -> DiscoveryNeed {
    DiscoveryNeed {
        need_id: NeedId::from_uuid(Uuid::from_u128(99)),
        intent_signature: IntentSignature::new(IntentFact::new(
            "find relevant resource".into(),
            IntentFactOrigin::Explicit,
        )),
        required_resource_types: kinds,
        required_claims: vec![],
        authority_requirements: vec![],
        freshness_requirements: vec![],
        constraints: vec![],
        completion_requirement: EvidenceRequirement::new(vec![]),
    }
}

fn source(
    id: u128,
    kinds: Vec<ResourceKind>,
    modes: Vec<DiscoveryMode>,
    enumeration: EnumerationSemantics,
) -> DiscoverableSource {
    let mut source = DiscoverableSource::new(
        source_id(id),
        "synthetic",
        enumeration,
        RetentionMode::PersistentDiscoveryMetadata,
    );
    source.resource_types = kinds;
    source.discovery_modes = modes;
    source
}

#[test]
fn missing_required_source_remains_a_blocking_initial_route_even_at_zero_budget() {
    let plan = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        &[],
        &RoutingConstraints {
            required_source_ids: vec![source_id(1)],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
    );

    assert_eq!(plan.routes.len(), 1);
    let route = &plan.routes[0];
    assert_eq!(route.source_id, source_id(1));
    assert_eq!(route.role, SourceRole::Required);
    assert_eq!(route.stage, RouteStage::Initial);
    assert!(matches!(route.state, RouteState::Unresolved(_)));
    assert!(
        route
            .unresolved_gaps
            .iter()
            .any(|gap| { gap.blocking && gap.reason == GapReason::Availability })
    );
}

#[test]
fn duplicate_source_id_routes_are_order_independent_and_have_no_capabilities() {
    let local = source(
        1,
        vec![ResourceKind::Knowledge],
        vec![DiscoveryMode::LocalDirectory],
        EnumerationSemantics::Complete,
    );
    let remote = source(
        1,
        vec![ResourceKind::Workflow],
        vec![DiscoveryMode::RemoteQuery],
        EnumerationSemantics::QueryOnly,
    );
    let request = need(vec![ResourceKind::Knowledge]);

    for constraints in [
        RoutingConstraints {
            required_source_ids: vec![source_id(1)],
            ..RoutingConstraints::default()
        },
        RoutingConstraints::default(),
    ] {
        let forward = SourceRouter::plan(&request, &[local.clone(), remote.clone()], &constraints);
        let reversed = SourceRouter::plan(&request, &[remote.clone(), local.clone()], &constraints);

        assert_eq!(forward, reversed);
        let support = RetrieverSupport {
            directory: true,
            ..RetrieverSupport::default()
        };
        let inputs = RetrievalInputs {
            max_initial_retrievers_per_source: 1,
            ..RetrievalInputs::default()
        };
        assert_eq!(
            RetrieverPlanner::plan(RetrieverProfile::Identity, &forward, &support, &inputs),
            RetrieverPlanner::plan(RetrieverProfile::Identity, &reversed, &support, &inputs)
        );
        assert_eq!(forward.routes.len(), 1);
        let route = &forward.routes[0];
        assert_eq!(
            route.state,
            RouteState::Unresolved(routing::RouteIssue::ConflictingRegistryEntries)
        );
        assert_eq!(route.discovery_mode, None);
        assert!(route.discovery_modes.is_empty());
        assert_eq!(route.enumeration_semantics, None);
        assert_eq!(
            route.stage,
            if constraints.required_source_ids.is_empty() {
                RouteStage::Expansion
            } else {
                RouteStage::Initial
            }
        );
        assert!(route.unresolved_gaps.iter().any(|gap| {
            gap.reason == GapReason::Availability
                && gap.blocking == !constraints.required_source_ids.is_empty()
        }));
    }
}

#[test]
fn registry_capabilities_and_typed_need_drive_optional_route_staging() {
    let local = source(
        1,
        vec![ResourceKind::Knowledge],
        vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
        ],
        EnumerationSemantics::Complete,
    );
    let remote = source(
        2,
        vec![ResourceKind::Knowledge],
        vec![DiscoveryMode::RemoteQuery],
        EnumerationSemantics::QueryOnly,
    );
    let unrelated = source(
        3,
        vec![ResourceKind::Workflow],
        vec![DiscoveryMode::LocalDirectory],
        EnumerationSemantics::Complete,
    );
    let plan = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        &[remote, unrelated, local],
        &RoutingConstraints {
            required_source_ids: vec![],
            preferred_source_ids: vec![source_id(2)],
            max_initial_optional_sources: 1,
        },
    );

    assert_eq!(plan.routes.len(), 2);
    assert_eq!(plan.routes[0].source_id, source_id(2));
    assert_eq!(plan.routes[0].role, SourceRole::Preferred);
    assert_eq!(plan.routes[0].stage, RouteStage::Expansion);
    assert!(matches!(plan.routes[0].state, RouteState::Unsupported(_)));
    assert_eq!(plan.routes[1].source_id, source_id(1));
    assert_eq!(plan.routes[1].stage, RouteStage::Initial);
    assert_eq!(plan.routes[1].discovery_modes.len(), 2);
}

#[test]
fn free_form_authority_and_freshness_labels_do_not_resolve_need_requirements() {
    let mut required = source(
        1,
        vec![ResourceKind::Policy],
        vec![DiscoveryMode::LocalDirectory],
        EnumerationSemantics::Complete,
    );
    required.authority_scope = Some("official".into());
    required.freshness_policy = Some("current".into());
    let mut request = need(vec![ResourceKind::Policy]);
    request.authority_requirements.push("official".into());
    request.freshness_requirements.push("current".into());

    let plan = SourceRouter::plan(
        &request,
        &[required],
        &RoutingConstraints {
            required_source_ids: vec![source_id(1)],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
    );

    assert!(
        plan.routes[0]
            .unresolved_gaps
            .iter()
            .any(|gap| gap.blocking && gap.reason == GapReason::Authority)
    );
    assert!(
        plan.routes[0]
            .unresolved_gaps
            .iter()
            .any(|gap| gap.blocking && gap.reason == GapReason::Freshness)
    );
}

#[test]
fn query_miss_never_proves_absence_even_if_source_claims_complete_coverage() {
    let complete = source(
        1,
        vec![ResourceKind::Knowledge],
        vec![DiscoveryMode::LocalDirectory],
        EnumerationSemantics::Complete,
    );
    let query_only = source(
        2,
        vec![ResourceKind::Knowledge],
        vec![DiscoveryMode::RemoteQuery],
        EnumerationSemantics::QueryOnly,
    );
    let plan = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        &[complete, query_only],
        &RoutingConstraints::default(),
    );
    let complete = plan
        .routes
        .iter()
        .find(|route| route.source_id == source_id(1))
        .unwrap();
    let query_only = plan
        .routes
        .iter()
        .find(|route| route.source_id == source_id(2))
        .unwrap();

    assert_eq!(
        retrieval::assess_presence(complete, RetrievalObservation::QueryMiss),
        PresenceAssessment::Unresolved
    );
    assert_eq!(
        retrieval::assess_presence(query_only, RetrievalObservation::QueryMiss),
        PresenceAssessment::Unresolved
    );
    assert_eq!(
        retrieval::assess_presence(complete, RetrievalObservation::CompleteEnumerationMiss),
        PresenceAssessment::AbsentByCompleteEnumeration
    );
    assert_eq!(
        retrieval::assess_presence(query_only, RetrievalObservation::CompleteEnumerationMiss),
        PresenceAssessment::Unresolved
    );
}

#[test]
fn capability_matched_actions_expose_only_planned_initial_order_to_s1() {
    let local = source(
        1,
        vec![ResourceKind::Knowledge],
        vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
        ],
        EnumerationSemantics::Complete,
    );
    let route_plan = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        &[local],
        &RoutingConstraints {
            required_source_ids: vec![],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 1,
        },
    );
    let plan = RetrieverPlanner::plan(
        RetrieverProfile::Knowledge,
        &route_plan,
        &RetrieverSupport {
            directory: true,
            structured: true,
            lexical: true,
            hypergraph: false,
            vector: false,
            ..RetrieverSupport::default()
        },
        &RetrievalInputs {
            lexical_query: Some("policy".into()),
            graph_plans: BTreeMap::new(),
            vector_query_available: false,
            max_initial_retrievers_per_source: 2,
            ..RetrievalInputs::default()
        },
    );

    assert_eq!(
        plan.initial_actions
            .iter()
            .map(|action| action.retriever)
            .collect::<Vec<_>>(),
        vec![RetrieverKind::Structured, RetrieverKind::Lexical]
    );
    assert!(
        plan.initial_actions
            .iter()
            .all(|action| action.state == ActionState::Planned)
    );
    assert_eq!(
        plan.s1_retriever_order,
        plan.initial_actions
            .iter()
            .map(|action| action.retriever_id.clone())
            .collect::<Vec<_>>()
    );
    assert!(plan.expansion_actions.iter().any(|action| {
        action.retriever == RetrieverKind::HyperGraph
            && matches!(action.state, ActionState::Unsupported(_))
    }));
    assert!(plan.expansion_actions.iter().any(|action| {
        action.retriever == RetrieverKind::Vector
            && matches!(action.state, ActionState::Unsupported(_))
    }));
}

#[test]
fn exploratory_starts_supported_actions_in_profile_order_without_a_vector_adapter() {
    let local = source(
        1,
        vec![ResourceKind::Knowledge],
        vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
            DiscoveryMode::RemoteQuery,
        ],
        EnumerationSemantics::Complete,
    );
    let routes = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        &[local],
        &RoutingConstraints {
            max_initial_optional_sources: 1,
            ..RoutingConstraints::default()
        },
    );
    let plan = RetrieverPlanner::plan(
        RetrieverProfile::Exploratory,
        &routes,
        &RetrieverSupport {
            directory: true,
            structured: true,
            lexical: true,
            hypergraph: true,
            vector: false,
            ..RetrieverSupport::default()
        },
        &RetrievalInputs {
            lexical_query: Some("policy".into()),
            graph_plans: BTreeMap::from([(source_id(1), valid_graph_plan())]),
            vector_query_available: false,
            max_initial_retrievers_per_source: 4,
            ..RetrievalInputs::default()
        },
    );

    assert_eq!(
        plan.initial_actions
            .iter()
            .map(|action| action.retriever)
            .collect::<Vec<_>>(),
        vec![
            RetrieverKind::Lexical,
            RetrieverKind::HyperGraph,
            RetrieverKind::Directory,
            RetrieverKind::Structured,
        ]
    );
    assert_eq!(
        plan.s1_retriever_order,
        plan.initial_actions
            .iter()
            .map(|action| action.retriever_id.clone())
            .collect::<Vec<_>>()
    );
    assert!(plan.expansion_actions.iter().any(|action| {
        action.retriever == RetrieverKind::Vector
            && action.state == ActionState::Unsupported(ActionIssue::AdapterUnavailable)
    }));
    assert!(plan.expansion_actions.iter().any(|action| {
        action.retriever == RetrieverKind::RemoteQuery
            && action.state == ActionState::Unsupported(ActionIssue::NoExecutionPort)
    }));
}

#[test]
fn exploratory_places_supported_vector_after_hypergraph() {
    let local = source(
        1,
        vec![ResourceKind::Knowledge],
        vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
        ],
        EnumerationSemantics::Complete,
    );
    let routes = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        &[local],
        &RoutingConstraints {
            max_initial_optional_sources: 1,
            ..RoutingConstraints::default()
        },
    );
    let plan = RetrieverPlanner::plan(
        RetrieverProfile::Exploratory,
        &routes,
        &RetrieverSupport {
            directory: true,
            structured: true,
            lexical: true,
            hypergraph: true,
            vector: true,
            ..RetrieverSupport::default()
        },
        &RetrievalInputs {
            lexical_query: Some("policy".into()),
            graph_plans: BTreeMap::from([(source_id(1), valid_graph_plan())]),
            vector_query_available: true,
            max_initial_retrievers_per_source: 3,
            ..RetrievalInputs::default()
        },
    );
    assert_eq!(
        plan.initial_actions
            .iter()
            .map(|action| action.retriever)
            .collect::<Vec<_>>(),
        // E: a Graph-only answer is never displaced by similarity candidates.
        vec![
            RetrieverKind::Lexical,
            RetrieverKind::HyperGraph,
            RetrieverKind::Vector,
        ]
    );
}

#[test]
fn hypergraph_action_requires_a_valid_plan_for_the_same_source() {
    let local = source(
        1,
        vec![ResourceKind::Knowledge],
        vec![DiscoveryMode::LocalDirectory],
        EnumerationSemantics::Complete,
    );
    let routes = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        &[local],
        &RoutingConstraints {
            required_source_ids: vec![],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 1,
        },
    );
    let support = RetrieverSupport {
        hypergraph: true,
        ..RetrieverSupport::default()
    };
    let mut inputs = RetrievalInputs {
        max_initial_retrievers_per_source: 1,
        ..RetrievalInputs::default()
    };
    inputs.graph_plans.insert(source_id(2), valid_graph_plan());

    let wrong_source = RetrieverPlanner::plan(
        RetrieverProfile::EvidenceInvestigation,
        &routes,
        &support,
        &inputs,
    );
    assert!(wrong_source.s1_retriever_order.is_empty());
    assert!(wrong_source.expansion_actions.iter().any(|action| {
        action.retriever == RetrieverKind::HyperGraph
            && action.state == ActionState::Unresolved(ActionIssue::MissingGraphPlan)
    }));

    let mut invalid = valid_graph_plan();
    invalid.expansion_budget.max_paths = 0;
    inputs.graph_plans.insert(source_id(1), invalid);
    let invalid = RetrieverPlanner::plan(
        RetrieverProfile::EvidenceInvestigation,
        &routes,
        &support,
        &inputs,
    );
    assert!(invalid.s1_retriever_order.is_empty());

    inputs.graph_plans.insert(source_id(1), valid_graph_plan());
    let valid = RetrieverPlanner::plan(
        RetrieverProfile::EvidenceInvestigation,
        &routes,
        &support,
        &inputs,
    );
    assert_eq!(valid.initial_actions.len(), 1);
    assert_eq!(
        valid.initial_actions[0].retriever,
        RetrieverKind::HyperGraph
    );
    assert_eq!(valid.initial_actions[0].state, ActionState::Planned);
}

#[test]
fn default_budget_does_not_start_optional_or_required_retrieval() {
    let local = source(
        1,
        vec![ResourceKind::Knowledge],
        vec![DiscoveryMode::LocalDirectory],
        EnumerationSemantics::Complete,
    );
    let support = RetrieverSupport {
        directory: true,
        ..RetrieverSupport::default()
    };

    let optional = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        std::slice::from_ref(&local),
        &RoutingConstraints::default(),
    );
    assert_eq!(optional.routes[0].stage, RouteStage::Expansion);
    let optional_retrieval = RetrieverPlanner::plan(
        RetrieverProfile::Identity,
        &optional,
        &support,
        &RetrievalInputs::default(),
    );
    assert!(optional_retrieval.initial_actions.is_empty());
    assert!(
        optional_retrieval
            .blocking_gaps
            .iter()
            .any(|gap| gap.blocking)
    );

    let required = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        &[local],
        &RoutingConstraints {
            required_source_ids: vec![source_id(1)],
            ..RoutingConstraints::default()
        },
    );
    assert_eq!(required.routes[0].stage, RouteStage::Initial);
    let required_retrieval = RetrieverPlanner::plan(
        RetrieverProfile::Identity,
        &required,
        &support,
        &RetrievalInputs::default(),
    );
    assert!(required_retrieval.initial_actions.is_empty());
    assert!(required_retrieval.s1_retriever_order.is_empty());
    assert!(
        required_retrieval
            .blocking_gaps
            .iter()
            .any(|gap| gap.blocking)
    );
}

#[test]
fn remote_only_required_route_is_blocking_and_has_no_s1_order() {
    let remote = source(
        2,
        vec![ResourceKind::Knowledge],
        vec![DiscoveryMode::RemoteQuery],
        EnumerationSemantics::QueryOnly,
    );
    let route_plan = SourceRouter::plan(
        &need(vec![ResourceKind::Knowledge]),
        &[remote],
        &RoutingConstraints {
            required_source_ids: vec![source_id(2)],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
    );
    let plan = RetrieverPlanner::plan(
        RetrieverProfile::Exploratory,
        &route_plan,
        &RetrieverSupport::default(),
        &RetrievalInputs::default(),
    );

    assert!(plan.s1_retriever_order.is_empty());
    assert!(plan.initial_actions.is_empty());
    assert!(plan.expansion_actions.iter().any(|action| {
        action.retriever == RetrieverKind::RemoteQuery
            && matches!(action.state, ActionState::Unsupported(_))
    }));
    assert!(plan.blocking_gaps.iter().any(|gap| gap.blocking));
}

#[test]
fn lexical_window_expansion_is_planning_only_without_cursor_port_support() {
    let state = retrieval::RetrieverCursorState::initial().plan_window_expansion(20);
    assert_eq!(state.execution, CursorExecution::PlanningOnly);
    assert_eq!(state.requested_limit, Some(20));
    assert!(!state.reuses_prior_results);
    let next = state.plan_window_expansion(40);
    assert_eq!(next.requested_limit, Some(40));
    assert!(!next.reuses_prior_results);
}
