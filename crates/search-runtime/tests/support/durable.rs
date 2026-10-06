//! Shared real Document Source + durable Search runtime fixture (P3-D01, P6-S04..S06).
#![allow(dead_code, unused_imports)]

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use document_application::{
    BootstrapRootPolicy, FileStorage, InvocationKind, StoreFileRequest, VerifiedActorContext,
};
use document_domain::{Action, FileId, MediaType, PolicyGrant, PolicySubject, PolicySubjectKind};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use document_storage_fs::FileSystemStorage;
use search_application::indexing_service::DocumentIndexingService;
use search_application::search_core::id::SourceId;
use search_application::search_core::source::RetentionMode;
use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_runtime::document_runtime::PgDocumentIndexRuntime;
use search_runtime::event_completion::PgPublication;
use search_runtime::full_guard::FullGuardTtl;
use search_runtime::source_registration::PgSourceRegistrationLedger;
use search_source_document::{
    BodyItemExtractor, DocumentBodyExtractor, DocumentOutboxIndexer, PostgresDocumentSnapshotReader,
};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub type Indexer = DocumentIndexingService<
    DocumentOutboxIndexer<
        PostgresDocumentSnapshotReader,
        super::discovery_support::Receipts,
        PgDocumentIndexRuntime,
    >,
>;

pub fn actor() -> VerifiedActorContext {
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
pub async fn publish(pool: &PgPool, storage: &FileSystemStorage, text: &str) -> Uuid {
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

pub struct Durable {
    pub _guard: super::support::postgres::DatabaseGuard,
    pub pool: PgPool,
    pub files: std::path::PathBuf,
    pub lexical_root: std::path::PathBuf,
    pub storage: FileSystemStorage,
    pub repository: Arc<PostgresDocumentRepository>,
    pub ledger: Arc<PgSourceRegistrationLedger>,
    pub source_id: SourceId,
    pub registration: search_application::source_registration::SourceRegistration,
    pub activation: search_application::source_registration::RegistrationActivation,
    pub host: Arc<SyntheticHostRegistrationAuthority>,
}

impl Drop for Durable {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.files);
        let _ = std::fs::remove_dir_all(&self.lexical_root);
    }
}

impl Durable {
    pub async fn start() -> Self {
        Self::start_with(false).await
    }

    /// `searchable`: the Source is registered for API search (content search
    /// over Knowledge Resources) instead of the minimal directory Source.
    pub async fn start_with(searchable: bool) -> Self {
        let (guard, pool, _) = super::support::postgres::postgres("durable_document_test").await;
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
        let remote =
            super::registration::publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
        ledger.reconcile(&remote).await.unwrap();
        let source_id = super::registration::source(7_950);
        let document = if searchable {
            super::registration::searchable_document(source_id, "tenant-a").await
        } else {
            super::registration::document(source_id, "tenant-a").await
        };
        let desired = super::registration::publish(
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
            host,
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
    pub async fn indexer(&self) -> Indexer {
        self.indexer_with(self.extractor(), false)
    }

    pub fn extractor(&self) -> Arc<dyn BodyItemExtractor> {
        Arc::new(DocumentBodyExtractor::new(
            self.source_id,
            self.storage.clone(),
            super::body_support::InProcessExtractor::new(super::body_support::Mode::Honest),
            super::body_support::registry(),
        ))
    }

    /// The same indexer, optionally completing deliveries through the P7 port.
    pub fn indexer_with(&self, extractor: Arc<dyn BodyItemExtractor>, fenced: bool) -> Indexer {
        let registrar = self
            .ledger
            .generation_registrar(self.registration.clone(), self.activation)
            .unwrap();
        let mut config = super::discovery_support::index_config(self.source_id);
        config.source.retention_mode = RetentionMode::PersistentResource;
        let runtime = PgDocumentIndexRuntime::new(
            self.pool.clone(),
            &self.lexical_root,
            config.source.clone(),
            registrar,
            FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
        );
        let mut indexer = DocumentOutboxIndexer::new(
            PostgresDocumentSnapshotReader::new(self.pool.clone()),
            config,
            runtime,
            super::discovery_support::Receipts::default(),
        )
        .with_body_extractor(extractor);
        if fenced {
            indexer =
                indexer.with_fenced_completion(Arc::new(PgPublication::new(self.pool.clone())));
        }
        DocumentIndexingService::new(indexer)
    }

    pub fn source(&self) -> search_application::search_core::source::DiscoverableSource {
        let mut source = super::discovery_support::source(self.source_id);
        source.retention_mode = RetentionMode::PersistentResource;
        source
    }
}
