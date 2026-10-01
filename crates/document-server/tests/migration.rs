#[path = "support/database.rs"]
mod database;

use database::TestDatabase;
use document_repository_postgres::{SchemaCompatibilityError, check_schema_compatibility, migrate};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

async fn migration_rows(pool: &PgPool) -> Vec<(i64, bool, Vec<u8>)> {
    sqlx::query_as("SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn compatibility_check_on_fresh_database_does_not_create_any_schema() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    assert_eq!(
        check_schema_compatibility(&pool).await,
        Err(SchemaCompatibilityError::NotInitialized)
    );
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tables, 0, "compatibility checks must never run migration");
    database.close().await;
}

#[tokio::test]
async fn explicit_migration_is_compatible_and_repeated_migration_preserves_state() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    check_schema_compatibility(&pool).await.unwrap();
    let history = migration_rows(&pool).await;
    let root: (Uuid, i64) = sqlx::query_as("SELECT folder_id, revision FROM folders")
        .fetch_one(&pool)
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    check_schema_compatibility(&pool).await.unwrap();
    assert_eq!(migration_rows(&pool).await, history);
    let root_after: (Uuid, i64) = sqlx::query_as("SELECT folder_id, revision FROM folders")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(root_after, root);
    database.close().await;
}

#[tokio::test]
async fn compatibility_rejects_missing_migration_without_repairing_history() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 1")
        .execute(&pool)
        .await
        .unwrap();
    let before = migration_rows(&pool).await;
    assert_eq!(
        check_schema_compatibility(&pool).await,
        Err(SchemaCompatibilityError::MissingMigration)
    );
    assert_eq!(migration_rows(&pool).await, before);
    database.close().await;
}

#[tokio::test]
async fn compatibility_rejects_newer_migration_without_downgrading_history() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    sqlx::query("INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (999999, 'unknown future migration', true, $1, 0)")
        .bind(vec![1_u8; 48])
        .execute(&pool)
        .await
        .unwrap();
    let before = migration_rows(&pool).await;
    assert_eq!(
        check_schema_compatibility(&pool).await,
        Err(SchemaCompatibilityError::UnexpectedMigration)
    );
    assert_eq!(migration_rows(&pool).await, before);
    database.close().await;
}

#[tokio::test]
async fn compatibility_rejects_changed_checksum_without_repairing_history() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum = $1 WHERE version = 1")
        .bind(vec![0_u8; 48])
        .execute(&pool)
        .await
        .unwrap();
    let before = migration_rows(&pool).await;
    assert_eq!(
        check_schema_compatibility(&pool).await,
        Err(SchemaCompatibilityError::ChecksumMismatch)
    );
    assert_eq!(migration_rows(&pool).await, before);
    database.close().await;
}

#[tokio::test]
async fn compatibility_rejects_failed_migration_without_repairing_history() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET success = false WHERE version = 1")
        .execute(&pool)
        .await
        .unwrap();
    let before = migration_rows(&pool).await;
    assert_eq!(
        check_schema_compatibility(&pool).await,
        Err(SchemaCompatibilityError::FailedMigration)
    );
    assert_eq!(migration_rows(&pool).await, before);
    database.close().await;
}

#[tokio::test]
async fn compatibility_error_redacts_unavailable_database_details() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://synthetic-secret:synthetic-secret@127.0.0.1/synthetic-secret")
        .unwrap();
    pool.close().await;
    let error = check_schema_compatibility(&pool).await.unwrap_err();
    assert_eq!(error, SchemaCompatibilityError::Unavailable);
    for rendered in [error.to_string(), format!("{error:?}")] {
        assert!(!rendered.contains("synthetic-secret"));
        assert!(!rendered.contains("postgres://"));
        assert!(!rendered.contains("127.0.0.1"));
    }
}

#[tokio::test]
async fn compatibility_check_succeeds_in_a_read_only_database_session() {
    let database = TestDatabase::new().await;
    migrate(&database.pool).await.unwrap();
    let readonly = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET default_transaction_read_only = on")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(database.pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    check_schema_compatibility(&readonly).await.unwrap();
    readonly.close().await;
    database.close().await;
}

#[tokio::test]
async fn compatibility_rejects_a_substituted_migration_even_when_row_count_matches() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET version = 999999 WHERE version = 1")
        .execute(&pool)
        .await
        .unwrap();
    let before = migration_rows(&pool).await;
    assert_eq!(
        check_schema_compatibility(&pool).await,
        Err(SchemaCompatibilityError::UnexpectedMigration)
    );
    assert_eq!(migration_rows(&pool).await, before);
    database.close().await;
}
