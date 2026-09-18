use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Arc, Mutex},
};

use document_application::{
    ApplicationError, Clock, ContentReader, DocumentService, FileStorage, IdGenerator,
    PublishDocumentCommand, PublishOperationId, StorageError, StorageObjectInfo, StoreFileRequest,
    StoredFile,
};
use document_domain::{
    DocumentId, DocumentVersionId, FileId, PrincipalRef, StorageKey,
};
use document_repository_postgres::{migrate, PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use sqlx::{postgres::PgPoolOptions, PgPool};
use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};
use time::OffsetDateTime;
use tokio::sync::Barrier;
use uuid::Uuid;

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
            .expect("publish test id sequence should not exhaust")
    }
}

#[derive(Clone)]
struct ReadableStorage;

impl FileStorage for ReadableStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        Err(StorageError::Internal(
            "concurrency test does not write storage".to_owned(),
        ))
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        Ok(Box::pin(Cursor::new(b"authoritative".to_vec())))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn distinct_publish_operations_yield_exactly_one_success_and_one_conflict() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");

    let created_at = OffsetDateTime::from_unix_timestamp(1_700_010_000).unwrap();
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_010_100).unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
    let file_id = FileId::from_uuid(Uuid::from_u128(12));
    seed_initial(&pool, document_id, version_id, file_id, created_at).await;

    let barrier = Arc::new(Barrier::new(2));
    let service_a = publish_service(
        pool.clone(),
        published_at,
        [Uuid::from_u128(600), Uuid::from_u128(601)],
    );
    let service_b = publish_service(
        pool.clone(),
        published_at,
        [Uuid::from_u128(602), Uuid::from_u128(603)],
    );
    let command_a = publish_command(publish_operation_id(20));
    let command_b = publish_command(publish_operation_id(21));

    let barrier_a = barrier.clone();
    let barrier_b = barrier.clone();
    let worker_a = async move {
        barrier_a.wait().await;
        service_a.publish_document(command_a).await
    };
    let worker_b = async move {
        barrier_b.wait().await;
        service_b.publish_document(command_b).await
    };

    let (result_a, result_b) = tokio::join!(worker_a, worker_b);
    let results = [result_a, result_b];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ApplicationError::Conflict)))
            .count(),
        1
    );

    assert_final_publish_state(&pool, document_id, version_id).await;
}

#[tokio::test]
async fn same_publish_operation_concurrently_replays_one_committed_result() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");

    let created_at = OffsetDateTime::from_unix_timestamp(1_700_011_000).unwrap();
    let published_at = OffsetDateTime::from_unix_timestamp(1_700_011_100).unwrap();
    let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
    let file_id = FileId::from_uuid(Uuid::from_u128(12));
    seed_initial(&pool, document_id, version_id, file_id, created_at).await;

    let barrier = Arc::new(Barrier::new(2));
    let service_a = publish_service(
        pool.clone(),
        published_at,
        [Uuid::from_u128(610), Uuid::from_u128(611)],
    );
    let service_b = publish_service(
        pool.clone(),
        published_at,
        [Uuid::from_u128(612), Uuid::from_u128(613)],
    );
    let command = publish_command(publish_operation_id(22));

    let barrier_a = barrier.clone();
    let barrier_b = barrier.clone();
    let command_b = command.clone();
    let worker_a = async move {
        barrier_a.wait().await;
        service_a.publish_document(command).await
    };
    let worker_b = async move {
        barrier_b.wait().await;
        service_b.publish_document(command_b).await
    };

    let (result_a, result_b) = tokio::join!(worker_a, worker_b);
    let result_a = result_a.expect("first same-operation caller should succeed");
    let result_b = result_b.expect("second same-operation caller should replay");
    assert_eq!(result_a, result_b);

    assert_final_publish_state(&pool, document_id, version_id).await;
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

fn publish_command(operation_id: PublishOperationId) -> PublishDocumentCommand {
    PublishDocumentCommand::new(
        operation_id,
        DocumentId::from_uuid(Uuid::from_u128(10)),
        DocumentVersionId::from_uuid(Uuid::from_u128(11)),
        0,
        PrincipalRef::new("test-idp", "actor-1").unwrap(),
    )
    .unwrap()
}

fn publish_operation_id(value: u8) -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8000-{value:012x}")).unwrap(),
    )
    .unwrap()
}

async fn assert_final_publish_state(
    pool: &PgPool,
    document_id: DocumentId,
    version_id: DocumentVersionId,
) {
    let state: (Option<Uuid>, i64, String) = sqlx::query_as(
        "SELECT d.current_version_id, d.revision, v.lifecycle_state \
         FROM documents d \
         JOIN document_versions v ON v.document_id = d.document_id \
           AND v.document_version_id = $2 \
         WHERE d.document_id = $1",
    )
    .bind(document_id.as_uuid())
    .bind(version_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("final publish state should query");

    assert_eq!(state.0, Some(version_id.as_uuid()));
    assert_eq!(state.1, 1);
    assert_eq!(state.2, "PUBLISHED");

    let operations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM document_publish_operations WHERE document_id = $1")
            .bind(document_id.as_uuid())
            .fetch_one(pool)
            .await
            .expect("publish operation count should query");
    assert_eq!(operations, 1);

    let domain_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM outbox_events \
         WHERE aggregate_id = $1 AND event_type = 'DocumentVersionPublished'",
    )
    .bind(document_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("publish domain event count should query");
    assert_eq!(domain_events, 1);

    let audit_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_outbox_events \
         WHERE resource_id = $1 AND event_type = 'document.version.published'",
    )
    .bind(document_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("publish audit event count should query");
    assert_eq!(audit_events, 1);
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
        .max_connections(6)
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
         VALUES ($1, $2, 'application/pdf', 13, $3, $4)",
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
