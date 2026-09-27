use document_application::{RepositoryError, SemanticInspectionRepository, VersioningRepository};
use document_domain::{
    ContentHash, FileId, FileObject, FileSize, MediaType, StorageKey, StoredFileDescriptor,
};
use document_repository_postgres::{PostgresDocumentRepository, migrate};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

fn file(hash: u8, created_at: i64) -> FileObject {
    FileObject::restore(
        FileId::from_uuid(Uuid::from_u128(1)),
        StoredFileDescriptor::new(
            StorageKey::new("objects/one").unwrap(),
            ContentHash::from_slice(&[hash; 32]).unwrap(),
            FileSize::new(3).unwrap(),
            MediaType::new("text/plain").unwrap(),
        ),
        OffsetDateTime::from_unix_timestamp(created_at).unwrap(),
    )
}

async fn postgres() -> (testcontainers::ContainerAsync<GenericImage>, PgPool) {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "versioning_registration_test")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/versioning_registration_test"
        ))
        .await
        .unwrap();
    (container, pool)
}

#[tokio::test]
async fn registration_replays_only_same_immutable_raw_binding() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.unwrap();
    let repository = PostgresDocumentRepository::new(pool.clone());
    repository.register_file_object(file(7, 1)).await.unwrap();
    repository.register_file_object(file(7, 2)).await.unwrap();
    assert_eq!(
        repository.register_file_object(file(8, 3)).await,
        Err(RepositoryError::IntegrityViolation)
    );
    let persisted = repository
        .get_file_object(FileId::from_uuid(Uuid::from_u128(1)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.content_hash().as_bytes(), &[7; 32]);
    assert_eq!(
        persisted.created_at(),
        OffsetDateTime::from_unix_timestamp(1).unwrap()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM file_objects WHERE file_id = $1")
        .bind(Uuid::from_u128(1))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
