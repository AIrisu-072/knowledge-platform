use document_domain::{DocumentVersionId, FileId};
use document_repository_postgres::{SYSTEM_ROOT_FOLDER_ID, migrate};
use search_source_document::{
    DocumentSnapshotReader, PostgresDocumentSnapshotReader, SnapshotReadError,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

struct Fixture {
    _container: testcontainers::ContainerAsync<GenericImage>,
    pool: PgPool,
}

impl Fixture {
    async fn new() -> Self {
        let container = GenericImage::new("postgres", "18.6-bookworm")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", "postgres")
            .with_env_var("POSTGRES_DB", "body_snapshot_test")
            .start()
            .await
            .unwrap();
        let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .connect(&format!(
                "postgres://postgres:postgres@127.0.0.1:{port}/body_snapshot_test"
            ))
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        Self {
            _container: container,
            pool,
        }
    }

    async fn published_version(&self, current: bool) -> DocumentVersionId {
        let document_id = Uuid::now_v7();
        let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
        sqlx::query(
            "INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) \
             VALUES ($1,$2,NULL,1,'{}'::jsonb,now())",
        )
        .bind(document_id)
        .bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state, \
             title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) \
             VALUES ($1,$2,1,'PUBLISHED','Body snapshot fixture',now(),'test-idp','test-user','{}'::jsonb,now())",
        )
        .bind(version_id.as_uuid())
        .bind(document_id)
        .execute(&self.pool)
        .await
        .unwrap();
        if current {
            sqlx::query("UPDATE documents SET current_version_id=$1 WHERE document_id=$2")
                .bind(version_id.as_uuid())
                .bind(document_id)
                .execute(&self.pool)
                .await
                .unwrap();
        }
        version_id
    }

    async fn file(&self, hash_byte: u8, media_type: &str) -> FileId {
        let file_id = FileId::from_uuid(Uuid::now_v7());
        sqlx::query(
            "INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) \
             VALUES ($1,$2,$3,8,$4,now())",
        )
        .bind(file_id.as_uuid())
        .bind(vec![hash_byte; 32])
        .bind(media_type)
        .bind(format!("objects/{}", file_id.as_uuid()))
        .execute(&self.pool)
        .await
        .unwrap();
        file_id
    }

    async fn part(
        &self,
        version_id: DocumentVersionId,
        file_id: FileId,
        path: &str,
        ordinal: i32,
    ) -> (Uuid, Uuid) {
        let item_id = Uuid::now_v7();
        let representation_id = Uuid::now_v7();
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) \
             VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(item_id)
        .bind(version_id.as_uuid())
        .bind(path)
        .bind(ordinal)
        .bind(representation_id)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) \
             VALUES ($1,$2,$3,'AUTHORITATIVE','source.txt')",
        )
        .bind(representation_id)
        .bind(item_id)
        .bind(file_id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        (item_id, representation_id)
    }

    async fn read(
        &self,
        version_id: DocumentVersionId,
    ) -> Result<search_source_document::VersionSnapshotRecord, SnapshotReadError> {
        PostgresDocumentSnapshotReader::new(self.pool.clone())
            .load_document_version(version_id)
            .await
            .map(Option::unwrap)
    }
}

#[tokio::test]
async fn live_and_historical_snapshots_keep_every_part_even_when_file_object_is_shared() {
    let f = Fixture::new().await;
    let live = f.published_version(true).await;
    let historical = f.published_version(false).await;
    let shared_file = f.file(0x11, "Text/Plain; charset=UTF-8").await;
    let (later_id, later_rep) = f.part(live, shared_file, "body/z", 1).await;
    let (middle_id, middle_rep) = f.part(live, shared_file, "body/b", 0).await;
    let (first_id, first_rep) = f.part(live, shared_file, "body/a", 0).await;
    let historic_file = f.file(0x22, "application/pdf").await;
    let (historic_id, _) = f.part(historical, historic_file, "primary", 0).await;

    let reader = PostgresDocumentSnapshotReader::new(f.pool.clone());
    let snapshot = reader.enumerate_outbox_snapshot().await.unwrap();
    assert_eq!(snapshot.live.len(), 1);
    assert_eq!(snapshot.historical.len(), 1);
    assert_eq!(snapshot.live[0].snapshot.document_version_id, live);
    assert_eq!(
        snapshot.historical[0].snapshot.document_version_id,
        historical
    );
    let items = &snapshot.live[0].authoritative_items;
    assert_eq!(items.len(), 3);
    assert_eq!(
        (items[0].content_item_id, items[0].representation_id),
        (first_id, first_rep)
    );
    assert_eq!(
        (items[1].content_item_id, items[1].representation_id),
        (middle_id, middle_rep)
    );
    assert_eq!(
        (items[2].content_item_id, items[2].representation_id),
        (later_id, later_rep)
    );
    assert_eq!(items[0].file_id, shared_file);
    assert_eq!(items[1].file_id, shared_file);
    assert_eq!(items[2].file_id, shared_file);
    assert_eq!(items[0].part.source_native_part_id, first_id.to_string());
    assert_eq!(items[0].part.logical_path, "body/a");
    assert_eq!(items[0].part.ordinal, 0);
    assert_eq!(items[1].part.source_native_part_id, middle_id.to_string());
    assert_eq!(items[1].part.logical_path, "body/b");
    assert_eq!(items[1].part.ordinal, 0);
    assert_eq!(items[2].part.source_native_part_id, later_id.to_string());
    assert_eq!(items[2].part.ordinal, 1);
    assert_eq!(items[0].raw.sha256, [0x11; 32]);
    assert_eq!(items[0].raw.size_bytes, 8);
    assert_eq!(items[0].raw.media_type, "text/plain");
    assert_eq!(
        items[0].storage_key.as_str(),
        format!("objects/{}", shared_file.as_uuid())
    );
    assert_eq!(
        snapshot.historical[0].authoritative_items[0].content_item_id,
        historic_id
    );
    assert_ne!(
        snapshot.historical[0].authoritative_items[0].file_id,
        shared_file
    );
    assert_eq!(
        snapshot.live[0].snapshot.source_snapshot,
        snapshot.source_snapshot
    );
    assert_eq!(
        snapshot.historical[0].snapshot.source_snapshot,
        snapshot.source_snapshot
    );
    assert_eq!(reader.enumerate_live().await.unwrap().len(), 1);
    assert_eq!(reader.enumerate_historical().await.unwrap().len(), 1);
}

#[tokio::test]
async fn live_version_without_authoritative_parts_rejects_legacy_and_dsi_only_data() {
    let f = Fixture::new().await;
    let version = f.published_version(true).await;
    let file = f.file(0x33, "text/plain").await;
    sqlx::query(
        "INSERT INTO version_files (document_version_id,file_id,role,ordinal,original_filename) \
         VALUES ($1,$2,'PRIMARY',0,'legacy.txt')",
    )
    .bind(version.as_uuid())
    .bind(file.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO document_semantic_inspections \
         (file_id,inspection_profile_version,worker_protocol_version,observed_raw_content_hash,observed_size_bytes, \
          detected_format,fingerprint_algorithm,fingerprint_digest,semantic_capabilities,editorial_provenance, \
          external_dependencies,digital_signature_evidence,worker_build_id,adapter_id,adapter_version, \
          parser_libraries,native_dependency_identity,diagnostics,inspected_at) \
         VALUES ($1,'dsi-v0','dsi-worker-v0',$2,8,'txt','sha256',$3,'[]'::jsonb,'{}'::jsonb, \
                 '[]'::jsonb,'[]'::jsonb,'test-worker','test-adapter','1','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,now())",
    )
    .bind(file.as_uuid())
    .bind(vec![0x33_u8; 32])
    .bind(vec![0x33_u8; 32])
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(matches!(
        f.read(version).await,
        Err(SnapshotReadError::Integrity(_))
    ));
}

#[tokio::test]
async fn missing_authoritative_representation_and_file_object_fail_closed() {
    let f = Fixture::new().await;
    let version_a = f.published_version(true).await;
    let file_a = f.file(0x44, "text/plain").await;
    let (item_a, representation_a) = f.part(version_a, file_a, "primary", 0).await;
    let rendition = f.file(0x45, "text/html").await;
    sqlx::query(
        "INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) \
         VALUES ($1,$2,$3,'RENDITION','rendered.html')",
    )
    .bind(Uuid::now_v7())
    .bind(item_a)
    .bind(rendition.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content_representations WHERE content_representation_id=$1")
        .bind(representation_a)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        f.read(version_a).await,
        Err(SnapshotReadError::Integrity(_))
    ));

    let version_b = f.published_version(true).await;
    let file_b = f.file(0x55, "text/plain").await;
    f.part(version_b, file_b, "primary", 0).await;
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM file_objects WHERE file_id=$1")
        .bind(file_b.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        f.read(version_b).await,
        Err(SnapshotReadError::Integrity(_))
    ));
}

#[tokio::test]
async fn invalid_path_ordinal_hash_size_media_type_and_storage_key_fail_closed() {
    let f = Fixture::new().await;
    sqlx::query("ALTER TABLE content_items DROP CONSTRAINT content_items_ordinal_check")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE file_objects DROP CONSTRAINT file_objects_content_hash_check")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE file_objects DROP CONSTRAINT file_objects_size_bytes_check")
        .execute(&f.pool)
        .await
        .unwrap();
    for (path, ordinal, hash, size, mime, key) in [
        ("a/../b", 0, vec![0x66_u8; 32], 8, "text/plain", "objects/a"),
        (
            "primary",
            -1,
            vec![0x66_u8; 32],
            8,
            "text/plain",
            "objects/b",
        ),
        (
            "primary",
            0,
            vec![0x66_u8; 31],
            8,
            "text/plain",
            "objects/c",
        ),
        (
            "primary",
            0,
            vec![0x66_u8; 32],
            -1,
            "text/plain",
            "objects/d",
        ),
        (
            "primary",
            0,
            vec![0x66_u8; 32],
            8,
            "text/plain/extra",
            "objects/e",
        ),
        (
            "primary",
            0,
            vec![0x66_u8; 32],
            8,
            "text/plain",
            "../escape",
        ),
    ] {
        let version = f.published_version(true).await;
        let file = f.file(0x66, "text/plain").await;
        let (item, _) = f.part(version, file, "primary", 0).await;
        sqlx::query("UPDATE content_items SET logical_path=$1,ordinal=$2 WHERE content_item_id=$3")
            .bind(path)
            .bind(ordinal)
            .bind(item)
            .execute(&f.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE file_objects SET content_hash=$1,size_bytes=$2,media_type=$3,storage_locator=$4 WHERE file_id=$5")
            .bind(hash).bind(size).bind(mime).bind(key).bind(file.as_uuid())
            .execute(&f.pool).await.unwrap();
        assert!(
            matches!(f.read(version).await, Err(SnapshotReadError::Integrity(_))),
            "invalid binding accepted for {key}"
        );
    }
    let nil_version = f.published_version(true).await;
    sqlx::query(
        "INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) \
         VALUES ($1,$2,'text/plain',8,'objects/nil',now())",
    )
    .bind(Uuid::nil())
    .bind(vec![0x66_u8; 32])
    .execute(&f.pool)
    .await
    .unwrap();
    f.part(nil_version, FileId::from_uuid(Uuid::nil()), "primary", 0)
        .await;
    assert!(matches!(
        f.read(nil_version).await,
        Err(SnapshotReadError::Integrity(_))
    ));
}

#[tokio::test]
async fn database_rejects_duplicate_authority_and_manifest_keys_and_reader_guards_broken_constraints()
 {
    let f = Fixture::new().await;
    let version = f.published_version(true).await;
    let file = f.file(0x77, "text/plain").await;
    let (item, _) = f.part(version, file, "primary", 0).await;
    let second = Uuid::now_v7();
    let duplicate = sqlx::query(
        "INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) \
         VALUES ($1,$2,$3,'AUTHORITATIVE','duplicate.txt')",
    )
    .bind(second)
    .bind(item)
    .bind(file.as_uuid())
    .execute(&f.pool)
    .await;
    assert_eq!(
        duplicate
            .unwrap_err()
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("23505")
    );

    sqlx::query("DROP INDEX uq_content_representations_one_authoritative")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) \
         VALUES ($1,$2,$3,'AUTHORITATIVE','duplicate.txt')",
    )
    .bind(second).bind(item).bind(file.as_uuid()).execute(&f.pool).await.unwrap();
    assert!(matches!(
        f.read(version).await,
        Err(SnapshotReadError::Integrity(_))
    ));

    let other_version = f.published_version(true).await;
    let other_file = f.file(0xaa, "text/plain").await;
    f.part(other_version, other_file, "primary", 0).await;
    let (other_item, _) = f.part(other_version, other_file, "secondary", 0).await;
    let duplicate_key =
        sqlx::query("UPDATE content_items SET logical_path='primary' WHERE content_item_id=$1")
            .bind(other_item)
            .execute(&f.pool)
            .await;
    assert_eq!(
        duplicate_key
            .unwrap_err()
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("23505")
    );
    sqlx::query("ALTER TABLE content_items DROP CONSTRAINT uq_content_items_manifest_key")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE content_items SET logical_path='primary' WHERE content_item_id=$1")
        .bind(other_item)
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(matches!(
        f.read(other_version).await,
        Err(SnapshotReadError::Integrity(_))
    ));
}

#[tokio::test]
async fn rendition_is_not_used_as_authoritative_body() {
    let f = Fixture::new().await;
    let version = f.published_version(true).await;
    let source_file = f.file(0x88, "text/plain").await;
    let rendition_file = f.file(0x99, "text/html").await;
    let (item, _) = f.part(version, source_file, "primary", 0).await;
    sqlx::query(
        "INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) \
         VALUES ($1,$2,$3,'RENDITION','rendered.html')",
    )
    .bind(Uuid::now_v7()).bind(item).bind(rendition_file.as_uuid())
    .execute(&f.pool).await.unwrap();
    let record = f.read(version).await.unwrap();
    assert_eq!(record.authoritative_items.len(), 1);
    assert_eq!(record.authoritative_items[0].file_id, source_file);
    assert_eq!(record.authoritative_items[0].raw.sha256, [0x88; 32]);
}
