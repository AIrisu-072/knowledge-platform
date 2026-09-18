use document_repository_postgres::{SYSTEM_ROOT_FOLDER_ID, migrate};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

#[tokio::test]
async fn publish_migration_enforces_current_version_and_operation_constraints() {
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
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("postgres should accept connections");

    migrate(&pool).await.expect("migrations should succeed");

    let document_a = id(10);
    let document_b = id(20);
    let version_a = id(11);
    let version_b = id(21);
    insert_document(&pool, document_a).await;
    insert_document(&pool, document_b).await;
    insert_working_version(&pool, version_a, document_a).await;
    insert_working_version(&pool, version_b, document_b).await;

    let cross_document_current = sqlx::query(
        "UPDATE documents SET current_version_id = $1 WHERE document_id = $2",
    )
    .bind(version_b)
    .bind(document_a)
    .execute(&pool)
    .await;
    assert!(
        cross_document_current.is_err(),
        "database must reject a current version owned by another document"
    );

    assert_operation_rejected(
        &pool,
        id(100),
        document_a,
        version_a,
        -1,
        0,
        "negative expected revision",
    )
    .await;

    assert_operation_rejected(
        &pool,
        id(101),
        document_a,
        version_a,
        0,
        2,
        "resulting revision must equal expected plus one",
    )
    .await;

    assert_operation_rejected(
        &pool,
        id(102),
        document_a,
        version_b,
        0,
        1,
        "publish target must belong to the same document",
    )
    .await;

    insert_publish_operation(&pool, id(103), document_a, version_a, 0, 1)
        .await
        .expect("valid publish operation should insert");
    let duplicate = insert_publish_operation(&pool, id(103), document_a, version_a, 0, 1).await;
    assert!(duplicate.is_err(), "duplicate publish operation id must fail");
}

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

async fn insert_document(pool: &PgPool, document_id: Uuid) {
    sqlx::query(
        "INSERT INTO documents \
         (document_id, folder_id, current_version_id, revision, metadata, created_at) \
         VALUES ($1, $2, NULL, 0, '{}'::jsonb, now())",
    )
    .bind(document_id)
    .bind(SYSTEM_ROOT_FOLDER_ID)
    .execute(pool)
    .await
    .expect("valid document should insert");
}

async fn insert_working_version(pool: &PgPool, version_id: Uuid, document_id: Uuid) {
    sqlx::query(
        "INSERT INTO document_versions \
         (document_version_id, document_id, version_no, lifecycle_state, title, \
          created_by_identity_provider, created_by_principal_id, metadata, created_at) \
         VALUES ($1, $2, 1, 'WORKING', 'Publish schema test', \
                 'test-idp', 'actor-1', '{}'::jsonb, now())",
    )
    .bind(version_id)
    .bind(document_id)
    .execute(pool)
    .await
    .expect("valid working version should insert");
}

async fn assert_operation_rejected(
    pool: &PgPool,
    operation_id: Uuid,
    document_id: Uuid,
    version_id: Uuid,
    expected_revision: i64,
    resulting_revision: i64,
    case: &str,
) {
    let result = insert_publish_operation(
        pool,
        operation_id,
        document_id,
        version_id,
        expected_revision,
        resulting_revision,
    )
    .await;
    assert!(result.is_err(), "database unexpectedly accepted {case}");
}

async fn insert_publish_operation(
    pool: &PgPool,
    operation_id: Uuid,
    document_id: Uuid,
    version_id: Uuid,
    expected_revision: i64,
    resulting_revision: i64,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO document_publish_operations \
         (publish_operation_id, document_id, target_document_version_id, \
          expected_document_revision, actor_identity_provider, actor_principal_id, \
          published_at, resulting_document_revision, created_at) \
         VALUES ($1, $2, $3, $4, 'test-idp', 'actor-1', now(), $5, now())",
    )
    .bind(operation_id)
    .bind(document_id)
    .bind(version_id)
    .bind(expected_revision)
    .bind(resulting_revision)
    .execute(pool)
    .await
}
