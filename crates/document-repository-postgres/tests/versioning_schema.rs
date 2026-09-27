use document_repository_postgres::{SYSTEM_ROOT_FOLDER_ID, migrate};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
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
        .with_env_var("POSTGRES_DB", "versioning_schema_test")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/versioning_schema_test"
        ))
        .await
        .unwrap();
    (container, pool)
}

async fn document(pool: &PgPool, document_id: Uuid) {
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,0,'{}',to_timestamp(0))")
        .bind(document_id)
        .bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(pool)
        .await
        .unwrap();
}

async fn version(pool: &PgPool, version_id: Uuid, document_id: Uuid, number: i64, state: &str) {
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,$3,$4,'Versioning test',CASE WHEN $4 = 'PUBLISHED' THEN to_timestamp(0) ELSE NULL END,'test','actor','{}',to_timestamp(0))")
        .bind(version_id)
        .bind(document_id)
        .bind(number)
        .bind(state)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn versioning_schema_enforces_lineage_working_and_authoritative_representation() {
    let (_container, pool) = postgres().await;
    migrate(&pool).await.unwrap();
    let canonical_table: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.content_items')::text")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(canonical_table.as_deref(), Some("content_items"));

    document(&pool, id(1)).await;
    document(&pool, id(2)).await;
    version(&pool, id(11), id(1), 1, "PUBLISHED").await;
    version(&pool, id(21), id(2), 1, "WORKING").await;

    assert!(
        sqlx::query("UPDATE document_versions SET base_document_version_id = $1 WHERE document_version_id = $2")
            .bind(id(11))
            .bind(id(21))
            .execute(&pool)
            .await
            .is_err(),
        "a Version may only cite a base from its own Document"
    );
    assert!(
        sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,2,'WORKING','second','test','actor','{}',to_timestamp(0))")
            .bind(id(22))
            .bind(id(2))
            .execute(&pool)
            .await
            .is_err(),
        "a Document may have only one WORKING Version"
    );

    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',1,'objects/versioning-schema',to_timestamp(0))")
        .bind(id(31))
        .bind(vec![1_u8; 32])
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(id(41)).bind(id(21)).bind(id(51)).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,'AUTHORITATIVE','test.txt')")
        .bind(id(51)).bind(id(41)).bind(id(31)).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,'AUTHORITATIVE','duplicate.txt')")
            .bind(id(52)).bind(id(41)).bind(id(31)).execute(&pool).await.is_err(),
        "one ContentItem cannot have two authoritative representations"
    );
}
