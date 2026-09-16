use std::{io::Cursor, sync::Arc};

use document_application::{
    AuthoritativeDocument, Clock, ContentReader, CreateDocumentCommand, CreateInitialDocumentRecord,
    DocumentRepository, DocumentService, FileStorage, IdGenerator, RepositoryError, StorageError,
    StorageObjectInfo, StoreFileRequest, StoredFile, AUDIT_DOCUMENT_CREATED,
    AUDIT_DOCUMENT_VERSION_CREATED, DOCUMENT_CREATED, DOCUMENT_VERSION_CREATED,
};
use document_domain::{
    ContentHash, CreateInitialDocument, DocumentId, DocumentVersionId, FileId, FileSize, FolderId,
    InitialDocument, LifecycleState, MediaType, Metadata, PrincipalRef, StorageKey,
    StoredFileDescriptor, Title,
};
use document_repository_postgres::{
    migrate, PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID,
};
use serde_json::{json, Map, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};
use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};
use time::OffsetDateTime;
use uuid::Uuid;

#[tokio::test]
async fn repository_persists_reads_and_rolls_back_authoritative_state_atomically() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migration should succeed");

    let repository = Arc::new(PostgresDocumentRepository::new(pool.clone()));
    let storage = Arc::new(StaticStorage);
    let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("fixed time is valid");

    let ids = Arc::new(SequenceIds::new(100));
    let service = DocumentService::new(
        ids,
        Arc::new(FixedClock(now)),
        storage.clone(),
        repository.clone(),
    );
    let result = service
        .create_document(command(SYSTEM_ROOT_FOLDER_ID, "Authoritative v0"))
        .await
        .expect("valid initial document should persist");

    assert_eq!(
        count_where_uuid(&pool, "documents", "document_id", result.document_id().as_uuid()).await,
        1
    );
    assert_eq!(
        count_where_uuid(
            &pool,
            "document_versions",
            "document_version_id",
            result.document_version_id().as_uuid(),
        )
        .await,
        1
    );
    assert_eq!(
        count_where_uuid(&pool, "file_objects", "file_id", result.file_id().as_uuid()).await,
        1
    );
    assert_eq!(
        count_where_uuid(
            &pool,
            "version_files",
            "document_version_id",
            result.document_version_id().as_uuid(),
        )
        .await,
        1
    );
    assert_eq!(
        count_where_uuid(
            &pool,
            "outbox_events",
            "aggregate_id",
            result.document_id().as_uuid(),
        )
        .await,
        2
    );
    assert_eq!(
        count_where_uuid(
            &pool,
            "audit_outbox_events",
            "resource_id",
            result.document_id().as_uuid(),
        )
        .await,
        2
    );

    let mut domain_types: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM outbox_events WHERE aggregate_id = $1 ORDER BY event_type",
    )
    .bind(result.document_id().as_uuid())
    .fetch_all(&pool)
    .await
    .expect("domain events should be queryable");
    domain_types.sort();
    assert_eq!(
        domain_types,
        vec![DOCUMENT_CREATED.to_owned(), DOCUMENT_VERSION_CREATED.to_owned()]
    );

    let mut audit_types: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM audit_outbox_events WHERE resource_id = $1 ORDER BY event_type",
    )
    .bind(result.document_id().as_uuid())
    .fetch_all(&pool)
    .await
    .expect("audit events should be queryable");
    audit_types.sort();
    assert_eq!(
        audit_types,
        vec![
            AUDIT_DOCUMENT_CREATED.to_owned(),
            AUDIT_DOCUMENT_VERSION_CREATED.to_owned(),
        ]
    );

    let audit_actor: (String, String) = sqlx::query_as(
        "SELECT actor_identity_provider, actor_principal_id \
         FROM audit_outbox_events \
         WHERE resource_id = $1 \
         ORDER BY event_type \
         LIMIT 1",
    )
    .bind(result.document_id().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("audit actor should be persisted");
    assert_eq!(audit_actor, ("test-idp".to_owned(), "actor-42".to_owned()));

    let loaded = service
        .get_document(result.document_id())
        .await
        .expect("authoritative document should round-trip");
    assert_eq!(loaded.document().document_id(), result.document_id());
    assert_eq!(loaded.document().folder_id().as_uuid(), SYSTEM_ROOT_FOLDER_ID);
    assert_eq!(loaded.document().current_version_id(), None);
    assert_eq!(loaded.document().revision(), 0);
    assert_eq!(
        loaded.document().metadata().as_map().get("documentClass"),
        Some(&json!("policy"))
    );
    assert_eq!(
        loaded.version().document_version_id(),
        result.document_version_id()
    );
    assert_eq!(loaded.version().version_no().get(), 1);
    assert_eq!(loaded.version().lifecycle_state(), LifecycleState::Working);
    assert_eq!(loaded.version().title().as_str(), "Authoritative v0");
    assert_eq!(loaded.version().revision_reason(), None);
    assert_eq!(loaded.version().published_at(), None);
    assert_eq!(loaded.version().withdrawn_at(), None);
    assert_eq!(loaded.version().created_by().identity_provider(), "test-idp");
    assert_eq!(loaded.version().created_by().principal_id(), "actor-42");
    assert_eq!(
        loaded.version().metadata().as_map().get("versionClass"),
        Some(&json!("working"))
    );
    assert_eq!(loaded.file().file_id(), result.file_id());
    assert_eq!(loaded.file().size_bytes().get(), 3);
    assert_eq!(loaded.version_file().file_id(), result.file_id());
    assert_eq!(loaded.version_file().ordinal(), 0);
    assert_eq!(loaded.version_file().original_filename(), "policy.pdf");

    assert!(
        repository
            .file_reference_exists(result.file_id())
            .await
            .expect("file reference lookup should succeed")
    );
    assert!(
        !repository
            .file_reference_exists(FileId::from_uuid(Uuid::from_u128(9_999)))
            .await
            .expect("missing file lookup should succeed")
    );
    assert!(
        repository
            .get_authoritative_document(DocumentId::from_uuid(Uuid::from_u128(9_998)))
            .await
            .expect("missing document lookup should succeed")
            .is_none()
    );

    let missing_folder_document = DocumentId::from_uuid(Uuid::from_u128(5_000));
    let missing_folder_record = direct_record(
        missing_folder_document,
        DocumentVersionId::from_uuid(Uuid::from_u128(5_001)),
        FileId::from_uuid(Uuid::from_u128(5_002)),
        FolderId::from_uuid(Uuid::from_u128(5_003)),
        now,
    );
    let error = repository
        .create_initial_document(missing_folder_record)
        .await
        .expect_err("missing folder must be rejected before writes");
    assert_eq!(error, RepositoryError::FolderNotFound);
    assert_eq!(
        count_where_uuid(
            &pool,
            "documents",
            "document_id",
            missing_folder_document.as_uuid(),
        )
        .await,
        0
    );

    sqlx::query(
        "CREATE OR REPLACE FUNCTION kp_fail_insert() RETURNS trigger \
         LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected repository failure'; END; $$",
    )
    .execute(&pool)
    .await
    .expect("failure trigger function should install");

    for (offset, table) in [
        (1_000_u128, "documents"),
        (2_000, "document_versions"),
        (3_000, "version_files"),
        (4_000, "outbox_events"),
        (5_000, "audit_outbox_events"),
    ] {
        let trigger_sql = format!(
            "CREATE TRIGGER kp_fail BEFORE INSERT ON {table} \
             FOR EACH STATEMENT EXECUTE FUNCTION kp_fail_insert()"
        );
        sqlx::query(&trigger_sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("trigger on {table} should install: {error}"));

        let ids = Arc::new(SequenceIds::new(10_000 + offset));
        let service = DocumentService::new(
            ids,
            Arc::new(FixedClock(now)),
            storage.clone(),
            repository.clone(),
        );
        let attempted_document_id = DocumentId::from_uuid(Uuid::from_u128(10_000 + offset));
        let attempted_version_id =
            DocumentVersionId::from_uuid(Uuid::from_u128(10_001 + offset));
        let attempted_file_id = FileId::from_uuid(Uuid::from_u128(10_002 + offset));

        assert!(
            service
                .create_document(command(SYSTEM_ROOT_FOLDER_ID, "Rollback proof"))
                .await
                .is_err(),
            "injected failure on {table} must fail create"
        );

        let drop_trigger_sql = format!("DROP TRIGGER kp_fail ON {table}");
        sqlx::query(&drop_trigger_sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("trigger on {table} should drop: {error}"));

        assert_attempt_rows_zero(
            &pool,
            attempted_document_id,
            attempted_version_id,
            attempted_file_id,
            table,
        )
        .await;
    }
}

async fn postgres() -> (testcontainers::ContainerAsync<GenericImage>, PgPool) {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "knowledge_platform_test")
        .start()
        .await
        .expect("postgres container should start");
    let port = container
        .get_host_port_ipv4(5432.tcp())
        .await
        .expect("postgres port should be mapped");
    let database_url =
        format!("postgres://postgres:postgres@127.0.0.1:{port}/knowledge_platform_test");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("postgres should accept connections");
    (container, pool)
}

fn command(folder_uuid: Uuid, title: &str) -> CreateDocumentCommand {
    CreateDocumentCommand {
        folder_id: FolderId::from_uuid(folder_uuid),
        title: title.to_owned(),
        document_metadata: metadata("documentClass", "policy"),
        version_metadata: metadata("versionClass", "working"),
        principal: PrincipalRef::new("test-idp", "actor-42").expect("principal is valid"),
        original_filename: "policy.pdf".to_owned(),
        media_type: MediaType::new("application/pdf").expect("media type is valid"),
        content: Box::pin(Cursor::new(vec![1_u8, 2, 3])),
    }
}

fn direct_record(
    document_id: DocumentId,
    version_id: DocumentVersionId,
    file_id: FileId,
    folder_id: FolderId,
    now: OffsetDateTime,
) -> CreateInitialDocumentRecord {
    let descriptor = StoredFileDescriptor::new(
        StorageKey::new(format!("objects/test/{}", file_id.as_uuid()))
            .expect("storage key is valid"),
        ContentHash::from_slice(&[7_u8; 32]).expect("hash width is valid"),
        FileSize::new(3).expect("size is valid"),
        MediaType::new("application/pdf").expect("media type is valid"),
    );
    let initial = InitialDocument::create(CreateInitialDocument {
        document_id,
        version_id,
        file_id,
        folder_id,
        title: Title::new("Direct record").expect("title is valid"),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: PrincipalRef::new("test-idp", "actor-42").expect("principal is valid"),
        stored_file: descriptor,
        original_filename: "direct.pdf".to_owned(),
        created_at: now,
    })
    .expect("initial aggregate is valid");
    CreateInitialDocumentRecord::new(AuthoritativeDocument::from_initial(initial), vec![], vec![])
}

fn metadata(key: &str, value: &str) -> Metadata {
    let mut map = Map::<String, Value>::new();
    map.insert(key.to_owned(), Value::String(value.to_owned()));
    Metadata::from_map(map)
}

async fn count_where_uuid(pool: &PgPool, table: &str, column: &str, id: Uuid) -> i64 {
    let sql = format!("SELECT count(*) FROM {table} WHERE {column} = $1");
    sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| panic!("count query for {table}.{column} should succeed: {error}"))
}

async fn assert_attempt_rows_zero(
    pool: &PgPool,
    document_id: DocumentId,
    version_id: DocumentVersionId,
    file_id: FileId,
    failed_table: &str,
) {
    assert_eq!(
        count_where_uuid(pool, "documents", "document_id", document_id.as_uuid()).await,
        0,
        "documents must roll back after {failed_table} failure"
    );
    assert_eq!(
        count_where_uuid(
            pool,
            "document_versions",
            "document_version_id",
            version_id.as_uuid(),
        )
        .await,
        0,
        "document_versions must roll back after {failed_table} failure"
    );
    assert_eq!(
        count_where_uuid(pool, "file_objects", "file_id", file_id.as_uuid()).await,
        0,
        "file_objects must roll back after {failed_table} failure"
    );
    assert_eq!(
        count_where_uuid(
            pool,
            "version_files",
            "document_version_id",
            version_id.as_uuid(),
        )
        .await,
        0,
        "version_files must roll back after {failed_table} failure"
    );
    assert_eq!(
        count_where_uuid(
            pool,
            "outbox_events",
            "aggregate_id",
            document_id.as_uuid(),
        )
        .await,
        0,
        "outbox_events must roll back after {failed_table} failure"
    );
    assert_eq!(
        count_where_uuid(
            pool,
            "audit_outbox_events",
            "resource_id",
            document_id.as_uuid(),
        )
        .await,
        0,
        "audit_outbox_events must roll back after {failed_table} failure"
    );
}

struct SequenceIds {
    next: std::sync::atomic::AtomicU64,
}

impl SequenceIds {
    fn new(start: u128) -> Self {
        Self {
            next: std::sync::atomic::AtomicU64::new(start as u64),
        }
    }
}

impl IdGenerator for SequenceIds {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::from_u128(
            self.next
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128,
        )
    }
}

struct FixedClock(OffsetDateTime);

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}

struct StaticStorage;

impl FileStorage for StaticStorage {
    async fn put_immutable(&self, request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        let file_id = request.file_id();
        Ok(StoredFile::new(
            StorageKey::new(format!("objects/test/{}", file_id.as_uuid()))
                .expect("storage key is valid"),
            ContentHash::from_slice(&[7_u8; 32]).expect("hash width is valid"),
            FileSize::new(3).expect("size is valid"),
            request.media_type().clone(),
        ))
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        Err(StorageError::NotFound)
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(vec![])
    }
}
