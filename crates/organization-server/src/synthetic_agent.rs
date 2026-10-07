//! Owned, cancellable one-shot Agent tasks behind the executor port. The only
//! executor composed today is the fixed simulated one: no model or MCP wire.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::Mutex as AsyncMutex, task::JoinHandle};
use uuid::Uuid;
use work_application::{
    AGENT_EXECUTOR_BUDGET, AgentDispatchPort, AgentExecutorPort, WorkFuture, WorkRepository,
};
use work_domain::{
    AgentDispatchContext, AgentExecutionStatus, AgentFailureCode, AgentOutput,
    GeneratedArtifactCandidate, MAX_AGENT_EXECUTIONS, ProposedActionCandidate,
    SuggestedActionCandidate, VerifiedActor, WorkError,
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
    executor: Arc<dyn AgentExecutorPort>,
    budget: Duration,
    actor: VerifiedActor,
    owned: Mutex<OwnedTasks>,
    shutdown_gate: AsyncMutex<()>,
}
impl OwnedAgentDispatcher {
    /// The fixed simulated executor.
    pub fn new(repository: Arc<dyn WorkRepository>, actor: VerifiedActor) -> Self {
        Self::with_executor(repository, actor, Arc::new(SyntheticAgentExecutor))
    }
    pub fn with_executor(
        repository: Arc<dyn WorkRepository>,
        actor: VerifiedActor,
        executor: Arc<dyn AgentExecutorPort>,
    ) -> Self {
        Self {
            repository,
            executor,
            budget: AGENT_EXECUTOR_BUDGET,
            actor,
            owned: Mutex::new(OwnedTasks::default()),
            shutdown_gate: AsyncMutex::new(()),
        }
    }
    /// A shorter executor bound (tests); never longer than the fixed budget.
    pub fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget.min(AGENT_EXECUTOR_BUDGET);
        self
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
        let executor = self.executor.clone();
        let budget = self.budget;
        owned.tasks.insert(
            id,
            runtime.spawn(async move {
                let result = execute_once(repository.clone(), executor, budget, actor, id).await;
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
    executor: Arc<dyn AgentExecutorPort>,
    budget: Duration,
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
    // The executor runs outside every Work lock with a fixed bound. It is
    // read-only, so expiry leaves no remote side effect to reconcile. Any
    // executor error is only "unavailable": commit uncertainty, denial and
    // staleness are Work's own verdicts, never an adapter's claim.
    let output = tokio::time::timeout(budget, executor.execute(context.clone(), budget))
        .await
        .map_err(|_| WorkError::DependencyUnavailable)?
        .map_err(|_| WorkError::DependencyUnavailable)?;
    // Finish validates the proposal, reauthorizes each selected source outside
    // Work locks, then checks this exact context under its transaction.
    repository.finish_agent_execution(context, output).await?;
    Ok(())
}

/// Fixed rule output: references only, never source bodies, locations or the
/// requester's purpose. Labeled simulated by the Work record itself.
pub struct SyntheticAgentExecutor;
impl AgentExecutorPort for SyntheticAgentExecutor {
    fn execute(
        &self,
        context: AgentDispatchContext,
        _remaining: Duration,
    ) -> WorkFuture<'_, AgentOutput> {
        Box::pin(async move { simulated_output(&context) })
    }
}
fn simulated_output(context: &AgentDispatchContext) -> Result<AgentOutput, WorkError> {
    context.validate_scope()?;
    let refs = &context.execution.evidence_revision_refs;
    let mut output = AgentOutput::referenced_finding(
        context,
        "合成実行による確認用候補・下書き・提案を作成しました（本文分析なし・実LLM/MCP通信なし）。",
        "選択した根拠参照に対応する確認用候補です。原本内容の確認と採否判断は人間が行ってください。",
        vec![
            "本文分析なし。原本の内容・正確性・業務上の適合性は未検証です。".into(),
            "実LLM/MCP通信なし。既存Document認可サービスで参照権限を確認した模擬結果です。".into(),
        ],
    );
    let lines: String = refs
        .iter()
        .map(|r| {
            format!(
                "- 根拠 {}（版 {}）：該当箇所を原本で確認する\n",
                r.id, r.revision
            )
        })
        .collect();
    output.generated_artifacts = vec![GeneratedArtifactCandidate {
        title: "確認メモの下書き（合成）".into(),
        text: format!(
            "【合成Agentの下書き】確認メモ\n{lines}\n原本本文は分析していません。内容・正確性・業務上の適合性は担当者が確認し、採否を判断してください。"
        ),
        source_revision_refs: refs.clone(),
    }];
    output.suggested_actions = vec![
        SuggestedActionCandidate {
            action: ProposedActionCandidate::ReviewFinding,
            rationale: "候補の根拠を原本で確認し、採否を人間判断として記録してください。".into(),
        },
        SuggestedActionCandidate {
            action: ProposedActionCandidate::UseGeneratedArtifact(0),
            rationale:
                "下書きを作業文案の出発点として使えます。保存するまで作業には反映されません。"
                    .into(),
        },
    ];
    Ok(output)
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
