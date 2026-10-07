#[path = "support/management.rs"]
mod support;

use document_repository_postgres::SYSTEM_ROOT_FOLDER_ID;
use serde_json::Value;
use sqlx::{Row, migrate::Migrator, postgres::PgPoolOptions};
use time::OffsetDateTime;
use uuid::Uuid;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[tokio::test]
async fn legacy_upgrade_preserves_timestamp_audit_checksums_and_insert_defaults() {
    // Reuse the existing management fixture and its pinned official image.
    let fixture = support::fixture().await;
    sqlx::query("CREATE DATABASE current_read_upgrade")
        .execute(&fixture.pool)
        .await
        .unwrap();
    let upgrade = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(
            fixture
                .pool
                .connect_options()
                .as_ref()
                .clone()
                .database("current_read_upgrade"),
        )
        .await
        .unwrap();
    let mut connection = upgrade.acquire().await.unwrap();
    Migrator::with_migrations(
        MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 11)
            .cloned()
            .collect(),
    )
    .run(&mut *connection)
    .await
    .unwrap();
    let document_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let first_read_at = OffsetDateTime::UNIX_EPOCH;
    sqlx::query("INSERT INTO documents(document_id,folder_id,revision,metadata,created_at) VALUES($1,$2,1,'{}',$3)")
        .bind(document_id).bind(SYSTEM_ROOT_FOLDER_ID).bind(first_read_at).execute(&mut *connection).await.unwrap();
    sqlx::query("INSERT INTO document_versions(document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES($1,$2,1,'PUBLISHED','Synthetic',now(),'test-idp','legacy','{}',$3)")
        .bind(version_id).bind(document_id).bind(first_read_at).execute(&mut *connection).await.unwrap();
    sqlx::query("INSERT INTO document_read_states(identity_provider,principal_id,document_version_id,first_read_at) VALUES('test-idp','legacy',$1,$2)")
        .bind(version_id).bind(first_read_at).execute(&mut *connection).await.unwrap();
    sqlx::query("INSERT INTO audit_outbox_events(event_id,event_type,source,subject,actor_identity_provider,actor_principal_id,resource_type,resource_id,resource_version_id,result,data,occurred_at) VALUES($1,'document.version.read_confirmed','fixture','document','test-idp','legacy','Document',$2,$3,'success','{}',$4)")
        .bind(Uuid::now_v7()).bind(document_id).bind(version_id).bind(first_read_at).execute(&mut *connection).await.unwrap();
    let before: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(event) ORDER BY event_id) FROM audit_outbox_events event",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    MIGRATOR.run(&mut *connection).await.unwrap();
    let row = sqlx::query("SELECT first_read_at,needs_recheck,read_state_revision FROM document_read_states WHERE principal_id='legacy'")
        .fetch_one(&mut *connection).await.unwrap();
    assert_eq!(row.get::<OffsetDateTime, _>("first_read_at"), first_read_at);
    assert!(!row.get::<bool, _>("needs_recheck"));
    assert_eq!(row.get::<i64, _>("read_state_revision"), 1);
    let after: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(event) ORDER BY event_id) FROM audit_outbox_events event",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(before, after);
    sqlx::query("INSERT INTO document_read_states(identity_provider,principal_id,document_version_id,first_read_at) VALUES('test-idp','old-insert',$1,$2)")
        .bind(version_id).bind(first_read_at).execute(&mut *connection).await.unwrap();
    sqlx::query("INSERT INTO document_read_states(identity_provider,principal_id,document_version_id,first_read_at,needs_recheck,read_state_revision) VALUES('test-idp','new-insert',$1,$2,DEFAULT,DEFAULT)")
        .bind(version_id).bind(first_read_at).execute(&mut *connection).await.unwrap();
    let defaults: Vec<(i64, bool)> = sqlx::query_as(
        "SELECT read_state_revision,needs_recheck FROM document_read_states ORDER BY principal_id",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    assert_eq!(defaults, vec![(1, false); 3]);
    let old_checksums: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT version,checksum FROM _sqlx_migrations WHERE version <= 11 ORDER BY version",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    assert_eq!(
        old_checksums,
        MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 11)
            .map(|migration| (migration.version, migration.checksum.to_vec()))
            .collect::<Vec<_>>()
    );
    let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM document_read_state_operations")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(receipts, 0);
}
