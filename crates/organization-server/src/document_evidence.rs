//! Reference-only source checks through Document's existing current authority.
//! These separate provider reads do not form an atomic transaction with Work.
use crate::{OrganizationProfile, SyntheticIdentityAdapter};
use document_application::{
    ApplicationError, DocumentHistoryRepository, DocumentHistoryService,
    DocumentRevisionDetailQuery, DocumentRevisionReadRepository, DocumentRevisionReadService,
    VersionPurpose, VersionRequest,
};
use document_domain::{DocumentId, DocumentVersionId};
use std::{sync::Arc, time::Duration};
use work_application::{EvidenceSourcePort, EvidenceSourcePurpose, WorkFuture};
use work_domain::{EvidenceSource, VerifiedActor, WorkError};

pub struct DocumentEvidenceSource<R> {
    repository: Arc<R>,
}

impl<R> DocumentEvidenceSource<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

impl<R: DocumentRevisionReadRepository + DocumentHistoryRepository> EvidenceSourcePort
    for DocumentEvidenceSource<R>
{
    fn authorize(
        &self,
        actor: VerifiedActor,
        source: EvidenceSource,
        purpose: EvidenceSourcePurpose,
    ) -> WorkFuture<'_, ()> {
        Box::pin(async move {
            if source.source_ref.provider_id != "document"
                || source.authoritative_locator.kind != "contentItem"
            {
                return Err(match purpose {
                    EvidenceSourcePurpose::RegisterPublished => WorkError::ValidationFailed,
                    EvidenceSourcePurpose::ReadHistory => WorkError::EvidenceNotFound,
                });
            }
            // Use the verified caller, never a configured privileged service actor.
            let profile = match actor {
                VerifiedActor::Sales01 => OrganizationProfile::Sales,
                VerifiedActor::Office01 => OrganizationProfile::Office,
            };
            let ctx = SyntheticIdentityAdapter::new(profile)
                .current_context()
                .map_err(|_| WorkError::DependencyUnavailable)?;
            tokio::time::timeout(Duration::from_secs(5), async {
                let revision = DocumentRevisionReadService::new(self.repository.clone())
                    .get_document_revision(
                        &ctx,
                        DocumentRevisionDetailQuery {
                            document_id: DocumentId::from_uuid(source.source_ref.resource_id),
                            revision_id: source.source_ref.revision_id,
                        },
                    )
                    .await
                    .map_err(source_error)?;
                if revision.summary.revision_id != source.source_ref.revision_id
                    || revision.summary.document_version_id.as_uuid()
                        != source.source_ref.version_id
                {
                    return Err(WorkError::EvidenceNotFound);
                }
                let files = DocumentHistoryService::new(self.repository.clone())
                    .list_version_files(
                        &ctx,
                        VersionRequest {
                            document_id: DocumentId::from_uuid(source.source_ref.resource_id),
                            document_version_id: DocumentVersionId::from_uuid(
                                source.source_ref.version_id,
                            ),
                            purpose: match purpose {
                                EvidenceSourcePurpose::RegisterPublished => {
                                    VersionPurpose::Published
                                }
                                EvidenceSourcePurpose::ReadHistory => VersionPurpose::History,
                            },
                        },
                    )
                    .await
                    .map_err(source_error)?;
                if !files.iter().any(|file| {
                    file.content_item_id == source.authoritative_locator.content_item_id
                        && file.representation_id == source.authoritative_locator.representation_id
                        && file.role == "AUTHORITATIVE"
                }) {
                    return Err(WorkError::EvidenceNotFound);
                }
                // No metadata, fragment, comparison verdict, or inferred truth leaves this port.
                Ok(())
            })
            .await
            .map_err(|_| WorkError::DependencyUnavailable)?
        })
    }
}

fn source_error(error: ApplicationError) -> WorkError {
    match error {
        ApplicationError::Forbidden
        | ApplicationError::FolderNotFound
        | ApplicationError::DocumentNotFound
        | ApplicationError::DocumentRevisionNotFound
        | ApplicationError::DocumentVersionNotFound
        | ApplicationError::FileObjectNotFound
        | ApplicationError::StaleVersion => WorkError::EvidenceNotFound,
        _ => WorkError::DependencyUnavailable,
    }
}
