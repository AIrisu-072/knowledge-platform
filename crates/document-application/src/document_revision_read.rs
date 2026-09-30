use std::future::Future;
use std::sync::Arc;

use document_domain::{DocumentId, PrincipalRef};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ApplicationError, Page, RepositoryError, VerifiedActorContext, validate_page_size};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRevisionPageQuery {
    pub document_id: DocumentId,
    pub page_size: Option<u16>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentRevisionDetailQuery {
    pub document_id: DocumentId,
    pub revision_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRevisionSummary {
    pub revision_id: Uuid,
    pub document_version_id: document_domain::DocumentVersionId,
    pub major_no: i64,
    pub minor_no: i64,
    pub metadata_snapshot_status: String,
    pub source_kind: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentRevisionDetail {
    pub summary: DocumentRevisionSummary,
    pub metadata_snapshot: Option<Value>,
    pub actor: Option<PrincipalRef>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionComparisonAuditRequest {
    pub document_id: DocumentId,
    pub base_revision_id: Uuid,
    pub target_revision_id: Uuid,
    pub content_comparison_status: String,
    pub content_result_digest: Option<[u8; 32]>,
    pub content_audit_event_id: Option<Uuid>,
    pub metadata_comparison_status: String,
    pub base_metadata_snapshot_digest: Option<[u8; 32]>,
    pub target_metadata_snapshot_digest: Option<[u8; 32]>,
}

pub trait DocumentRevisionReadRepository: Send + Sync {
    fn list_document_revisions(
        &self,
        ctx: &VerifiedActorContext,
        query: DocumentRevisionPageQuery,
    ) -> impl Future<Output = Result<Page<DocumentRevisionSummary>, RepositoryError>> + Send;

    fn get_document_revision(
        &self,
        ctx: &VerifiedActorContext,
        query: DocumentRevisionDetailQuery,
    ) -> impl Future<Output = Result<DocumentRevisionDetail, RepositoryError>> + Send;

    fn authorize_and_audit_revision_comparison(
        &self,
        ctx: &VerifiedActorContext,
        request: RevisionComparisonAuditRequest,
    ) -> impl Future<Output = Result<Uuid, RepositoryError>> + Send;
}

pub struct DocumentRevisionReadService<R> {
    repository: Arc<R>,
}

impl<R: DocumentRevisionReadRepository> DocumentRevisionReadService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn list_document_revisions(
        &self,
        ctx: &VerifiedActorContext,
        query: DocumentRevisionPageQuery,
    ) -> Result<Page<DocumentRevisionSummary>, ApplicationError> {
        ctx.ensure_current()?;
        validate_page_size(query.page_size.unwrap_or(50))?;
        self.repository
            .list_document_revisions(ctx, query)
            .await
            .map_err(Into::into)
    }

    pub async fn get_document_revision(
        &self,
        ctx: &VerifiedActorContext,
        query: DocumentRevisionDetailQuery,
    ) -> Result<DocumentRevisionDetail, ApplicationError> {
        ctx.ensure_current()?;
        self.repository
            .get_document_revision(ctx, query)
            .await
            .map_err(Into::into)
    }
}
