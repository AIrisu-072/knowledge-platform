use document_application::{
    PublishCandidate, PublishCommandIdentity, PublishDocumentResult, PublishOperationId,
    PublishOperationRecord, RepositoryError,
};
use document_domain::{DocumentId, DocumentVersionId, PrincipalRef};
use sqlx::PgPool;

use crate::{
    error::map_statement_error, mapping::to_authoritative, publish_rows::PublishOperationRow,
    rows::AuthoritativeRow,
};

pub(crate) async fn get_publish_operation(
    pool: &PgPool,
    operation_id: PublishOperationId,
) -> Result<Option<PublishOperationRecord>, RepositoryError> {
    let row = sqlx::query_as::<_, PublishOperationRow>(
        "SELECT publish_operation_id, document_id, target_document_version_id, \
                expected_document_revision, actor_identity_provider, actor_principal_id, \
                published_at, resulting_document_revision, created_at \
         FROM document_publish_operations \
         WHERE publish_operation_id = $1",
    )
    .bind(operation_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(map_statement_error)?;

    row.map(|row| {
        let stored_operation_id = PublishOperationId::try_from_uuid(row.publish_operation_id)
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        let principal =
            PrincipalRef::new(row.actor_identity_provider, row.actor_principal_id)
                .map_err(|_| RepositoryError::IntegrityViolation)?;
        let document_id = DocumentId::from_uuid(row.document_id);
        let version_id = DocumentVersionId::from_uuid(row.target_document_version_id);
        let identity = PublishCommandIdentity::from_persisted(
            stored_operation_id,
            document_id,
            version_id,
            row.expected_document_revision,
            principal,
        );
        let result = PublishDocumentResult::from_persisted(
            stored_operation_id,
            document_id,
            version_id,
            row.resulting_document_revision,
            row.published_at,
        );
        Ok(PublishOperationRecord::new(identity, result))
    })
    .transpose()
}

pub(crate) async fn get_publish_candidate(
    pool: &PgPool,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
) -> Result<PublishCandidate, RepositoryError> {
    let document_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM documents WHERE document_id = $1)")
            .bind(document_id.as_uuid())
            .fetch_one(pool)
            .await
            .map_err(map_statement_error)?;
    if !document_exists {
        return Err(RepositoryError::DocumentNotFound);
    }

    let version_document_id: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT document_id FROM document_versions WHERE document_version_id = $1")
            .bind(target_version_id.as_uuid())
            .fetch_optional(pool)
            .await
            .map_err(map_statement_error)?;
    let Some(version_document_id) = version_document_id else {
        return Err(RepositoryError::DocumentVersionNotFound);
    };
    if version_document_id != document_id.as_uuid() {
        return Err(RepositoryError::IntegrityViolation);
    }

    let row = sqlx::query_as::<_, AuthoritativeRow>(
        "SELECT \
            d.document_id, \
            d.folder_id, \
            d.current_version_id, \
            d.revision AS document_revision, \
            d.metadata AS document_metadata, \
            d.created_at AS document_created_at, \
            v.document_version_id, \
            v.version_no, \
            v.lifecycle_state, \
            v.title, \
            v.revision_reason, \
            v.approved_at, \
            v.scheduled_publish_at, \
            v.published_at, \
            v.withdrawn_at, \
            v.effective_from, \
            v.effective_to, \
            v.created_by_identity_provider, \
            v.created_by_principal_id, \
            v.metadata AS version_metadata, \
            v.created_at AS version_created_at, \
            f.file_id, \
            f.content_hash, \
            f.media_type, \
            f.size_bytes, \
            f.storage_locator, \
            f.created_at AS file_created_at, \
            vf.role, \
            vf.ordinal, \
            vf.original_filename \
         FROM documents d \
         JOIN document_versions v \
           ON v.document_id = d.document_id AND v.document_version_id = $2 \
         JOIN version_files vf \
           ON vf.document_version_id = v.document_version_id AND vf.role = 'PRIMARY' \
         JOIN file_objects f \
           ON f.file_id = vf.file_id \
         WHERE d.document_id = $1 \
         LIMIT 1",
    )
    .bind(document_id.as_uuid())
    .bind(target_version_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(map_statement_error)?
    .ok_or(RepositoryError::IntegrityViolation)?;

    let authoritative = to_authoritative(row)?;
    Ok(PublishCandidate::new(
        authoritative.document().clone(),
        authoritative.version().clone(),
        authoritative.file().clone(),
        authoritative.version_file().clone(),
    ))
}

#[cfg(test)]
mod tests {
    use document_application::{PublishOperationId, RepositoryError};
    use document_domain::{DocumentId, DocumentVersionId, FileId, LifecycleState};
    use sqlx::{PgPool, postgres::PgPoolOptions};
    use testcontainers::{
        GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::{SYSTEM_ROOT_FOLDER_ID, migrate};

    #[tokio::test]
    async fn publish_read_helpers_round_trip_operation_and_working_candidate() {
        let (_container, pool) = postgres().await;
        migrate(&pool).await.expect("migrations should succeed");
        let created_at = OffsetDateTime::from_unix_timestamp(1_700_001_000).unwrap();
        let document_id = DocumentId::from_uuid(id(10));
        let version_id = DocumentVersionId::from_uuid(id(11));
        let file_id = FileId::from_uuid(id(12));

        seed_initial(&pool, document_id, version_id, file_id, created_at).await;

        let unknown_operation = PublishOperationId::try_from_uuid(v7(1)).unwrap();
        assert!(
            super::get_publish_operation(&pool, unknown_operation)
                .await
                .expect("unknown operation lookup should succeed")
                .is_none()
        );

        let candidate = super::get_publish_candidate(&pool, document_id, version_id)
            .await
            .expect("working candidate should load");
        assert_eq!(candidate.document().document_id(), document_id);
        assert_eq!(candidate.document().current_version_id(), None);
        assert_eq!(candidate.document().revision(), 0);
        assert_eq!(candidate.version().document_version_id(), version_id);
        assert_eq!(candidate.version().lifecycle_state(), LifecycleState::Working);
        assert_eq!(candidate.file().file_id(), file_id);

        let operation_id = PublishOperationId::try_from_uuid(v7(2)).unwrap();
        let published_at = OffsetDateTime::from_unix_timestamp(1_700_001_100).unwrap();
        sqlx::query(
            "INSERT INTO document_publish_operations \
             (publish_operation_id, document_id, target_document_version_id, \
              expected_document_revision, actor_identity_provider, actor_principal_id, \
              published_at, resulting_document_revision, created_at) \
             VALUES ($1, $2, $3, 0, 'test-idp', 'actor-1', $4, 1, $4)",
        )
        .bind(operation_id.as_uuid())
        .bind(document_id.as_uuid())
        .bind(version_id.as_uuid())
        .bind(published_at)
        .execute(&pool)
        .await
        .expect("publish operation seed should insert");

        let stored = super::get_publish_operation(&pool, operation_id)
            .await
            .expect("operation lookup should succeed")
            .expect("operation should exist");
        assert_eq!(stored.identity().publish_operation_id(), operation_id);
        assert_eq!(stored.identity().document_id(), document_id);
        assert_eq!(
            stored.identity().target_document_version_id(),
            version_id
        );
        assert_eq!(stored.identity().expected_document_revision(), 0);
        assert_eq!(stored.identity().principal().identity_provider(), "test-idp");
        assert_eq!(stored.identity().principal().principal_id(), "actor-1");
        assert_eq!(stored.result().published_at(), published_at);
        assert_eq!(stored.result().resulting_document_revision(), 1);
    }

    #[tokio::test]
    async fn publish_candidate_lookup_preserves_missing_and_integrity_distinctions() {
        let (_container, pool) = postgres().await;
        migrate(&pool).await.expect("migrations should succeed");
        let created_at = OffsetDateTime::from_unix_timestamp(1_700_001_200).unwrap();

        let document_a = DocumentId::from_uuid(id(20));
        let version_a = DocumentVersionId::from_uuid(id(21));
        let file_a = FileId::from_uuid(id(22));
        seed_initial(&pool, document_a, version_a, file_a, created_at).await;

        let missing_document = super::get_publish_candidate(
            &pool,
            DocumentId::from_uuid(id(30)),
            DocumentVersionId::from_uuid(id(31)),
        )
        .await
        .unwrap_err();
        assert_eq!(missing_document, RepositoryError::DocumentNotFound);

        let missing_version =
            super::get_publish_candidate(&pool, document_a, DocumentVersionId::from_uuid(id(32)))
                .await
                .unwrap_err();
        assert_eq!(missing_version, RepositoryError::DocumentVersionNotFound);

        let document_b = DocumentId::from_uuid(id(40));
        let version_b = DocumentVersionId::from_uuid(id(41));
        let file_b = FileId::from_uuid(id(42));
        seed_initial(&pool, document_b, version_b, file_b, created_at).await;

        let wrong_owner = super::get_publish_candidate(&pool, document_a, version_b)
            .await
            .unwrap_err();
        assert_eq!(wrong_owner, RepositoryError::IntegrityViolation);

        sqlx::query(
            "DELETE FROM version_files WHERE document_version_id = $1 AND role = 'PRIMARY'",
        )
        .bind(version_a.as_uuid())
        .execute(&pool)
        .await
        .expect("primary link delete should succeed");

        let missing_primary = super::get_publish_candidate(&pool, document_a, version_a)
            .await
            .unwrap_err();
        assert_eq!(missing_primary, RepositoryError::IntegrityViolation);
    }

    async fn postgres() -> (testcontainers::ContainerAsync<GenericImage>, PgPool) {
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
            .max_connections(4)
            .connect(&database_url)
            .await
            .expect("postgres should accept connections");
        (container, pool)
    }

    async fn seed_initial(
        pool: &PgPool,
        document_id: DocumentId,
        version_id: DocumentVersionId,
        file_id: FileId,
        created_at: OffsetDateTime,
    ) {
        sqlx::query(
            "INSERT INTO documents \
             (document_id, folder_id, current_version_id, revision, metadata, created_at) \
             VALUES ($1, $2, NULL, 0, '{}'::jsonb, $3)",
        )
        .bind(document_id.as_uuid())
        .bind(SYSTEM_ROOT_FOLDER_ID)
        .bind(created_at)
        .execute(pool)
        .await
        .expect("document should insert");

        sqlx::query(
            "INSERT INTO document_versions \
             (document_version_id, document_id, version_no, lifecycle_state, title, \
              created_by_identity_provider, created_by_principal_id, metadata, created_at) \
             VALUES ($1, $2, 1, 'WORKING', 'Policy v1', \
                     'test-idp', 'creator-1', '{}'::jsonb, $3)",
        )
        .bind(version_id.as_uuid())
        .bind(document_id.as_uuid())
        .bind(created_at)
        .execute(pool)
        .await
        .expect("version should insert");

        sqlx::query(
            "INSERT INTO file_objects \
             (file_id, content_hash, media_type, size_bytes, storage_locator, created_at) \
             VALUES ($1, $2, 'application/pdf', 3, $3, $4)",
        )
        .bind(file_id.as_uuid())
        .bind(vec![7_u8; 32])
        .bind(format!("objects/{}/file", file_id.as_uuid()))
        .bind(created_at)
        .execute(pool)
        .await
        .expect("file should insert");

        sqlx::query(
            "INSERT INTO version_files \
             (document_version_id, file_id, role, ordinal, original_filename) \
             VALUES ($1, $2, 'PRIMARY', 0, 'policy.pdf')",
        )
        .bind(version_id.as_uuid())
        .bind(file_id.as_uuid())
        .execute(pool)
        .await
        .expect("primary file link should insert");
    }

    fn id(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    fn v7(value: u8) -> Uuid {
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8000-{value:012x}")).unwrap()
    }
}
