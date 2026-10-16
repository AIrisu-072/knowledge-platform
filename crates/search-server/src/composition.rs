//! Composes the production Search API runtime (`build_search_api_runtime`)
//! over the durable Document read model for one Document Source, with the
//! PoC identity of this process.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use document_application::{DocumentAccessCheckService, VerifiedActorContext};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef as DocumentPrincipal};
use document_repository_postgres::PostgresDocumentRepository;
use search_api_http::auth::{SearchAuthSchemeBinding, StaticBearerChallenge};
use search_api_http::router::ApiFuture;
use search_application::SearchError;
use search_application::api_scope::ApiError;
use search_application::discovery_service::{DiscoveryConfig, TemporalPolicy};
use search_application::materialization::ProbeBudget;
use search_application::ports::BoxFuture;
use search_application::retrieval::{RetrievalInputs, RetrieverProfile, RetrieverSupport};
use search_application::routing::RoutingConstraints;
use search_application::scoped::{
    RegistrationRevision, TenantId, TrustedSearchScope, VisibilityRevision,
};
use search_application::search_core::id::SourceId;
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
    SearchApiDurablePorts, SearchApiHostConfig, SearchApiIdentityScheme, SearchApiRuntime,
    build_search_api_runtime,
};
use search_runtime::durable_read::{
    DocumentActorAccess, DocumentActorAccessPort, DurableDocumentPorts, DurableDocumentReadModel,
};
use search_runtime::vector_runtime::{RegisteredVectorActivation, VectorServices};
use search_runtime::vector_store::{PgVectorGenerations, PgVectorIndex};
use search_source_document::{DocumentApiRead, DocumentCurrentAccessAdapter};
use serde::Deserialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::config::{RetrieverProfileName, ServeConfig};
use crate::identity::{Credentials, PocIdentity, PocProfile, Token};

/// The subset of the Search worker's Source file this server needs; other
/// worker fields are ignored so both processes read one file.
#[derive(Deserialize)]
pub struct SourceFile {
    pub tenant: String,
    pub source_id: Uuid,
    pub source_name: String,
    pub document_adapter_ref: String,
    pub deployment_revision: u64,
    pub registration_revision: u64,
    pub visibility_revision: u64,
    pub allowed_resource_kinds: Vec<ResourceKind>,
    pub supported_modes: Vec<DiscoveryMode>,
    pub enumeration_semantics: EnumerationSemantics,
    pub lens: DiscoveryLens,
    pub lexical_root: PathBuf,
    #[serde(default)]
    pub vector: Option<VectorFile>,
}

#[derive(Deserialize)]
pub struct VectorFile {
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub model_dir: PathBuf,
    #[serde(default = "floor")]
    pub similarity_floor: f32,
}

fn enabled() -> bool {
    true
}

fn floor() -> f32 {
    search_runtime::vector_runtime::DEFAULT_SIMILARITY_FLOOR
}

/// Maps the verified Search actor to its Document identity: the `poc` issuer,
/// principal and group the Document PoC runtime and its ACLs use.
struct DocumentAccess {
    profile: PocProfile,
    source: SourceId,
    pool: PgPool,
    repository: Arc<PostgresDocumentRepository>,
}

impl DocumentActorAccessPort for DocumentAccess {
    fn for_actor<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
    ) -> ApiFuture<'a, DocumentActorAccess> {
        Box::pin(async move {
            let principal = actor.principal().as_str();
            if principal != self.profile.principal() {
                return Err(ApiError::IdentityUnavailable);
            }
            let subjects = vec![
                PolicySubject::new(PolicySubjectKind::Principal, "poc", principal)
                    .map_err(|_| ApiError::IdentityUnavailable)?,
                PolicySubject::new(PolicySubjectKind::Group, "poc", self.profile.group())
                    .map_err(|_| ApiError::IdentityUnavailable)?,
            ];
            let document_actor = VerifiedActorContext::from_trusted_adapter(
                DocumentPrincipal::new("poc", principal)
                    .map_err(|_| ApiError::IdentityUnavailable)?,
                subjects,
                OffsetDateTime::now_utc() + time::Duration::minutes(5),
                self.profile.invocation(),
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

fn discovery_config(profile: RetrieverProfileName) -> DiscoveryConfig {
    DiscoveryConfig {
        routing: RoutingConstraints {
            required_source_ids: vec![],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
        retriever_profile: match profile {
            RetrieverProfileName::Exploratory => RetrieverProfile::Exploratory,
            RetrieverProfileName::Capability => RetrieverProfile::Capability,
            RetrieverProfileName::Knowledge => RetrieverProfile::Knowledge,
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

fn invalid(what: &str) -> impl Fn(SearchError) -> String + '_ {
    move |error| format!("{what}: {error}")
}

/// Builds the runtime of one serving process. Errors never include secrets.
pub async fn compose(serve: &ServeConfig, pool: PgPool) -> Result<SearchApiRuntime, String> {
    let source_file: SourceFile = serde_json::from_slice(
        &std::fs::read(&serve.source_config)
            .map_err(|_| "SEARCH_WORKER_CONFIG cannot be read".to_string())?,
    )
    .map_err(|error| format!("Source file: {error}"))?;
    let token = Token::read(&serve.token_file).map_err(|error| error.to_string())?;

    let source_id = SourceId::from_uuid(source_file.source_id);
    let tenant = TenantId::new(source_file.tenant.clone()).map_err(invalid("tenant"))?;
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
                tenant: tenant.clone(),
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

    let identity = Arc::new(PocIdentity::new(
        serve.profile,
        token,
        tenant,
        source_id,
        source_file.registration_revision,
        source_file.visibility_revision,
    ));

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
            profile: serve.profile,
            source: source_id,
            pool: pool.clone(),
            repository: Arc::new(PostgresDocumentRepository::new(pool.clone())),
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

    build_search_api_runtime(
        SearchApiHostConfig {
            actors: identity.clone(),
            visibility: identity.clone(),
            registrations: host,
            claims: Some(Arc::new(InMemoryClaimCatalog::new(vec![]))),
            remote_transports: None,
            config: discovery_config(serve.retriever_profile),
            operation_timeout: Duration::from_secs(30),
            disclosure_ttl: Duration::from_secs(30),
        },
        SearchApiDurablePorts {
            pool,
            actor_ports: Some(Arc::new(ports)),
        },
        SearchApiIdentityScheme {
            credentials: Some(Arc::new(Credentials(identity))),
            auth: Some(SearchAuthSchemeBinding::bearer(Arc::new(
                StaticBearerChallenge,
            ))),
        },
    )
    .await
    .map_err(|error| format!("Search API runtime: {error:?}"))
}
