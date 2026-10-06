//! A test backend over the real Search application services and in-memory
//! Sources: the union catalog world, a durable corpus, a Claim catalog and a
//! resource store. Everything is leaked to `'static` for the router.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use search_api_http::auth::{CredentialError, CredentialFuture, SearchCredentialVerifierPort};
use search_api_http::router::{ApiFuture, SearchApiBackend, SearchOperation};
use search_application::api_cursor::{CursorHandle, PublicCursorStore};
use search_application::api_scope::{
    ApiError, SearchOperationContext, prepare_api_visible_sources,
};
use search_application::discover_route::{
    DiscoverInput, DiscoverRouteService, DiscoverRouteWiring,
};
use search_application::ports::BoxFuture;
use search_application::remote_disclosure::{
    CurrentDisclosureAccessPort, ScopedDisclosureGate, TransientDisclosure,
};
use search_application::remote_lease::SystemLeaseClock;
use search_application::resource_read::{
    CurrentResourceReadPort, ResourceCoverage, ResourceLocatorPort, ResourceReadService,
    ResourceSnapshot, ResourceView, VisibleResourceBinding,
};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::scoped::{
    AccessContextHandle, AuthorizedSourceScope, SyntheticVisibilityAdapter, TrustedSearchScope,
    VisibleCatalogSnapshot,
};
use search_application::search_query::{SearchInput, SearchQueryService, SearchResultView};
use search_application::source_browse::{SourceBrowseService, SourceView};
use search_application::source_registration::TrustedVisibleRegistry;
use search_application::visible_claim::InMemoryClaimCatalog;
use search_core::discovery::DiscoveryResult;
use search_core::id::ResourceId;
use search_core::resource::ResourceKind;

use crate::api::ApiWorld;
use crate::corpus::{Corpus, doc, rid};

/// Durable current Resources of the world's first Document Source.
pub struct Resources {
    pub source: search_core::id::SourceId,
}

impl ResourceLocatorPort for Resources {
    fn resolve_visible<'a>(
        &'a self,
        _: &'a TrustedSearchScope,
        visible: &'a [AuthorizedSourceScope],
        resource_id: ResourceId,
    ) -> BoxFuture<'a, Vec<VisibleResourceBinding>> {
        Box::pin(async move {
            Ok(visible
                .iter()
                .filter(|scope| scope.source_id() == self.source && resource_id == rid(11))
                .map(|scope| {
                    VisibleResourceBinding::new(
                        scope.clone(),
                        resource_id,
                        "private-locator".into(),
                    )
                })
                .collect())
        })
    }
}

impl CurrentResourceReadPort for Resources {
    fn read_current<'a>(
        &'a self,
        _: &'a TrustedSearchScope,
        binding: &'a VisibleResourceBinding,
    ) -> BoxFuture<'a, Option<ResourceSnapshot>> {
        Box::pin(async move {
            Ok(Some(ResourceSnapshot {
                resource_id: binding.resource_id(),
                source_id: binding.scope().source_id(),
                resource_type: ResourceKind::Knowledge,
                resource_version: None,
                title: Some("規程 A1".into()),
                coverage: ResourceCoverage::TitleAndPermittedMetadata,
            }))
        })
    }
}

pub struct Backend {
    pub world: &'static ApiWorld,
    pub visibility: &'static SyntheticVisibilityAdapter<'static>,
    pub corpus: &'static Corpus,
    pub cursors: &'static PublicCursorStore,
    pub claims: &'static InMemoryClaimCatalog,
    pub resources: &'static Resources,
    pub gate: ScopedDisclosureGate<'static>,
    pub denied: Mutex<BTreeSet<(String, &'static str)>>,
    pub seen: Mutex<Vec<(SearchOperation, String)>>,
    pub stall: AtomicBool,
    pub upstream_timeout: AtomicBool,
}

fn name(operation: SearchOperation) -> &'static str {
    match operation {
        SearchOperation::Search => "search",
        SearchOperation::Discover => "discover",
        SearchOperation::Resource => "resource",
        SearchOperation::Sources => "sources",
    }
}

impl Backend {
    pub async fn new() -> &'static Self {
        Self::with_documents(vec![doc(11, "規程 A1", None), doc(12, "規程 A2", None)]).await
    }

    /// The same world with `documents` in the first Document Source.
    pub async fn with_documents(documents: Vec<crate::corpus::Doc>) -> &'static Self {
        let world: &'static ApiWorld = Box::leak(Box::new(ApiWorld::new().await));
        let visibility: &'static SyntheticVisibilityAdapter<'static> =
            Box::leak(Box::new(world.visibility()));
        let corpus: &'static Corpus = Box::leak(Box::new(Corpus::new(vec![
            (world.document, documents),
            (world.second, vec![doc(21, "規程 B1", None)]),
        ])));
        Box::leak(Box::new(Self {
            world,
            visibility,
            corpus,
            cursors: Box::leak(Box::new(PublicCursorStore::new(Arc::new(SystemLeaseClock)))),
            claims: Box::leak(Box::new(InMemoryClaimCatalog::new(vec![]))),
            resources: Box::leak(Box::new(Resources {
                source: world.document,
            })),
            gate: ScopedDisclosureGate::new(&world.authority, visibility),
            denied: Mutex::new(BTreeSet::new()),
            seen: Mutex::new(vec![]),
            stall: AtomicBool::new(false),
            upstream_timeout: AtomicBool::new(false),
        }))
    }

    /// A verified handle for `principal` with both tenant-a Documents.
    pub async fn handle(&self, principal: &str) -> AccessContextHandle {
        self.handle_with(principal, &[self.world.document, self.world.second])
            .await
    }

    pub async fn handle_with(
        &self,
        principal: &str,
        sources: &[search_core::id::SourceId],
    ) -> AccessContextHandle {
        self.world
            .actor("tenant-a", principal, self.visibility, sources)
            .await
    }

    async fn checkpoint(&self) -> Result<(), ApiError> {
        if self.stall.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        if self.upstream_timeout.load(Ordering::SeqCst) {
            return Err(ApiError::UpstreamTimeout);
        }
        Ok(())
    }
}

impl SearchApiBackend for &'static Backend {
    fn authenticate<'a>(
        &'a self,
        handle: &'a AccessContextHandle,
        deadline: Instant,
    ) -> ApiFuture<'a, SearchOperationContext> {
        Box::pin(SearchOperationContext::authenticate(
            &self.world.authority,
            handle,
            deadline,
        ))
    }

    fn authorize<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        operation: SearchOperation,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let principal = context.actor().principal().as_str().to_owned();
            self.seen
                .lock()
                .unwrap()
                .push((operation, principal.clone()));
            if self
                .denied
                .lock()
                .unwrap()
                .contains(&(principal, name(operation)))
            {
                return Err(ApiError::Forbidden);
            }
            Ok(())
        })
    }

    fn visible<'a>(
        &'a self,
        context: &'a SearchOperationContext,
    ) -> ApiFuture<'a, VisibleCatalogSnapshot> {
        Box::pin(async move {
            self.checkpoint().await?;
            let registry = TrustedVisibleRegistry::new(
                &self.world.authority,
                self.visibility,
                &self.world.catalog,
            );
            prepare_api_visible_sources(&self.world.authority, &registry, context).await
        })
    }

    fn search<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        input: SearchInput,
    ) -> ApiFuture<'a, TransientDisclosure<SearchResultView>> {
        Box::pin(async move {
            let discovery = self.corpus.service();
            SearchQueryService::new(
                &discovery,
                self.cursors,
                Arc::new(SystemLeaseClock),
                Duration::from_secs(30),
            )
            .search(context, snapshot, input)
            .await
        })
    }

    fn discover<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        input: DiscoverInput,
    ) -> ApiFuture<'a, TransientDisclosure<DiscoveryResult>> {
        Box::pin(async move {
            let corpus = self.corpus;
            let registry = TrustedVisibleRegistry::new(
                &self.world.authority,
                self.visibility,
                &self.world.catalog,
            );
            let route = DiscoverRouteService::new(DiscoverRouteWiring {
                config: corpus.service_config(),
                sources: corpus,
                generations: corpus,
                concepts: corpus,
                retrieval: RetrievalExecutionPorts {
                    directory: None,
                    structured: None,
                    lexical: Some(corpus),
                    hypergraph: None,
                    graph_resource_access: None,
                    remote: None,
                    access: corpus,
                    vector: None,
                },
                assertions: corpus,
                evidence: corpus,
                probe: None,
                probe_catalog: None,
                source_policy: None,
                authority: &self.world.authority,
                visibility: self.visibility,
                registry: &registry,
                claims: Some(self.claims),
                remote: vec![],
                clock: Arc::new(SystemLeaseClock),
                disclosure_ttl: Duration::from_secs(30),
            });
            route.discover(context, snapshot, input).await
        })
    }

    fn resource<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        resource_id: ResourceId,
    ) -> ApiFuture<'a, TransientDisclosure<ResourceView>> {
        Box::pin(async move {
            ResourceReadService::new(
                self.resources,
                self.resources,
                Arc::new(SystemLeaseClock),
                Duration::from_secs(30),
            )
            .read(context, snapshot, resource_id)
            .await
        })
    }

    fn sources<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        page_size: usize,
        cursor: Option<CursorHandle>,
    ) -> ApiFuture<'a, TransientDisclosure<SourceView>> {
        Box::pin(async move {
            SourceBrowseService::new(
                self.cursors,
                Arc::new(SystemLeaseClock),
                Duration::from_secs(30),
                1024 * 1024,
            )
            .page(context, snapshot, page_size, cursor)
            .await
        })
    }

    fn gate(&self) -> &dyn CurrentDisclosureAccessPort {
        &self.gate
    }
}

/// Server-side token → verified handle map; `outage` fails the verifier.
#[derive(Default)]
pub struct Tokens(pub Mutex<BTreeMap<String, AccessContextHandle>>);

impl SearchCredentialVerifierPort for Tokens {
    fn verify<'a>(&'a self, token: &'a str) -> CredentialFuture<'a> {
        Box::pin(async move {
            if token == "outage" {
                return Err(CredentialError::Unavailable);
            }
            Ok(self.0.lock().unwrap().get(token).cloned())
        })
    }
}
