use std::sync::Arc;

use crate::{
    ApplicationError, ManagementCommand, ManagementMutationResult, ManagementRepository,
    ManagementResult, RepositoryError, VerifiedActorContext, canonical_command_bytes,
};

pub struct DocumentManagementService<R> {
    repository: Arc<R>,
}

impl<R: ManagementRepository> DocumentManagementService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn update_document_metadata(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementMutationResult, ApplicationError> {
        if !matches!(command, ManagementCommand::UpdateDocumentMetadata { .. }) {
            return Err(ApplicationError::Validation(
                "update_document_metadata requires a metadata command".into(),
            ));
        }
        canonical_command_bytes(ctx, &command)?;
        let operation_id = command.operation_id();
        match self.repository.execute(ctx, command).await {
            Ok(ManagementResult::MetadataUpdate(result)) => Ok(result),
            Ok(_) => Err(ApplicationError::IntegrityViolation),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::ManagementCommitOutcomeUnknown { operation_id })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn move_document(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementMutationResult, ApplicationError> {
        if !matches!(command, ManagementCommand::MoveDocument { .. }) {
            return Err(ApplicationError::Validation(
                "move_document requires a document move command".into(),
            ));
        }
        canonical_command_bytes(ctx, &command)?;
        let operation_id = command.operation_id();
        match self.repository.execute(ctx, command).await {
            Ok(ManagementResult::DocumentMove(result)) => Ok(result),
            Ok(_) => Err(ApplicationError::IntegrityViolation),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::ManagementCommitOutcomeUnknown { operation_id })
            }
            Err(error) => Err(error.into()),
        }
    }
}
