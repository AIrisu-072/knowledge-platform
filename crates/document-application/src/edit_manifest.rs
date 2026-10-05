use std::{future::Future, sync::Arc};

use document_domain::{DocumentId, DocumentVersionId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApplicationError, RepositoryError, VerifiedActorContext, VersionPurpose, VersionRequest,
};

/// Editing deliberately has no history purpose or implicit history fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EditManifestPurpose {
    Published,
    Authoring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditManifestRequest {
    pub document_id: DocumentId,
    pub source_version_id: DocumentVersionId,
    pub purpose: EditManifestPurpose,
}

impl From<EditManifestRequest> for VersionRequest {
    fn from(request: EditManifestRequest) -> Self {
        Self {
            document_id: request.document_id,
            document_version_id: request.source_version_id,
            purpose: match request.purpose {
                EditManifestPurpose::Published => VersionPurpose::Published,
                EditManifestPurpose::Authoring => VersionPurpose::Authoring,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EditManifestRole {
    Authoritative,
    Rendition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditManifestRepresentation {
    pub representation_id: Uuid,
    pub role: EditManifestRole,
    pub file_id: Uuid,
    pub original_filename: String,
    pub media_type: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditManifestItem {
    pub content_item_id: Uuid,
    pub logical_path: String,
    pub ordinal: i32,
    pub representations: Vec<EditManifestRepresentation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditManifest {
    pub document_id: DocumentId,
    pub source_version_id: DocumentVersionId,
    pub document_revision: i64,
    pub purpose: EditManifestPurpose,
    pub title: String,
    pub items: Vec<EditManifestItem>,
}

pub trait EditManifestRepository: Send + Sync {
    /// Read+Write and purpose-specific lifecycle authorization, document revision,
    /// source title and the complete ordered manifest must share one snapshot.
    fn get_edit_manifest(
        &self,
        ctx: &VerifiedActorContext,
        request: EditManifestRequest,
    ) -> impl Future<Output = Result<EditManifest, RepositoryError>> + Send;
}

pub struct EditManifestService<R> {
    repository: Arc<R>,
}

impl<R: EditManifestRepository> EditManifestService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn read(
        &self,
        ctx: &VerifiedActorContext,
        request: EditManifestRequest,
    ) -> Result<EditManifest, ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .get_edit_manifest(ctx, request)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InvocationKind;
    use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use time::{Duration, OffsetDateTime};

    struct RejectingRepository(AtomicUsize);
    impl EditManifestRepository for RejectingRepository {
        async fn get_edit_manifest(
            &self,
            _: &VerifiedActorContext,
            _: EditManifestRequest,
        ) -> Result<EditManifest, RepositoryError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(RepositoryError::Forbidden)
        }
    }

    fn context(valid_until: OffsetDateTime) -> VerifiedActorContext {
        VerifiedActorContext::from_trusted_adapter(
            PrincipalRef::new("test", "editor").unwrap(),
            vec![PolicySubject::new(PolicySubjectKind::Principal, "test", "editor").unwrap()],
            valid_until,
            InvocationKind::Agent,
            None,
        )
        .unwrap()
    }

    fn request() -> EditManifestRequest {
        EditManifestRequest {
            document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
            source_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(2)),
            purpose: EditManifestPurpose::Published,
        }
    }

    #[tokio::test]
    async fn expired_edit_context_is_rejected_before_repository_access() {
        let ctx = context(OffsetDateTime::now_utc() + Duration::milliseconds(20));
        let repository = Arc::new(RejectingRepository(AtomicUsize::new(0)));
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let error = EditManifestService::new(repository.clone())
            .read(&ctx, request())
            .await
            .unwrap_err();
        assert!(matches!(error, ApplicationError::Validation(_)));
        assert_eq!(repository.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn edit_authorization_failure_propagates_without_fallback() {
        let repository = Arc::new(RejectingRepository(AtomicUsize::new(0)));
        let error = EditManifestService::new(repository.clone())
            .read(
                &context(OffsetDateTime::now_utc() + Duration::hours(1)),
                request(),
            )
            .await
            .unwrap_err();
        assert_eq!(error, ApplicationError::Forbidden);
        assert_eq!(repository.0.load(Ordering::SeqCst), 1);
    }
}
