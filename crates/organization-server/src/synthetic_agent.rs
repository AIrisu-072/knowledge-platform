//! Fixed simulated output in owned, cancellable one-shot tasks. No model or MCP wire.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::{sync::Mutex as AsyncMutex, task::JoinHandle};
use uuid::Uuid;
use work_application::{AgentDispatchPort, WorkRepository};
use work_domain::{
    AgentDispatchContext, AgentExecutionStatus, AgentFailureCode, AgentFindingOutput,
    MAX_AGENT_EXECUTIONS, VerifiedActor, WorkError,
};

#[derive(Default)]
struct OwnedTasks {
    closed: bool,
    tasks: BTreeMap<Uuid, JoinHandle<()>>,
}

/// The process-fixed owner is supplied only by the trusted composition root.
/// Repository transactions, not this in-memory task map, fence replay and output.
pub struct OwnedAgentDispatcher {
    repository: Arc<dyn WorkRepository>,
    actor: VerifiedActor,
    owned: Mutex<OwnedTasks>,
    shutdown_gate: AsyncMutex<()>,
}
impl OwnedAgentDispatcher {
    pub fn new(repository: Arc<dyn WorkRepository>, actor: VerifiedActor) -> Self {
        Self {
            repository,
            actor,
            owned: Mutex::new(OwnedTasks::default()),
            shutdown_gate: AsyncMutex::new(()),
        }
    }

    /// Stop admission, abort and join every owned task, then preserve any remaining
    /// nonterminal durable execution as unknown before the caller closes its pool.
    pub async fn shutdown(&self) -> Result<(), WorkError> {
        let _gate = self.shutdown_gate.lock().await;
        let tasks = {
            let mut owned = self.owned.lock().unwrap_or_else(|error| error.into_inner());
            owned.closed = true;
            std::mem::take(&mut owned.tasks)
        };
        for task in tasks.values() {
            task.abort();
        }
        for task in tasks.into_values() {
            let _ = task.await;
        }
        // Repeat this actor-only idempotent sweep after HTTP drain: a request
        // already admitted before shutdown may have committed a queued receipt
        // after the first sweep. Terminal records remain unchanged in Work.
        self.repository
            .interrupt_agent_executions(self.actor)
            .await?;
        Ok(())
    }
}
impl AgentDispatchPort for OwnedAgentDispatcher {
    fn dispatch(&self, actor: VerifiedActor, id: Uuid) -> Result<(), WorkError> {
        if actor != self.actor {
            return Err(WorkError::Forbidden);
        }
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|_| WorkError::DependencyUnavailable)?;
        let mut owned = self
            .owned
            .lock()
            .map_err(|_| WorkError::DependencyUnavailable)?;
        if owned.closed {
            return Err(WorkError::DependencyUnavailable);
        }
        owned.tasks.retain(|_, task| !task.is_finished());
        if owned.tasks.contains_key(&id) {
            return Ok(());
        }
        if owned.tasks.len() >= MAX_AGENT_EXECUTIONS {
            return Err(WorkError::DependencyUnavailable);
        }
        let repository = self.repository.clone();
        owned.tasks.insert(
            id,
            runtime.spawn(async move {
                let result = execute_once(repository.clone(), actor, id).await;
                if let Err(error) = result {
                    // Record uncertainty for this execution before the owned task
                    // ends. Work conditionally updates only nonterminal state,
                    // preserving a succeeded commit and never re-executing it.
                    let _ = repository
                        .fail_agent_execution(actor, id, failure_code(error))
                        .await;
                }
            }),
        );
        Ok(())
    }
}
impl Drop for OwnedAgentDispatcher {
    fn drop(&mut self) {
        // Unexpected owner drop must not detach provider work. Normal shutdown
        // additionally joins the tasks and persists unknown outcomes above.
        let owned = self
            .owned
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        for task in owned.tasks.values() {
            task.abort();
        }
    }
}

async fn execute_once(
    repository: Arc<dyn WorkRepository>,
    actor: VerifiedActor,
    id: Uuid,
) -> Result<(), WorkError> {
    let Some(started) = repository.start_agent_execution(actor, id).await? else {
        return Ok(());
    };
    started.validate_scope()?;
    if started.execution.id != id || started.execution.requested_by != actor {
        return Err(WorkError::Forbidden);
    }
    let context = repository.build_agent_context(actor, id).await?;
    if context != started || context.execution.status != AgentExecutionStatus::Running {
        return Err(WorkError::WorkContextStale);
    }
    let output = simulated_candidate(&context)?;
    // Finish reauthorizes each selected source outside Work locks, then checks
    // this exact context under its transaction before storing the one Finding.
    repository.finish_agent_execution(context, output).await?;
    Ok(())
}

fn simulated_candidate(context: &AgentDispatchContext) -> Result<AgentFindingOutput, WorkError> {
    context.validate_scope()?;
    Ok(AgentFindingOutput {
        summary: "合成実行による確認用候補を作成しました（本文分析なし・実LLM/MCP通信なし）。".into(),
        claim: "選択した根拠参照に対応する確認用候補です。原本内容の確認と採否判断は人間が行ってください。".into(),
        uncertainty: vec![
            "本文分析なし。原本の内容・正確性・業務上の適合性は未検証です。".into(),
            "実LLM/MCP通信なし。既存Document認可サービスで参照権限を確認した模擬結果です。".into(),
        ],
    })
}
fn failure_code(error: WorkError) -> AgentFailureCode {
    match error {
        WorkError::CommitOutcomeUnknown => AgentFailureCode::CommitOutcomeUnknown,
        WorkError::EvidenceNotFound | WorkError::Forbidden => AgentFailureCode::ProviderDenied,
        WorkError::WorkContextStale
        | WorkError::WorkItemNotFound
        | WorkError::RevisionConflict
        | WorkError::WorkAssignmentConflict => AgentFailureCode::ContextStale,
        WorkError::ValidationFailed | WorkError::IntegrityViolation => {
            AgentFailureCode::InvalidOutput
        }
        _ => AgentFailureCode::DependencyUnavailable,
    }
}
