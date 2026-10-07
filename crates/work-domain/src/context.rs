//! Synthetic WorkContexts (versioned fixture config, Domain §3/§13), derived
//! Attention (§10) and WorkViewProfile presentation over the same records.
//! Nothing here widens authorization: every projection reuses the U1 checks.
use super::*;
use time::{Duration, OffsetDateTime};

pub const REVIEW_STEP_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b011);
pub const REVIEW_WORK_TYPE_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b012);
/// Sales → review: forward submit, return, completion and hold/resume.
pub const REVIEW_DEFINITION_VERSION_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b013);
pub const CONTEXT_B_WORKFLOW_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b001);
pub const CONTEXT_B_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b002);
pub const CONTEXT_B_SALES_TASK_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b003);
pub const CONTEXT_B_REVIEW_TASK_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b004);
pub const CONTEXT_C_WORKFLOW_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b101);
pub const CONTEXT_C_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b102);
pub const CONTEXT_C_SALES_TASK_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b103);
pub const CONTEXT_C_OFFICE_TASK_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000b104);
pub const PROFILE_SALES_CONTEXT_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000c001);
pub const PROFILE_OFFICE_QUEUE_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000c002);
pub const PROFILE_REVIEW_QUEUE_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000c003);
/// The PoC reads every instance for one projection; this bound keeps that honest.
pub const MAX_WORK_CONTEXTS: usize = 32;
/// Synthetic WorkType advance notice; not an organization-wide deadline rule.
const SYNTHETIC_DUE_LEAD: Duration = Duration::hours(24);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkContextKind {
    Case,
    RoutineRun,
    Batch,
    Request,
}
/// How the fixture's first sales attempt starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStart {
    /// The existing fixture: active and assigned to sales-01 (unchanged JSON).
    AssignedToSales,
    /// Ready for an eligible claim, with an explicit due instant relative to seeding.
    ReadyDueInMinutes(i64),
}
/// One synthetic WorkContext bound to exactly one two-step workflow instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextFixture {
    pub workflow_id: Uuid,
    pub context_id: Uuid,
    pub kind: WorkContextKind,
    pub title: &'static str,
    pub owner_unit_id: Uuid,
    pub definition_version_id: Uuid,
    /// Definition versions a stored instance of this fixture may carry.
    pub accepted_definitions: &'static [Uuid],
    pub source_task_id: Uuid,
    pub source_attempt_id: Uuid,
    pub next_task_id: Uuid,
    pub next_attempt_id: Uuid,
    pub next_step_id: Uuid,
    pub next_work_type_id: Uuid,
    pub start: SourceStart,
}
pub const CONTEXT_FIXTURES: [ContextFixture; 3] = [
    ContextFixture {
        workflow_id: WORKFLOW_ID,
        context_id: CONTEXT_ID,
        kind: WorkContextKind::Case,
        title: "合成案件A・設備更新相談",
        owner_unit_id: UNIT_SALES_ID,
        definition_version_id: HOLD_RESUME_DEFINITION_VERSION_ID,
        accepted_definitions: &[
            DEFINITION_VERSION_ID,
            RETURN_DEFINITION_VERSION_ID,
            COMPLETE_DEFINITION_VERSION_ID,
            HOLD_RESUME_DEFINITION_VERSION_ID,
        ],
        source_task_id: SALES_TASK_ID,
        source_attempt_id: SALES_ATTEMPT_ID,
        next_task_id: OFFICE_TASK_ID,
        next_attempt_id: OFFICE_ATTEMPT_ID,
        next_step_id: OFFICE_STEP_ID,
        next_work_type_id: OFFICE_WORK_TYPE_ID,
        start: SourceStart::AssignedToSales,
    },
    ContextFixture {
        workflow_id: CONTEXT_B_WORKFLOW_ID,
        context_id: CONTEXT_B_ID,
        kind: WorkContextKind::Case,
        title: "合成案件B・運転資金相談",
        owner_unit_id: UNIT_SALES_ID,
        definition_version_id: REVIEW_DEFINITION_VERSION_ID,
        accepted_definitions: &[REVIEW_DEFINITION_VERSION_ID],
        source_task_id: CONTEXT_B_SALES_TASK_ID,
        source_attempt_id: Uuid::from_u128(0x0190000000007000800000000000b005),
        next_task_id: CONTEXT_B_REVIEW_TASK_ID,
        next_attempt_id: Uuid::from_u128(0x0190000000007000800000000000b006),
        next_step_id: REVIEW_STEP_ID,
        next_work_type_id: REVIEW_WORK_TYPE_ID,
        start: SourceStart::ReadyDueInMinutes(6 * 60),
    },
    ContextFixture {
        workflow_id: CONTEXT_C_WORKFLOW_ID,
        context_id: CONTEXT_C_ID,
        kind: WorkContextKind::Request,
        title: "合成依頼C・住所変更届",
        owner_unit_id: UNIT_SALES_ID,
        definition_version_id: HOLD_RESUME_DEFINITION_VERSION_ID,
        accepted_definitions: &[HOLD_RESUME_DEFINITION_VERSION_ID],
        source_task_id: CONTEXT_C_SALES_TASK_ID,
        source_attempt_id: Uuid::from_u128(0x0190000000007000800000000000b105),
        next_task_id: CONTEXT_C_OFFICE_TASK_ID,
        next_attempt_id: Uuid::from_u128(0x0190000000007000800000000000b106),
        next_step_id: OFFICE_STEP_ID,
        next_work_type_id: OFFICE_WORK_TYPE_ID,
        start: SourceStart::ReadyDueInMinutes(-60),
    },
];
pub fn context_fixture(workflow_id: Uuid) -> Option<&'static ContextFixture> {
    CONTEXT_FIXTURES
        .iter()
        .find(|value| value.workflow_id == workflow_id)
}
pub fn context_fixture_for_context(context_id: Uuid) -> Option<&'static ContextFixture> {
    CONTEXT_FIXTURES
        .iter()
        .find(|value| value.context_id == context_id)
}
/// Generic step label; it never discloses a customer or a private draft.
pub fn step_label(step_id: Uuid) -> &'static str {
    match step_id {
        SALES_STEP_ID => "営業内容整理",
        OFFICE_STEP_ID => "事務内容確認",
        REVIEW_STEP_ID => "審査内容確認",
        _ => "工程",
    }
}
pub fn work_type_label(work_type_id: Uuid) -> &'static str {
    match work_type_id {
        SALES_WORK_TYPE_ID => "営業内容整理",
        OFFICE_WORK_TYPE_ID => "事務内容確認",
        REVIEW_WORK_TYPE_ID => "審査内容確認",
        _ => "業務",
    }
}
/// Explicit WorkType policy for `due_soon`; absent means no due-soon attention.
pub fn work_type_due_lead(work_type_id: Uuid) -> Option<Duration> {
    matches!(
        work_type_id,
        SALES_WORK_TYPE_ID | OFFICE_WORK_TYPE_ID | REVIEW_WORK_TYPE_ID
    )
    .then_some(SYNTHETIC_DUE_LEAD)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileGrouping {
    Context,
    WorkType,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextModule {
    Document,
    History,
    Evidence,
    Agent,
    Search,
    Resources,
    Return,
}
/// Presentation state only; never an authorization state (Product §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulePresentation {
    Hidden,
    Available,
    Visible,
    Prominent,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModulePriority {
    pub module: ContextModule,
    pub presentation: ModulePresentation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkViewProfile {
    pub id: Uuid,
    pub key: String,
    pub label: String,
    pub archetype: WorkViewArchetype,
    pub primary_grouping: ProfileGrouping,
    pub default_sort: String,
    pub initial_module: ContextModule,
    pub modules: Vec<ModulePriority>,
}
pub fn work_view_profiles() -> Vec<WorkViewProfile> {
    use ContextModule::*;
    use ModulePresentation::*;
    let profile =
        |id,
         key: &str,
         label: &str,
         archetype,
         grouping,
         initial,
         modules: &[(ContextModule, ModulePresentation)]| WorkViewProfile {
            id,
            key: key.into(),
            label: label.into(),
            archetype,
            primary_grouping: grouping,
            default_sort: "due_at".into(),
            initial_module: initial,
            modules: modules
                .iter()
                .map(|(module, presentation)| ModulePriority {
                    module: *module,
                    presentation: *presentation,
                })
                .collect(),
        };
    vec![
        profile(
            PROFILE_SALES_CONTEXT_ID,
            "sales-context",
            "営業・文脈",
            WorkViewArchetype::Context,
            ProfileGrouping::Context,
            Document,
            &[
                (Document, Prominent),
                (History, Prominent),
                (Evidence, Available),
                (Agent, Available),
                (Return, Available),
                (Search, Hidden),
                (Resources, Hidden),
            ],
        ),
        profile(
            PROFILE_OFFICE_QUEUE_ID,
            "office-queue",
            "事務・キュー",
            WorkViewArchetype::Queue,
            ProfileGrouping::WorkType,
            Document,
            &[
                (Document, Visible),
                (Evidence, Available),
                (History, Available),
                (Agent, Available),
                (Return, Available),
                (Search, Hidden),
                (Resources, Hidden),
            ],
        ),
        profile(
            PROFILE_REVIEW_QUEUE_ID,
            "review-queue",
            "審査・キュー",
            WorkViewArchetype::Queue,
            ProfileGrouping::WorkType,
            Evidence,
            &[
                (Evidence, Prominent),
                (Document, Prominent),
                (History, Available),
                (Agent, Available),
                (Return, Available),
                (Search, Hidden),
                (Resources, Hidden),
            ],
        ),
    ]
}
/// Role override first, then the unit default; presentation only.
pub fn profile_for(role_id: Uuid, unit_id: Uuid) -> Uuid {
    if role_id == ROLE_REVIEWING_ID {
        return PROFILE_REVIEW_QUEUE_ID;
    }
    match unit_id {
        UNIT_SALES_ID => PROFILE_SALES_CONTEXT_ID,
        UNIT_REVIEW_ID => PROFILE_REVIEW_QUEUE_ID,
        _ => PROFILE_OFFICE_QUEUE_ID,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    NewlyAssigned,
    Returned,
    DueSoon,
    Overdue,
}
/// Derived from an authoritative record; never a lifecycle state. The source
/// identifier is disclosed only to the current assignee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub kind: AttentionKind,
    pub source_id: Option<Uuid>,
    pub due_at: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAttention {
    pub task_id: Uuid,
    pub attempt_id: Uuid,
    pub evaluated_at: String,
    pub items: Vec<Attention>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionAcknowledgement {
    pub task_id: Uuid,
    pub attempt_id: Uuid,
    pub work_assignment_id: Uuid,
    pub principal_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextProgressItem {
    pub task_id: Uuid,
    pub step_label: String,
    pub work_type_id: Uuid,
    pub state: TaskState,
    pub attempt_number: u32,
    pub due_at: Option<String>,
    pub assigned: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkContextView {
    pub id: Uuid,
    pub kind: WorkContextKind,
    pub title: String,
    pub owner_unit_id: Uuid,
    /// Null unless `context.progress.read` holds for the owner unit.
    pub progress: Option<Vec<ContextProgressItem>>,
    pub can_read_history: bool,
    /// The actor's own current tasks in this context.
    pub own_task_ids: Vec<Uuid>,
    pub attention_count: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkContextHistory {
    pub context_id: Uuid,
    pub entries: Vec<HistoryEntry>,
}
/// A record identity a repository resolves to its owning workflow instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkTarget {
    Task(Uuid),
    Artifact(Uuid),
    Snapshot(Uuid),
    ReturnInstruction(Uuid),
    Evidence(Uuid),
    Finding(Uuid),
    AgentExecution(Uuid),
    Context(Uuid),
}

impl Workflow {
    /// A fresh fixture instance. The existing context keeps its exact legacy JSON.
    pub fn from_fixture(
        fixture: &ContextFixture,
        document_id: Option<Uuid>,
        seeded_at: OffsetDateTime,
    ) -> Result<Self, WorkError> {
        let mut workflow = Self::synthetic(document_id);
        if fixture.workflow_id == WORKFLOW_ID {
            return Ok(workflow);
        }
        workflow.id = fixture.workflow_id;
        workflow.context_id = fixture.context_id;
        workflow.definition_version_id = fixture.definition_version_id;
        let item = &mut workflow.source;
        item.id = fixture.source_task_id;
        item.workflow_instance_id = fixture.workflow_id;
        item.attempt_id = fixture.source_attempt_id;
        item.created_at = format_instant(seeded_at)?;
        if let SourceStart::ReadyDueInMinutes(minutes) = fixture.start {
            item.state = TaskState::Ready;
            item.assignee = None;
            item.acting_assignment_id = None;
            item.work_assignment_id = None;
            item.due_at = Some(format_instant(seeded_at + Duration::minutes(minutes))?);
        }
        workflow.validate_integrity()?;
        Ok(workflow)
    }
    pub(crate) fn plan(&self) -> Result<&'static ContextFixture, WorkError> {
        context_fixture(self.id).ok_or(WorkError::IntegrityViolation)
    }
    /// The verified actor's acknowledged assignment periods for this evaluation.
    pub fn attach_acknowledgements(
        &mut self,
        actor: VerifiedActor,
        ids: std::collections::BTreeSet<Uuid>,
    ) {
        self.authority.acknowledged = Some(std::sync::Arc::new((actor, ids)));
    }
    fn acknowledged(&self, actor: VerifiedActor, id: Uuid) -> bool {
        self.authority
            .acknowledged
            .as_deref()
            .is_some_and(|(owner, ids)| *owner == actor && ids.contains(&id))
    }
    fn current_items(&self) -> impl Iterator<Item = &WorkItem> {
        std::iter::once(&self.source).chain(self.next.iter())
    }
    pub(crate) fn attention_for(&self, actor: VerifiedActor, item: &WorkItem) -> Vec<Attention> {
        let mut items = vec![];
        if item.state == TaskState::Completed {
            return items;
        }
        let readable = self.can_read(actor, item);
        if readable
            && let Some(record) = item
                .work_assignment_id
                .and_then(|id| self.assignments.iter().find(|value| value.id == id))
            && record.assigned_by.is_some_and(|by| by != actor)
            && record.principal == actor
            && !self.acknowledged(actor, record.id)
        {
            items.push(Attention {
                kind: AttentionKind::NewlyAssigned,
                source_id: Some(record.id),
                due_at: None,
            });
        }
        if let Some(id) = item.return_instruction_id {
            items.push(Attention {
                kind: AttentionKind::Returned,
                source_id: readable.then_some(id),
                due_at: None,
            });
        }
        if let Some(due) = item.due_at.as_deref()
            && let Some(due_instant) = stored(due)
        {
            let now = self.evaluated_at();
            let kind = if now >= due_instant {
                Some(AttentionKind::Overdue)
            } else {
                work_type_due_lead(item.work_type_id)
                    .filter(|lead| now >= due_instant - *lead)
                    .map(|_| AttentionKind::DueSoon)
            };
            if let Some(kind) = kind {
                items.push(Attention {
                    kind,
                    source_id: None,
                    due_at: Some(due.into()),
                });
            }
        }
        items
    }
    /// Same visibility as the list row; not a separate disclosure path.
    pub fn attention(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
    ) -> Result<TaskAttention, WorkError> {
        let item = self.item(task_id)?;
        if !self.visible(actor, item) {
            return Err(WorkError::WorkItemNotFound);
        }
        Ok(TaskAttention {
            task_id: item.id,
            attempt_id: item.attempt_id,
            evaluated_at: format_instant(self.evaluated_at())?,
            items: self.attention_for(actor, item),
        })
    }
    /// Validates an acknowledgment of the actor's own current assignment period.
    /// It records presentation state only: no Work revision, ledger or staging.
    pub fn acknowledge(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
        work_assignment_id: Uuid,
    ) -> Result<AttentionAcknowledgement, WorkError> {
        let item = self.item(task_id)?;
        if !self.can_read(actor, item) {
            return Err(WorkError::WorkItemNotFound);
        }
        if item.state == TaskState::Completed || item.work_assignment_id != Some(work_assignment_id)
        {
            return Err(WorkError::ValidationFailed);
        }
        Ok(AttentionAcknowledgement {
            task_id: item.id,
            attempt_id: item.attempt_id,
            work_assignment_id,
            principal_id: actor.principal_id().into(),
        })
    }
    /// Responsibilities that hold `action` over this context's owner unit.
    fn context_grant(
        &self,
        actor: VerifiedActor,
        action: PolicyAction,
        scope: Option<Uuid>,
    ) -> bool {
        let Ok(plan) = self.plan() else {
            return false;
        };
        self.policy()
            .responsibilities(actor, self.evaluated_at())
            .iter()
            .any(|value| {
                scope.is_none_or(|id| value.id == id)
                    && value.unit_id == plan.owner_unit_id
                    && value.allows(action)
            })
    }
    fn assignee_in(&self, actor: VerifiedActor, scope: Option<Uuid>) -> bool {
        self.current_items().any(|item| {
            self.can_read(actor, item)
                && scope.is_none_or(|id| item.acting_assignment_id == Some(id))
        })
    }
    pub(crate) fn context_title_for(
        &self,
        actor: VerifiedActor,
        scope: Option<Uuid>,
    ) -> Option<String> {
        (self.context_grant(actor, PolicyAction::ContextRead, scope)
            || self.assignee_in(actor, scope))
        .then(|| self.plan().ok().map(|plan| plan.title.to_string()))
        .flatten()
    }
    /// Authorized context projection; `None` hides the context's existence.
    pub fn context_view(
        &self,
        actor: VerifiedActor,
        scope: Option<Uuid>,
    ) -> Option<WorkContextView> {
        let plan = self.plan().ok()?;
        let title = self.context_title_for(actor, scope)?;
        let progress = self
            .context_grant(actor, PolicyAction::ContextProgressRead, scope)
            .then(|| {
                self.current_items()
                    .map(|item| ContextProgressItem {
                        task_id: item.id,
                        step_label: step_label(item.step_id).into(),
                        work_type_id: item.work_type_id,
                        state: item.state,
                        attempt_number: item.attempt_number,
                        due_at: item.due_at.clone(),
                        assigned: item.assignee.is_some(),
                    })
                    .collect()
            });
        let own_task_ids = self
            .current_items()
            .filter(|item| {
                self.can_read(actor, item)
                    && scope.is_none_or(|id| item.acting_assignment_id == Some(id))
            })
            .map(|item| item.id)
            .collect();
        let attention_count = self
            .current_items()
            .filter(|item| self.visible(actor, item))
            .map(|item| self.attention_for(actor, item).len())
            .sum::<usize>();
        Some(WorkContextView {
            id: self.context_id,
            kind: plan.kind,
            title,
            owner_unit_id: plan.owner_unit_id,
            progress,
            can_read_history: self.context_grant(actor, PolicyAction::ContextHistoryRead, scope),
            own_task_ids,
            attention_count: u32::try_from(attention_count).unwrap_or(u32::MAX),
        })
    }
    /// Closed progress history: kinds and instants only, never reasons or bodies.
    pub fn context_history(&self, actor: VerifiedActor) -> Result<WorkContextHistory, WorkError> {
        if self.context_view(actor, None).is_none() {
            return Err(WorkError::WorkContextNotFound);
        }
        if !self.context_grant(actor, PolicyAction::ContextHistoryRead, None) {
            return Err(WorkError::Forbidden);
        }
        Ok(WorkContextHistory {
            context_id: self.context_id,
            entries: self.history.clone(),
        })
    }
    /// Whether this instance stores the identified record.
    pub fn owns(&self, target: WorkTarget) -> bool {
        match target {
            WorkTarget::Task(id) => {
                self.current_items().any(|item| item.id == id)
                    || self.plan().is_ok_and(|plan| plan.next_task_id == id)
            }
            WorkTarget::Artifact(id) => self.artifacts.iter().any(|value| value.id == id),
            WorkTarget::Snapshot(id) => self.snapshots.iter().any(|value| value.id == id),
            WorkTarget::ReturnInstruction(id) => {
                self.return_instructions.iter().any(|value| value.id == id)
            }
            WorkTarget::Evidence(id) => self.evidence.iter().any(|value| value.id == id),
            WorkTarget::Finding(id) => self.findings.iter().any(|value| value.id == id),
            WorkTarget::AgentExecution(id) => {
                self.agent_executions.iter().any(|value| value.id == id)
            }
            WorkTarget::Context(id) => self.context_id == id,
        }
    }
}
fn stored(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
}
