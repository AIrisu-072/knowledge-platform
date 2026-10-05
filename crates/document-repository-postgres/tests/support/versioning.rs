#![allow(dead_code)]

use std::{
    collections::HashMap,
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use document_application::{
    Clock, ContentReader, DocumentVersionService, FileStorage, IdGenerator,
    InspectionExecutionError, SemanticInspectionExecutor, StorageError, StorageObjectInfo,
    StoreFileRequest, StoredFile, VersionOperationId, VersioningItemInput, VersioningPreflight,
};
use document_domain::{
    ContentHash, DocumentId, DocumentVersionId, FileId, FileSize, LogicalPath, MediaType,
    PrincipalRef, StorageKey, Title,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use document_semantic_inspection_core::{InspectionProfileVersion, WorkerRequest, WorkerResponse};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

pub(super) struct TestClock;
impl Clock for TestClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
    }
}
pub(super) struct TestIds;
impl IdGenerator for TestIds {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

#[derive(Default)]
pub(super) struct TestStorage {
    objects: Mutex<HashMap<String, Vec<u8>>>,
}
impl TestStorage {
    fn insert(&self, key: &str, bytes: Vec<u8>) {
        self.objects.lock().unwrap().insert(key.to_owned(), bytes);
    }
    pub(super) fn remove(&self, key: &str) {
        self.objects.lock().unwrap().remove(key);
    }
}
impl FileStorage for TestStorage {
    async fn put_immutable(&self, request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        let (file_id, mut content, media_type) = request.into_parts();
        let mut bytes = Vec::new();
        content
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| StorageError::WriteFailed)?;
        let key = StorageKey::new(format!("objects/{}", file_id.as_uuid())).unwrap();
        self.insert(key.as_str(), bytes.clone());
        Ok(StoredFile::new(
            key,
            ContentHash::from_slice(&[bytes[0]; 32]).unwrap(),
            FileSize::new(bytes.len() as i64).unwrap(),
            media_type,
        ))
    }
    async fn open(&self, key: &StorageKey) -> Result<ContentReader, StorageError> {
        let bytes = self
            .objects
            .lock()
            .unwrap()
            .get(key.as_str())
            .cloned()
            .ok_or(StorageError::NotFound)?;
        Ok(Box::pin(Cursor::new(bytes)))
    }
    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(vec![])
    }
}

#[derive(Default)]
pub(super) struct TestExecutor {
    unavailable: AtomicBool,
}
impl TestExecutor {
    pub(super) fn set_unavailable(&self) {
        self.unavailable.store(true, Ordering::SeqCst);
    }
    pub(super) fn set_available(&self) {
        self.unavailable.store(false, Ordering::SeqCst);
    }
}
impl SemanticInspectionExecutor for TestExecutor {
    async fn inspect(
        &self,
        request: WorkerRequest,
        mut content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(InspectionExecutionError::ExtractorUnavailable);
        }
        let mut bytes = Vec::new();
        content.read_to_end(&mut bytes).await.unwrap();
        serde_json::from_value(serde_json::json!({
            "protocol_version":"dsi-worker-v0", "inspection_profile_version":"dsi-v0",
            "observed_raw_content_hash":request.expected_raw_content_hash,
            "observed_size_bytes":request.expected_size_bytes,
            "detected_format":"txt", "semantic_fingerprint":{"algorithm":"sha256","digest":vec![bytes[0]; 32]},
            "semantic_capabilities":[],
            "editorial_provenance":{"tracked_changes":[],"comments":[],"document_author_labels":[],"last_modified_by":null,"modification_metadata":{}},
            "external_dependencies":[], "digital_signature_evidence":[],
            "extractor_provenance":{"worker_build_id":"test","adapter_id":"txt","adapter_version":"1","parser_libraries":[],"native_dependency_identity":[]},
            "diagnostics":[]
        })).map_err(|_| InspectionExecutionError::InvalidWorkerResult)
    }
}

pub(super) struct Fixture {
    _container: testcontainers::ContainerAsync<GenericImage>,
    pub(super) pool: PgPool,
    pub(super) repository: Arc<PostgresDocumentRepository>,
    pub(super) storage: Arc<TestStorage>,
    pub(super) clock: Arc<TestClock>,
    pub(super) executor: Arc<TestExecutor>,
    pub(super) document_id: DocumentId,
    pub(super) base_id: DocumentVersionId,
}
impl Fixture {
    pub(super) fn preflight(
        &self,
    ) -> VersioningPreflight<PostgresDocumentRepository, TestStorage, TestExecutor, TestClock> {
        VersioningPreflight::new(
            self.repository.clone(),
            self.storage.clone(),
            self.executor.clone(),
            self.clock.clone(),
        )
    }
    pub(super) fn service(
        &self,
    ) -> DocumentVersionService<
        TestIds,
        TestClock,
        TestStorage,
        TestExecutor,
        PostgresDocumentRepository,
    > {
        DocumentVersionService::new(
            Arc::new(TestIds),
            self.clock.clone(),
            self.storage.clone(),
            self.executor.clone(),
            self.repository.clone(),
        )
    }
    pub(super) async fn prepare(
        &self,
        title: &str,
        byte: u8,
    ) -> document_application::PreparedManifest {
        VersioningPreflight::new(
            self.repository.clone(),
            self.storage.clone(),
            self.executor.clone(),
            self.clock.clone(),
        )
        .prepare(
            Title::new(title).unwrap(),
            vec![VersioningItemInput::new(
                LogicalPath::new("primary").unwrap(),
                0,
                FileId::from_uuid(Uuid::now_v7()),
                MediaType::new("text/plain").unwrap(),
                "version.txt",
                Box::pin(Cursor::new(vec![byte; 3])),
            )],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap()
    }
}

pub(super) fn operation_id(value: u8) -> VersionOperationId {
    VersionOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8000-{value:012x}")).unwrap(),
    )
    .unwrap()
}
pub(super) fn actor() -> PrincipalRef {
    PrincipalRef::new("test-idp", "editor").unwrap()
}

pub(super) async fn fixture() -> Fixture {
    fixture_with_publication(true).await
}

pub(super) async fn initial_fixture() -> Fixture {
    fixture_with_publication(false).await
}

async fn fixture_with_publication(published: bool) -> Fixture {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "versioning_transaction_test")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/versioning_transaction_test"
        ))
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    let document_id = DocumentId::from_uuid(Uuid::now_v7());
    let base_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let base_file = FileId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,$3,'{}',to_timestamp(0))")
        .bind(document_id.as_uuid()).bind(SYSTEM_ROOT_FOLDER_ID).bind(i64::from(published)).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,$3,'Base',CASE WHEN $3 = 'PUBLISHED' THEN to_timestamp(0) ELSE NULL END,'test-idp','editor','{}',to_timestamp(0))")
        .bind(base_id.as_uuid()).bind(document_id.as_uuid()).bind(if published { "PUBLISHED" } else { "WORKING" }).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',3,'objects/base',to_timestamp(0))")
        .bind(base_file.as_uuid()).bind(vec![1_u8; 32]).execute(&pool).await.unwrap();
    let item_id = Uuid::now_v7();
    let representation_id = Uuid::now_v7();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(item_id).bind(base_id.as_uuid()).bind(representation_id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,'AUTHORITATIVE','base.txt')")
        .bind(representation_id).bind(item_id).bind(base_file.as_uuid()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    if published {
        sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
            .bind(base_id.as_uuid())
            .bind(document_id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO document_revisions \
         (document_id,document_version_id,major_no,minor_no,metadata_snapshot, \
          metadata_snapshot_status,source_kind,operation_id,created_at, \
          actor_identity_provider,actor_principal_id,reason) \
         VALUES ($1,$2,1,0, \
                 jsonb_build_object('document_type',NULL,'owning_department',NULL, \
                                    'category',NULL,'extensions',NULL), \
                 'complete','legacyBackfill',NULL,to_timestamp(0),NULL,NULL,NULL)",
        )
        .bind(document_id.as_uuid())
        .bind(base_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    }
    let storage = Arc::new(TestStorage::default());
    storage.insert("objects/base", vec![1; 3]);
    Fixture {
        _container: container,
        pool: pool.clone(),
        repository: Arc::new(PostgresDocumentRepository::new(pool)),
        storage,
        clock: Arc::new(TestClock),
        executor: Arc::new(TestExecutor::default()),
        document_id,
        base_id,
    }
}

#[allow(dead_code)]
pub(super) async fn install_new_current(f: &Fixture, title: &str, byte: u8) -> DocumentVersionId {
    let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let file_id = FileId::from_uuid(Uuid::now_v7());
    let storage_key = format!("objects/{}", file_id.as_uuid());
    f.storage.insert(&storage_key, vec![byte; 3]);
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,base_document_version_id,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,3,$3,'PUBLISHED',$4,to_timestamp(1),'test-idp','editor','{}',to_timestamp(1))")
        .bind(version_id.as_uuid()).bind(f.document_id.as_uuid()).bind(f.base_id.as_uuid()).bind(title)
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',3,$3,to_timestamp(1))")
        .bind(file_id.as_uuid()).bind(vec![byte; 32]).bind(&storage_key).execute(&f.pool).await.unwrap();
    let item_id = Uuid::now_v7();
    let representation_id = Uuid::now_v7();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(item_id).bind(version_id.as_uuid()).bind(representation_id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,'AUTHORITATIVE','new.txt')")
        .bind(representation_id).bind(item_id).bind(file_id.as_uuid()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1, revision = revision + 1 WHERE document_id = $2")
        .bind(version_id.as_uuid()).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    version_id
}
