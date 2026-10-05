#![forbid(unsafe_code)]
//! Infrastructure-free Work application ports and canonical operation digest.
use std::{future::Future, pin::Pin};
use uuid::Uuid;
mod agent;
pub use agent::*;
use work_domain::{
    AgentDispatchContext, AgentExecution, AgentFailureCode, AgentFindingOutput, AgentResult,
    Command, EvidenceRecord, EvidenceSource, Finding, HandoffSnapshot, HumanDecision,
    MutationResult, ReturnInstruction, TaskDetail, TaskSummary, TaskView, VerifiedActor, WorkError,
    WorkingArtifact,
};

pub type WorkFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, WorkError>> + Send + 'a>>;
/// Each implementation must bound the whole authorization operation to five seconds,
/// cancel on timeout, and never return stored credentials or source content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSourcePurpose {
    RegisterPublished,
    ReadHistory,
}
pub trait EvidenceSourcePort: Send + Sync {
    fn authorize(
        &self,
        actor: VerifiedActor,
        source: EvidenceSource,
        purpose: EvidenceSourcePurpose,
    ) -> WorkFuture<'_, ()>;
}
pub trait WorkRepository: Send + Sync {
    fn request_agent_execution(
        &self,
        _actor: VerifiedActor,
        _command: Command,
    ) -> WorkFuture<'_, AgentExecutionAcceptance> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn agent_execution(&self, _actor: VerifiedActor, _id: Uuid) -> WorkFuture<'_, AgentExecution> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn agent_result(&self, _actor: VerifiedActor, _id: Uuid) -> WorkFuture<'_, AgentResult> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn start_agent_execution(
        &self,
        _actor: VerifiedActor,
        _id: Uuid,
    ) -> WorkFuture<'_, Option<AgentDispatchContext>> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn build_agent_context(
        &self,
        _actor: VerifiedActor,
        _id: Uuid,
    ) -> WorkFuture<'_, AgentDispatchContext> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn finish_agent_execution(
        &self,
        _context: AgentDispatchContext,
        _output: AgentFindingOutput,
    ) -> WorkFuture<'_, AgentExecution> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn fail_agent_execution(
        &self,
        _actor: VerifiedActor,
        _id: Uuid,
        _code: AgentFailureCode,
    ) -> WorkFuture<'_, AgentExecution> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn interrupt_agent_executions(&self, _actor: VerifiedActor) -> WorkFuture<'_, usize> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }

    fn list_evidence(
        &self,
        _actor: VerifiedActor,
        _task_id: Uuid,
    ) -> WorkFuture<'_, Vec<EvidenceRecord>> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn evidence(&self, _actor: VerifiedActor, _id: Uuid) -> WorkFuture<'_, EvidenceRecord> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn list_findings(&self, _actor: VerifiedActor, _task_id: Uuid) -> WorkFuture<'_, Vec<Finding>> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn finding(&self, _actor: VerifiedActor, _id: Uuid) -> WorkFuture<'_, Finding> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn list_decisions(
        &self,
        _actor: VerifiedActor,
        _finding_id: Uuid,
    ) -> WorkFuture<'_, Vec<HumanDecision>> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }

    fn list_tasks(&self, actor: VerifiedActor, view: TaskView) -> WorkFuture<'_, Vec<TaskSummary>>;
    fn task(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, TaskDetail>;
    fn artifact(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, WorkingArtifact>;
    fn snapshot(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, HandoffSnapshot>;
    fn return_instruction(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, ReturnInstruction>;
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
    fn workflow_action_digest_binds_kind_attempt_action_and_current_responsibility() {
        for kind in ["complete", "hold", "resume"] {
            let original = serde_json::json!({
                "kind":kind, "task_id":work_domain::OFFICE_TASK_ID,
                "context":{"operationId":Uuid::now_v7(),"expectedRevision":1,"actingAssignmentId":work_domain::OFFICE_ASSIGNMENT_ID},
                "expected_attempt_id":work_domain::OFFICE_ATTEMPT_ID,
                "definition_action_id":work_domain::COMPLETE_ACTION_ID
            });
            let command: Command = serde_json::from_value(original.clone()).unwrap();
            let digest = command_digest(VerifiedActor::Office01, &command).unwrap();
            assert_eq!(
                digest,
                command_digest(VerifiedActor::Office01, &command.clone()).unwrap()
            );
            assert_ne!(
                digest,
                command_digest(VerifiedActor::Sales01, &command).unwrap()
            );
            for field in ["expected_attempt_id", "definition_action_id", "task_id"] {
                let mut changed = original.clone();
                changed[field] = serde_json::json!(Uuid::now_v7());
                assert_ne!(
                    digest,
                    command_digest(
                        VerifiedActor::Office01,
                        &serde_json::from_value(changed).unwrap()
                    )
                    .unwrap()
                );
            }
            let mut changed = original.clone();
            changed["context"]["expectedRevision"] = serde_json::json!(2);
            assert_ne!(
                digest,
                command_digest(
                    VerifiedActor::Office01,
                    &serde_json::from_value(changed).unwrap()
                )
                .unwrap()
            );
            let mut changed = original.clone();
            changed["context"]["actingAssignmentId"] =
                serde_json::json!(work_domain::SALES_ASSIGNMENT_ID);
            assert_ne!(
                digest,
                command_digest(
                    VerifiedActor::Office01,
                    &serde_json::from_value(changed).unwrap()
                )
                .unwrap()
            );
            let mut changed = original;
            changed["kind"] = serde_json::json!(if kind == "hold" { "resume" } else { "hold" });
            assert_ne!(
                digest,
                command_digest(
                    VerifiedActor::Office01,
                    &serde_json::from_value(changed).unwrap()
                )
                .unwrap()
            );
        }
    }
    #[test]
    fn legacy_submit_serialization_preserves_existing_operation_digest() {
        let legacy = serde_json::json!({"kind":"submit","task_id":SALES_TASK_ID,"context":{"operationId":Uuid::now_v7(),"expectedRevision":1,"actingAssignmentId":work_domain::SALES_ASSIGNMENT_ID},"artifacts":[{"artifactId":Uuid::now_v7(),"revision":0}]});
        let command: Command = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(serde_json::to_value(command).unwrap(), legacy);
    }
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
    #[test]
    fn agent_digest_binds_purpose_selection_attempt_and_assignment_without_new_legacy_fields() {
        let original = Command::RequestAgentExecution {
            task_id: SALES_TASK_ID,
            context: CommandContext {
                operation_id: Uuid::now_v7(),
                expected_revision: 1,
                acting_assignment_id: work_domain::SALES_ASSIGNMENT_ID,
            },
            expected_attempt_id: work_domain::SALES_ATTEMPT_ID,
            purpose: "bounded purpose".into(),
            evidence_revision_refs: vec![work_domain::RevisionRef {
                id: Uuid::now_v7(),
                revision: 1,
            }],
        };
        let digest = command_digest(VerifiedActor::Sales01, &original).unwrap();
        assert_eq!(
            digest,
            command_digest(VerifiedActor::Sales01, &original.clone()).unwrap()
        );
        for field in ["purpose", "expected_attempt_id", "evidence_revision_refs"] {
            let mut value = serde_json::to_value(&original).unwrap();
            value[field] = match field {
                "purpose" => serde_json::json!("changed"),
                "expected_attempt_id" => serde_json::json!(Uuid::now_v7()),
                _ => serde_json::json!([{"id":Uuid::now_v7(),"revision":1}]),
            };
            let changed: Command = serde_json::from_value(value).unwrap();
            assert_ne!(
                digest,
                command_digest(VerifiedActor::Sales01, &changed).unwrap()
            );
        }
    }
    #[test]
    fn agent_requester_json_is_canonical_while_legacy_actor_and_digest_stay_unchanged() {
        use sha2::{Digest, Sha256};
        let id = Uuid::now_v7();
        let legacy = serde_json::json!({"id":id,"contextId":work_domain::CONTEXT_ID,"workItemId":SALES_TASK_ID,"attemptId":work_domain::SALES_ATTEMPT_ID,"requestedBy":"sales01","requesterResponsibility":work_domain::SALES_ASSIGNMENT_ID,"executedBy":"organization-synthetic/agent-01","executorInvocationKind":"agent","providerPrincipalBindings":[{"providerId":"document","principalId":"poc/poc-agent","invocationKind":"agent"}],"effectiveContextRevision":2,"taskRevision":2,"purpose":"synthetic request","evidenceRevisionRefs":[{"id":Uuid::now_v7(),"revision":1}],"status":"queued","startedAt":"2026-10-04T00:00:00Z","endedAt":null,"result":null,"failureCode":null});
        let mut execution: AgentExecution = serde_json::from_value(legacy.clone()).unwrap();
        let task = work_domain::Workflow::synthetic(None)
            .detail(VerifiedActor::Sales01, SALES_TASK_ID)
            .unwrap()
            .task;
        let stored_receipt: MutationResult = serde_json::from_value(
            serde_json::json!({"kind":"agent_execution_requested","task":task,"execution":legacy}),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(stored_receipt).unwrap()["execution"]["requestedBy"],
            "sales-01"
        );
        for (actor, canonical, old) in [
            (VerifiedActor::Sales01, "sales-01", "sales01"),
            (VerifiedActor::Office01, "office-01", "office01"),
        ] {
            execution.requested_by = actor;
            let canonical_json = serde_json::to_value(&execution).unwrap();
            assert_eq!(canonical_json["requestedBy"], canonical);
            assert_eq!(
                serde_json::from_value::<AgentExecution>(canonical_json.clone()).unwrap(),
                execution
            );
            let mut old_json = canonical_json;
            old_json["requestedBy"] = serde_json::json!(old);
            assert_eq!(
                serde_json::from_value::<AgentExecution>(old_json.clone()).unwrap(),
                execution
            );
            old_json["requestedBy"] = serde_json::json!("poc-agent");
            assert!(serde_json::from_value::<AgentExecution>(old_json).is_err());
            // Canonical Agent DTO output must not rename the shared legacy enum.
            assert_eq!(serde_json::to_value(actor).unwrap(), serde_json::json!(old));
            assert!(serde_json::from_value::<VerifiedActor>(serde_json::json!(canonical)).is_err());
            let command = Command::Claim {
                task_id: SALES_TASK_ID,
                context: CommandContext {
                    operation_id: id,
                    expected_revision: 0,
                    acting_assignment_id: actor.assignment_id(),
                },
            };
            // Serialize the same tuple layout as the original command digest, including key order.
            let original_bytes = serde_json::to_vec(&(old, &command)).unwrap();
            assert_eq!(
                command_digest(actor, &command).unwrap(),
                Sha256::digest(original_bytes).to_vec()
            );
        }
    }
}
