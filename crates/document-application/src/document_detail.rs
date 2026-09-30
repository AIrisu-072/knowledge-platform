use std::sync::Arc;

use document_domain::DocumentId;

use crate::{
    ApplicationError, AuthoringDocumentSummary, AuthoringQuery, DocumentListFilter,
    DocumentQueryRepository, DocumentQueryService, PublishedDocumentSummary, PublishedQuery,
    VerifiedActorContext,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentDetailPurpose {
    Published,
    Authoring,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DocumentDetailRead {
    Published(PublishedDocumentSummary),
    Authoring(AuthoringDocumentSummary),
}

pub struct DocumentDetailReadService<R> {
    repository: Arc<R>,
}

impl<R: DocumentQueryRepository> DocumentDetailReadService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn read(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
        purpose: DocumentDetailPurpose,
    ) -> Result<DocumentDetailRead, ApplicationError> {
        let filter = DocumentListFilter {
            exact_document_id: Some(document_id),
            ..DocumentListFilter::default()
        };
        let service = DocumentQueryService::new(self.repository.clone());
        match purpose {
            DocumentDetailPurpose::Published => service
                .list_published_documents(
                    ctx,
                    PublishedQuery {
                        filter,
                        page_size: Some(1),
                        ..PublishedQuery::default()
                    },
                )
                .await?
                .items
                .into_iter()
                .next()
                .map(DocumentDetailRead::Published)
                .ok_or(ApplicationError::DocumentNotFound),
            DocumentDetailPurpose::Authoring => service
                .list_authoring_documents(
                    ctx,
                    AuthoringQuery {
                        filter,
                        page_size: Some(1),
                        ..AuthoringQuery::default()
                    },
                )
                .await?
                .items
                .into_iter()
                .next()
                .map(DocumentDetailRead::Authoring)
                .ok_or(ApplicationError::DocumentNotFound),
        }
    }
}
