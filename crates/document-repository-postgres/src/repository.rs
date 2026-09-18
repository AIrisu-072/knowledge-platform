use document_application::{
    AuthoritativeDocument, CreateInitialDocumentRecord, DocumentPublishRepository,
    DocumentRepository, PublishCandidate, PublishDocumentResult, PublishInitialVersionRecord,
    PublishOperationId, PublishOperationRecord, RepositoryError,
};
use document_domain::{DocumentId, FileId, FileRole};
use serde_json::Value;
use sqlx::PgPool;

use crate::{
    error::{map_commit_error, map_statement_error},
    mapping::to_authoritative,
    publish,
    rows::AuthoritativeRow,
};

const AUDIT_SOURCE: &str = "urn:knowledge-platform:document-platform";

#[derive(Debug, Clone)]
pub struct PostgresDocumentRepository {
    pool: PgPool,
}

impl PostgresDocumentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl DocumentRepository for PostgresDocumentRepository {
    async fn create_initial_document(
        &self,
        record: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        let (authoritative, domain_events, audit_events) = record.into_parts();
        let document = authoritative.document();
        let version = authoritative.version();
        let file = authoritative.file();
        let version_file = authoritative.version_file();

        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;

        let result: Result<(), RepositoryError> = async {
            let folder_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM folders WHERE folder_id = $1)",
            )
            .bind(document.folder_id().as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if !folder_exists {
                return Err(RepositoryError::FolderNotFound);
            }

            sqlx::query(
                "INSERT INTO file_objects \
                 (file_id, content_hash, media_type, size_bytes, storage_locator, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(file.file_id().as_uuid())
            .bind(file.content_hash().as_bytes().to_vec())
            .bind(file.media_type().as_str())
            .bind(file.size_bytes().get())
            .bind(file.storage_key().as_str())
            .bind(file.created_at())
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;

            sqlx::query(
                "INSERT INTO documents \
                 (document_id, folder_id, current_version_id, revision, metadata, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(document.document_id().as_uuid())
            .bind(document.folder_id().as_uuid())
            .bind(document.current_version_id().map(|id| id.as_uuid()))
            .bind(document.revision())
            .bind(Value::Object(document.metadata().as_map().clone()))
            .bind(document.created_at())
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;

            sqlx::query(
                "INSERT INTO document_versions \
                 (document_version_id, document_id, version_no, lifecycle_state, title, \
                  revision_reason, approved_at, scheduled_publish_at, published_at, withdrawn_at, \
                  effective_from, effective_to, created_by_identity_provider, \
                  created_by_principal_id, metadata, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
            )
            .bind(version.document_version_id().as_uuid())
            .bind(version.document_id().as_uuid())
            .bind(version.version_no().get())
            .bind(lifecycle_state(version.lifecycle_state()))
            .bind(version.title().as_str())
            .bind(version.revision_reason())
            .bind(version.approved_at())
            .bind(version.scheduled_publish_at())
            .bind(version.published_at())
            .bind(version.withdrawn_at())
            .bind(version.effective_from())
            .bind(version.effective_to())
            .bind(version.created_by().identity_provider())
            .bind(version.created_by().principal_id())
            .bind(Value::Object(version.metadata().as_map().clone()))
            .bind(version.created_at())
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;

            sqlx::query(
                "INSERT INTO version_files \
                 (document_version_id, file_id, role, ordinal, original_filename) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(version_file.document_version_id().as_uuid())
            .bind(version_file.file_id().as_uuid())
            .bind(file_role(version_file.role()))
            .bind(i64::from(version_file.ordinal()))
            .bind(version_file.original_filename())
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;

            for event in &domain_events {
                sqlx::query(
                    "INSERT INTO outbox_events \
                     (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at, \
                      available_at, attempt_count, delivered_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $6, 0, NULL)",
                )
                .bind(event.event_id().as_uuid())
                .bind(event.event_type())
                .bind(event.aggregate_type())
                .bind(event.aggregate_id().as_uuid())
                .bind(event.payload().clone())
                .bind(event.occurred_at())
                .execute(&mut *tx)
                .await
                .map_err(map_statement_error)?;
            }

            for event in &audit_events {
                let subject = format!("document/{}", event.resource_id().as_uuid());
                sqlx::query(
                    "INSERT INTO audit_outbox_events \
                     (event_id, event_type, source, subject, actor_identity_provider, \
                      actor_principal_id, resource_id, resource_version_id, result, trace_id, data, \
                      occurred_at, attempt_count, delivered_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10, $11, 0, NULL)",
                )
                .bind(event.event_id().as_uuid())
                .bind(event.event_type())
                .bind(AUDIT_SOURCE)
                .bind(subject)
                .bind(event.actor().identity_provider())
                .bind(event.actor().principal_id())
                .bind(event.resource_id().as_uuid())
                .bind(event.resource_version_id().map(|id| id.as_uuid()))
                .bind(event.result())
                .bind(event.data().clone())
                .bind(event.occurred_at())
                .execute(&mut *tx)
                .await
                .map_err(map_statement_error)?;
            }

            Ok(())
        }
        .await;

        if let Err(error) = result {
            let _ = tx.rollback().await;
            return Err(error);
        }

        tx.commit().await.map_err(map_commit_error)
    }

    async fn get_authoritative_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
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
               ON v.document_id = d.document_id AND v.version_no = 1 \
             JOIN version_files vf \
               ON vf.document_version_id = v.document_version_id AND vf.role = 'PRIMARY' \
             JOIN file_objects f \
               ON f.file_id = vf.file_id \
             WHERE d.document_id = $1 \
             LIMIT 1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_statement_error)?;

        row.map(to_authoritative).transpose()
    }

    async fn file_reference_exists(&self, file_id: FileId) -> Result<bool, RepositoryError> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM version_files WHERE file_id = $1)")
            .bind(file_id.as_uuid())
            .fetch_one(&self.pool)
            .await
            .map_err(map_statement_error)
    }

    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError> {
        let ids: Vec<uuid::Uuid> =
            sqlx::query_scalar("SELECT DISTINCT file_id FROM version_files ORDER BY file_id")
                .fetch_all(&self.pool)
                .await
                .map_err(map_statement_error)?;
        Ok(ids.into_iter().map(FileId::from_uuid).collect())
    }
}

fn lifecycle_state(state: document_domain::LifecycleState) -> &'static str {
    match state {
        document_domain::LifecycleState::Working => "WORKING",
        document_domain::LifecycleState::Published => "PUBLISHED",
        document_domain::LifecycleState::Withdrawn => "WITHDRAWN",
    }
}

fn file_role(role: FileRole) -> &'static str {
    match role {
        FileRole::Primary => "PRIMARY",
        FileRole::Attachment => "ATTACHMENT",
    }
}

impl DocumentPublishRepository for PostgresDocumentRepository {
    async fn get_publish_operation(
        &self,
        operation_id: PublishOperationId,
    ) -> Result<Option<PublishOperationRecord>, RepositoryError> {
        publish::get_publish_operation(&self.pool, operation_id).await
    }

    async fn get_publish_candidate(
        &self,
        document_id: DocumentId,
        target_version_id: document_domain::DocumentVersionId,
    ) -> Result<PublishCandidate, RepositoryError> {
        publish::get_publish_candidate(&self.pool, document_id, target_version_id).await
    }

    async fn publish_initial_version(
        &self,
        record: PublishInitialVersionRecord,
    ) -> Result<PublishDocumentResult, RepositoryError> {
        publish::publish_initial_version(&self.pool, record).await
    }
}
