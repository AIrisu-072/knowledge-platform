use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use document_application::{
    ApplicationError, AuthoritativeDocument, Clock, ContentReader, CreateDocumentCommand,
    CreateDocumentResult, CreateInitialDocumentRecord, DocumentPublishRepository,
    DocumentRepository, DocumentService, FileStorage, IdGenerator, PublishCandidate,
    PublishDocumentCommand, PublishDocumentResult, PublishInitialVersionRecord, PublishOperationId,
    PublishOperationRecord, RepositoryError,
};
use document_domain::{
    DocumentId, FileId, FolderId, LifecycleState, MediaType, Metadata, PrincipalRef,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use document_storage_fs::FileSystemStorage;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tempfile::TempDir;
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

const INPUT: &[u8] = b"publish-vertical-slice-content";

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
            .expect("id sequence lock should not poison")
            .pop_front()
            .expect("id sequence should not exhaust")
    }
}

#[derive(Debug, Clone, Copy)]
enum UnknownCommitMode {
    BeforeCommit,
    AfterCommit,
}

#[derive(Clone)]
struct UnknownCommitRepository {
    inner: PostgresDocumentRepository,
    mode: UnknownCommitMode,
    first_publish: Arc<AtomicBool>,
}

impl UnknownCommitRepository {
    fn new(inner: PostgresDocumentRepository, mode: UnknownCommitMode) -> Self {
        Self {
            inner,
            mode,
            first_publish: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl DocumentRepository for UnknownCommitRepository {
    async fn create_initial_document(
        &self,
        record: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        self.inner.create_initial_document(record).await
    }

    async fn get_authoritative_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        self.inner.get_authoritative_document(id).await
    }

    async fn file_reference_exists(&self, file_id: FileId) -> Result<bool, RepositoryError> {
        self.inner.file_reference_exists(file_id).await
    }

    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError> {
        self.inner.list_referenced_file_ids().await
    }
}

impl DocumentPublishRepository for UnknownCommitRepository {
    async fn get_publish_operation(
        &self,
        operation_id: PublishOperationId,
    ) -> Result<Option<PublishOperationRecord>, RepositoryError> {
        self.inner.get_publish_operation(operation_id).await
    }

    async fn get_publish_candidate(
        &self,
        document_id: DocumentId,
        target_version_id: document_domain::DocumentVersionId,
    ) -> Result<PublishCandidate, RepositoryError> {
        self.inner
            .get_publish_candidate(document_id, target_version_id)
            .await
    }

    async fn publish_initial_version(
        &self,
        record: PublishInitialVersionRecord,
    ) -> Result<PublishDocumentResult, RepositoryError> {
        if self.first_publish.swap(false, Ordering::SeqCst) {
            match self.mode {
                UnknownCommitMode::BeforeCommit => {
                    return Err(RepositoryError::CommitOutcomeUnknown);
                }
                UnknownCommitMode::AfterCommit => {
                    self.inner.publish_initial_version(record).await?;
                    return Err(RepositoryError::CommitOutcomeUnknown);
                }
            }
        }

        self.inner.publish_initial_version(record).await
    }
}

fn fixed_v7(value: u64) -> Uuid {
    Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8000-{value:012x}"))
        .expect("fixture UUIDv7 should parse")
}

fn server_ids(start: u64) -> Vec<Uuid> {
    (start..start + 16).map(fixed_v7).collect()
}

fn content() -> ContentReader {
    Box::pin(Cursor::new(INPUT.to_vec()))
}

fn create_command(principal_id: &str) -> CreateDocumentCommand {
    CreateDocumentCommand {
        folder_id: FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
        title: "Publish Vertical Slice Policy".to_owned(),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: PrincipalRef::new("test-idp", principal_id).unwrap(),
        original_filename: "policy.txt".to_owned(),
        media_type: MediaType::new("text/plain").unwrap(),
        content: content(),
    }
}

async fn create_working<I, C, F, R>(
    service: &DocumentService<I, C, F, R>,
    principal_id: &str,
) -> CreateDocumentResult
where
    I: IdGenerator,
    C: Clock,
    F: FileStorage,
    R: DocumentRepository,
{
    service
        .create_document(create_command(principal_id))
        .await
        .expect("CreateDocument should succeed")
}

#[tokio::test]
async fn real_create_publish_get_and_open_round_trip() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");
    let storage_root = TempDir::new().expect("temporary storage root should be created");
    let now = OffsetDateTime::from_unix_timestamp(1_700_020_000).unwrap();

    let service = DocumentService::new(
        Arc::new(SequenceIds::new(server_ids(100))),
        Arc::new(FixedClock(now)),
        Arc::new(FileSystemStorage::new(storage_root.path())),
        Arc::new(PostgresDocumentRepository::new(pool.clone())),
    );

    let created = create_working(&service, "creator-1").await;
    let working = service
        .get_document(created.document_id())
        .await
        .expect("working document should load");
    assert_eq!(working.version().lifecycle_state(), LifecycleState::Working);
    assert_eq!(working.document().current_version_id(), None);
    assert_eq!(working.document().revision(), 0);

    let operation_id = PublishOperationId::try_from_uuid(fixed_v7(900)).unwrap();
    let command = PublishDocumentCommand::new(
        operation_id,
        created.document_id(),
        created.document_version_id(),
        0,
        PrincipalRef::new("test-idp", "publisher-1").unwrap(),
    )
    .unwrap();

    let published = service
        .publish_document(command)
        .await
        .expect("initial Publish should succeed");
    assert_eq!(published.publish_operation_id(), operation_id);
    assert_eq!(published.resulting_document_revision(), 1);

    let authoritative = service
        .get_document(created.document_id())
        .await
        .expect("published document should load");
    assert_eq!(
        authoritative.version().lifecycle_state(),
        LifecycleState::Published
    );
    assert_eq!(
        authoritative.document().current_version_id(),
        Some(created.document_version_id())
    );
    assert_eq!(authoritative.document().revision(), 1);
    assert_eq!(authoritative.version().published_at(), Some(now));

    let mut reader = service
        .open_primary_file(created.document_id())
        .await
        .expect("published primary file should remain readable");
    let mut read_back = Vec::new();
    reader
        .read_to_end(&mut read_back)
        .await
        .expect("published bytes should read");
    assert_eq!(read_back, INPUT);

    assert_publish_counts(&pool, created.document_id()).await;
}

#[tokio::test]
async fn before_commit_unknown_retries_exact_command_without_duplicate_publish() {
    exercise_unknown_commit(UnknownCommitMode::BeforeCommit, 200, 901).await;
}

#[tokio::test]
async fn after_commit_unknown_retries_exact_command_as_stored_replay() {
    exercise_unknown_commit(UnknownCommitMode::AfterCommit, 300, 902).await;
}

async fn exercise_unknown_commit(mode: UnknownCommitMode, id_start: u64, operation_raw: u64) {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.expect("migrations should succeed");
    let storage_root = TempDir::new().expect("temporary storage root should be created");
    let now = OffsetDateTime::from_unix_timestamp(1_700_021_000 + id_start as i64).unwrap();
    let repository =
        UnknownCommitRepository::new(PostgresDocumentRepository::new(pool.clone()), mode);
    let service = DocumentService::new(
        Arc::new(SequenceIds::new(server_ids(id_start))),
        Arc::new(FixedClock(now)),
        Arc::new(FileSystemStorage::new(storage_root.path())),
        Arc::new(repository),
    );

    let created = create_working(&service, "creator-unknown").await;
    let operation_id = PublishOperationId::try_from_uuid(fixed_v7(operation_raw)).unwrap();
    let command = PublishDocumentCommand::new(
        operation_id,
        created.document_id(),
        created.document_version_id(),
        0,
        PrincipalRef::new("test-idp", "publisher-unknown").unwrap(),
    )
    .unwrap();

    let first = service.publish_document(command.clone()).await.unwrap_err();
    assert_eq!(
        first,
        ApplicationError::PublishCommitOutcomeUnknown {
            publish_operation_id: operation_id,
            document_id: created.document_id(),
            document_version_id: created.document_version_id(),
        }
    );

    let retry = service
        .publish_document(command)
        .await
        .expect("exact-command retry should resolve unknown commit");
    assert_eq!(retry.publish_operation_id(), operation_id);
    assert_eq!(retry.resulting_document_revision(), 1);

    let authoritative = service
        .get_document(created.document_id())
        .await
        .expect("final published document should load");
    assert_eq!(
        authoritative.version().lifecycle_state(),
        LifecycleState::Published
    );
    assert_eq!(authoritative.document().revision(), 1);
    assert_eq!(
        authoritative.document().current_version_id(),
        Some(created.document_version_id())
    );

    assert_publish_counts(&pool, created.document_id()).await;
}

async fn assert_publish_counts(pool: &PgPool, document_id: DocumentId) {
    let operations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM document_publish_operations WHERE document_id = $1",
    )
    .bind(document_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("operation count should query");
    assert_eq!(operations, 1);

    let domain_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE aggregate_id = $1")
            .bind(document_id.as_uuid())
            .fetch_one(pool)
            .await
            .expect("domain event count should query");
    assert_eq!(domain_events, 3);

    let publish_domain_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM outbox_events \
         WHERE aggregate_id = $1 AND event_type = 'DocumentVersionPublished'",
    )
    .bind(document_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("publish domain event count should query");
    assert_eq!(publish_domain_events, 1);

    let audit_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_outbox_events WHERE resource_id = $1")
            .bind(document_id.as_uuid())
            .fetch_one(pool)
            .await
            .expect("audit event count should query");
    assert_eq!(audit_events, 3);

    let publish_audit_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_outbox_events \
         WHERE resource_id = $1 AND event_type = 'document.version.published'",
    )
    .bind(document_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("publish audit event count should query");
    assert_eq!(publish_audit_events, 1);
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
