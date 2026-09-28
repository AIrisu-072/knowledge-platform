use std::sync::Arc;

use document_domain::{DocumentId, DocumentVersionId, PrincipalRef};
use time::OffsetDateTime;

use crate::{ApplicationError, InvocationKind, RepositoryError, VerifiedActorContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkVersionRead {
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadStateResult {
    pub principal: PrincipalRef,
    pub document_version_id: DocumentVersionId,
    pub first_read_at: OffsetDateTime,
    pub inserted: bool,
}

#[allow(async_fn_in_trait)]
pub trait ReadStateRepository: Send + Sync {
    async fn mark_version_read(
        &self,
        ctx: &VerifiedActorContext,
        command: MarkVersionRead,
    ) -> Result<ReadStateResult, RepositoryError>;
}

pub struct ReadStateService<R> {
    repository: Arc<R>,
}

impl<R: ReadStateRepository> ReadStateService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn mark_version_read(
        &self,
        ctx: &VerifiedActorContext,
        command: MarkVersionRead,
    ) -> Result<ReadStateResult, ApplicationError> {
        ctx.ensure_current()?;
        if ctx.invocation_kind() != InvocationKind::HumanInteractive {
            return Err(ApplicationError::Forbidden);
        }
        self.repository
            .mark_version_read(ctx, command)
            .await
            .map_err(|error| match error {
                RepositoryError::CommitOutcomeUnknown => {
                    ApplicationError::ReadStateCommitOutcomeUnknown {
                        document_version_id: command.document_version_id,
                    }
                }
                other => other.into(),
            })
    }
}
