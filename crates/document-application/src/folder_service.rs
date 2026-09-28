use std::sync::Arc;

use document_domain::normalize_folder_name;

use crate::{
    ApplicationError, ManagementCommand, ManagementMutationResult, ManagementRepository,
    ManagementResult, RepositoryError, VerifiedActorContext, canonical_command_bytes,
};

pub struct FolderService<R> {
    repository: Arc<R>,
}

impl<R: ManagementRepository> FolderService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn create_folder(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementMutationResult, ApplicationError> {
        let ManagementCommand::CreateFolder { name, .. } = &command else {
            return Err(ApplicationError::Validation(
                "create_folder requires a create command".into(),
            ));
        };
        normalize_folder_name(name)?;
        self.execute(ctx, command).await
    }

    pub async fn rename_folder(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementMutationResult, ApplicationError> {
        let ManagementCommand::RenameFolder { name, .. } = &command else {
            return Err(ApplicationError::Validation(
                "rename_folder requires a rename command".into(),
            ));
        };
        normalize_folder_name(name)?;
        self.execute(ctx, command).await
    }

    async fn execute(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementMutationResult, ApplicationError> {
        canonical_command_bytes(ctx, &command)?;
        let operation_id = command.operation_id();
        match self.repository.execute(ctx, command).await {
            Ok(ManagementResult::FolderMutation(result)) => Ok(result),
            Ok(_) => Err(ApplicationError::IntegrityViolation),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::ManagementCommitOutcomeUnknown { operation_id })
            }
            Err(error) => Err(error.into()),
        }
    }
}
