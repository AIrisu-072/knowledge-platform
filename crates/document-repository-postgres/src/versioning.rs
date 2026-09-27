use document_application::{
    AuthoritativeDocument, RepositoryError, SemanticInspectionRepository, VersionMutationRecord,
    VersionOperationId, VersionOperationRecord, VersionOperationResult, VersioningRepository,
};
use document_domain::{DocumentId, DocumentVersionId, FileObject};

use crate::{
    error::map_statement_error, repository::PostgresDocumentRepository, versioning_mutation,
    versioning_rows,
};

impl VersioningRepository for PostgresDocumentRepository {
    async fn register_file_object(&self, file: FileObject) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO file_objects \
             (file_id, content_hash, media_type, size_bytes, storage_locator, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT DO NOTHING",
        )
        .bind(file.file_id().as_uuid())
        .bind(file.content_hash().as_bytes().to_vec())
        .bind(file.media_type().as_str())
        .bind(file.size_bytes().get())
        .bind(file.storage_key().as_str())
        .bind(file.created_at())
        .execute(&self.pool)
        .await
        .map_err(map_statement_error)?;

        let stored = self
            .get_file_object(file.file_id())
            .await?
            .ok_or(RepositoryError::IntegrityViolation)?;
        if stored.content_hash() != file.content_hash()
            || stored.size_bytes() != file.size_bytes()
            || stored.media_type() != file.media_type()
            || stored.storage_key() != file.storage_key()
        {
            return Err(RepositoryError::IntegrityViolation);
        }
        Ok(())
    }

    async fn get_version_operation(
        &self,
        operation_id: VersionOperationId,
    ) -> Result<Option<VersionOperationRecord>, RepositoryError> {
        versioning_mutation::get_operation(&self.pool, operation_id).await
    }

    async fn get_version_snapshot(
        &self,
        document_id: DocumentId,
        version_id: DocumentVersionId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        versioning_rows::load_version(&self.pool, document_id, version_id).await
    }

    async fn create_version(
        &self,
        record: VersionMutationRecord,
    ) -> Result<VersionOperationResult, RepositoryError> {
        if record.identity().kind() != document_application::VersionOperationKind::Create {
            return Err(RepositoryError::IntegrityViolation);
        }
        versioning_mutation::mutate(&self.pool, record).await
    }

    async fn update_working(
        &self,
        record: VersionMutationRecord,
    ) -> Result<VersionOperationResult, RepositoryError> {
        if record.identity().kind() != document_application::VersionOperationKind::Update {
            return Err(RepositoryError::IntegrityViolation);
        }
        versioning_mutation::mutate(&self.pool, record).await
    }

    async fn rebase_working(
        &self,
        record: VersionMutationRecord,
    ) -> Result<VersionOperationResult, RepositoryError> {
        if record.identity().kind() != document_application::VersionOperationKind::Rebase {
            return Err(RepositoryError::IntegrityViolation);
        }
        versioning_mutation::mutate(&self.pool, record).await
    }
}
