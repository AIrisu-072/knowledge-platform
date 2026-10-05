//! P5-03: RAM-only public Search cursors.

#[path = "support/api.rs"]
mod api;
#[path = "support/search_corpus.rs"]
mod corpus;

use std::sync::Arc;
use std::time::{Duration, Instant};

use api::*;
use corpus::*;
use search_application::api_cursor::{CursorHandle, PublicCursorStore};
use search_application::api_scope::{
    ApiError, SearchOperationContext, prepare_api_visible_sources,
};
use search_application::ports::BoxFuture;
use search_application::remote_disclosure::{
    CurrentDisclosureAccessPort, DisclosedFields, DisclosureOwner,
};
use search_application::remote_lease::LeaseClock;
use search_application::scoped::{
    AccessContextAuthorityPort, AccessContextHandle, ScopedSourceRegistryPort,
    SyntheticVisibilityAdapter, TrustedSearchScope, VisibleCatalogSnapshot, VisibleSetStamp,
};
use search_application::search_query::{
    PublicGapCode, SearchCoverage, SearchInput, SearchQueryService,
};
use search_application::source_registration::TrustedVisibleRegistry;
use search_core::id::ResourceId;
use uuid::Uuid;

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

/// A registry with an authoritative visible-set epoch.
struct Stamped<'a>(&'a dyn ScopedSourceRegistryPort);
impl ScopedSourceRegistryPort for Stamped<'_> {
    fn visible_sources<'b>(
        &'b self,
        actor: &'b TrustedSearchScope,
    ) -> BoxFuture<'b, VisibleCatalogSnapshot> {
        Box::pin(async move {
            let entries = self.0.visible_sources(actor).await?.entries().to_vec();
            let stamp = VisibleSetStamp::of(actor, &entries);
            Ok(VisibleCatalogSnapshot::stamped(entries, stamp))
        })
    }
}

/// A clock the test moves by hand.
struct Clock(std::sync::Mutex<Instant>);
impl LeaseClock for Clock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

#[derive(Debug)]
struct Page {
    ids: Vec<ResourceId>,
    ranks: Vec<usize>,
    next: Option<CursorHandle>,
    partial: bool,
    pagination_gap: bool,
}

#[allow(clippy::too_many_arguments)]
async fn search(
    world: &ApiWorld,
    corpus: &Corpus,
    registry: &dyn ScopedSourceRegistryPort,
    cursors: &PublicCursorStore,
    clock: Arc<dyn LeaseClock>,
    handle: &AccessContextHandle,
    query: &str,
    cursor: Option<CursorHandle>,
) -> Result<Page, ApiError> {
    let context = SearchOperationContext::authenticate(
        &world.authority,
        handle,
        Instant::now() + Duration::from_secs(5),
    )
    .await?;
    let snapshot = prepare_api_visible_sources(&world.authority, registry, &context).await?;
    let discovery = corpus.service();
    let service = SearchQueryService::new(&discovery, cursors, clock, Duration::from_secs(30));
    let mut disclosure = service
        .search(
            &context,
            &snapshot,
            SearchInput {
                query: query.into(),
                resource_types: vec![],
                source_ids: vec![],
                coverage: SearchCoverage::TitleAndPermittedMetadata,
                page_size: 1,
                cursor,
            },
        )
        .await?;
    let mut page = None;
    disclosure
        .disclose_with(&Open, |view| {
            page = Some(Page {
                ids: view.items().iter().map(|item| item.resource_id).collect(),
                ranks: view.items().iter().map(|item| item.rank).collect(),
                next: view.next_cursor(),
                partial: view.partial(),
                pagination_gap: view
                    .gaps()
                    .iter()
                    .any(|gap| gap.code == PublicGapCode::PaginationUnavailable),
            });
            Ok(())
        })
        .await
        .unwrap();
    Ok(page.unwrap())
}

fn corpus(world: &ApiWorld) -> Corpus {
    Corpus::new(vec![
        (
            world.document,
            vec![doc(11, "規程 A1", None), doc(12, "規程 A2", None)],
        ),
        (world.second, vec![doc(21, "規程 B1", None)]),
    ])
}

async fn reader(
    world: &ApiWorld,
    visibility: &SyntheticVisibilityAdapter<'_>,
    name: &str,
) -> AccessContextHandle {
    world
        .actor(
            "tenant-a",
            name,
            visibility,
            &[world.document, world.second],
        )
        .await
}

#[tokio::test]
async fn uuid_v4_handle_has_no_payload_and_no_log() {
    let world = ApiWorld::new().await;
    let corpus = corpus(&world);
    let visibility = world.visibility();
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let registry = Stamped(&trusted);
    let clock: Arc<dyn LeaseClock> = Arc::new(Clock(std::sync::Mutex::new(Instant::now())));
    let cursors = PublicCursorStore::new(clock.clone());
    let handle = reader(&world, &visibility, "reader").await;
    let page = search(
        &world, &corpus, &registry, &cursors, clock, &handle, "規程", None,
    )
    .await
    .unwrap();
    let cursor = page.next.unwrap();
    let wire = cursor.to_wire();
    let uuid = Uuid::parse_str(&wire).unwrap();
    assert_eq!(uuid.get_version_num(), 4);
    assert_eq!(CursorHandle::parse(&wire), Some(cursor));
    // Nothing of the request, Source or Resource is inside the handle.
    for leaked in [
        "規程".to_owned(),
        world.document.as_uuid().to_string(),
        rid(11).as_uuid().to_string(),
    ] {
        assert!(!wire.contains(&leaked));
    }
    assert_eq!(format!("{cursor:?}"), "CursorHandle(<redacted>)");
    // Only the canonical v4 form parses.
    assert!(CursorHandle::parse(&Uuid::now_v7().to_string()).is_none());
    assert!(CursorHandle::parse(&wire.to_uppercase()).is_none());
    assert!(CursorHandle::parse("not-a-cursor").is_none());
}

#[tokio::test]
async fn other_actor_session_query_visibility_generation_retention_stale() {
    let world = ApiWorld::new().await;
    let corpus = corpus(&world);
    let visibility = world.visibility();
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let registry = Stamped(&trusted);
    let clock: Arc<dyn LeaseClock> = Arc::new(Clock(std::sync::Mutex::new(Instant::now())));
    let cursors = PublicCursorStore::new(clock.clone());
    let handle = reader(&world, &visibility, "reader").await;
    let first = |cursor| {
        search(
            &world,
            &corpus,
            &registry,
            &cursors,
            clock.clone(),
            &handle,
            "規程",
            cursor,
        )
    };

    // Positive control: the same actor and request continue the S1 order.
    let page = first(None).await.unwrap();
    assert_eq!(
        (page.ids.clone(), page.ranks.clone()),
        (vec![rid(11)], vec![1])
    );
    let next = first(page.next).await.unwrap();
    assert_eq!((next.ids, next.ranks), (vec![rid(12)], vec![2]));
    // A cursor is consumed by its first use.
    assert_eq!(first(page.next).await.unwrap_err(), ApiError::CursorStale);

    // Another actor of the same tenant.
    let cursor = first(None).await.unwrap().next;
    let other = reader(&world, &visibility, "other").await;
    assert_eq!(
        search(
            &world,
            &corpus,
            &registry,
            &cursors,
            clock.clone(),
            &other,
            "規程",
            cursor
        )
        .await
        .unwrap_err(),
        ApiError::CursorStale
    );
    // Another request.
    let cursor = first(None).await.unwrap().next;
    assert_eq!(
        search(
            &world,
            &corpus,
            &registry,
            &cursors,
            clock.clone(),
            &handle,
            "規",
            cursor
        )
        .await
        .unwrap_err(),
        ApiError::CursorStale
    );
    // A new generation of one Source.
    let cursor = first(None).await.unwrap().next;
    corpus.bump(world.document);
    assert_eq!(first(cursor).await.unwrap_err(), ApiError::CursorStale);
    // A changed visible set.
    let cursor = first(None).await.unwrap().next;
    let actor = world.authority.resolve(&handle).await.unwrap().unwrap();
    visibility.revoke(actor.principal(), world.second).unwrap();
    assert_eq!(first(cursor).await.unwrap_err(), ApiError::CursorStale);
    // A visible NO_RETENTION Source: no continuation at all.
    let retained = world
        .actor(
            "tenant-a",
            "remote-reader",
            &visibility,
            &[world.document, world.second, world.remote],
        )
        .await;
    let page = search(
        &world,
        &corpus,
        &registry,
        &cursors,
        clock.clone(),
        &retained,
        "規程",
        None,
    )
    .await
    .unwrap();
    assert!(page.next.is_none());
    assert!(page.partial && page.pagination_gap);
}

#[tokio::test]
async fn expiry_and_restart_stale() {
    let world = ApiWorld::new().await;
    let corpus = corpus(&world);
    let visibility = world.visibility();
    let trusted = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let registry = Stamped(&trusted);
    let manual = Arc::new(Clock(std::sync::Mutex::new(Instant::now())));
    let clock: Arc<dyn LeaseClock> = manual.clone();
    let cursors = PublicCursorStore::new(clock.clone());
    let handle = reader(&world, &visibility, "reader").await;
    let run = |store, cursor| {
        search(
            &world,
            &corpus,
            &registry,
            store,
            clock.clone(),
            &handle,
            "規程",
            cursor,
        )
    };

    // Idle for longer than a minute.
    let cursor = run(&cursors, None).await.unwrap().next;
    *manual.0.lock().unwrap() += Duration::from_secs(61);
    assert_eq!(
        run(&cursors, cursor).await.unwrap_err(),
        ApiError::CursorStale
    );
    // Within the idle window it continues.
    let cursor = run(&cursors, None).await.unwrap().next;
    *manual.0.lock().unwrap() += Duration::from_secs(30);
    assert!(run(&cursors, cursor).await.is_ok());
    // A restarted process has no cursor state.
    let cursor = run(&cursors, None).await.unwrap().next;
    let restarted = PublicCursorStore::new(clock.clone());
    assert_eq!(
        run(&restarted, cursor).await.unwrap_err(),
        ApiError::CursorStale
    );
}

#[tokio::test]
async fn no_retention_or_unstable_continuation_emits_no_cursor() {
    let world = ApiWorld::new().await;
    let corpus = corpus(&world);
    let visibility = world.visibility();
    // The union catalog has no authoritative stamp: no cursor authority.
    let unstamped = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let clock: Arc<dyn LeaseClock> = Arc::new(Clock(std::sync::Mutex::new(Instant::now())));
    let cursors = PublicCursorStore::new(clock.clone());
    let handle = reader(&world, &visibility, "reader").await;
    let page = search(
        &world,
        &corpus,
        &unstamped,
        &cursors,
        clock.clone(),
        &handle,
        "規程",
        None,
    )
    .await
    .unwrap();
    assert_eq!(page.ids, vec![rid(11)]);
    assert!(page.next.is_none());
    assert!(page.partial && page.pagination_gap);
    assert!(cursors.is_empty());
    // A client-held handle cannot continue an unstamped catalog either.
    let stamped = Stamped(&unstamped);
    let cursor = search(
        &world,
        &corpus,
        &stamped,
        &cursors,
        clock.clone(),
        &handle,
        "規程",
        None,
    )
    .await
    .unwrap()
    .next;
    assert_eq!(
        search(
            &world,
            &corpus,
            &unstamped,
            &cursors,
            clock.clone(),
            &handle,
            "規程",
            cursor
        )
        .await
        .unwrap_err(),
        ApiError::CursorStale
    );
    // A single page that fits needs no cursor and is not partial.
    let only = search(
        &world, &corpus, &unstamped, &cursors, clock, &handle, "B1", None,
    )
    .await
    .unwrap();
    assert_eq!(only.ids, vec![rid(21)]);
    assert!(only.next.is_none() && !only.partial && !only.pagination_gap);
}
