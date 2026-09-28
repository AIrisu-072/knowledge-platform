use std::sync::Arc;

use document_domain::{MediaType, StorageKey};
use uuid::Uuid;

use crate::{
    ApplicationError, ContentReader, FileStorage, RepositoryError, VerifiedActorContext,
    VersionFileRequest,
};

pub struct AuditedFileGrant {
    storage_key: StorageKey,
    media_type: MediaType,
    size_bytes: i64,
    safe_display_name: String,
    audit_event_id: Uuid,
}

impl AuditedFileGrant {
    pub fn new(
        storage_key: StorageKey,
        media_type: MediaType,
        size_bytes: i64,
        safe_display_name: String,
        audit_event_id: Uuid,
    ) -> Self {
        Self {
            storage_key,
            media_type,
            size_bytes,
            safe_display_name,
            audit_event_id,
        }
    }

    fn into_parts(self) -> (StorageKey, MediaType, i64, String, Uuid) {
        (
            self.storage_key,
            self.media_type,
            self.size_bytes,
            self.safe_display_name,
            self.audit_event_id,
        )
    }
}

pub struct OpenedVersionFile {
    pub content: ContentReader,
    pub media_type: MediaType,
    pub size_bytes: i64,
    pub safe_display_name: String,
    pub audit_event_id: Uuid,
}

#[allow(async_fn_in_trait)]
pub trait VersionFileAccessRepository: Send + Sync {
    async fn authorize_and_audit_file(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionFileRequest,
    ) -> Result<AuditedFileGrant, RepositoryError>;
}

pub struct VersionFileAccessService<R, F> {
    repository: Arc<R>,
    storage: Arc<F>,
}

impl<R: VersionFileAccessRepository, F: FileStorage> VersionFileAccessService<R, F> {
    pub fn new(repository: Arc<R>, storage: Arc<F>) -> Self {
        Self {
            repository,
            storage,
        }
    }

    pub async fn open_version_file(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionFileRequest,
    ) -> Result<OpenedVersionFile, ApplicationError> {
        ctx.ensure_current()?;
        let grant = self
            .repository
            .authorize_and_audit_file(ctx, request)
            .await
            .map_err(|error| match error {
                RepositoryError::CommitOutcomeUnknown => {
                    ApplicationError::FileAccessAuditCommitOutcomeUnknown {
                        document_id: request.version.document_id,
                        document_version_id: request.version.document_version_id,
                    }
                }
                other => other.into(),
            })?;
        let (key, media_type, size_bytes, safe_display_name, audit_event_id) = grant.into_parts();
        let content = self.storage.open(&key).await?;
        Ok(OpenedVersionFile {
            content,
            media_type,
            size_bytes,
            safe_display_name,
            audit_event_id,
        })
    }
}
