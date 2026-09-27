use document_application::{VersionOperationId, VersioningRepository};
use document_repository_postgres::PostgresDocumentRepository;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[tokio::test]
async fn operation_lookup_is_the_replay_boundary() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unused")
        .unwrap();
    let repository = PostgresDocumentRepository::new(pool);
    let operation_id = VersionOperationId::try_from_uuid(
        Uuid::parse_str("01890f7a-6f6e-7b0a-8000-000000000001").unwrap(),
    )
    .unwrap();
    // The query is intentionally unexecuted during RED: the missing trait contract
    // must be the only compile failure. GREEN tests use a real PostgreSQL container.
    let _future = repository.get_version_operation(operation_id);
}
