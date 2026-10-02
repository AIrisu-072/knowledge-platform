use std::future::Future;
use std::sync::Arc;

use document_domain::{DocumentId, FolderId};
use serde::Serialize;

use crate::{ApplicationError, RepositoryError, VerifiedActorContext, VersionRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CapabilityDisabledReason {
    Permission,
    Lifecycle,
    PendingSchedule,
    StaleBase,
    NotCurrent,
    NotHumanInteractive,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ActionAvailability {
    Available,
    Disabled { reason: CapabilityDisabledReason },
}

impl ActionAvailability {
    pub const fn available() -> Self {
        Self::Available
    }

    pub const fn disabled(reason: CapabilityDisabledReason) -> Self {
        Self::Disabled { reason }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentActionCapabilities {
    pub create_version: ActionAvailability,
    pub update_metadata: ActionAvailability,
    pub move_document: ActionAvailability,
    pub end_publication: ActionAvailability,
    pub manage_access: ActionAvailability,
    pub compare_versions: ActionAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionActionCapabilities {
    pub edit: ActionAvailability,
    pub rebase: ActionAvailability,
    pub publish: ActionAvailability,
    pub withdraw: ActionAvailability,
    pub schedule_publication: ActionAvailability,
    pub cancel_publication_schedule: ActionAvailability,
    pub download: ActionAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderActionCapabilities {
    pub create_document: ActionAvailability,
    pub create_folder: ActionAvailability,
    pub rename_folder: ActionAvailability,
    pub move_folder: ActionAvailability,
    pub manage_access: ActionAvailability,
}

pub trait ActionCapabilityReadRepository: Send + Sync {
    fn read_document_action_capabilities(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
    ) -> impl Future<Output = Result<DocumentActionCapabilities, RepositoryError>> + Send;

    fn read_version_action_capabilities(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> impl Future<Output = Result<VersionActionCapabilities, RepositoryError>> + Send;

    fn read_folder_action_capabilities(
        &self,
        ctx: &VerifiedActorContext,
        folder_id: FolderId,
    ) -> impl Future<Output = Result<FolderActionCapabilities, RepositoryError>> + Send;
}

pub struct ActionCapabilityReadService<R> {
    repository: Arc<R>,
}

impl<R: ActionCapabilityReadRepository> ActionCapabilityReadService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn read_document(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
    ) -> Result<DocumentActionCapabilities, ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .read_document_action_capabilities(ctx, document_id)
            .await
            .map_err(Into::into)
    }

    pub async fn read_version(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> Result<VersionActionCapabilities, ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .read_version_action_capabilities(ctx, request)
            .await
            .map_err(Into::into)
    }

    pub async fn read_folder(
        &self,
        ctx: &VerifiedActorContext,
        folder_id: FolderId,
    ) -> Result<FolderActionCapabilities, ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .read_folder_action_capabilities(ctx, folder_id)
            .await
            .map_err(Into::into)
    }
}
