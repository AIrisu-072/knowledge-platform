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
    if row.version_no != 1
        || row.revision_reason.is_some()
        || row.approved_at.is_some()
        || row.scheduled_publish_at.is_some()
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

    let lifecycle_state = row.lifecycle_state.clone();
    let current_version_id = row.current_version_id;
    let document_revision = row.document_revision;
    let document_version_id = row.document_version_id;
    let published_at = row.published_at;

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
    let content_hash = ContentHash::from_slice(&row.content_hash)
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let size_bytes =
        FileSize::new(row.size_bytes).map_err(|_| RepositoryError::IntegrityViolation)?;
    let media_type =
        MediaType::new(row.media_type).map_err(|_| RepositoryError::IntegrityViolation)?;

    let input = CreateInitialDocument {
        document_id: DocumentId::from_uuid(row.document_id),
        version_id: DocumentVersionId::from_uuid(document_version_id),
        file_id: FileId::from_uuid(row.file_id),
        folder_id: FolderId::from_uuid(row.folder_id),
        title,
        document_metadata,
        version_metadata,
        principal,
        stored_file: StoredFileDescriptor::new(storage_key, content_hash, size_bytes, media_type),
        original_filename: row.original_filename,
        created_at: row.document_created_at,
    };

    let initial = match lifecycle_state.as_str() {
        "WORKING"
            if current_version_id.is_none() && document_revision == 0 && published_at.is_none() =>
        {
            InitialDocument::create(input)
        }
        "PUBLISHED"
            if current_version_id == Some(document_version_id)
                && document_revision == 1
                && published_at.is_some() =>
        {
            InitialDocument::restore_published(
                input,
                published_at.expect("published state checked above"),
            )
        }
        _ => return Err(RepositoryError::IntegrityViolation),
    }
    .map_err(|_| RepositoryError::IntegrityViolation)?;

    Ok(AuthoritativeDocument::from_initial(initial))
}

fn metadata(value: Value) -> Result<Metadata, RepositoryError> {
    match value {
        Value::Object(map) => Ok(Metadata::from_map(map)),
        _ => Err(RepositoryError::IntegrityViolation),
    }
}
