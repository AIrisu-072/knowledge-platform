use document_application::DocumentRepository;
use document_domain::{DocumentId, DocumentVersionId, FileId, LifecycleState};
use document_repository_postgres::{
    PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate,
};
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
