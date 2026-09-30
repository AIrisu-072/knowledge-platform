use std::fs;

use document_application::DocumentRepository;
use document_domain::{DocumentId, LifecycleState};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

fn id(raw: u128) -> Uuid {
    Uuid::from_u128(raw)
}

async fn postgres() -> (testcontainers::ContainerAsync<GenericImage>, PgPool) {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "versioning_legacy_test")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/versioning_legacy_test"
        ))
        .await
        .unwrap();
    (container, pool)
}

async fn migration(pool: &PgPool, number: u8, name: &str) {
    let path = format!(
        "{}/migrations/{number:04}_{name}.sql",
        env!("CARGO_MANIFEST_DIR")
    );
    let sql = fs::read_to_string(path).expect("migration file must exist");
    // The path is assembled only from the fixed test migration names below.
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
        .execute(pool)
        .await
        .unwrap();
}

async fn legacy_document(pool: &PgPool, document_id: Uuid, version_id: Uuid, file_id: Uuid) {
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,0,'{}',to_timestamp(0))")
        .bind(document_id).bind(SYSTEM_ROOT_FOLDER_ID).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Legacy','test','actor','{}',to_timestamp(0))")
        .bind(version_id).bind(document_id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',1,$3,to_timestamp(0))")
        .bind(file_id).bind(vec![1_u8;32]).bind(format!("objects/{file_id}")).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO version_files (document_version_id,file_id,role,ordinal,original_filename) VALUES ($1,$2,'PRIMARY',0,'legacy.txt')")
        .bind(version_id).bind(file_id).execute(pool).await.unwrap();
}

#[tokio::test]
async fn versioning_migration_backfills_simple_primary_and_marks_ambiguous_legacy() {
    let (_container, pool) = postgres().await;
    migration(&pool, 1, "document_authoritative_core").await;
    migration(&pool, 2, "document_publish_v0").await;
    migration(&pool, 3, "document_semantic_inspection_v0").await;
    legacy_document(&pool, id(1), id(11), id(21)).await;
    legacy_document(&pool, id(2), id(12), id(22)).await;
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',1,'objects/attachment',to_timestamp(0))")
        .bind(id(23)).bind(vec![2_u8;32]).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO version_files (document_version_id,file_id,role,ordinal,original_filename) VALUES ($1,$2,'ATTACHMENT',1,'ambiguous.txt')")
        .bind(id(12)).bind(id(23)).execute(&pool).await.unwrap();

    let migration_sql = fs::read_to_string(format!(
        "{}/migrations/0004_document_versioning_v0.sql",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut dry_run = pool.begin().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration_sql.as_str()))
        .execute(&mut *dry_run)
        .await
        .unwrap();
    let dry_run_marked: bool = sqlx::query_scalar(
        "SELECT requires_content_classification FROM document_versions WHERE document_version_id = $1",
    )
    .bind(id(12))
    .fetch_one(&mut *dry_run)
    .await
    .unwrap();
    assert!(dry_run_marked);
    dry_run.rollback().await.unwrap();
    let migration_rolled_back: bool =
        sqlx::query_scalar("SELECT to_regclass('public.content_items') IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(migration_rolled_back);
    let retained_legacy: i64 =
        sqlx::query_scalar("SELECT count(*) FROM version_files WHERE document_version_id = $1")
            .bind(id(12))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(retained_legacy, 2);

    migration(&pool, 4, "document_versioning_v0").await;
    let simple: (String, i32) = sqlx::query_as(
        "SELECT logical_path,ordinal FROM content_items WHERE document_version_id = $1",
    )
    .bind(id(11))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(simple, ("primary".to_owned(), 0));
    let marked: bool = sqlx::query_scalar("SELECT requires_content_classification FROM document_versions WHERE document_version_id = $1")
        .bind(id(12)).fetch_one(&pool).await.unwrap();
    assert!(marked);
    let ambiguous_items: i64 =
        sqlx::query_scalar("SELECT count(*) FROM content_items WHERE document_version_id = $1")
            .bind(id(12))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ambiguous_items, 0);
    let retained: i64 =
        sqlx::query_scalar("SELECT count(*) FROM version_files WHERE document_version_id = $1")
            .bind(id(12))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(retained, 2);
    migration(&pool, 5, "document_publication_end_v0").await;
    migration(&pool, 6, "document_management_access_v0").await;
    let repository = PostgresDocumentRepository::new(pool.clone());
    let ambiguous = repository
        .get_authoritative_document(DocumentId::from_uuid(id(2)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ambiguous.version().lifecycle_state(),
        LifecycleState::Working
    );
    assert!(ambiguous.requires_content_classification());
    assert!(ambiguous.content_items().is_empty());
    assert_eq!(ambiguous.version_file().file_id().as_uuid(), id(22));

    sqlx::query("UPDATE document_versions SET lifecycle_state='PUBLISHED',published_at=to_timestamp(1) WHERE document_version_id=$1")
        .bind(id(11)).execute(&pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id=$1,revision=1 WHERE document_id=$2")
        .bind(id(11))
        .bind(id(1))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,base_document_version_id,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,2,$3,'PUBLISHED','Version Two',to_timestamp(2),'test','actor','{}',to_timestamp(2))")
        .bind(id(13)).bind(id(1)).bind(id(11)).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',1,'objects/version-two',to_timestamp(2))")
        .bind(id(24)).bind(vec![3_u8;32]).execute(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(id(31)).bind(id(13)).bind(id(41)).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,'AUTHORITATIVE','new.txt')")
        .bind(id(41)).bind(id(31)).bind(id(24)).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id=$1,revision=2 WHERE document_id=$2")
        .bind(id(13))
        .bind(id(1))
        .execute(&pool)
        .await
        .unwrap();
    let current = repository
        .get_authoritative_document(DocumentId::from_uuid(id(1)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.version().document_version_id().as_uuid(), id(13));
    assert_eq!(
        current
            .version()
            .base_document_version_id()
            .unwrap()
            .as_uuid(),
        id(11)
    );
    assert_eq!(current.file().file_id().as_uuid(), id(24));
    assert!(!current.requires_content_classification());
    assert_eq!(current.content_items().len(), 1);
}

#[tokio::test]
async fn document_revision_backfill_preserves_published_order_and_marks_unknown_metadata() {
    let (_container, pool) = postgres().await;
    for (number, name) in [
        (1, "document_authoritative_core"),
        (2, "document_publish_v0"),
        (3, "document_semantic_inspection_v0"),
        (4, "document_versioning_v0"),
        (5, "document_publication_end_v0"),
        (6, "document_management_access_v0"),
        (7, "document_folder_names_v0"),
        (8, "document_read_state_v0"),
    ] {
        migration(&pool, number, name).await;
    }

    let document_id = id(70);
    let working_document_id = id(71);
    let earlier_version_id = id(72);
    let later_withdrawn_version_id = id(73);
    let working_version_id = id(74);
    let current_metadata = serde_json::json!({
        "document_type": "policy",
        "owning_department": "legal",
        "category": "internal",
        "extensions": {"retention": "seven-years"}
    });
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,3,$3,to_timestamp(0))")
        .bind(document_id).bind(SYSTEM_ROOT_FOLDER_ID).bind(&current_metadata)
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,0,'{}',to_timestamp(0))")
        .bind(working_document_id).bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(&pool).await.unwrap();

    let earlier_published_at = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
    let later_published_at = OffsetDateTime::from_unix_timestamp(1_700_000_100).unwrap();
    for (version_id, owner, version_no, state, published_at, withdrawn_at) in [
        (
            earlier_version_id,
            document_id,
            1_i64,
            "PUBLISHED",
            Some(earlier_published_at),
            None,
        ),
        (
            later_withdrawn_version_id,
            document_id,
            2_i64,
            "WITHDRAWN",
            Some(later_published_at),
            Some(later_published_at),
        ),
        (
            working_version_id,
            working_document_id,
            1_i64,
            "WORKING",
            None,
            None,
        ),
    ] {
        sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,withdrawn_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,$3,$4,'Legacy', $5,$6,'legacy-idp','legacy-actor','{}',to_timestamp(0))")
            .bind(version_id).bind(owner).bind(version_no).bind(state).bind(published_at).bind(withdrawn_at)
            .execute(&pool).await.unwrap();
    }
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(earlier_version_id)
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();

    let migration_sql = fs::read_to_string(format!(
        "{}/migrations/0009_document_revisions_v0.sql",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut rolled_back = pool.begin().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration_sql.as_str()))
        .execute(&mut *rolled_back)
        .await
        .unwrap();
    let transactional_backfill: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_revisions WHERE document_id = $1")
            .bind(document_id)
            .fetch_one(&mut *rolled_back)
            .await
            .unwrap();
    assert_eq!(transactional_backfill, 2);
    rolled_back.rollback().await.unwrap();
    let rolled_back_table_exists: bool =
        sqlx::query_scalar("SELECT to_regclass('public.document_revisions') IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !rolled_back_table_exists,
        "migration DDL and backfill are rollbackable"
    );

    migration(&pool, 9, "document_revisions_v0").await;

    let rows: Vec<(i64, i64, Uuid, String, Option<Value>, String, Option<Uuid>)> =
        sqlx::query_as("SELECT major_no,minor_no,document_version_id,metadata_snapshot_status,metadata_snapshot,source_kind,operation_id FROM document_revisions WHERE document_id = $1 ORDER BY major_no,minor_no")
            .bind(document_id).fetch_all(&pool).await.unwrap();
    assert_eq!(
        rows.len(),
        2,
        "each previously published Version gets one legacy revision"
    );
    assert_eq!(
        rows[0],
        (
            1,
            0,
            earlier_version_id,
            "complete".into(),
            Some(current_metadata),
            "legacyBackfill".into(),
            None
        )
    );
    assert_eq!(
        rows[1],
        (
            2,
            0,
            later_withdrawn_version_id,
            "unavailable_legacy".into(),
            None,
            "legacyBackfill".into(),
            None
        )
    );

    let working_revisions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_revisions WHERE document_id = $1")
            .bind(working_document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        working_revisions, 0,
        "WORKING-only documents have no issued revisions"
    );

    migration(&pool, 9, "document_revisions_v0").await;
    let rerun_revisions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_revisions WHERE document_id = $1")
            .bind(document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        rerun_revisions, 2,
        "rerunning the backfill does not duplicate rows"
    );
}
