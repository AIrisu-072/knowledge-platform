//! Owned-task tests exercise no database, listeners, storage or external execution.
use organization_server::OwnedAgentDispatcher;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
use uuid::Uuid;
use work_application::{AgentDispatchPort, WorkFuture, WorkRepository};
use work_domain::*;

const NOW: &str = "2026-10-04T13:00:00Z";
const ACTOR: VerifiedActor = VerifiedActor::Sales01;
fn fixture_context() -> (Workflow, AgentDispatchContext) {
    let mut workflow = Workflow::synthetic(Some(Uuid::from_u128(1)));
    let context = |revision| CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: SALES_ASSIGNMENT_ID,
    };
    workflow
        .apply(
            ACTOR,
            &Command::RegisterEvidence {
                task_id: SALES_TASK_ID,
                context: context(workflow.source.revision),
                expected_attempt_id: SALES_ATTEMPT_ID,
                source: EvidenceSource {
                    source_ref: SourceRef {
                        provider_id: "document".into(),
                        resource_id: Uuid::from_u128(1),
                        revision_id: Uuid::from_u128(2),
                        version_id: Uuid::from_u128(3),
                    },
                    authoritative_locator: AuthoritativeLocator {
                        kind: "contentItem".into(),
                        content_item_id: Uuid::from_u128(4),
                        representation_id: Uuid::from_u128(5),
                    },
                },
                relevant_location: "PRIVATE SOURCE LOCATION NEVER COPIED".into(),
            },
            NOW,
        )
        .unwrap();
    let command = Command::RequestAgentExecution {
        task_id: SALES_TASK_ID,
        context: context(workflow.source.revision),
        expected_attempt_id: SALES_ATTEMPT_ID,
        purpose: "PRIVATE PURPOSE NEVER ECHOED".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: workflow.evidence[0].id,
            revision: 1,
        }],
    };
    let execution = match workflow.apply(ACTOR, &command, NOW).unwrap() {
        MutationResult::AgentExecutionRequested { execution, .. } => execution,
        _ => unreachable!(),
    };
    let context = workflow
        .clone()
        .start_agent_execution(ACTOR, execution.id, NOW)
        .unwrap()
        .unwrap();
    (workflow, context)
}

struct TestWork {
    context: AgentDispatchContext,
    workflow: Mutex<Workflow>,
    blocked: AtomicBool,
    cancelled: AtomicBool,
    already_consumed: AtomicBool,
    start_unknown: bool,
    finish_unknown: bool,
    finish_committed: bool,
    starts: AtomicUsize,
    builds: AtomicUsize,
    active_calls: AtomicUsize,
    interruptions: AtomicUsize,
    output: Mutex<Vec<AgentOutput>>,
    failures: Mutex<Vec<AgentFailureCode>>,
    entered: Notify,
    release: Notify,
    completed: Notify,
}
impl TestWork {
    fn new() -> Self {
        let (workflow, context) = fixture_context();
        Self {
            context,
            workflow: Mutex::new(workflow),
            blocked: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            already_consumed: AtomicBool::new(false),
            start_unknown: false,
            finish_unknown: false,
            finish_committed: false,
            starts: AtomicUsize::new(0),
            builds: AtomicUsize::new(0),
            active_calls: AtomicUsize::new(0),
            interruptions: AtomicUsize::new(0),
            output: Mutex::new(vec![]),
            failures: Mutex::new(vec![]),
            entered: Notify::new(),
            release: Notify::new(),
            completed: Notify::new(),
        }
    }
    async fn wait(&self, event: &Notify) {
        tokio::time::timeout(std::time::Duration::from_secs(2), event.notified())
            .await
            .expect("bounded one-shot task must settle");
    }
}
struct ActiveCall<'a>(&'a AtomicUsize);
impl Drop for ActiveCall<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl WorkRepository for TestWork {
    fn start_agent_execution(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, Option<AgentDispatchContext>> {
        Box::pin(async move {
            assert_eq!(actor, ACTOR);
            assert_eq!(id, self.context.execution.id);
            self.starts.fetch_add(1, Ordering::SeqCst);
            if self.start_unknown {
                self.completed.notify_one();
                return Err(WorkError::CommitOutcomeUnknown);
            }
            if self.already_consumed.swap(true, Ordering::SeqCst) {
                self.completed.notify_one();
                Ok(None)
            } else {
                self.workflow
                    .lock()
                    .unwrap()
                    .start_agent_execution(actor, id, NOW)
            }
        })
    }
    fn build_agent_context(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, AgentDispatchContext> {
        Box::pin(async move {
            assert_eq!(actor, ACTOR);
            assert_eq!(id, self.context.execution.id);
            self.builds.fetch_add(1, Ordering::SeqCst);
            self.active_calls.fetch_add(1, Ordering::SeqCst);
            let _active = ActiveCall(&self.active_calls);
            self.entered.notify_one();
            if self.blocked.load(Ordering::SeqCst) {
                self.release.notified().await;
            }
            if self.cancelled.load(Ordering::SeqCst) {
                return Err(WorkError::WorkItemNotFound);
            }
            self.workflow.lock().unwrap().build_agent_context(actor, id)
        })
    }
    fn finish_agent_execution(
        &self,
        context: AgentDispatchContext,
        output: AgentOutput,
    ) -> WorkFuture<'_, AgentExecution> {
        Box::pin(async move {
            assert_eq!(context, self.context);
            self.output.lock().unwrap().push(output.clone());
            let result = if !self.finish_unknown || self.finish_committed {
                self.workflow
                    .lock()
                    .unwrap()
                    .finish_agent_execution(&context, output, NOW)
            } else {
                Err(WorkError::CommitOutcomeUnknown)
            };
            self.completed.notify_one();
            if self.finish_unknown {
                Err(WorkError::CommitOutcomeUnknown)
            } else {
                result
            }
        })
    }
    fn fail_agent_execution(
        &self,
        actor: VerifiedActor,
        id: Uuid,
        code: AgentFailureCode,
    ) -> WorkFuture<'_, AgentExecution> {
        Box::pin(async move {
            assert_eq!(actor, ACTOR);
            assert_eq!(id, self.context.execution.id);
            self.failures.lock().unwrap().push(code);
            let result = self
                .workflow
                .lock()
                .unwrap()
                .fail_agent_execution(actor, id, code, NOW);
            self.completed.notify_one();
            result
        })
    }
    fn interrupt_agent_executions(&self, actor: VerifiedActor) -> WorkFuture<'_, usize> {
        Box::pin(async move {
            assert_eq!(actor, ACTOR);
            assert_eq!(
                self.active_calls.load(Ordering::SeqCst),
                0,
                "owned tasks must be drained before interruption/pool closure"
            );
            self.interruptions.fetch_add(1, Ordering::SeqCst);
            self.workflow
                .lock()
                .unwrap()
                .interrupt_agent_executions(actor, NOW)
        })
    }
    fn list_tasks(&self, _: VerifiedActor, _: TaskView) -> WorkFuture<'_, Vec<TaskSummary>> {
        unreachable!()
    }
    fn task(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, TaskDetail> {
        unreachable!()
    }
    fn artifact(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, WorkingArtifact> {
        unreachable!()
    }
    fn snapshot(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, HandoffSnapshot> {
        unreachable!()
    }
    fn return_instruction(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, ReturnInstruction> {
        unreachable!()
    }
    fn execute(&self, _: VerifiedActor, _: Command) -> WorkFuture<'_, MutationResult> {
        unreachable!()
    }
    fn recover(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, MutationResult> {
        unreachable!()
    }
}

#[tokio::test]
async fn one_shot_output_is_fixed_bounded_and_never_echoes_source_or_purpose() {
    let repository = Arc::new(TestWork::new());
    let dispatcher = OwnedAgentDispatcher::new(repository.clone(), ACTOR);
    dispatcher
        .dispatch(ACTOR, repository.context.execution.id)
        .unwrap();
    repository.wait(&repository.completed).await;
    dispatcher.shutdown().await.unwrap();
    assert_eq!(repository.starts.load(Ordering::SeqCst), 1);
    assert_eq!(repository.builds.load(Ordering::SeqCst), 1);
    let outputs = repository.output.lock().unwrap();
    assert_eq!(outputs.len(), 1);
    let output = &outputs[0];
    let finding = output.finding.as_ref().unwrap();
    assert_eq!(
        finding.evidence_revision_refs,
        repository.context.execution.evidence_revision_refs
    );
    assert_eq!(output.generated_artifacts.len(), 1);
    assert_eq!(
        output
            .suggested_actions
            .iter()
            .map(|s| s.action)
            .collect::<Vec<_>>(),
        [
            ProposedActionCandidate::ReviewFinding,
            ProposedActionCandidate::UseGeneratedArtifact(0)
        ]
    );
    assert!(
        output
            .source_outcomes
            .iter()
            .all(|o| o.outcome == AgentSourceUse::Referenced)
    );
    let draft = &output.generated_artifacts[0];
    for text in [&output.summary, &finding.claim, &draft.title, &draft.text]
        .into_iter()
        .chain(output.suggested_actions.iter().map(|s| &s.rationale))
    {
        assert!(!text.contains("PRIVATE"));
        assert!(text.len() < 2048);
    }
    let stored = repository.workflow.lock().unwrap().clone();
    let result = stored
        .agent_result(ACTOR, repository.context.execution.id)
        .unwrap();
    assert_eq!(result.generated_artifact_ids.len(), 1);
    assert_eq!(result.suggested_action_ids.len(), 2);
    assert!(!output.uncertainty.is_empty());
    assert!(
        output
            .uncertainty
            .iter()
            .any(|s| s.contains("本文分析なし"))
    );
    assert!(
        output
            .uncertainty
            .iter()
            .any(|s| s.contains("実LLM/MCP通信なし"))
    );
    assert!(repository.failures.lock().unwrap().is_empty());
}

#[tokio::test]
async fn owned_shutdown_stops_admission_cancels_inflight_and_drains_before_pool_close() {
    let repository = Arc::new(TestWork::new());
    repository.blocked.store(true, Ordering::SeqCst);
    let dispatcher = OwnedAgentDispatcher::new(repository.clone(), ACTOR);
    let id = repository.context.execution.id;
    assert!(dispatcher.dispatch(VerifiedActor::Office01, id).is_err());
    dispatcher.dispatch(ACTOR, id).unwrap();
    repository.wait(&repository.entered).await;
    dispatcher.dispatch(ACTOR, id).unwrap();
    dispatcher.shutdown().await.unwrap();
    assert!(dispatcher.dispatch(ACTOR, id).is_err());
    assert_eq!(repository.active_calls.load(Ordering::SeqCst), 0);
    assert_eq!(repository.starts.load(Ordering::SeqCst), 1);
    assert_eq!(repository.interruptions.load(Ordering::SeqCst), 1);
    repository.release.notify_one();
    tokio::task::yield_now().await;
    assert!(repository.output.lock().unwrap().is_empty());
    // A previously admitted HTTP request may store a queued receipt during
    // server drain. The final post-HTTP shutdown must sweep that durable state
    // again, even though the owned task was already aborted and joined.
    dispatcher.shutdown().await.unwrap();
    assert_eq!(repository.interruptions.load(Ordering::SeqCst), 2);
    assert_eq!(repository.starts.load(Ordering::SeqCst), 1);
    assert_eq!(repository.builds.load(Ordering::SeqCst), 1);
    assert_eq!(repository.active_calls.load(Ordering::SeqCst), 0);
    assert!(repository.output.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancelled_context_produces_no_late_output() {
    let repository = Arc::new(TestWork::new());
    repository.blocked.store(true, Ordering::SeqCst);
    let dispatcher = OwnedAgentDispatcher::new(repository.clone(), ACTOR);
    dispatcher
        .dispatch(ACTOR, repository.context.execution.id)
        .unwrap();
    repository.wait(&repository.entered).await;
    repository.cancelled.store(true, Ordering::SeqCst);
    repository.release.notify_one();
    repository.wait(&repository.completed).await;
    dispatcher.shutdown().await.unwrap();
    assert!(repository.output.lock().unwrap().is_empty());
}

#[tokio::test]
async fn uncertain_commit_settles_before_shutdown_and_preserves_an_already_committed_result() {
    for stage in ["start_rollback", "finish_rollback", "finish_committed"] {
        let mut fixture = TestWork::new();
        fixture.start_unknown = stage == "start_rollback";
        fixture.finish_unknown = !fixture.start_unknown;
        fixture.finish_committed = stage == "finish_committed";
        let repository = Arc::new(fixture);
        let dispatcher = OwnedAgentDispatcher::new(repository.clone(), ACTOR);
        let id = repository.context.execution.id;
        dispatcher.dispatch(ACTOR, id).unwrap();
        repository.wait(&repository.completed).await;
        let workflow = repository.workflow.lock().unwrap().clone();
        let execution = workflow.agent_execution(ACTOR, id).unwrap();
        assert_eq!(
            execution.status,
            if stage == "finish_committed" {
                AgentExecutionStatus::Succeeded
            } else {
                AgentExecutionStatus::OutcomeUnknown
            },
            "{stage} must settle without requiring shutdown or restart"
        );
        assert_eq!(
            repository.interruptions.load(Ordering::SeqCst),
            0,
            "uncertain completion must fence only this execution"
        );
        assert_eq!(
            *repository.failures.lock().unwrap(),
            [AgentFailureCode::CommitOutcomeUnknown],
            "uncertainty must never become a confirmed-failure reason"
        );
        assert_eq!(repository.starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            repository.builds.load(Ordering::SeqCst),
            usize::from(stage != "start_rollback")
        );
        assert_eq!(
            workflow.findings.len(),
            usize::from(stage == "finish_committed")
        );
        assert!(workflow.decisions.is_empty());
        if stage == "finish_committed" {
            assert!(execution.result.is_some());
            assert!(execution.failure_code.is_none());
        }
        dispatcher.shutdown().await.unwrap();
        assert_eq!(
            *repository.workflow.lock().unwrap(),
            workflow,
            "terminal result/history must remain unchanged by shutdown"
        );
    }
}

#[tokio::test]
async fn durable_start_fence_blocks_dispatch_replay() {
    let fixture = TestWork::new();
    fixture.already_consumed.store(true, Ordering::SeqCst);
    let repository = Arc::new(fixture);
    let dispatcher = OwnedAgentDispatcher::new(repository.clone(), ACTOR);
    dispatcher
        .dispatch(ACTOR, repository.context.execution.id)
        .unwrap();
    repository.wait(&repository.completed).await;
    dispatcher.shutdown().await.unwrap();
    assert_eq!(repository.builds.load(Ordering::SeqCst), 0);
    assert!(repository.output.lock().unwrap().is_empty());
}

#[tokio::test]
async fn dropping_the_owner_aborts_inflight_tasks_instead_of_detaching_them() {
    let repository = Arc::new(TestWork::new());
    repository.blocked.store(true, Ordering::SeqCst);
    let dispatcher = OwnedAgentDispatcher::new(repository.clone(), ACTOR);
    dispatcher
        .dispatch(ACTOR, repository.context.execution.id)
        .unwrap();
    repository.wait(&repository.entered).await;
    drop(dispatcher);
    tokio::task::yield_now().await;
    assert_eq!(repository.active_calls.load(Ordering::SeqCst), 0);
    assert!(repository.output.lock().unwrap().is_empty());
}

/// A stand-in for a future real adapter, proving the port boundary.
struct ScriptedExecutor {
    delay: std::time::Duration,
    output: Result<fn(&AgentDispatchContext) -> AgentOutput, WorkError>,
    calls: AtomicUsize,
}
impl work_application::AgentExecutorPort for ScriptedExecutor {
    fn execute(
        &self,
        context: AgentDispatchContext,
        remaining: std::time::Duration,
    ) -> WorkFuture<'_, AgentOutput> {
        Box::pin(async move {
            assert!(remaining <= work_application::AGENT_EXECUTOR_BUDGET);
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.output.map(|build| build(&context))
        })
    }
}
fn partial(context: &AgentDispatchContext) -> AgentOutput {
    let mut output =
        AgentOutput::referenced_finding(context, "外部adapter", "候補", vec!["不確実".into()]);
    output.generated_artifacts = vec![GeneratedArtifactCandidate {
        title: "下書き".into(),
        text: "本文".into(),
        source_revision_refs: context.execution.evidence_revision_refs.clone(),
    }];
    output
}
fn claims_body_analysis(context: &AgentDispatchContext) -> AgentOutput {
    let mut output = partial(context);
    output.source_outcomes[0].outcome = AgentSourceUse::Analyzed;
    output
}

#[tokio::test]
async fn executor_port_output_is_validated_bounded_and_never_retried() {
    for (name, executor, expected) in [
        (
            "valid",
            ScriptedExecutor {
                delay: std::time::Duration::ZERO,
                output: Ok(partial),
                calls: AtomicUsize::new(0),
            },
            None,
        ),
        (
            "timeout",
            ScriptedExecutor {
                delay: std::time::Duration::from_secs(5),
                output: Ok(partial),
                calls: AtomicUsize::new(0),
            },
            Some(AgentFailureCode::DependencyUnavailable),
        ),
        (
            "executor error",
            ScriptedExecutor {
                delay: std::time::Duration::ZERO,
                output: Err(WorkError::DependencyUnavailable),
                calls: AtomicUsize::new(0),
            },
            Some(AgentFailureCode::DependencyUnavailable),
        ),
        // An adapter's own error never claims Work's commit uncertainty or a
        // provider/context verdict: Work alone decides those.
        (
            "executor claims commit uncertainty",
            ScriptedExecutor {
                delay: std::time::Duration::ZERO,
                output: Err(WorkError::CommitOutcomeUnknown),
                calls: AtomicUsize::new(0),
            },
            Some(AgentFailureCode::DependencyUnavailable),
        ),
        (
            "executor claims denial",
            ScriptedExecutor {
                delay: std::time::Duration::ZERO,
                output: Err(WorkError::Forbidden),
                calls: AtomicUsize::new(0),
            },
            Some(AgentFailureCode::DependencyUnavailable),
        ),
        (
            "invalid output",
            ScriptedExecutor {
                delay: std::time::Duration::ZERO,
                output: Ok(claims_body_analysis),
                calls: AtomicUsize::new(0),
            },
            Some(AgentFailureCode::InvalidOutput),
        ),
    ] {
        let repository = Arc::new(TestWork::new());
        let executor = Arc::new(executor);
        let dispatcher =
            OwnedAgentDispatcher::with_executor(repository.clone(), ACTOR, executor.clone())
                .with_budget(std::time::Duration::from_millis(200));
        let id = repository.context.execution.id;
        dispatcher.dispatch(ACTOR, id).unwrap();
        repository.wait(&repository.completed).await;
        dispatcher.shutdown().await.unwrap();
        let workflow = repository.workflow.lock().unwrap().clone();
        let execution = workflow.agent_execution(ACTOR, id).unwrap();
        assert_eq!(executor.calls.load(Ordering::SeqCst), 1, "{name}");
        match expected {
            None => {
                assert_eq!(execution.status, AgentExecutionStatus::Succeeded, "{name}");
                assert_eq!(workflow.generated_artifacts.len(), 1, "{name}");
                assert!(repository.failures.lock().unwrap().is_empty(), "{name}");
            }
            Some(code) => {
                assert_eq!(execution.status, AgentExecutionStatus::Failed, "{name}");
                assert_eq!(*repository.failures.lock().unwrap(), [code], "{name}");
                assert!(workflow.findings.is_empty(), "{name}");
                assert!(workflow.generated_artifacts.is_empty(), "{name}");
                assert!(workflow.suggested_actions.is_empty(), "{name}");
            }
        }
    }
}
