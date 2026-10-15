use std::future::Future;
use std::sync::Arc;

use document_domain::{DocumentId, DocumentVersionId, FileId};

use crate::{ApplicationError, CreateDocumentResult, RepositoryError, VerifiedActorContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOutcomeProbe {
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub file_id: FileId,
    pub file_ids: Option<Vec<FileId>>,
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
        if let Some(ids) = &probe.file_ids {
            let unique: std::collections::HashSet<_> = ids.iter().collect();
            if ids.is_empty()
                || ids.len() > 63
                || unique.len() != ids.len()
                || ids[0] != probe.file_id
            {
                return Err(ApplicationError::Validation(
                    "invalid initial recovery file ids".into(),
                ));
            }
        }
        if self
            .repository
            .recover_initial_create(ctx, probe.clone())
            .await?
        {
            Ok(Some(
                CreateDocumentResult::new(
                    probe.document_id,
                    probe.document_version_id,
                    probe.file_id,
                )
                .with_file_ids(probe.file_ids),
            ))
        } else {
            Ok(None)
        }
    }
}
