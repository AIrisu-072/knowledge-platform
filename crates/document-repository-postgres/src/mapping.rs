use document_application::{AuthoritativeContentItem, AuthoritativeDocument, RepositoryError};
use document_domain::{
    ContentHash, Document, DocumentId, DocumentVersion, DocumentVersionId, FileId, FileObject,
    FileSize, FolderId, LifecycleState, LogicalPath, MediaType, Metadata, PrincipalRef,
    RestoreDocument, RestoreDocumentVersion, StorageKey, StoredFileDescriptor, Title, VersionFile,
    VersionNo,
};
use serde_json::Value;

use crate::rows::{AuthoritativeRow, CanonicalContentRow};

pub(crate) fn to_authoritative(
    row: AuthoritativeRow,
) -> Result<AuthoritativeDocument, RepositoryError> {
    let content_rows = match &row.logical_path {
        Some(path) => vec![CanonicalContentRow {
            logical_path: path.clone(),
            ordinal: row.ordinal,
            file_id: row.file_id,
            content_hash: row.content_hash.clone(),
            media_type: row.media_type.clone(),
            size_bytes: row.size_bytes,
            storage_locator: row.storage_locator.clone(),
            file_created_at: row.file_created_at,
            original_filename: row.original_filename.clone(),
        }],
        None => vec![],
    };
    to_authoritative_with_items(row, content_rows)
}

pub(crate) fn to_authoritative_with_items(
    row: AuthoritativeRow,
    content_rows: Vec<CanonicalContentRow>,
) -> Result<AuthoritativeDocument, RepositoryError> {
    if row.role != "PRIMARY"
        || (row.requires_content_classification && !content_rows.is_empty())
        || (!row.requires_content_classification && content_rows.is_empty())
    {
        return Err(RepositoryError::IntegrityViolation);
    }
    let document_id = DocumentId::from_uuid(row.document_id);
    let version_id = DocumentVersionId::from_uuid(row.document_version_id);
    let document = Document::restore(RestoreDocument {
        document_id,
        folder_id: FolderId::from_uuid(row.folder_id),
        current_version_id: row.current_version_id.map(DocumentVersionId::from_uuid),
        revision: row.document_revision,
        metadata: metadata(row.document_metadata)?,
        created_at: row.document_created_at,
    })
    .map_err(|_| RepositoryError::IntegrityViolation)?;
    let lifecycle_state = match row.lifecycle_state.as_str() {
        "WORKING" => LifecycleState::Working,
        "PUBLISHED" => LifecycleState::Published,
        "WITHDRAWN" => LifecycleState::Withdrawn,
        _ => return Err(RepositoryError::IntegrityViolation),
    };
    if row.current_version_id == Some(row.document_version_id)
        && lifecycle_state != LifecycleState::Published
    {
        return Err(RepositoryError::IntegrityViolation);
    }
    let version = DocumentVersion::restore(RestoreDocumentVersion {
        document_version_id: version_id,
        document_id,
        version_no: VersionNo::new(row.version_no)
            .map_err(|_| RepositoryError::IntegrityViolation)?,
        base_document_version_id: row
            .base_document_version_id
            .map(DocumentVersionId::from_uuid),
        lifecycle_state,
        title: Title::new(row.title).map_err(|_| RepositoryError::IntegrityViolation)?,
        revision_reason: row.revision_reason,
        approved_at: row.approved_at,
        scheduled_publish_at: row.scheduled_publish_at,
        published_at: row.published_at,
        withdrawn_at: row.withdrawn_at,
        effective_from: row.effective_from,
        effective_to: row.effective_to,
        created_by: PrincipalRef::new(
            row.created_by_identity_provider,
            row.created_by_principal_id,
        )
        .map_err(|_| RepositoryError::IntegrityViolation)?,
        metadata: metadata(row.version_metadata)?,
        created_at: row.version_created_at,
    })
    .map_err(|_| RepositoryError::IntegrityViolation)?;
    let file = file_object(
        row.file_id,
        row.content_hash,
        row.media_type,
        row.size_bytes,
        row.storage_locator,
        row.file_created_at,
    )?;
    let version_file = VersionFile::restore_primary(
        version_id,
        FileId::from_uuid(row.file_id),
        row.original_filename,
    )
    .map_err(|_| RepositoryError::IntegrityViolation)?;
    let content_items = content_rows
        .into_iter()
        .map(|item| {
            let path = LogicalPath::new(&item.logical_path)
                .map_err(|_| RepositoryError::IntegrityViolation)?;
            if path.as_str() != item.logical_path || item.ordinal < 0 {
                return Err(RepositoryError::IntegrityViolation);
            }
            let file = file_object(
                item.file_id,
                item.content_hash,
                item.media_type,
                item.size_bytes,
                item.storage_locator,
                item.file_created_at,
            )?;
            Ok(AuthoritativeContentItem::new(
                path,
                u32::try_from(item.ordinal).map_err(|_| RepositoryError::IntegrityViolation)?,
                file,
                item.original_filename,
            ))
        })
        .collect::<Result<Vec<_>, RepositoryError>>()?;
    Ok(AuthoritativeDocument::from_parts_with_items(
        document,
        version,
        file,
        version_file,
        content_items,
        row.requires_content_classification,
    ))
}

fn file_object(
    file_id: uuid::Uuid,
    content_hash: Vec<u8>,
    media_type: String,
    size_bytes: i64,
    storage_locator: String,
    created_at: time::OffsetDateTime,
) -> Result<FileObject, RepositoryError> {
    let content_hash =
        ContentHash::from_slice(&content_hash).map_err(|_| RepositoryError::IntegrityViolation)?;
    let media_type = MediaType::new(media_type).map_err(|_| RepositoryError::IntegrityViolation)?;
    let size_bytes = FileSize::new(size_bytes).map_err(|_| RepositoryError::IntegrityViolation)?;
    let storage_key =
        StorageKey::new(storage_locator).map_err(|_| RepositoryError::IntegrityViolation)?;
    Ok(FileObject::restore(
        FileId::from_uuid(file_id),
        StoredFileDescriptor::new(storage_key, content_hash, size_bytes, media_type),
        created_at,
    ))
}

fn metadata(value: Value) -> Result<Metadata, RepositoryError> {
    match value {
        Value::Object(map) => Ok(Metadata::from_map(map)),
        _ => Err(RepositoryError::IntegrityViolation),
    }
}
