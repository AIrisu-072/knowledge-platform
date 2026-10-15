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
            let rows: Vec<(Option<uuid::Uuid>, String, i64)> = sqlx::query_as(
                "SELECT file.file_id, item.logical_path, item.ordinal::bigint \
                   FROM document_versions v \
                   JOIN content_items item ON item.document_version_id = v.document_version_id \
                   LEFT JOIN content_representations representation \
                     ON representation.content_item_id = item.content_item_id \
                    AND representation.content_representation_id = item.authoritative_representation_id \
                    AND representation.role = 'AUTHORITATIVE' \
                   LEFT JOIN file_objects file ON file.file_id = representation.file_id \
                  WHERE v.document_id = $1 AND v.document_version_id = $2 AND v.version_no = 1 \
                  ORDER BY item.ordinal, item.logical_path COLLATE \"C\"",
            )
            .bind(probe.document_id.as_uuid())
            .bind(probe.document_version_id.as_uuid())
            .fetch_all(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            Ok(match probe.file_ids {
                Some(ids) => !ids.is_empty() && ids[0] == probe.file_id &&
                    rows.iter().map(|row| row.0).eq(ids.iter().map(|id| Some(id.as_uuid()))),
                None => rows.len() == 1 && rows[0].0 == Some(probe.file_id.as_uuid()) &&
                    rows[0].1 == "primary" && rows[0].2 == 0,
            })
        }
        .await;
        tx.rollback().await.map_err(map_statement_error)?;
        result
    }
}
