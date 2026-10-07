use document_repository_postgres::{SchemaCompatibilityError, check_schema_compatibility, migrate};
use serde_json::Value;
use sha2::{Digest, Sha256, Sha384};
use sqlx::{
    PgPool,
    migrate::{MigrateError, Migrator},
    postgres::PgPoolOptions,
};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const OUTBOX_SQL: &str = include_str!("../migrations/0011_outbox_delivery_v0.sql");

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn packaged_migrations_preserve_canonical_sql_and_unique_versions() {
    assert_eq!(
        MIGRATOR.iter().map(|m| m.version).collect::<Vec<_>>(),
        (1..=12).collect::<Vec<_>>()
    );
    for (sql, expected) in [
        (
            include_str!("../migrations/0009_document_revisions_v0.sql"),
            "cb0bb8030e012e3777a939935bf147f59c0a72359829e783a81f3201f7966255",
        ),
        (
            include_str!("../migrations/0010_document_version_updated_at.sql"),
            "9e1478d0d42cd7d3239d5cd067531d479c81fbd30af5a600a02f89170092cc76",
        ),
        (
            OUTBOX_SQL,
            "6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb",
        ),
    ] {
        assert_eq!(hex(&Sha256::digest(sql.as_bytes())), expected);
    }
    assert_eq!(
        hex(&Sha384::digest(OUTBOX_SQL.as_bytes())),
        "df0d246c1b3ab0c344a18c13936630a8d618fad2ae21649c9a13a16ac7d77cca75ca201b00f2b7acc956f93549b84783"
    );
}

async fn postgres() -> (testcontainers::ContainerAsync<GenericImage>, PgPool) {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "search_main_migration_test")
        .start()
        .await
        .expect("disposable PostgreSQL should start");
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/search_main_migration_test"
        ))
        .await
        .unwrap();
    (container, pool)
}

async fn apply_canonical_prefix(pool: &PgPool, version: i64) {
    Migrator::with_migrations(
        MIGRATOR
            .iter()
            .filter(|migration| migration.version <= version)
            .cloned()
            .collect(),
    )
    .run(pool)
    .await
    .unwrap();
}

async fn history(pool: &PgPool) -> Value {
    sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(m) ORDER BY version),'[]'::jsonb) \
         FROM _sqlx_migrations m",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_existing_rows(pool: &PgPool) {
    let document_id = Uuid::from_u128(100);
    let version_id = Uuid::from_u128(101);
    sqlx::query("INSERT INTO documents(document_id,folder_id,revision,metadata,created_at) VALUES($1,$2,1,'{\"document_type\":\"synthetic\",\"owning_department\":\"fixture\",\"category\":\"fixture\",\"extensions\":{}}',to_timestamp(10))")
        .bind(document_id)
        .bind(document_repository_postgres::SYSTEM_ROOT_FOLDER_ID)
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO document_versions(document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES($1,$2,1,'PUBLISHED','Synthetic retained document',to_timestamp(20),'test-idp','fixture','{}',to_timestamp(10))")
        .bind(version_id).bind(document_id).execute(pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id=$1 WHERE document_id=$2")
        .bind(version_id)
        .bind(document_id)
        .execute(pool)
        .await
        .unwrap();
    for (id, delivered, attempt_count) in [(1, false, 0), (2, true, 2), (3, false, 8)] {
        sqlx::query("INSERT INTO outbox_events(event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at,attempt_count,delivered_at) VALUES($1,'DocumentRegistered','Document',$2,'{\"keep\":[null,1]}',to_timestamp(20),to_timestamp(21),$3,CASE WHEN $4 THEN to_timestamp(22) ELSE NULL END)")
            .bind(Uuid::from_u128(id)).bind(document_id).bind(attempt_count).bind(delivered)
            .execute(pool).await.unwrap();
    }
    sqlx::query("INSERT INTO audit_outbox_events(event_id,event_type,source,subject,actor_identity_provider,actor_principal_id,resource_id,result,data,occurred_at,attempt_count,delivered_at) VALUES($1,'document.registered','fixture','document','test-idp','fixture',$2,'SUCCEEDED','{\"keep\":null}',to_timestamp(20),3,to_timestamp(22))")
        .bind(Uuid::from_u128(11)).bind(document_id).execute(pool).await.unwrap();
}

async fn existing_rows(pool: &PgPool) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'documents',(SELECT jsonb_agg(to_jsonb(d) ORDER BY document_id) FROM documents d), \
         'versions',(SELECT jsonb_agg(to_jsonb(v)-'updated_at' ORDER BY document_version_id) FROM document_versions v), \
         'outbox',(SELECT jsonb_agg(to_jsonb(o)-ARRAY['lease_token','lease_owner','lease_expires_at','last_attempt_at','dead_lettered_at','last_error_code','attempt_limit','traceparent','tracestate']::text[] ORDER BY event_id) FROM outbox_events o), \
         'audit',(SELECT jsonb_agg(to_jsonb(a) ORDER BY event_id) FROM audit_outbox_events a))",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_integrated(pool: &PgPool) {
    check_schema_compatibility(pool).await.unwrap();
    let applied: Vec<(i64, bool, Vec<u8>)> =
        sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .unwrap();
    let expected = MIGRATOR
        .iter()
        .map(|m| (m.version, true, m.checksum.to_vec()))
        .collect::<Vec<_>>();
    assert_eq!(applied, expected);
    let schema: (bool, bool, i64) = sqlx::query_as(
        "SELECT to_regclass('public.document_revisions') IS NOT NULL, \
         EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='public' \
         AND table_name='document_versions' AND column_name='updated_at'), \
         (SELECT count(*) FROM outbox_delivery_policy)",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(schema, (true, true, 1));
    let before = history(pool).await;
    migrate(pool).await.unwrap();
    assert_eq!(history(pool).await, before);
}

#[tokio::test]
async fn fresh_history_applies_all_twelve_migrations() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.unwrap();
    assert_integrated(&pool).await;
}

#[tokio::test]
async fn base_eight_history_preserves_data_while_applying_document_and_outbox() {
    let (_container, pool) = postgres().await;
    apply_canonical_prefix(&pool, 8).await;
    seed_existing_rows(&pool).await;
    let before = existing_rows(&pool).await;
    let prefix = history(&pool).await;
    migrate(&pool).await.unwrap();
    assert_eq!(existing_rows(&pool).await, before);
    assert_eq!(
        &history(&pool).await.as_array().unwrap()[..8],
        prefix.as_array().unwrap()
    );
    let backfill: (i64, String, bool) = sqlx::query_as("SELECT count(*),min(source_kind),bool_and(metadata_snapshot_status='complete') FROM document_revisions")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(backfill, (1, "legacyBackfill".into(), true));
    let timestamp_preserved: bool =
        sqlx::query_scalar("SELECT bool_and(updated_at=created_at) FROM document_versions")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(timestamp_preserved);
    assert_integrated(&pool).await;
}

#[tokio::test]
async fn document_ten_history_preserves_revisions_timestamps_and_ledger() {
    let (_container, pool) = postgres().await;
    apply_canonical_prefix(&pool, 8).await;
    seed_existing_rows(&pool).await;
    apply_canonical_prefix(&pool, 10).await;
    let before = existing_rows(&pool).await;
    let prefix = history(&pool).await;
    let document_state: Value = sqlx::query_scalar("SELECT jsonb_build_object('revisions',(SELECT jsonb_agg(to_jsonb(r) ORDER BY revision_id) FROM document_revisions r),'versions',(SELECT jsonb_agg(to_jsonb(v) ORDER BY document_version_id) FROM document_versions v))")
        .fetch_one(&pool).await.unwrap();
    migrate(&pool).await.unwrap();
    assert_eq!(existing_rows(&pool).await, before);
    assert_eq!(
        &history(&pool).await.as_array().unwrap()[..10],
        prefix.as_array().unwrap()
    );
    let after: Value = sqlx::query_scalar("SELECT jsonb_build_object('revisions',(SELECT jsonb_agg(to_jsonb(r) ORDER BY revision_id) FROM document_revisions r),'versions',(SELECT jsonb_agg(to_jsonb(v) ORDER BY document_version_id) FROM document_versions v))")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(after, document_state);
    assert_integrated(&pool).await;
}

#[tokio::test]
async fn old_search_nine_history_stops_without_applying_document_or_rewriting_ledger() {
    let (_container, pool) = postgres().await;
    let mut predecessor = MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 8)
        .cloned()
        .collect::<Vec<_>>();
    let mut old_outbox = MIGRATOR.iter().find(|m| m.version == 11).unwrap().clone();
    // Build the real predecessor history through SQLx. Never rewrite its ledger.
    old_outbox.version = 9;
    predecessor.push(old_outbox);
    Migrator::with_migrations(predecessor)
        .run(&pool)
        .await
        .unwrap();
    seed_existing_rows(&pool).await;
    let before = existing_rows(&pool).await;
    let old_history = history(&pool).await;
    assert_eq!(
        check_schema_compatibility(&pool).await,
        Err(SchemaCompatibilityError::ChecksumMismatch)
    );
    let error = migrate(&pool).await.unwrap_err();
    assert!(matches!(error, MigrateError::VersionMismatch(9)), "{error}");
    assert_eq!(history(&pool).await, old_history);
    assert_eq!(existing_rows(&pool).await, before);
    let document_schema_absent: (bool, bool) = sqlx::query_as(
        "SELECT to_regclass('public.document_revisions') IS NULL, \
         NOT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='public' \
         AND table_name='document_versions' AND column_name='updated_at')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(document_schema_absent, (true, true));
}
