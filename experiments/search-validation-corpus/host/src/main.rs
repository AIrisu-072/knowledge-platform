//! Validation-only Search API host.
//!
//! Composes the production `build_search_api_runtime` over the durable
//! Document read model (lexical, Graph and, when enabled, Vector) and serves
//! it with the production socket server on a loopback address. The only
//! validation-specific part is identity: synthetic Bearer tokens from a local
//! file map to a Search principal and to the Document policy subjects
//! (`poc` issuer) that the Document PoC runtime and its ACLs use. It is not a
//! production authentication path and refuses a non-loopback bind.
//!
//! Environment: `SEARCH_API_DATABASE_URL` (secret, never logged),
//! `SEARCH_WORKER_CONFIG` (the worker's Source file, read for the shared
//! Source identity) and `SEARCH_VALIDATION_ACTORS` (the synthetic actors).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use document_application::{DocumentAccessCheckService, InvocationKind, VerifiedActorContext};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef as DocumentPrincipal};
use document_repository_postgres::PostgresDocumentRepository;
use search_api_http::auth::{
    CredentialError, CredentialFuture, SearchAuthSchemeBinding, SearchCredentialVerifierPort,
    StaticBearerChallenge,
};
use search_api_http::router::ApiFuture;
use search_api_http::send::{ServeOptions, serve};
use search_application::SearchError;
use search_application::api_scope::ApiError;
use search_application::discovery_service::{DiscoveryConfig, TemporalPolicy};
use search_application::materialization::ProbeBudget;
use search_application::ports::BoxFuture;
use search_application::retrieval::{RetrievalInputs, RetrieverProfile, RetrieverSupport};
use search_application::routing::RoutingConstraints;
use search_application::scoped::{
    AccessRevision, CheckedAuthorityAdapter, PrincipalRef, RegistrationRevision, TenantId,
    TrustedSearchScope, VerifiedActorDescriptor, VerifiedActorResolverPort, VerifiedSourceGrant,
    VerifiedSourceVisibilityPort, VisibilityRevision,
};
use search_application::search_core::id::{SessionId, SourceId};
use search_application::search_core::profile::DiscoveryLens;
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{
    DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode,
};
use search_application::source_registration::{
    ConnectedDocumentAdapterCapabilities, ConnectedDocumentAdapterWitness,
    DocumentAdapterCapabilityPort, DocumentAdapterRef, DocumentSourceRegistration,
    HostRegistrationSnapshot, HostRegistrationSnapshotPort, RegistrationNamespace,
    RegistrationSetRevision, ServerDocumentRegistrationConfig, SourceRegistration,
};
use search_application::visible_claim::InMemoryClaimCatalog;
use search_runtime::api::{
    SearchApiDurablePorts, SearchApiHostConfig, SearchApiIdentityScheme, build_search_api_runtime,
};
use search_runtime::durable_read::{
    DocumentActorAccess, DocumentActorAccessPort, DurableDocumentPorts, DurableDocumentReadModel,
};
use search_runtime::vector_runtime::{RegisteredVectorActivation, VectorServices};
use search_runtime::vector_store::{PgVectorGenerations, PgVectorIndex};
use search_source_document::{DocumentApiRead, DocumentCurrentAccessAdapter};
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use time::OffsetDateTime;
use uuid::Uuid;

/// The subset of the worker's Source file this host needs; other worker
/// fields are ignored so both processes read one file.
#[derive(Deserialize)]
struct SourceFile {
    tenant: String,
    source_id: Uuid,
    source_name: String,
    document_adapter_ref: String,
    deployment_revision: u64,
    registration_revision: u64,
    visibility_revision: u64,
    allowed_resource_kinds: Vec<ResourceKind>,
    supported_modes: Vec<DiscoveryMode>,
    enumeration_semantics: EnumerationSemantics,
    lens: DiscoveryLens,
    lexical_root: PathBuf,
    #[serde(default)]
    vector: Option<VectorFile>,
}

#[derive(Deserialize)]
struct VectorFile {
    #[serde(default = "enabled")]
    enabled: bool,
    model_dir: PathBuf,
    #[serde(default = "floor")]
    similarity_floor: f32,
}

fn enabled() -> bool {
    true
}

fn floor() -> f32 {
    search_runtime::vector_runtime::DEFAULT_SIMILARITY_FLOOR
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorsFile {
    bind: SocketAddr,
    #[serde(default)]
    profile: Profile,
    actors: Vec<ActorFile>,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Profile {
    #[default]
    Exploratory,
    Capability,
    Knowledge,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorFile {
    token: String,
    principal: String,
    #[serde(default)]
    groups: Vec<String>,
    /// Whether the host grants this actor the Source at all.
    granted: bool,
}

/// Synthetic sessions end with the validation run; restart renews them.
const SESSION_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

struct Actor {
    principal: String,
    groups: Vec<String>,
    session: Uuid,
}

/// Synthetic sessions: the raw handle is the token itself.
struct Actors {
    tenant: String,
    by_token: BTreeMap<String, Actor>,
    granted: BTreeMap<String, bool>,
    source: SourceId,
    registration_revision: u64,
    visibility_revision: u64,
    started: Instant,
}

impl VerifiedActorResolverPort for Actors {
    fn resolve_verified<'a>(
        &'a self,
        raw_handle: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedActorDescriptor>> {
        Box::pin(async move {
            let Some(actor) = self.by_token.get(raw_handle) else {
                return Ok(None);
            };
            // A synthetic session's lifetime is fixed at startup, so every
            // re-resolution of the same handle describes the same session.
            Ok(Some(VerifiedActorDescriptor::new(
                TenantId::new(self.tenant.clone())?,
                PrincipalRef::new(actor.principal.clone())?,
                Some(SessionId::from_uuid(actor.session)),
                AccessRevision::new(1)?,
                self.started,
                self.started + SESSION_LIFETIME,
            )?))
        })
    }
}

impl VerifiedSourceVisibilityPort for Actors {
    fn grant_for<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<VerifiedSourceGrant>> {
        Box::pin(async move {
            let granted = source == self.source
                && self
                    .granted
                    .get(actor.principal().as_str())
                    .copied()
                    .unwrap_or(false);
            Ok(granted.then(|| {
                VerifiedSourceGrant::new(
                    TenantId::new(self.tenant.clone()).expect("validated tenant"),
                    source,
                    RegistrationRevision::new(self.registration_revision).expect("revision"),
                    VisibilityRevision::new(self.visibility_revision).expect("revision"),
                )
            }))
        })
    }
}

struct Credentials(Arc<Actors>);

impl SearchCredentialVerifierPort for Credentials {
    fn verify<'a>(&'a self, token: &'a str) -> CredentialFuture<'a> {
        Box::pin(async move {
            if !self.0.by_token.contains_key(token) {
                return Ok(None);
            }
            let scope = CheckedAuthorityAdapter::new(&*self.0)
                .authenticate_handle(token)
                .await
                .map_err(|_| CredentialError::Unavailable)?;
            Ok(scope.map(|scope| scope.access_handle().clone()))
        })
    }
}

/// Maps a verified Search actor to its Document identity: the same `poc`
/// issuer and subjects the Document PoC runtime and its ACLs use.
struct DocumentAccess {
    source: SourceId,
    pool: sqlx::PgPool,
    repository: Arc<PostgresDocumentRepository>,
    actors: Arc<Actors>,
}

impl DocumentActorAccessPort for DocumentAccess {
    fn for_actor<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
    ) -> ApiFuture<'a, DocumentActorAccess> {
        Box::pin(async move {
            let principal = actor.principal().as_str();
            let groups = self
                .actors
                .by_token
                .values()
                .find(|known| known.principal == principal)
                .map(|known| known.groups.clone())
                .ok_or(ApiError::IdentityUnavailable)?;
            let mut subjects = vec![
                PolicySubject::new(PolicySubjectKind::Principal, "poc", principal)
                    .map_err(|_| ApiError::IdentityUnavailable)?,
            ];
            for group in &groups {
                subjects.push(
                    PolicySubject::new(PolicySubjectKind::Group, "poc", group)
                        .map_err(|_| ApiError::IdentityUnavailable)?,
                );
            }
            let document_actor = VerifiedActorContext::from_trusted_adapter(
                DocumentPrincipal::new("poc", principal)
                    .map_err(|_| ApiError::IdentityUnavailable)?,
                subjects,
                OffsetDateTime::now_utc() + time::Duration::minutes(5),
                InvocationKind::HumanInteractive,
                None,
            )
            .map_err(|_| ApiError::IdentityUnavailable)?;
            let binding = actor.access_handle().to_opaque_string();
            let access = Arc::new(DocumentCurrentAccessAdapter::new(
                self.source,
                self.pool.clone(),
                DocumentAccessCheckService::new(self.repository.clone()),
                document_actor,
                binding.clone(),
            ));
            let read = Arc::new(DocumentApiRead::new(
                self.source,
                self.pool.clone(),
                access.clone(),
                binding,
            ));
            Ok(DocumentActorAccess {
                access,
                resource_locator: read.clone(),
                resource_reader: read,
            })
        })
    }
}

/// The configured Document adapter declares exactly the configured scope.
struct ConfiguredAdapter {
    kinds: Vec<ResourceKind>,
    modes: Vec<DiscoveryMode>,
    semantics: EnumerationSemantics,
}

impl DocumentAdapterCapabilityPort for ConfiguredAdapter {
    fn connected_capabilities<'a>(
        &'a self,
        binding: &'a DocumentAdapterRef,
    ) -> BoxFuture<'a, Option<ConnectedDocumentAdapterCapabilities>> {
        Box::pin(async move {
            Ok(Some(ConnectedDocumentAdapterCapabilities::new(
                binding.clone(),
                self.kinds.clone(),
                self.modes.clone(),
                vec![self.semantics],
                vec![RetentionMode::PersistentResource],
            )?))
        })
    }
}

/// The same complete inventory as the worker: one Document Source.
struct ConfiguredHost {
    revision: RegistrationSetRevision,
    document: SourceRegistration,
}

impl HostRegistrationSnapshotPort for ConfiguredHost {
    fn snapshot<'a>(
        &'a self,
        namespace: RegistrationNamespace,
    ) -> BoxFuture<'a, HostRegistrationSnapshot> {
        Box::pin(async move {
            let registrations = match namespace {
                RegistrationNamespace::Document => vec![self.document.clone()],
                RegistrationNamespace::Remote => vec![],
            };
            HostRegistrationSnapshot::from_complete_host_inventory(
                namespace,
                self.revision,
                registrations,
            )
        })
    }
}

fn discovery_config(profile: Profile) -> DiscoveryConfig {
    DiscoveryConfig {
        routing: RoutingConstraints {
            required_source_ids: vec![],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
        retriever_profile: match profile {
            Profile::Exploratory => RetrieverProfile::Exploratory,
            Profile::Capability => RetrieverProfile::Capability,
            Profile::Knowledge => RetrieverProfile::Knowledge,
        },
        retriever_support: RetrieverSupport {
            directory: true,
            lexical: true,
            hypergraph: true,
            ..RetrieverSupport::default()
        },
        retrieval_inputs: RetrievalInputs {
            max_initial_retrievers_per_source: 4,
            ..RetrievalInputs::default()
        },
        structured_filters: vec![],
        discriminators: vec![],
        lexical_query: None,
        temporal_policy: TemporalPolicy::default(),
        probe_budget: ProbeBudget {
            max_content_bytes: 0,
            max_latency_ms: 0,
            max_remote_calls: 0,
            max_monetary_cost_minor_units: 0,
            currency: "USD".into(),
        },
        max_actions: 8,
        evaluation_currency: "USD".into(),
    }
}

fn env(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is required"))
}

fn invalid(what: &str) -> impl Fn(SearchError) -> String + '_ {
    move |error| format!("{what}: {error}")
}

async fn run() -> Result<(), String> {
    // `migrate`: the packaged Search runtime and Graph migrations, in the
    // order the durable tests use (after `document-server migrate`).
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&env("SEARCH_API_DATABASE_URL")?)
            .await
            .map_err(|_| "Search database unavailable".to_string())?;
        search_runtime::migrate(&pool)
            .await
            .map_err(|error| format!("Search migrations: {error}"))?;
        search_graph::migrate(&pool)
            .await
            .map_err(|error| format!("Graph migrations: {error}"))?;
        eprintln!("search-validation-host: Search and Graph migrations applied");
        return Ok(());
    }
    let source_file: SourceFile = serde_json::from_slice(
        &std::fs::read(env("SEARCH_WORKER_CONFIG")?)
            .map_err(|_| "SEARCH_WORKER_CONFIG cannot be read".to_string())?,
    )
    .map_err(|error| format!("Source file: {error}"))?;
    let actors_file: ActorsFile = serde_json::from_slice(
        &std::fs::read(env("SEARCH_VALIDATION_ACTORS")?)
            .map_err(|_| "SEARCH_VALIDATION_ACTORS cannot be read".to_string())?,
    )
    .map_err(|error| format!("actors file: {error}"))?;
    if !actors_file.bind.ip().is_loopback() {
        return Err("the validation host binds only a loopback address".into());
    }
    // Requests hold a connection for each access check; the pool bounds how
    // many run at once (SEARCH_API_POOL_SIZE, default 16).
    let pool_size = std::env::var("SEARCH_API_POOL_SIZE")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|size| (1..=256).contains(size))
        .unwrap_or(16);
    let pool = PgPoolOptions::new()
        .max_connections(pool_size)
        .connect(&env("SEARCH_API_DATABASE_URL")?)
        .await
        .map_err(|_| "Search database unavailable".to_string())?;

    let source_id = SourceId::from_uuid(source_file.source_id);
    let binding = DocumentAdapterRef::new(source_file.document_adapter_ref.clone())
        .map_err(invalid("adapter ref"))?;
    let adapter = ConfiguredAdapter {
        kinds: source_file.allowed_resource_kinds.clone(),
        modes: source_file.supported_modes.clone(),
        semantics: source_file.enumeration_semantics,
    };
    let witness = ConnectedDocumentAdapterWitness::from_connected_port(&adapter, &binding)
        .await
        .map_err(invalid("adapter"))?
        .ok_or("Document adapter is not connected")?;
    let registration = SourceRegistration::Document(
        DocumentSourceRegistration::from_server_config(
            ServerDocumentRegistrationConfig {
                tenant: TenantId::new(source_file.tenant.clone()).map_err(invalid("tenant"))?,
                source_id,
                document_adapter_ref: binding,
                allowed_resource_kinds: source_file.allowed_resource_kinds.clone(),
                supported_modes: source_file.supported_modes.clone(),
                enumeration_semantics: source_file.enumeration_semantics,
                retention_mode: RetentionMode::PersistentResource,
                registration_revision: RegistrationRevision::new(source_file.registration_revision)
                    .map_err(invalid("registration revision"))?,
                visibility_revision: VisibilityRevision::new(source_file.visibility_revision)
                    .map_err(invalid("visibility revision"))?,
            },
            &witness,
        )
        .map_err(invalid("registration"))?,
    );
    let host = Arc::new(ConfiguredHost {
        revision: RegistrationSetRevision::new(source_file.deployment_revision)
            .map_err(invalid("deployment revision"))?,
        document: registration,
    });

    let mut source = DiscoverableSource::new(
        source_id,
        source_file.source_name.clone(),
        source_file.enumeration_semantics,
        RetentionMode::PersistentResource,
    );
    source.discovery_modes = source_file.supported_modes.clone();
    source.resource_types = vec![source_file.lens.resource_type];

    let actors = Arc::new(Actors {
        tenant: source_file.tenant.clone(),
        granted: actors_file
            .actors
            .iter()
            .map(|actor| (actor.principal.clone(), actor.granted))
            .collect(),
        by_token: actors_file
            .actors
            .into_iter()
            .map(|actor| {
                (
                    actor.token,
                    Actor {
                        principal: actor.principal,
                        groups: actor.groups,
                        session: Uuid::now_v7(),
                    },
                )
            })
            .collect(),
        source: source_id,
        registration_revision: source_file.registration_revision,
        visibility_revision: source_file.visibility_revision,
        started: Instant::now(),
    });

    let vector_enabled = source_file
        .vector
        .as_ref()
        .is_some_and(|vector| vector.enabled);
    let mut model = DurableDocumentReadModel::new(pool.clone(), &source_file.lexical_root, source);
    if !vector_enabled {
        model = model.without_vector_units();
    }
    let mut ports = DurableDocumentPorts::new(
        Arc::new(model),
        Arc::new(DocumentAccess {
            source: source_id,
            pool: pool.clone(),
            repository: Arc::new(PostgresDocumentRepository::new(pool.clone())),
            actors: actors.clone(),
        }),
    );
    if let Some(vector) = source_file.vector.filter(|vector| vector.enabled) {
        let provider: Arc<dyn search_application::vector::EmbeddingProvider> = Arc::new(
            search_vector_adapter::CandleEmbeddingProvider::load(&vector.model_dir)
                .map_err(|error| format!("vector model: {error}"))?,
        );
        ports = ports.with_vector(VectorServices {
            activations: Arc::new(RegisteredVectorActivation::new(
                [source_id],
                provider.as_ref(),
            )),
            provider,
            index: Arc::new(PgVectorIndex::new(pool.clone(), vector.similarity_floor)),
            generations: Arc::new(PgVectorGenerations::new(pool.clone())),
        });
    }

    let runtime = build_search_api_runtime(
        SearchApiHostConfig {
            actors: actors.clone(),
            visibility: actors.clone(),
            registrations: host,
            claims: Some(Arc::new(InMemoryClaimCatalog::new(vec![]))),
            remote_transports: None,
            config: discovery_config(actors_file.profile),
            operation_timeout: Duration::from_secs(30),
            disclosure_ttl: Duration::from_secs(30),
        },
        SearchApiDurablePorts {
            pool,
            actor_ports: Some(Arc::new(ports)),
        },
        SearchApiIdentityScheme {
            credentials: Some(Arc::new(Credentials(actors))),
            auth: Some(SearchAuthSchemeBinding::bearer(Arc::new(
                StaticBearerChallenge,
            ))),
        },
    )
    .await
    .map_err(|error| format!("Search API runtime: {error:?}"))?;

    let listener = tokio::net::TcpListener::bind(actors_file.bind)
        .await
        .map_err(|_| "cannot bind the loopback address".to_string())?;
    eprintln!("search-validation-host: listening on {}", actors_file.bind);
    serve(
        listener,
        runtime.router(),
        ServeOptions {
            send_deadline: Duration::from_secs(60),
            send_buffer_bytes: None,
        },
        None,
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
    )
    .await
    .map_err(|_| "server stopped".to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("search-validation-host: {message}");
            ExitCode::FAILURE
        }
    }
}
