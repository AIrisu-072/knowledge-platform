use std::sync::Arc;
use std::time::Duration;

use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig, TrustedVisibleRegistry,
};
use search_application::routing::{RoutingConstraints, SourceRole, SourceRouter};
use search_application::scoped::{
    AccessContextAuthorityPort, AccessRevision, PrincipalRef, RegistrationRevision,
    ScopedSourceRegistryPort, SyntheticAuthorityAdapter, SyntheticVisibilityAdapter, TenantId,
    TrustedSearchScope, VisibilityRevision, VisibleSourceRegistration,
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
use search_core::resource::ResourceKind;
use search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use uuid::Uuid;

fn source(value: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(value))
}

fn registration(tenant: &str, value: u128) -> SourceRegistration {
    SourceRegistration::Remote(
        RemoteSourceRegistration::from_server_config(ServerRemoteRegistrationConfig {
            tenant: TenantId::new(tenant).unwrap(),
            source_id: source(value),
            provider_kind: "synthetic-routing".into(),
            endpoint: RegisteredEndpoint::new("https", "routing.example.test", 443, "/v1").unwrap(),
            supported_modes: vec![DiscoveryMode::RemoteQuery],
            enumeration_semantics: EnumerationSemantics::QueryOnly,
            authority_predicates: vec![],
            allowed_resource_kinds: vec![ResourceKind::Knowledge],
            current_access_contract: CurrentAccessContract::PerItem,
            retention_mode: RetentionMode::SessionOnly,
            freshness_policy: None,
            canonical_upstream_lineage: "synthetic-routing".into(),
            limits: RemoteRegistrationLimits::synthetic_canary(),
            registration_revision: RegistrationRevision::new(1).unwrap(),
            visibility_revision: VisibilityRevision::new(1).unwrap(),
        })
        .unwrap(),
    )
}

async fn visible() -> (TrustedSearchScope, Vec<VisibleSourceRegistration>) {
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    for (namespace, registrations) in [
        (RegistrationNamespace::Document, vec![]),
        (
            RegistrationNamespace::Remote,
            vec![
                registration("tenant-a", 1),
                registration("tenant-a", 2),
                registration("tenant-a", 3),
                registration("tenant-b", 4),
            ],
        ),
    ] {
        host.publish(
            namespace,
            RegistrationSetRevision::new(1).unwrap(),
            registrations,
        )
        .unwrap();
    }
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
    let tenant = TenantId::new("tenant-a").unwrap();
    let principal = PrincipalRef::new("alice").unwrap();
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
    for id in [source(1), source(2)] {
        visibility
            .grant(
                tenant.clone(),
                principal.clone(),
                id,
                RegistrationRevision::new(1).unwrap(),
                VisibilityRevision::new(1).unwrap(),
            )
            .unwrap();
    }
    let registry = TrustedVisibleRegistry::new(&authority, &visibility, &catalog);
    let visible = registry.visible_sources(&actor).await.unwrap();
    (actor, visible.entries().to_vec())
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

#[tokio::test]
async fn hidden_required_is_indistinguishable_from_missing() {
    let (actor, visible) = visible().await;
    let output = |ids| {
        VisibleRouting::prepare(
            &actor,
            &visible,
            &RoutingConstraints {
                required_source_ids: ids,
                ..RoutingConstraints::default()
            },
        )
        .unwrap()
    };
    let hidden = output(vec![source(3)]);
    assert_eq!(hidden, output(vec![source(4)]));
    assert_eq!(hidden, output(vec![source(999)]));
    assert_eq!(
        hidden,
        output(vec![source(3), source(4), source(999), source(3)])
    );
    assert!(hidden.1.required_source_ids.is_empty());
    assert_eq!(hidden.2.len(), 1);
    assert_eq!(hidden.2[0].required_fact, "required_source_unavailable");
    assert_eq!(hidden.2[0].reason, GapReason::Availability);
    assert!(hidden.2[0].blocking);
    let plan = SourceRouter::plan(&need(), &hidden.0, &hidden.1);
    assert!(
        plan.routes
            .iter()
            .all(|route| [source(1), source(2)].contains(&route.source_id))
    );
}

#[tokio::test]
async fn foreign_preferred_never_appears_in_route_or_trace() {
    let (actor, visible) = visible().await;
    let requested = RoutingConstraints {
        preferred_source_ids: vec![source(4), source(2), source(3), source(999)],
        max_initial_optional_sources: 2,
        ..RoutingConstraints::default()
    };
    let (sources, safe, gaps) = VisibleRouting::prepare(&actor, &visible, &requested).unwrap();
    assert_eq!(safe.preferred_source_ids, [source(2)]);
    assert!(gaps.is_empty());
    let expected = RoutingConstraints {
        preferred_source_ids: vec![source(2)],
        max_initial_optional_sources: 2,
        ..RoutingConstraints::default()
    };
    assert_eq!(
        SourceRouter::plan(&need(), &sources, &safe),
        SourceRouter::plan(&need(), &sources, &expected)
    );
}

#[tokio::test]
async fn duplicate_registration_never_routes() {
    let (actor, mut visible) = visible().await;
    visible.push(visible[0].clone());
    let error =
        VisibleRouting::prepare(&actor, &visible, &RoutingConstraints::default()).unwrap_err();
    assert!(matches!(
        error,
        search_application::SearchError::OperationFailed(_)
    ));
    assert_eq!(
        error.to_string(),
        "search operation failed: visible routing unavailable"
    );
}

#[tokio::test]
async fn actor_binding_mismatch_rejects_complete_input() {
    let (actor, _) = visible().await;
    let (_, other_session_visible) = visible().await;
    let error = VisibleRouting::prepare(
        &actor,
        &other_session_visible,
        &RoutingConstraints::default(),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "search operation failed: visible routing unavailable"
    );
}

#[tokio::test]
async fn visible_order_roles_and_optional_budget_remain_unchanged() {
    let (actor, visible) = visible().await;
    let requested = RoutingConstraints {
        required_source_ids: vec![source(2)],
        preferred_source_ids: vec![source(1)],
        max_initial_optional_sources: 1,
    };
    let (sources, safe, gaps) = VisibleRouting::prepare(&actor, &visible, &requested).unwrap();
    assert_eq!(safe, requested);
    assert!(gaps.is_empty());
    assert_eq!(
        sources,
        visible
            .iter()
            .map(VisibleSourceRegistration::discoverable_source)
            .collect::<Vec<_>>()
    );
    let plan = SourceRouter::plan(&need(), &sources, &safe);
    assert_eq!(plan, SourceRouter::plan(&need(), &sources, &requested));
    assert_eq!(plan.routes[0].source_id, source(2));
    assert_eq!(plan.routes[0].role, SourceRole::Required);
}
