use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Arc, Mutex},
};

use document_application::{
    ApplicationError, Clock, ContentReader, DocumentRepository, DocumentService, FileStorage,
    IdGenerator, PublishDocumentCommand, PublishOperationId, StorageError, StorageObjectInfo,
    StoreFileRequest, StoredFile,
};
use document_domain::{
    DocumentId, DocumentVersionId, FileId, LifecycleState, PrincipalRef, StorageKey,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

#[tokio::test]
async fn get_document_round_trips_initial_published_state() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");

    let created_at = OffsetDateTime::from_unix_timestamp(1_700_002_000).unwrap();
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_002_100).unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
    let file_id = FileId::from_uuid(Uuid::from_u128(12));

    seed_initial(&pool, document_id, version_id, file_id, created_at).await;

    let mut tx = pool
        .begin()
        .await
        .expect("publish seed transaction should begin");
    sqlx::query(
        "UPDATE document_versions \
         SET lifecycle_state = 'PUBLISHED', published_at = $1 \
         WHERE document_version_id = $2",
    )
    .bind(published_at)
    .bind(version_id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("version should become published");
    sqlx::query(
        "UPDATE documents \
         SET current_version_id = $1, revision = 1 \
         WHERE document_id = $2",
    )
    .bind(version_id.as_uuid())
    .bind(document_id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("document should point at published version");
    tx.commit()
        .await
        .expect("publish seed transaction should commit");

    let repository = PostgresDocumentRepository::new(pool);
    let loaded = repository
        .get_authoritative_document(document_id)
        .await
        .expect("published authoritative read should not fail")
        .expect("document should exist");

    assert_eq!(loaded.document().document_id(), document_id);
    assert_eq!(loaded.document().current_version_id(), Some(version_id));
    assert_eq!(loaded.document().revision(), 1);
    assert_eq!(loaded.version().document_version_id(), version_id);
    assert_eq!(
        loaded.version().lifecycle_state(),
        LifecycleState::Published
    );
    assert_eq!(loaded.version().published_at(), Some(published_at));
    assert_eq!(loaded.file().file_id(), file_id);
}

#[tokio::test]
async fn publish_transaction_commits_state_events_audit_and_operation_atomically() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");
    let created_at = OffsetDateTime::from_unix_timestamp(1_700_003_000).unwrap();
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_003_100).unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
    let file_id = FileId::from_uuid(Uuid::from_u128(12));
    seed_initial(&pool, document_id, version_id, file_id, created_at).await;

    let operation_id = publish_operation_id(1);
    let service = publish_service(
        pool.clone(),
        published_at,
        [Uuid::from_u128(500), Uuid::from_u128(501)],
    );
    let result = service
        .publish_document(publish_command(operation_id, 0, "actor-1"))
        .await
        .expect("initial publish should commit");

    assert_eq!(result.publish_operation_id(), operation_id);
    assert_eq!(result.document_id(), document_id);
    assert_eq!(result.document_version_id(), version_id);
    assert_eq!(result.resulting_document_revision(), 1);
    assert_eq!(result.published_at(), published_at);

    let state: (Option<Uuid>, i64, String, Option<OffsetDateTime>) = sqlx::query_as(
        "SELECT d.current_version_id, d.revision, v.lifecycle_state, v.published_at \
         FROM documents d \
         JOIN document_versions v ON v.document_id = d.document_id AND v.document_version_id = $2 \
         WHERE d.document_id = $1",
    )
    .bind(document_id.as_uuid())
    .bind(version_id.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("published state should be queryable");
    assert_eq!(state.0, Some(version_id.as_uuid()));
    assert_eq!(state.1, 1);
    assert_eq!(state.2, "PUBLISHED");
    assert_eq!(state.3, Some(published_at));

    assert_eq!(count_publish_operations(&pool, document_id).await, 1);
    assert_eq!(
        count_event_type(&pool, "outbox_events", "DocumentVersionPublished").await,
        1
    );
    assert_eq!(
        count_event_type(&pool, "audit_outbox_events", "document.version.published",).await,
        1
    );
}

#[tokio::test]
async fn publish_replay_is_idempotent_and_operation_id_misuse_conflicts() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");
    let created_at = OffsetDateTime::from_unix_timestamp(1_700_003_200).unwrap();
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_003_300).unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
    let file_id = FileId::from_uuid(Uuid::from_u128(12));
    seed_initial(&pool, document_id, version_id, file_id, created_at).await;

    let operation_id = publish_operation_id(2);
    let service = publish_service(
        pool.clone(),
        published_at,
        [
            Uuid::from_u128(510),
            Uuid::from_u128(511),
            Uuid::from_u128(512),
            Uuid::from_u128(513),
        ],
    );
    let command = publish_command(operation_id, 0, "actor-1");

    let first = service
        .publish_document(command.clone())
        .await
        .expect("first publish should commit");
    let replay = service
        .publish_document(command)
        .await
        .expect("same operation should replay");
    assert_eq!(first, replay);
    assert_eq!(count_publish_operations(&pool, document_id).await, 1);
    assert_eq!(
        count_event_type(&pool, "outbox_events", "DocumentVersionPublished").await,
        1
    );
    assert_eq!(
        count_event_type(&pool, "audit_outbox_events", "document.version.published",).await,
        1
    );

    let misuse = service
        .publish_document(publish_command(operation_id, 0, "actor-2"))
        .await
        .unwrap_err();
    assert_eq!(misuse, ApplicationError::Conflict);
}

#[tokio::test]
async fn publish_stale_revision_conflicts_without_partial_state() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");
    let created_at = OffsetDateTime::from_unix_timestamp(1_700_003_400).unwrap();
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_003_500).unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
    let file_id = FileId::from_uuid(Uuid::from_u128(12));
    seed_initial(&pool, document_id, version_id, file_id, created_at).await;

    let service = publish_service(
        pool.clone(),
        published_at,
        [Uuid::from_u128(520), Uuid::from_u128(521)],
    );
    let error = service
        .publish_document(publish_command(publish_operation_id(3), 1, "actor-1"))
        .await
        .unwrap_err();
    assert_eq!(error, ApplicationError::Conflict);
    assert_initial_state(&pool, document_id, version_id).await;
    assert_eq!(count_publish_operations(&pool, document_id).await, 0);
}

#[tokio::test]
async fn publish_existing_other_current_conflicts() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");
    let created_at = OffsetDateTime::from_unix_timestamp(1_700_003_600).unwrap();
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_003_700).unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
    let file_id = FileId::from_uuid(Uuid::from_u128(12));
    let other_version_id = DocumentVersionId::from_uuid(Uuid::from_u128(13));
    seed_initial(&pool, document_id, version_id, file_id, created_at).await;

    sqlx::query(
        "INSERT INTO document_versions \
         (document_version_id, document_id, version_no, lifecycle_state, title, published_at, \
          created_by_identity_provider, created_by_principal_id, metadata, created_at) \
         VALUES ($1, $2, 2, 'PUBLISHED', 'Policy v2', $3, \
                 'test-idp', 'creator-1', '{}'::jsonb, $4)",
    )
    .bind(other_version_id.as_uuid())
    .bind(document_id.as_uuid())
    .bind(published_at)
    .bind(created_at)
    .execute(&pool)
    .await
    .expect("other published version should insert");
    sqlx::query(
        "UPDATE documents SET current_version_id = $1, revision = 1 WHERE document_id = $2",
    )
    .bind(other_version_id.as_uuid())
    .bind(document_id.as_uuid())
    .execute(&pool)
    .await
    .expect("other version should become current");

    let service = publish_service(
        pool,
        published_at,
        [Uuid::from_u128(530), Uuid::from_u128(531)],
    );
    let error = service
        .publish_document(publish_command(publish_operation_id(4), 1, "actor-1"))
        .await
        .unwrap_err();
    assert_eq!(error, ApplicationError::Conflict);
}

#[tokio::test]
async fn publish_withdrawn_target_is_business_rule() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");
    let created_at = OffsetDateTime::from_unix_timestamp(1_700_003_800).unwrap();
    let changed_at = OffsetDateTime::from_unix_timestamp(1_700_003_900).unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
    let file_id = FileId::from_uuid(Uuid::from_u128(12));
    seed_initial(&pool, document_id, version_id, file_id, created_at).await;
    sqlx::query(
        "UPDATE document_versions \
         SET lifecycle_state = 'WITHDRAWN', withdrawn_at = $1 \
         WHERE document_version_id = $2",
    )
    .bind(changed_at)
    .bind(version_id.as_uuid())
    .execute(&pool)
    .await
    .expect("withdrawn fixture should update");

    let service = publish_service(
        pool,
        changed_at,
        [Uuid::from_u128(540), Uuid::from_u128(541)],
    );
    let error = service
        .publish_document(publish_command(publish_operation_id(5), 0, "actor-1"))
        .await
        .unwrap_err();
    assert_eq!(error, ApplicationError::BusinessRule);
}

#[tokio::test]
async fn publish_outbox_collision_rolls_back_state_operation_and_audit() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");
    let created_at = OffsetDateTime::from_unix_timestamp(1_700_004_000).unwrap();
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_004_100).unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
    let file_id = FileId::from_uuid(Uuid::from_u128(12));
    let collision_event_id = Uuid::from_u128(550);
    seed_initial(&pool, document_id, version_id, file_id, created_at).await;

    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at, \
          available_at, attempt_count, delivered_at) \
         VALUES ($1, 'CollisionFixture', 'Document', $2, '{}'::jsonb, $3, $3, 0, NULL)",
    )
    .bind(collision_event_id)
    .bind(document_id.as_uuid())
    .bind(created_at)
    .execute(&pool)
    .await
    .expect("collision fixture should insert");

    let service = publish_service(
        pool.clone(),
        published_at,
        [collision_event_id, Uuid::from_u128(551)],
    );
    let error = service
        .publish_document(publish_command(publish_operation_id(6), 0, "actor-1"))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ApplicationError::Internal("postgres operation failed".to_owned())
    );

    assert_initial_state(&pool, document_id, version_id).await;
    assert_eq!(count_publish_operations(&pool, document_id).await, 0);
    assert_eq!(
        count_event_type(&pool, "audit_outbox_events", "document.version.published",).await,
        0
    );
    assert_eq!(
        count_event_type(&pool, "outbox_events", "CollisionFixture").await,
        1
    );
}

#[derive(Clone)]
struct FixedClock(OffsetDateTime);

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}

#[derive(Clone)]
struct SequenceIds {
    values: Arc<Mutex<VecDeque<Uuid>>>,
}

impl SequenceIds {
    fn new(values: impl IntoIterator<Item = Uuid>) -> Self {
        Self {
            values: Arc::new(Mutex::new(values.into_iter().collect())),
        }
    }
}

impl IdGenerator for SequenceIds {
    fn next_uuid_v7(&self) -> Uuid {
        self.values
            .lock()
            .expect("id queue lock should not poison")
            .pop_front()
            .expect("test id sequence should not exhaust")
    }
}

#[derive(Clone)]
struct ReadableStorage;

impl FileStorage for ReadableStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        Err(StorageError::Internal(
            "publish transaction tests do not write storage".to_owned(),
        ))
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        Ok(Box::pin(Cursor::new(b"authoritative".to_vec())))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

fn publish_service(
    pool: PgPool,
    now: OffsetDateTime,
    ids: impl IntoIterator<Item = Uuid>,
) -> DocumentService<SequenceIds, FixedClock, ReadableStorage, PostgresDocumentRepository> {
    DocumentService::new(
        Arc::new(SequenceIds::new(ids)),
        Arc::new(FixedClock(now)),
        Arc::new(ReadableStorage),
        Arc::new(PostgresDocumentRepository::new(pool)),
    )
}

fn publish_operation_id(value: u8) -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8000-{value:012x}")).unwrap(),
    )
    .unwrap()
}

fn publish_command(
    operation_id: PublishOperationId,
    expected_revision: i64,
    actor: &str,
) -> PublishDocumentCommand {
    PublishDocumentCommand::new(
        operation_id,
        DocumentId::from_uuid(Uuid::from_u128(10)),
        DocumentVersionId::from_uuid(Uuid::from_u128(11)),
        expected_revision,
        PrincipalRef::new("test-idp", actor).unwrap(),
    )
    .unwrap()
}

async fn count_publish_operations(pool: &PgPool, document_id: DocumentId) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM document_publish_operations WHERE document_id = $1")
        .bind(document_id.as_uuid())
        .fetch_one(pool)
        .await
        .expect("publish operation count should query")
}

async fn count_event_type(pool: &PgPool, table: &str, event_type: &str) -> i64 {
    match table {
        "outbox_events" => sqlx::query_scalar(
            "SELECT COUNT(*) FROM outbox_events WHERE event_type = $1",
        )
        .bind(event_type)
        .fetch_one(pool)
        .await
        .expect("domain event count should query"),
        "audit_outbox_events" => sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_outbox_events WHERE event_type = $1",
        )
        .bind(event_type)
        .fetch_one(pool)
        .await
        .expect("audit event count should query"),
        _ => panic!("unsupported event table fixture"),
    }
}

async fn assert_initial_state(
    pool: &PgPool,
    document_id: DocumentId,
    version_id: DocumentVersionId,
) {
    let state: (Option<Uuid>, i64, String, Option<OffsetDateTime>) = sqlx::query_as(
        "SELECT d.current_version_id, d.revision, v.lifecycle_state, v.published_at \
         FROM documents d \
         JOIN document_versions v ON v.document_id = d.document_id AND v.document_version_id = $2 \
         WHERE d.document_id = $1",
    )
    .bind(document_id.as_uuid())
    .bind(version_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("initial state should query");
    assert_eq!(state.0, None);
    assert_eq!(state.1, 0);
    assert_eq!(state.2, "WORKING");
    assert_eq!(state.3, None);
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

async fn seed_initial(
    pool: &PgPool,
    document_id: DocumentId,
    version_id: DocumentVersionId,
    file_id: FileId,
    created_at: OffsetDateTime,
) {
    sqlx::query(
        "INSERT INTO documents \
         (document_id, folder_id, current_version_id, revision, metadata, created_at) \
         VALUES ($1, $2, NULL, 0, '{}'::jsonb, $3)",
    )
    .bind(document_id.as_uuid())
    .bind(SYSTEM_ROOT_FOLDER_ID)
    .bind(created_at)
    .execute(pool)
    .await
    .expect("document should insert");

    sqlx::query(
        "INSERT INTO document_versions \
         (document_version_id, document_id, version_no, lifecycle_state, title, \
          created_by_identity_provider, created_by_principal_id, metadata, created_at) \
         VALUES ($1, $2, 1, 'WORKING', 'Policy v1', \
                 'test-idp', 'creator-1', '{}'::jsonb, $3)",
    )
    .bind(version_id.as_uuid())
    .bind(document_id.as_uuid())
    .bind(created_at)
    .execute(pool)
    .await
    .expect("version should insert");

    sqlx::query(
        "INSERT INTO file_objects \
         (file_id, content_hash, media_type, size_bytes, storage_locator, created_at) \
         VALUES ($1, $2, 'application/pdf', 3, $3, $4)",
    )
    .bind(file_id.as_uuid())
    .bind(vec![7_u8; 32])
    .bind(format!("objects/{}/file", file_id.as_uuid()))
    .bind(created_at)
    .execute(pool)
    .await
    .expect("file should insert");

    sqlx::query(
        "INSERT INTO version_files \
         (document_version_id, file_id, role, ordinal, original_filename) \
         VALUES ($1, $2, 'PRIMARY', 0, 'policy.pdf')",
    )
    .bind(version_id.as_uuid())
    .bind(file_id.as_uuid())
    .execute(pool)
    .await
    .expect("primary file link should insert");
}
