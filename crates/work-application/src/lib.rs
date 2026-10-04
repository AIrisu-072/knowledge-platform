#![forbid(unsafe_code)]
//! Infrastructure-free Work application ports and canonical operation digest.
use std::{future::Future, pin::Pin};
use uuid::Uuid;
use work_domain::{
    Command, HandoffSnapshot, MutationResult, TaskDetail, TaskSummary, TaskView, VerifiedActor,
    WorkError, WorkingArtifact,
};

pub type WorkFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, WorkError>> + Send + 'a>>;
pub trait WorkRepository: Send + Sync {
    fn list_tasks(&self, actor: VerifiedActor, view: TaskView) -> WorkFuture<'_, Vec<TaskSummary>>;
    fn task(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, TaskDetail>;
    fn artifact(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, WorkingArtifact>;
    fn snapshot(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, HandoffSnapshot>;
    fn execute(&self, actor: VerifiedActor, command: Command) -> WorkFuture<'_, MutationResult>;
    fn recover(&self, actor: VerifiedActor, operation_id: Uuid) -> WorkFuture<'_, MutationResult>;
}
/// Digest includes actual actor, target, OCC revision, responsibility and typed payload.
pub fn command_digest(actor: VerifiedActor, command: &Command) -> Result<Vec<u8>, WorkError> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&(actor, command)).map_err(|_| WorkError::IntegrityViolation)?;
    Ok(Sha256::digest(bytes).to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use work_domain::{CommandContext, SALES_TASK_ID};
    #[test]
    fn digest_binds_actor_payload_target_and_revision_but_is_stable_for_exact_retry() {
        let command = Command::Claim {
            task_id: SALES_TASK_ID,
            context: CommandContext {
                operation_id: Uuid::now_v7(),
                expected_revision: 0,
                acting_assignment_id: Uuid::now_v7(),
            },
        };
        let digest = command_digest(VerifiedActor::Sales01, &command).unwrap();
        assert_eq!(
            digest,
            command_digest(VerifiedActor::Sales01, &command.clone()).unwrap()
        );
        assert_ne!(
            digest,
            command_digest(VerifiedActor::Office01, &command).unwrap()
        );
        let mut changed = command.clone();
        if let Command::Claim { context, .. } = &mut changed {
            context.expected_revision = 1;
        }
        assert_ne!(
            digest,
            command_digest(VerifiedActor::Sales01, &changed).unwrap()
        );
        if let Command::Claim { task_id, .. } = &mut changed {
            *task_id = Uuid::now_v7();
        }
        assert_ne!(
            command_digest(VerifiedActor::Sales01, &command).unwrap(),
            command_digest(VerifiedActor::Sales01, &changed).unwrap()
        );
    }
}
