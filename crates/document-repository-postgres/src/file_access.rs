use document_application::{
    AuditedFileGrant, RepositoryError, VerifiedActorContext, VersionFileAccessRepository,
    VersionFileRequest, VersionPurpose,
};
use document_domain::{MediaType, StorageKey};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, lock_access_state},
    document_history::{authorize_version_in_tx, begin_snapshot, safe_display_name},
    error::{map_commit_error, map_statement_error},
};

impl VersionFileAccessRepository for PostgresDocumentRepository {
    async fn authorize_and_audit_file(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionFileRequest,
    ) -> Result<AuditedFileGrant, RepositoryError> {
        let mut tx = begin_snapshot(&self.pool).await?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        authorize_version_in_tx(&mut tx, ctx, request.version).await?;
        let row = sqlx::query(
            "SELECT file.storage_locator,file.media_type,file.size_bytes,rep.original_filename \
             FROM content_items item \
             JOIN content_representations rep ON rep.content_item_id = item.content_item_id \
             JOIN file_objects file ON file.file_id = rep.file_id \
             WHERE item.document_version_id = $1 AND item.content_item_id = $2 \
               AND rep.content_representation_id = $3",
        )
        .bind(request.version.document_version_id.as_uuid())
        .bind(request.content_item_id)
        .bind(request.representation_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_statement_error)?
        .ok_or(RepositoryError::FileObjectNotFound)?;
        let key: String = row
            .try_get("storage_locator")
            .map_err(map_statement_error)?;
        let media_type: String = row.try_get("media_type").map_err(map_statement_error)?;
        let size_bytes: i64 = row.try_get("size_bytes").map_err(map_statement_error)?;
        let name: String = row
            .try_get("original_filename")
            .map_err(map_statement_error)?;
        let key = StorageKey::new(key).map_err(|_| RepositoryError::IntegrityViolation)?;
        let media_type =
            MediaType::new(media_type).map_err(|_| RepositoryError::IntegrityViolation)?;
        let audit_event_id = Uuid::now_v7();
        let purpose = match request.version.purpose {
            VersionPurpose::Published => "published",
            VersionPurpose::Authoring => "authoring",
            VersionPurpose::History => "history",
        };
        sqlx::query(
            "INSERT INTO audit_outbox_events \
             (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id, \
              resource_type,resource_id,resource_version_id,result,data,occurred_at) \
             VALUES ($1,'document.file.access_granted', \
                     'urn:knowledge-platform:document-platform',$2,$3,$4, \
                     'Document',$5,$6,'success',$7,now())",
        )
        .bind(audit_event_id)
        .bind(format!(
            "document/{}/version/{}/representation/{}",
            request.version.document_id.as_uuid(),
            request.version.document_version_id.as_uuid(),
            request.representation_id,
        ))
        .bind(ctx.principal().identity_provider())
        .bind(ctx.principal().principal_id())
        .bind(request.version.document_id.as_uuid())
        .bind(request.version.document_version_id.as_uuid())
        .bind(json!({
            "content_item_id": request.content_item_id,
            "representation_id": request.representation_id,
            "purpose": purpose,
        }))
        .execute(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        tx.commit().await.map_err(map_commit_error)?;
        Ok(AuditedFileGrant::new(
            key,
            media_type,
            size_bytes,
            safe_display_name(&name),
            audit_event_id,
        ))
    }
}
