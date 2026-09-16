use document_repository_postgres::{SYSTEM_ROOT_FOLDER_ID, migrate};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

#[tokio::test]
async fn migration_seeds_root_and_enforces_authoritative_constraints() {
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
        .expect("postgres should accept connections after readiness signal");

    migrate(&pool).await.expect("migration should succeed");

    let root_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM folders WHERE folder_id = $1)")
            .bind(SYSTEM_ROOT_FOLDER_ID)
            .fetch_one(&pool)
            .await
            .expect("system root folder query should succeed");
    assert!(
        root_exists,
        "migration must seed the fixed system root folder"
    );

    assert_rejected(
        &pool,
        "negative document revision",
        sqlx::query(
            "INSERT INTO documents \
             (document_id, folder_id, current_version_id, revision, metadata, created_at) \
             VALUES ($1, $2, NULL, -1, '{}'::jsonb, now())",
        )
        .bind(id(1))
        .bind(SYSTEM_ROOT_FOLDER_ID),
    )
    .await;

    let document_id = id(10);
    insert_document(&pool, document_id).await;

    assert_rejected(
        &pool,
        "version number below one",
        version_insert(id(11), document_id, 0, "WORKING"),
    )
    .await;

    assert_rejected(
        &pool,
        "unknown lifecycle state",
        version_insert(id(12), document_id, 1, "DRAFT"),
    )
    .await;

    assert_rejected(
        &pool,
        "published version without published_at",
        version_insert(id(13), document_id, 1, "PUBLISHED"),
    )
    .await;

    assert_rejected(
        &pool,
        "withdrawn version without withdrawn_at",
        version_insert(id(14), document_id, 1, "WITHDRAWN"),
    )
    .await;

    let version_id = id(20);
    insert_working_version(&pool, version_id, document_id, 1).await;

    assert_rejected(
        &pool,
        "duplicate document version number",
        version_insert(id(21), document_id, 1, "WORKING"),
    )
    .await;

    assert_rejected(
        &pool,
        "non-SHA256 hash width",
        file_insert(id(30), vec![0_u8; 31], 1, "objects/hash-width"),
    )
    .await;

    assert_rejected(
        &pool,
        "negative file size",
        file_insert(id(31), vec![0_u8; 32], -1, "objects/negative-size"),
    )
    .await;

    let first_file_id = id(40);
    insert_file(&pool, first_file_id, "objects/unique-locator").await;

    assert_rejected(
        &pool,
        "duplicate storage locator",
        file_insert(id(41), vec![1_u8; 32], 1, "objects/unique-locator"),
    )
    .await;

    let second_file_id = id(42);
    insert_file(&pool, second_file_id, "objects/second-primary").await;
    insert_primary_file(&pool, version_id, first_file_id, "first.pdf").await;

    assert_rejected(
        &pool,
        "more than one primary file for a version",
        sqlx::query(
            "INSERT INTO version_files \
             (document_version_id, file_id, role, ordinal, original_filename) \
             VALUES ($1, $2, 'PRIMARY', 1, 'second.pdf')",
        )
        .bind(version_id)
        .bind(second_file_id),
    )
    .await;
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

fn version_insert(
    version_id: Uuid,
    document_id: Uuid,
    version_no: i64,
    lifecycle_state: &'static str,
) -> sqlx::query::Query<'static, sqlx::Postgres, sqlx::postgres::PgArguments> {
    sqlx::query(
        "INSERT INTO document_versions \
         (document_version_id, document_id, version_no, lifecycle_state, title, \
          created_by_identity_provider, created_by_principal_id, metadata, created_at) \
         VALUES ($1, $2, $3, $4, 'Schema test', 'test-idp', 'test-actor', '{}'::jsonb, now())",
    )
    .bind(version_id)
    .bind(document_id)
    .bind(version_no)
    .bind(lifecycle_state)
}

async fn insert_working_version(
    pool: &PgPool,
    version_id: Uuid,
    document_id: Uuid,
    version_no: i64,
) {
    version_insert(version_id, document_id, version_no, "WORKING")
        .execute(pool)
        .await
        .expect("valid working version should insert");
}

fn file_insert(
    file_id: Uuid,
    content_hash: Vec<u8>,
    size_bytes: i64,
    storage_locator: &'static str,
) -> sqlx::query::Query<'static, sqlx::Postgres, sqlx::postgres::PgArguments> {
    sqlx::query(
        "INSERT INTO file_objects \
         (file_id, content_hash, media_type, size_bytes, storage_locator, created_at) \
         VALUES ($1, $2, 'application/pdf', $3, $4, now())",
    )
    .bind(file_id)
    .bind(content_hash)
    .bind(size_bytes)
    .bind(storage_locator)
}

async fn insert_file(pool: &PgPool, file_id: Uuid, storage_locator: &'static str) {
    file_insert(file_id, vec![0_u8; 32], 1, storage_locator)
        .execute(pool)
        .await
        .expect("valid file object should insert");
}

async fn insert_primary_file(
    pool: &PgPool,
    version_id: Uuid,
    file_id: Uuid,
    original_filename: &'static str,
) {
    sqlx::query(
        "INSERT INTO version_files \
         (document_version_id, file_id, role, ordinal, original_filename) \
         VALUES ($1, $2, 'PRIMARY', 0, $3)",
    )
    .bind(version_id)
    .bind(file_id)
    .bind(original_filename)
    .execute(pool)
    .await
    .expect("first primary file should insert");
}

async fn assert_rejected<'q>(
    pool: &PgPool,
    case: &str,
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
) {
    let result = query.execute(pool).await;
    assert!(result.is_err(), "database unexpectedly accepted {case}");
}
