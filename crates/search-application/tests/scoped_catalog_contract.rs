use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use search_application::ports::{AccessDecision, BoxFuture};
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig, SyntheticRegistrationLedger, TrustedVisibleRegistry,
};
use search_application::scoped::{
    AccessContextAuthorityPort, AccessRevision, CheckedAuthorityAdapter,
    CheckedSourceVisibilityAdapter, CurrentSourceVisibilityPort, PrincipalRef,
    RegistrationRevision, ScopedSourceRegistryPort, SyntheticAuthorityAdapter,
    SyntheticVisibilityAdapter, TenantId, VerifiedActorDescriptor, VerifiedActorResolverPort,
    VerifiedSourceGrant, VerifiedSourceVisibilityPort, VisibilityRevision, prepare_visible_sources,
};
use search_application::search_core::id::{DiscoveryEvaluationId, SourceId};
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use uuid::Uuid;

#[path = "support/source_catalog.rs"]
mod source_catalog;
use source_catalog::RemoteCatalogFixture;

fn tenant(value: &str) -> TenantId {
    TenantId::new(value).unwrap()
}

struct HostIdentityResolver {
    issued_at: Instant,
    deadline: Instant,
    alice_handle: String,
    bob_handle: String,
}

impl VerifiedActorResolverPort for HostIdentityResolver {
    fn resolve_verified<'a>(
        &'a self,
        raw_handle: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedActorDescriptor>> {
        Box::pin(async move {
            let identity = if raw_handle == self.alice_handle {
                Some(("tenant-a", "alice"))
            } else if raw_handle == self.bob_handle {
                Some(("tenant-b", "bob"))
            } else {
                None
            };
            Ok(identity.map(|(tenant_id, principal_id)| {
                VerifiedActorDescriptor::new(
                    tenant(tenant_id),
                    principal(principal_id),
                    None,
                    AccessRevision::new(1).unwrap(),
                    self.issued_at,
                    self.deadline,
                )
                .unwrap()
            }))
        })
    }
}

struct HostVisibilityResolver {
    visible_source: SourceId,
}

impl VerifiedSourceVisibilityPort for HostVisibilityResolver {
    fn grant_for<'a>(
        &'a self,
        actor: &'a search_application::scoped::TrustedSearchScope,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<VerifiedSourceGrant>> {
        Box::pin(async move {
            Ok((actor.tenant() == &tenant("tenant-a")
                && actor.principal() == &principal("alice")
                && source_id == self.visible_source)
                .then(|| {
                    VerifiedSourceGrant::new(
                        tenant("tenant-a"),
                        source_id,
                        RegistrationRevision::new(1).unwrap(),
                        VisibilityRevision::new(1).unwrap(),
                    )
                }))
        })
    }
}

#[tokio::test]
async fn host_verified_adapters_issue_scopes_but_foreign_handle_never_reaches_registry() {
    let issued_at = Instant::now();
    let alice_handle = Uuid::now_v7().to_string();
    let bob_handle = Uuid::now_v7().to_string();
    let host_identity = HostIdentityResolver {
        issued_at,
        deadline: issued_at + Duration::from_secs(60),
        alice_handle: alice_handle.clone(),
        bob_handle: bob_handle.clone(),
    };
    let authority = CheckedAuthorityAdapter::new(&host_identity);
    let actor = authority
        .authenticate_handle(&alice_handle)
        .await
        .unwrap()
        .unwrap();
    let foreign = authority
        .authenticate_handle(&bob_handle)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(actor.tenant(), foreign.tenant());
    assert!(
        authority
            .authenticate_handle("caller-chosen")
            .await
            .unwrap()
            .is_none()
    );

    let evaluation = DiscoveryEvaluationId::from_uuid(Uuid::from_u128(401));
    let binding = authority
        .bind_discovery(&actor, evaluation)
        .await
        .unwrap()
        .unwrap();
    let id = source(402);
    let catalog = RemoteCatalogFixture::start_complete(vec![registration(tenant("tenant-a"), id)])
        .await
        .unwrap();
    let host_visibility = HostVisibilityResolver { visible_source: id };
    let visibility = CheckedSourceVisibilityAdapter::new(&host_visibility, &catalog);
    let registry = TrustedVisibleRegistry::new(&authority, &visibility, &catalog);

    let valid = prepare_visible_sources(&authority, &registry, &binding, &alice_handle, evaluation)
        .await
        .unwrap();
    assert_eq!(valid.len(), 1);
    assert_eq!(valid[0].scope().actor(), &actor);
    assert_eq!(valid[0].scope().source_id(), id);

    let counting = CountingRegistry {
        calls: AtomicUsize::new(0),
    };
    let rejected =
        prepare_visible_sources(&authority, &counting, &binding, &bob_handle, evaluation)
            .await
            .unwrap_err();
    assert_eq!(
        rejected.to_string(),
        "invalid search request: trusted scope unavailable"
    );
    assert_eq!(counting.calls.load(Ordering::SeqCst), 0);
}

fn principal(value: &str) -> PrincipalRef {
    PrincipalRef::new(value).unwrap()
}

fn source(value: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(value))
}

fn registration(tenant_id: TenantId, source_id: SourceId) -> RemoteSourceRegistration {
    registration_at(tenant_id, source_id, 1, 1)
}

fn registration_at(
    tenant_id: TenantId,
    source_id: SourceId,
    registration_revision: u64,
    visibility_revision: u64,
) -> RemoteSourceRegistration {
    RemoteSourceRegistration::from_server_config(ServerRemoteRegistrationConfig {
        tenant: tenant_id,
        source_id,
        provider_kind: "synthetic-catalog".into(),
        endpoint: RegisteredEndpoint::new("https", "catalog.example.test", 443, "/v1").unwrap(),
        supported_modes: vec![DiscoveryMode::RemoteQuery],
        enumeration_semantics: EnumerationSemantics::QueryOnly,
        authority_predicates: vec!["policy".into()],
        allowed_resource_kinds: vec![ResourceKind::Knowledge],
        current_access_contract: CurrentAccessContract::PerItem,
        retention_mode: RetentionMode::SessionOnly,
        freshness_policy: Some("current-at-read".into()),
        canonical_upstream_lineage: "synthetic-upstream".into(),
        limits: RemoteRegistrationLimits::synthetic_canary(),
        registration_revision: RegistrationRevision::new(registration_revision).unwrap(),
        visibility_revision: VisibilityRevision::new(visibility_revision).unwrap(),
    })
    .unwrap()
}

#[tokio::test]
async fn registration_revision_can_advance_without_visibility_change() {
    let id = source(104);
    let original = registration_at(tenant("tenant-a"), id, 1, 1);
    let revised = registration_at(tenant("tenant-a"), id, 2, 1);
    let catalog = RemoteCatalogFixture::start_complete(vec![original])
        .await
        .unwrap();

    catalog.publish_complete(vec![revised]).await.unwrap();
    assert_eq!(
        catalog
            .get_for_server(id)
            .unwrap()
            .registration_revision()
            .get(),
        2
    );
    assert_eq!(
        catalog
            .get_for_server(id)
            .unwrap()
            .visibility_revision()
            .get(),
        1
    );
}

#[test]
fn registered_endpoint_rejects_encoded_path_escape() {
    assert!(
        RegisteredEndpoint::new("https", "catalog.example.test", 443, "/v1/%2e%2e/admin").is_err()
    );
}

#[tokio::test]
async fn revoked_source_scope_cannot_be_reactivated_by_revision_rollback() {
    let authority = SyntheticAuthorityAdapter::new();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal("alice"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let id = source(105);
    let catalog = RemoteCatalogFixture::start_complete(vec![registration(tenant("tenant-a"), id)])
        .await
        .unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    let registration_revision = RegistrationRevision::new(1).unwrap();
    let old_revision = VisibilityRevision::new(1).unwrap();
    visibility
        .grant(
            tenant("tenant-a"),
            principal("alice"),
            id,
            registration_revision,
            old_revision,
        )
        .unwrap();
    let old_scope = visibility.bind_source(&actor, id).await.unwrap().unwrap();
    assert_eq!(
        visibility.current(&old_scope).await.unwrap(),
        AccessDecision::Allowed
    );

    visibility.revoke(&principal("alice"), id).unwrap();
    assert_eq!(
        visibility.current(&old_scope).await.unwrap(),
        AccessDecision::Denied
    );
    assert!(
        visibility
            .grant(
                tenant("tenant-a"),
                principal("alice"),
                id,
                registration_revision,
                old_revision
            )
            .is_err()
    );
    assert_eq!(
        visibility.current(&old_scope).await.unwrap(),
        AccessDecision::Denied
    );
}

#[tokio::test]
async fn cross_tenant_source_id_collision_rejected_on_start_and_update() {
    let id = source(101);
    let a = registration(tenant("tenant-a"), id);
    let b = registration(tenant("tenant-b"), id);

    assert!(
        RemoteCatalogFixture::start_complete(vec![a.clone(), b.clone()])
            .await
            .is_err()
    );

    let catalog = RemoteCatalogFixture::start_complete(vec![a.clone()])
        .await
        .unwrap();
    assert!(catalog.publish_complete(vec![b]).await.is_err());
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog.get_for_server(id).unwrap().tenant(), a.tenant());
    assert!(catalog.publish_complete(vec![a.clone(), a]).await.is_err());
    assert_eq!(catalog.len(), 1);
}

#[tokio::test]
async fn shared_ledger_rejects_owner_change_after_catalog_recreation() {
    let id = source(501);
    let first = registration(tenant("tenant-a"), id);
    let second = registration(tenant("tenant-b"), id);
    let ledger = Arc::new(SyntheticRegistrationLedger::new());
    let catalog = RemoteCatalogFixture::start_complete_with_ledger(ledger.clone(), vec![first])
        .await
        .unwrap();
    catalog.publish_complete(vec![]).await.unwrap();
    drop(catalog);

    assert!(
        RemoteCatalogFixture::start_complete_with_ledger(ledger, vec![second])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn registration_removal_invalidates_issued_scope_even_with_grant_unchanged() {
    let authority = SyntheticAuthorityAdapter::new();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal("alice"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let id = source(502);
    let catalog = RemoteCatalogFixture::start_complete(vec![registration(tenant("tenant-a"), id)])
        .await
        .unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    visibility
        .grant(
            tenant("tenant-a"),
            principal("alice"),
            id,
            RegistrationRevision::new(1).unwrap(),
            VisibilityRevision::new(1).unwrap(),
        )
        .unwrap();
    let old_scope = visibility.bind_source(&actor, id).await.unwrap().unwrap();
    assert_eq!(
        visibility.current(&old_scope).await.unwrap(),
        AccessDecision::Allowed
    );

    catalog.publish_complete(vec![]).await.unwrap();
    assert_eq!(
        visibility.current(&old_scope).await.unwrap(),
        AccessDecision::Denied
    );
    assert!(
        catalog
            .publish_complete(vec![registration(tenant("tenant-a"), id)])
            .await
            .is_err()
    );
    assert_eq!(
        visibility.current(&old_scope).await.unwrap(),
        AccessDecision::Denied
    );

    catalog
        .publish_complete(vec![registration_at(tenant("tenant-a"), id, 2, 1)])
        .await
        .unwrap();
    assert!(visibility.bind_source(&actor, id).await.unwrap().is_none());
    visibility
        .grant(
            tenant("tenant-a"),
            principal("alice"),
            id,
            RegistrationRevision::new(2).unwrap(),
            VisibilityRevision::new(1).unwrap(),
        )
        .unwrap();
    let new_scope = visibility.bind_source(&actor, id).await.unwrap().unwrap();
    assert_ne!(
        old_scope.registration_activation(),
        new_scope.registration_activation()
    );
    assert_eq!(
        visibility.current(&old_scope).await.unwrap(),
        AccessDecision::Denied
    );
    assert_eq!(
        visibility.current(&new_scope).await.unwrap(),
        AccessDecision::Allowed
    );
}

struct RevokeDuringCurrent<'a, 'b> {
    inner: &'a SyntheticVisibilityAdapter<'b>,
    revoked_source: SourceId,
}

impl CurrentSourceVisibilityPort for RevokeDuringCurrent<'_, '_> {
    fn bind_source<'a>(
        &'a self,
        actor: &'a search_application::scoped::TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<search_application::scoped::AuthorizedSourceScope>> {
        self.inner.bind_source(actor, source)
    }

    fn current<'a>(
        &'a self,
        scope: &'a search_application::scoped::AuthorizedSourceScope,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if scope.source_id() == self.revoked_source {
                self.inner
                    .revoke(scope.actor().principal(), self.revoked_source)?;
            }
            self.inner.current(scope).await
        })
    }
}

#[tokio::test]
async fn one_source_revoked_between_bind_and_current_does_not_hide_other_source() {
    let authority = SyntheticAuthorityAdapter::new();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal("alice"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let id_a = source(503);
    let id_b = source(504);
    let catalog = RemoteCatalogFixture::start_complete(vec![
        registration(tenant("tenant-a"), id_a),
        registration(tenant("tenant-a"), id_b),
    ])
    .await
    .unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    for id in [id_a, id_b] {
        visibility
            .grant(
                tenant("tenant-a"),
                principal("alice"),
                id,
                RegistrationRevision::new(1).unwrap(),
                VisibilityRevision::new(1).unwrap(),
            )
            .unwrap();
    }
    let race = RevokeDuringCurrent {
        inner: &visibility,
        revoked_source: id_a,
    };
    let registry = TrustedVisibleRegistry::new(&authority, &race, &catalog);
    let visible = registry.visible_sources(&actor).await.unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].scope().source_id(), id_b);
}

struct AdvanceRegistrationDuringBind<'a, 'b> {
    inner: &'a SyntheticVisibilityAdapter<'b>,
    catalog: &'b RemoteCatalogFixture,
    advanced_source: SourceId,
    other_source: SourceId,
}

impl CurrentSourceVisibilityPort for AdvanceRegistrationDuringBind<'_, '_> {
    fn bind_source<'a>(
        &'a self,
        actor: &'a search_application::scoped::TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<search_application::scoped::AuthorizedSourceScope>> {
        Box::pin(async move {
            if source == self.advanced_source {
                self.catalog
                    .publish_complete(vec![
                        registration_at(tenant("tenant-a"), source, 2, 2),
                        registration(tenant("tenant-a"), self.other_source),
                    ])
                    .await?;
                self.inner.grant(
                    tenant("tenant-a"),
                    principal("alice"),
                    source,
                    RegistrationRevision::new(2).unwrap(),
                    VisibilityRevision::new(2).unwrap(),
                )?;
            }
            self.inner.bind_source(actor, source).await
        })
    }

    fn current<'a>(
        &'a self,
        scope: &'a search_application::scoped::AuthorizedSourceScope,
    ) -> BoxFuture<'a, AccessDecision> {
        self.inner.current(scope)
    }
}

#[tokio::test]
async fn catalog_revision_change_during_bind_excludes_only_changed_source() {
    let authority = SyntheticAuthorityAdapter::new();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal("alice"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let id_a = source(505);
    let id_b = source(506);
    let catalog = RemoteCatalogFixture::start_complete(vec![
        registration(tenant("tenant-a"), id_a),
        registration(tenant("tenant-a"), id_b),
    ])
    .await
    .unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    for id in [id_a, id_b] {
        visibility
            .grant(
                tenant("tenant-a"),
                principal("alice"),
                id,
                RegistrationRevision::new(1).unwrap(),
                VisibilityRevision::new(1).unwrap(),
            )
            .unwrap();
    }
    let race = AdvanceRegistrationDuringBind {
        inner: &visibility,
        catalog: &catalog,
        advanced_source: id_a,
        other_source: id_b,
    };
    let registry = TrustedVisibleRegistry::new(&authority, &race, &catalog);
    let visible = registry.visible_sources(&actor).await.unwrap();

    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].scope().source_id(), id_b);
    assert_eq!(visible[0].registration().registration_revision().get(), 1);
    assert_eq!(
        catalog
            .get_for_server(id_a)
            .unwrap()
            .registration_revision()
            .get(),
        2
    );
    let new_scope = visibility.bind_source(&actor, id_a).await.unwrap().unwrap();
    assert_eq!(new_scope.registration_revision().get(), 2);
    assert_eq!(
        visibility.current(&new_scope).await.unwrap(),
        AccessDecision::Allowed
    );
}

struct ReplayScope {
    scope: search_application::scoped::AuthorizedSourceScope,
}

impl CurrentSourceVisibilityPort for ReplayScope {
    fn bind_source<'a>(
        &'a self,
        _actor: &'a search_application::scoped::TrustedSearchScope,
        _source: SourceId,
    ) -> BoxFuture<'a, Option<search_application::scoped::AuthorizedSourceScope>> {
        Box::pin(async move { Ok(Some(self.scope.clone())) })
    }

    fn current<'a>(
        &'a self,
        _scope: &'a search_application::scoped::AuthorizedSourceScope,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async { Ok(AccessDecision::Allowed) })
    }
}

#[tokio::test]
async fn structural_actor_or_source_mismatch_remains_an_error() {
    let authority = SyntheticAuthorityAdapter::new();
    let alice_handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal("alice"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let bob_handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal("bob"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let alice = authority.resolve(&alice_handle).await.unwrap().unwrap();
    let bob = authority.resolve(&bob_handle).await.unwrap().unwrap();
    let id_a = source(507);
    let id_b = source(508);
    let catalog = RemoteCatalogFixture::start_complete(vec![
        registration(tenant("tenant-a"), id_a),
        registration(tenant("tenant-a"), id_b),
    ])
    .await
    .unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    for (principal_name, id) in [("alice", id_b), ("bob", id_a)] {
        visibility
            .grant(
                tenant("tenant-a"),
                principal(principal_name),
                id,
                RegistrationRevision::new(1).unwrap(),
                VisibilityRevision::new(1).unwrap(),
            )
            .unwrap();
    }
    let wrong_source = ReplayScope {
        scope: visibility.bind_source(&alice, id_b).await.unwrap().unwrap(),
    };
    let wrong_actor = ReplayScope {
        scope: visibility.bind_source(&bob, id_a).await.unwrap().unwrap(),
    };

    let mut failures = Vec::new();
    for mismatch in [&wrong_source, &wrong_actor] {
        let registry = TrustedVisibleRegistry::new(&authority, mismatch, &catalog);
        let error = registry.visible_sources(&alice).await.unwrap_err();
        assert!(matches!(
            &error,
            search_application::SearchError::OperationFailed(_)
        ));
        failures.push(error.to_string());
    }
    assert_eq!(failures[0], failures[1]);
    assert_eq!(
        failures[0],
        "search operation failed: trusted scope unavailable"
    );
    assert!(!failures[0].contains(id_a.as_uuid().to_string().as_str()));
    assert!(!failures[0].contains(id_b.as_uuid().to_string().as_str()));
}

struct FailDuringCurrent<'a, 'b> {
    inner: &'a SyntheticVisibilityAdapter<'b>,
    failed_source: SourceId,
}

impl CurrentSourceVisibilityPort for FailDuringCurrent<'_, '_> {
    fn bind_source<'a>(
        &'a self,
        actor: &'a search_application::scoped::TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<search_application::scoped::AuthorizedSourceScope>> {
        self.inner.bind_source(actor, source)
    }

    fn current<'a>(
        &'a self,
        scope: &'a search_application::scoped::AuthorizedSourceScope,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if scope.source_id() == self.failed_source {
                return Err(search_application::SearchError::OperationFailed(
                    "synthetic visibility unavailable".into(),
                ));
            }
            self.inner.current(scope).await
        })
    }
}

#[tokio::test]
async fn visibility_infrastructure_error_still_fails_entire_catalog() {
    let authority = SyntheticAuthorityAdapter::new();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal("alice"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let id_a = source(509);
    let id_b = source(510);
    let catalog = RemoteCatalogFixture::start_complete(vec![
        registration(tenant("tenant-a"), id_a),
        registration(tenant("tenant-a"), id_b),
    ])
    .await
    .unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    for id in [id_a, id_b] {
        visibility
            .grant(
                tenant("tenant-a"),
                principal("alice"),
                id,
                RegistrationRevision::new(1).unwrap(),
                VisibilityRevision::new(1).unwrap(),
            )
            .unwrap();
    }
    let failing = FailDuringCurrent {
        inner: &visibility,
        failed_source: id_a,
    };
    let registry = TrustedVisibleRegistry::new(&authority, &failing, &catalog);

    let error = registry.visible_sources(&actor).await.unwrap_err();
    assert!(matches!(
        error,
        search_application::SearchError::OperationFailed(_)
    ));
}

struct CountingRegistry {
    calls: AtomicUsize,
}

impl ScopedSourceRegistryPort for CountingRegistry {
    fn visible_sources<'a>(
        &'a self,
        _actor: &'a search_application::scoped::TrustedSearchScope,
    ) -> BoxFuture<'a, search_application::scoped::VisibleCatalogSnapshot> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(search_application::scoped::VisibleCatalogSnapshot::unstamped(Vec::new()))
        })
    }
}

#[tokio::test]
async fn foreign_handle_and_expired_revision_rejected_before_registry() {
    let authority = SyntheticAuthorityAdapter::new();
    let handle_a = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal("alice"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let handle_b = authority
        .issue_verified_identity(
            tenant("tenant-b"),
            principal("bob"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor_a = authority.resolve(&handle_a).await.unwrap().unwrap();
    let evaluation = DiscoveryEvaluationId::from_uuid(Uuid::from_u128(201));
    let binding = authority
        .bind_discovery(&actor_a, evaluation)
        .await
        .unwrap()
        .unwrap();
    let registry = CountingRegistry {
        calls: AtomicUsize::new(0),
    };

    let foreign = prepare_visible_sources(
        &authority,
        &registry,
        &binding,
        &handle_b.to_opaque_string(),
        evaluation,
    )
    .await
    .unwrap_err();
    authority
        .replace_access_revision(&handle_a, AccessRevision::new(2).unwrap())
        .unwrap();
    let stale = prepare_visible_sources(
        &authority,
        &registry,
        &binding,
        &handle_a.to_opaque_string(),
        evaluation,
    )
    .await
    .unwrap_err();

    assert_eq!(foreign.to_string(), stale.to_string());
    assert_eq!(registry.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn visible_sources_only_return_bound_scopes() {
    let authority = SyntheticAuthorityAdapter::new();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal("alice"),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let evaluation = DiscoveryEvaluationId::from_uuid(Uuid::from_u128(202));
    let binding = authority
        .bind_discovery(&actor, evaluation)
        .await
        .unwrap()
        .unwrap();

    let visible_id = source(301);
    let hidden_id = source(302);
    let foreign_id = source(303);
    let catalog = RemoteCatalogFixture::start_complete(vec![
        registration(tenant("tenant-a"), visible_id),
        registration(tenant("tenant-a"), hidden_id),
        registration(tenant("tenant-b"), foreign_id),
    ])
    .await
    .unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    visibility
        .grant(
            tenant("tenant-a"),
            principal("alice"),
            visible_id,
            RegistrationRevision::new(1).unwrap(),
            VisibilityRevision::new(1).unwrap(),
        )
        .unwrap();
    // A misconfigured visibility adapter cannot expose a registration from another tenant.
    visibility
        .grant(
            tenant("tenant-a"),
            principal("alice"),
            foreign_id,
            RegistrationRevision::new(1).unwrap(),
            VisibilityRevision::new(1).unwrap(),
        )
        .unwrap();
    let registry = TrustedVisibleRegistry::new(&authority, &visibility, &catalog);
    let visible = prepare_visible_sources(
        &authority,
        &registry,
        &binding,
        &handle.to_opaque_string(),
        evaluation,
    )
    .await
    .unwrap();

    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].scope().source_id(), visible_id);
    assert_eq!(visible[0].scope().actor(), &actor);
    assert_eq!(visible[0].registration().tenant(), &tenant("tenant-a"));
    let projected = visible[0].discoverable_source();
    assert_eq!(projected.source_id, visible_id);
    assert!(projected.supports(DiscoveryMode::RemoteQuery));
    assert!(!projected.supports(DiscoveryMode::RemoteEnumeration));
    assert_eq!(projected.retention_mode, RetentionMode::SessionOnly);
    assert!(catalog.get_for_server(hidden_id).is_some());
}
