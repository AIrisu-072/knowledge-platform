use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use search_application::ports::StructuredFacetFilter;
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig, TrustedVisibleRegistry,
};
use search_application::retrieval::{
    ActionIssue, ActionState, CursorExecution, LiveInput, OpaqueNativeId, PresenceAssessment,
    RemoteQueryInput, RetrievalInputs, RetrievalObservation, RetrieverKind, RetrieverPlanner,
    RetrieverProfile, RetrieverSupport, assess_presence,
};
use search_application::routing::{
    RouteIssue, RouteStage, RouteState, RoutingConstraints, SourceRoutePlan, SourceRouter,
};
use search_application::scoped::{
    AccessContextAuthorityPort, AccessRevision, PrincipalRef, RegistrationRevision,
    ScopedSourceRegistryPort, SyntheticAuthorityAdapter, SyntheticVisibilityAdapter, TenantId,
    VisibilityRevision,
};
use search_application::source_registration::{
    CompleteDesiredRegistrations, RegistrationNamespace, RegistrationSetRevision,
    SourceRegistration, SourceRegistrationCatalog, SyntheticHostRegistrationAuthority,
    SyntheticRegistrationLedger,
};
use search_application::visible_routing::VisibleRouting;
use search_core::discovery::{DiscoveryNeed, GapReason};
use search_core::evidence::EvidenceRequirement;
use search_core::id::{NeedId, SourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::predicate::TypedValue;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use uuid::Uuid;

const REMOTE_MODES: [DiscoveryMode; 4] = [
    DiscoveryMode::RemoteEnumeration,
    DiscoveryMode::RemoteQuery,
    DiscoveryMode::DirectAddress,
    DiscoveryMode::LiveOnly,
];

fn sid(value: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(value))
}

fn need() -> DiscoveryNeed {
    DiscoveryNeed {
        need_id: NeedId::from_uuid(Uuid::from_u128(99)),
        intent_signature: IntentSignature::new(IntentFact::new(
            "find knowledge".into(),
            IntentFactOrigin::Explicit,
        )),
        required_resource_types: vec![ResourceKind::Knowledge],
        required_claims: vec![],
        authority_requirements: vec![],
        freshness_requirements: vec![],
        constraints: vec![],
        completion_requirement: EvidenceRequirement::new(vec![]),
    }
}

fn source(id: u128, modes: Vec<DiscoveryMode>) -> DiscoverableSource {
    let mut source = DiscoverableSource::new(
        sid(id),
        "synthetic",
        EnumerationSemantics::Complete,
        RetentionMode::SessionOnly,
    );
    source.discovery_modes = modes;
    source.resource_types = vec![ResourceKind::Knowledge];
    source
}

fn support() -> RetrieverSupport {
    RetrieverSupport {
        remote_enumeration: true,
        remote_query: true,
        direct_address: true,
        live_only: true,
        ..RetrieverSupport::default()
    }
}

fn query() -> RemoteQueryInput {
    RemoteQueryInput::new("policy", vec![], 10, &[]).unwrap()
}

fn inputs() -> RetrievalInputs {
    RetrievalInputs {
        remote_queries: BTreeMap::from([(sid(1), query())]),
        native_ids: BTreeMap::from([(sid(1), OpaqueNativeId::new("catalog-one").unwrap())]),
        live_inputs: BTreeMap::from([(sid(1), LiveInput::query(query()))]),
        max_initial_retrievers_per_source: 4,
        ..RetrievalInputs::default()
    }
}

fn required_routes(modes: Vec<DiscoveryMode>) -> SourceRoutePlan {
    SourceRouter::plan_with_runtime_modes(
        &need(),
        &[source(1, modes)],
        &RoutingConstraints {
            required_source_ids: vec![sid(1)],
            ..RoutingConstraints::default()
        },
        &REMOTE_MODES,
    )
}

#[tokio::test]
async fn registered_remote_modes_plan_only_with_port_and_input() {
    let tenant = TenantId::new("tenant-a").unwrap();
    let principal = PrincipalRef::new("reader").unwrap();
    let registration = |id, modes| {
        SourceRegistration::Remote(
            RemoteSourceRegistration::from_server_config(ServerRemoteRegistrationConfig {
                tenant: tenant.clone(),
                source_id: sid(id),
                provider_kind: "synthetic".into(),
                endpoint: RegisteredEndpoint::new("https", "catalog.example.test", 443, "/v1")
                    .unwrap(),
                supported_modes: modes,
                enumeration_semantics: EnumerationSemantics::QueryOnly,
                authority_predicates: vec![],
                allowed_resource_kinds: vec![ResourceKind::Knowledge],
                current_access_contract: CurrentAccessContract::PerItem,
                retention_mode: RetentionMode::SessionOnly,
                freshness_policy: None,
                canonical_upstream_lineage: "synthetic".into(),
                limits: RemoteRegistrationLimits::synthetic_canary(),
                registration_revision: RegistrationRevision::new(1).unwrap(),
                visibility_revision: VisibilityRevision::new(1).unwrap(),
            })
            .unwrap(),
        )
    };
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    host.publish(
        RegistrationNamespace::Document,
        RegistrationSetRevision::new(1).unwrap(),
        vec![],
    )
    .unwrap();
    host.publish(
        RegistrationNamespace::Remote,
        RegistrationSetRevision::new(1).unwrap(),
        vec![
            registration(1, REMOTE_MODES.to_vec()),
            registration(2, vec![DiscoveryMode::RemoteQuery]),
            registration(3, REMOTE_MODES.to_vec()),
        ],
    )
    .unwrap();
    let document = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Document)
        .await
        .unwrap();
    let remote = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote)
        .await
        .unwrap();
    let catalog = SourceRegistrationCatalog::try_new(
        Arc::new(SyntheticRegistrationLedger::with_host(host)),
        &document,
        &remote,
    )
    .await
    .unwrap();
    let authority = SyntheticAuthorityAdapter::new();
    let handle = authority
        .issue_verified_identity(
            tenant.clone(),
            principal.clone(),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    for id in [1, 2] {
        visibility
            .grant(
                tenant.clone(),
                principal.clone(),
                sid(id),
                RegistrationRevision::new(1).unwrap(),
                VisibilityRevision::new(1).unwrap(),
            )
            .unwrap();
    }
    let registry = TrustedVisibleRegistry::new(&authority, &visibility, &catalog);
    let visible = registry.visible_sources(&actor).await.unwrap();
    let (sources, constraints, visibility_gaps) = VisibleRouting::prepare(
        &actor,
        visible.entries(),
        &RoutingConstraints {
            required_source_ids: vec![sid(1), sid(2), sid(3), sid(999)],
            ..RoutingConstraints::default()
        },
    )
    .unwrap();
    assert_eq!(visibility_gaps.len(), 1);
    assert_eq!(
        visibility_gaps[0].required_fact,
        "required_source_unavailable"
    );
    let routes =
        SourceRouter::plan_with_runtime_modes(&need(), &sources, &constraints, &REMOTE_MODES);
    for (index, kind) in [
        RetrieverKind::RemoteEnumeration,
        RetrieverKind::RemoteQuery,
        RetrieverKind::DirectAddress,
        RetrieverKind::LiveOnly,
    ]
    .into_iter()
    .enumerate()
    {
        let mut wired = support();
        match index {
            0 => wired.remote_enumeration = false,
            1 => wired.remote_query = false,
            2 => wired.direct_address = false,
            _ => wired.live_only = false,
        }
        let plan = RetrieverPlanner::plan(RetrieverProfile::Knowledge, &routes, &wired, &inputs());
        assert!(
            plan.expansion_actions
                .iter()
                .any(|action| action.source_id == sid(1)
                    && action.retriever == kind
                    && action.state == ActionState::Unsupported(ActionIssue::NoExecutionPort))
        );
        assert_eq!(plan.initial_actions.len(), 3);
        assert!(
            plan.initial_actions
                .iter()
                .chain(&plan.expansion_actions)
                .all(|action| action.source_id == sid(1)
                    || (action.source_id == sid(2)
                        && action.retriever == RetrieverKind::RemoteQuery))
        );
    }
    let plan = RetrieverPlanner::plan(RetrieverProfile::Knowledge, &routes, &support(), &inputs());
    assert_eq!(plan.initial_actions.len(), 4);
    assert!(
        plan.expansion_actions
            .iter()
            .any(|action| action.source_id == sid(2)
                && action.state == ActionState::Unresolved(ActionIssue::MissingRemoteQuery))
    );
}

#[test]
fn required_remote_has_initial_action() {
    for mode in REMOTE_MODES {
        let routes = required_routes(vec![mode]);
        let plan =
            RetrieverPlanner::plan(RetrieverProfile::Knowledge, &routes, &support(), &inputs());
        assert_eq!(routes.routes[0].stage, RouteStage::Initial);
        assert_eq!(routes.routes[0].state, RouteState::Planned);
        assert!(routes.routes[0].unresolved_gaps.is_empty());
        assert_eq!(plan.initial_actions.len(), 1);
        assert!(plan.blocking_gaps.is_empty());
        assert_eq!(plan.initial_actions[0].state, ActionState::Planned);
        assert_eq!(
            plan.initial_actions[0].cursor.execution,
            CursorExecution::PlanningOnly
        );
        assert!(!plan.initial_actions[0].cursor.reuses_prior_results);
    }
}

#[test]
fn missing_live_or_native_id_is_unresolved() {
    let routes = required_routes(REMOTE_MODES.to_vec());
    let plan = RetrieverPlanner::plan(
        RetrieverProfile::Knowledge,
        &routes,
        &support(),
        &RetrievalInputs {
            max_initial_retrievers_per_source: 4,
            ..RetrievalInputs::default()
        },
    );
    assert_eq!(
        plan.initial_actions[0].retriever,
        RetrieverKind::RemoteEnumeration
    );
    assert_eq!(plan.initial_actions.len(), 1);
    for (kind, issue) in [
        (RetrieverKind::RemoteQuery, ActionIssue::MissingRemoteQuery),
        (RetrieverKind::DirectAddress, ActionIssue::MissingNativeId),
        (RetrieverKind::LiveOnly, ActionIssue::MissingLiveInput),
    ] {
        assert!(plan.expansion_actions.iter().any(
            |action| action.retriever == kind && action.state == ActionState::Unresolved(issue)
        ));
    }
}

#[test]
fn remote_order_preserves_s1_local_order() {
    let local = source(
        2,
        vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
        ],
    );
    let routes = SourceRouter::plan_with_runtime_modes(
        &need(),
        &[source(1, REMOTE_MODES.to_vec()), local],
        &RoutingConstraints {
            required_source_ids: vec![sid(2), sid(1)],
            ..RoutingConstraints::default()
        },
        &[
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
            DiscoveryMode::RemoteEnumeration,
            DiscoveryMode::RemoteQuery,
            DiscoveryMode::DirectAddress,
            DiscoveryMode::LiveOnly,
        ],
    );
    let plan = RetrieverPlanner::plan(
        RetrieverProfile::Knowledge,
        &routes,
        &RetrieverSupport {
            directory: true,
            structured: true,
            lexical: true,
            ..support()
        },
        &RetrievalInputs {
            lexical_query: Some("policy".into()),
            ..inputs()
        },
    );
    assert_eq!(
        plan.initial_actions
            .iter()
            .map(|action| (action.source_id, action.retriever))
            .collect::<Vec<_>>(),
        vec![
            (sid(2), RetrieverKind::Structured),
            (sid(2), RetrieverKind::Lexical),
            (sid(2), RetrieverKind::Directory),
            (sid(1), RetrieverKind::RemoteEnumeration),
            (sid(1), RetrieverKind::RemoteQuery),
            (sid(1), RetrieverKind::DirectAddress),
            (sid(1), RetrieverKind::LiveOnly)
        ]
    );
    assert_eq!(
        plan.s1_retriever_order,
        plan.initial_actions
            .iter()
            .map(|action| action.retriever_id.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn local_only_profiles_keep_their_order() {
    let sources = [source(
        1,
        vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
        ],
    )];
    let constraints = RoutingConstraints {
        required_source_ids: vec![sid(1)],
        ..RoutingConstraints::default()
    };
    let legacy = SourceRouter::plan(&need(), &sources, &constraints);
    let explicit = SourceRouter::plan_with_runtime_modes(
        &need(),
        &sources,
        &constraints,
        &[
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
        ],
    );
    assert_eq!(legacy, explicit);
    for (profile, expected) in [
        (
            RetrieverProfile::Identity,
            vec![
                RetrieverKind::Directory,
                RetrieverKind::Structured,
                RetrieverKind::Lexical,
            ],
        ),
        (
            RetrieverProfile::Capability,
            vec![
                RetrieverKind::Structured,
                RetrieverKind::Directory,
                RetrieverKind::Lexical,
            ],
        ),
        (
            RetrieverProfile::Knowledge,
            vec![
                RetrieverKind::Structured,
                RetrieverKind::Lexical,
                RetrieverKind::Directory,
            ],
        ),
        (
            RetrieverProfile::EvidenceInvestigation,
            vec![
                RetrieverKind::Structured,
                RetrieverKind::Lexical,
                RetrieverKind::Directory,
            ],
        ),
        (
            RetrieverProfile::Exploratory,
            vec![
                RetrieverKind::Lexical,
                RetrieverKind::Directory,
                RetrieverKind::Structured,
            ],
        ),
    ] {
        let plan = RetrieverPlanner::plan(
            profile,
            &explicit,
            &RetrieverSupport {
                directory: true,
                structured: true,
                lexical: true,
                ..RetrieverSupport::default()
            },
            &RetrievalInputs {
                lexical_query: Some("policy".into()),
                max_initial_retrievers_per_source: 5,
                ..RetrievalInputs::default()
            },
        );
        assert_eq!(
            plan.initial_actions
                .iter()
                .map(|action| action.retriever)
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn budgets_and_route_failures_remain_explicit() {
    let routes = required_routes(REMOTE_MODES.to_vec());
    for limit in [0, 1, 4] {
        let plan = RetrieverPlanner::plan(
            RetrieverProfile::Knowledge,
            &routes,
            &support(),
            &RetrievalInputs {
                max_initial_retrievers_per_source: limit,
                ..inputs()
            },
        );
        assert_eq!(plan.initial_actions.len(), limit);
        assert_eq!(plan.expansion_actions.len(), 4 - limit);
        assert_eq!(plan.blocking_gaps.is_empty(), limit > 0);
    }
    let mut constrained_need = need();
    constrained_need
        .authority_requirements
        .push("issuer".into());
    constrained_need
        .freshness_requirements
        .push("current".into());
    let remote = source(1, REMOTE_MODES.to_vec());
    let constraints = RoutingConstraints {
        required_source_ids: vec![sid(1)],
        ..RoutingConstraints::default()
    };
    let legacy = SourceRouter::plan(&need(), std::slice::from_ref(&remote), &constraints);
    assert_eq!(
        legacy.routes[0].state,
        RouteState::Unsupported(RouteIssue::RuntimePortUnavailable)
    );
    let explicit = SourceRouter::plan_with_runtime_modes(
        &constrained_need,
        &[remote],
        &constraints,
        &REMOTE_MODES,
    );
    let plan = RetrieverPlanner::plan(
        RetrieverProfile::Knowledge,
        &explicit,
        &support(),
        &inputs(),
    );
    assert_eq!(
        plan.blocking_gaps
            .iter()
            .map(|gap| gap.reason)
            .collect::<Vec<_>>(),
        vec![GapReason::Authority, GapReason::Freshness]
    );
    let conflicting = SourceRouter::plan_with_runtime_modes(
        &need(),
        &[
            source(1, REMOTE_MODES.to_vec()),
            source(1, REMOTE_MODES.to_vec()),
        ],
        &constraints,
        &REMOTE_MODES,
    );
    let plan = RetrieverPlanner::plan(
        RetrieverProfile::Knowledge,
        &conflicting,
        &support(),
        &inputs(),
    );
    assert!(plan.initial_actions.is_empty());
    assert!(!plan.blocking_gaps.is_empty());
}

#[test]
fn query_and_live_misses_never_prove_absence() {
    for route in required_routes(REMOTE_MODES.to_vec()).routes {
        assert_eq!(
            assess_presence(&route, RetrievalObservation::QueryMiss),
            PresenceAssessment::Unresolved
        );
        assert_eq!(
            assess_presence(&route, RetrievalObservation::CompleteEnumerationMiss),
            PresenceAssessment::Unresolved
        );
    }
}

#[test]
fn typed_inputs_enforce_bounds_and_facet_allowlist() {
    let filter = || StructuredFacetFilter::eq("kind", TypedValue::String("policy".into()));
    let query = RemoteQueryInput::new("", vec![filter()], 100, &["kind"]).unwrap();
    assert_eq!(query.text(), "");
    assert_eq!(query.window(), 100);
    assert_eq!(query.facets(), &[filter()]);
    assert!(RemoteQueryInput::new(" ", vec![], 1, &[]).is_err());
    assert!(RemoteQueryInput::new("query", vec![], 0, &[]).is_err());
    assert!(RemoteQueryInput::new("query", vec![], 101, &[]).is_err());
    assert!(RemoteQueryInput::new("query", vec![filter()], 1, &[]).is_err());
    assert!(RemoteQueryInput::new("query", vec![filter(), filter()], 1, &["kind"]).is_err());
    assert!(RemoteQueryInput::new("x".repeat(4097), vec![], 1, &[]).is_err());
    assert!(RemoteQueryInput::new("\0".repeat(4096), vec![], 1, &[]).is_err());
    let names = (0..17)
        .map(|index| format!("facet{index}"))
        .collect::<Vec<_>>();
    let filters = names
        .iter()
        .map(|name| StructuredFacetFilter::eq(name, TypedValue::Bool(true)))
        .collect();
    let allowed = names.iter().map(String::as_str).collect::<Vec<_>>();
    assert!(RemoteQueryInput::new("query", filters, 1, &allowed).is_err());
    assert!(
        RemoteQueryInput::new(
            "query",
            vec![StructuredFacetFilter::eq(
                "kind",
                TypedValue::String("x".repeat(1025))
            )],
            1,
            &["kind"]
        )
        .is_err()
    );
    assert!(
        RemoteQueryInput::new(
            "query",
            vec![StructuredFacetFilter::eq("kind", TypedValue::List(vec![]))],
            1,
            &["kind"]
        )
        .is_err()
    );
    for invalid in [
        "".to_owned(),
        " ".to_owned(),
        "x".repeat(513),
        "識".repeat(171),
        "https://example.test/item".into(),
        "file:/tmp/item".into(),
        "https:example.test".into(),
        "custom+v1.data-item:value".into(),
        "//example.test/item".into(),
        "id\nother".into(),
    ] {
        assert!(OpaqueNativeId::new(invalid).is_err());
    }
    let native = OpaqueNativeId::new("識別子:一").unwrap();
    assert_eq!(native.as_str(), "識別子:一");
    assert_eq!(LiveInput::lookup(native.clone()).native_id(), Some(&native));
    assert_eq!(LiveInput::query(query.clone()).query_input(), Some(&query));
    assert_eq!(LiveInput::lookup(native).query_input(), None);
}

#[test]
fn input_debug_does_not_disclose_query_facets_or_native_id() {
    let query = RemoteQueryInput::new(
        "private-query-fixture",
        vec![StructuredFacetFilter::eq(
            "private-facet-fixture",
            TypedValue::String("private-value-fixture".into()),
        )],
        1,
        &["private-facet-fixture"],
    )
    .unwrap();
    let native = OpaqueNativeId::new("private-native-fixture").unwrap();
    for debug in [
        format!("{query:?}"),
        format!("{native:?}"),
        format!("{:?}", LiveInput::query(query)),
        format!("{:?}", LiveInput::lookup(native)),
    ] {
        assert!(!debug.contains("private-"));
    }
}
