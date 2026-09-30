use std::future::Future;
use std::sync::Arc;

use document_domain::{DocumentId, DocumentVersionId, FileId};

use crate::{ApplicationError, CreateDocumentResult, RepositoryError, VerifiedActorContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateOutcomeProbe {
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub file_id: FileId,
}

pub trait CreateOutcomeRepository: Send + Sync {
    fn recover_initial_create(
        &self,
        ctx: &VerifiedActorContext,
        probe: CreateOutcomeProbe,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
}

pub struct CreateOutcomeRecoveryService<R> {
    repository: Arc<R>,
}

impl<R: CreateOutcomeRepository> CreateOutcomeRecoveryService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn recover(
        &self,
        ctx: &VerifiedActorContext,
        probe: CreateOutcomeProbe,
    ) -> Result<Option<CreateDocumentResult>, ApplicationError> {
        ctx.ensure_current()?;
        if self.repository.recover_initial_create(ctx, probe).await? {
            Ok(Some(CreateDocumentResult::new(
                probe.document_id,
                probe.document_version_id,
                probe.file_id,
            )))
        } else {
            Ok(None)
        }
    }
}
