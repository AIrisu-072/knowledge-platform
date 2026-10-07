use std::future::Future;
use std::sync::Arc;

use document_domain::{DocumentId, DocumentVersionId, PrincipalRef};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApplicationError, GuiVersionFileSummary, Page, RepositoryError, VerifiedActorContext,
    validate_page_size,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionPurpose {
    Published,
    Authoring,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionRequest {
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub purpose: VersionPurpose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionFileRequest {
    pub version: VersionRequest,
    pub content_item_id: Uuid,
    pub representation_id: Uuid,
    pub correlation_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionPageQuery {
    pub document_id: DocumentId,
    pub purpose: VersionPurpose,
    pub page_size: Option<u16>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPageQuery {
    pub document_id: DocumentId,
    pub page_size: Option<u16>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VersionSummary {
    pub document_version_id: DocumentVersionId,
    pub version_no: i64,
    pub base_document_version_id: Option<DocumentVersionId>,
    pub lifecycle_state: String,
    pub is_current: bool,
    pub created_at: OffsetDateTime,
    pub approved_at: Option<OffsetDateTime>,
    pub scheduled_publish_at: Option<OffsetDateTime>,
    pub published_at: Option<OffsetDateTime>,
    pub withdrawn_at: Option<OffsetDateTime>,
    pub updated_at: OffsetDateTime,
    pub file_summary: GuiVersionFileSummary,
    /// Historical first record; use CurrentReadProjection for the current badge.
    pub first_read_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VersionDetail {
    pub summary: VersionSummary,
    pub title: String,
    pub metadata: Value,
    /// 同じ認可済みsnapshotで対象Version・時刻に一致するPENDING予約のPublish ID。
    pub current_publication_schedule_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionFileSummary {
    pub content_item_id: Uuid,
    pub representation_id: Uuid,
    pub logical_path: String,
    pub ordinal: i32,
    pub role: String,
    pub safe_display_name: String,
    pub media_type: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceQuality {
    OperationLedger,
    VersionFallback,
    LegacyUnknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentHistoryEntry {
    pub source_kind: String,
    pub source_key: String,
    pub occurred_at: Option<OffsetDateTime>,
    pub actor: Option<PrincipalRef>,
    pub action_code: String,
    pub details: Value,
    pub provenance_quality: ProvenanceQuality,
}

pub trait DocumentHistoryRepository: Send + Sync {
    fn list_document_versions(
        &self,
        ctx: &VerifiedActorContext,
        query: VersionPageQuery,
    ) -> impl Future<Output = Result<Page<VersionSummary>, RepositoryError>> + Send;

    fn list_document_history(
        &self,
        ctx: &VerifiedActorContext,
        query: HistoryPageQuery,
    ) -> impl Future<Output = Result<Page<DocumentHistoryEntry>, RepositoryError>> + Send;

    fn get_document_version(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> impl Future<Output = Result<VersionDetail, RepositoryError>> + Send;

    fn list_version_files(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> impl Future<Output = Result<Vec<VersionFileSummary>, RepositoryError>> + Send;
}

pub struct DocumentHistoryService<R> {
    repository: Arc<R>,
}

impl<R: DocumentHistoryRepository> DocumentHistoryService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn list_document_versions(
        &self,
        ctx: &VerifiedActorContext,
        query: VersionPageQuery,
    ) -> Result<Page<VersionSummary>, ApplicationError> {
        ctx.ensure_current()?;
        validate_page_size(query.page_size.unwrap_or(50))?;
        self.repository
            .list_document_versions(ctx, query)
            .await
            .map_err(Into::into)
    }

    pub async fn list_document_history(
        &self,
        ctx: &VerifiedActorContext,
        query: HistoryPageQuery,
    ) -> Result<Page<DocumentHistoryEntry>, ApplicationError> {
        ctx.ensure_current()?;
        validate_page_size(query.page_size.unwrap_or(50))?;
        self.repository
            .list_document_history(ctx, query)
            .await
            .map_err(Into::into)
    }

    pub async fn get_document_version(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> Result<VersionDetail, ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .get_document_version(ctx, request)
            .await
            .map_err(Into::into)
    }

    pub async fn list_version_files(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> Result<Vec<VersionFileSummary>, ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .list_version_files(ctx, request)
            .await
            .map_err(Into::into)
    }
}
