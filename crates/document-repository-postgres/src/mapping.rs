use document_application::{AuthoritativeDocument, RepositoryError};
use document_domain::{
    ContentHash, CreateInitialDocument, DocumentId, DocumentVersionId, FileId, FileSize, FolderId,
    InitialDocument, MediaType, Metadata, PrincipalRef, StorageKey, StoredFileDescriptor, Title,
};
use serde_json::Value;

use crate::rows::AuthoritativeRow;

pub(crate) fn to_authoritative(
    row: AuthoritativeRow,
) -> Result<AuthoritativeDocument, RepositoryError> {
    if row.current_version_id.is_some()
        || row.document_revision != 0
        || row.version_no != 1
        || row.lifecycle_state != "WORKING"
        || row.revision_reason.is_some()
        || row.approved_at.is_some()
        || row.scheduled_publish_at.is_some()
        || row.published_at.is_some()
        || row.withdrawn_at.is_some()
        || row.effective_from.is_some()
        || row.effective_to.is_some()
        || row.role != "PRIMARY"
        || row.ordinal != 0
        || row.document_created_at != row.version_created_at
        || row.document_created_at != row.file_created_at
    {
        return Err(RepositoryError::IntegrityViolation);
    }

    let document_metadata = metadata(row.document_metadata)?;
    let version_metadata = metadata(row.version_metadata)?;
    let title = Title::new(row.title).map_err(|_| RepositoryError::IntegrityViolation)?;
    let principal = PrincipalRef::new(
        row.created_by_identity_provider,
        row.created_by_principal_id,
    )
    .map_err(|_| RepositoryError::IntegrityViolation)?;
    let storage_key =
        StorageKey::new(row.storage_locator).map_err(|_| RepositoryError::IntegrityViolation)?;
    let content_hash =
        ContentHash::from_slice(&row.content_hash).map_err(|_| RepositoryError::IntegrityViolation)?;
    let size_bytes =
        FileSize::new(row.size_bytes).map_err(|_| RepositoryError::IntegrityViolation)?;
    let media_type =
        MediaType::new(row.media_type).map_err(|_| RepositoryError::IntegrityViolation)?;

    let initial = InitialDocument::create(CreateInitialDocument {
        document_id: DocumentId::from_uuid(row.document_id),
        version_id: DocumentVersionId::from_uuid(row.document_version_id),
        file_id: FileId::from_uuid(row.file_id),
        folder_id: FolderId::from_uuid(row.folder_id),
        title,
        document_metadata,
        version_metadata,
        principal,
        stored_file: StoredFileDescriptor::new(storage_key, content_hash, size_bytes, media_type),
        original_filename: row.original_filename,
        created_at: row.document_created_at,
    })
    .map_err(|_| RepositoryError::IntegrityViolation)?;

    Ok(AuthoritativeDocument::from_initial(initial))
}

fn metadata(value: Value) -> Result<Metadata, RepositoryError> {
    match value {
        Value::Object(map) => Ok(Metadata::from_map(map)),
        _ => Err(RepositoryError::IntegrityViolation),
    }
}
