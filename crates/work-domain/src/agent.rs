//! Bounded synthetic execution state. No model, provider, workflow action or credentials.
use super::*;
pub const MAX_AGENT_EXECUTIONS: usize = 16;
pub const SYNTHETIC_EXECUTOR: &str = "organization-synthetic/agent-01";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    OutcomeUnknown,
}
impl AgentExecutionStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentFailureCode {
    ProviderDenied,
    ContextStale,
    InvalidOutput,
    DependencyUnavailable,
    Interrupted,
    CommitOutcomeUnknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPrincipalBinding {
    pub provider_id: String,
    pub principal_id: String,
    pub invocation_kind: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResult {
    pub summary: String,
    pub finding_revision_refs: Vec<RevisionRef>,
    pub evidence_revision_refs: Vec<RevisionRef>,
    pub uncertainty: Vec<String>,
    pub simulated: bool,
    pub body_analyzed: bool,
    pub live_llm: bool,
    pub mcp_wire_executed: bool,
    /// U4 structured output. Absent in earlier results, whose JSON is unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_outcomes: Vec<AgentSourceOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_artifact_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_action_ids: Vec<Uuid>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecution {
    pub id: Uuid,
    pub context_id: Uuid,
    pub work_item_id: Uuid,
    pub attempt_id: Uuid,
    #[serde(
        serialize_with = "serialize_requester_principal",
        deserialize_with = "deserialize_requester_principal"
    )]
    pub requested_by: VerifiedActor,
    pub requester_responsibility: Uuid,
    pub executed_by: String,
    pub executor_invocation_kind: String,
    pub provider_principal_bindings: Vec<ProviderPrincipalBinding>,
    pub effective_context_revision: i64,
    pub task_revision: i64,
    pub purpose: String,
    pub evidence_revision_refs: Vec<RevisionRef>,
    pub status: AgentExecutionStatus,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub result: Option<AgentResult>,
    pub failure_code: Option<AgentFailureCode>,
}
// Agent records use public principal IDs. The shared VerifiedActor encoding is
// intentionally unchanged because existing operation digests include that enum.
fn serialize_requester_principal<S: serde::Serializer>(
    actor: &VerifiedActor,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(actor.principal_id())
}
fn deserialize_requester_principal<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<VerifiedActor, D::Error> {
    let principal = String::deserialize(deserializer)?;
    // Only this stored Agent field accepts the previous serde spellings.
    VerifiedActor::from_principal_id(&principal)
        .or_else(|| VerifiedActor::from_legacy_encoding(&principal))
        .ok_or_else(|| serde::de::Error::custom("unknown synthetic principal"))
}

/// Trusted application-only input/output, deliberately not an HTTP request DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDispatchContext {
    pub execution: AgentExecution,
    pub evidence: Vec<EvidenceRecord>,
    pub allowed_tools: Vec<AgentReadTool>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentReadTool {
    DocumentRevision,
    DocumentVersionFiles,
}
impl AgentDispatchContext {
    pub fn validate_scope(&self) -> Result<(), WorkError> {
        let e = &self.execution;
        super::evidence::bounded_text(&e.purpose)?;
        validate_refs(&e.evidence_revision_refs)?;
        if self.allowed_tools
            != vec![
                AgentReadTool::DocumentRevision,
                AgentReadTool::DocumentVersionFiles,
            ]
            || e.attempt_id.is_nil()
            || e.task_revision < 0
            || e.effective_context_revision < 0
            || e.id.get_version_num() != 7
            || context_fixture_for_context(e.context_id).is_none()
            || e.requester_responsibility.is_nil()
            || e.executed_by != SYNTHETIC_EXECUTOR
            || e.executor_invocation_kind != "agent"
            || e.provider_principal_bindings
                != vec![ProviderPrincipalBinding {
                    provider_id: "document".into(),
                    principal_id: "poc/poc-agent".into(),
                    invocation_kind: "agent".into(),
                }]
            || e.evidence_revision_refs.is_empty()
            || e.evidence_revision_refs.len() > MAX_AGENT_EXECUTIONS
            || self.evidence.len() != e.evidence_revision_refs.len()
            || self
                .evidence
                .iter()
                .zip(&e.evidence_revision_refs)
                .any(|(record, r)| {
                    record.id != r.id
                        || record.revision != r.revision
                        || record.context_id != e.context_id
                })
        {
            return Err(WorkError::Forbidden);
        }
        Ok(())
    }
}
impl Workflow {
    pub(super) fn validate_agent_integrity(&self) -> Result<(), WorkError> {
        let mut ids = std::collections::BTreeSet::new();
        let mut active = std::collections::BTreeSet::new();
        let mut counts = std::collections::BTreeMap::new();
        for e in &self.agent_executions {
            let count = counts
                .entry((e.work_item_id, e.attempt_id))
                .or_insert(0usize);
            *count += 1;
            if !ids.insert(e.id)
                || *count > MAX_AGENT_EXECUTIONS
                || (e.status.is_active() && !active.insert(e.work_item_id))
                || e.status.is_active() != e.ended_at.is_none()
                || (e.status == AgentExecutionStatus::Succeeded) != e.result.is_some()
            {
                return Err(WorkError::IntegrityViolation);
            }
            let evidence = e
                .evidence_revision_refs
                .iter()
                .map(|r| {
                    self.evidence
                        .iter()
                        .find(|record| record.id == r.id && record.revision == r.revision)
                        .cloned()
                        .ok_or(WorkError::IntegrityViolation)
                })
                .collect::<Result<Vec<_>, _>>()?;
            AgentDispatchContext {
                execution: e.clone(),
                evidence,
                allowed_tools: vec![
                    AgentReadTool::DocumentRevision,
                    AgentReadTool::DocumentVersionFiles,
                ],
            }
            .validate_scope()
            .map_err(|_| WorkError::IntegrityViolation)?;
            if let Some(result) = &e.result {
                // Earlier results have no per-source outcomes and exactly one
                // finding over every selected source; structured ones list one
                // outcome per selected source and cite only usable sources.
                let legacy = result.source_outcomes.is_empty();
                let usable = super::agent_result::usable_sources(result);
                if result.finding_revision_refs.len() > 1
                    || (legacy && result.finding_revision_refs.len() != 1)
                    || (!legacy
                        && (result.source_outcomes.len() != e.evidence_revision_refs.len()
                            || result
                                .source_outcomes
                                .iter()
                                .zip(&e.evidence_revision_refs)
                                .any(|(o, r)| &o.evidence_revision_ref != r)
                            || usable.is_empty()))
                    || result.evidence_revision_refs != e.evidence_revision_refs
                    || !result.simulated
                    || result.body_analyzed
                    || result
                        .source_outcomes
                        .iter()
                        .any(|o| o.outcome == AgentSourceUse::Analyzed)
                    || result.live_llm
                    || result.mcp_wire_executed
                {
                    return Err(WorkError::IntegrityViolation);
                }
                for r in &result.finding_revision_refs {
                    if !self.findings.iter().any(|f| {
                        f.id == r.id
                            && f.revision == r.revision
                            && f.origin_execution_id == Some(e.id)
                            && f.author == SYNTHETIC_EXECUTOR
                            && if legacy {
                                f.evidence_revision_refs == e.evidence_revision_refs
                            } else {
                                !f.evidence_revision_refs.is_empty()
                                    && f.evidence_revision_refs.iter().all(|r| usable.contains(r))
                            }
                            && f.task_id == e.work_item_id
                            && f.attempt_id == e.attempt_id
                    }) {
                        return Err(WorkError::IntegrityViolation);
                    }
                }
            }
        }
        for f in &self.findings {
            if let Some(id) = f.origin_execution_id
                && !self.agent_executions.iter().any(|e| {
                    e.id == id
                        && e.status == AgentExecutionStatus::Succeeded
                        && e.result.as_ref().is_some_and(|result| {
                            result.finding_revision_refs.contains(&RevisionRef {
                                id: f.id,
                                revision: f.revision,
                            })
                        })
                })
            {
                return Err(WorkError::IntegrityViolation);
            }
        }
        Ok(())
    }
    pub(super) fn invalidate_agent_contexts(&mut self, now: &str) -> Result<(), WorkError> {
        let stale: Vec<_> = self
            .agent_executions
            .iter()
            .filter(|e| {
                e.status.is_active()
                    && (e.effective_context_revision != self.revision
                        || self.item(e.work_item_id).ok().is_none_or(|item| {
                            item.attempt_id != e.attempt_id
                                || item.revision != e.task_revision
                                || item.state != TaskState::Active
                        }))
            })
            .map(|e| e.id)
            .collect();
        for id in stale {
            let e = self
                .agent_executions
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or(WorkError::IntegrityViolation)?;
            e.status = AgentExecutionStatus::Failed;
            e.failure_code = Some(AgentFailureCode::ContextStale);
            e.ended_at = Some(now.into());
        }
        Ok(())
    }
    pub fn agent_execution(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<AgentExecution, WorkError> {
        let e = self
            .agent_executions
            .iter()
            .find(|e| e.id == id)
            .ok_or(WorkError::WorkItemNotFound)?;
        let item = self.item(e.work_item_id)?;
        if e.requested_by != actor
            || e.context_id != self.context_id
            || e.attempt_id != item.attempt_id
            || item.acting_assignment_id != Some(e.requester_responsibility)
            || !self.can_read(actor, item)
        {
            return Err(WorkError::WorkItemNotFound);
        }
        Ok(e.clone())
    }
    pub fn agent_result(&self, actor: VerifiedActor, id: Uuid) -> Result<AgentResult, WorkError> {
        let e = self.agent_execution(actor, id)?;
        if e.status != AgentExecutionStatus::Succeeded {
            return Err(WorkError::AgentResultNotReady);
        }
        e.result.ok_or(WorkError::IntegrityViolation)
    }
    /// Build source-only disclosure context for a result whose Work membership has already been authorized.
    pub fn agent_disclosure_context(&self, id: Uuid) -> Result<AgentDispatchContext, WorkError> {
        let execution = self
            .agent_executions
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or(WorkError::IntegrityViolation)?;
        let context = AgentDispatchContext {
            evidence: self
                .resolve_evidence(execution.requested_by, &execution.evidence_revision_refs)?,
            execution,
            allowed_tools: vec![
                AgentReadTool::DocumentRevision,
                AgentReadTool::DocumentVersionFiles,
            ],
        };
        context.validate_scope()?;
        Ok(context)
    }
    pub fn build_agent_context(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<AgentDispatchContext, WorkError> {
        let e = self.agent_execution(actor, id)?;
        let item = self.item(e.work_item_id)?;
        if e.status != AgentExecutionStatus::Running
            || item.state != TaskState::Active
            || e.effective_context_revision != self.revision
            || e.task_revision != item.revision
        {
            return Err(WorkError::WorkContextStale);
        }
        self.validate_selection(actor, e.work_item_id, &e.evidence_revision_refs, &[], &[])?;
        self.agent_disclosure_context(id)
    }
    pub(super) fn apply_agent(
        &mut self,
        actor: VerifiedActor,
        command: &Command,
        now: &str,
    ) -> Result<MutationResult, WorkError> {
        let task_id = command.task_id();
        let item = self.item(task_id)?.clone();
        match command {
            Command::RequestAgentExecution {
                expected_attempt_id,
                purpose,
                evidence_revision_refs,
                ..
            } => {
                super::evidence::bounded_text(purpose)?;
                if item.state != TaskState::Active {
                    return Err(WorkError::HandoffNotReady);
                }
                if item.attempt_id != *expected_attempt_id {
                    return Err(WorkError::RevisionConflict);
                }
                if evidence_revision_refs.is_empty()
                    || evidence_revision_refs.len() > MAX_AGENT_EXECUTIONS
                    || self
                        .agent_executions
                        .iter()
                        .filter(|e| e.work_item_id == task_id && e.attempt_id == item.attempt_id)
                        .count()
                        >= MAX_AGENT_EXECUTIONS
                {
                    return Err(WorkError::ValidationFailed);
                }
                if self
                    .agent_executions
                    .iter()
                    .any(|e| e.work_item_id == task_id && e.status.is_active())
                {
                    return Err(WorkError::WorkAssignmentConflict);
                }
                self.validate_selection(actor, task_id, evidence_revision_refs, &[], &[])?;
                self.next_record_revision(task_id)?;
                let execution = AgentExecution {
                    id: command.context().operation_id,
                    context_id: self.context_id,
                    work_item_id: task_id,
                    attempt_id: item.attempt_id,
                    requested_by: actor,
                    requester_responsibility: command.context().acting_assignment_id,
                    executed_by: SYNTHETIC_EXECUTOR.into(),
                    executor_invocation_kind: "agent".into(),
                    provider_principal_bindings: vec![ProviderPrincipalBinding {
                        provider_id: "document".into(),
                        principal_id: "poc/poc-agent".into(),
                        invocation_kind: "agent".into(),
                    }],
                    effective_context_revision: self
                        .revision
                        .checked_add(1)
                        .ok_or(WorkError::IntegrityViolation)?,
                    task_revision: self.item(task_id)?.revision,
                    purpose: purpose.clone(),
                    evidence_revision_refs: evidence_revision_refs.clone(),
                    status: AgentExecutionStatus::Queued,
                    started_at: now.into(),
                    ended_at: None,
                    result: None,
                    failure_code: None,
                };
                self.agent_executions.push(execution.clone());
                Ok(MutationResult::AgentExecutionRequested {
                    task: self.summary(actor, self.item(task_id)?),
                    execution,
                })
            }
            Command::CancelAgentExecution {
                expected_attempt_id,
                execution_id,
                ..
            } => {
                let e = self.agent_execution(actor, *execution_id)?;
                if item.attempt_id != *expected_attempt_id || e.work_item_id != task_id {
                    return Err(WorkError::RevisionConflict);
                }
                if e.status.is_active() {
                    self.next_record_revision(task_id)?;
                    let revision = self.item(task_id)?.revision;
                    let e = self
                        .agent_executions
                        .iter_mut()
                        .find(|e| e.id == *execution_id)
                        .ok_or(WorkError::IntegrityViolation)?;
                    e.status = AgentExecutionStatus::Cancelled;
                    e.ended_at = Some(now.into());
                    e.task_revision = revision;
                }
                Ok(MutationResult::AgentExecutionCancelled {
                    task: self.summary(actor, self.item(task_id)?),
                    execution: self.agent_execution(actor, *execution_id)?,
                })
            }
            _ => Err(WorkError::IntegrityViolation),
        }
    }
    pub fn start_agent_execution(
        &mut self,
        actor: VerifiedActor,
        id: Uuid,
        now: &str,
    ) -> Result<Option<AgentDispatchContext>, WorkError> {
        let e = self.agent_execution(actor, id)?;
        if e.status != AgentExecutionStatus::Queued {
            return Ok(None);
        }
        let item = self.item(e.work_item_id)?;
        if item.state != TaskState::Active
            || e.task_revision != item.revision
            || e.effective_context_revision != self.revision
        {
            return Err(WorkError::WorkContextStale);
        }
        let mut next = self.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(WorkError::IntegrityViolation)?;
        let stored = next
            .agent_executions
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(WorkError::IntegrityViolation)?;
        stored.status = AgentExecutionStatus::Running;
        stored.started_at = now.into();
        stored.effective_context_revision = next.revision;
        let context = next.build_agent_context(actor, id)?;
        *self = next;
        Ok(Some(context))
    }
    pub fn finish_agent_execution(
        &mut self,
        context: &AgentDispatchContext,
        output: AgentOutput,
        now: &str,
    ) -> Result<AgentExecution, WorkError> {
        context.validate_scope()?;
        let actor = context.execution.requested_by;
        let id = context.execution.id;
        if self.build_agent_context(actor, id)? != *context {
            return Err(WorkError::WorkContextStale);
        }
        output.validate(context)?;
        let mut next = self.clone();
        let task = context.execution.work_item_id;
        let finding = match &output.finding {
            Some(candidate) => {
                let mut finding = next.create_finding(
                    actor,
                    task,
                    super::evidence::FindingInput {
                        claim: &candidate.claim,
                        evidence_revision_refs: &candidate.evidence_revision_refs,
                        supersedes_finding_id: None,
                    },
                    Some(id),
                    now,
                )?;
                finding.uncertainty = output.uncertainty.clone();
                let reference = RevisionRef {
                    id: finding.id,
                    revision: finding.revision,
                };
                next.findings.push(finding);
                Some(reference)
            }
            None => None,
        };
        let (generated_artifact_ids, suggested_action_ids) =
            next.record_candidates(context, &output, finding.clone(), now)?;
        let result = AgentResult {
            summary: output.summary,
            finding_revision_refs: finding.into_iter().collect(),
            evidence_revision_refs: context.execution.evidence_revision_refs.clone(),
            uncertainty: output.uncertainty,
            simulated: true,
            body_analyzed: false,
            live_llm: false,
            mcp_wire_executed: false,
            source_outcomes: output.source_outcomes,
            generated_artifact_ids,
            suggested_action_ids,
        };
        next.next_record_revision(task)?;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(WorkError::IntegrityViolation)?;
        let revision = next.item(task)?.revision;
        let e = next
            .agent_executions
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(WorkError::IntegrityViolation)?;
        e.status = AgentExecutionStatus::Succeeded;
        e.result = Some(result);
        e.ended_at = Some(now.into());
        e.task_revision = revision;
        let done = e.clone();
        next.validate_integrity()?;
        *self = next;
        Ok(done)
    }
    pub fn fail_agent_execution(
        &mut self,
        actor: VerifiedActor,
        id: Uuid,
        code: AgentFailureCode,
        now: &str,
    ) -> Result<AgentExecution, WorkError> {
        let e = self
            .agent_executions
            .iter_mut()
            .find(|e| e.id == id && e.requested_by == actor)
            .ok_or(WorkError::WorkItemNotFound)?;
        if e.status.is_active() {
            e.status = if matches!(
                code,
                AgentFailureCode::Interrupted | AgentFailureCode::CommitOutcomeUnknown
            ) {
                AgentExecutionStatus::OutcomeUnknown
            } else {
                AgentExecutionStatus::Failed
            };
            e.failure_code = Some(code);
            e.ended_at = Some(now.into());
            self.revision = self
                .revision
                .checked_add(1)
                .ok_or(WorkError::IntegrityViolation)?;
        }
        Ok(e.clone())
    }
    pub fn interrupt_agent_executions(
        &mut self,
        actor: VerifiedActor,
        now: &str,
    ) -> Result<usize, WorkError> {
        let ids: Vec<_> = self
            .agent_executions
            .iter()
            .filter(|e| e.requested_by == actor && e.status.is_active())
            .map(|e| e.id)
            .collect();
        for id in &ids {
            self.fail_agent_execution(actor, *id, AgentFailureCode::Interrupted, now)?;
        }
        Ok(ids.len())
    }
}
