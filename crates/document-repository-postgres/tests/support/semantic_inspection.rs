#![allow(dead_code)]

use document_application::SemanticInspectionRecord;
use document_domain::FileId;
use document_semantic_inspection_core::WorkerResponse;
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

pub const RAW_HASH: [u8; 32] = [7; 32];
pub const RAW_SIZE: i64 = 13;

pub async fn postgres() -> (testcontainers::ContainerAsync<GenericImage>, PgPool) {
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
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/knowledge_platform_test");
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&url)
        .await
        .unwrap();
    (container, pool)
}

pub async fn seed_file(pool: &PgPool, id: Uuid) {
    sqlx::query(
        "INSERT INTO file_objects \
         (file_id, content_hash, media_type, size_bytes, storage_locator, created_at) \
         VALUES ($1, $2, 'text/plain', $3, $4, now())",
    )
    .bind(id)
    .bind(RAW_HASH.to_vec())
    .bind(RAW_SIZE)
    .bind(format!("objects/{id}/file"))
    .execute(pool)
    .await
    .unwrap();
}

pub fn response(fingerprint: u8) -> WorkerResponse {
    serde_json::from_value(serde_json::json!({
        "protocol_version": "dsi-worker-v0",
        "inspection_profile_version": "dsi-v0",
        "observed_raw_content_hash": RAW_HASH,
        "observed_size_bytes": RAW_SIZE,
        "detected_format": "txt",
        "semantic_fingerprint": {"algorithm": "sha256", "digest": vec![fingerprint; 32]},
        "semantic_capabilities": [{
            "capability_id": "visible_text", "presence": "present",
            "version_significant": true,
            "equivalence_fingerprint": {"algorithm": "sha256", "digest": vec![fingerprint; 32]}
        }],
        "editorial_provenance": {
            "tracked_changes": [], "comments": [], "document_author_labels": [],
            "last_modified_by": null, "modification_metadata": {}
        },
        "external_dependencies": [],
        "digital_signature_evidence": [],
        "extractor_provenance": {
            "worker_build_id": "task11-test", "adapter_id": "text", "adapter_version": "0",
            "parser_libraries": [], "native_dependency_identity": []
        },
        "diagnostics": []
    }))
    .unwrap()
}

pub fn record(id: Uuid, fingerprint: u8) -> SemanticInspectionRecord {
    SemanticInspectionRecord::restore(
        FileId::from_uuid(id),
        response(fingerprint),
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap()
}
