//! P6-I04: the Search outbox worker process.
//!
//! Secrets come only from the environment (`SEARCH_DELIVERY_DATABASE_URL`
//! for the generic outbox role, `SEARCH_COMPLETION_DATABASE_URL` for the
//! Search roles); the trusted host composition comes from the JSON file named
//! by `SEARCH_WORKER_CONFIG`. Startup verifies the schema, collects expired
//! builds and re-verifies the current key before any claim; an unusable
//! current refuses to start. SIGTERM or Ctrl-C stops claiming and drains.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use document_storage_fs::FileSystemStorage;
use outbox_delivery::{DeliveryConfig, DeliveryPolicy};
use search_application::SearchError;
use search_application::ports::{BoxFuture, SemanticRegistrySnapshot};
use search_application::scoped::{RegistrationRevision, TenantId, VisibilityRevision};
use search_application::search_core::id::SourceId;
use search_application::search_core::knowledge_unit::ExtractionProfileDefinitionV1;
use search_application::search_core::profile::DiscoveryLens;
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{
    DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode,
};
use search_application::source_registration::{
    CompleteDesiredRegistrations, ConnectedDocumentAdapterCapabilities,
    ConnectedDocumentAdapterWitness, DocumentAdapterCapabilityPort, DocumentAdapterRef,
    DocumentSourceRegistration, HostRegistrationSnapshot, HostRegistrationSnapshotPort,
    RegistrationNamespace, RegistrationSetRevision, ServerDocumentRegistrationConfig,
    SourceRegistration, SourceRegistrationLedgerPort,
};
use search_extraction_runner::{SearchExtractionRunner, SearchRunnerConfig};
use search_runtime::full_guard::FullGuardTtl;
use search_runtime::recovery::{CurrentState, PgStartupRecovery};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use search_runtime::worker::{SearchWorkerConfig, compose};
use search_source_document::{BodyProfileRegistry, DocumentBodyExtractor};
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerFile {
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
    projection_schema_version: String,
    analyzer_version: String,
    semantic_registry_version: String,
    lexical_root: PathBuf,
    file_root: PathBuf,
    extraction_worker: PathBuf,
    parser_build_id: String,
    profiles: Vec<ExtractionProfileDefinitionV1>,
    source_lease_ms: u64,
    guard_ttl_ms: u64,
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

/// The complete host inventory of this worker: one Document Source and no
/// Remote Sources, at the configured deployment revision.
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

fn env(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is required"))
}

fn invalid(what: &str) -> impl Fn(SearchError) -> String + '_ {
    move |error| format!("{what}: {error}")
}

async fn run() -> Result<(), String> {
    let file = std::fs::read(env("SEARCH_WORKER_CONFIG")?)
        .map_err(|_| "SEARCH_WORKER_CONFIG cannot be read".to_string())?;
    let config: WorkerFile =
        serde_json::from_slice(&file).map_err(|error| format!("worker config: {error}"))?;
    let delivery_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&env("SEARCH_DELIVERY_DATABASE_URL")?)
        .await
        .map_err(|_| "delivery database unavailable".to_string())?;
    let search_pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&env("SEARCH_COMPLETION_DATABASE_URL")?)
        .await
        .map_err(|_| "Search database unavailable".to_string())?;

    let source_id = SourceId::from_uuid(config.source_id);
    let binding =
        DocumentAdapterRef::new(config.document_adapter_ref).map_err(invalid("adapter ref"))?;
    let adapter = ConfiguredAdapter {
        kinds: config.allowed_resource_kinds.clone(),
        modes: config.supported_modes.clone(),
        semantics: config.enumeration_semantics,
    };
    let witness = ConnectedDocumentAdapterWitness::from_connected_port(&adapter, &binding)
        .await
        .map_err(invalid("adapter"))?
        .ok_or("Document adapter is not connected")?;
    let registration = SourceRegistration::Document(
        DocumentSourceRegistration::from_server_config(
            ServerDocumentRegistrationConfig {
                tenant: TenantId::new(config.tenant).map_err(invalid("tenant"))?,
                source_id,
                document_adapter_ref: binding,
                allowed_resource_kinds: config.allowed_resource_kinds,
                supported_modes: config.supported_modes.clone(),
                enumeration_semantics: config.enumeration_semantics,
                retention_mode: RetentionMode::PersistentResource,
                registration_revision: RegistrationRevision::new(config.registration_revision)
                    .map_err(invalid("registration revision"))?,
                visibility_revision: VisibilityRevision::new(config.visibility_revision)
                    .map_err(invalid("visibility revision"))?,
            },
            &witness,
        )
        .map_err(invalid("registration"))?,
    );
    let host = Arc::new(ConfiguredHost {
        revision: RegistrationSetRevision::new(config.deployment_revision)
            .map_err(invalid("deployment revision"))?,
        document: registration.clone(),
    });
    let ledger = PgSourceRegistrationLedger::new(search_pool.clone(), host.clone());
    for namespace in [
        RegistrationNamespace::Remote,
        RegistrationNamespace::Document,
    ] {
        let desired = CompleteDesiredRegistrations::capture(host.as_ref(), namespace)
            .await
            .map_err(invalid("host inventory"))?;
        ledger
            .reconcile(&desired)
            .await
            .map_err(invalid("ledger"))?;
    }
    let desired =
        CompleteDesiredRegistrations::capture(host.as_ref(), RegistrationNamespace::Document)
            .await
            .map_err(invalid("host inventory"))?;
    let activation = *ledger
        .reconcile(&desired)
        .await
        .map_err(invalid("ledger"))?
        .get(&source_id)
        .ok_or("configured Source is not active")?;

    let mut source = DiscoverableSource::new(
        source_id,
        config.source_name,
        config.enumeration_semantics,
        RetentionMode::PersistentResource,
    );
    source.discovery_modes = config.supported_modes;
    source.resource_types = vec![config.lens.resource_type];

    // No claim before the schema, expired builds and current are settled.
    let report = PgStartupRecovery::new(search_pool.clone(), &config.lexical_root, source.clone())
        .startup(64)
        .await
        .map_err(|error| format!("startup recovery: {error:?}"))?;
    if let CurrentState::Unusable(_, error) = report.current {
        return Err(format!("current generation is unusable: {error:?}"));
    }

    let registry = BodyProfileRegistry::new(config.parser_build_id, config.profiles.clone())
        .map_err(|error| format!("extraction profiles: {error:?}"))?;
    let profiles = config
        .profiles
        .into_iter()
        .filter_map(|definition| {
            search_extraction_core::RegisteredProfile::register_definition(definition).ok()
        })
        .collect();
    let runner =
        SearchExtractionRunner::new(SearchRunnerConfig::new(config.extraction_worker), profiles)
            .map_err(|error| format!("extraction runner: {error:?}"))?;
    let extractor = Arc::new(DocumentBodyExtractor::new(
        source_id,
        FileSystemStorage::new(&config.file_root),
        runner,
        registry,
    ));
    let worker = compose(
        delivery_pool,
        search_pool,
        &ledger,
        extractor,
        SearchWorkerConfig {
            registration,
            activation,
            source,
            lens: config.lens,
            projection_schema_version: config.projection_schema_version,
            analyzer_version: config.analyzer_version,
            semantic_registry: SemanticRegistrySnapshot::new(config.semantic_registry_version),
            lexical_root: config.lexical_root,
            delivery: DeliveryConfig::default(),
            policy: DeliveryPolicy::default(),
            source_lease: Duration::from_millis(config.source_lease_ms),
            guard_ttl: FullGuardTtl::new(Duration::from_millis(config.guard_ttl_ms))
                .ok_or("guard TTL must be positive and at most 120 s")?,
        },
    )
    .map_err(|error| format!("worker composition: {error:?}"))?;

    let (stop, stopped) = watch::channel(false);
    tokio::spawn(async move {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async {
                match terminate.as_mut() {
                    Some(signal) => { signal.recv().await; }
                    None => std::future::pending::<()>().await,
                }
            } => {}
        }
        let _ = stop.send(true);
    });
    worker
        .run_until_shutdown(stopped)
        .await
        .map(|_| ())
        .map_err(|error| format!("worker stopped: {error}"))
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("search_outbox_worker: {message}");
            ExitCode::FAILURE
        }
    }
}
