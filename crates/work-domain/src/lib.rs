#![forbid(unsafe_code)]
//! Work-owned synthetic workflow values. No Document authority or infrastructure.
use serde::{Deserialize, Serialize};
use uuid::Uuid;
mod agent;
mod evidence;
mod organization;
pub use agent::*;
pub use evidence::*;
pub use organization::*;
use time::OffsetDateTime;

pub const WORKFLOW_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000001);
pub const CONTEXT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000002);
pub const SALES_TASK_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000003);
pub const OFFICE_TASK_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000004);
pub const SALES_ATTEMPT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000005);
pub const OFFICE_ATTEMPT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000006);
pub const SALES_ASSIGNMENT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000007);
pub const OFFICE_ASSIGNMENT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000008);
pub const DEFINITION_VERSION_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000009);
pub const RETURN_DEFINITION_VERSION_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000f);
pub const COMPLETE_DEFINITION_VERSION_ID: Uuid =
    Uuid::from_u128(0x01900000000070008000000000000011);
pub const HOLD_RESUME_DEFINITION_VERSION_ID: Uuid =
    Uuid::from_u128(0x01900000000070008000000000000013);
pub const HOLD_ACTION_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000014);
pub const RESUME_ACTION_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000015);
pub const COMPLETE_ACTION_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000012);
pub const RETURN_TRANSITION_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000010);
pub const SALES_STEP_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000a);
pub const OFFICE_STEP_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000b);
pub const SALES_WORK_TYPE_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000c);
pub const OFFICE_WORK_TYPE_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000d);
pub const SALES_WORK_ASSIGNMENT_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000e);
const FIXTURE_CREATED_AT: &str = "2026-10-04T00:00:00Z";
pub const TEXT_SCHEMA_ID: &str = "organization.text-draft.v1";
pub const MAX_TEXT_BYTES: usize = 8 * 1024;
pub const MAX_ARTIFACTS: usize = 16;
/// Responsibility periods per attempt (claim plus reassignments); bounds body growth.
pub const MAX_ASSIGNMENTS_PER_ATTEMPT: usize = 16;

/// Closed allowlist of synthetic Human principals (Domain §16). The serde
/// encoding is the legacy operation-digest input and must not be renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerifiedActor {
    Sales01,
    Office01,
    Review01,
    Approver01,
    MultiRole01,
    Delegate01,
}
impl VerifiedActor {
    pub const ALL: [Self; 6] = [
        Self::Sales01,
        Self::Office01,
        Self::Review01,
        Self::Approver01,
        Self::MultiRole01,
        Self::Delegate01,
    ];
    pub fn from_startup_profile(profile: &str) -> Result<Self, WorkError> {
        Self::from_principal_id(profile).ok_or(WorkError::Forbidden)
    }
    pub fn from_principal_id(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|actor| actor.principal_id() == value)
    }
    /// Only stored records written before the public spelling may use this.
    pub fn from_legacy_encoding(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|actor| actor.legacy_encoding() == value)
    }
    /// Exactly the derived serde spelling above.
    pub fn legacy_encoding(self) -> &'static str {
        match self {
            Self::Sales01 => "sales01",
            Self::Office01 => "office01",
            Self::Review01 => "review01",
            Self::Approver01 => "approver01",
            Self::MultiRole01 => "multi-role01",
            Self::Delegate01 => "delegate01",
        }
    }
    pub fn principal_id(self) -> &'static str {
        match self {
            Self::Sales01 => "sales-01",
            Self::Office01 => "office-01",
            Self::Review01 => "review-01",
            Self::Approver01 => "approver-01",
            Self::MultiRole01 => "multi-role-01",
            Self::Delegate01 => "delegate-01",
        }
    }
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Sales01 => "営業担当（模擬）",
            Self::Office01 => "事務担当（模擬）",
            Self::Review01 => "審査担当（模擬）",
            Self::Approver01 => "承認・業務管理（模擬）",
            Self::MultiRole01 => "兼務担当（模擬）",
            Self::Delegate01 => "代理担当（模擬）",
        }
    }
    /// Fixture default formal assignment. It is a selection hint, never a grant:
    /// every use is re-resolved against the current Organization policy.
    pub fn assignment_id(self) -> Uuid {
        match self {
            Self::Sales01 => SALES_ASSIGNMENT_ID,
            Self::Office01 => OFFICE_ASSIGNMENT_ID,
            Self::Review01 => REVIEW_ASSIGNMENT_ID,
            Self::Approver01 => APPROVER_ASSIGNMENT_ID,
            Self::MultiRole01 => MULTI_ROLE_PROCESSING_ASSIGNMENT_ID,
            Self::Delegate01 => Uuid::nil(),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkError {
    #[error("VALIDATION_FAILED")]
    ValidationFailed,
    #[error("FORBIDDEN")]
    Forbidden,
    #[error("WORK_ITEM_NOT_FOUND")]
    WorkItemNotFound,
    #[error("WORK_ARTIFACT_NOT_FOUND")]
    WorkArtifactNotFound,
    #[error("EVIDENCE_NOT_FOUND")]
    EvidenceNotFound,
    #[error("FINDING_NOT_FOUND")]
    FindingNotFound,
    #[error("REVISION_CONFLICT")]
    RevisionConflict,
    #[error("OPERATION_CONFLICT")]
    OperationConflict,
    #[error("WORK_ASSIGNMENT_CONFLICT")]
    WorkAssignmentConflict,
    #[error("HANDOFF_NOT_READY")]
    HandoffNotReady,
    #[error("DEPENDENCY_UNAVAILABLE")]
    DependencyUnavailable,
    #[error("COMMIT_OUTCOME_UNKNOWN")]
    CommitOutcomeUnknown,
    #[error("INTEGRITY_VIOLATION")]
    IntegrityViolation,
    #[error("AGENT_RESULT_NOT_READY")]
    AgentResultNotReady,
    #[error("WORK_CONTEXT_STALE")]
    WorkContextStale,
    #[error("CURSOR_STALE")]
    CursorStale,
    #[error("ORGANIZATION_RECORD_NOT_FOUND")]
    OrganizationRecordNotFound,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Ready,
    Active,
    Held,
    Completed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskView {
    Context,
    Queue,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextValue {
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingArtifact {
    pub id: Uuid,
    pub task_id: Uuid,
    pub attempt_id: Uuid,
    pub revision: i64,
    pub schema_id: String,
    pub value: TextValue,
    pub visibility: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinnedArtifact {
    pub artifact_id: Uuid,
    pub revision: i64,
    pub schema_id: String,
    pub value: TextValue,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffSnapshot {
    #[serde(default)]
    pub evidence_revision_refs: Vec<RevisionRef>,
    #[serde(default)]
    pub finding_revision_refs: Vec<RevisionRef>,
    #[serde(default)]
    pub decision_revision_refs: Vec<RevisionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_submission_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_instruction_id: Option<Uuid>,
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub context_id: Uuid,
    pub submission_number: u32,
    pub submitted_by: String,
    pub acting_assignment_id: Uuid,
    pub source_task_id: Uuid,
    pub source_attempt_id: Uuid,
    pub target_task_id: Uuid,
    pub created_at: String,
    pub artifacts: Vec<PinnedArtifact>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnTransition {
    pub transition_id: Uuid,
    pub target_task_id: Uuid,
    pub previous_submission_id: Uuid,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnInstruction {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub context_id: Uuid,
    pub source_task_id: Uuid,
    pub source_attempt_id: Uuid,
    pub target_task_id: Uuid,
    pub target_attempt_id: Uuid,
    pub previous_submission_id: Uuid,
    pub transition_id: Uuid,
    pub reason: String,
    pub returned_by: String,
    pub acting_assignment_id: Uuid,
    pub created_at: String,
}
fn first_attempt() -> u32 {
    1
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    #[serde(default)]
    pub can_hold: bool,
    #[serde(default)]
    pub hold_action_id: Option<Uuid>,
    #[serde(default)]
    pub can_resume: bool,
    #[serde(default)]
    pub resume_action_id: Option<Uuid>,
    #[serde(default)]
    pub can_complete: bool,
    #[serde(default)]
    pub completion_action_id: Option<Uuid>,
    #[serde(default)]
    pub can_request_agent: bool,
    #[serde(default)]
    pub can_register_evidence: bool,
    #[serde(default)]
    pub can_register_finding: bool,
    #[serde(default)]
    pub can_record_decision: bool,
    /// Generic step responsibility segment; a label is never a grant.
    #[serde(default)]
    pub required_role_id: Option<Uuid>,
    /// Server-chosen eligible responsibility for an explicit claim, if any.
    #[serde(default)]
    pub claim_assignment_id: Option<Uuid>,
    #[serde(default)]
    pub can_assign: bool,
    /// Disclosed only to the current assignee and to a current `work.assign` holder.
    #[serde(default)]
    pub assignment: Option<TaskAssignmentView>,
    #[serde(default = "first_attempt")]
    pub attempt_number: u32,
    #[serde(default)]
    pub can_return: bool,
    #[serde(default)]
    pub return_instruction_id: Option<Uuid>,
    #[serde(default)]
    pub return_transition: Option<ReturnTransition>,
    pub id: Uuid,
    pub context_id: Uuid,
    pub attempt_id: Uuid,
    pub revision: i64,
    pub title: String,
    pub step_label: String,
    pub state: TaskState,
    pub can_claim: bool,
    pub can_edit: bool,
    pub can_submit: bool,
    pub handoff_snapshot_id: Option<Uuid>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAssignmentView {
    pub principal_id: String,
    pub display_name: String,
    pub acting_assignment_id: Uuid,
    pub acting_kind: Option<ResponsibilityKind>,
    pub role_label: Option<String>,
    pub delegator_principal_id: Option<String>,
    pub responsibility_effective: bool,
}
/// Append-only attribution of one attempt's responsibility period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkAssignmentRecord {
    pub id: Uuid,
    pub task_id: Uuid,
    pub attempt_id: Uuid,
    #[serde(with = "principal_serde")]
    pub principal: VerifiedActor,
    pub acting_assignment_id: Uuid,
    #[serde(with = "principal_serde::option")]
    pub assigned_by: Option<VerifiedActor>,
    pub manager_assignment_id: Option<Uuid>,
    pub reason: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    #[serde(with = "principal_serde::option")]
    pub ended_by: Option<VerifiedActor>,
}
/// Non-persisted evaluation input: the separately owned Organization policy and
/// the trusted server instant. A missing policy uses the fixed synthetic fixture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PolicyAuthority {
    policy: Option<std::sync::Arc<OrganizationPolicy>>,
    evaluated_at: Option<OffsetDateTime>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputResourceRef {
    pub kind: String,
    pub document_id: Uuid,
    pub label: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub kind: String,
    pub occurred_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    #[serde(default)]
    pub agent_execution_ids: Vec<Uuid>,
    #[serde(flatten)]
    pub task: TaskSummary,
    pub input_resources: Vec<InputResourceRef>,
    pub history: Vec<HistoryEntry>,
    pub working_artifacts: Vec<WorkingArtifact>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_instruction_id: Option<Uuid>,
    pub id: Uuid,
    pub workflow_instance_id: Uuid,
    pub step_id: Uuid,
    pub work_type_id: Uuid,
    pub attempt_number: u32,
    pub work_assignment_id: Option<Uuid>,
    pub acting_assignment_id: Option<Uuid>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub attempt_id: Uuid,
    pub revision: i64,
    pub state: TaskState,
    pub assignee: Option<VerifiedActor>,
    pub handoff_snapshot_id: Option<Uuid>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_executions: Vec<AgentExecution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<HumanDecision>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completed_attempts: Vec<WorkItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub return_instructions: Vec<ReturnInstruction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<WorkAssignmentRecord>,
    #[serde(skip)]
    pub authority: PolicyAuthority,
    pub id: Uuid,
    pub definition_version_id: Uuid,
    pub context_id: Uuid,
    pub revision: i64,
    pub source: WorkItem,
    pub next: Option<WorkItem>,
    pub artifacts: Vec<WorkingArtifact>,
    pub snapshots: Vec<HandoffSnapshot>,
    pub history: Vec<HistoryEntry>,
    pub input_resources: Vec<InputResourceRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandContext {
    pub operation_id: Uuid,
    pub expected_revision: i64,
    pub acting_assignment_id: Uuid,
}
impl CommandContext {
    /// Validate operation identity only. Acting responsibility is resolved against
    /// the current Organization policy by the aggregate; immutable replay is
    /// governed by the exact digest plus current read authority.
    pub fn authorize(&self, _actor: VerifiedActor) -> Result<(), WorkError> {
        if self.operation_id.get_version_num() != 7 || self.expected_revision < 0 {
            return Err(WorkError::ValidationFailed);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactSelection {
    pub artifact_id: Uuid,
    pub revision: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    Hold {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        definition_action_id: Uuid,
    },
    Resume {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        definition_action_id: Uuid,
    },
    Complete {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        definition_action_id: Uuid,
    },
    RequestAgentExecution {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        purpose: String,
        evidence_revision_refs: Vec<RevisionRef>,
    },
    CancelAgentExecution {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        execution_id: Uuid,
    },
    RegisterEvidence {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        source: EvidenceSource,
        relevant_location: String,
    },
    RegisterFinding {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        claim: String,
        evidence_revision_refs: Vec<RevisionRef>,
        #[serde(default)]
        supersedes_finding_id: Option<Uuid>,
    },
    RecordDecision {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        finding_id: Uuid,
        finding_revision: i64,
        decision: DecisionKind,
        #[serde(default)]
        adopted_claim: Option<String>,
        #[serde(default)]
        reason: Option<String>,
        evidence_revision_refs: Vec<RevisionRef>,
        #[serde(default)]
        supersedes_decision_id: Option<Uuid>,
    },
    Return {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        previous_submission_id: Uuid,
        target_task_id: Uuid,
        transition_id: Uuid,
        reason: String,
    },
    /// Assign or reassign the current attempt; requires a current `work.assign`.
    Assign {
        task_id: Uuid,
        context: CommandContext,
        expected_attempt_id: Uuid,
        #[serde(with = "principal_serde")]
        assignee: VerifiedActor,
        assignee_responsibility_id: Uuid,
        reason: String,
    },
    SaveDraft {
        task_id: Uuid,
        artifact_id: Option<Uuid>,
        context: CommandContext,
        value: TextValue,
    },
    Claim {
        task_id: Uuid,
        context: CommandContext,
    },
    Submit {
        task_id: Uuid,
        context: CommandContext,
        artifacts: Vec<ArtifactSelection>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_attempt_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        evidence_revision_refs: Vec<RevisionRef>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        finding_revision_refs: Vec<RevisionRef>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        decision_revision_refs: Vec<RevisionRef>,
    },
}
impl Command {
    pub fn context(&self) -> &CommandContext {
        match self {
            Self::Hold { context, .. }
            | Self::Resume { context, .. }
            | Self::Complete { context, .. }
            | Self::RequestAgentExecution { context, .. }
            | Self::CancelAgentExecution { context, .. }
            | Self::RegisterEvidence { context, .. }
            | Self::RegisterFinding { context, .. }
            | Self::RecordDecision { context, .. }
            | Self::SaveDraft { context, .. }
            | Self::Claim { context, .. }
            | Self::Submit { context, .. }
            | Self::Assign { context, .. }
            | Self::Return { context, .. } => context,
        }
    }
    pub fn task_id(&self) -> Uuid {
        match self {
            Self::Hold { task_id, .. }
            | Self::Resume { task_id, .. }
            | Self::Complete { task_id, .. }
            | Self::RequestAgentExecution { task_id, .. }
            | Self::CancelAgentExecution { task_id, .. }
            | Self::RegisterEvidence { task_id, .. }
            | Self::RegisterFinding { task_id, .. }
            | Self::RecordDecision { task_id, .. }
            | Self::SaveDraft { task_id, .. }
            | Self::Claim { task_id, .. }
            | Self::Submit { task_id, .. }
            | Self::Assign { task_id, .. }
            | Self::Return { task_id, .. } => *task_id,
        }
    }
    /// The policy action each Human command requires on its target step.
    pub fn required_action(&self) -> PolicyAction {
        match self {
            Self::Hold { .. } => PolicyAction::WorkHold,
            Self::Resume { .. } => PolicyAction::WorkResume,
            Self::Complete { .. } => PolicyAction::WorkComplete,
            Self::RequestAgentExecution { .. } | Self::CancelAgentExecution { .. } => {
                PolicyAction::AgentRequest
            }
            Self::RegisterEvidence { .. } => PolicyAction::EvidenceRegister,
            Self::RegisterFinding { .. } => PolicyAction::FindingRegister,
            Self::RecordDecision { .. } => PolicyAction::DecisionRecord,
            Self::Return { .. } => PolicyAction::WorkReturn,
            Self::Assign { .. } => PolicyAction::WorkAssign,
            Self::SaveDraft { .. } => PolicyAction::WorkEdit,
            Self::Claim { .. } => PolicyAction::WorkClaim,
            Self::Submit { .. } => PolicyAction::WorkSubmit,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationResult {
    Held {
        task: TaskSummary,
    },
    Resumed {
        task: TaskSummary,
    },
    Completed {
        task: TaskSummary,
    },
    AgentExecutionRequested {
        task: TaskSummary,
        execution: AgentExecution,
    },
    AgentExecutionCancelled {
        task: TaskSummary,
        execution: AgentExecution,
    },
    EvidenceRegistered {
        task: TaskSummary,
        evidence: EvidenceRecord,
    },
    FindingRegistered {
        task: TaskSummary,
        finding: Finding,
    },
    DecisionRecorded {
        task: TaskSummary,
        decision: HumanDecision,
    },
    Returned {
        task: TaskSummary,
        #[serde(rename = "returnInstruction")]
        return_instruction: ReturnInstruction,
        #[serde(rename = "nextTask")]
        next_task: TaskSummary,
    },
    DraftSaved {
        task: TaskSummary,
        artifact: WorkingArtifact,
    },
    Claimed {
        task: TaskSummary,
    },
    Submitted {
        task: TaskSummary,
        snapshot: HandoffSnapshot,
        #[serde(rename = "nextTask")]
        next_task: TaskSummary,
    },
    Assigned {
        task: TaskSummary,
        assignment: WorkAssignmentRecord,
    },
    RoleAssignmentCreated {
        assignment: RoleAssignment,
        #[serde(rename = "policyRevision")]
        policy_revision: i64,
    },
    RoleAssignmentRevoked {
        assignment: RoleAssignment,
        #[serde(rename = "policyRevision")]
        policy_revision: i64,
    },
    DelegationCreated {
        delegation: Delegation,
        #[serde(rename = "policyRevision")]
        policy_revision: i64,
    },
    DelegationRevoked {
        delegation: Delegation,
        #[serde(rename = "policyRevision")]
        policy_revision: i64,
    },
}
impl MutationResult {
    /// Organization policy receipts carry no Work target.
    pub fn is_policy(&self) -> bool {
        matches!(
            self,
            Self::RoleAssignmentCreated { .. }
                | Self::RoleAssignmentRevoked { .. }
                | Self::DelegationCreated { .. }
                | Self::DelegationRevoked { .. }
        )
    }
}
impl Workflow {
    pub fn synthetic(document_id: Option<Uuid>) -> Self {
        Self {
            agent_executions: vec![],
            evidence: vec![],
            findings: vec![],
            decisions: vec![],
            id: WORKFLOW_ID,
            definition_version_id: HOLD_RESUME_DEFINITION_VERSION_ID,
            completed_attempts: vec![],
            return_instructions: vec![],
            assignments: vec![],
            // The fixture evaluates against the synthetic policy until a repository
            // attaches the stored one; a deserialized workflow carries none.
            authority: PolicyAuthority {
                policy: Some(std::sync::Arc::new(OrganizationPolicy::synthetic())),
                evaluated_at: None,
            },
            context_id: CONTEXT_ID,
            revision: 0,
            source: WorkItem {
                return_instruction_id: None,
                id: SALES_TASK_ID,
                workflow_instance_id: WORKFLOW_ID,
                step_id: SALES_STEP_ID,
                work_type_id: SALES_WORK_TYPE_ID,
                attempt_number: 1,
                work_assignment_id: Some(SALES_WORK_ASSIGNMENT_ID),
                acting_assignment_id: Some(SALES_ASSIGNMENT_ID),
                created_at: FIXTURE_CREATED_AT.into(),
                completed_at: None,
                attempt_id: SALES_ATTEMPT_ID,
                revision: 0,
                state: TaskState::Active,
                assignee: Some(VerifiedActor::Sales01),
                handoff_snapshot_id: None,
            },
            next: None,
            artifacts: vec![],
            snapshots: vec![],
            history: vec![],
            input_resources: document_id
                .into_iter()
                .map(|document_id| InputResourceRef {
                    kind: "document".into(),
                    document_id,
                    label: "共有入力文書".into(),
                })
                .collect(),
        }
    }
    fn item(&self, id: Uuid) -> Result<&WorkItem, WorkError> {
        if id == self.source.id {
            return Ok(&self.source);
        }
        self.next
            .as_ref()
            .filter(|item| item.id == id)
            .ok_or(WorkError::WorkItemNotFound)
    }
    /// Attach the separately owned Organization policy and the trusted server
    /// instant used by every eligibility and assignment check of one evaluation.
    pub fn attach_authority(
        &mut self,
        policy: std::sync::Arc<OrganizationPolicy>,
        at: OffsetDateTime,
    ) {
        self.authority = PolicyAuthority {
            policy: Some(policy),
            evaluated_at: Some(at),
        };
    }
    pub fn with_authority(mut self, policy: OrganizationPolicy, at: OffsetDateTime) -> Self {
        self.attach_authority(std::sync::Arc::new(policy), at);
        self
    }
    /// A workflow evaluated without an attached policy grants nothing.
    fn policy(&self) -> &OrganizationPolicy {
        static EMPTY: std::sync::OnceLock<OrganizationPolicy> = std::sync::OnceLock::new();
        self.authority
            .policy
            .as_deref()
            .unwrap_or_else(|| EMPTY.get_or_init(OrganizationPolicy::empty))
    }
    fn evaluated_at(&self) -> OffsetDateTime {
        self.authority
            .evaluated_at
            .unwrap_or_else(OffsetDateTime::now_utc)
    }
    /// Labels of a responsibility in the attached policy, regardless of effect.
    pub fn describe_responsibility(&self, id: Uuid) -> Option<Responsibility> {
        self.policy().describe(id)
    }
    /// The responsibility segment of each fixed definition step.
    pub fn step_role(&self, item: &WorkItem) -> Option<Uuid> {
        match item.step_id {
            SALES_STEP_ID => Some(ROLE_SALES_ID),
            OFFICE_STEP_ID => Some(ROLE_PROCESSING_ID),
            _ => None,
        }
    }
    fn step_responsibility(
        &self,
        actor: VerifiedActor,
        id: Uuid,
        item: &WorkItem,
        action: PolicyAction,
    ) -> Option<Responsibility> {
        let role = self.step_role(item)?;
        self.policy()
            .responsibility(actor, id, self.evaluated_at())
            .filter(|value| value.role_id == role && value.allows(action))
    }
    /// First effective responsibility (optionally only the selected one) whose
    /// role is the step's segment and which allows the action.
    fn eligible_in(
        &self,
        actor: VerifiedActor,
        item: &WorkItem,
        action: PolicyAction,
        scope: Option<Uuid>,
    ) -> Option<Responsibility> {
        let role = self.step_role(item)?;
        self.policy()
            .responsibilities(actor, self.evaluated_at())
            .into_iter()
            .find(|value| {
                scope.is_none_or(|id| value.id == id)
                    && value.role_id == role
                    && value.allows(action)
            })
    }
    fn eligible(&self, actor: VerifiedActor, item: &WorkItem, action: PolicyAction) -> bool {
        self.eligible_in(actor, item, action, None).is_some()
    }
    // Historical participation is never an authority to read an attempt-private
    // draft: the assignee's recorded acting responsibility must be effective now.
    fn can_read(&self, actor: VerifiedActor, item: &WorkItem) -> bool {
        self.can_act(actor, item, PolicyAction::WorkRead)
    }
    fn can_act(&self, actor: VerifiedActor, item: &WorkItem, action: PolicyAction) -> bool {
        item.assignee == Some(actor)
            && item
                .acting_assignment_id
                .is_some_and(|id| self.step_responsibility(actor, id, item, action).is_some())
    }
    fn assigning_in(&self, actor: VerifiedActor, scope: Option<Uuid>) -> Option<Responsibility> {
        self.policy()
            .responsibilities(actor, self.evaluated_at())
            .into_iter()
            .find(|value| {
                scope.is_none_or(|id| value.id == id) && value.allows(PolicyAction::WorkAssign)
            })
    }
    /// Minimal disclosure check used only to choose 403 over hidden 404.
    fn visible(&self, actor: VerifiedActor, item: &WorkItem) -> bool {
        self.can_read(actor, item)
            || (item.state == TaskState::Ready
                && item.assignee.is_none()
                && self.eligible(actor, item, PolicyAction::QueueRead))
            || (item.state != TaskState::Completed && self.assigning_in(actor, None).is_some())
            || self
                .policy()
                .responsibilities(actor, self.evaluated_at())
                .iter()
                .any(|value| value.allows(PolicyAction::ContextProgressRead))
    }
    fn assignment_view(&self, item: &WorkItem) -> Option<TaskAssignmentView> {
        let principal = item.assignee?;
        let id = item.acting_assignment_id?;
        let described = self.policy().describe(id);
        Some(TaskAssignmentView {
            principal_id: principal.principal_id().into(),
            display_name: principal.display_name().into(),
            acting_assignment_id: id,
            acting_kind: described.as_ref().map(|value| value.kind),
            role_label: described.as_ref().map(|value| value.role_label.clone()),
            delegator_principal_id: described
                .as_ref()
                .and_then(|value| value.delegator)
                .map(|value| value.principal_id().into()),
            responsibility_effective: self
                .step_responsibility(principal, id, item, PolicyAction::WorkRead)
                .is_some(),
        })
    }
    fn return_transition(&self, actor: VerifiedActor, item: &WorkItem) -> Option<ReturnTransition> {
        if !matches!(
            self.definition_version_id,
            RETURN_DEFINITION_VERSION_ID
                | COMPLETE_DEFINITION_VERSION_ID
                | HOLD_RESUME_DEFINITION_VERSION_ID
        ) || item.id != OFFICE_TASK_ID
            || !self.can_act(actor, item, PolicyAction::WorkReturn)
            || item.state != TaskState::Active
            || self.source.state != TaskState::Completed
        {
            return None;
        }
        let snapshot = self.snapshots.iter().find(|snapshot| {
            Some(snapshot.id) == item.handoff_snapshot_id
                && Some(snapshot.id) == self.source.handoff_snapshot_id
                && snapshot.source_task_id == self.source.id
                && snapshot.source_attempt_id == self.source.attempt_id
                && snapshot.target_task_id == item.id
        })?;
        Some(ReturnTransition {
            transition_id: RETURN_TRANSITION_ID,
            target_task_id: self.source.id,
            previous_submission_id: snapshot.id,
        })
    }
    fn completion_action(&self, actor: VerifiedActor, item: &WorkItem) -> Option<Uuid> {
        (matches!(
            self.definition_version_id,
            COMPLETE_DEFINITION_VERSION_ID | HOLD_RESUME_DEFINITION_VERSION_ID
        ) && item.id == OFFICE_TASK_ID
            && item.step_id == OFFICE_STEP_ID
            && self.can_act(actor, item, PolicyAction::WorkComplete)
            && item.state == TaskState::Active)
            .then_some(COMPLETE_ACTION_ID)
    }
    fn pause_action(&self, actor: VerifiedActor, item: &WorkItem, resume: bool) -> Option<Uuid> {
        let state = if resume {
            TaskState::Held
        } else {
            TaskState::Active
        };
        let action = if resume {
            PolicyAction::WorkResume
        } else {
            PolicyAction::WorkHold
        };
        (self.definition_version_id == HOLD_RESUME_DEFINITION_VERSION_ID
            && matches!(
                (item.id, item.step_id),
                (SALES_TASK_ID, SALES_STEP_ID) | (OFFICE_TASK_ID, OFFICE_STEP_ID)
            )
            && self.can_act(actor, item, action)
            && item.work_assignment_id.is_some()
            && item.state == state)
            .then_some(if resume {
                RESUME_ACTION_ID
            } else {
                HOLD_ACTION_ID
            })
    }
    fn summary(&self, actor: VerifiedActor, item: &WorkItem) -> TaskSummary {
        self.summary_in(actor, item, None)
    }
    fn summary_in(
        &self,
        actor: VerifiedActor,
        item: &WorkItem,
        scope: Option<Uuid>,
    ) -> TaskSummary {
        let source = item.id == self.source.id;
        let active = item.state == TaskState::Active;
        let readable = self.can_read(actor, item);
        let editable = source && active && self.can_act(actor, item, PolicyAction::WorkEdit);
        let label = if source {
            "営業内容整理"
        } else {
            "事務内容確認"
        };
        let return_transition = self.return_transition(actor, item);
        let completion_action_id = self.completion_action(actor, item);
        let hold_action_id = self.pause_action(actor, item, false);
        let resume_action_id = self.pause_action(actor, item, true);
        let claim = (item.state == TaskState::Ready && item.assignee.is_none())
            .then(|| self.eligible_in(actor, item, PolicyAction::WorkClaim, scope))
            .flatten();
        let can_assign =
            item.state != TaskState::Completed && self.assigning_in(actor, scope).is_some();
        TaskSummary {
            can_hold: hold_action_id.is_some(),
            hold_action_id,
            can_resume: resume_action_id.is_some(),
            resume_action_id,
            can_complete: completion_action_id.is_some(),
            completion_action_id,
            can_request_agent: active
                && self.can_act(actor, item, PolicyAction::AgentRequest)
                && self
                    .agent_executions
                    .iter()
                    .filter(|e| e.work_item_id == item.id && e.attempt_id == item.attempt_id)
                    .count()
                    < MAX_AGENT_EXECUTIONS
                && !self
                    .agent_executions
                    .iter()
                    .any(|e| e.work_item_id == item.id && e.status.is_active()),
            can_register_evidence: active
                && self.can_act(actor, item, PolicyAction::EvidenceRegister),
            can_register_finding: active
                && self.can_act(actor, item, PolicyAction::FindingRegister),
            can_record_decision: active && self.can_act(actor, item, PolicyAction::DecisionRecord),
            required_role_id: self.step_role(item),
            claim_assignment_id: claim.as_ref().map(|value| value.id),
            can_assign,
            assignment: (item.assignee == Some(actor) || can_assign)
                .then(|| self.assignment_view(item))
                .flatten(),
            attempt_number: item.attempt_number,
            can_return: return_transition.is_some(),
            // Submission and return identifiers belong to the assignee's projection;
            // eligible-only, management and continuity rows never carry them.
            return_instruction_id: readable.then_some(item.return_instruction_id).flatten(),
            return_transition,
            id: item.id,
            context_id: self.context_id,
            attempt_id: item.attempt_id,
            revision: item.revision,
            title: label.into(),
            step_label: label.into(),
            state: item.state,
            can_claim: claim.is_some(),
            can_edit: editable,
            can_submit: editable
                && self.can_act(actor, item, PolicyAction::WorkSubmit)
                && self
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.attempt_id == item.attempt_id),
            handoff_snapshot_id: readable.then_some(item.handoff_snapshot_id).flatten(),
        }
    }
    /// Union of every current responsibility of the actor.
    pub fn list_tasks(&self, actor: VerifiedActor, view: TaskView) -> Vec<TaskSummary> {
        self.list_tasks_in(actor, view, None).unwrap_or_default()
    }
    /// Projection for one selected acting responsibility, or the union when absent.
    /// The same WorkItem identities and revisions appear in both views.
    pub fn list_tasks_in(
        &self,
        actor: VerifiedActor,
        view: TaskView,
        scope: Option<Uuid>,
    ) -> Result<Vec<TaskSummary>, WorkError> {
        let responsibilities: Vec<_> = self
            .policy()
            .responsibilities(actor, self.evaluated_at())
            .into_iter()
            .filter(|value| scope.is_none_or(|id| value.id == id))
            .collect();
        if scope.is_some() && responsibilities.is_empty() {
            return Err(WorkError::Forbidden);
        }
        let manages = responsibilities
            .iter()
            .any(|value| value.allows(PolicyAction::WorkAssign));
        let continuity = view == TaskView::Context
            && responsibilities
                .iter()
                .any(|value| value.allows(PolicyAction::ContextProgressRead));
        let mut items = vec![];
        for item in std::iter::once(&self.source).chain(self.next.iter()) {
            let role = self.step_role(item);
            let own = self.can_read(actor, item)
                && scope.is_none_or(|id| item.acting_assignment_id == Some(id));
            let queue = item.state == TaskState::Ready
                && item.assignee.is_none()
                && responsibilities.iter().any(|value| {
                    Some(value.role_id) == role
                        && value.allows(PolicyAction::QueueRead)
                        && value.allows(PolicyAction::WorkClaim)
                });
            let managed = manages && item.state != TaskState::Completed;
            if own || queue || managed || continuity {
                items.push(self.summary_in(actor, item, scope));
            }
        }
        Ok(items)
    }
    pub fn detail(&self, actor: VerifiedActor, id: Uuid) -> Result<TaskDetail, WorkError> {
        let item = self.item(id)?;
        if !self.can_read(actor, item) {
            return Err(WorkError::WorkItemNotFound);
        }
        Ok(TaskDetail {
            agent_execution_ids: self
                .agent_executions
                .iter()
                .filter(|e| {
                    e.work_item_id == item.id
                        && e.attempt_id == item.attempt_id
                        && e.requested_by == actor
                        && item.acting_assignment_id == Some(e.requester_responsibility)
                })
                .map(|e| e.id)
                .collect(),
            task: self.summary(actor, item),
            input_resources: self.input_resources.clone(),
            history: self.history.clone(),
            working_artifacts: self
                .artifacts
                .iter()
                .filter(|artifact| artifact.attempt_id == item.attempt_id)
                .cloned()
                .collect(),
        })
    }
    pub fn artifact(&self, actor: VerifiedActor, id: Uuid) -> Result<WorkingArtifact, WorkError> {
        let artifact = self
            .artifacts
            .iter()
            .find(|artifact| artifact.id == id)
            .ok_or(WorkError::WorkArtifactNotFound)?;
        let item = self
            .item(artifact.task_id)
            .map_err(|_| WorkError::WorkArtifactNotFound)?;
        if !self.can_read(actor, item) || artifact.attempt_id != item.attempt_id {
            return Err(WorkError::WorkArtifactNotFound);
        }
        Ok(artifact.clone())
    }
    pub fn snapshot(&self, actor: VerifiedActor, id: Uuid) -> Result<HandoffSnapshot, WorkError> {
        let snapshot = self
            .snapshots
            .iter()
            .find(|snapshot| snapshot.id == id)
            .ok_or(WorkError::WorkArtifactNotFound)?;
        // The submitter keeps its own immutable submission only while it still
        // holds the source step's responsibility.
        let source_readable = snapshot.submitted_by == actor.principal_id()
            && self
                .item(snapshot.source_task_id)
                .is_ok_and(|item| self.eligible(actor, item, PolicyAction::WorkRead));
        // Immutable submitted membership remains readable for a current recipient
        // attempt assignee, even after the receiving attempt closes.
        let target_readable = self
            .next
            .iter()
            .chain(self.completed_attempts.iter())
            .any(|item| {
                item.id == snapshot.target_task_id
                    && self.can_read(actor, item)
                    && item.handoff_snapshot_id == Some(id)
            });
        // A current assignee reads the submission its attempt received or was
        // returned with, and that submission's immediate predecessor.
        let current_readable = std::iter::once(&self.source)
            .chain(self.next.iter())
            .any(|item| {
                self.can_read(actor, item)
                    && item.handoff_snapshot_id.is_some_and(|current| {
                        current == id
                            || self.snapshots.iter().any(|value| {
                                value.id == current && value.previous_submission_id == Some(id)
                            })
                    })
            });
        if !source_readable && !target_readable && !current_readable {
            return Err(WorkError::WorkArtifactNotFound);
        }
        Ok(snapshot.clone())
    }
    pub fn return_instruction(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<ReturnInstruction, WorkError> {
        let instruction = self
            .return_instructions
            .iter()
            .find(|value| value.id == id)
            .ok_or(WorkError::WorkArtifactNotFound)?;
        self.snapshot(actor, instruction.previous_submission_id)?;
        let eligible = |task_id| {
            self.item(task_id)
                .is_ok_and(|item| self.eligible(actor, item, PolicyAction::WorkRead))
        };
        if !eligible(instruction.source_task_id) && !eligible(instruction.target_task_id) {
            return Err(WorkError::WorkArtifactNotFound);
        }
        Ok(instruction.clone())
    }
    /// One current pointer per stable task, unique attempt identities/numbers,
    /// and completed-only archives, checked before and after every transition.
    pub fn validate_integrity(&self) -> Result<(), WorkError> {
        if self.id != WORKFLOW_ID
            || self.source.id != SALES_TASK_ID
            || !matches!(
                self.definition_version_id,
                DEFINITION_VERSION_ID
                    | RETURN_DEFINITION_VERSION_ID
                    | COMPLETE_DEFINITION_VERSION_ID
                    | HOLD_RESUME_DEFINITION_VERSION_ID
            )
            || self
                .next
                .as_ref()
                .is_some_and(|item| item.id != OFFICE_TASK_ID)
        {
            return Err(WorkError::IntegrityViolation);
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut numbers = std::collections::BTreeSet::new();
        for item in std::iter::once(&self.source)
            .chain(self.next.iter())
            .chain(self.completed_attempts.iter())
        {
            if item.workflow_instance_id != self.id
                || item.attempt_number == 0
                || item.revision < 0
                || !ids.insert(item.attempt_id)
                || !numbers.insert((item.id, item.attempt_number))
                || item.assignee.is_some() != item.acting_assignment_id.is_some()
            {
                return Err(WorkError::IntegrityViolation);
            }
        }
        for archived in &self.completed_attempts {
            let current = self
                .item(archived.id)
                .map_err(|_| WorkError::IntegrityViolation)?;
            if archived.state != TaskState::Completed
                || archived.completed_at.is_none()
                || archived.attempt_number >= current.attempt_number
                || archived.revision >= current.revision
            {
                return Err(WorkError::IntegrityViolation);
            }
        }
        // At most one open responsibility period per attempt, matching its pointer.
        let mut open = std::collections::BTreeSet::new();
        for record in &self.assignments {
            if record.ended_at.is_some() {
                continue;
            }
            let current = std::iter::once(&self.source)
                .chain(self.next.iter())
                .find(|item| item.attempt_id == record.attempt_id)
                .ok_or(WorkError::IntegrityViolation)?;
            if !open.insert(record.attempt_id)
                || current.id != record.task_id
                || current.work_assignment_id != Some(record.id)
                || current.assignee != Some(record.principal)
                || current.acting_assignment_id != Some(record.acting_assignment_id)
            {
                return Err(WorkError::IntegrityViolation);
            }
        }
        self.validate_agent_integrity()?;
        Ok(())
    }
    pub fn authorize_command(
        &self,
        actor: VerifiedActor,
        command: &Command,
    ) -> Result<(), WorkError> {
        command.context().authorize(actor)?;
        let acting = command.context().acting_assignment_id;
        // The requested responsibility must belong to the verified actor now.
        let responsibility = self
            .policy()
            .responsibility(actor, acting, self.evaluated_at())
            .ok_or(WorkError::Forbidden)?;
        let item = self.item(command.task_id())?;
        let action = command.required_action();
        let hidden = |visible: bool| {
            if visible {
                WorkError::Forbidden
            } else {
                WorkError::WorkItemNotFound
            }
        };
        match command {
            Command::Claim { .. } => {
                if self.step_role(item) != Some(responsibility.role_id)
                    || !responsibility.allows(action)
                {
                    return Err(hidden(self.visible(actor, item)));
                }
            }
            Command::Assign { assignee, .. } => {
                if !responsibility.allows(action) {
                    return Err(hidden(self.visible(actor, item)));
                }
                // Separation of duties: the management path never grants its own
                // caller task access; a manager with a step role claims instead.
                if *assignee == actor {
                    return Err(WorkError::Forbidden);
                }
            }
            Command::SaveDraft { artifact_id, .. } => {
                self.authorize_assigned(actor, item, acting, action)?;
                if let Some(id) = artifact_id {
                    let artifact = self.artifact(actor, *id)?;
                    if artifact.task_id != item.id {
                        return Err(WorkError::WorkArtifactNotFound);
                    }
                }
            }
            Command::Hold { .. }
            | Command::Resume { .. }
            | Command::Complete { .. }
            | Command::RequestAgentExecution { .. }
            | Command::CancelAgentExecution { .. }
            | Command::RegisterEvidence { .. }
            | Command::RegisterFinding { .. }
            | Command::RecordDecision { .. }
            | Command::Submit { .. }
            | Command::Return { .. } => self.authorize_assigned(actor, item, acting, action)?,
        }
        Ok(())
    }
    /// Commands on an assigned attempt use exactly its recorded acting responsibility.
    fn authorize_assigned(
        &self,
        actor: VerifiedActor,
        item: &WorkItem,
        acting: Uuid,
        action: PolicyAction,
    ) -> Result<(), WorkError> {
        if !self.can_read(actor, item) {
            return Err(WorkError::WorkItemNotFound);
        }
        if item.acting_assignment_id != Some(acting)
            || self
                .step_responsibility(actor, acting, item, action)
                .is_none()
        {
            return Err(WorkError::Forbidden);
        }
        Ok(())
    }
    pub fn authorize_recovery(
        &self,
        actor: VerifiedActor,
        result: &MutationResult,
    ) -> Result<(), WorkError> {
        match result {
            MutationResult::AgentExecutionRequested { execution, .. }
            | MutationResult::AgentExecutionCancelled { execution, .. } => {
                self.agent_execution(actor, execution.id)?;
            }
            MutationResult::EvidenceRegistered { evidence, .. } => {
                self.evidence_record(actor, evidence.id)?;
            }
            MutationResult::FindingRegistered { finding, .. } => {
                self.finding(actor, finding.id)?;
            }
            MutationResult::DecisionRecorded { decision, .. } => {
                self.decision(actor, decision.id)?;
            }
            MutationResult::Returned {
                return_instruction, ..
            } => {
                self.return_instruction(actor, return_instruction.id)?;
            }
            MutationResult::DraftSaved { artifact, .. } => {
                self.artifact(actor, artifact.id)?;
            }
            MutationResult::Held { task }
            | MutationResult::Resumed { task }
            | MutationResult::Claimed { task }
            | MutationResult::Completed { task } => {
                self.detail(actor, task.id)?;
            }
            MutationResult::Submitted { snapshot, .. } => {
                self.snapshot(actor, snapshot.id)?;
            }
            MutationResult::Assigned { task, .. } => {
                // The assigning manager keeps the receipt only while it still holds
                // a current `work.assign`; the new assignee reads through detail.
                let item = self.item(task.id)?;
                if self.assigning_in(actor, None).is_none() && !self.can_read(actor, item) {
                    return Err(WorkError::WorkItemNotFound);
                }
            }
            MutationResult::RoleAssignmentCreated { .. }
            | MutationResult::RoleAssignmentRevoked { .. }
            | MutationResult::DelegationCreated { .. }
            | MutationResult::DelegationRevoked { .. } => {
                return Err(WorkError::IntegrityViolation);
            }
        }
        Ok(())
    }
    pub fn apply(
        &mut self,
        actor: VerifiedActor,
        command: &Command,
        now: &str,
    ) -> Result<MutationResult, WorkError> {
        let at = parse_instant(now).map_err(|_| WorkError::IntegrityViolation)?;
        // Validate against a private copy evaluated at the trusted commit instant
        // so every failed command leaves the aggregate untouched.
        let mut next = self.clone();
        next.authority.evaluated_at = Some(at);
        next.validate_integrity()?;
        next.authorize_command(actor, command)?;
        let item = next.item(command.task_id())?;
        if item.revision != command.context().expected_revision {
            return Err(WorkError::RevisionConflict);
        }
        // This guards only a new mutation. Repository replay/recovery resolves the
        // committed receipt first and keeps current read authorization while held.
        if item.state == TaskState::Held
            && !matches!(command, Command::Resume { .. } | Command::Assign { .. })
        {
            return Err(WorkError::HandoffNotReady);
        }
        let result = next.apply_validated(actor, command, now)?;
        next.close_finished_assignments(now);
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(WorkError::IntegrityViolation)?;
        next.invalidate_agent_contexts(now)?;
        next.validate_integrity()?;
        next.authority = self.authority.clone();
        *self = next;
        Ok(result)
    }
    /// A responsibility period ends with its attempt (`endedBy` stays empty);
    /// reassignment ends it explicitly with the assigning actor instead.
    fn close_finished_assignments(&mut self, now: &str) {
        let open: Vec<Uuid> = std::iter::once(&self.source)
            .chain(self.next.iter())
            .filter(|item| item.state != TaskState::Completed)
            .map(|item| item.attempt_id)
            .collect();
        for record in &mut self.assignments {
            if record.ended_at.is_none() && !open.contains(&record.attempt_id) {
                record.ended_at = Some(now.into());
            }
        }
    }
    fn apply_validated(
        &mut self,
        actor: VerifiedActor,
        command: &Command,
        now: &str,
    ) -> Result<MutationResult, WorkError> {
        match command {
            Command::Hold {
                task_id,
                expected_attempt_id,
                definition_action_id,
                ..
            }
            | Command::Resume {
                task_id,
                expected_attempt_id,
                definition_action_id,
                ..
            } => {
                let resume = matches!(command, Command::Resume { .. });
                let item = self.item(*task_id)?;
                if item.attempt_id != *expected_attempt_id {
                    return Err(WorkError::RevisionConflict);
                }
                if self.pause_action(actor, item, resume) != Some(*definition_action_id) {
                    return Err(WorkError::HandoffNotReady);
                }
                let item = if *task_id == self.source.id {
                    &mut self.source
                } else {
                    self.next.as_mut().ok_or(WorkError::IntegrityViolation)?
                };
                item.state = if resume {
                    TaskState::Active
                } else {
                    TaskState::Held
                };
                item.revision = item
                    .revision
                    .checked_add(1)
                    .ok_or(WorkError::IntegrityViolation)?;
                self.history.push(HistoryEntry {
                    kind: if resume { "resumed" } else { "held" }.into(),
                    occurred_at: now.into(),
                });
                let task = self.summary(actor, self.item(*task_id)?);
                Ok(if resume {
                    MutationResult::Resumed { task }
                } else {
                    MutationResult::Held { task }
                })
            }
            Command::Complete {
                task_id,
                expected_attempt_id,
                definition_action_id,
                ..
            } => {
                let item = self.item(*task_id)?;
                if item.attempt_id != *expected_attempt_id {
                    return Err(WorkError::RevisionConflict);
                }
                if self.completion_action(actor, item) != Some(*definition_action_id) {
                    return Err(WorkError::HandoffNotReady);
                }
                let item = self.next.as_mut().ok_or(WorkError::IntegrityViolation)?;
                item.state = TaskState::Completed;
                item.completed_at = Some(now.into());
                item.revision = item
                    .revision
                    .checked_add(1)
                    .ok_or(WorkError::IntegrityViolation)?;
                self.history.push(HistoryEntry {
                    kind: "completed".into(),
                    occurred_at: now.into(),
                });
                Ok(MutationResult::Completed {
                    task: self.summary(actor, self.item(*task_id)?),
                })
            }
            Command::RequestAgentExecution { .. } | Command::CancelAgentExecution { .. } => {
                self.apply_agent(actor, command, now)
            }
            Command::RegisterEvidence { .. }
            | Command::RegisterFinding { .. }
            | Command::RecordDecision { .. } => self.apply_evidence(actor, command, now),
            Command::Return {
                expected_attempt_id,
                previous_submission_id,
                target_task_id,
                transition_id,
                reason,
                task_id,
                ..
            } => {
                let item = self.item(*task_id)?;
                if item.attempt_id != *expected_attempt_id {
                    return Err(WorkError::RevisionConflict);
                }
                if reason.trim().is_empty() || reason.len() > MAX_TEXT_BYTES {
                    return Err(WorkError::ValidationFailed);
                }
                let permitted = self
                    .return_transition(actor, item)
                    .ok_or(WorkError::HandoffNotReady)?;
                if permitted.transition_id != *transition_id
                    || permitted.target_task_id != *target_task_id
                    || permitted.previous_submission_id != *previous_submission_id
                {
                    return Err(WorkError::HandoffNotReady);
                }
                let target_attempt_id = Uuid::now_v7();
                let instruction = ReturnInstruction {
                    id: Uuid::now_v7(),
                    workflow_id: self.id,
                    context_id: self.context_id,
                    source_task_id: *task_id,
                    source_attempt_id: item.attempt_id,
                    target_task_id: *target_task_id,
                    target_attempt_id,
                    previous_submission_id: *previous_submission_id,
                    transition_id: *transition_id,
                    reason: reason.clone(),
                    returned_by: actor.principal_id().into(),
                    acting_assignment_id: command.context().acting_assignment_id,
                    created_at: now.into(),
                };
                self.completed_attempts.push(self.source.clone());
                self.source = WorkItem {
                    return_instruction_id: Some(instruction.id),
                    id: self.source.id,
                    workflow_instance_id: self.id,
                    step_id: self.source.step_id,
                    work_type_id: self.source.work_type_id,
                    attempt_number: self
                        .source
                        .attempt_number
                        .checked_add(1)
                        .ok_or(WorkError::IntegrityViolation)?,
                    revision: self
                        .source
                        .revision
                        .checked_add(1)
                        .ok_or(WorkError::IntegrityViolation)?,
                    attempt_id: target_attempt_id,
                    state: TaskState::Ready,
                    assignee: None,
                    work_assignment_id: None,
                    acting_assignment_id: None,
                    created_at: now.into(),
                    completed_at: None,
                    handoff_snapshot_id: Some(*previous_submission_id),
                };
                let office = self.next.as_mut().ok_or(WorkError::IntegrityViolation)?;
                office.state = TaskState::Completed;
                office.completed_at = Some(now.into());
                office.revision = office
                    .revision
                    .checked_add(1)
                    .ok_or(WorkError::IntegrityViolation)?;
                office.return_instruction_id = Some(instruction.id);
                self.return_instructions.push(instruction.clone());
                self.history.push(HistoryEntry {
                    kind: "returned".into(),
                    occurred_at: now.into(),
                });
                Ok(MutationResult::Returned {
                    task: self.summary(actor, self.item(*task_id)?),
                    return_instruction: instruction,
                    next_task: self.summary(actor, &self.source),
                })
            }

            Command::SaveDraft {
                task_id,
                artifact_id,
                value,
                ..
            } => {
                if *task_id != self.source.id || self.source.state != TaskState::Active {
                    return Err(WorkError::HandoffNotReady);
                }
                if value.text.trim().is_empty() || value.text.len() > MAX_TEXT_BYTES {
                    return Err(WorkError::ValidationFailed);
                }
                let artifact = if let Some(id) = artifact_id {
                    let artifact = self
                        .artifacts
                        .iter_mut()
                        .find(|artifact| artifact.id == *id)
                        .ok_or(WorkError::WorkArtifactNotFound)?;
                    artifact.revision = artifact
                        .revision
                        .checked_add(1)
                        .ok_or(WorkError::IntegrityViolation)?;
                    artifact.value = value.clone();
                    artifact.clone()
                } else {
                    if self
                        .artifacts
                        .iter()
                        .filter(|artifact| artifact.attempt_id == self.source.attempt_id)
                        .count()
                        >= MAX_ARTIFACTS
                    {
                        return Err(WorkError::ValidationFailed);
                    }
                    let artifact = WorkingArtifact {
                        id: Uuid::now_v7(),
                        task_id: *task_id,
                        attempt_id: self.source.attempt_id,
                        revision: 0,
                        schema_id: TEXT_SCHEMA_ID.into(),
                        value: value.clone(),
                        visibility: "work_item_private".into(),
                    };
                    self.artifacts.push(artifact.clone());
                    artifact
                };
                self.source.revision = self
                    .source
                    .revision
                    .checked_add(1)
                    .ok_or(WorkError::IntegrityViolation)?;
                Ok(MutationResult::DraftSaved {
                    task: self.summary(actor, &self.source),
                    artifact,
                })
            }
            Command::Claim { task_id, .. } => {
                let next = if *task_id == self.source.id {
                    &mut self.source
                } else {
                    self.next.as_mut().ok_or(WorkError::WorkItemNotFound)?
                };
                if next.state != TaskState::Ready || next.assignee.is_some() {
                    return Err(WorkError::WorkAssignmentConflict);
                }
                let record = WorkAssignmentRecord {
                    id: Uuid::now_v7(),
                    task_id: next.id,
                    attempt_id: next.attempt_id,
                    principal: actor,
                    acting_assignment_id: command.context().acting_assignment_id,
                    assigned_by: None,
                    manager_assignment_id: None,
                    reason: None,
                    started_at: now.into(),
                    ended_at: None,
                    ended_by: None,
                };
                next.state = TaskState::Active;
                next.assignee = Some(actor);
                next.work_assignment_id = Some(record.id);
                next.acting_assignment_id = Some(record.acting_assignment_id);
                next.revision = next
                    .revision
                    .checked_add(1)
                    .ok_or(WorkError::IntegrityViolation)?;
                self.assignments.push(record);
                let task = self.summary(actor, self.item(*task_id)?);
                self.history.push(HistoryEntry {
                    kind: "claimed".into(),
                    occurred_at: now.into(),
                });
                Ok(MutationResult::Claimed { task })
            }
            Command::Assign {
                task_id,
                context,
                expected_attempt_id,
                assignee,
                assignee_responsibility_id,
                reason,
            } => {
                bounded_reason(reason)?;
                let item = self.item(*task_id)?;
                if item.attempt_id != *expected_attempt_id {
                    return Err(WorkError::RevisionConflict);
                }
                // The bound never strands an attempt: once its assignee's responsibility
                // has ended, reassignment stays possible (growth is then bounded by
                // the policy's own record limits).
                let current_effective =
                    item.assignee
                        .zip(item.acting_assignment_id)
                        .is_some_and(|(principal, id)| {
                            self.step_responsibility(principal, id, item, PolicyAction::WorkRead)
                                .is_some()
                        });
                if current_effective
                    && self
                        .assignments
                        .iter()
                        .filter(|record| record.attempt_id == item.attempt_id)
                        .count()
                        >= MAX_ASSIGNMENTS_PER_ATTEMPT
                {
                    return Err(WorkError::ValidationFailed);
                }
                if item.state == TaskState::Completed {
                    return Err(WorkError::HandoffNotReady);
                }
                // The new assignee must be currently eligible for this exact step.
                if self
                    .step_responsibility(
                        *assignee,
                        *assignee_responsibility_id,
                        item,
                        PolicyAction::WorkClaim,
                    )
                    .is_none()
                {
                    return Err(WorkError::ValidationFailed);
                }
                if item.assignee == Some(*assignee)
                    && item.acting_assignment_id == Some(*assignee_responsibility_id)
                {
                    return Err(WorkError::ValidationFailed);
                }
                let previous = item.work_assignment_id;
                let record = WorkAssignmentRecord {
                    id: Uuid::now_v7(),
                    task_id: item.id,
                    attempt_id: item.attempt_id,
                    principal: *assignee,
                    acting_assignment_id: *assignee_responsibility_id,
                    assigned_by: Some(actor),
                    manager_assignment_id: Some(context.acting_assignment_id),
                    reason: Some(reason.clone()),
                    started_at: now.into(),
                    ended_at: None,
                    ended_by: None,
                };
                // End the previous responsibility period; legacy claims have no record.
                if let Some(open) = self
                    .assignments
                    .iter_mut()
                    .find(|value| Some(value.id) == previous && value.ended_at.is_none())
                {
                    open.ended_at = Some(now.into());
                    open.ended_by = Some(actor);
                }
                let target = if *task_id == self.source.id {
                    &mut self.source
                } else {
                    self.next.as_mut().ok_or(WorkError::WorkItemNotFound)?
                };
                if target.state == TaskState::Ready {
                    target.state = TaskState::Active;
                }
                target.assignee = Some(*assignee);
                target.work_assignment_id = Some(record.id);
                target.acting_assignment_id = Some(record.acting_assignment_id);
                target.revision = target
                    .revision
                    .checked_add(1)
                    .ok_or(WorkError::IntegrityViolation)?;
                self.assignments.push(record.clone());
                self.history.push(HistoryEntry {
                    kind: "assigned".into(),
                    occurred_at: now.into(),
                });
                Ok(MutationResult::Assigned {
                    task: self.summary(actor, self.item(*task_id)?),
                    assignment: record,
                })
            }
            Command::Submit {
                task_id,
                artifacts,
                expected_attempt_id,
                evidence_revision_refs,
                finding_revision_refs,
                decision_revision_refs,
                ..
            } => {
                if expected_attempt_id.is_some_and(|id| id != self.source.attempt_id) {
                    return Err(WorkError::RevisionConflict);
                }
                self.validate_selection(
                    actor,
                    *task_id,
                    evidence_revision_refs,
                    finding_revision_refs,
                    decision_revision_refs,
                )?;
                if *task_id != self.source.id
                    || self.source.state != TaskState::Active
                    || self
                        .next
                        .as_ref()
                        .is_some_and(|item| item.state != TaskState::Completed)
                    || (self.next.is_some()
                        && !matches!(
                            self.definition_version_id,
                            RETURN_DEFINITION_VERSION_ID
                                | COMPLETE_DEFINITION_VERSION_ID
                                | HOLD_RESUME_DEFINITION_VERSION_ID
                        ))
                {
                    return Err(WorkError::HandoffNotReady);
                }
                if artifacts.is_empty() || artifacts.len() > MAX_ARTIFACTS {
                    return Err(WorkError::HandoffNotReady);
                }
                let mut pinned = vec![];
                let mut seen = std::collections::BTreeSet::new();
                for selected in artifacts {
                    if !seen.insert(selected.artifact_id) {
                        return Err(WorkError::ValidationFailed);
                    }
                    let artifact = self.artifact(actor, selected.artifact_id)?;
                    if artifact.revision != selected.revision {
                        return Err(WorkError::RevisionConflict);
                    }
                    if artifact.task_id != self.source.id
                        || artifact.attempt_id != self.source.attempt_id
                        || artifact.schema_id != TEXT_SCHEMA_ID
                    {
                        return Err(WorkError::HandoffNotReady);
                    }
                    pinned.push(PinnedArtifact {
                        artifact_id: artifact.id,
                        revision: artifact.revision,
                        schema_id: artifact.schema_id,
                        value: artifact.value,
                    });
                }
                let snapshot = HandoffSnapshot {
                    evidence_revision_refs: evidence_revision_refs.clone(),
                    finding_revision_refs: finding_revision_refs.clone(),
                    decision_revision_refs: decision_revision_refs.clone(),
                    previous_submission_id: self
                        .source
                        .return_instruction_id
                        .and(self.source.handoff_snapshot_id),
                    return_instruction_id: self.source.return_instruction_id,
                    id: Uuid::now_v7(),
                    workflow_id: self.id,
                    context_id: self.context_id,
                    submission_number: u32::try_from(self.snapshots.len())
                        .map_err(|_| WorkError::IntegrityViolation)?
                        .checked_add(1)
                        .ok_or(WorkError::IntegrityViolation)?,
                    submitted_by: actor.principal_id().into(),
                    acting_assignment_id: command.context().acting_assignment_id,
                    source_task_id: self.source.id,
                    source_attempt_id: self.source.attempt_id,
                    target_task_id: OFFICE_TASK_ID,
                    created_at: now.into(),
                    artifacts: pinned,
                };
                self.source.state = TaskState::Completed;
                self.source.completed_at = Some(now.into());
                self.source.revision = self
                    .source
                    .revision
                    .checked_add(1)
                    .ok_or(WorkError::IntegrityViolation)?;
                self.source.handoff_snapshot_id = Some(snapshot.id);
                let (attempt_number, revision, attempt_id) = if let Some(previous) = &self.next {
                    self.completed_attempts.push(previous.clone());
                    (
                        previous
                            .attempt_number
                            .checked_add(1)
                            .ok_or(WorkError::IntegrityViolation)?,
                        previous
                            .revision
                            .checked_add(1)
                            .ok_or(WorkError::IntegrityViolation)?,
                        Uuid::now_v7(),
                    )
                } else {
                    (1, 0, OFFICE_ATTEMPT_ID)
                };
                let next = WorkItem {
                    return_instruction_id: self.source.return_instruction_id,
                    id: OFFICE_TASK_ID,
                    workflow_instance_id: WORKFLOW_ID,
                    step_id: OFFICE_STEP_ID,
                    work_type_id: OFFICE_WORK_TYPE_ID,
                    attempt_number,
                    work_assignment_id: None,
                    acting_assignment_id: None,
                    created_at: now.into(),
                    completed_at: None,
                    attempt_id,
                    revision,
                    state: TaskState::Ready,
                    assignee: None,
                    handoff_snapshot_id: Some(snapshot.id),
                };
                let next_task = self.summary(actor, &next);
                self.next = Some(next);
                self.snapshots.push(snapshot.clone());
                self.history.push(HistoryEntry {
                    kind: "submitted".into(),
                    occurred_at: now.into(),
                });
                Ok(MutationResult::Submitted {
                    task: self.summary(actor, &self.source),
                    snapshot,
                    next_task,
                })
            }
        }
    }
}
