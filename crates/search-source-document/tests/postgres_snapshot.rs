use std::sync::Arc;

use document_application::{DocumentAccessCheckService, InvocationKind, VerifiedActorContext};
use document_domain::{
    DocumentId, DocumentVersionId, PolicySubject, PolicySubjectKind, PrincipalRef,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use search_application::ports::{AccessDecision, CurrentAccessEvaluatorPort};
use search_core::id::ResourceId;
use search_source_document::{
    DocumentCurrentAccessAdapter, DocumentSnapshotReader, DsiReadState,
    PostgresDocumentSnapshotReader,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::{Duration, OffsetDateTime};
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
            .with_env_var("POSTGRES_DB", "search_snapshot_test")
            .start()
            .await
            .unwrap();
        let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .connect(&format!(
                "postgres://postgres:postgres@127.0.0.1:{port}/search_snapshot_test"
            ))
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        Self {
            _container: container,
            pool,
        }
    }

    async fn published_document(&self, current: bool) -> (DocumentId, DocumentVersionId) {
        let document_id = DocumentId::from_uuid(Uuid::now_v7());
        let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
        sqlx::query(
            "INSERT INTO documents (document_id, folder_id, current_version_id, revision, metadata, created_at) \
             VALUES ($1, $2, NULL, 1, '{\"document_type\":\"policy\",\"category\":\"test\",\"private\":\"DO_NOT_INDEX\"}'::jsonb, now())",
        )
        .bind(document_id.as_uuid())
        .bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO document_versions (document_version_id, document_id, version_no, lifecycle_state, \
             title, published_at, created_by_identity_provider, created_by_principal_id, metadata, created_at) \
             VALUES ($1, $2, 1, 'PUBLISHED', 'Search source title', now(), 'test-idp', 'test-user', '{}'::jsonb, now())",
        )
        .bind(version_id.as_uuid())
        .bind(document_id.as_uuid())
        .execute(&self.pool)
        .await
        .unwrap();
        if current {
            sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
                .bind(version_id.as_uuid())
                .bind(document_id.as_uuid())
                .execute(&self.pool)
                .await
                .unwrap();
        }
        (document_id, version_id)
    }

    async fn end_publication(&self, document_id: DocumentId, version_id: DocumentVersionId) {
        sqlx::query(
            "UPDATE documents SET current_version_id = NULL, revision = 2 WHERE document_id = $1",
        )
        .bind(document_id.as_uuid())
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO document_publication_end_operations \
             (operation_id, document_id, command_digest, expected_document_revision, expected_current_version_id, \
              actor_identity_provider, actor_principal_id, reason, former_current_version_id, \
              resulting_document_revision, ended_at) \
             VALUES ($1, $2, $3, 1, $4, 'test-idp', 'test-user', 'test end', $4, 2, now())",
        )
        .bind(Uuid::now_v7())
        .bind(document_id.as_uuid())
        .bind(vec![1_u8; 32])
        .bind(version_id.as_uuid())
        .execute(&self.pool)
        .await
        .unwrap();
    }

    async fn authoritative_dsi(&self, version_id: DocumentVersionId) -> Uuid {
        let file_id = Uuid::now_v7();
        let item_id = Uuid::now_v7();
        let representation_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO file_objects (file_id, content_hash, media_type, size_bytes, storage_locator, created_at) \
             VALUES ($1, $2, 'text/plain', 8, $3, now())",
        )
        .bind(file_id)
        .bind(vec![7_u8; 32])
        .bind(format!("test/{file_id}"))
        .execute(&self.pool)
        .await
        .unwrap();
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO content_items (content_item_id, document_version_id, logical_path, ordinal, authoritative_representation_id) \
             VALUES ($1, $2, 'primary', 0, $3)",
        )
        .bind(item_id)
        .bind(version_id.as_uuid())
        .bind(representation_id)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO content_representations (content_representation_id, content_item_id, file_id, role, original_filename, \
             detected_format, inspection_profile_version, semantic_fingerprint) \
             VALUES ($1, $2, $3, 'AUTHORITATIVE', 'source.txt', 'txt', 'dsi-v0', $4)",
        )
        .bind(representation_id)
        .bind(item_id)
        .bind(file_id)
        .bind(vec![3_u8; 32])
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        sqlx::query(
            "INSERT INTO document_semantic_inspections \
             (file_id, inspection_profile_version, worker_protocol_version, observed_raw_content_hash, observed_size_bytes, \
              detected_format, fingerprint_algorithm, fingerprint_digest, semantic_capabilities, editorial_provenance, \
              external_dependencies, digital_signature_evidence, worker_build_id, adapter_id, adapter_version, \
              parser_libraries, native_dependency_identity, diagnostics, inspected_at) \
             VALUES ($1, 'dsi-v0', 'dsi-worker-v0', $2, 8, 'txt', 'sha256', $3, \
              '[{\"capability_id\":\"visible_text\",\"presence\":\"present\",\"version_significant\":true,\"equivalence_fingerprint\":null}]'::jsonb, \
              '{}'::jsonb, '[]'::jsonb, '[]'::jsonb, 'test-worker', 'test-adapter', '1', \
              '[]'::jsonb, '[]'::jsonb, '[]'::jsonb, now())",
        )
        .bind(file_id)
        .bind(vec![7_u8; 32])
        .bind(vec![3_u8; 32])
        .execute(&self.pool)
        .await
        .unwrap();
        file_id
    }

    async fn grant_read(&self) -> Uuid {
        let policy_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO access_policy_bindings (policy_id, folder_id, mode, revision, created_at, updated_at) \
             VALUES ($1, $2, 'EXPLICIT', 1, now(), now())",
        )
        .bind(policy_id)
        .bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO access_policy_grants (policy_id, subject_kind, identity_provider, subject_id, action) \
             VALUES ($1, 'principal', 'test-idp', 'test-user', 'read')",
        )
        .bind(policy_id)
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query("UPDATE document_access_state SET access_revision = 1 WHERE id = 1")
            .execute(&self.pool)
            .await
            .unwrap();
        policy_id
    }
}

fn actor() -> VerifiedActorContext {
    let principal = PrincipalRef::new("test-idp", "test-user").unwrap();
    let subject =
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "test-user").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![subject],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

#[tokio::test]
async fn snapshot_reads_only_authoritative_dsi_and_marks_missing_or_invalid_unknown() {
    let f = Fixture::new().await;
    let (document_id, version_id) = f.published_document(true).await;
    f.grant_read().await;
    let file_id = f.authoritative_dsi(version_id).await;
    let reader = PostgresDocumentSnapshotReader::new(f.pool.clone());

    let record = reader
        .load_document_version(version_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.snapshot.document_id, document_id);
    assert_eq!(record.snapshot.current_version_id, Some(version_id));
    assert_eq!(record.access_revision, 1);
    assert_eq!(record.dsi_state, DsiReadState::Verified);
    assert_eq!(
        record.snapshot.metadata.document_type.as_deref(),
        Some("policy")
    );
    assert_eq!(record.snapshot.metadata.category.as_deref(), Some("test"));
    assert_eq!(
        record.snapshot.dsi.unwrap().evidence_refs,
        vec![format!("dsi:{file_id}:dsi-v0")]
    );
    let access_hint = record.snapshot.access.access_scope.unwrap();
    assert!(access_hint.contains(&document_id.as_uuid().to_string()));
    assert!(access_hint.contains("access-revision:1"));
    assert!(!access_hint.contains("test-user"));
    assert!(!access_hint.contains("read"));
    assert!(!record.snapshot.source_snapshot.is_empty());

    sqlx::query(
        "UPDATE document_semantic_inspections SET semantic_capabilities = \
         '[{\"capability_id\":\"visible_text\",\"presence\":\"present\"}]'::jsonb WHERE file_id = $1",
    )
    .bind(file_id)
    .execute(&f.pool)
    .await
    .unwrap();
    let malformed = reader
        .load_document_version(version_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(malformed.dsi_state, DsiReadState::UnknownInvalid);
    assert!(malformed.snapshot.dsi.is_none());
    sqlx::query(
        "UPDATE document_semantic_inspections SET semantic_capabilities = \
         '[{\"capability_id\":\"visible_text\",\"presence\":\"present\",\"version_significant\":true,\"equivalence_fingerprint\":null}]'::jsonb WHERE file_id = $1",
    )
    .bind(file_id)
    .execute(&f.pool)
    .await
    .unwrap();

    for (break_binding, restore_binding) in [
        (
            "UPDATE document_semantic_inspections SET observed_raw_content_hash = decode(repeat('09', 32), 'hex') WHERE file_id = $1",
            "UPDATE document_semantic_inspections SET observed_raw_content_hash = decode(repeat('07', 32), 'hex') WHERE file_id = $1",
        ),
        (
            "UPDATE content_representations SET detected_format = 'csv' WHERE file_id = $1",
            "UPDATE content_representations SET detected_format = 'txt' WHERE file_id = $1",
        ),
        (
            "UPDATE content_representations SET inspection_profile_version = 'other' WHERE file_id = $1",
            "UPDATE content_representations SET inspection_profile_version = 'dsi-v0' WHERE file_id = $1",
        ),
        (
            "UPDATE content_representations SET semantic_fingerprint = decode(repeat('04', 32), 'hex') WHERE file_id = $1",
            "UPDATE content_representations SET semantic_fingerprint = decode(repeat('03', 32), 'hex') WHERE file_id = $1",
        ),
    ] {
        sqlx::query(break_binding)
            .bind(file_id)
            .execute(&f.pool)
            .await
            .unwrap();
        let invalid = reader
            .load_document_version(version_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(invalid.dsi_state, DsiReadState::UnknownInvalid);
        assert!(invalid.snapshot.dsi.is_none());
        sqlx::query(restore_binding)
            .bind(file_id)
            .execute(&f.pool)
            .await
            .unwrap();
    }

    sqlx::query(
        "UPDATE document_semantic_inspections SET observed_size_bytes = 9 WHERE file_id = $1",
    )
    .bind(file_id)
    .execute(&f.pool)
    .await
    .unwrap();
    let invalid = reader
        .load_document_version(version_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invalid.dsi_state, DsiReadState::UnknownInvalid);
    assert!(invalid.snapshot.dsi.is_none());

    sqlx::query("DELETE FROM document_semantic_inspections WHERE file_id = $1")
        .bind(file_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let missing = reader
        .load_document_version(version_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(missing.dsi_state, DsiReadState::UnknownMissing);
    assert!(missing.snapshot.dsi.is_none());
}

#[tokio::test]
async fn version_snapshot_uses_version_created_at_and_one_source_snapshot_for_enumeration() {
    let f = Fixture::new().await;
    let (_, first_version) = f.published_document(true).await;
    let (_, second_version) = f.published_document(true).await;
    sqlx::query(
        "UPDATE document_versions SET created_at = now() + interval '1 day' WHERE document_version_id = $1",
    )
    .bind(first_version.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    let expected: OffsetDateTime = sqlx::query_scalar(
        "SELECT created_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(first_version.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let reader = PostgresDocumentSnapshotReader::new(f.pool.clone());
    let live = reader.enumerate_live().await.unwrap();
    assert_eq!(live.len(), 2);
    assert_eq!(
        live[0].snapshot.source_snapshot,
        live[1].snapshot.source_snapshot
    );
    assert_eq!(
        live.iter()
            .find(|record| record.snapshot.document_version_id == first_version)
            .unwrap()
            .snapshot
            .created_at,
        expected
    );
    assert!(
        live.iter()
            .any(|record| record.snapshot.document_version_id == second_version)
    );
}

#[tokio::test]
async fn live_and_historical_enumeration_use_t10_operation_not_current_null_inference() {
    let f = Fixture::new().await;
    let (document_id, version_id) = f.published_document(true).await;
    let reader = PostgresDocumentSnapshotReader::new(f.pool.clone());
    assert_eq!(reader.enumerate_live().await.unwrap().len(), 1);
    assert!(reader.enumerate_historical().await.unwrap().is_empty());

    sqlx::query("UPDATE documents SET current_version_id = NULL WHERE document_id = $1")
        .bind(document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let prior_to_t10 = reader
        .load_document_version(version_id)
        .await
        .unwrap()
        .unwrap();
    assert!(prior_to_t10.snapshot.publication_end.is_none());
    assert!(reader.enumerate_live().await.unwrap().is_empty());
    assert_eq!(reader.enumerate_historical().await.unwrap().len(), 1);

    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id.as_uuid())
        .bind(document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    f.end_publication(document_id, version_id).await;
    let ended = reader
        .load_document_version(version_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ended
            .snapshot
            .publication_end
            .as_ref()
            .unwrap()
            .operation_id
            .as_bytes()
            .len(),
        16
    );
    assert!(reader.enumerate_live().await.unwrap().is_empty());
    assert_eq!(reader.enumerate_historical().await.unwrap().len(), 1);
}

#[tokio::test]
async fn current_access_uses_bound_actor_and_rechecks_current_version_after_revocation_and_t10() {
    let f = Fixture::new().await;
    let (document_id, version_id) = f.published_document(true).await;
    let policy_id = f.grant_read().await;
    let service =
        DocumentAccessCheckService::new(Arc::new(PostgresDocumentRepository::new(f.pool.clone())));
    let adapter = DocumentCurrentAccessAdapter::new(
        f.pool.clone(),
        service,
        actor(),
        "trusted-session".to_owned(),
    );
    let resource = ResourceId::from_uuid(version_id.as_uuid());
    assert_eq!(
        adapter.evaluate(resource, "trusted-session").await.unwrap(),
        AccessDecision::Allowed
    );
    assert_eq!(
        adapter
            .evaluate(resource, "public-request-user")
            .await
            .unwrap(),
        AccessDecision::Unknown
    );

    sqlx::query("DELETE FROM access_policy_grants WHERE policy_id = $1 AND action = 'read'")
        .bind(policy_id)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE document_access_state SET access_revision = 2 WHERE id = 1")
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        adapter.evaluate(resource, "trusted-session").await.unwrap(),
        AccessDecision::Denied
    );

    sqlx::query(
        "INSERT INTO access_policy_grants (policy_id, subject_kind, identity_provider, subject_id, action) \
         VALUES ($1, 'principal', 'test-idp', 'test-user', 'read')",
    )
    .bind(policy_id)
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE document_access_state SET access_revision = 3 WHERE id = 1")
        .execute(&f.pool)
        .await
        .unwrap();
    f.end_publication(document_id, version_id).await;
    assert_eq!(
        adapter.evaluate(resource, "trusted-session").await.unwrap(),
        AccessDecision::Denied
    );
}
