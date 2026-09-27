use document_application::{AuthoritativeDocument, RepositoryError};
use document_domain::{DocumentId, DocumentVersionId};
use sqlx::PgPool;

use crate::{
    error::map_statement_error,
    mapping::to_authoritative_with_items,
    rows::{AuthoritativeRow, CanonicalContentRow},
};

pub(crate) async fn load_current(
    pool: &PgPool,
    document_id: DocumentId,
) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
    let row = sqlx::query_as::<_, AuthoritativeRow>(
        "SELECT \
            d.document_id, d.folder_id, d.current_version_id, \
            d.revision AS document_revision, d.metadata AS document_metadata, \
            d.created_at AS document_created_at, \
            v.document_version_id, v.version_no, v.base_document_version_id, \
            v.requires_content_classification, v.lifecycle_state, v.title, \
            v.revision_reason, v.approved_at, v.scheduled_publish_at, \
            v.published_at, v.withdrawn_at, v.effective_from, v.effective_to, \
            v.created_by_identity_provider, v.created_by_principal_id, \
            v.metadata AS version_metadata, v.created_at AS version_created_at, \
            f.file_id, f.content_hash, f.media_type, f.size_bytes, \
            f.storage_locator, f.created_at AS file_created_at, \
            COALESCE(vf.role, 'PRIMARY') AS role, \
            COALESCE(first_item.ordinal, vf.ordinal) AS ordinal, \
            first_item.logical_path, \
            COALESCE(first_item.original_filename, vf.original_filename) AS original_filename \
         FROM documents d \
         JOIN document_versions v ON v.document_version_id = COALESCE( \
            d.current_version_id, \
            (SELECT working.document_version_id FROM document_versions working \
             WHERE working.document_id = d.document_id AND working.lifecycle_state = 'WORKING' \
             ORDER BY working.version_no DESC LIMIT 1), \
            (SELECT latest.document_version_id FROM document_versions latest \
             WHERE latest.document_id = d.document_id \
             ORDER BY latest.version_no DESC LIMIT 1) \
         ) \
         LEFT JOIN LATERAL ( \
            SELECT ci.logical_path, ci.ordinal, cr.file_id, cr.original_filename \
            FROM content_items ci \
            JOIN content_representations cr \
              ON cr.content_representation_id = ci.authoritative_representation_id \
             AND cr.content_item_id = ci.content_item_id AND cr.role = 'AUTHORITATIVE' \
            WHERE ci.document_version_id = v.document_version_id \
            ORDER BY ci.ordinal, ci.logical_path LIMIT 1 \
         ) first_item ON TRUE \
         LEFT JOIN version_files vf \
           ON vf.document_version_id = v.document_version_id \
          AND vf.role = 'PRIMARY' AND v.requires_content_classification \
         JOIN file_objects f ON f.file_id = COALESCE(first_item.file_id, vf.file_id) \
         WHERE d.document_id = $1 \
         LIMIT 1",
    )
    .bind(document_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(map_statement_error)?;

    let Some(row) = row else {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM documents WHERE document_id = $1)")
                .bind(document_id.as_uuid())
                .fetch_one(pool)
                .await
                .map_err(map_statement_error)?;
        return if exists {
            Err(RepositoryError::IntegrityViolation)
        } else {
            Ok(None)
        };
    };
    let content_rows = load_content_items(pool, row.document_version_id).await?;
    Ok(Some(to_authoritative_with_items(row, content_rows)?))
}

pub(crate) async fn load_content_items(
    pool: &PgPool,
    version_id: uuid::Uuid,
) -> Result<Vec<CanonicalContentRow>, RepositoryError> {
    sqlx::query_as::<_, CanonicalContentRow>(
        "SELECT ci.logical_path, ci.ordinal, f.file_id, f.content_hash, \
                f.media_type, f.size_bytes, f.storage_locator, \
                f.created_at AS file_created_at, cr.original_filename \
         FROM content_items ci \
         JOIN content_representations cr \
           ON cr.content_representation_id = ci.authoritative_representation_id \
          AND cr.content_item_id = ci.content_item_id AND cr.role = 'AUTHORITATIVE' \
         JOIN file_objects f ON f.file_id = cr.file_id \
         WHERE ci.document_version_id = $1 \
         ORDER BY ci.ordinal, ci.logical_path",
    )
    .bind(version_id)
    .fetch_all(pool)
    .await
    .map_err(map_statement_error)
}

pub(crate) async fn load_version(
    pool: &PgPool,
    document_id: DocumentId,
    version_id: DocumentVersionId,
) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
    let row = sqlx::query_as::<_, AuthoritativeRow>(
        "SELECT d.document_id, d.folder_id, d.current_version_id, \
                d.revision AS document_revision, d.metadata AS document_metadata, \
                d.created_at AS document_created_at, \
                v.document_version_id, v.version_no, v.base_document_version_id, \
                v.requires_content_classification, v.lifecycle_state, v.title, \
                v.revision_reason, v.approved_at, v.scheduled_publish_at, \
                v.published_at, v.withdrawn_at, v.effective_from, v.effective_to, \
                v.created_by_identity_provider, v.created_by_principal_id, \
                v.metadata AS version_metadata, v.created_at AS version_created_at, \
                f.file_id, f.content_hash, f.media_type, f.size_bytes, \
                f.storage_locator, f.created_at AS file_created_at, \
                COALESCE(vf.role, 'PRIMARY') AS role, \
                COALESCE(first_item.ordinal, vf.ordinal) AS ordinal, \
                first_item.logical_path, \
                COALESCE(first_item.original_filename, vf.original_filename) AS original_filename \
         FROM documents d \
         JOIN document_versions v ON v.document_id = d.document_id AND v.document_version_id = $2 \
         LEFT JOIN LATERAL ( \
            SELECT ci.logical_path, ci.ordinal, cr.file_id, cr.original_filename \
            FROM content_items ci JOIN content_representations cr \
              ON cr.content_representation_id = ci.authoritative_representation_id \
             AND cr.content_item_id = ci.content_item_id AND cr.role = 'AUTHORITATIVE' \
            WHERE ci.document_version_id = v.document_version_id \
            ORDER BY ci.ordinal, ci.logical_path LIMIT 1 \
         ) first_item ON TRUE \
         LEFT JOIN version_files vf ON vf.document_version_id = v.document_version_id \
             AND vf.role = 'PRIMARY' AND v.requires_content_classification \
         JOIN file_objects f ON f.file_id = COALESCE(first_item.file_id, vf.file_id) \
         WHERE d.document_id = $1 LIMIT 1",
    )
    .bind(document_id.as_uuid())
    .bind(version_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(map_statement_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let content_rows = load_content_items(pool, row.document_version_id).await?;
    Ok(Some(to_authoritative_with_items(row, content_rows)?))
}
