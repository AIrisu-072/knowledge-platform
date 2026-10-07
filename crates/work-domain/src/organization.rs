//! Organization policy authority: units, business roles, formal role assignments
//! and bounded delegation. Synthetic fixture values only; no directory mapping.
use super::*;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub const ORGANIZATION_POLICY_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a001);
pub const UNIT_SALES_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a011);
pub const UNIT_OFFICE_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a012);
pub const UNIT_REVIEW_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a013);
pub const UNIT_APPROVAL_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a014);
pub const ROLE_SALES_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a021);
pub const ROLE_PROCESSING_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a022);
pub const ROLE_REVIEWING_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a023);
pub const ROLE_APPROVING_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a024);
pub const ROLE_MANAGEMENT_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a025);
pub const REVIEW_ASSIGNMENT_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a031);
pub const APPROVER_ASSIGNMENT_ID: Uuid = Uuid::from_u128(0x0190000000007000800000000000a032);
pub const APPROVER_MANAGEMENT_ASSIGNMENT_ID: Uuid =
    Uuid::from_u128(0x0190000000007000800000000000a033);
pub const MULTI_ROLE_PROCESSING_ASSIGNMENT_ID: Uuid =
    Uuid::from_u128(0x0190000000007000800000000000a034);
pub const MULTI_ROLE_REVIEW_ASSIGNMENT_ID: Uuid =
    Uuid::from_u128(0x0190000000007000800000000000a035);
/// Records are never deleted, so every bound counts revoked and ended records.
/// Only a current manager creates formal assignments.
pub const MAX_ROLE_ASSIGNMENTS: usize = 96;
/// Per delegator, so one holder cannot exhaust another principal's delegations.
pub const MAX_DELEGATIONS_PER_DELEGATOR: usize = 16;
pub const MAX_DELEGATIONS: usize = MAX_DELEGATIONS_PER_DELEGATOR * VerifiedActor::ALL.len();
/// A requested start may trail the server clock by this much; it is stored as now.
const VALID_FROM_SKEW: time::Duration = time::Duration::minutes(5);
const FIXTURE_VALID_FROM: &str = "2026-10-01T00:00:00Z";

/// The closed policy action vocabulary of Domain §5. A label never grants one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PolicyAction {
    #[serde(rename = "context.read")]
    ContextRead,
    #[serde(rename = "context.progress.read")]
    ContextProgressRead,
    #[serde(rename = "context.history.read")]
    ContextHistoryRead,
    #[serde(rename = "queue.read")]
    QueueRead,
    #[serde(rename = "work.read")]
    WorkRead,
    #[serde(rename = "work.claim")]
    WorkClaim,
    #[serde(rename = "work.assign")]
    WorkAssign,
    #[serde(rename = "work.edit")]
    WorkEdit,
    #[serde(rename = "work.submit")]
    WorkSubmit,
    #[serde(rename = "work.return")]
    WorkReturn,
    #[serde(rename = "work.complete")]
    WorkComplete,
    #[serde(rename = "work.hold")]
    WorkHold,
    #[serde(rename = "work.resume")]
    WorkResume,
    #[serde(rename = "evidence.register")]
    EvidenceRegister,
    #[serde(rename = "finding.register")]
    FindingRegister,
    #[serde(rename = "decision.record")]
    DecisionRecord,
    #[serde(rename = "agent.request")]
    AgentRequest,
    #[serde(rename = "organization.manage")]
    OrganizationManage,
}
impl PolicyAction {
    /// Privilege-administration actions are never delegable in v0.
    pub fn delegable(self) -> bool {
        !matches!(self, Self::OrganizationManage | Self::WorkAssign)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkViewArchetype {
    Context,
    Queue,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationalUnit {
    pub id: Uuid,
    pub label: String,
    pub default_archetype: WorkViewArchetype,
    pub role_ids: Vec<Uuid>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusinessRole {
    pub id: Uuid,
    pub key: String,
    pub label: String,
    pub actions: Vec<PolicyAction>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignment {
    pub id: Uuid,
    #[serde(with = "principal_serde")]
    pub principal: VerifiedActor,
    pub role_id: Uuid,
    pub unit_id: Uuid,
    pub valid_from: String,
    pub valid_until: Option<String>,
    pub reason: String,
    #[serde(with = "principal_serde::option")]
    pub created_by: Option<VerifiedActor>,
    pub created_at: String,
    pub revoked_at: Option<String>,
    #[serde(with = "principal_serde::option")]
    pub revoked_by: Option<VerifiedActor>,
    pub revoke_reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delegation {
    pub id: Uuid,
    pub source_assignment_id: Uuid,
    #[serde(with = "principal_serde")]
    pub delegator: VerifiedActor,
    #[serde(with = "principal_serde")]
    pub recipient: VerifiedActor,
    pub actions: Vec<PolicyAction>,
    pub valid_from: String,
    pub valid_until: String,
    pub reason: String,
    #[serde(with = "principal_serde")]
    pub created_by: VerifiedActor,
    pub created_at: String,
    pub revoked_at: Option<String>,
    #[serde(with = "principal_serde::option")]
    pub revoked_by: Option<VerifiedActor>,
    pub revoke_reason: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsibilityKind {
    RoleAssignment,
    Delegation,
}
/// A currently effective responsibility, derived at one evaluation instant.
/// It is an explanation for the verified actor, never a reusable grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Responsibility {
    pub id: Uuid,
    pub kind: ResponsibilityKind,
    #[serde(with = "principal_serde")]
    pub principal: VerifiedActor,
    pub role_id: Uuid,
    pub role_label: String,
    pub unit_id: Uuid,
    pub unit_label: String,
    pub actions: Vec<PolicyAction>,
    pub valid_from: String,
    pub valid_until: Option<String>,
    pub source_assignment_id: Option<Uuid>,
    #[serde(with = "principal_serde::option")]
    pub delegator: Option<VerifiedActor>,
    /// Presentation default derived from role override and unit; never a grant.
    #[serde(default)]
    pub work_view_profile_id: Uuid,
}
impl Responsibility {
    pub fn allows(&self, action: PolicyAction) -> bool {
        self.actions.contains(&action)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationPolicy {
    pub id: Uuid,
    pub revision: i64,
    pub units: Vec<OrganizationalUnit>,
    pub roles: Vec<BusinessRole>,
    pub role_assignments: Vec<RoleAssignment>,
    pub delegations: Vec<Delegation>,
}
/// Bounded read model for one verified actor at one evaluation instant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationView {
    pub principal_id: String,
    pub display_name: String,
    pub policy_revision: i64,
    pub evaluated_at: String,
    pub can_manage: bool,
    pub responsibilities: Vec<Responsibility>,
    pub units: Vec<OrganizationalUnit>,
    pub roles: Vec<BusinessRole>,
    pub role_assignments: Vec<RoleAssignment>,
    pub delegations: Vec<Delegation>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PolicyCommand {
    CreateRoleAssignment {
        context: CommandContext,
        #[serde(with = "principal_serde")]
        principal: VerifiedActor,
        role_id: Uuid,
        unit_id: Uuid,
        #[serde(default)]
        valid_from: Option<String>,
        #[serde(default)]
        valid_until: Option<String>,
        reason: String,
    },
    RevokeRoleAssignment {
        context: CommandContext,
        assignment_id: Uuid,
        reason: String,
    },
    CreateDelegation {
        context: CommandContext,
        source_assignment_id: Uuid,
        #[serde(with = "principal_serde")]
        recipient: VerifiedActor,
        actions: Vec<PolicyAction>,
        #[serde(default)]
        valid_from: Option<String>,
        valid_until: String,
        reason: String,
    },
    RevokeDelegation {
        context: CommandContext,
        delegation_id: Uuid,
        reason: String,
    },
}
impl PolicyCommand {
    pub fn context(&self) -> &CommandContext {
        match self {
            Self::CreateRoleAssignment { context, .. }
            | Self::RevokeRoleAssignment { context, .. }
            | Self::CreateDelegation { context, .. }
            | Self::RevokeDelegation { context, .. } => context,
        }
    }
}

pub(crate) fn parse_instant(value: &str) -> Result<OffsetDateTime, WorkError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| WorkError::ValidationFailed)
}
pub(crate) fn format_instant(value: OffsetDateTime) -> Result<String, WorkError> {
    value
        .to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|_| WorkError::IntegrityViolation)
}
/// History is never backdated: an explicit start may only trail the trusted
/// server instant by a small clock lag, and then starts now.
fn requested_start(value: Option<&str>, at: OffsetDateTime) -> Result<OffsetDateTime, WorkError> {
    let Some(value) = value else {
        return Ok(at);
    };
    let from = parse_instant(value)?;
    if from < at - VALID_FROM_SKEW {
        return Err(WorkError::ValidationFailed);
    }
    Ok(from.max(at))
}
fn stored_instant(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}
/// Policy and reassignment reasons are bounded below the Work text limit. Control
/// characters other than line breaks and tabs are rejected so JSON escaping at
/// most doubles a reason, keeping full policy collections within 1 MiB.
pub const MAX_POLICY_REASON_BYTES: usize = 1024;
pub(crate) fn bounded_reason(value: &str) -> Result<(), WorkError> {
    if value.trim().is_empty()
        || value.len() > MAX_POLICY_REASON_BYTES
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(WorkError::ValidationFailed);
    }
    Ok(())
}
fn role(id: Uuid, key: &str, label: &str, actions: &[PolicyAction]) -> BusinessRole {
    BusinessRole {
        id,
        key: key.into(),
        label: label.into(),
        actions: actions.to_vec(),
    }
}
fn fixture_assignment(
    id: Uuid,
    principal: VerifiedActor,
    role_id: Uuid,
    unit_id: Uuid,
) -> RoleAssignment {
    RoleAssignment {
        id,
        principal,
        role_id,
        unit_id,
        valid_from: FIXTURE_VALID_FROM.into(),
        valid_until: None,
        reason: "合成fixtureの正式割当".into(),
        created_by: None,
        created_at: FIXTURE_VALID_FROM.into(),
        revoked_at: None,
        revoked_by: None,
        revoke_reason: None,
    }
}
impl OrganizationPolicy {
    /// Explicit synthetic fixture: no generic administrator and no department inference.
    /// Grants nothing: the evaluation policy when none has been attached.
    pub fn empty() -> Self {
        Self {
            id: ORGANIZATION_POLICY_ID,
            revision: 0,
            units: vec![],
            roles: vec![],
            role_assignments: vec![],
            delegations: vec![],
        }
    }
    pub fn synthetic() -> Self {
        use PolicyAction::*;
        let work = [
            QueueRead,
            WorkRead,
            WorkClaim,
            WorkEdit,
            WorkSubmit,
            WorkHold,
            WorkResume,
            EvidenceRegister,
            FindingRegister,
            DecisionRecord,
            AgentRequest,
        ];
        let sales: Vec<_> = [ContextRead, ContextProgressRead, ContextHistoryRead]
            .into_iter()
            .chain(work)
            .collect();
        let processing: Vec<_> = work.into_iter().chain([WorkReturn, WorkComplete]).collect();
        let approving = [
            QueueRead,
            WorkRead,
            WorkClaim,
            WorkReturn,
            WorkComplete,
            WorkHold,
            WorkResume,
            EvidenceRegister,
            FindingRegister,
            DecisionRecord,
            AgentRequest,
        ];
        Self {
            id: ORGANIZATION_POLICY_ID,
            revision: 0,
            units: vec![
                OrganizationalUnit {
                    id: UNIT_SALES_ID,
                    label: "営業店".into(),
                    default_archetype: WorkViewArchetype::Context,
                    role_ids: vec![ROLE_SALES_ID],
                },
                OrganizationalUnit {
                    id: UNIT_OFFICE_ID,
                    label: "事務".into(),
                    default_archetype: WorkViewArchetype::Queue,
                    role_ids: vec![ROLE_PROCESSING_ID],
                },
                OrganizationalUnit {
                    id: UNIT_REVIEW_ID,
                    label: "融資審査".into(),
                    default_archetype: WorkViewArchetype::Queue,
                    role_ids: vec![ROLE_REVIEWING_ID],
                },
                OrganizationalUnit {
                    id: UNIT_APPROVAL_ID,
                    label: "承認".into(),
                    default_archetype: WorkViewArchetype::Queue,
                    role_ids: vec![ROLE_APPROVING_ID, ROLE_MANAGEMENT_ID],
                },
            ],
            roles: vec![
                role(ROLE_SALES_ID, "sales", "営業", &sales),
                role(ROLE_PROCESSING_ID, "processing", "事務処理", &processing),
                role(ROLE_REVIEWING_ID, "reviewing", "審査", &processing),
                role(ROLE_APPROVING_ID, "approving", "承認", &approving),
                role(
                    ROLE_MANAGEMENT_ID,
                    "management",
                    "業務管理",
                    &[QueueRead, WorkAssign, OrganizationManage],
                ),
            ],
            role_assignments: vec![
                fixture_assignment(
                    SALES_ASSIGNMENT_ID,
                    VerifiedActor::Sales01,
                    ROLE_SALES_ID,
                    UNIT_SALES_ID,
                ),
                fixture_assignment(
                    OFFICE_ASSIGNMENT_ID,
                    VerifiedActor::Office01,
                    ROLE_PROCESSING_ID,
                    UNIT_OFFICE_ID,
                ),
                fixture_assignment(
                    REVIEW_ASSIGNMENT_ID,
                    VerifiedActor::Review01,
                    ROLE_REVIEWING_ID,
                    UNIT_REVIEW_ID,
                ),
                fixture_assignment(
                    APPROVER_ASSIGNMENT_ID,
                    VerifiedActor::Approver01,
                    ROLE_APPROVING_ID,
                    UNIT_APPROVAL_ID,
                ),
                fixture_assignment(
                    APPROVER_MANAGEMENT_ASSIGNMENT_ID,
                    VerifiedActor::Approver01,
                    ROLE_MANAGEMENT_ID,
                    UNIT_APPROVAL_ID,
                ),
                fixture_assignment(
                    MULTI_ROLE_PROCESSING_ASSIGNMENT_ID,
                    VerifiedActor::MultiRole01,
                    ROLE_PROCESSING_ID,
                    UNIT_OFFICE_ID,
                ),
                fixture_assignment(
                    MULTI_ROLE_REVIEW_ASSIGNMENT_ID,
                    VerifiedActor::MultiRole01,
                    ROLE_REVIEWING_ID,
                    UNIT_REVIEW_ID,
                ),
            ],
            delegations: vec![],
        }
    }
    fn role(&self, id: Uuid) -> Option<&BusinessRole> {
        self.roles.iter().find(|role| role.id == id)
    }
    fn unit(&self, id: Uuid) -> Option<&OrganizationalUnit> {
        self.units.iter().find(|unit| unit.id == id)
    }
    fn assignment(&self, id: Uuid) -> Option<&RoleAssignment> {
        self.role_assignments.iter().find(|value| value.id == id)
    }
    fn delegation(&self, id: Uuid) -> Option<&Delegation> {
        self.delegations.iter().find(|value| value.id == id)
    }
    fn assignment_effective(&self, value: &RoleAssignment, at: OffsetDateTime) -> bool {
        let Some(from) = stored_instant(&value.valid_from) else {
            return false;
        };
        value.revoked_at.is_none()
            && from <= at
            && value
                .valid_until
                .as_deref()
                .is_none_or(|until| stored_instant(until).is_some_and(|until| at < until))
            && self.role(value.role_id).is_some()
            && self.unit(value.unit_id).is_some()
    }
    fn delegation_effective(&self, value: &Delegation, at: OffsetDateTime) -> bool {
        let (Some(from), Some(until)) = (
            stored_instant(&value.valid_from),
            stored_instant(&value.valid_until),
        ) else {
            return false;
        };
        value.revoked_at.is_none()
            && from <= at
            && at < until
            && self
                .assignment(value.source_assignment_id)
                .is_some_and(|source| {
                    source.principal == value.delegator && self.assignment_effective(source, at)
                })
    }
    fn assignment_responsibility(&self, value: &RoleAssignment) -> Option<Responsibility> {
        let role = self.role(value.role_id)?;
        let unit = self.unit(value.unit_id)?;
        Some(Responsibility {
            id: value.id,
            kind: ResponsibilityKind::RoleAssignment,
            principal: value.principal,
            role_id: role.id,
            role_label: role.label.clone(),
            unit_id: unit.id,
            unit_label: unit.label.clone(),
            actions: role.actions.clone(),
            valid_from: value.valid_from.clone(),
            valid_until: value.valid_until.clone(),
            source_assignment_id: None,
            delegator: None,
            work_view_profile_id: profile_for(role.id, unit.id),
        })
    }
    fn delegation_responsibility(&self, value: &Delegation) -> Option<Responsibility> {
        let source = self.assignment(value.source_assignment_id)?;
        let mut derived = self.assignment_responsibility(source)?;
        // Delegation can only narrow its source responsibility.
        derived
            .actions
            .retain(|action| value.actions.contains(action));
        derived.id = value.id;
        derived.kind = ResponsibilityKind::Delegation;
        derived.principal = value.recipient;
        derived.valid_from = value.valid_from.clone();
        derived.valid_until = Some(value.valid_until.clone());
        derived.source_assignment_id = Some(source.id);
        derived.delegator = Some(value.delegator);
        Some(derived)
    }
    /// Every responsibility effective for the actor at `at`, in stable order.
    pub fn responsibilities(
        &self,
        actor: VerifiedActor,
        at: OffsetDateTime,
    ) -> Vec<Responsibility> {
        let assignments = self
            .role_assignments
            .iter()
            .filter(|value| value.principal == actor && self.assignment_effective(value, at))
            .filter_map(|value| self.assignment_responsibility(value));
        let delegations = self
            .delegations
            .iter()
            .filter(|value| value.recipient == actor && self.delegation_effective(value, at))
            .filter_map(|value| self.delegation_responsibility(value));
        assignments.chain(delegations).collect()
    }
    /// The requested acting responsibility, only when it belongs to the actor now.
    pub fn responsibility(
        &self,
        actor: VerifiedActor,
        id: Uuid,
        at: OffsetDateTime,
    ) -> Option<Responsibility> {
        if let Some(value) = self.assignment(id) {
            return (value.principal == actor && self.assignment_effective(value, at))
                .then(|| self.assignment_responsibility(value))
                .flatten();
        }
        let value = self.delegation(id)?;
        (value.recipient == actor && self.delegation_effective(value, at))
            .then(|| self.delegation_responsibility(value))
            .flatten()
    }
    /// Labels for an assignment or delegation regardless of current effect,
    /// used only to explain an ended responsibility; never an authorization.
    pub fn describe(&self, id: Uuid) -> Option<Responsibility> {
        match self.assignment(id) {
            Some(value) => self.assignment_responsibility(value),
            None => self
                .delegation(id)
                .and_then(|value| self.delegation_responsibility(value)),
        }
    }
    pub fn can_manage(&self, actor: VerifiedActor, at: OffsetDateTime) -> bool {
        self.responsibilities(actor, at).iter().any(|value| {
            value.kind == ResponsibilityKind::RoleAssignment
                && value.allows(PolicyAction::OrganizationManage)
        })
    }
    fn management(
        &self,
        actor: VerifiedActor,
        acting: Uuid,
        at: OffsetDateTime,
    ) -> Result<Responsibility, WorkError> {
        self.responsibility(actor, acting, at)
            .filter(|value| {
                value.kind == ResponsibilityKind::RoleAssignment
                    && value.allows(PolicyAction::OrganizationManage)
            })
            .ok_or(WorkError::Forbidden)
    }
    fn assignment_visible(
        &self,
        actor: VerifiedActor,
        value: &RoleAssignment,
        at: OffsetDateTime,
    ) -> bool {
        value.principal == actor || self.can_manage(actor, at)
    }
    fn delegation_visible(
        &self,
        actor: VerifiedActor,
        value: &Delegation,
        at: OffsetDateTime,
    ) -> bool {
        value.delegator == actor || value.recipient == actor || self.can_manage(actor, at)
    }
    pub fn view(
        &self,
        actor: VerifiedActor,
        at: OffsetDateTime,
    ) -> Result<OrganizationView, WorkError> {
        Ok(OrganizationView {
            principal_id: actor.principal_id().into(),
            display_name: actor.display_name().into(),
            policy_revision: self.revision,
            evaluated_at: format_instant(at)?,
            can_manage: self.can_manage(actor, at),
            responsibilities: self.responsibilities(actor, at),
            units: self.units.clone(),
            roles: self.roles.clone(),
            role_assignments: self
                .role_assignments
                .iter()
                .filter(|value| self.assignment_visible(actor, value, at))
                .cloned()
                .collect(),
            delegations: self
                .delegations
                .iter()
                .filter(|value| self.delegation_visible(actor, value, at))
                .cloned()
                .collect(),
        })
    }
    pub fn validate_integrity(&self) -> Result<(), WorkError> {
        let mut ids = std::collections::BTreeSet::new();
        if self.id != ORGANIZATION_POLICY_ID
            || self.revision < 0
            || self.role_assignments.len() > MAX_ROLE_ASSIGNMENTS
            || self.delegations.len() > MAX_DELEGATIONS
        {
            return Err(WorkError::IntegrityViolation);
        }
        for id in self
            .units
            .iter()
            .map(|value| value.id)
            .chain(self.roles.iter().map(|value| value.id))
            .chain(self.role_assignments.iter().map(|value| value.id))
            .chain(self.delegations.iter().map(|value| value.id))
        {
            if !ids.insert(id) {
                return Err(WorkError::IntegrityViolation);
            }
        }
        for unit in &self.units {
            if unit.role_ids.iter().any(|id| self.role(*id).is_none()) {
                return Err(WorkError::IntegrityViolation);
            }
        }
        for value in &self.role_assignments {
            let from = stored_instant(&value.valid_from).ok_or(WorkError::IntegrityViolation)?;
            let until = value
                .valid_until
                .as_deref()
                .map(|until| stored_instant(until).ok_or(WorkError::IntegrityViolation))
                .transpose()?;
            if self.role(value.role_id).is_none()
                || self.unit(value.unit_id).is_none()
                || until.is_some_and(|until| until <= from)
                || value.revoked_at.is_some() != value.revoked_by.is_some()
            {
                return Err(WorkError::IntegrityViolation);
            }
        }
        for value in &self.delegations {
            let from = stored_instant(&value.valid_from).ok_or(WorkError::IntegrityViolation)?;
            let until = stored_instant(&value.valid_until).ok_or(WorkError::IntegrityViolation)?;
            let source = self
                .assignment(value.source_assignment_id)
                .ok_or(WorkError::IntegrityViolation)?;
            let role = self
                .role(source.role_id)
                .ok_or(WorkError::IntegrityViolation)?;
            if until <= from
                || source.principal != value.delegator
                || value.recipient == value.delegator
                || value.actions.is_empty()
                || value
                    .actions
                    .iter()
                    .any(|action| !action.delegable() || !role.actions.contains(action))
                || value.revoked_at.is_some() != value.revoked_by.is_some()
            {
                return Err(WorkError::IntegrityViolation);
            }
        }
        Ok(())
    }
    /// Validate and apply one policy mutation on a private copy. `now` is the
    /// trusted server commit instant; every failure leaves the policy unchanged.
    pub fn apply(
        &mut self,
        actor: VerifiedActor,
        command: &PolicyCommand,
        now: &str,
    ) -> Result<MutationResult, WorkError> {
        self.validate_integrity()?;
        let context = command.context();
        if context.operation_id.get_version_num() != 7 || context.expected_revision < 0 {
            return Err(WorkError::ValidationFailed);
        }
        let at = parse_instant(now).map_err(|_| WorkError::IntegrityViolation)?;
        // Authority precedes OCC so an unauthorized caller learns nothing new.
        self.authorize_command(actor, command, at)?;
        if self.revision != context.expected_revision {
            return Err(WorkError::RevisionConflict);
        }
        let mut next = self.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(WorkError::IntegrityViolation)?;
        let result = next.apply_validated(actor, command, at, now)?;
        next.validate_integrity()?;
        *self = next;
        Ok(result)
    }
    fn authorize_command(
        &self,
        actor: VerifiedActor,
        command: &PolicyCommand,
        at: OffsetDateTime,
    ) -> Result<(), WorkError> {
        let acting = command.context().acting_assignment_id;
        match command {
            PolicyCommand::CreateRoleAssignment { principal, .. } => {
                self.management(actor, acting, at)?;
                // Separation of duties: management never grants its own caller a role.
                if *principal == actor {
                    return Err(WorkError::Forbidden);
                }
            }
            PolicyCommand::RevokeRoleAssignment { .. } => {
                self.management(actor, acting, at)?;
            }
            PolicyCommand::CreateDelegation {
                source_assignment_id,
                ..
            } => {
                // Only the holder delegates; a manager never acts in a delegator's name.
                let own = acting == *source_assignment_id
                    && self
                        .responsibility(actor, acting, at)
                        .is_some_and(|value| value.kind == ResponsibilityKind::RoleAssignment);
                if !own {
                    return Err(WorkError::Forbidden);
                }
            }
            PolicyCommand::RevokeDelegation { delegation_id, .. } => {
                let own = self.delegation(*delegation_id).is_some_and(|value| {
                    value.delegator == actor && value.source_assignment_id == acting
                }) && self
                    .responsibility(actor, acting, at)
                    .is_some_and(|value| value.kind == ResponsibilityKind::RoleAssignment);
                if !own {
                    self.management(actor, acting, at)?;
                }
            }
        }
        Ok(())
    }
    fn apply_validated(
        &mut self,
        actor: VerifiedActor,
        command: &PolicyCommand,
        at: OffsetDateTime,
        now: &str,
    ) -> Result<MutationResult, WorkError> {
        let policy_revision = self.revision;
        match command {
            PolicyCommand::CreateRoleAssignment {
                principal,
                role_id,
                unit_id,
                valid_from,
                valid_until,
                reason,
                ..
            } => {
                bounded_reason(reason)?;
                let unit = self.unit(*unit_id).ok_or(WorkError::ValidationFailed)?;
                if self.role(*role_id).is_none() || !unit.role_ids.contains(role_id) {
                    return Err(WorkError::ValidationFailed);
                }
                let from = requested_start(valid_from.as_deref(), at)?;
                let until = valid_until.as_deref().map(parse_instant).transpose()?;
                if until.is_some_and(|until| until <= from || until <= at)
                    || self.role_assignments.len() >= MAX_ROLE_ASSIGNMENTS
                {
                    return Err(WorkError::ValidationFailed);
                }
                // A concurrent identical formal assignment would make revocation ambiguous.
                let overlaps = self.role_assignments.iter().any(|value| {
                    value.principal == *principal
                        && value.role_id == *role_id
                        && value.unit_id == *unit_id
                        && value.revoked_at.is_none()
                        && intervals_overlap(
                            stored_instant(&value.valid_from),
                            match value.valid_until.as_deref() {
                                None => Some(None),
                                Some(until) => stored_instant(until).map(Some),
                            },
                            from,
                            until,
                        )
                });
                if overlaps {
                    return Err(WorkError::ValidationFailed);
                }
                let assignment = RoleAssignment {
                    id: Uuid::now_v7(),
                    principal: *principal,
                    role_id: *role_id,
                    unit_id: *unit_id,
                    valid_from: format_instant(from)?,
                    valid_until: until.map(format_instant).transpose()?,
                    reason: reason.clone(),
                    created_by: Some(actor),
                    created_at: now.into(),
                    revoked_at: None,
                    revoked_by: None,
                    revoke_reason: None,
                };
                self.role_assignments.push(assignment.clone());
                Ok(MutationResult::RoleAssignmentCreated {
                    assignment,
                    policy_revision,
                })
            }
            PolicyCommand::RevokeRoleAssignment {
                context,
                assignment_id,
                reason,
            } => {
                bounded_reason(reason)?;
                // Revoking the acting management assignment would lock out its own caller.
                if *assignment_id == context.acting_assignment_id {
                    return Err(WorkError::ValidationFailed);
                }
                let value = self
                    .role_assignments
                    .iter_mut()
                    .find(|value| value.id == *assignment_id)
                    .ok_or(WorkError::OrganizationRecordNotFound)?;
                if value.revoked_at.is_some() {
                    return Err(WorkError::ValidationFailed);
                }
                value.revoked_at = Some(now.into());
                value.revoked_by = Some(actor);
                value.revoke_reason = Some(reason.clone());
                Ok(MutationResult::RoleAssignmentRevoked {
                    assignment: value.clone(),
                    policy_revision,
                })
            }
            PolicyCommand::CreateDelegation {
                source_assignment_id,
                recipient,
                actions,
                valid_from,
                valid_until,
                reason,
                ..
            } => {
                bounded_reason(reason)?;
                let source = self
                    .assignment(*source_assignment_id)
                    .ok_or(WorkError::OrganizationRecordNotFound)?;
                if !self.assignment_effective(source, at) || source.principal == *recipient {
                    return Err(WorkError::ValidationFailed);
                }
                let role = self
                    .role(source.role_id)
                    .ok_or(WorkError::IntegrityViolation)?;
                let unique: std::collections::BTreeSet<_> = actions.iter().collect();
                if actions.is_empty()
                    || unique.len() != actions.len()
                    || actions
                        .iter()
                        .any(|action| !action.delegable() || !role.actions.contains(action))
                {
                    return Err(WorkError::ValidationFailed);
                }
                let from = requested_start(valid_from.as_deref(), at)?;
                let until = parse_instant(valid_until)?;
                let source_until = source.valid_until.as_deref().and_then(stored_instant);
                if until <= from
                    || until <= at
                    || source_until.is_some_and(|source_until| until > source_until)
                    || self
                        .delegations
                        .iter()
                        .filter(|value| value.delegator == source.principal)
                        .count()
                        >= MAX_DELEGATIONS_PER_DELEGATOR
                {
                    return Err(WorkError::ValidationFailed);
                }
                let overlaps = self.delegations.iter().any(|value| {
                    value.source_assignment_id == *source_assignment_id
                        && value.recipient == *recipient
                        && value.revoked_at.is_none()
                        && intervals_overlap(
                            stored_instant(&value.valid_from),
                            stored_instant(&value.valid_until).map(Some),
                            from,
                            Some(until),
                        )
                });
                if overlaps {
                    return Err(WorkError::ValidationFailed);
                }
                let delegation = Delegation {
                    id: Uuid::now_v7(),
                    source_assignment_id: *source_assignment_id,
                    delegator: source.principal,
                    recipient: *recipient,
                    actions: actions.clone(),
                    valid_from: format_instant(from)?,
                    valid_until: format_instant(until)?,
                    reason: reason.clone(),
                    created_by: actor,
                    created_at: now.into(),
                    revoked_at: None,
                    revoked_by: None,
                    revoke_reason: None,
                };
                self.delegations.push(delegation.clone());
                Ok(MutationResult::DelegationCreated {
                    delegation,
                    policy_revision,
                })
            }
            PolicyCommand::RevokeDelegation {
                delegation_id,
                reason,
                ..
            } => {
                bounded_reason(reason)?;
                let value = self
                    .delegations
                    .iter_mut()
                    .find(|value| value.id == *delegation_id)
                    .ok_or(WorkError::OrganizationRecordNotFound)?;
                if value.revoked_at.is_some() {
                    return Err(WorkError::ValidationFailed);
                }
                value.revoked_at = Some(now.into());
                value.revoked_by = Some(actor);
                value.revoke_reason = Some(reason.clone());
                Ok(MutationResult::DelegationRevoked {
                    delegation: value.clone(),
                    policy_revision,
                })
            }
        }
    }
    /// A committed policy receipt is replayed only while its record stays visible.
    pub fn authorize_recovery(
        &self,
        actor: VerifiedActor,
        result: &MutationResult,
        at: OffsetDateTime,
    ) -> Result<(), WorkError> {
        let visible = match result {
            MutationResult::RoleAssignmentCreated { assignment, .. }
            | MutationResult::RoleAssignmentRevoked { assignment, .. } => self
                .assignment(assignment.id)
                .is_some_and(|value| self.assignment_visible(actor, value, at)),
            MutationResult::DelegationCreated { delegation, .. }
            | MutationResult::DelegationRevoked { delegation, .. } => self
                .delegation(delegation.id)
                .is_some_and(|value| self.delegation_visible(actor, value, at)),
            _ => false,
        };
        visible
            .then_some(())
            .ok_or(WorkError::OrganizationRecordNotFound)
    }
}
/// Half-open intervals; a missing end is unbounded. Unparseable stored bounds
/// are treated as overlapping so a corrupt record can never hide a duplicate.
fn intervals_overlap(
    from: Option<OffsetDateTime>,
    until: Option<Option<OffsetDateTime>>,
    other_from: OffsetDateTime,
    other_until: Option<OffsetDateTime>,
) -> bool {
    let (Some(from), Some(until)) = (from, until) else {
        return true;
    };
    until.is_none_or(|until| other_from < until) && other_until.is_none_or(|other| from < other)
}

/// Public principal spelling (`sales-01`). The legacy enum encoding remains the
/// operation-digest input and is accepted only for reading stored records.
pub mod principal_serde {
    use super::VerifiedActor;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(
        actor: &VerifiedActor,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(actor.principal_id())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<VerifiedActor, D::Error> {
        let value = String::deserialize(deserializer)?;
        VerifiedActor::from_principal_id(&value)
            .or_else(|| VerifiedActor::from_legacy_encoding(&value))
            .ok_or_else(|| serde::de::Error::custom("unknown synthetic principal"))
    }
    pub mod option {
        use super::VerifiedActor;
        use serde::{Deserialize, Deserializer, Serializer};
        pub fn serialize<S: Serializer>(
            actor: &Option<VerifiedActor>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match actor {
                Some(actor) => serializer.serialize_some(actor.principal_id()),
                None => serializer.serialize_none(),
            }
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<VerifiedActor>, D::Error> {
            Option::<String>::deserialize(deserializer)?
                .map(|value| {
                    VerifiedActor::from_principal_id(&value)
                        .or_else(|| VerifiedActor::from_legacy_encoding(&value))
                        .ok_or_else(|| serde::de::Error::custom("unknown synthetic principal"))
                })
                .transpose()
        }
    }
}
