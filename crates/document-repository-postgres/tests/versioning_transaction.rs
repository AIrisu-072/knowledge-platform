use std::{
    collections::HashMap,
    io::Cursor,
    sync::{Arc, Mutex},
};

use document_application::{
    ApplicationError, Clock, ContentReader, CreateVersionCommand, DocumentVersionService,
    FileStorage, IdGenerator, InspectionExecutionError, RebaseWorkingVersionCommand,
    SemanticInspectionExecutor, StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
    UpdateWorkingVersionCommand, VersionOperationId, VersioningItemInput, VersioningPreflight,
    VersioningRepository,
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

struct TestClock;
impl Clock for TestClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
    }
}
struct TestIds;
impl IdGenerator for TestIds {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

#[derive(Default)]
struct TestStorage {
    objects: Mutex<HashMap<String, Vec<u8>>>,
}
impl TestStorage {
    fn insert(&self, key: &str, bytes: Vec<u8>) {
        self.objects.lock().unwrap().insert(key.to_owned(), bytes);
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

struct TestExecutor;
impl SemanticInspectionExecutor for TestExecutor {
    async fn inspect(
        &self,
        request: WorkerRequest,
        mut content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
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

struct Fixture {
    _container: testcontainers::ContainerAsync<GenericImage>,
    pool: PgPool,
    repository: Arc<PostgresDocumentRepository>,
    storage: Arc<TestStorage>,
    clock: Arc<TestClock>,
    executor: Arc<TestExecutor>,
    document_id: DocumentId,
    base_id: DocumentVersionId,
}
impl Fixture {
    fn service(
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
    async fn prepare(&self, title: &str, byte: u8) -> document_application::PreparedManifest {
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

fn operation_id(value: u8) -> VersionOperationId {
    VersionOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8000-{value:012x}")).unwrap(),
    )
    .unwrap()
}
fn actor() -> PrincipalRef {
    PrincipalRef::new("test-idp", "editor").unwrap()
}

async fn fixture() -> Fixture {
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
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,1,'{}',to_timestamp(0))")
        .bind(document_id.as_uuid()).bind(SYSTEM_ROOT_FOLDER_ID).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Base',to_timestamp(0),'test-idp','editor','{}',to_timestamp(0))")
        .bind(base_id.as_uuid()).bind(document_id.as_uuid()).execute(&pool).await.unwrap();
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
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(base_id.as_uuid())
        .bind(document_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let storage = Arc::new(TestStorage::default());
    storage.insert("objects/base", vec![1; 3]);
    Fixture {
        _container: container,
        pool: pool.clone(),
        repository: Arc::new(PostgresDocumentRepository::new(pool)),
        storage,
        clock: Arc::new(TestClock),
        executor: Arc::new(TestExecutor),
        document_id,
        base_id,
    }
}

async fn install_new_current(f: &Fixture, title: &str, byte: u8) -> DocumentVersionId {
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

#[tokio::test]
async fn create_update_replay_and_outbox_are_atomic() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let prepared = f.prepare("Changed", 2).await;
    let create =
        CreateVersionCommand::new(operation_id(1), f.document_id, target_id, 1, actor()).unwrap();
    let first = service
        .create_version(create.clone(), prepared.clone())
        .await
        .unwrap();
    assert_eq!(
        (
            first.version_no(),
            first.base_version_id(),
            first.resulting_revision()
        ),
        (2, f.base_id, 2)
    );
    assert_eq!(
        service
            .create_version(create.clone(), prepared.clone())
            .await
            .unwrap(),
        first
    );
    assert_eq!(
        f.repository
            .get_version_operation(operation_id(1))
            .await
            .unwrap()
            .unwrap()
            .result(),
        &first
    );
    assert_eq!(
        service
            .create_version(create, f.prepare("Changed again", 3).await)
            .await,
        Err(ApplicationError::Conflict)
    );

    let update =
        UpdateWorkingVersionCommand::new(operation_id(2), f.document_id, target_id, 2, actor())
            .unwrap();
    let updated = service
        .update_working(update.clone(), f.prepare("Revised", 4).await)
        .await
        .unwrap();
    assert_eq!(
        (
            updated.version_no(),
            updated.target_version_id(),
            updated.resulting_revision()
        ),
        (2, target_id, 3)
    );
    assert_eq!(
        service
            .update_working(update, f.prepare("Other", 5).await)
            .await,
        Err(ApplicationError::Conflict)
    );
    let version_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_versions WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(version_count, 2);
    let item_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM content_items WHERE document_version_id = $1")
            .bind(target_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(item_count, 1);
    let event_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE aggregate_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let audit_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE resource_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!((event_count, audit_count), (2, 2));
}

#[tokio::test]
async fn no_change_missing_base_and_duplicate_working_fail_closed() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let same = f.prepare("Base", 1).await;
    let command =
        CreateVersionCommand::new(operation_id(3), f.document_id, target_id, 1, actor()).unwrap();
    assert_eq!(
        service.create_version(command, same).await,
        Err(ApplicationError::BusinessRule)
    );
    let changed = f.prepare("Changed", 2).await;
    let create =
        CreateVersionCommand::new(operation_id(4), f.document_id, target_id, 1, actor()).unwrap();
    service.create_version(create, changed).await.unwrap();
    let duplicate = CreateVersionCommand::new(
        operation_id(5),
        f.document_id,
        DocumentVersionId::from_uuid(Uuid::now_v7()),
        2,
        actor(),
    )
    .unwrap();
    assert_eq!(
        service
            .create_version(duplicate, f.prepare("Again", 3).await)
            .await,
        Err(ApplicationError::BusinessRule)
    );
    sqlx::query("UPDATE documents SET current_version_id = NULL, revision = revision + 1 WHERE document_id = $1")
        .bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let absent = CreateVersionCommand::new(
        operation_id(6),
        f.document_id,
        DocumentVersionId::from_uuid(Uuid::now_v7()),
        3,
        actor(),
    )
    .unwrap();
    assert_eq!(
        service
            .create_version(absent, f.prepare("Absent", 4).await)
            .await,
        Err(ApplicationError::BusinessRule)
    );
}

#[tokio::test]
async fn two_creators_allocate_only_one_working_number() {
    let f = fixture().await;
    let service = f.service();
    let one = CreateVersionCommand::new(
        operation_id(7),
        f.document_id,
        DocumentVersionId::from_uuid(Uuid::now_v7()),
        1,
        actor(),
    )
    .unwrap();
    let two = CreateVersionCommand::new(
        operation_id(8),
        f.document_id,
        DocumentVersionId::from_uuid(Uuid::now_v7()),
        1,
        actor(),
    )
    .unwrap();
    let prepared_one = f.prepare("One", 2).await;
    let prepared_two = f.prepare("Two", 3).await;
    let (a, b) = tokio::join!(
        service.create_version(one, prepared_one),
        service.create_version(two, prepared_two)
    );
    assert_eq!(a.is_ok() as u8 + b.is_ok() as u8, 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM document_versions WHERE document_id = $1 AND lifecycle_state = 'WORKING'")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn stale_working_requires_explicit_rebase_and_keeps_version_number() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(9), f.document_id, target_id, 1, actor()).unwrap();
    service
        .create_version(create, f.prepare("Working", 2).await)
        .await
        .unwrap();
    let new_current = install_new_current(&f, "New current", 3).await;
    let stale_update =
        UpdateWorkingVersionCommand::new(operation_id(10), f.document_id, target_id, 3, actor())
            .unwrap();
    assert_eq!(
        service
            .update_working(stale_update, f.prepare("Edited", 4).await)
            .await,
        Err(ApplicationError::Conflict)
    );
    let rebase =
        RebaseWorkingVersionCommand::new(operation_id(11), f.document_id, target_id, 3, actor())
            .unwrap();
    let rebased = service.rebase_working(rebase.clone()).await.unwrap();
    assert_eq!(
        (
            rebased.version_no(),
            rebased.base_version_id(),
            rebased.resulting_revision()
        ),
        (2, new_current, 4)
    );
    assert_eq!(service.rebase_working(rebase).await.unwrap(), rebased);
    let persisted: (Uuid, i64) = sqlx::query_as("SELECT base_document_version_id, version_no FROM document_versions WHERE document_version_id = $1")
        .bind(target_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(persisted, (new_current.as_uuid(), 2));
    let event_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE aggregate_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(event_count, 2);
}

#[tokio::test]
async fn failed_update_keeps_previous_complete_manifest() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(12), f.document_id, target_id, 1, actor()).unwrap();
    service
        .create_version(create, f.prepare("Working", 2).await)
        .await
        .unwrap();
    let before: (Uuid, String) = sqlx::query_as(
        "SELECT cr.file_id, v.title FROM document_versions v \
         JOIN content_items ci ON ci.document_version_id = v.document_version_id \
         JOIN content_representations cr ON cr.content_representation_id = ci.authoritative_representation_id \
         WHERE v.document_version_id = $1",
    )
    .bind(target_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    let prepared = f.prepare("Broken update", 4).await;
    let file_id = prepared.items()[0].file().file_id().as_uuid();
    sqlx::query(
        "UPDATE document_semantic_inspections SET fingerprint_digest = $1 WHERE file_id = $2",
    )
    .bind(vec![9_u8; 32])
    .bind(file_id)
    .execute(&f.pool)
    .await
    .unwrap();
    let update =
        UpdateWorkingVersionCommand::new(operation_id(13), f.document_id, target_id, 2, actor())
            .unwrap();
    assert_eq!(
        service.update_working(update, prepared).await,
        Err(ApplicationError::IntegrityViolation)
    );
    let after: (Uuid, String) = sqlx::query_as(
        "SELECT cr.file_id, v.title FROM document_versions v \
         JOIN content_items ci ON ci.document_version_id = v.document_version_id \
         JOIN content_representations cr ON cr.content_representation_id = ci.authoritative_representation_id \
         WHERE v.document_version_id = $1",
    )
    .bind(target_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(after, before);
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(revision, 2);
}
