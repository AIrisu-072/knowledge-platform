#![forbid(unsafe_code)]
//! Work-owned synthetic workflow values. No Document authority or infrastructure.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const WORKFLOW_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000001);
pub const CONTEXT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000002);
pub const SALES_TASK_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000003);
pub const OFFICE_TASK_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000004);
pub const SALES_ATTEMPT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000005);
pub const OFFICE_ATTEMPT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000006);
pub const SALES_ASSIGNMENT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000007);
pub const OFFICE_ASSIGNMENT_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000008);
pub const DEFINITION_VERSION_ID: Uuid = Uuid::from_u128(0x01900000000070008000000000000009);
pub const SALES_STEP_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000a);
pub const OFFICE_STEP_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000b);
pub const SALES_WORK_TYPE_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000c);
pub const OFFICE_WORK_TYPE_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000d);
pub const SALES_WORK_ASSIGNMENT_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000000e);
const FIXTURE_CREATED_AT: &str = "2026-10-04T00:00:00Z";
pub const TEXT_SCHEMA_ID: &str = "organization.text-draft.v1";
pub const MAX_TEXT_BYTES: usize = 8 * 1024;
pub const MAX_ARTIFACTS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerifiedActor {
    Sales01,
    Office01,
}
impl VerifiedActor {
    pub fn from_startup_profile(profile: &str) -> Result<Self, WorkError> {
        match profile {
            "sales-01" => Ok(Self::Sales01),
            "office-01" => Ok(Self::Office01),
            _ => Err(WorkError::Forbidden),
        }
    }
    pub fn principal_id(self) -> &'static str {
        match self {
            Self::Sales01 => "sales-01",
            Self::Office01 => "office-01",
        }
    }
    pub fn assignment_id(self) -> Uuid {
        match self {
            Self::Sales01 => SALES_ASSIGNMENT_ID,
            Self::Office01 => OFFICE_ASSIGNMENT_ID,
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
    #[error("CURSOR_STALE")]
    CursorStale,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Ready,
    Active,
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
pub struct TaskSummary {
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
    #[serde(flatten)]
    pub task: TaskSummary,
    pub input_resources: Vec<InputResourceRef>,
    pub history: Vec<HistoryEntry>,
    pub working_artifacts: Vec<WorkingArtifact>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactSelection {
    pub artifact_id: Uuid,
    pub revision: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
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
    },
}
impl Command {
    pub fn context(&self) -> &CommandContext {
        match self {
            Self::SaveDraft { context, .. }
            | Self::Claim { context, .. }
            | Self::Submit { context, .. } => context,
        }
    }
    pub fn task_id(&self) -> Uuid {
        match self {
            Self::SaveDraft { task_id, .. }
            | Self::Claim { task_id, .. }
            | Self::Submit { task_id, .. } => *task_id,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationResult {
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
}
impl Workflow {
    pub fn synthetic(document_id: Option<Uuid>) -> Self {
        Self {
            id: WORKFLOW_ID,
            definition_version_id: DEFINITION_VERSION_ID,
            context_id: CONTEXT_ID,
            revision: 0,
            source: WorkItem {
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
    fn can_read(&self, actor: VerifiedActor, item: &WorkItem) -> bool {
        item.assignee == Some(actor)
    }
    fn summary(&self, actor: VerifiedActor, item: &WorkItem) -> TaskSummary {
        let source = item.id == self.source.id;
        let editable = source && self.can_read(actor, item) && item.state == TaskState::Active;
        let label = if source {
            "営業内容整理"
        } else {
            "事務内容確認"
        };
        TaskSummary {
            id: item.id,
            context_id: self.context_id,
            attempt_id: item.attempt_id,
            revision: item.revision,
            title: label.into(),
            step_label: label.into(),
            state: item.state,
            can_claim: !source
                && actor == VerifiedActor::Office01
                && item.state == TaskState::Ready,
            can_edit: editable,
            can_submit: editable && !self.artifacts.is_empty(),
            handoff_snapshot_id: item.handoff_snapshot_id,
        }
    }
    pub fn list_tasks(&self, actor: VerifiedActor, view: TaskView) -> Vec<TaskSummary> {
        let mut items = vec![];
        if actor == VerifiedActor::Sales01 {
            items.push(self.summary(actor, &self.source));
        }
        if let Some(next) = &self.next
            && (actor == VerifiedActor::Office01
                || (actor == VerifiedActor::Sales01 && view == TaskView::Context))
        {
            items.push(self.summary(actor, next));
        }
        items
    }
    pub fn detail(&self, actor: VerifiedActor, id: Uuid) -> Result<TaskDetail, WorkError> {
        let item = self.item(id)?;
        if !self.can_read(actor, item) {
            return Err(WorkError::WorkItemNotFound);
        }
        Ok(TaskDetail {
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
        let source_readable = self.source.assignee == Some(actor);
        let target_readable = self.next.as_ref().is_some_and(|next| {
            next.assignee == Some(actor) && next.handoff_snapshot_id == Some(id)
        });
        if !source_readable && !target_readable {
            return Err(WorkError::WorkArtifactNotFound);
        }
        Ok(snapshot.clone())
    }
    pub fn authorize_command(
        &self,
        actor: VerifiedActor,
        command: &Command,
    ) -> Result<(), WorkError> {
        let context = command.context();
        if context.operation_id.get_version_num() != 7 || context.expected_revision < 0 {
            return Err(WorkError::ValidationFailed);
        }
        if context.acting_assignment_id != actor.assignment_id() {
            return Err(WorkError::Forbidden);
        }
        let item = self.item(command.task_id())?;
        match command {
            Command::Claim { .. } => {
                if item.id != OFFICE_TASK_ID || actor != VerifiedActor::Office01 {
                    return Err(WorkError::WorkItemNotFound);
                }
            }
            Command::SaveDraft { artifact_id, .. } => {
                if !self.can_read(actor, item) {
                    return Err(WorkError::WorkItemNotFound);
                }
                if let Some(id) = artifact_id {
                    let artifact = self.artifact(actor, *id)?;
                    if artifact.task_id != item.id {
                        return Err(WorkError::WorkArtifactNotFound);
                    }
                }
            }
            Command::Submit { .. } => {
                if !self.can_read(actor, item) {
                    return Err(WorkError::WorkItemNotFound);
                }
            }
        }
        Ok(())
    }
    pub fn authorize_recovery(
        &self,
        actor: VerifiedActor,
        result: &MutationResult,
    ) -> Result<(), WorkError> {
        match result {
            MutationResult::DraftSaved { artifact, .. } => {
                self.artifact(actor, artifact.id)?;
            }
            MutationResult::Claimed { task } => {
                self.detail(actor, task.id)?;
            }
            MutationResult::Submitted { snapshot, .. } => {
                self.snapshot(actor, snapshot.id)?;
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
        self.authorize_command(actor, command)?;
        let item = self.item(command.task_id())?;
        if item.revision != command.context().expected_revision {
            return Err(WorkError::RevisionConflict);
        }
        // Validate against a private copy so every failed command leaves the aggregate untouched.
        let mut next = self.clone();
        let result = next.apply_validated(actor, command, now)?;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(WorkError::IntegrityViolation)?;
        *self = next;
        Ok(result)
    }
    fn apply_validated(
        &mut self,
        actor: VerifiedActor,
        command: &Command,
        now: &str,
    ) -> Result<MutationResult, WorkError> {
        match command {
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
                    if self.artifacts.len() >= MAX_ARTIFACTS {
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
            Command::Claim { .. } => {
                let next = self.next.as_mut().ok_or(WorkError::WorkItemNotFound)?;
                if next.state != TaskState::Ready || next.assignee.is_some() {
                    return Err(WorkError::WorkAssignmentConflict);
                }
                next.state = TaskState::Active;
                next.assignee = Some(actor);
                next.work_assignment_id = Some(Uuid::now_v7());
                next.acting_assignment_id = Some(actor.assignment_id());
                next.revision = next
                    .revision
                    .checked_add(1)
                    .ok_or(WorkError::IntegrityViolation)?;
                let task = self.summary(
                    actor,
                    self.next.as_ref().ok_or(WorkError::IntegrityViolation)?,
                );
                self.history.push(HistoryEntry {
                    kind: "claimed".into(),
                    occurred_at: now.into(),
                });
                Ok(MutationResult::Claimed { task })
            }
            Command::Submit {
                task_id, artifacts, ..
            } => {
                if *task_id != self.source.id
                    || self.source.state != TaskState::Active
                    || self.next.is_some()
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
                    id: Uuid::now_v7(),
                    workflow_id: self.id,
                    context_id: self.context_id,
                    submission_number: 1,
                    submitted_by: actor.principal_id().into(),
                    acting_assignment_id: actor.assignment_id(),
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
                let next = WorkItem {
                    id: OFFICE_TASK_ID,
                    workflow_instance_id: WORKFLOW_ID,
                    step_id: OFFICE_STEP_ID,
                    work_type_id: OFFICE_WORK_TYPE_ID,
                    attempt_number: 1,
                    work_assignment_id: None,
                    acting_assignment_id: None,
                    created_at: now.into(),
                    completed_at: None,
                    attempt_id: OFFICE_ATTEMPT_ID,
                    revision: 0,
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
