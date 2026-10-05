//! P5-08: the production factory served on a real `127.0.0.1:0` socket.
//! Documents live in real PostgreSQL rows and real filesystem bytes and are
//! indexed by the existing outbox indexer with the real body extractor into
//! the in-memory index runtime; the remote Source is the synthetic catalog
//! over real TCP through the loopback-only test transport. Known human, LLM
//! and agent credentials become opaque sessions in the host verifier.

mod support;

#[path = "support/api.rs"]
mod api;
#[path = "../../search-source-document/tests/support/body.rs"]
mod body_support;
#[path = "../../search-source-document/tests/support/document_discovery.rs"]
mod discovery_support;
#[path = "../../search-source-http/tests/support/synthetic_catalog.rs"]
mod synthetic_catalog;

use std::collections::BTreeSet;
use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use api::{Host, Wire, discovery_config, durable, exchange, raw_request};
use document_application::{
    BootstrapRootPolicy, DocumentAccessCheckService, FileStorage, InvocationKind, StoreFileRequest,
    VerifiedActorContext,
};
use document_domain::{
    Action, FileId, MediaType, PolicyGrant, PolicySubject, PolicySubjectKind,
    PrincipalRef as DocumentPrincipal,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use document_storage_fs::FileSystemStorage;
use search_api_http::problem::ProblemCode;
use search_api_http::router::ApiFuture;
use search_api_http::send::{CloseReason, SendObserver, ServeOptions, serve};
use search_application::SearchError;
use search_application::api_scope::ApiError;
use search_application::indexing_service::{DocumentIndexingService, IndexingOutcome};
use search_application::ports::BoxFuture;
use search_application::remote_generation::REMOTE_CLAIM_SUBJECT;
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig,
};
use search_application::retrieval::{RetrievalInputs, RetrieverSupport};
use search_application::scoped::{
    RegistrationRevision, TenantId, TrustedSearchScope, VisibilityRevision,
};
use search_application::search_core::id::{ClaimId, SourceId};
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_application::source_registration::{
    ConnectedDocumentAdapterCapabilities, ConnectedDocumentAdapterWitness,
    DocumentAdapterCapabilityPort, DocumentAdapterRef, DocumentSourceRegistration,
    HostRegistrationSnapshotPort, RegistrationNamespace, RegistrationSetRevision,
    ServerDocumentRegistrationConfig, SourceRegistration, SyntheticHostRegistrationAuthority,
};
use search_application::visible_claim::ClaimDefinition;
use search_runtime::api::{
    ActorPorts, ActorPortsFactory, RemoteTransportFactory, SearchApiRuntime,
    build_search_api_runtime,
};
use search_source_document::{
    DocumentApiRead, DocumentBodyExtractor, DocumentCurrentAccessAdapter, DocumentEvidenceCatalog,
    DocumentOutboxIndexer, MemoryDocumentIndexRuntime, PostgresDocumentSnapshotReader,
};
use search_source_http::transport::{
    AddressResolver, GuardedHttpTransport, TransportFuture, TransportLimits,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokio::sync::oneshot;
use uuid::Uuid;

const REMOTE_BASE: &str = "/r/v1";

/// Builds one raw request for a measured route.
type RequestBytes<'a> = Box<dyn Fn() -> Vec<u8> + 'a>;
const PRINCIPALS: [&str; 3] = ["human", "llm", "agent"];

type Indexer = DocumentIndexingService<
    DocumentOutboxIndexer<
        PostgresDocumentSnapshotReader,
        discovery_support::Receipts,
        MemoryDocumentIndexRuntime,
    >,
>;

fn document_principal(principal: &str) -> DocumentPrincipal {
    DocumentPrincipal::new("test-idp", principal).unwrap()
}

fn subject(principal: &str) -> PolicySubject {
    PolicySubject::new(PolicySubjectKind::Principal, "test-idp", principal).unwrap()
}

fn editor() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        document_principal("editor"),
        vec![subject("editor")],
        OffsetDateTime::now_utc() + time::Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

struct Connected;

impl DocumentAdapterCapabilityPort for Connected {
    fn connected_capabilities<'a>(
        &'a self,
        binding: &'a DocumentAdapterRef,
    ) -> BoxFuture<'a, Option<ConnectedDocumentAdapterCapabilities>> {
        Box::pin(async move {
            Ok(Some(ConnectedDocumentAdapterCapabilities::new(
                binding.clone(),
                vec![ResourceKind::Knowledge],
                vec![
                    DiscoveryMode::LocalDirectory,
                    DiscoveryMode::LocalContentSearch,
                ],
                vec![EnumerationSemantics::Complete],
                vec![RetentionMode::PersistentResource],
            )?))
        })
    }
}

async fn document_registration(source_id: SourceId) -> SourceRegistration {
    let config = ServerDocumentRegistrationConfig {
        tenant: TenantId::new("tenant-a").unwrap(),
        source_id,
        document_adapter_ref: DocumentAdapterRef::new("document-binding").unwrap(),
        allowed_resource_kinds: vec![ResourceKind::Knowledge],
        supported_modes: vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
        ],
        enumeration_semantics: EnumerationSemantics::Complete,
        retention_mode: RetentionMode::PersistentResource,
        registration_revision: RegistrationRevision::new(1).unwrap(),
        visibility_revision: VisibilityRevision::new(1).unwrap(),
    };
    let witness = ConnectedDocumentAdapterWitness::from_connected_port(
        &Connected,
        &config.document_adapter_ref,
    )
    .await
    .unwrap()
    .unwrap();
    SourceRegistration::Document(
        DocumentSourceRegistration::from_server_config(config, &witness).unwrap(),
    )
}

fn remote_registration(source_id: SourceId, port: u16) -> SourceRegistration {
    SourceRegistration::Remote(
        RemoteSourceRegistration::from_server_config(ServerRemoteRegistrationConfig {
            tenant: TenantId::new("tenant-a").unwrap(),
            source_id,
            provider_kind: "synthetic".into(),
            endpoint: RegisteredEndpoint::new("http", "catalog.example.test", port, REMOTE_BASE)
                .unwrap(),
            supported_modes: vec![
                DiscoveryMode::RemoteEnumeration,
                DiscoveryMode::RemoteQuery,
                DiscoveryMode::DirectAddress,
                DiscoveryMode::LiveOnly,
            ],
            enumeration_semantics: EnumerationSemantics::Complete,
            authority_predicates: vec!["catalog.title".into()],
            allowed_resource_kinds: vec![ResourceKind::Knowledge],
            current_access_contract: CurrentAccessContract::PerItem,
            retention_mode: RetentionMode::NoRetention,
            freshness_policy: None,
            canonical_upstream_lineage: "synthetic-catalog".into(),
            limits: RemoteRegistrationLimits::synthetic_canary(),
            registration_revision: RegistrationRevision::new(1).unwrap(),
            visibility_revision: VisibilityRevision::new(1).unwrap(),
        })
        .unwrap(),
    )
}

/// The registered host name always resolves to the synthetic catalog.
struct Loopback(u16);

impl AddressResolver for Loopback {
    fn resolve<'a>(&'a self, _: &'a str, _: u16) -> TransportFuture<'a, Vec<SocketAddr>> {
        let port = self.0;
        Box::pin(async move { Ok(vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)]) })
    }
}

/// The explicit test-only loopback constructor; never the production one.
struct LoopbackTransports(u16);

impl RemoteTransportFactory for LoopbackTransports {
    fn transport(
        &self,
        registration: &RemoteSourceRegistration,
    ) -> Result<GuardedHttpTransport, SearchError> {
        GuardedHttpTransport::new_loopback_for_test(
            registration.endpoint().clone(),
            Arc::new(Loopback(self.0)),
            TransportLimits::from_registration(registration.limits()),
        )
    }
}

/// Document read ports for one verified actor: the Search session handle is
/// only the exact binding key of a session-scoped Document access adapter.
struct DocumentPorts {
    source: SourceId,
    pool: PgPool,
    repository: Arc<PostgresDocumentRepository>,
    index: MemoryDocumentIndexRuntime,
    /// From this many calls on, the actor's Document Read is gone (0: never).
    revoke_from_call: std::sync::atomic::AtomicUsize,
    calls: std::sync::atomic::AtomicUsize,
}

impl ActorPortsFactory for DocumentPorts {
    fn for_actor<'a>(&'a self, actor: &'a TrustedSearchScope) -> ApiFuture<'a, ActorPorts> {
        Box::pin(async move {
            use std::sync::atomic::Ordering;
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let revoke_from = self.revoke_from_call.load(Ordering::SeqCst);
            // A principal without the Document Read grant stands for a
            // revocation that happened after the earlier calls.
            let principal = if revoke_from != 0 && call >= revoke_from {
                "outsider"
            } else {
                actor.principal().as_str()
            };
            let document_actor = VerifiedActorContext::from_trusted_adapter(
                DocumentPrincipal::new("test-idp", principal)
                    .map_err(|_| ApiError::IdentityUnavailable)?,
                vec![
                    PolicySubject::new(PolicySubjectKind::Principal, "test-idp", principal)
                        .map_err(|_| ApiError::IdentityUnavailable)?,
                ],
                OffsetDateTime::now_utc() + time::Duration::minutes(10),
                if principal == "human" {
                    InvocationKind::HumanInteractive
                } else {
                    InvocationKind::Agent
                },
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
            let reader = self.index.projection_reader();
            let read = Arc::new(DocumentApiRead::new(
                self.source,
                self.pool.clone(),
                access.clone(),
                binding,
            ));
            Ok(ActorPorts {
                generations: Arc::new(reader.clone()),
                concepts: Arc::new(reader.clone()),
                assertions: Arc::new(reader.clone()),
                evidence: Arc::new(DocumentEvidenceCatalog::new(reader.clone())),
                directory: Some(Arc::new(reader.clone())),
                structured: Some(Arc::new(reader)),
                lexical: Some(Arc::new(self.index.lexical_reader())),
                hypergraph: None,
                graph_resource_access: None,
                access,
                resource_locator: read.clone(),
                resource_reader: read,
            })
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Opened(bool),
    Closed(CloseReason),
}

#[derive(Default)]
struct Events(Mutex<Vec<Event>>);

impl SendObserver for Events {
    fn opened(&self, evaluation_closed: bool) {
        self.0
            .lock()
            .unwrap()
            .push(Event::Opened(evaluation_closed));
    }
    fn closed(&self, reason: CloseReason) {
        self.0.lock().unwrap().push(Event::Closed(reason));
    }
}

impl Events {
    fn all(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }
    async fn wait_len(&self, count: usize) -> Vec<Event> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.all().len() < count && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.all()
    }
}

struct World {
    _guard: support::postgres::DatabaseGuard,
    pool: PgPool,
    files: PathBuf,
    storage: FileSystemStorage,
    host: Host,
    catalog: synthetic_catalog::Catalog,
    document: SourceId,
    remote: SourceId,
    document_claim: ClaimId,
    remote_claim: ClaimId,
    indexer: Indexer,
    addr: SocketAddr,
    events: Arc<Events>,
    ports: Arc<DocumentPorts>,
    _api: SearchApiRuntime,
    _stop: oneshot::Sender<()>,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.files);
    }
}

impl World {
    async fn start() -> Self {
        let (guard, pool, _) = support::postgres::postgres("search_server_e2e").await;
        document_repository_postgres::migrate(&pool).await.unwrap();
        search_runtime::migrate(&pool).await.unwrap();
        let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
            pool.clone(),
            document_principal("editor"),
        ));
        // The three roles may read every Document; "outsider" may not.
        repository
            .initialize_root_policy(
                &editor(),
                PRINCIPALS
                    .iter()
                    .chain(["reader"].iter())
                    .map(|principal| PolicyGrant::new(subject(principal), [Action::Read]).unwrap())
                    .collect(),
            )
            .await
            .unwrap();
        let files = std::env::temp_dir().join(format!("search-server-e2e-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&files).unwrap();
        let storage = FileSystemStorage::new(&files);

        let catalog = synthetic_catalog::Catalog::start().await;
        let document = SourceId::from_uuid(Uuid::now_v7());
        let remote = SourceId::from_uuid(Uuid::now_v7());
        let readers: Vec<&str> = PRINCIPALS.iter().copied().chain(["reader"]).collect();
        catalog.serve(
            REMOTE_BASE,
            synthetic_catalog::Collection::new(
                "tenant-a",
                &remote.as_uuid().to_string(),
                vec![
                    // One title: differing values of one Claim would conflict.
                    synthetic_catalog::Doc::new("r-1", "規程 R", &readers),
                    synthetic_catalog::Doc::new("r-2", "規程 R", &readers),
                ],
            ),
        );
        let registrations = Arc::new(SyntheticHostRegistrationAuthority::new());
        registrations
            .publish(
                RegistrationNamespace::Document,
                RegistrationSetRevision::new(1).unwrap(),
                vec![document_registration(document).await],
            )
            .unwrap();
        registrations
            .publish(
                RegistrationNamespace::Remote,
                RegistrationSetRevision::new(1).unwrap(),
                vec![remote_registration(remote, catalog.port())],
            )
            .unwrap();

        let index = MemoryDocumentIndexRuntime::new();
        let mut config = discovery_support::index_config(document);
        config.source.retention_mode = RetentionMode::PersistentResource;
        let indexer = DocumentIndexingService::new(
            DocumentOutboxIndexer::new(
                PostgresDocumentSnapshotReader::new(pool.clone()),
                config,
                index.clone(),
                discovery_support::Receipts::default(),
            )
            .with_body_extractor(Arc::new(DocumentBodyExtractor::new(
                document,
                storage.clone(),
                body_support::InProcessExtractor::new(body_support::Mode::Honest),
                body_support::registry(),
            ))),
        );

        let host = Host::new();
        let (document_claim, remote_claim) = (
            ClaimId::from_uuid(Uuid::now_v7()),
            ClaimId::from_uuid(Uuid::now_v7()),
        );
        host.claims
            .upsert(ClaimDefinition {
                claim_id: remote_claim,
                tenant: TenantId::new("tenant-a").unwrap(),
                source_id: remote,
                subject_ref: REMOTE_CLAIM_SUBJECT.into(),
                predicate: "catalog.title".into(),
                expected_value: None,
                revision: 1,
            })
            .unwrap();
        let snapshot: Arc<dyn HostRegistrationSnapshotPort> = registrations.clone();
        let ports = Arc::new(DocumentPorts {
            source: document,
            pool: pool.clone(),
            repository,
            index,
            revoke_from_call: std::sync::atomic::AtomicUsize::new(0),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let transports: Arc<dyn RemoteTransportFactory> =
            Arc::new(LoopbackTransports(catalog.port()));
        let api = build_search_api_runtime(
            host.config(
                snapshot,
                Some(transports),
                discovery_config(
                    RetrieverSupport {
                        remote_enumeration: true,
                        ..RetrieverSupport::default()
                    },
                    RetrievalInputs::default(),
                ),
            ),
            durable(&pool, ports.clone()),
            host.identity(),
        )
        .await
        .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let events = Arc::new(Events::default());
        let observer: Arc<dyn SendObserver> = events.clone();
        let (stop, stopped) = oneshot::channel::<()>();
        tokio::spawn(serve(
            listener,
            api.router(),
            ServeOptions {
                send_deadline: Duration::from_secs(20),
                send_buffer_bytes: None,
            },
            Some(observer),
            async {
                let _ = stopped.await;
            },
        ));
        Self {
            _guard: guard,
            pool,
            files,
            storage,
            host,
            catalog,
            document,
            remote,
            document_claim,
            remote_claim,
            indexer,
            addr,
            events,
            ports,
            _api: api,
            _stop: stop,
        }
    }

    /// One login per principal, each granted both Sources.
    fn login(&self, principal: &str) -> String {
        self.host.grants.grant("tenant-a", principal, self.document);
        self.host.grants.grant("tenant-a", principal, self.remote);
        self.host
            .login(&format!("{principal}-token"), "tenant-a", principal)
    }

    /// Publishes a current Version whose Parts are real files, then lets the
    /// outbox indexer build the next generation from the real rows.
    async fn publish(&self, title: &str, parts: &[(&str, &[u8])]) -> Uuid {
        let document_id = Uuid::now_v7();
        let version_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) \
             VALUES ($1,$2,NULL,1,'{}'::jsonb,now())",
        )
        .bind(document_id)
        .bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state, \
             title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) \
             VALUES ($1,$2,1,'PUBLISHED',$3,now(),'test-idp','editor','{}'::jsonb,now())",
        )
        .bind(version_id)
        .bind(document_id)
        .bind(title)
        .execute(&self.pool)
        .await
        .unwrap();
        for (ordinal, (media_type, bytes)) in parts.iter().enumerate() {
            let file_id = FileId::from_uuid(Uuid::now_v7());
            let stored = self
                .storage
                .put_immutable(StoreFileRequest::new(
                    file_id,
                    Box::pin(Cursor::new(bytes.to_vec())),
                    MediaType::new(*media_type).unwrap(),
                ))
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) \
                 VALUES ($1,$2,$3,$4,$5,now())",
            )
            .bind(file_id.as_uuid())
            .bind(stored.content_hash().as_bytes().to_vec())
            .bind(*media_type)
            .bind(stored.size_bytes().get())
            .bind(stored.storage_key().as_str())
            .execute(&self.pool)
            .await
            .unwrap();
            let (item, representation) = (Uuid::now_v7(), Uuid::now_v7());
            let mut tx = self.pool.begin().await.unwrap();
            sqlx::query(
                "INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) \
                 VALUES ($1,$2,$3,$4,$5)",
            )
            .bind(item)
            .bind(version_id)
            .bind(format!("body/part-{ordinal}"))
            .bind(ordinal as i32)
            .bind(representation)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) \
                 VALUES ($1,$2,$3,'AUTHORITATIVE','source')",
            )
            .bind(representation)
            .bind(item)
            .bind(file_id.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
            tx.commit().await.unwrap();
        }
        sqlx::query(
            "UPDATE documents SET current_version_id=$1, revision=revision+1 WHERE document_id=$2",
        )
        .bind(version_id)
        .bind(document_id)
        .execute(&self.pool)
        .await
        .unwrap();
        match self
            .indexer
            .handle(discovery_support::event(
                "DocumentVersionPublished",
                document_id,
            ))
            .await
            .unwrap()
        {
            IndexingOutcome::Published(_) => {}
            other => panic!("{other:?}"),
        }
        version_id
    }

    /// A Claim on one Document Version's title, owned by the server.
    fn claim_title(&self, version: Uuid) {
        self.host
            .claims
            .upsert(ClaimDefinition {
                claim_id: self.document_claim,
                tenant: TenantId::new("tenant-a").unwrap(),
                source_id: self.document,
                subject_ref: format!("document-version:{version}"),
                predicate: "document.title".into(),
                expected_value: None,
                revision: 1,
            })
            .unwrap();
    }

    async fn post(&self, path: &str, token: Option<&str>, body: &Value) -> Wire {
        exchange(
            self.addr,
            &raw_request("POST", path, token, &body.to_string()),
        )
        .await
    }

    async fn get(&self, path: &str, token: Option<&str>) -> Wire {
        exchange(self.addr, &raw_request("GET", path, token, "")).await
    }
}

fn search(query: &str, coverage: &str) -> Value {
    json!({"query": query, "coverage": coverage})
}

fn discover(claims: &[ClaimId]) -> Value {
    json!({
        "need": {
            "purpose": "find the current rule",
            "requiredResourceTypes": ["knowledge"],
            "requiredClaimIds": claims.iter().map(|claim| claim.as_uuid().to_string()).collect::<Vec<_>>(),
        },
        "coverage": "titleAndPermittedMetadata"
    })
}

fn ids(value: &Value, list: &str, field: &str) -> BTreeSet<String> {
    value[list]
        .as_array()
        .unwrap_or_else(|| panic!("{value}"))
        .iter()
        .map(|item| item[field].as_str().unwrap().to_owned())
        .collect()
}

/// (Claim, Source, value) of every supported evidence entry. Candidates of
/// other visible Sources cannot evaluate a Source-owned Claim, so a union
/// Discover is judged by its supported evidence, not by sufficiency.
fn supported(value: &Value) -> BTreeSet<(String, String, String)> {
    value["evidence"]
        .as_array()
        .unwrap_or_else(|| panic!("{value}"))
        .iter()
        .filter(|entry| entry["state"] == "supported")
        .map(|entry| {
            (
                entry["claimId"].as_str().unwrap().to_owned(),
                entry["sourceId"].as_str().unwrap_or_default().to_owned(),
                entry["value"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn fact(claim: ClaimId, source: SourceId, value: &str) -> (String, String, String) {
    (
        claim.as_uuid().to_string(),
        source.as_uuid().to_string(),
        value.into(),
    )
}

fn set(values: &[String]) -> BTreeSet<String> {
    values.iter().cloned().collect()
}

fn problem(wire: &Wire, code: ProblemCode) {
    let (status, code_text, title, detail) = code.registry();
    let body = wire.json();
    assert_eq!(wire.status, status, "{body}");
    assert_eq!(
        wire.header("content-type"),
        Some("application/problem+json")
    );
    assert_eq!(wire.header("cache-control"), Some("private, no-store"));
    assert_eq!(body["code"], code_text);
    assert_eq!(body["title"], title);
    assert_eq!(body["detail"], detail);
}

#[tokio::test]
async fn human_llm_agent_same_four_routes_one_core() {
    let world = World::start().await;
    let version = world
        .publish("規程 共通", &[("text/plain", "共通 本文".as_bytes())])
        .await;
    world.claim_title(version);
    let sessions: Vec<String> = PRINCIPALS
        .iter()
        .map(|principal| world.login(principal))
        .collect();
    let mut seen = Vec::new();
    for principal in PRINCIPALS {
        let token = format!("{principal}-token");
        let token = Some(token.as_str());
        let found = world
            .post(
                "/v1/search",
                token,
                &search("規程", "titleAndPermittedMetadata"),
            )
            .await;
        assert_eq!(found.status, 200, "{}", found.json());
        let evaluated = world
            .post(
                "/v1/discover",
                token,
                &discover(&[world.document_claim, world.remote_claim]),
            )
            .await;
        assert_eq!(evaluated.status, 200, "{}", evaluated.json());
        let read = world.get(&format!("/v1/resources/{version}"), token).await;
        assert_eq!(read.status, 200, "{}", read.json());
        let sources = world.get("/v1/sources", token).await;
        assert_eq!(sources.status, 200, "{}", sources.json());
        seen.push((
            ids(&found.json(), "items", "resourceId"),
            ids(&evaluated.json(), "qualifiedResources", "sourceId"),
            supported(&evaluated.json()),
            read.json()["title"].clone(),
            ids(&sources.json(), "items", "sourceId"),
        ));
    }
    // One core: every role sees the same visible-set answer.
    assert!(seen.windows(2).all(|pair| pair[0] == pair[1]), "{seen:?}");
    let (items, qualified_sources, facts, title, sources) = &seen[0];
    assert_eq!(items, &set(&[version.to_string()]));
    assert_eq!(
        qualified_sources,
        &set(&[
            world.document.as_uuid().to_string(),
            world.remote.as_uuid().to_string()
        ])
    );
    assert!(facts.contains(&fact(world.document_claim, world.document, "規程 共通")));
    assert!(facts.contains(&fact(world.remote_claim, world.remote, "規程 R")));
    assert_eq!(title, "規程 共通");
    assert_eq!(
        sources,
        &set(&[
            world.document.as_uuid().to_string(),
            world.remote.as_uuid().to_string()
        ])
    );
    // Each credential is its own opaque session.
    world.host.sessions.close(&sessions[0]);
    problem(
        &world.get("/v1/sources", Some("human-token")).await,
        ProblemCode::AuthenticationRequired,
    );
    assert_eq!(
        world.get("/v1/sources", Some("llm-token")).await.status,
        200
    );
    assert_eq!(
        world.get("/v1/sources", Some("agent-token")).await.status,
        200
    );
}

#[tokio::test]
async fn document_real_db_fs_and_remote_real_tcp_search_discover_get_sources() {
    let world = World::start().await;
    let first = world
        .publish("規程 A", &[("text/plain", "第一 本文".as_bytes())])
        .await;
    let second = world
        .publish("規程 B", &[("text/plain", "第二 本文".as_bytes())])
        .await;
    world.login("reader");
    let token = Some("reader-token");

    // Search: the durable Document generation built from the real rows.
    let found = world
        .post(
            "/v1/search",
            token,
            &search("規程", "titleAndPermittedMetadata"),
        )
        .await;
    assert_eq!(found.status, 200, "{}", found.json());
    let body = found.json();
    assert_eq!(
        ids(&body, "items", "resourceId"),
        set(&[first.to_string(), second.to_string()])
    );
    assert_eq!(
        ids(&body, "items", "sourceId"),
        set(&[world.document.as_uuid().to_string()])
    );
    assert_eq!(
        ids(&body, "items", "title"),
        set(&["規程 A".into(), "規程 B".into()])
    );

    // Discover: the Document Claim from the generation, the remote Claim
    // from the synthetic catalog over a real socket.
    world.claim_title(first);
    let evaluated = world
        .post("/v1/discover", token, &discover(&[world.document_claim]))
        .await;
    assert_eq!(evaluated.status, 200, "{}", evaluated.json());
    assert!(
        supported(&evaluated.json()).contains(&fact(
            world.document_claim,
            world.document,
            "規程 A"
        )),
        "{}",
        evaluated.json()
    );
    assert!(
        ids(&evaluated.json(), "qualifiedResources", "resourceId").contains(&first.to_string())
    );
    let before = world.catalog.requests();
    let remote = world
        .post("/v1/discover", token, &discover(&[world.remote_claim]))
        .await;
    assert_eq!(remote.status, 200, "{}", remote.json());
    assert!(
        supported(&remote.json()).contains(&fact(world.remote_claim, world.remote, "規程 R")),
        "{}",
        remote.json()
    );
    assert!(
        ids(&remote.json(), "qualifiedResources", "sourceId")
            .contains(&world.remote.as_uuid().to_string())
    );
    assert!(world.catalog.requests() > before);

    // Get: the current Version from the real rows.
    let read = world.get(&format!("/v1/resources/{first}"), token).await;
    assert_eq!(read.status, 200, "{}", read.json());
    assert_eq!(read.json()["title"], "規程 A");
    assert_eq!(
        read.json()["sourceId"],
        world.document.as_uuid().to_string()
    );
    assert_eq!(read.json()["resourceVersionId"], first.to_string());

    // Sources: both namespaces of the union catalog.
    let sources = world.get("/v1/sources", token).await;
    assert_eq!(
        ids(&sources.json(), "items", "sourceId"),
        set(&[
            world.document.as_uuid().to_string(),
            world.remote.as_uuid().to_string()
        ])
    );
}

#[tokio::test]
async fn body_positive_partial_negative_archive_mixed_leaf() {
    let world = World::start().await;
    let positive = world
        .publish("規程 本文", &[("text/plain", "本文 alpha".as_bytes())])
        .await;
    let archive = world
        .publish(
            "規程 書庫",
            &[(
                "application/zip",
                &body_support::zip(&[
                    ("notes.txt", "beta\n".as_bytes()),
                    ("table.csv", "k,v\nalpha,1\n".as_bytes()),
                ]),
            )],
        )
        .await;
    let partial = world
        .publish(
            "規程 一部",
            &[
                ("text/plain", "alpha partial".as_bytes()),
                ("application/octet-stream", &[0_u8, 1, 2, 3]),
            ],
        )
        .await;
    let negative = world
        .publish("規程 対象外", &[("text/plain", "gamma only".as_bytes())])
        .await;
    world.login("reader");
    let token = Some("reader-token");

    let body = world
        .post("/v1/search", token, &search("alpha", "bodyRequired"))
        .await;
    assert_eq!(body.status, 200, "{}", body.json());
    let json = body.json();
    let found = ids(&json, "items", "resourceId");
    assert_eq!(
        found,
        set(&[
            positive.to_string(),
            archive.to_string(),
            partial.to_string()
        ])
    );
    assert!(!found.contains(&negative.to_string()));
    for item in json["items"].as_array().unwrap() {
        assert_eq!(item["matchedFields"], json!(["body"]), "{item}");
    }
    let coverage: Vec<(String, String)> = json["coverage"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["sourceId"].as_str().unwrap().to_owned(),
                entry["kind"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(coverage.contains(&(
        world.document.as_uuid().to_string(),
        "bodySearchWithPerItemCoverage".into()
    )));
    // The remote Source executed no body search and claims none.
    assert!(coverage.contains(&(
        world.remote.as_uuid().to_string(),
        "titleAndPermittedMetadata".into()
    )));

    // The same word in no title: metadata coverage finds nothing.
    let titles = world
        .post(
            "/v1/search",
            token,
            &search("alpha", "titleAndPermittedMetadata"),
        )
        .await;
    assert_eq!(titles.status, 200);
    assert!(
        titles.json()["items"].as_array().unwrap().is_empty(),
        "{}",
        titles.json()
    );
}

#[tokio::test]
async fn sourcepage_overflow_503_and_hidden_resource_generic_404() {
    let world = World::start().await;
    let version = world
        .publish("規程 秘", &[("text/plain", "内部".as_bytes())])
        .await;
    world.login("reader");
    // Granted both Sources by Search, but no Document Read authorization.
    world.login("outsider");
    let reader = Some("reader-token");

    // An unstamped union catalog never truncates: overflow is a 503.
    problem(
        &world.get("/v1/sources?pageSize=1", reader).await,
        ProblemCode::DependencyUnavailable,
    );
    assert_eq!(
        world.get("/v1/sources?pageSize=2", reader).await.status,
        200
    );

    assert_eq!(
        world
            .get(&format!("/v1/resources/{version}"), reader)
            .await
            .status,
        200
    );
    let hidden = world
        .get(&format!("/v1/resources/{version}"), Some("outsider-token"))
        .await;
    let absent = world
        .get(&format!("/v1/resources/{}", Uuid::new_v4()), reader)
        .await;
    for wire in [&hidden, &absent] {
        problem(wire, ProblemCode::ResourceNotFound);
    }
    let strip = |wire: &Wire| {
        let mut body = wire.json();
        body.as_object_mut().unwrap().remove("trace_id");
        body
    };
    assert_eq!(strip(&hidden), strip(&absent));
    // A Source grant revoked after login hides the same Resource the same way.
    world.host.grants.revoke("reader", world.document);
    let revoked = world.get(&format!("/v1/resources/{version}"), reader).await;
    problem(&revoked, ProblemCode::ResourceNotFound);
    assert_eq!(strip(&revoked), strip(&absent));
}

#[tokio::test]
async fn auth_challenge_and_problem_oas_parity() {
    let world = World::start().await;
    let session = world.login("reader");
    let missing = world.get("/v1/sources", None).await;
    let unknown = world.get("/v1/sources", Some("unknown-token")).await;
    world.host.sessions.close(&session);
    let closed = world.get("/v1/sources", Some("reader-token")).await;
    for wire in [&missing, &unknown, &closed] {
        problem(wire, ProblemCode::AuthenticationRequired);
        assert_eq!(
            wire.header("www-authenticate"),
            Some("Bearer realm=\"search\"")
        );
    }
    // The registry the server answers with is the one the contract lists.
    let oas = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/api/search-openapi.yaml"),
    )
    .unwrap();
    for code in ProblemCode::ALL {
        let (status, code_text, title, detail) = code.registry();
        for value in [code_text, title, detail] {
            assert!(oas.contains(&format!("- {value}")), "{value}");
        }
        let start = oas
            .find(&format!("\n    Problem{status}:"))
            .unwrap_or_else(|| panic!("Problem{status}"));
        let block_end = oas[start + 1..]
            .find("\n    Problem")
            .map_or(oas.len(), |offset| start + 1 + offset);
        assert!(
            oas[start..block_end].contains(&format!("- {code_text}")),
            "{code_text} in Problem{status}"
        );
    }
    assert!(oas.contains("WWW-Authenticate"));
}

#[tokio::test]
async fn lease_send_finish_and_no_retention() {
    let world = World::start().await;
    world
        .publish("規程 A", &[("text/plain", "本文".as_bytes())])
        .await;
    world.login("reader");
    let token = Some("reader-token");
    let before = world.events.all().len();
    let first_calls = world.catalog.requests();
    let first = world
        .post("/v1/discover", token, &discover(&[world.remote_claim]))
        .await;
    assert_eq!(first.status, 200, "{}", first.json());
    // The NO_RETENTION evaluation closed before any public byte, and the
    // disclosure lease closed only after the whole response was sent.
    let events = world.events.wait_len(before + 2).await;
    assert_eq!(
        &events[before..],
        &[Event::Opened(true), Event::Closed(CloseReason::Completed)]
    );
    // Nothing of the remote answer is retained: the next Discover asks the
    // Source again, and Search over durable generations never shows it.
    let between = world.catalog.requests();
    assert!(between > first_calls);
    let second = world
        .post("/v1/discover", token, &discover(&[world.remote_claim]))
        .await;
    assert_eq!(second.status, 200);
    assert!(world.catalog.requests() > between);
    let found = world
        .post(
            "/v1/search",
            token,
            &search("規程", "titleAndPermittedMetadata"),
        )
        .await;
    assert_eq!(found.status, 200);
    assert!(!ids(&found.json(), "items", "sourceId").contains(&world.remote.as_uuid().to_string()));
}

/// G1 capacity sample, not a CI gate: sequential requests per route on the
/// real socket against 24 indexed Documents and the 2-item remote catalog.
/// Run optimized with
/// `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo test --release -p search-runtime --features synthetic-loopback-test-only --test server_e2e -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "capacity measurement; run explicitly"]
async fn measure_route_latency() {
    const SAMPLES: usize = 200;
    let world = World::start().await;
    let mut first = None;
    for n in 0..24 {
        let version = world
            .publish(
                &format!("規程 {n}"),
                &[("text/plain", format!("本文 {n} alpha").as_bytes())],
            )
            .await;
        first.get_or_insert(version);
    }
    world.claim_title(first.unwrap());
    world.login("reader");
    let token = Some("reader-token");
    let routes: Vec<(&str, RequestBytes)> = vec![
        (
            "search",
            Box::new(|| {
                raw_request(
                    "POST",
                    "/v1/search",
                    token,
                    &search("規程", "titleAndPermittedMetadata").to_string(),
                )
            }),
        ),
        (
            "search_body",
            Box::new(|| {
                raw_request(
                    "POST",
                    "/v1/search",
                    token,
                    &search("alpha", "bodyRequired").to_string(),
                )
            }),
        ),
        (
            "discover_remote",
            Box::new(|| {
                raw_request(
                    "POST",
                    "/v1/discover",
                    token,
                    &discover(&[world.remote_claim]).to_string(),
                )
            }),
        ),
        (
            "resource",
            Box::new(|| {
                raw_request(
                    "GET",
                    &format!("/v1/resources/{}", first.unwrap()),
                    token,
                    "",
                )
            }),
        ),
        (
            "sources",
            Box::new(|| raw_request("GET", "/v1/sources", token, "")),
        ),
    ];
    for (name, request) in routes {
        for _ in 0..10 {
            assert_eq!(exchange(world.addr, &request()).await.status, 200);
        }
        let mut samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let started = Instant::now();
            let wire = exchange(world.addr, &request()).await;
            samples.push(started.elapsed().as_secs_f64() * 1_000.0);
            assert_eq!(wire.status, 200, "{name}");
        }
        samples.sort_by(f64::total_cmp);
        let at =
            |q: f64| samples[((samples.len() as f64 * q).ceil() as usize).min(samples.len()) - 1];
        println!(
            "route={name} n={SAMPLES} p50_ms={:.2} p95_ms={:.2} p99_ms={:.2} max_ms={:.2}",
            at(0.50),
            at(0.95),
            at(0.99),
            samples[samples.len() - 1]
        );
    }
}

#[tokio::test]
async fn item_revoked_before_disclosure_is_never_sent() {
    use std::sync::atomic::Ordering;
    let world = World::start().await;
    let version = world
        .publish("規程 取消", &[("text/plain", "本文".as_bytes())])
        .await;
    world.login("reader");
    let token = Some("reader-token");
    assert_eq!(
        ids(
            &world
                .post(
                    "/v1/search",
                    token,
                    &search("規程", "titleAndPermittedMetadata")
                )
                .await
                .json(),
            "items",
            "resourceId"
        ),
        set(&[version.to_string()])
    );
    // The route's evaluation still sees the item; the Read is gone by the
    // final gate, so nothing of the item is sent.
    let next = world.ports.calls.load(Ordering::SeqCst) + 2;
    world.ports.revoke_from_call.store(next, Ordering::SeqCst);
    let revoked = world
        .post(
            "/v1/search",
            token,
            &search("規程", "titleAndPermittedMetadata"),
        )
        .await;
    assert_ne!(revoked.status, 200, "{}", revoked.json());
    assert!(!String::from_utf8_lossy(&revoked.body).contains(&version.to_string()));
    let next = world.ports.calls.load(Ordering::SeqCst) + 2;
    world.ports.revoke_from_call.store(next, Ordering::SeqCst);
    let read = world.get(&format!("/v1/resources/{version}"), token).await;
    assert_ne!(read.status, 200);
    assert!(!String::from_utf8_lossy(&read.body).contains("規程 取消"));
}
