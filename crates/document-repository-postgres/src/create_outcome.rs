use document_application::{
    CreateOutcomeProbe, CreateOutcomeRepository, RepositoryError, VerifiedActorContext,
};
use document_domain::{Action, ResourceRef};

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state},
    error::map_statement_error,
};

impl CreateOutcomeRepository for PostgresDocumentRepository {
    async fn recover_initial_create(
        &self,
        ctx: &VerifiedActorContext,
        probe: CreateOutcomeProbe,
    ) -> Result<bool, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<bool, RepositoryError> = async {
            lock_access_state(&mut tx, AccessLockMode::Shared).await?;
            match authorize_in_tx(
                &mut tx,
                ctx,
                &[(ResourceRef::Document(probe.document_id), vec![Action::Read, Action::Write])],
            )
            .await
            {
                Ok(()) => {}
                Err(RepositoryError::Forbidden) => return Ok(false),
                Err(error) => return Err(error),
            }
            sqlx::query_scalar(
                "SELECT EXISTS ( \
                   SELECT 1 FROM documents d \
                   JOIN document_versions v ON v.document_id = d.document_id \
                   JOIN content_items item ON item.document_version_id = v.document_version_id \
                   JOIN content_representations representation \
                     ON representation.content_item_id = item.content_item_id \
                    AND representation.content_representation_id = item.authoritative_representation_id \
                   JOIN file_objects file ON file.file_id = representation.file_id \
                  WHERE d.document_id = $1 AND v.document_version_id = $2 AND v.version_no = 1 \
                    AND item.logical_path = 'primary' AND item.ordinal = 0 \
                    AND representation.role = 'AUTHORITATIVE' AND file.file_id = $3 \
                 )",
            )
            .bind(probe.document_id.as_uuid())
            .bind(probe.document_version_id.as_uuid())
            .bind(probe.file_id.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(map_statement_error)
        }
        .await;
        tx.rollback().await.map_err(map_statement_error)?;
        result
    }
}
