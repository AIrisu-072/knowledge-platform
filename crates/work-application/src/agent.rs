use crate::WorkFuture;
use uuid::Uuid;
use work_domain::{AgentDispatchContext, MutationResult, RevisionRef, VerifiedActor, WorkError};
/// The fixed provider adapter must independently verify requester and actual
/// provider identity/current rights, bound every call, and retain no credentials.
/// Success certifies the exact immutable binding in this trusted context.
pub trait AgentSourcePort: Send + Sync {
    fn authorize(
        &self,
        context: AgentDispatchContext,
        reference: RevisionRef,
        remaining: std::time::Duration,
    ) -> WorkFuture<'_, ()>;
}
/// The composition root owns these one-shot tasks and drains them on shutdown.
/// Admission never performs provider work inline and never implies completion.
pub trait AgentDispatchPort: Send + Sync {
    fn dispatch(&self, actor: VerifiedActor, id: Uuid) -> Result<(), WorkError>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionAcceptance {
    pub outcome: MutationResult,
    pub dispatch: bool,
}
