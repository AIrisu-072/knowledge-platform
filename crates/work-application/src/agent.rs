use crate::WorkFuture;
use uuid::Uuid;
use work_domain::{
    AgentDispatchContext, AgentOutput, MutationResult, RevisionRef, VerifiedActor, WorkError,
};
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
/// Upper bound for one executor call. Executors are read-only, so expiry
/// records a failed execution without remote side effects to reconcile.
pub const AGENT_EXECUTOR_BUDGET: std::time::Duration = std::time::Duration::from_secs(20);
/// The executor behind an Agent execution: the fixed synthetic one today, a
/// reviewed real Agent/MCP adapter later. It receives only the Work-built,
/// already-authorized context and returns a proposal; it never writes Work or
/// Document state, holds credentials, or sees physical paths. Work validates
/// the output, rechecks every selected source and fences late output by the
/// execution ID and context revision before recording anything.
pub trait AgentExecutorPort: Send + Sync {
    fn execute(
        &self,
        context: AgentDispatchContext,
        remaining: std::time::Duration,
    ) -> WorkFuture<'_, AgentOutput>;
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
