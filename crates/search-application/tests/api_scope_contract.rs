//! P5-02: one request scope and visible catalog for the four routes.

#[path = "support/api.rs"]
mod api;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use api::*;
use search_application::SearchError;
use search_application::api_scope::{
    ApiError, SearchOperationContext, prepare_api_visible_sources,
};
use search_application::ports::BoxFuture;
use search_application::scoped::{
    ScopedSourceRegistryPort, SyntheticAuthorityAdapter, TrustedSearchScope, VisibleCatalogSnapshot,
};
use search_application::source_registration::{SourceKind, TrustedVisibleRegistry};

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

/// Wraps the trusted registry: counts reads and can corrupt or fail them.
struct Registry<'a> {
    inner: &'a dyn ScopedSourceRegistryPort,
    calls: Mutex<usize>,
    extra: Option<VisibleCatalogSnapshot>,
    fail: bool,
}

impl<'a> Registry<'a> {
    fn new(inner: &'a dyn ScopedSourceRegistryPort) -> Self {
        Self {
            inner,
            calls: Mutex::new(0),
            extra: None,
            fail: false,
        }
    }
    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl ScopedSourceRegistryPort for Registry<'_> {
    fn visible_sources<'b>(
        &'b self,
        actor: &'b TrustedSearchScope,
    ) -> BoxFuture<'b, VisibleCatalogSnapshot> {
        Box::pin(async move {
            *self.calls.lock().unwrap() += 1;
            if self.fail {
                return Err(SearchError::SourceUnavailable("ledger".into()));
            }
            let mut entries = self.inner.visible_sources(actor).await?.entries().to_vec();
            if let Some(extra) = &self.extra {
                entries.extend(extra.entries().iter().cloned());
            }
            Ok(VisibleCatalogSnapshot::unstamped(entries))
        })
    }
}

async fn context(
    world: &ApiWorld,
    handle: &search_application::scoped::AccessContextHandle,
) -> SearchOperationContext {
    SearchOperationContext::authenticate(&world.authority, handle, soon())
        .await
        .unwrap()
}

#[tokio::test]
async fn same_actor_local_remote_complete_snapshot() {
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let handle = world
        .actor(
            "tenant-a",
            "reader",
            &visibility,
            &[world.document, world.remote],
        )
        .await;
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let context = context(&world, &handle).await;
    let snapshot = prepare_api_visible_sources(&world.authority, &trusted, &context)
        .await
        .unwrap();
    let mut kinds: Vec<_> = snapshot
        .entries()
        .iter()
        .map(|entry| (entry.scope().source_id(), entry.registration().kind()))
        .collect();
    kinds.sort();
    let mut expected = vec![
        (world.document, SourceKind::Document),
        (world.remote, SourceKind::Remote),
    ];
    expected.sort();
    assert_eq!(kinds, expected);
    assert!(
        snapshot
            .entries()
            .iter()
            .all(|entry| entry.scope().actor() == context.actor())
    );
}

#[tokio::test]
async fn foreign_tenant_and_duplicate_source_fail_closed() {
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let ours = world
        .actor(
            "tenant-a",
            "reader",
            &visibility,
            &[world.document, world.remote],
        )
        .await;
    let theirs = world
        .actor("tenant-b", "reader", &visibility, &[world.foreign])
        .await;
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let ours = context(&world, &ours).await;
    let theirs = context(&world, &theirs).await;
    let foreign = trusted.visible_sources(theirs.actor()).await.unwrap();
    let duplicate = trusted.visible_sources(ours.actor()).await.unwrap();
    for extra in [foreign, duplicate] {
        let mut registry = Registry::new(&trusted);
        registry.extra = Some(extra);
        // No partial list survives a structural inconsistency.
        assert_eq!(
            prepare_api_visible_sources(&world.authority, &registry, &ours)
                .await
                .unwrap_err(),
            ApiError::DependencyUnavailable
        );
    }
}

#[tokio::test]
async fn individual_denied_unknown_or_revision_race_preserves_other_visible_source() {
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let handle = world
        .actor(
            "tenant-a",
            "reader",
            &visibility,
            &[world.document, world.remote],
        )
        .await;
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let context = context(&world, &handle).await;
    visibility
        .revoke(context.actor().principal(), world.remote)
        .unwrap();
    let snapshot = prepare_api_visible_sources(&world.authority, &trusted, &context)
        .await
        .unwrap();
    assert_eq!(
        snapshot
            .entries()
            .iter()
            .map(|entry| entry.scope().source_id())
            .collect::<Vec<_>>(),
        vec![world.document]
    );
    // Never granted at all (unknown to this actor) is equally absent.
    let other = world.actor("tenant-a", "other", &visibility, &[]).await;
    let other = SearchOperationContext::authenticate(&world.authority, &other, soon())
        .await
        .unwrap();
    assert!(
        prepare_api_visible_sources(&world.authority, &trusted, &other)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn registry_ledger_visibility_error_is_generic_dependency_failure() {
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let handle = world
        .actor("tenant-a", "reader", &visibility, &[world.document])
        .await;
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let mut failing = Registry::new(&trusted);
    failing.fail = true;
    let context = context(&world, &handle).await;
    assert_eq!(
        prepare_api_visible_sources(&world.authority, &failing, &context)
            .await
            .unwrap_err(),
        ApiError::DependencyUnavailable
    );
    // An authority that cannot answer is an identity outage, not a 401.
    struct Down;
    impl search_application::scoped::AccessContextAuthorityPort for Down {
        fn resolve<'a>(
            &'a self,
            _: &'a search_application::scoped::AccessContextHandle,
        ) -> BoxFuture<'a, Option<TrustedSearchScope>> {
            Box::pin(async { Err(SearchError::SourceUnavailable("idp".into())) })
        }
        fn bind_discovery<'a>(
            &'a self,
            _: &'a TrustedSearchScope,
            _: search_core::id::DiscoveryEvaluationId,
        ) -> BoxFuture<'a, Option<search_application::scoped::TrustedDiscoveryBinding>> {
            Box::pin(async { Ok(None) })
        }
        fn current<'a>(
            &'a self,
            _: &'a TrustedSearchScope,
        ) -> BoxFuture<'a, search_application::scoped::AccessBindingState> {
            Box::pin(async { Err(SearchError::SourceUnavailable("idp".into())) })
        }
    }
    assert_eq!(
        SearchOperationContext::authenticate(&Down, &handle, soon())
            .await
            .unwrap_err(),
        ApiError::IdentityUnavailable
    );
    // The same outage after authentication, while preparing Sources.
    assert_eq!(
        prepare_api_visible_sources(&Down, &failing, &context)
            .await
            .unwrap_err(),
        ApiError::IdentityUnavailable
    );
}

#[tokio::test]
async fn actor_revoked_before_route_stops_all_source_io() {
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let handle = world
        .actor(
            "tenant-a",
            "reader",
            &visibility,
            &[world.document, world.remote],
        )
        .await;
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let registry = Registry::new(&trusted);
    let context = context(&world, &handle).await;
    world.authority.revoke(&handle).unwrap();
    assert_eq!(
        prepare_api_visible_sources(&world.authority, &registry, &context)
            .await
            .unwrap_err(),
        ApiError::AuthenticationRequired
    );
    assert_eq!(registry.calls(), 0);
    // A revoked or foreign handle never authenticates.
    assert_eq!(
        SearchOperationContext::authenticate(&world.authority, &handle, soon())
            .await
            .unwrap_err(),
        ApiError::AuthenticationRequired
    );
    let elsewhere = SyntheticAuthorityAdapter::new();
    assert_eq!(
        SearchOperationContext::authenticate(&elsewhere, &handle, soon())
            .await
            .unwrap_err(),
        ApiError::AuthenticationRequired
    );
    // A cancelled or expired operation stops before the registry too.
    let fresh = world
        .actor("tenant-a", "reader2", &visibility, &[world.document])
        .await;
    let late = SearchOperationContext::authenticate(&world.authority, &fresh, Instant::now())
        .await
        .unwrap();
    assert_eq!(
        prepare_api_visible_sources(&world.authority, &registry, &late)
            .await
            .unwrap_err(),
        ApiError::ServiceUnavailable
    );
    assert_eq!(registry.calls(), 0);
}
