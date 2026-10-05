//! P5-05: one current visible durable Resource, or the same 404.

#[path = "support/api.rs"]
mod api;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use api::*;
use search_application::SearchError;
use search_application::api_scope::{
    ApiError, Cancellation, SearchOperationContext, prepare_api_visible_sources,
};
use search_application::ports::BoxFuture;
use search_application::remote_disclosure::{
    CurrentDisclosureAccessPort, DisclosedFields, DisclosureOwner,
};
use search_application::remote_lease::SystemLeaseClock;
use search_application::resource_read::{
    CurrentResourceReadPort, ResourceCoverage, ResourceLocatorPort, ResourceReadService,
    ResourceSnapshot, VisibleResourceBinding,
};
use search_application::scoped::{AuthorizedSourceScope, TrustedSearchScope};
use search_application::source_registration::TrustedVisibleRegistry;
use search_core::id::{ResourceId, SourceId};
use search_core::resource::ResourceKind;
use uuid::Uuid;

fn rid(value: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(value))
}

struct Open;
impl CurrentDisclosureAccessPort for Open {
    fn authorize<'a>(
        &'a self,
        _: &'a DisclosureOwner,
        _: &'a DisclosedFields,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

/// Source-owned current durable Resources, with scripted state changes.
#[derive(Default)]
struct Store {
    current: BTreeMap<ResourceId, Vec<SourceId>>,
    history: BTreeSet<ResourceId>,
    terminated: Mutex<BTreeSet<ResourceId>>,
    revoked: Mutex<BTreeSet<ResourceId>>,
    fail_locate: bool,
    fail_read: bool,
    cancel_on_read: Mutex<Option<Cancellation>>,
    scopes_seen: Mutex<Vec<SourceId>>,
}

impl ResourceLocatorPort for Store {
    fn resolve_visible<'a>(
        &'a self,
        _: &'a TrustedSearchScope,
        visible: &'a [AuthorizedSourceScope],
        resource_id: ResourceId,
    ) -> BoxFuture<'a, Vec<VisibleResourceBinding>> {
        Box::pin(async move {
            if self.fail_locate {
                return Err(SearchError::SourceUnavailable("target storage".into()));
            }
            self.scopes_seen
                .lock()
                .unwrap()
                .extend(visible.iter().map(AuthorizedSourceScope::source_id));
            let holders = self.current.get(&resource_id).cloned().unwrap_or_default();
            Ok(visible
                .iter()
                .filter(|scope| holders.contains(&scope.source_id()))
                .map(|scope| {
                    VisibleResourceBinding::new(
                        scope.clone(),
                        resource_id,
                        format!("private://{resource_id:?}"),
                    )
                })
                .collect())
        })
    }
}

impl CurrentResourceReadPort for Store {
    fn read_current<'a>(
        &'a self,
        _: &'a TrustedSearchScope,
        binding: &'a VisibleResourceBinding,
    ) -> BoxFuture<'a, Option<ResourceSnapshot>> {
        Box::pin(async move {
            if let Some(cancel) = self.cancel_on_read.lock().unwrap().as_ref() {
                cancel.cancel();
            }
            if self.fail_read {
                return Err(SearchError::SourceUnavailable("part read".into()));
            }
            let id = binding.resource_id();
            if self.terminated.lock().unwrap().contains(&id)
                || self.revoked.lock().unwrap().contains(&id)
            {
                return Ok(None);
            }
            Ok(Some(ResourceSnapshot {
                resource_id: id,
                source_id: binding.scope().source_id(),
                resource_type: ResourceKind::Knowledge,
                resource_version: None,
                title: Some("規程".into()),
                coverage: ResourceCoverage::BodyUnknown,
            }))
        })
    }
}

fn store(world: &ApiWorld) -> Store {
    Store {
        current: BTreeMap::from([
            (rid(1), vec![world.document]),
            (rid(2), vec![world.foreign_document]),
            (rid(3), vec![world.hidden]),
            (rid(4), vec![world.document, world.second]),
            (rid(5), vec![world.document]),
            (rid(6), vec![world.document]),
        ]),
        history: BTreeSet::from([rid(9)]),
        ..Store::default()
    }
}

async fn read(
    world: &ApiWorld,
    store: &Store,
    id: ResourceId,
) -> Result<ResourceSnapshot, ApiError> {
    let visibility = world.visibility();
    let handle = world
        .actor(
            "tenant-a",
            "reader",
            &visibility,
            &[world.document, world.second],
        )
        .await;
    let registry = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let context = SearchOperationContext::authenticate(
        &world.authority,
        &handle,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    // Arm the scripted cancellation with this request's own handle.
    let armed = store.cancel_on_read.lock().unwrap().is_some();
    if armed {
        *store.cancel_on_read.lock().unwrap() = Some(context.cancellation().clone());
    }
    let snapshot = prepare_api_visible_sources(&world.authority, &registry, &context)
        .await
        .unwrap();
    let service = ResourceReadService::new(
        store,
        store,
        Arc::new(SystemLeaseClock),
        Duration::from_secs(30),
    );
    let mut disclosure = service.read(&context, &snapshot, id).await?;
    let mut detail = None;
    disclosure
        .disclose_with(&Open, |view| {
            detail = Some(view.snapshot().clone());
            Ok(())
        })
        .await
        .unwrap();
    Ok(detail.unwrap())
}

#[tokio::test]
async fn unknown_foreign_hidden_old_t10_revoked_and_duplicate_locator_identical_404() {
    let world = ApiWorld::new().await;
    let store = store(&world);
    // Positive control.
    let found = read(&world, &store, rid(1)).await.unwrap();
    assert_eq!(
        (found.resource_id, found.source_id),
        (rid(1), world.document)
    );
    store.terminated.lock().unwrap().insert(rid(5));
    store.revoked.lock().unwrap().insert(rid(6));
    for id in [
        rid(1_000), // unknown
        rid(2),     // another tenant's Source
        rid(3),     // a Source this actor cannot see
        rid(9),     // an old Version
        rid(5),     // publication ended (T10)
        rid(6),     // Read revoked
        rid(4),     // located in two visible Sources
    ] {
        assert_eq!(
            read(&world, &store, id).await.unwrap_err(),
            ApiError::ResourceNotFound
        );
    }
}

#[tokio::test]
async fn previsibility_target_error_is_404() {
    let world = ApiWorld::new().await;
    let locate = Store {
        fail_locate: true,
        ..store(&world)
    };
    assert_eq!(
        read(&world, &locate, rid(1)).await.unwrap_err(),
        ApiError::ResourceNotFound
    );
    let read_failure = Store {
        fail_read: true,
        ..store(&world)
    };
    assert_eq!(
        read(&world, &read_failure, rid(1)).await.unwrap_err(),
        ApiError::ResourceNotFound
    );
}

#[tokio::test]
async fn no_native_ephemeral_or_history_fallback() {
    let world = ApiWorld::new().await;
    let store = store(&world);
    // History is never a fallback for the current Resource.
    assert!(store.history.contains(&rid(9)));
    assert_eq!(
        read(&world, &store, rid(9)).await.unwrap_err(),
        ApiError::ResourceNotFound
    );
    // The locator only ever saw the actor's visible scopes: no global lookup.
    let seen: BTreeSet<_> = store.scopes_seen.lock().unwrap().iter().copied().collect();
    assert_eq!(seen, BTreeSet::from([world.document, world.second]));
}

#[tokio::test]
async fn field_cancel_drops_detail() {
    let world = ApiWorld::new().await;
    let store = store(&world);
    *store.cancel_on_read.lock().unwrap() = Some(Cancellation::default());
    assert_eq!(
        read(&world, &store, rid(1)).await.unwrap_err(),
        ApiError::ServiceUnavailable
    );
}
