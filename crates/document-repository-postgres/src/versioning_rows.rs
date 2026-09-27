use document_application::{AuthoritativeDocument, RepositoryError};
use document_domain::{DocumentId, DocumentVersionId};
use sqlx::{PgPool, Row, postgres::PgRow};

use crate::{
    error::map_statement_error,
    mapping::to_authoritative_with_items,
    rows::{AuthoritativeRow, CanonicalContentRow},
};

#[derive(Clone, Copy)]
enum ReadSelection {
    Internal = 0,
    Authoring = 1,
    CurrentPublished = 2,
}

pub(crate) async fn load_current(
    pool: &PgPool,
    document_id: DocumentId,
) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
    load_selected(pool, document_id, ReadSelection::Internal).await
}

pub(crate) async fn load_authoring(
    pool: &PgPool,
    document_id: DocumentId,
) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
    load_selected(pool, document_id, ReadSelection::Authoring).await
}

pub(crate) async fn load_current_published(
    pool: &PgPool,
    document_id: DocumentId,
) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
    load_selected(pool, document_id, ReadSelection::CurrentPublished).await
}

async fn load_selected(
    pool: &PgPool,
    document_id: DocumentId,
    selection: ReadSelection,
) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
    // The Document row is retained when its Version is absent, so one statement
    // distinguishes a valid null current from a broken nonnull current.
    let row = sqlx::query(
        "SELECT \
            d.document_id, d.folder_id, d.current_version_id, \
            d.revision AS document_revision, d.metadata AS document_metadata, \
            d.created_at AS document_created_at, \
            EXISTS (SELECT 1 FROM document_publication_end_operations e \
                    WHERE e.document_id = d.document_id) AS publication_ended, \
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
         LEFT JOIN document_versions v ON v.document_id = d.document_id \
          AND v.document_version_id = CASE WHEN $2 = 2 THEN d.current_version_id ELSE COALESCE( \
            d.current_version_id, \
            (SELECT working.document_version_id FROM document_versions working \
             WHERE working.document_id = d.document_id AND working.lifecycle_state = 'WORKING' \
             ORDER BY working.version_no DESC LIMIT 1), \
            (SELECT latest.document_version_id FROM document_versions latest \
             WHERE latest.document_id = d.document_id \
             ORDER BY latest.version_no DESC LIMIT 1) \
         ) END \
          AND ($2 <> 2 OR v.lifecycle_state = 'PUBLISHED') \
          AND ($2 = 0 OR NOT EXISTS \
               (SELECT 1 FROM document_publication_end_operations e WHERE e.document_id = d.document_id)) \
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
         LEFT JOIN file_objects f ON f.file_id = COALESCE(first_item.file_id, vf.file_id) \
         WHERE d.document_id = $1 \
         LIMIT 1",
    )
    .bind(document_id.as_uuid())
    .bind(selection as i32)
    .fetch_optional(pool)
    .await
    .map_err(map_statement_error)?;

    let Some(row) = row else { return Ok(None) };
    let ended: bool = row.get("publication_ended");
    let current_id: Option<uuid::Uuid> = row.get("current_version_id");
    if !matches!(selection, ReadSelection::Internal) && ended {
        return Ok(None);
    }
    if matches!(selection, ReadSelection::CurrentPublished) && current_id.is_none() {
        return Ok(None);
    }
    let version_id: Option<uuid::Uuid> = row.get("document_version_id");
    if version_id.is_none() {
        return Err(RepositoryError::IntegrityViolation);
    }
    let row = authoritative_row(row)?;
    if matches!(selection, ReadSelection::CurrentPublished)
        && (Some(row.document_version_id) != current_id || row.lifecycle_state != "PUBLISHED")
    {
        return Err(RepositoryError::IntegrityViolation);
    };
    let content_rows = load_content_items(pool, row.document_version_id).await?;
    Ok(Some(to_authoritative_with_items(row, content_rows)?))
}

fn authoritative_row(row: PgRow) -> Result<AuthoritativeRow, RepositoryError> {
    macro_rules! required {
        ($name:literal, $type:ty) => {
            row.try_get::<$type, _>($name)
                .map_err(|_| RepositoryError::IntegrityViolation)?
        };
    }
    Ok(AuthoritativeRow {
        document_id: required!("document_id", uuid::Uuid),
        folder_id: required!("folder_id", uuid::Uuid),
        current_version_id: required!("current_version_id", Option<uuid::Uuid>),
        document_revision: required!("document_revision", i64),
        document_metadata: required!("document_metadata", serde_json::Value),
        document_created_at: required!("document_created_at", time::OffsetDateTime),
        document_version_id: required!("document_version_id", uuid::Uuid),
        version_no: required!("version_no", i64),
        base_document_version_id: required!("base_document_version_id", Option<uuid::Uuid>),
        requires_content_classification: required!("requires_content_classification", bool),
        lifecycle_state: required!("lifecycle_state", String),
        title: required!("title", String),
        revision_reason: required!("revision_reason", Option<String>),
        approved_at: required!("approved_at", Option<time::OffsetDateTime>),
        scheduled_publish_at: required!("scheduled_publish_at", Option<time::OffsetDateTime>),
        published_at: required!("published_at", Option<time::OffsetDateTime>),
        withdrawn_at: required!("withdrawn_at", Option<time::OffsetDateTime>),
        effective_from: required!("effective_from", Option<time::OffsetDateTime>),
        effective_to: required!("effective_to", Option<time::OffsetDateTime>),
        created_by_identity_provider: required!("created_by_identity_provider", String),
        created_by_principal_id: required!("created_by_principal_id", String),
        version_metadata: required!("version_metadata", serde_json::Value),
        version_created_at: required!("version_created_at", time::OffsetDateTime),
        file_id: required!("file_id", uuid::Uuid),
        content_hash: required!("content_hash", Vec<u8>),
        media_type: required!("media_type", String),
        size_bytes: required!("size_bytes", i64),
        storage_locator: required!("storage_locator", String),
        file_created_at: required!("file_created_at", time::OffsetDateTime),
        role: required!("role", String),
        ordinal: required!("ordinal", i32),
        logical_path: required!("logical_path", Option<String>),
        original_filename: required!("original_filename", String),
    })
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
