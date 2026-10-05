//! P5-05: the visible Source page.

#[path = "support/api.rs"]
mod api;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use api::*;
use search_application::SearchError;
use search_application::api_cursor::{CursorHandle, PublicCursorStore};
use search_application::api_scope::{
    ApiError, SearchOperationContext, prepare_api_visible_sources,
};
use search_application::ports::BoxFuture;
use search_application::remote_disclosure::{
    CurrentDisclosureAccessPort, DisclosedFields, DisclosureOwner,
};
use search_application::remote_lease::SystemLeaseClock;
use search_application::scoped::{
    AccessContextAuthorityPort, AccessContextHandle, ScopedSourceRegistryPort, TrustedSearchScope,
    VisibleCatalogSnapshot, VisibleSetStamp,
};
use search_application::source_browse::{SourceBrowseService, SourceCoverageKind, SourceItemView};
use search_application::source_registration::{SourceKind, TrustedVisibleRegistry};

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

/// A registry with an authoritative visible-set epoch, or a failing one.
struct Registry<'a> {
    inner: &'a dyn ScopedSourceRegistryPort,
    stamped: bool,
    fail: Mutex<bool>,
}
impl ScopedSourceRegistryPort for Registry<'_> {
    fn visible_sources<'b>(
        &'b self,
        actor: &'b TrustedSearchScope,
    ) -> BoxFuture<'b, VisibleCatalogSnapshot> {
        Box::pin(async move {
            if *self.fail.lock().unwrap() {
                return Err(SearchError::SourceUnavailable("ledger".into()));
            }
            let entries = self.inner.visible_sources(actor).await?.entries().to_vec();
            Ok(if self.stamped {
                let stamp = VisibleSetStamp::of(actor, &entries);
                VisibleCatalogSnapshot::stamped(entries, stamp)
            } else {
                VisibleCatalogSnapshot::unstamped(entries)
            })
        })
    }
}

async fn page(
    world: &ApiWorld,
    registry: &dyn ScopedSourceRegistryPort,
    cursors: &PublicCursorStore,
    handle: &AccessContextHandle,
    page_size: usize,
    cursor: Option<CursorHandle>,
    max_bytes: usize,
) -> Result<(Vec<SourceItemView>, Option<CursorHandle>), ApiError> {
    let context = SearchOperationContext::authenticate(
        &world.authority,
        handle,
        Instant::now() + Duration::from_secs(5),
    )
    .await?;
    let snapshot = prepare_api_visible_sources(&world.authority, registry, &context).await?;
    let service = SourceBrowseService::new(
        cursors,
        Arc::new(SystemLeaseClock),
        Duration::from_secs(30),
        max_bytes,
    );
    let mut disclosure = service.page(&context, &snapshot, page_size, cursor).await?;
    let mut out = None;
    disclosure
        .disclose_with(&Open, |view| {
            out = Some((view.items().to_vec(), view.next_cursor()));
            Ok(())
        })
        .await
        .unwrap();
    Ok(out.unwrap())
}

const MIB: usize = 1024 * 1024;

#[tokio::test]
async fn complete_union_visible_page_and_safe_capabilities() {
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
    let registry = Registry {
        inner: &trusted,
        stamped: true,
        fail: Mutex::new(false),
    };
    let cursors = PublicCursorStore::new(Arc::new(SystemLeaseClock));
    let (items, next) = page(&world, &registry, &cursors, &handle, 10, None, MIB)
        .await
        .unwrap();
    assert!(next.is_none());
    let kinds: Vec<_> = items
        .iter()
        .map(|item| (item.source_id, item.source_type))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (world.document, SourceKind::Document),
            (world.remote, SourceKind::Remote)
        ]
    );
    let document = &items[0];
    assert!(
        document
            .coverage
            .contains(&SourceCoverageKind::BodySearchWithPerItemCoverage)
    );
    assert_eq!(
        items[1].coverage,
        vec![SourceCoverageKind::TitleAndPermittedMetadata]
    );
    // Nothing of the endpoint, authority or retention appears.
    let printed = format!("{items:?}");
    for private in [
        "catalog.example.test",
        "synthetic-catalog",
        "Retention",
        "document-binding",
    ] {
        assert!(!printed.contains(private), "{private}");
    }
}

#[tokio::test]
async fn source_page_no_stamp_fits_one_bounded_response() {
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let handle = world
        .actor(
            "tenant-a",
            "reader",
            &visibility,
            &[world.document, world.second],
        )
        .await;
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let registry = Registry {
        inner: &trusted,
        stamped: false,
        fail: Mutex::new(false),
    };
    let cursors = PublicCursorStore::new(Arc::new(SystemLeaseClock));
    let (items, next) = page(&world, &registry, &cursors, &handle, 10, None, MIB)
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
    assert!(next.is_none());
    assert!(cursors.is_empty());
}

#[tokio::test]
async fn source_page_no_stamp_overflow_returns_dependency_503_not_truncated_200() {
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let handle = world
        .actor(
            "tenant-a",
            "reader",
            &visibility,
            &[world.document, world.second],
        )
        .await;
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let registry = Registry {
        inner: &trusted,
        stamped: false,
        fail: Mutex::new(false),
    };
    let cursors = PublicCursorStore::new(Arc::new(SystemLeaseClock));
    // More items than one page, or more bytes than one response.
    assert_eq!(
        page(&world, &registry, &cursors, &handle, 1, None, MIB)
            .await
            .unwrap_err(),
        ApiError::DependencyUnavailable
    );
    assert_eq!(
        page(&world, &registry, &cursors, &handle, 10, None, 1_500)
            .await
            .unwrap_err(),
        ApiError::DependencyUnavailable
    );
}

#[tokio::test]
async fn page_turn_visibility_change_stale() {
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let handle = world
        .actor(
            "tenant-a",
            "reader",
            &visibility,
            &[world.document, world.second],
        )
        .await;
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let registry = Registry {
        inner: &trusted,
        stamped: true,
        fail: Mutex::new(false),
    };
    let cursors = PublicCursorStore::new(Arc::new(SystemLeaseClock));
    // Positive control: page two continues.
    let (first, next) = page(&world, &registry, &cursors, &handle, 1, None, MIB)
        .await
        .unwrap();
    let (second, last) = page(&world, &registry, &cursors, &handle, 1, next, MIB)
        .await
        .unwrap();
    assert_eq!(first[0].source_id, world.document);
    assert_eq!(second[0].source_id, world.second);
    assert!(last.is_none());
    // A visibility change between pages is stale, not a shifted page.
    let (_, next) = page(&world, &registry, &cursors, &handle, 1, None, MIB)
        .await
        .unwrap();
    let actor = world.authority.resolve(&handle).await.unwrap().unwrap();
    visibility.revoke(actor.principal(), world.second).unwrap();
    assert_eq!(
        page(&world, &registry, &cursors, &handle, 1, next, MIB)
            .await
            .unwrap_err(),
        ApiError::CursorStale
    );
}

#[tokio::test]
async fn infra_error_never_partial_200() {
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let handle = world
        .actor(
            "tenant-a",
            "reader",
            &visibility,
            &[world.document, world.second],
        )
        .await;
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let registry = Registry {
        inner: &trusted,
        stamped: true,
        fail: Mutex::new(true),
    };
    let cursors = PublicCursorStore::new(Arc::new(SystemLeaseClock));
    assert_eq!(
        page(&world, &registry, &cursors, &handle, 10, None, MIB)
            .await
            .unwrap_err(),
        ApiError::DependencyUnavailable
    );
}
