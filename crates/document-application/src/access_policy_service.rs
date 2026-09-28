use std::sync::Arc;

use crate::{
    ApplicationError, ManagementCommand, ManagementMutationResult, ManagementRepository,
    ManagementResult, VerifiedActorContext, canonical_command_bytes,
};

pub struct AccessPolicyService<R> {
    repository: Arc<R>,
}

impl<R: ManagementRepository> AccessPolicyService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn set_access_policy(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementMutationResult, ApplicationError> {
        if !matches!(command, ManagementCommand::SetAccessPolicy { .. }) {
            return Err(ApplicationError::Validation(
                "set_access_policy requires a policy command".into(),
            ));
        }
        canonical_command_bytes(ctx, &command)?;
        match self.repository.execute(ctx, command).await? {
            ManagementResult::PolicyMutation(result) => Ok(result),
            _ => Err(ApplicationError::IntegrityViolation),
        }
    }
}
