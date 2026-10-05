//! P5-08: the Search API production factory.
//!
//! `build_search_api_runtime` refuses to start unless every trust input is
//! wired by the host: the Bearer credential verifier and challenge binding,
//! the durable Source registration ledger with both namespaces' complete
//! desired sets reconciled, the actor-visible Claim catalog, the per-actor
//! Source read ports, and a transport for every registered remote Source.
//! Actors and Source visibility come only from the host's verified resolver
//! and grant policy behind the checked adapters; nothing is read from a
//! request or provider body, and the production transport factory cannot
//! select a loopback transport. Each request builds its core services from
//! these owned parts, so the four routes share one core per actor.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use search_api_http::auth::{SearchAuthSchemeBinding, SearchCredentialVerifierPort};
use search_api_http::limits::SUCCESS_BODY_BYTES;
use search_api_http::router::{
    ApiFuture, SearchApiBackend, SearchOperation, SearchRouterConfig, build_search_router,
};
use search_application::SearchError;
use search_application::api_cursor::{CursorHandle, PublicCursorStore};
use search_application::api_scope::{
    ApiError, SearchOperationContext, prepare_api_visible_sources,
};
use search_application::discover_route::{
    DiscoverInput, DiscoverRouteService, DiscoverRouteWiring,
};
use search_application::discovery_service::{DiscoveryConfig, DiscoveryPorts, DiscoveryService};
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort,
    ConceptRegistryPort, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
    DirectoryRetrieverPort, EvidenceResolverPort, GenerationReadPort, HyperGraphRetrieverPort,
    LexicalRetrieverPort, StructuredRetrieverPort,
};
use search_application::remote_disclosure::{
    CurrentDisclosureAccessPort, DisclosedFields, DisclosureOwner, MAX_DISCLOSURE_TTL,
    RemoteWiring, ScopedDisclosureGate, TransientDisclosure,
};
use search_application::remote_evidence::RegisteredLineage;
use search_application::remote_lease::{LeaseClock, SystemLeaseClock};
use search_application::remote_registration::RemoteSourceRegistration;
use search_application::resource_read::{
    CurrentResourceReadPort, ResourceLocatorPort, ResourceReadService, ResourceView,
};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::scoped::{
    AccessContextHandle, CheckedAuthorityAdapter, CheckedSourceVisibilityAdapter,
    TrustedSearchScope, VerifiedActorResolverPort, VerifiedSourceVisibilityPort,
    VisibleCatalogSnapshot,
};
use search_application::search_query::{SearchInput, SearchQueryService, SearchResultView};
use search_application::source_browse::{SourceBrowseService, SourceView};
use search_application::source_registration::{
    CompleteDesiredRegistrations, HostRegistrationSnapshotPort, RegistrationNamespace,
    SourceRegistration, SourceRegistrationCatalog, TrustedVisibleRegistry,
};
use search_application::source_registry::InMemorySourceRegistry;
use search_application::visible_claim::ActorVisibleClaimCatalogPort;
use search_core::discovery::{CandidateIdentityClass, DiscoveryResult, FederatedCandidate};
use search_core::id::{ClaimId, ResourceId, SourceId};
use search_core::projection::ProjectionGenerationKey;
use search_source_http::adapter::HttpRemoteSourceAdapter;
use search_source_http::transport::{AddressResolver, GuardedHttpTransport, TransportLimits};
use sqlx::PgPool;

use crate::source_registration::PgSourceRegistrationLedger;

pub use search_api_http::router::MAX_OPERATION_TIMEOUT;

/// Source read ports bound to one verified actor for one request.
#[derive(Clone)]
pub struct ActorPorts {
    pub generations: Arc<dyn GenerationReadPort>,
    pub concepts: Arc<dyn ConceptRegistryPort>,
    pub assertions: Arc<dyn AssertionStorePort>,
    pub evidence: Arc<dyn EvidenceResolverPort>,
    pub directory: Option<Arc<dyn DirectoryRetrieverPort>>,
    pub structured: Option<Arc<dyn StructuredRetrieverPort>>,
    pub lexical: Option<Arc<dyn LexicalRetrieverPort>>,
    /// Graph needs both the retriever and current access to every participant.
    pub hypergraph: Option<Arc<dyn HyperGraphRetrieverPort>>,
    pub graph_resource_access: Option<Arc<dyn CurrentAccessEvaluatorPort>>,
    pub access: Arc<dyn CurrentCandidateAccessEvaluatorPort>,
    pub resource_locator: Arc<dyn ResourceLocatorPort>,
    pub resource_reader: Arc<dyn CurrentResourceReadPort>,
}

impl fmt::Debug for ActorPorts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActorPorts(<actor-bound>)")
    }
}

/// Host factory of the durable Source read ports of one verified actor.
pub trait ActorPortsFactory: Send + Sync {
    fn for_actor<'a>(&'a self, actor: &'a TrustedSearchScope) -> ApiFuture<'a, ActorPorts>;
}

/// The fixed-origin transport of one registered remote Source.
pub trait RemoteTransportFactory: Send + Sync {
    fn transport(
        &self,
        registration: &RemoteSourceRegistration,
    ) -> Result<GuardedHttpTransport, SearchError>;
}

/// Production transports: HTTPS to the registered origin only.
pub struct ProductionTransports {
    resolver: Arc<dyn AddressResolver>,
}

impl ProductionTransports {
    pub fn new(resolver: Arc<dyn AddressResolver>) -> Self {
        Self { resolver }
    }
}

impl RemoteTransportFactory for ProductionTransports {
    fn transport(
        &self,
        registration: &RemoteSourceRegistration,
    ) -> Result<GuardedHttpTransport, SearchError> {
        GuardedHttpTransport::new_production(
            registration.endpoint().clone(),
            self.resolver.clone(),
            TransportLimits::from_registration(registration.limits()),
        )
    }
}

/// The host's trust and Source wiring; a `None` refuses startup.
pub struct SearchApiHostConfig {
    pub actors: Arc<dyn VerifiedActorResolverPort>,
    pub visibility: Arc<dyn VerifiedSourceVisibilityPort>,
    pub registrations: Arc<dyn HostRegistrationSnapshotPort>,
    pub claims: Option<Arc<dyn ActorVisibleClaimCatalogPort>>,
    pub remote_transports: Option<Arc<dyn RemoteTransportFactory>>,
    pub config: DiscoveryConfig,
    pub operation_timeout: Duration,
    pub disclosure_ttl: Duration,
}

/// The durable Source registration store and the per-actor read ports.
pub struct SearchApiDurablePorts {
    pub pool: PgPool,
    pub actor_ports: Option<Arc<dyn ActorPortsFactory>>,
}

/// The v0 identity scheme: a credential verifier and the Bearer challenge.
pub struct SearchApiIdentityScheme {
    pub credentials: Option<Arc<dyn SearchCredentialVerifierPort>>,
    pub auth: Option<SearchAuthSchemeBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupError {
    Http(search_api_http::router::StartupError),
    ClaimCatalogUnwired,
    ActorPortsUnwired,
    RemoteTransportUnwired,
    /// Zero, or longer than the longest disclosure lease.
    InvalidDisclosureTtl,
    /// A namespace's complete desired set or the durable ledger refused.
    Registration,
}

pub struct SearchApiRuntime {
    router: Router,
    catalog: Arc<SourceRegistrationCatalog>,
    remote_transports: bool,
}

impl fmt::Debug for SearchApiRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SearchApiRuntime(<wired>)")
    }
}

impl SearchApiRuntime {
    pub fn router(&self) -> Router {
        self.router.clone()
    }

    /// Reconciles one namespace's new complete set through the durable
    /// ledger. Any failure, or a cancelled call, leaves every current gate
    /// closed until a later reconcile of the host's state succeeds.
    pub async fn reconcile(
        &self,
        desired: &CompleteDesiredRegistrations,
    ) -> Result<(), SearchError> {
        if desired.namespace() == RegistrationNamespace::Remote
            && !desired.is_empty()
            && !self.remote_transports
        {
            return Err(SearchError::InvalidRequest(
                "remote Sources need a transport".into(),
            ));
        }
        self.catalog.replace_checked(desired).await
    }
}

pub async fn build_search_api_runtime(
    host_config: SearchApiHostConfig,
    durable_ports: SearchApiDurablePorts,
    identity_scheme: SearchApiIdentityScheme,
) -> Result<SearchApiRuntime, StartupError> {
    use search_api_http::router::StartupError as HttpStartupError;
    // Every wiring check passes before the durable ledger is touched.
    if identity_scheme.credentials.is_none() {
        return Err(StartupError::Http(
            HttpStartupError::CredentialVerifierUnwired,
        ));
    }
    identity_scheme
        .auth
        .as_ref()
        .ok_or(StartupError::Http(HttpStartupError::ChallengeUnwired))?
        .validate()
        .map_err(|error| StartupError::Http(HttpStartupError::Challenge(error)))?;
    if host_config.operation_timeout.is_zero()
        || host_config.operation_timeout > MAX_OPERATION_TIMEOUT
    {
        return Err(StartupError::Http(HttpStartupError::InvalidTimeout));
    }
    if host_config.disclosure_ttl.is_zero() || host_config.disclosure_ttl > MAX_DISCLOSURE_TTL {
        return Err(StartupError::InvalidDisclosureTtl);
    }
    let claims = host_config
        .claims
        .ok_or(StartupError::ClaimCatalogUnwired)?;
    let actor_ports = durable_ports
        .actor_ports
        .ok_or(StartupError::ActorPortsUnwired)?;
    let host = host_config.registrations;
    let document = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Document)
        .await
        .map_err(|_| StartupError::Registration)?;
    let remote = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote)
        .await
        .map_err(|_| StartupError::Registration)?;
    if !remote.is_empty() && host_config.remote_transports.is_none() {
        return Err(StartupError::RemoteTransportUnwired);
    }
    let ledger = Arc::new(PgSourceRegistrationLedger::new(durable_ports.pool, host));
    let catalog = Arc::new(
        SourceRegistrationCatalog::try_new(ledger, &document, &remote)
            .await
            .map_err(|_| StartupError::Registration)?,
    );
    let remote_transports = host_config.remote_transports.is_some();
    let backend = Arc::new(RuntimeBackend {
        actors: host_config.actors,
        visibility: host_config.visibility,
        catalog: catalog.clone(),
        actor_ports,
        claims,
        remote_transports: host_config.remote_transports,
        config: host_config.config,
        cursors: PublicCursorStore::new(Arc::new(SystemLeaseClock)),
        clock: Arc::new(SystemLeaseClock),
        disclosure_ttl: host_config.disclosure_ttl,
    });
    let router = build_search_router(SearchRouterConfig {
        backend,
        credentials: identity_scheme.credentials,
        auth: identity_scheme.auth,
        operation_timeout: host_config.operation_timeout,
    })
    .map_err(StartupError::Http)?;
    Ok(SearchApiRuntime {
        router,
        catalog,
        remote_transports,
    })
}

struct RuntimeBackend {
    actors: Arc<dyn VerifiedActorResolverPort>,
    visibility: Arc<dyn VerifiedSourceVisibilityPort>,
    catalog: Arc<SourceRegistrationCatalog>,
    actor_ports: Arc<dyn ActorPortsFactory>,
    claims: Arc<dyn ActorVisibleClaimCatalogPort>,
    remote_transports: Option<Arc<dyn RemoteTransportFactory>>,
    config: DiscoveryConfig,
    cursors: PublicCursorStore,
    clock: Arc<dyn LeaseClock>,
    disclosure_ttl: Duration,
}

/// Search binds no Claims, so it reads no selector.
struct NoSelectors;

impl ClaimSelectorPort for NoSelectors {
    fn selector_for<'a>(
        &'a self,
        _: ProjectionGenerationKey,
        _: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async { Ok(None) })
    }
}

/// A remote candidate's access is asked of its own adapter only.
struct AccessDispatch<'a> {
    local: &'a dyn CurrentCandidateAccessEvaluatorPort,
    remote: &'a [HttpRemoteSourceAdapter<'a>],
}

impl CurrentCandidateAccessEvaluatorPort for AccessDispatch<'_> {
    fn evaluate<'b>(
        &'b self,
        candidate: &'b FederatedCandidate,
        access_context: &'b str,
    ) -> BoxFuture<'b, AccessDecision> {
        Box::pin(async move {
            match self
                .remote
                .iter()
                .find(|adapter| adapter.registration().source_id() == candidate.source_ref)
            {
                Some(adapter) => adapter.evaluate(candidate, access_context).await,
                None => self.local.evaluate(candidate, access_context).await,
            }
        })
    }
}

impl RuntimeBackend {
    fn authority(&self) -> CheckedAuthorityAdapter<'_> {
        CheckedAuthorityAdapter::new(&*self.actors)
    }

    fn checked_visibility(&self) -> CheckedSourceVisibilityAdapter<'_> {
        CheckedSourceVisibilityAdapter::new(&*self.visibility, &self.catalog)
    }

    fn remote_adapters<'a>(
        &self,
        snapshot: &VisibleCatalogSnapshot,
        authority: &'a CheckedAuthorityAdapter<'a>,
        visibility: &'a CheckedSourceVisibilityAdapter<'a>,
    ) -> Result<Vec<HttpRemoteSourceAdapter<'a>>, ApiError> {
        let mut adapters = Vec::new();
        for entry in snapshot.entries() {
            let SourceRegistration::Remote(registration) = entry.registration() else {
                continue;
            };
            let transport = self
                .remote_transports
                .as_ref()
                .ok_or(ApiError::DependencyUnavailable)?
                .transport(registration)
                .map_err(|_| ApiError::DependencyUnavailable)?;
            adapters.push(
                HttpRemoteSourceAdapter::new(
                    registration.clone(),
                    transport,
                    authority,
                    visibility,
                )
                .map_err(|_| ApiError::DependencyUnavailable)?,
            );
        }
        Ok(adapters)
    }
}

impl SearchApiBackend for RuntimeBackend {
    fn authenticate<'a>(
        &'a self,
        handle: &'a AccessContextHandle,
        deadline: Instant,
    ) -> ApiFuture<'a, SearchOperationContext> {
        Box::pin(async move {
            SearchOperationContext::authenticate(&self.authority(), handle, deadline).await
        })
    }

    /// v0 grants every verified actor the four read operations; Source and
    /// item visibility decide what each one returns.
    fn authorize<'a>(
        &'a self,
        _context: &'a SearchOperationContext,
        _operation: SearchOperation,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn visible<'a>(
        &'a self,
        context: &'a SearchOperationContext,
    ) -> ApiFuture<'a, VisibleCatalogSnapshot> {
        Box::pin(async move {
            let authority = self.authority();
            let visibility = self.checked_visibility();
            let registry = TrustedVisibleRegistry::new(&authority, &visibility, &self.catalog);
            prepare_api_visible_sources(&authority, &registry, context).await
        })
    }

    fn search<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        input: SearchInput,
    ) -> ApiFuture<'a, TransientDisclosure<SearchResultView>> {
        Box::pin(async move {
            let ports = self.actor_ports.for_actor(context.actor()).await?;
            let sources = InMemorySourceRegistry::default();
            let discovery = DiscoveryService::new(
                self.config.clone(),
                DiscoveryPorts {
                    sources: &sources,
                    generations: &*ports.generations,
                    concepts: &*ports.concepts,
                    retrieval: RetrievalExecutionPorts {
                        directory: ports.directory.as_deref(),
                        structured: ports.structured.as_deref(),
                        lexical: ports.lexical.as_deref(),
                        hypergraph: None,
                        graph_resource_access: None,
                        remote: None,
                        access: &*ports.access,
                    },
                    selectors: &NoSelectors,
                    assertions: &*ports.assertions,
                    evidence: &*ports.evidence,
                    probe: None,
                    probe_catalog: None,
                    source_policy: None,
                },
            )
            .map_err(|_| ApiError::ServiceUnavailable)?;
            SearchQueryService::new(
                &discovery,
                &self.cursors,
                self.clock.clone(),
                self.disclosure_ttl,
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
            let ports = self.actor_ports.for_actor(context.actor()).await?;
            let authority = self.authority();
            let visibility = self.checked_visibility();
            let registry = TrustedVisibleRegistry::new(&authority, &visibility, &self.catalog);
            let adapters = self.remote_adapters(snapshot, &authority, &visibility)?;
            let access = AccessDispatch {
                local: &*ports.access,
                remote: &adapters,
            };
            let mut remote = Vec::with_capacity(adapters.len());
            for adapter in &adapters {
                let registration = adapter.registration();
                remote.push((
                    registration.source_id(),
                    RemoteWiring {
                        port: adapter,
                        lineage: RegisteredLineage::new(registration, vec![])
                            .map_err(|_| ApiError::DependencyUnavailable)?,
                        provenance: adapter,
                        evaluation_ttl: Duration::from_millis(
                            registration.limits().evaluation_millis,
                        ),
                        idle_timeout: None,
                    },
                ));
            }
            let sources = InMemorySourceRegistry::default();
            DiscoverRouteService::new(DiscoverRouteWiring {
                config: self.config.clone(),
                sources: &sources,
                generations: &*ports.generations,
                concepts: &*ports.concepts,
                retrieval: RetrievalExecutionPorts {
                    directory: ports.directory.as_deref(),
                    structured: ports.structured.as_deref(),
                    lexical: ports.lexical.as_deref(),
                    hypergraph: ports.hypergraph.as_deref(),
                    graph_resource_access: ports.graph_resource_access.as_deref(),
                    remote: None,
                    access: &access,
                },
                assertions: &*ports.assertions,
                evidence: &*ports.evidence,
                probe: None,
                probe_catalog: None,
                source_policy: None,
                authority: &authority,
                visibility: &visibility,
                registry: &registry,
                claims: Some(&*self.claims),
                remote,
                clock: self.clock.clone(),
                disclosure_ttl: self.disclosure_ttl,
            })
            .discover(context, snapshot, input)
            .await
        })
    }

    fn resource<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        resource_id: ResourceId,
    ) -> ApiFuture<'a, TransientDisclosure<ResourceView>> {
        Box::pin(async move {
            let ports = self.actor_ports.for_actor(context.actor()).await?;
            ResourceReadService::new(
                &*ports.resource_locator,
                &*ports.resource_reader,
                self.clock.clone(),
                self.disclosure_ttl,
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
                &self.cursors,
                self.clock.clone(),
                self.disclosure_ttl,
                SUCCESS_BODY_BYTES,
            )
            .page(context, snapshot, page_size, cursor)
            .await
        })
    }

    fn gate(&self) -> &dyn CurrentDisclosureAccessPort {
        self
    }
}

/// The final gate: actor and Source through the checked adapters, then each
/// disclosed item's current access at its owning Source. A Document item is
/// rechecked right here; a remote item's evaluation, with its per-item
/// checks, closed before disclosure and nothing of it remains to ask. An
/// item without a visible owning Source is refused.
impl CurrentDisclosureAccessPort for RuntimeBackend {
    fn authorize<'a>(
        &'a self,
        owner: &'a DisclosureOwner,
        disclosed_fields: &'a DisclosedFields,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let authority = self.authority();
            let visibility = self.checked_visibility();
            ScopedDisclosureGate::new(&authority, &visibility)
                .authorize(owner, disclosed_fields)
                .await?;
            let unavailable = || SearchError::SourceUnavailable("disclosure unavailable".into());
            let visible = |source: &SourceId| {
                owner
                    .sources()
                    .iter()
                    .any(|scope| scope.source_id() == *source)
            };
            if disclosed_fields.resources.iter().any(|resource| {
                !disclosed_fields
                    .resource_sources
                    .iter()
                    .any(|(id, _)| id == resource)
            }) || disclosed_fields
                .resource_sources
                .iter()
                .any(|(_, source)| !visible(source))
            {
                return Err(unavailable());
            }
            let mut ports = None;
            for (resource, source) in &disclosed_fields.resource_sources {
                match self.catalog.get_for_server(*source) {
                    Some(SourceRegistration::Document(_)) => {
                        if ports.is_none() {
                            ports = Some(
                                self.actor_ports
                                    .for_actor(owner.actor())
                                    .await
                                    .map_err(|_| unavailable())?,
                            );
                        }
                        let mut candidate = FederatedCandidate::new(
                            format!("{}:{}", source.as_uuid(), resource.as_uuid()),
                            CandidateIdentityClass::DurableResource,
                            *source,
                            "disclosure",
                        );
                        candidate.resource_ref = Some(*resource);
                        let access_context = owner.actor().access_handle().to_opaque_string();
                        let decision = ports
                            .as_ref()
                            .expect("ports were bound above")
                            .access
                            .evaluate(&candidate, &access_context)
                            .await?;
                        if decision != AccessDecision::Allowed {
                            return Err(unavailable());
                        }
                    }
                    Some(SourceRegistration::Remote(_)) => {}
                    None => return Err(unavailable()),
                }
            }
            // A Claim that left the actor's visible catalog after the
            // evaluation is not disclosed.
            for claim in &disclosed_fields.claims {
                if self
                    .claims
                    .bind(owner.actor(), owner.sources(), *claim)
                    .await?
                    .is_none()
                {
                    return Err(unavailable());
                }
            }
            Ok(())
        })
    }
}
