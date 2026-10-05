//! P3-D01: the existing Document outbox indexer on the durable runtime. One
//! real Document Source snapshot becomes a READY, published P7 bundle whose
//! Graph rows carry the snapshot-bound Document mapping; a restarted runtime
//! with no RAM state sees the same current key and leaves it unchanged.

#[path = "../../search-source-document/tests/support/body.rs"]
mod body_support;
#[path = "../../search-source-document/tests/support/document_discovery.rs"]
mod discovery_support;
#[path = "support/registration.rs"]
mod registration;
mod support;

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use document_application::{
    BootstrapRootPolicy, DocumentAccessCheckService, FileStorage, InvocationKind, StoreFileRequest,
    VerifiedActorContext,
};
use document_domain::{Action, FileId, MediaType, PolicyGrant, PolicySubject, PolicySubjectKind};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use document_storage_fs::FileSystemStorage;
use search_application::graph_generation::GraphSourceMapping;
use search_application::indexing_service::{DocumentIndexingService, IndexingOutcome};
use search_application::ports::AccessDecision;
use search_application::search_core::id::{ResourceId, SourceId};
use search_application::search_core::projection::ProjectionGenerationKey;
use search_application::search_core::source::RetentionMode;
use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_graph::PostgresGraphStore;
use search_runtime::document_runtime::PgDocumentIndexRuntime;
use search_runtime::full_guard::FullGuardTtl;
use search_runtime::recovery::{CurrentState, PgStartupRecovery};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use search_source_document::{
    BodyItemExtractor, DocumentBodyExtractor, DocumentCurrentAccessAdapter,
    DocumentGenerationAccess, DocumentOutboxIndexer, PostgresDocumentSnapshotReader,
};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

fn actor() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        document_domain::PrincipalRef::new("test-idp", "editor").unwrap(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap()],
        OffsetDateTime::now_utc() + time::Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

/// Publishes one Document with a plain-text body part as its current Version.
async fn publish(pool: &PgPool, storage: &FileSystemStorage, text: &str) -> Uuid {
    let file_id = FileId::from_uuid(Uuid::now_v7());
    let stored = storage
        .put_immutable(StoreFileRequest::new(
            file_id,
            Box::pin(Cursor::new(text.as_bytes().to_vec())),
            MediaType::new("text/plain").unwrap(),
        ))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) \
         VALUES ($1,$2,'text/plain',$3,$4,now())",
    )
    .bind(file_id.as_uuid())
    .bind(stored.content_hash().as_bytes().to_vec())
    .bind(stored.size_bytes().get())
    .bind(stored.storage_key().as_str())
    .execute(pool)
    .await
    .unwrap();
    let document = Uuid::now_v7();
    let version = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) \
         VALUES ($1,$2,NULL,1,'{}'::jsonb,now())",
    )
    .bind(document)
    .bind(SYSTEM_ROOT_FOLDER_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state, \
         title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) \
         VALUES ($1,$2,1,'PUBLISHED','規程',now(),'test-idp','editor','{}'::jsonb,now())",
    )
    .bind(version)
    .bind(document)
    .execute(pool)
    .await
    .unwrap();
    let item = Uuid::now_v7();
    let representation = Uuid::now_v7();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) \
         VALUES ($1,$2,'body/part-0',0,$3)",
    )
    .bind(item)
    .bind(version)
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
    sqlx::query(
        "UPDATE documents SET current_version_id=$1, revision=revision+1 WHERE document_id=$2",
    )
    .bind(version)
    .bind(document)
    .execute(pool)
    .await
    .unwrap();
    document
}

struct Durable {
    _guard: support::postgres::DatabaseGuard,
    pool: PgPool,
    files: std::path::PathBuf,
    lexical_root: std::path::PathBuf,
    storage: FileSystemStorage,
    repository: Arc<PostgresDocumentRepository>,
    ledger: Arc<PgSourceRegistrationLedger>,
    source_id: SourceId,
    registration: search_application::source_registration::SourceRegistration,
    activation: search_application::source_registration::RegistrationActivation,
}

impl Drop for Durable {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.files);
        let _ = std::fs::remove_dir_all(&self.lexical_root);
    }
}

impl Durable {
    async fn start() -> Self {
        let (guard, pool, _) = support::postgres::postgres("durable_document_test").await;
        document_repository_postgres::migrate(&pool).await.unwrap();
        search_runtime::migrate(&pool).await.unwrap();
        search_graph::migrate(&pool).await.unwrap();
        let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
            pool.clone(),
            document_domain::PrincipalRef::new("test-idp", "editor").unwrap(),
        ));
        repository
            .initialize_root_policy(
                &actor(),
                vec![
                    PolicyGrant::new(
                        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor")
                            .unwrap(),
                        [Action::Read],
                    )
                    .unwrap(),
                ],
            )
            .await
            .unwrap();
        let files = std::env::temp_dir().join(format!("search-durable-files-{}", Uuid::now_v7()));
        let lexical_root =
            std::env::temp_dir().join(format!("search-durable-lexical-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&files).unwrap();
        let host = Arc::new(SyntheticHostRegistrationAuthority::new());
        let ledger = Arc::new(PgSourceRegistrationLedger::new(pool.clone(), host.clone()));
        let remote = registration::publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
        ledger.reconcile(&remote).await.unwrap();
        let source_id = registration::source(7_950);
        let document = registration::document(source_id, "tenant-a").await;
        let desired = registration::publish(
            &host,
            RegistrationNamespace::Document,
            1,
            vec![document.clone()],
        )
        .await;
        let activation = ledger.reconcile(&desired).await.unwrap()[&source_id];
        Self {
            registration: document,
            activation,
            _guard: guard,
            storage: FileSystemStorage::new(&files),
            pool,
            files,
            lexical_root,
            repository,
            ledger,
            source_id,
        }
    }

    /// A fresh indexer over a fresh durable runtime: no state survives in RAM.
    async fn indexer(
        &self,
    ) -> DocumentIndexingService<
        DocumentOutboxIndexer<
            PostgresDocumentSnapshotReader,
            discovery_support::Receipts,
            PgDocumentIndexRuntime,
        >,
    > {
        let registrar = self
            .ledger
            .generation_registrar(self.registration.clone(), self.activation)
            .unwrap();
        let mut config = discovery_support::index_config(self.source_id);
        config.source.retention_mode = RetentionMode::PersistentResource;
        let runtime = PgDocumentIndexRuntime::new(
            self.pool.clone(),
            &self.lexical_root,
            config.source.clone(),
            registrar,
            FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
        );
        let extractor: Arc<dyn BodyItemExtractor> = Arc::new(DocumentBodyExtractor::new(
            self.source_id,
            self.storage.clone(),
            body_support::InProcessExtractor::new(body_support::Mode::Honest),
            body_support::registry(),
        ));
        DocumentIndexingService::new(
            DocumentOutboxIndexer::new(
                PostgresDocumentSnapshotReader::new(self.pool.clone()),
                config,
                runtime,
                discovery_support::Receipts::default(),
            )
            .with_body_extractor(extractor),
        )
    }

    fn source(&self) -> search_application::search_core::source::DiscoverableSource {
        let mut source = discovery_support::source(self.source_id);
        source.retention_mode = RetentionMode::PersistentResource;
        source
    }
}

async fn graph_nodes(pool: &PgPool, key: ProjectionGenerationKey) -> Vec<ResourceId> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT resource_id FROM search_graph.resource WHERE source_id=$1 AND generation_id=$2 \
         ORDER BY resource_id",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_all(pool)
    .await
    .unwrap();
    ids.into_iter().map(ResourceId::from_uuid).collect()
}

#[tokio::test]
async fn document_outbox_rebuild_has_durable_same_key_graph_and_survives_restart() {
    let durable = Durable::start().await;
    let document = publish(&durable.pool, &durable.storage, "東京の規程本文").await;
    let event = || discovery_support::event("DocumentVersionPublished", document);

    let first = durable.indexer().await;
    let key = match first.handle(event()).await.unwrap() {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected a published durable generation: {other:?}"),
    };
    // The pointer, the bundle and the Graph all carry this one key.
    let report = PgStartupRecovery::new(
        durable.pool.clone(),
        &durable.lexical_root,
        durable.source(),
    )
    .startup(10)
    .await
    .unwrap();
    assert!(matches!(&report.current, CurrentState::Verified(bundle) if bundle.key() == key));

    // Every Graph node is a snapshot-bound Document mapping with current access.
    let access = DocumentCurrentAccessAdapter::new(
        durable.source_id,
        durable.pool.clone(),
        DocumentAccessCheckService::new(durable.repository.clone()),
        actor(),
        discovery_support::ACCESS_CONTEXT.into(),
    );
    let gate = DocumentGenerationAccess::new(
        PostgresGraphStore::new(durable.pool.clone()),
        &access,
        discovery_support::ACCESS_CONTEXT,
    );
    let store = PostgresGraphStore::new(durable.pool.clone());
    let nodes = graph_nodes(&durable.pool, key).await;
    assert_eq!(nodes.len(), 3, "Version, Document and folder placement");
    let mut kinds = Vec::new();
    for node in nodes {
        let stored = store.ready_resource(key, node).await.unwrap().unwrap();
        kinds.push(match stored.mapping {
            GraphSourceMapping::Version { .. } => "version",
            GraphSourceMapping::Document { .. } => "document",
            GraphSourceMapping::FolderPlacement { .. } => "folder",
            GraphSourceMapping::Registered { .. } => "registered",
        });
        assert_eq!(
            gate.evaluate_stored(key, &stored, discovery_support::ACCESS_CONTEXT)
                .await
                .unwrap(),
            AccessDecision::Allowed
        );
    }
    kinds.sort_unstable();
    assert_eq!(kinds, vec!["document", "folder", "version"]);

    // A restarted runtime sees the same current key and does not rebuild it.
    drop(first);
    let restarted = durable.indexer().await;
    assert_eq!(
        restarted.handle(event()).await.unwrap(),
        IndexingOutcome::Unchanged(key)
    );
    let generations: i64 = sqlx::query_scalar("SELECT count(*) FROM search_generation")
        .fetch_one(&durable.pool)
        .await
        .unwrap();
    assert_eq!(generations, 1);

    // A changed Source snapshot builds and publishes a new key.
    let second = publish(&durable.pool, &durable.storage, "大阪の規程本文").await;
    let next = match restarted
        .handle(discovery_support::event("DocumentVersionPublished", second))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(next) => next,
        other => panic!("expected a new durable generation: {other:?}"),
    };
    assert_ne!(next, key);
    assert_eq!(
        sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT current_generation_id FROM search_source_coordination WHERE source_id=$1"
        )
        .bind(durable.source_id.as_uuid())
        .fetch_one(&durable.pool)
        .await
        .unwrap(),
        Some(next.generation_id.as_uuid())
    );
}
