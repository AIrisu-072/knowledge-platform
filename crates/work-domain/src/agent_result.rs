//! Structured Agent output: private draft candidates, typed non-executable
//! suggestions and explicit per-source use. Executors propose; Work validates.
use super::*;
use std::collections::BTreeSet;

pub const MAX_GENERATED_ARTIFACTS: usize = 2;
pub const MAX_SUGGESTED_ACTIONS: usize = 4;
pub const MAX_AGENT_UNCERTAINTY: usize = 4;
pub const MAX_CANDIDATE_TITLE_BYTES: usize = 200;
pub const MAX_RATIONALE_BYTES: usize = 1024;
pub const AGENT_PRIVATE_VISIBILITY: &str = "agent_execution_private";

/// What the executor could use of each selected source. Work separately
/// rechecks authorization; this never upgrades a source to verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSourceUse {
    Referenced,
    Analyzed,
    Unavailable,
    Unsupported,
}
impl AgentSourceUse {
    pub fn usable(self) -> bool {
        matches!(self, Self::Referenced | Self::Analyzed)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSourceOutcome {
    pub evidence_revision_ref: RevisionRef,
    pub outcome: AgentSourceUse,
}

/// Executor output, never an HTTP DTO. Indices refer to this output only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOutput {
    pub summary: String,
    pub uncertainty: Vec<String>,
    pub source_outcomes: Vec<AgentSourceOutcome>,
    pub finding: Option<AgentFindingCandidate>,
    pub generated_artifacts: Vec<GeneratedArtifactCandidate>,
    pub suggested_actions: Vec<SuggestedActionCandidate>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFindingCandidate {
    pub claim: String,
    pub evidence_revision_refs: Vec<RevisionRef>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifactCandidate {
    pub title: String,
    pub text: String,
    pub source_revision_refs: Vec<RevisionRef>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestedActionCandidate {
    pub action: ProposedActionCandidate,
    pub rationale: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposedActionCandidate {
    ReviewFinding,
    UseGeneratedArtifact(usize),
}
impl AgentOutput {
    /// A finding over every selected source, used by reference only.
    pub fn referenced_finding(
        context: &AgentDispatchContext,
        summary: impl Into<String>,
        claim: impl Into<String>,
        uncertainty: Vec<String>,
    ) -> Self {
        let refs = context.execution.evidence_revision_refs.clone();
        Self {
            summary: summary.into(),
            uncertainty,
            source_outcomes: refs
                .iter()
                .map(|r| AgentSourceOutcome {
                    evidence_revision_ref: r.clone(),
                    outcome: AgentSourceUse::Referenced,
                })
                .collect(),
            finding: Some(AgentFindingCandidate {
                claim: claim.into(),
                evidence_revision_refs: refs,
            }),
            generated_artifacts: vec![],
            suggested_actions: vec![],
        }
    }
    /// Shape checks independent of the stored workflow.
    pub(super) fn validate(&self, context: &AgentDispatchContext) -> Result<(), WorkError> {
        let selected = &context.execution.evidence_revision_refs;
        super::evidence::bounded_text(&self.summary)?;
        if self.uncertainty.is_empty() || self.uncertainty.len() > MAX_AGENT_UNCERTAINTY {
            return Err(WorkError::ValidationFailed);
        }
        for text in &self.uncertainty {
            super::evidence::bounded_text(text)?;
        }
        if self.source_outcomes.len() != selected.len()
            || self
                .source_outcomes
                .iter()
                .zip(selected)
                .any(|(o, r)| &o.evidence_revision_ref != r)
        {
            return Err(WorkError::ValidationFailed);
        }
        // The simulated executor never reads source bodies.
        if context.execution.executed_by == SYNTHETIC_EXECUTOR
            && self
                .source_outcomes
                .iter()
                .any(|o| o.outcome == AgentSourceUse::Analyzed)
        {
            return Err(WorkError::ValidationFailed);
        }
        let usable: Vec<&RevisionRef> = self
            .source_outcomes
            .iter()
            .filter(|o| o.outcome.usable())
            .map(|o| &o.evidence_revision_ref)
            .collect();
        if usable.is_empty() {
            return Err(WorkError::ValidationFailed);
        }
        let cites_usable = |refs: &[RevisionRef]| {
            !refs.is_empty()
                && validate_refs(refs).is_ok()
                && refs.iter().all(|r| usable.contains(&r))
        };
        if let Some(finding) = &self.finding {
            super::evidence::bounded_text(&finding.claim)?;
            if !cites_usable(&finding.evidence_revision_refs) {
                return Err(WorkError::ValidationFailed);
            }
        }
        if self.generated_artifacts.len() > MAX_GENERATED_ARTIFACTS {
            return Err(WorkError::ValidationFailed);
        }
        for candidate in &self.generated_artifacts {
            valid_candidate_title(&candidate.title)?;
            super::evidence::bounded_text(&candidate.text)?;
            if !cites_usable(&candidate.source_revision_refs) {
                return Err(WorkError::ValidationFailed);
            }
        }
        if self.suggested_actions.len() > MAX_SUGGESTED_ACTIONS {
            return Err(WorkError::ValidationFailed);
        }
        let mut proposals = BTreeSet::new();
        for suggestion in &self.suggested_actions {
            if suggestion.rationale.trim().is_empty()
                || suggestion.rationale.len() > MAX_RATIONALE_BYTES
            {
                return Err(WorkError::ValidationFailed);
            }
            let key = match suggestion.action {
                ProposedActionCandidate::ReviewFinding if self.finding.is_some() => None,
                ProposedActionCandidate::UseGeneratedArtifact(index)
                    if index < self.generated_artifacts.len() =>
                {
                    Some(index)
                }
                _ => return Err(WorkError::ValidationFailed),
            };
            if !proposals.insert(key) {
                return Err(WorkError::ValidationFailed);
            }
        }
        Ok(())
    }
}
fn valid_candidate_title(title: &str) -> Result<(), WorkError> {
    if title.trim().is_empty()
        || title.len() > MAX_CANDIDATE_TITLE_BYTES
        || title.chars().any(char::is_control)
    {
        return Err(WorkError::ValidationFailed);
    }
    Ok(())
}

/// A private draft candidate. It is not a Work artifact: it never enters a
/// submission, and a Human adopts it only through the normal draft save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedArtifact {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub context_id: Uuid,
    pub work_item_id: Uuid,
    pub attempt_id: Uuid,
    pub schema_id: String,
    pub title: String,
    pub value: TextValue,
    pub source_revision_refs: Vec<RevisionRef>,
    pub author: String,
    pub simulated: bool,
    pub visibility: String,
    pub created_at: String,
}
/// A typed proposal. There is no API that executes it; the Human's later
/// command is authorized on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProposedAction {
    ReviewFinding {
        #[serde(rename = "findingRevisionRef")]
        finding_revision_ref: RevisionRef,
    },
    UseGeneratedArtifact {
        #[serde(rename = "generatedArtifactId")]
        generated_artifact_id: Uuid,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedAction {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub context_id: Uuid,
    pub work_item_id: Uuid,
    pub attempt_id: Uuid,
    pub action: ProposedAction,
    pub rationale: String,
    pub supporting_revision_refs: Vec<RevisionRef>,
    pub author: String,
    pub visibility: String,
    pub created_at: String,
}

impl Workflow {
    /// Readable exactly when its execution is readable and succeeded.
    pub fn generated_artifact(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<GeneratedArtifact, WorkError> {
        let record = self
            .generated_artifacts
            .iter()
            .find(|g| g.id == id)
            .ok_or(WorkError::WorkItemNotFound)?;
        self.candidate_execution(actor, record.execution_id)?;
        Ok(record.clone())
    }
    pub fn suggested_action(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<SuggestedAction, WorkError> {
        let record = self
            .suggested_actions
            .iter()
            .find(|s| s.id == id)
            .ok_or(WorkError::WorkItemNotFound)?;
        self.candidate_execution(actor, record.execution_id)?;
        Ok(record.clone())
    }
    fn candidate_execution(
        &self,
        actor: VerifiedActor,
        execution_id: Uuid,
    ) -> Result<AgentExecution, WorkError> {
        let e = self.agent_execution(actor, execution_id)?;
        if e.status != AgentExecutionStatus::Succeeded {
            return Err(WorkError::WorkItemNotFound);
        }
        Ok(e)
    }
    /// Materialize validated candidates; returns their IDs and the finding ref.
    pub(super) fn record_candidates(
        &mut self,
        context: &AgentDispatchContext,
        output: &AgentOutput,
        finding: Option<RevisionRef>,
        now: &str,
    ) -> Result<(Vec<Uuid>, Vec<Uuid>), WorkError> {
        let e = &context.execution;
        let generated: Vec<GeneratedArtifact> = output
            .generated_artifacts
            .iter()
            .map(|candidate| GeneratedArtifact {
                id: Uuid::now_v7(),
                execution_id: e.id,
                context_id: e.context_id,
                work_item_id: e.work_item_id,
                attempt_id: e.attempt_id,
                schema_id: TEXT_SCHEMA_ID.into(),
                title: candidate.title.clone(),
                value: TextValue {
                    text: candidate.text.clone(),
                },
                source_revision_refs: candidate.source_revision_refs.clone(),
                author: e.executed_by.clone(),
                simulated: e.executed_by == SYNTHETIC_EXECUTOR,
                visibility: AGENT_PRIVATE_VISIBILITY.into(),
                created_at: now.into(),
            })
            .collect();
        let usable: Vec<RevisionRef> = output
            .source_outcomes
            .iter()
            .filter(|o| o.outcome.usable())
            .map(|o| o.evidence_revision_ref.clone())
            .collect();
        let suggested = output
            .suggested_actions
            .iter()
            .map(|candidate| {
                let (action, supporting) = match candidate.action {
                    ProposedActionCandidate::ReviewFinding => (
                        ProposedAction::ReviewFinding {
                            finding_revision_ref: finding
                                .clone()
                                .ok_or(WorkError::ValidationFailed)?,
                        },
                        output
                            .finding
                            .as_ref()
                            .map(|f| f.evidence_revision_refs.clone())
                            .unwrap_or_default(),
                    ),
                    ProposedActionCandidate::UseGeneratedArtifact(index) => (
                        ProposedAction::UseGeneratedArtifact {
                            generated_artifact_id: generated
                                .get(index)
                                .ok_or(WorkError::ValidationFailed)?
                                .id,
                        },
                        generated[index].source_revision_refs.clone(),
                    ),
                };
                Ok(SuggestedAction {
                    id: Uuid::now_v7(),
                    execution_id: e.id,
                    context_id: e.context_id,
                    work_item_id: e.work_item_id,
                    attempt_id: e.attempt_id,
                    action,
                    rationale: candidate.rationale.clone(),
                    supporting_revision_refs: if supporting.is_empty() {
                        usable.clone()
                    } else {
                        supporting
                    },
                    author: e.executed_by.clone(),
                    visibility: AGENT_PRIVATE_VISIBILITY.into(),
                    created_at: now.into(),
                })
            })
            .collect::<Result<Vec<_>, WorkError>>()?;
        let ids = (
            generated.iter().map(|g| g.id).collect(),
            suggested.iter().map(|s| s.id).collect(),
        );
        self.generated_artifacts.extend(generated);
        self.suggested_actions.extend(suggested);
        Ok(ids)
    }
    /// Every candidate belongs to exactly one succeeded result that lists it,
    /// inside that execution's scope, citing only sources it could use.
    pub(super) fn validate_candidate_integrity(&self) -> Result<(), WorkError> {
        let mut ids = BTreeSet::new();
        for g in &self.generated_artifacts {
            let (e, result) = self.listing_result(g.execution_id)?;
            if !ids.insert(g.id)
                || !result.generated_artifact_ids.contains(&g.id)
                || !in_scope(e, g.context_id, g.work_item_id, g.attempt_id)
                || g.schema_id != TEXT_SCHEMA_ID
                || g.author != e.executed_by
                || g.simulated != (e.executed_by == SYNTHETIC_EXECUTOR)
                || g.visibility != AGENT_PRIVATE_VISIBILITY
                || valid_candidate_title(&g.title).is_err()
                || super::evidence::bounded_text(&g.value.text).is_err()
                || !cites_usable(result, &g.source_revision_refs)
            {
                return Err(WorkError::IntegrityViolation);
            }
        }
        for s in &self.suggested_actions {
            let (e, result) = self.listing_result(s.execution_id)?;
            let target = match &s.action {
                ProposedAction::ReviewFinding {
                    finding_revision_ref,
                } => result.finding_revision_refs.contains(finding_revision_ref),
                ProposedAction::UseGeneratedArtifact {
                    generated_artifact_id,
                } => result
                    .generated_artifact_ids
                    .contains(generated_artifact_id),
            };
            if !ids.insert(s.id)
                || !target
                || !result.suggested_action_ids.contains(&s.id)
                || !in_scope(e, s.context_id, s.work_item_id, s.attempt_id)
                || s.author != e.executed_by
                || s.visibility != AGENT_PRIVATE_VISIBILITY
                || s.rationale.trim().is_empty()
                || s.rationale.len() > MAX_RATIONALE_BYTES
                || !cites_usable(result, &s.supporting_revision_refs)
            {
                return Err(WorkError::IntegrityViolation);
            }
        }
        for e in &self.agent_executions {
            let Some(result) = &e.result else { continue };
            let listed = result.generated_artifact_ids.len() + result.suggested_action_ids.len();
            let found = self
                .generated_artifacts
                .iter()
                .filter(|g| g.execution_id == e.id)
                .count()
                + self
                    .suggested_actions
                    .iter()
                    .filter(|s| s.execution_id == e.id)
                    .count();
            if listed != found
                || result.generated_artifact_ids.len() > MAX_GENERATED_ARTIFACTS
                || result.suggested_action_ids.len() > MAX_SUGGESTED_ACTIONS
            {
                return Err(WorkError::IntegrityViolation);
            }
        }
        Ok(())
    }
    fn listing_result(
        &self,
        execution_id: Uuid,
    ) -> Result<(&AgentExecution, &AgentResult), WorkError> {
        let e = self
            .agent_executions
            .iter()
            .find(|e| e.id == execution_id && e.status == AgentExecutionStatus::Succeeded)
            .ok_or(WorkError::IntegrityViolation)?;
        Ok((e, e.result.as_ref().ok_or(WorkError::IntegrityViolation)?))
    }
}
fn in_scope(e: &AgentExecution, context_id: Uuid, work_item_id: Uuid, attempt_id: Uuid) -> bool {
    e.context_id == context_id && e.work_item_id == work_item_id && e.attempt_id == attempt_id
}
/// Legacy results (no per-source outcomes) used every selected source.
pub(super) fn usable_sources(result: &AgentResult) -> Vec<RevisionRef> {
    if result.source_outcomes.is_empty() {
        return result.evidence_revision_refs.clone();
    }
    result
        .source_outcomes
        .iter()
        .filter(|o| o.outcome.usable())
        .map(|o| o.evidence_revision_ref.clone())
        .collect()
}
fn cites_usable(result: &AgentResult, refs: &[RevisionRef]) -> bool {
    let usable = usable_sources(result);
    !refs.is_empty() && validate_refs(refs).is_ok() && refs.iter().all(|r| usable.contains(r))
}
