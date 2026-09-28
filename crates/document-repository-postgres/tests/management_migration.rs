use document_repository_postgres::SYSTEM_ROOT_FOLDER_ID;
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

const M1: &str = include_str!("../migrations/0001_document_authoritative_core.sql");
const M2: &str = include_str!("../migrations/0002_document_publish_v0.sql");
const M3: &str = include_str!("../migrations/0003_document_semantic_inspection_v0.sql");
const M4: &str = include_str!("../migrations/0004_document_versioning_v0.sql");
const M5: &str = include_str!("../migrations/0005_document_publication_end_v0.sql");
const M6: &str = include_str!("../migrations/0006_document_management_access_v0.sql");
const M7: &str = include_str!("../migrations/0007_document_folder_names_v0.sql");
const M8: &str = include_str!("../migrations/0008_document_read_state_v0.sql");

async fn apply(pool: &PgPool, migration: &'static str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration))
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn legacy_snapshot_upgrades_atomically_and_can_be_restored_in_isolation() {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "pre_migration")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let url = |database: &str| format!("postgres://postgres:postgres@127.0.0.1:{port}/{database}");
    let original = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url("pre_migration"))
        .await
        .unwrap();
    for migration in [M1, M2, M3, M4, M5] {
        apply(&original, migration).await;
    }
    let document_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let schedule_id = Uuid::now_v7();
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,1,'{}',now())")
        .bind(document_id).bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(&original).await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Legacy',now(),'test-idp','legacy-user','{}',now())")
        .bind(version_id).bind(document_id).execute(&original).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id)
        .bind(document_id)
        .execute(&original)
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,terminal_reason,created_at) VALUES ($1,$2,$3,1,2,to_timestamp(30),'test-idp','legacy-user',$4,'TERMINAL','legacy_reason',to_timestamp(15))")
        .bind(schedule_id).bind(document_id).bind(version_id).bind(vec![1_u8;32])
        .execute(&original).await.unwrap();
    original.close().await;

    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url("postgres"))
        .await
        .unwrap();
    sqlx::query("CREATE DATABASE upgrade_trial TEMPLATE pre_migration")
        .execute(&admin)
        .await
        .unwrap();
    let trial = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url("upgrade_trial"))
        .await
        .unwrap();

    // Failed M7 rolls back M6 and the intentionally invalid old Folder together.
    let mut failed = trial.begin().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(M6))
        .execute(&mut *failed)
        .await
        .unwrap();
    sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$2,'bad/name','ACTIVE',0,now())")
        .bind(Uuid::now_v7()).bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(&mut *failed).await.unwrap();
    assert!(
        sqlx::raw_sql(sqlx::AssertSqlSafe(M7))
            .execute(&mut *failed)
            .await
            .is_err()
    );
    failed.rollback().await.unwrap();
    let preflight_state: (bool, i64) = sqlx::query_as(
        "SELECT to_regclass('public.document_access_state') IS NULL, (SELECT count(*) FROM documents)",
    )
    .fetch_one(&trial)
    .await
    .unwrap();
    assert_eq!(preflight_state, (true, 1));

    for migration in [M6, M7, M8] {
        apply(&trial, migration).await;
    }
    let upgraded: (Uuid, i64, bool, bool) = sqlx::query_as(
        "SELECT d.current_version_id,d.revision, \
         to_regclass('public.document_read_states') IS NOT NULL, \
         to_regclass('public.access_policy_bindings') IS NOT NULL \
         FROM documents d WHERE d.document_id = $1",
    )
    .bind(document_id)
    .fetch_one(&trial)
    .await
    .unwrap();
    assert_eq!(upgraded, (version_id, 1, true, true));
    let old_terminal_is_unknown: Option<time::OffsetDateTime> = sqlx::query_scalar(
        "SELECT terminal_at FROM document_publish_schedules WHERE publish_operation_id = $1",
    )
    .bind(schedule_id)
    .fetch_one(&trial)
    .await
    .unwrap();
    assert_eq!(old_terminal_is_unknown, None);

    // Restore the saved 0005 database snapshot into the trial name.
    trial.close().await;
    sqlx::query("DROP DATABASE upgrade_trial WITH (FORCE)")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("CREATE DATABASE upgrade_trial TEMPLATE pre_migration")
        .execute(&admin)
        .await
        .unwrap();
    let restored = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url("upgrade_trial"))
        .await
        .unwrap();
    let restored_state: (Uuid, i64, bool, bool) = sqlx::query_as(
        "SELECT d.current_version_id,d.revision, \
         to_regclass('public.document_read_states') IS NULL, \
         to_regclass('public.access_policy_bindings') IS NULL \
         FROM documents d WHERE d.document_id = $1",
    )
    .bind(document_id)
    .fetch_one(&restored)
    .await
    .unwrap();
    assert_eq!(restored_state, (version_id, 1, true, true));
    let restored_schedule: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_publish_schedules WHERE publish_operation_id = $1 AND status = 'TERMINAL'",
    )
    .bind(schedule_id)
    .fetch_one(&restored)
    .await
    .unwrap();
    assert_eq!(restored_schedule, 1);
}
