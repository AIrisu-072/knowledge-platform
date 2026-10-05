//! P5-03: the Search route over the actor-visible catalog.

#[path = "support/api.rs"]
mod api;
#[path = "support/search_corpus.rs"]
mod corpus;

use std::sync::Arc;
use std::time::{Duration, Instant};

use api::*;
use corpus::*;
use search_application::api_cursor::PublicCursorStore;
use search_application::api_scope::{SearchOperationContext, prepare_api_visible_sources};
use search_application::discovery_service::MatchedField;
use search_application::ports::BoxFuture;
use search_application::remote_disclosure::{
    CurrentDisclosureAccessPort, DisclosedFields, DisclosureOwner, TransientDisclosure,
};
use search_application::remote_lease::SystemLeaseClock;
use search_application::search_query::{
    PublicGapCode, PublicGapView, SearchCoverage, SearchInput, SearchItemView, SearchQueryService,
    SearchResultView,
};
use search_application::source_registration::TrustedVisibleRegistry;
use search_core::id::{ResourceId, SourceId};

/// The final gate is exercised elsewhere; here every disclosure is allowed.
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

#[derive(Debug, PartialEq, Eq)]
struct Page {
    items: Vec<SearchItemView>,
    partial: bool,
    coverage: Vec<(SourceId, bool)>,
    gaps: Vec<PublicGapView>,
    cursor: bool,
}

async fn page(mut disclosure: TransientDisclosure<SearchResultView>) -> Page {
    let mut page = None;
    disclosure
        .disclose_with(&Open, |view| {
            page = Some(Page {
                items: view.items().to_vec(),
                partial: view.partial(),
                coverage: view
                    .coverage()
                    .iter()
                    .map(|coverage| (coverage.source_id, coverage.body))
                    .collect(),
                gaps: view.gaps().to_vec(),
                cursor: view.next_cursor().is_some(),
            });
            Ok(())
        })
        .await
        .unwrap();
    page.unwrap()
}

fn input(query: &str) -> SearchInput {
    SearchInput {
        query: query.into(),
        resource_types: vec![],
        source_ids: vec![],
        coverage: SearchCoverage::TitleAndPermittedMetadata,
        page_size: 20,
        cursor: None,
    }
}

fn ids(page: &Page) -> Vec<ResourceId> {
    page.items.iter().map(|item| item.resource_id).collect()
}

/// Searches as a tenant-a reader granted `document` and `second`.
async fn search(world: &ApiWorld, corpus: &Corpus, input: SearchInput) -> Page {
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
    let snapshot = prepare_api_visible_sources(&world.authority, &registry, &context)
        .await
        .unwrap();
    let discovery = corpus.service();
    let cursors = PublicCursorStore::new(Arc::new(SystemLeaseClock));
    let service = SearchQueryService::new(
        &discovery,
        &cursors,
        Arc::new(SystemLeaseClock),
        Duration::from_secs(30),
    );
    page(service.search(&context, &snapshot, input).await.unwrap()).await
}

fn corpus(world: &ApiWorld) -> Corpus {
    Corpus::new(vec![
        (
            world.document,
            vec![
                doc(11, "規程 A1", None),
                doc(12, "規程 A2", None),
                doc(13, "手順", None),
            ],
        ),
        (world.second, vec![doc(21, "規程 B1", None)]),
        (world.hidden, vec![doc(31, "規程 H1", None)]),
        (world.foreign_document, vec![doc(41, "規程 F1", None)]),
    ])
}

#[tokio::test]
async fn search_uses_visible_sources_only_and_priority_concat() {
    let world = ApiWorld::new().await;
    let corpus = corpus(&world);
    let page = search(&world, &corpus, input("規程")).await;
    // Per-list order is kept and lists concatenate in plan order.
    assert_eq!(ids(&page), vec![rid(11), rid(12), rid(21)]);
    assert_eq!(
        page.items.iter().map(|item| item.rank).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(
        page.items
            .iter()
            .all(|item| item.matched == vec![MatchedField::Title])
    );
    assert!(!page.partial);
    // Hidden and foreign Sources were never even read.
    let read = corpus.retrieved.lock().unwrap().clone();
    assert!(!read.contains(&world.hidden));
    assert!(!read.contains(&world.foreign_document));
}

#[tokio::test]
async fn unknown_hidden_foreign_source_ids_same_empty_intersection() {
    let world = ApiWorld::new().await;
    let mut pages = Vec::new();
    for requested in [source(9_999), world.hidden, world.foreign_document] {
        let corpus = corpus(&world);
        let mut request = input("規程");
        request.source_ids = vec![requested];
        pages.push(search(&world, &corpus, request).await);
        assert!(corpus.retrieved.lock().unwrap().is_empty());
    }
    assert!(pages[0].items.is_empty());
    assert_eq!(pages[0], pages[1]);
    assert_eq!(pages[0], pages[2]);
    // A visible ID narrows the search to that Source only.
    let corpus = corpus(&world);
    let mut request = input("規程");
    request.source_ids = vec![world.second];
    assert_eq!(ids(&search(&world, &corpus, request).await), vec![rid(21)]);
}

#[tokio::test]
async fn final_revoke_recomputes_rank_count_gap() {
    let world = ApiWorld::new().await;
    let corpus = corpus(&world);
    // Allowed when retrieved, revoked before the result is ranked.
    corpus
        .revoke_after_first_check
        .lock()
        .unwrap()
        .insert(rid(11));
    let page = search(&world, &corpus, input("規程")).await;
    assert_eq!(ids(&page), vec![rid(12), rid(21)]);
    assert_eq!(
        page.items.iter().map(|item| item.rank).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(page.gaps.is_empty());
}

#[tokio::test]
async fn body_required_never_uses_title_graph_vector_hit() {
    let world = ApiWorld::new().await;
    let corpus = Corpus::new(vec![
        (
            world.document,
            vec![
                doc(11, "規程", None),
                doc(12, "別名", Some("本文に規程を含む")),
            ],
        ),
        (
            world.second,
            vec![doc(21, "規程", Some("本文に規程を含む"))],
        ),
    ]);
    corpus.body_refused.lock().unwrap().insert(world.second);
    let mut request = input("規程");
    request.coverage = SearchCoverage::BodyRequired;
    let page = search(&world, &corpus, request).await;
    // Only the verified body hit; a title match never stands in for body.
    assert_eq!(ids(&page), vec![rid(12)]);
    assert_eq!(page.items[0].matched, vec![MatchedField::Body]);
    // The refusing Source claims no body coverage and makes the page partial.
    assert!(page.coverage.contains(&(world.document, true)));
    assert!(page.coverage.contains(&(world.second, false)));
    assert!(page.partial);
}

#[tokio::test]
async fn optional_timeout_safe_partial_required_missing_never_sufficient() {
    let world = ApiWorld::new().await;
    let corpus = corpus(&world);
    corpus.failing.lock().unwrap().insert(world.second);
    let page = search(&world, &corpus, input("規程")).await;
    // The independent Source's results survive; the page says it is partial.
    assert_eq!(ids(&page), vec![rid(11), rid(12)]);
    assert!(page.partial);
    assert!(
        page.gaps
            .iter()
            .any(|gap| gap.code == PublicGapCode::Availability && !gap.blocking)
    );
}
