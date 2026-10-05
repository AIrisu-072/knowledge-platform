use std::future::Future;
use std::sync::Arc;

use document_domain::{Action, DocumentId, DocumentVersionId, FolderId};
use serde_json::{Value, json};
use time::OffsetDateTime;
use unicode_normalization::UnicodeNormalization;

use crate::{
    ApplicationError, DocumentRevisionSummary, DocumentSort, RepositoryError, VerifiedActorContext,
    validate_page_size,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiPrimaryFileSummary {
    pub display_name: String,
    pub media_type: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiVersionFileSummary {
    pub authoritative_item_count: i64,
    pub total_size_bytes: i64,
    pub primary: Option<GuiPrimaryFileSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiVersionSummary {
    pub document_version_id: DocumentVersionId,
    pub version_no: i64,
    pub base_document_version_id: Option<DocumentVersionId>,
    pub lifecycle_state: String,
    pub is_current: bool,
    pub approved_at: Option<OffsetDateTime>,
    pub scheduled_publish_at: Option<OffsetDateTime>,
    pub published_at: Option<OffsetDateTime>,
    pub withdrawn_at: Option<OffsetDateTime>,
    pub updated_at: OffsetDateTime,
    pub file_summary: GuiVersionFileSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayTimestampKind {
    RevisionCreatedAt,
    WorkingUpdatedAt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiDisplayTimestamp {
    pub kind: DisplayTimestampKind,
    pub value: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuiDocumentReadModel {
    pub display_version: GuiVersionSummary,
    pub display_revision: Option<DocumentRevisionSummary>,
    pub first_read_at: Option<OffsetDateTime>,
    pub display_timestamp: GuiDisplayTimestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentListFilter {
    pub exact_document_id: Option<DocumentId>,
    pub title_contains: Option<String>,
    pub folder_id: Option<FolderId>,
    pub include_descendants: bool,
    pub document_type: Option<String>,
    pub owning_department: Option<String>,
    pub category: Option<String>,
    pub created_from: Option<OffsetDateTime>,
    pub created_before: Option<OffsetDateTime>,
}

impl DocumentListFilter {
    pub fn normalized(mut self) -> Result<Self, ApplicationError> {
        if let Some(title) = &self.title_contains {
            let normalized = title.trim().nfc().collect::<String>();
            if normalized.is_empty() || normalized.len() > 1024 {
                return Err(ApplicationError::Validation("invalid title filter".into()));
            }
            self.title_contains = Some(normalized);
        }
        for value in [&self.document_type, &self.owning_department, &self.category]
            .into_iter()
            .flatten()
        {
            if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
                return Err(ApplicationError::Validation(
                    "invalid metadata filter".into(),
                ));
            }
        }
        if self.include_descendants && self.folder_id.is_none() {
            return Err(ApplicationError::Validation(
                "descendant scope requires a folder".into(),
            ));
        }
        if let (Some(from), Some(before)) = (self.created_from, self.created_before)
            && from >= before
        {
            return Err(ApplicationError::Validation("invalid date range".into()));
        }
        Ok(self)
    }

    pub fn fingerprint_value(&self) -> Value {
        json!({
            "exact_document_id": self.exact_document_id.map(|id| id.as_uuid()),
            "title_contains": self.title_contains,
            "folder_id": self.folder_id.map(|id| id.as_uuid()),
            "include_descendants": self.include_descendants,
            "document_type": self.document_type,
            "owning_department": self.owning_department,
            "category": self.category,
            "created_from_micros": self.created_from.map(|date| date.unix_timestamp_nanos() / 1000),
            "created_before_micros": self.created_before.map(|date| date.unix_timestamp_nanos() / 1000),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedQuery {
    pub filter: DocumentListFilter,
    pub sort: DocumentSort,
    pub page_size: Option<u16>,
    pub cursor: Option<String>,
    pub unread_only: bool,
}

impl Default for PublishedQuery {
    fn default() -> Self {
        Self {
            filter: DocumentListFilter::default(),
            sort: DocumentSort::CreatedAtDesc,
            page_size: None,
            cursor: None,
            unread_only: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoringQuery {
    pub filter: DocumentListFilter,
    pub sort: DocumentSort,
    pub page_size: Option<u16>,
    pub cursor: Option<String>,
}

impl Default for AuthoringQuery {
    fn default() -> Self {
        Self {
            filter: DocumentListFilter::default(),
            sort: DocumentSort::CreatedAtDesc,
            page_size: None,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryQuery {
    pub filter: DocumentListFilter,
    pub sort: DocumentSort,
    pub page_size: Option<u16>,
    pub cursor: Option<String>,
}

impl Default for HistoryQuery {
    fn default() -> Self {
        Self {
            filter: DocumentListFilter::default(),
            sort: DocumentSort::CreatedAtDesc,
            page_size: None,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderPageQuery {
    pub parent_folder_id: FolderId,
    pub page_size: Option<u16>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublishedDocumentSummary {
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub title: String,
    pub folder_id: Option<FolderId>,
    pub folder_name: Option<String>,
    pub document_metadata: Value,
    pub created_at: OffsetDateTime,
    pub published_at: OffsetDateTime,
    pub first_read_at: Option<OffsetDateTime>,
    pub document_revision: i64,
    pub gui: GuiDocumentReadModel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthoringDocumentSummary {
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub title: String,
    pub lifecycle_state: String,
    pub current_version_id: Option<DocumentVersionId>,
    pub folder_id: Option<FolderId>,
    pub folder_name: Option<String>,
    pub document_metadata: Value,
    pub created_at: OffsetDateTime,
    pub document_revision: i64,
    pub gui: GuiDocumentReadModel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryDocumentSummary {
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub title: String,
    pub lifecycle_state: String,
    pub ended: bool,
    pub folder_id: Option<FolderId>,
    pub folder_name: Option<String>,
    pub document_metadata: Value,
    pub created_at: OffsetDateTime,
    pub document_revision: i64,
    pub gui: GuiDocumentReadModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSummary {
    pub folder_id: FolderId,
    pub parent_folder_id: FolderId,
    pub name: String,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootFolderSummary {
    pub folder_id: FolderId,
    pub name: String,
    pub revision: i64,
}

pub trait DocumentQueryRepository: Send + Sync {
    fn get_root_folder(
        &self,
        ctx: &VerifiedActorContext,
    ) -> impl Future<Output = Result<RootFolderSummary, RepositoryError>> + Send;

    fn list_published_documents(
        &self,
        ctx: &VerifiedActorContext,
        query: PublishedQuery,
    ) -> impl Future<Output = Result<Page<PublishedDocumentSummary>, RepositoryError>> + Send;

    fn list_authoring_documents(
        &self,
        ctx: &VerifiedActorContext,
        query: AuthoringQuery,
    ) -> impl Future<Output = Result<Page<AuthoringDocumentSummary>, RepositoryError>> + Send;

    fn list_history_documents(
        &self,
        ctx: &VerifiedActorContext,
        query: HistoryQuery,
    ) -> impl Future<Output = Result<Page<HistoryDocumentSummary>, RepositoryError>> + Send;

    fn list_child_folders(
        &self,
        ctx: &VerifiedActorContext,
        query: FolderPageQuery,
    ) -> impl Future<Output = Result<Page<FolderSummary>, RepositoryError>> + Send;
}

pub struct DocumentQueryService<R> {
    repository: Arc<R>,
}

impl<R: DocumentQueryRepository> DocumentQueryService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn get_root_folder(
        &self,
        ctx: &VerifiedActorContext,
    ) -> Result<RootFolderSummary, ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .get_root_folder(ctx)
            .await
            .map_err(Into::into)
    }

    pub async fn list_published_documents(
        &self,
        ctx: &VerifiedActorContext,
        mut query: PublishedQuery,
    ) -> Result<Page<PublishedDocumentSummary>, ApplicationError> {
        ctx.ensure_current()?;
        query.filter = query.filter.normalized()?;
        validate_page_size(query.page_size.unwrap_or(50))?;
        self.repository
            .list_published_documents(ctx, query)
            .await
            .map_err(Into::into)
    }

    pub async fn list_authoring_documents(
        &self,
        ctx: &VerifiedActorContext,
        mut query: AuthoringQuery,
    ) -> Result<Page<AuthoringDocumentSummary>, ApplicationError> {
        ctx.ensure_current()?;
        if query.sort == DocumentSort::PublishedAtDesc {
            return Err(ApplicationError::Validation(
                "published-at sort is only valid for published documents".into(),
            ));
        }
        query.filter = query.filter.normalized()?;
        validate_page_size(query.page_size.unwrap_or(50))?;
        self.repository
            .list_authoring_documents(ctx, query)
            .await
            .map_err(Into::into)
    }

    pub async fn list_history_documents(
        &self,
        ctx: &VerifiedActorContext,
        mut query: HistoryQuery,
    ) -> Result<Page<HistoryDocumentSummary>, ApplicationError> {
        ctx.ensure_current()?;
        if query.sort == DocumentSort::PublishedAtDesc {
            return Err(ApplicationError::Validation(
                "published-at sort is only valid for published documents".into(),
            ));
        }
        query.filter = query.filter.normalized()?;
        validate_page_size(query.page_size.unwrap_or(50))?;
        self.repository
            .list_history_documents(ctx, query)
            .await
            .map_err(Into::into)
    }

    pub async fn list_child_folders(
        &self,
        ctx: &VerifiedActorContext,
        query: FolderPageQuery,
    ) -> Result<Page<FolderSummary>, ApplicationError> {
        ctx.ensure_current()?;
        validate_page_size(query.page_size.unwrap_or(50))?;
        self.repository
            .list_child_folders(ctx, query)
            .await
            .map_err(Into::into)
    }
}

/// Checks the current Document AccessPolicy only. Callers must separately verify
/// that a requested version is the current published version and has not been
/// removed from normal visibility by T10. Historical content uses the separate
/// Document history path, including both Read and ReadHistory (and Write for
/// remaining Working content); this check alone cannot grant history visibility.
#[allow(async_fn_in_trait)]
pub trait DocumentAccessCheckRepository: Send + Sync {
    async fn check_document_access(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
        required: &[Action],
    ) -> Result<(), RepositoryError>;
}

pub struct DocumentAccessCheckService<R> {
    repository: Arc<R>,
}

impl<R: DocumentAccessCheckRepository> DocumentAccessCheckService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn check(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
        required: &[Action],
    ) -> Result<(), ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .check_document_access(ctx, document_id, required)
            .await
            .map_err(Into::into)
    }
}
