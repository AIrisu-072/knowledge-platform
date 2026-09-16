use std::io::Cursor;
use std::sync::Arc;

use document_application::{
    ApplicationError, Clock, ContentReader, CreateDocumentCommand, DocumentService, IdGenerator,
};
use document_domain::{FolderId, LifecycleState, MediaType, Metadata, PrincipalRef};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use document_storage_fs::FileSystemStorage;
use sqlx::postgres::PgPoolOptions;
use tempfile::TempDir;
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

const INPUT: &[u8] = b"vertical-slice-content";
const INPUT_SHA256: [u8; 32] = [
    184, 170, 32, 88, 29, 1, 5, 103, 152, 246, 85, 123, 33, 235, 91, 157, 18, 242, 5, 93, 204, 33,
    121, 74, 47, 185, 175, 16, 171, 197, 248, 208,
];

#[derive(Debug)]
struct UuidV7Generator;

impl IdGenerator for UuidV7Generator {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

#[derive(Debug)]
struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

fn content() -> ContentReader {
    Box::pin(Cursor::new(INPUT.to_vec()))
}

#[tokio::test]
async fn real_filesystem_and_postgres_round_trip_authoritative_create_get_open() {
    let postgres = GenericImage::new("postgres", "18.6-bookworm")
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

    let port = postgres
        .get_host_port_ipv4(5432.tcp())
        .await
        .expect("postgres port should be mapped");
    let database_url =
        format!("postgres://postgres:postgres@127.0.0.1:{port}/knowledge_platform_test");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("postgres should accept connections after readiness signal");
    migrate(&pool).await.expect("migration should succeed");

    let storage_root = TempDir::new().expect("temporary storage root should be created");
    let service = DocumentService::new(
        Arc::new(UuidV7Generator),
        Arc::new(SystemClock),
        Arc::new(FileSystemStorage::new(storage_root.path())),
        Arc::new(PostgresDocumentRepository::new(pool.clone())),
    );

    let created = service
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
            title: "Vertical Slice Policy".into(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: PrincipalRef::new("test", "principal-1").unwrap(),
            original_filename: "policy.txt".into(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: content(),
        })
        .await
        .expect("real CreateDocument should succeed");

    let authoritative = service
        .get_document(created.document_id())
        .await
        .expect("real GetDocument should return authoritative state");

    assert_eq!(authoritative.version().version_no().get(), 1);
    assert_eq!(
        authoritative.version().lifecycle_state(),
        LifecycleState::Working
    );
    assert_eq!(authoritative.document().current_version_id(), None);
    assert_eq!(authoritative.document().revision(), 0);
    assert_eq!(
        authoritative.file().content_hash().as_bytes(),
        &INPUT_SHA256
    );

    let mut reader = service
        .open_primary_file(created.document_id())
        .await
        .expect("real open_primary_file should open finalized bytes");
    let mut read_back = Vec::new();
    reader
        .read_to_end(&mut read_back)
        .await
        .expect("finalized file should be readable");
    assert_eq!(read_back, INPUT);

    let file_objects: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM file_objects WHERE file_id = $1")
            .bind(created.file_id().as_uuid())
            .fetch_one(&pool)
            .await
            .expect("file object count query should succeed");
    assert_eq!(file_objects, 1);

    let primary_version_files: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM version_files \
         WHERE document_version_id = $1 AND role = 'PRIMARY'",
    )
    .bind(created.document_version_id().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("version file count query should succeed");
    assert_eq!(primary_version_files, 1);

    let domain_outbox: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE aggregate_id = $1")
            .bind(created.document_id().as_uuid())
            .fetch_one(&pool)
            .await
            .expect("domain outbox count query should succeed");
    assert_eq!(domain_outbox, 2);

    let audit_outbox: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_outbox_events WHERE resource_id = $1")
            .bind(created.document_id().as_uuid())
            .fetch_one(&pool)
            .await
            .expect("audit outbox count query should succeed");
    assert_eq!(audit_outbox, 2);
}

#[tokio::test]
async fn missing_physical_file_preserves_authoritative_state_and_surfaces_integrity_violation() {
    let postgres = GenericImage::new("postgres", "18.6-bookworm")
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

    let port = postgres
        .get_host_port_ipv4(5432.tcp())
        .await
        .expect("postgres port should be mapped");
    let database_url =
        format!("postgres://postgres:postgres@127.0.0.1:{port}/knowledge_platform_test");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("postgres should accept connections after readiness signal");
    migrate(&pool).await.expect("migration should succeed");

    let storage_root = TempDir::new().expect("temporary storage root should be created");
    let service = DocumentService::new(
        Arc::new(UuidV7Generator),
        Arc::new(SystemClock),
        Arc::new(FileSystemStorage::new(storage_root.path())),
        Arc::new(PostgresDocumentRepository::new(pool.clone())),
    );

    let created = service
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
            title: "Corruption Policy".into(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: PrincipalRef::new("test", "principal-2").unwrap(),
            original_filename: "corruption.txt".into(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: content(),
        })
        .await
        .expect("real CreateDocument should succeed");

    let before_loss = service
        .get_document(created.document_id())
        .await
        .expect("authoritative state should exist before physical loss");
    let final_path = storage_root
        .path()
        .join(before_loss.file().storage_key().as_str());
    std::fs::remove_file(&final_path).expect("test should remove the finalized physical file");

    let after_loss = service
        .get_document(created.document_id())
        .await
        .expect("authoritative state must survive physical file loss");
    assert_eq!(after_loss.document().document_id(), created.document_id());
    assert_eq!(after_loss.version().version_no().get(), 1);
    assert_eq!(after_loss.file().file_id(), created.file_id());

    let error = match service.open_primary_file(created.document_id()).await {
        Ok(_) => panic!("missing referenced binary must not be reported as a successful read"),
        Err(error) => error,
    };
    assert_eq!(error, ApplicationError::IntegrityViolation);

    let documents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents WHERE document_id = $1")
        .bind(created.document_id().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("document count query should succeed");
    assert_eq!(documents, 1);

    let versions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM document_versions WHERE document_version_id = $1",
    )
    .bind(created.document_version_id().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("document version count query should succeed");
    assert_eq!(versions, 1);

    let file_objects: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM file_objects WHERE file_id = $1")
            .bind(created.file_id().as_uuid())
            .fetch_one(&pool)
            .await
            .expect("file object count query should succeed");
    assert_eq!(file_objects, 1);

    let primary_version_files: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM version_files \
         WHERE document_version_id = $1 AND role = 'PRIMARY'",
    )
    .bind(created.document_version_id().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("version file count query should succeed");
    assert_eq!(primary_version_files, 1);

    let domain_outbox: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE aggregate_id = $1")
            .bind(created.document_id().as_uuid())
            .fetch_one(&pool)
            .await
            .expect("domain outbox count query should succeed");
    assert_eq!(domain_outbox, 2);

    let audit_outbox: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_outbox_events WHERE resource_id = $1")
            .bind(created.document_id().as_uuid())
            .fetch_one(&pool)
            .await
            .expect("audit outbox count query should succeed");
    assert_eq!(audit_outbox, 2);
}
