//! Organization policy and assignment transport. Principals and acting
//! responsibilities in a body are requests the aggregate re-resolves; they are
//! never identity or grants.
use super::*;

fn principal(value: &str) -> Result<VerifiedActor, Problem> {
    VerifiedActor::from_principal_id(value).ok_or(Problem(WorkError::ValidationFailed))
}
fn context(
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
) -> CommandContext {
    CommandContext {
        operation_id,
        expected_revision,
        acting_assignment_id,
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RoleAssignmentBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    principal_id: String,
    role_id: Uuid,
    unit_id: Uuid,
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
    reason: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RevokeBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    reason: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DelegationBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    source_assignment_id: Uuid,
    recipient_principal_id: String,
    actions: Vec<PolicyAction>,
    #[serde(default)]
    valid_from: Option<String>,
    valid_until: String,
    reason: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AssignmentBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_attempt_id: Uuid,
    assignee_principal_id: String,
    assignee_responsibility_id: Uuid,
    reason: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Items<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}
fn items<T>(items: Vec<T>) -> Json<Items<T>> {
    Json(Items {
        items,
        next_cursor: None,
    })
}
pub(super) async fn session(State(state): State<ApiState>) -> Json<serde_json::Value> {
    // Identity is process-fixed; responsibilities are evaluated now and may be
    // unavailable, which is never presented as an empty grant.
    let organization = state.repository.organization(state.actor).await.ok();
    let acting = organization.as_ref().and_then(|view| {
        let default = state.actor.assignment_id();
        view.responsibilities
            .iter()
            .find(|value| value.id == default)
            .or_else(|| view.responsibilities.first())
            .map(|value| value.id)
    });
    Json(serde_json::json!({
        "principalId": state.actor.principal_id(),
        "displayName": state.actor.display_name(),
        "actingAssignmentId": acting,
        "responsibilities": organization.as_ref().map(|view| &view.responsibilities),
        "canManageOrganization": organization.as_ref().is_some_and(|view| view.can_manage),
        "policyRevision": organization.as_ref().map(|view| view.policy_revision),
        "capabilities": {"nativeWorkspace": false, "agent": state.agent_dispatch.is_some(), "search": false, "fileUpload": false, "return": true}
    }))
}
pub(super) async fn units(
    State(state): State<ApiState>,
) -> Result<Json<Items<OrganizationalUnit>>, Problem> {
    Ok(items(
        state.repository.organization(state.actor).await?.units,
    ))
}
pub(super) async fn roles(
    State(state): State<ApiState>,
) -> Result<Json<Items<BusinessRole>>, Problem> {
    Ok(items(
        state.repository.organization(state.actor).await?.roles,
    ))
}
pub(super) async fn role_assignments(
    State(state): State<ApiState>,
) -> Result<Json<Items<RoleAssignment>>, Problem> {
    Ok(items(
        state
            .repository
            .organization(state.actor)
            .await?
            .role_assignments,
    ))
}
pub(super) async fn delegations(
    State(state): State<ApiState>,
) -> Result<Json<Items<Delegation>>, Problem> {
    Ok(items(
        state
            .repository
            .organization(state.actor)
            .await?
            .delegations,
    ))
}
pub(super) async fn create_role_assignment(
    State(state): State<ApiState>,
    body: Result<Json<RoleAssignmentBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let body = json_body(body)?;
    let command = PolicyCommand::CreateRoleAssignment {
        context: context(
            body.operation_id,
            body.expected_revision,
            body.acting_assignment_id,
        ),
        principal: principal(&body.principal_id)?,
        role_id: body.role_id,
        unit_id: body.unit_id,
        valid_from: body.valid_from,
        valid_until: body.valid_until,
        reason: body.reason,
    };
    Ok(Json(
        state
            .repository
            .execute_policy(state.actor, command)
            .await?,
    ))
}
pub(super) async fn revoke_role_assignment(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RevokeBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let assignment_id = path_id(path)?;
    let body = json_body(body)?;
    let command = PolicyCommand::RevokeRoleAssignment {
        context: context(
            body.operation_id,
            body.expected_revision,
            body.acting_assignment_id,
        ),
        assignment_id,
        reason: body.reason,
    };
    Ok(Json(
        state
            .repository
            .execute_policy(state.actor, command)
            .await?,
    ))
}
pub(super) async fn create_delegation(
    State(state): State<ApiState>,
    body: Result<Json<DelegationBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let body = json_body(body)?;
    let command = PolicyCommand::CreateDelegation {
        context: context(
            body.operation_id,
            body.expected_revision,
            body.acting_assignment_id,
        ),
        source_assignment_id: body.source_assignment_id,
        recipient: principal(&body.recipient_principal_id)?,
        actions: body.actions,
        valid_from: body.valid_from,
        valid_until: body.valid_until,
        reason: body.reason,
    };
    Ok(Json(
        state
            .repository
            .execute_policy(state.actor, command)
            .await?,
    ))
}
pub(super) async fn revoke_delegation(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RevokeBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let delegation_id = path_id(path)?;
    let body = json_body(body)?;
    let command = PolicyCommand::RevokeDelegation {
        context: context(
            body.operation_id,
            body.expected_revision,
            body.acting_assignment_id,
        ),
        delegation_id,
        reason: body.reason,
    };
    Ok(Json(
        state
            .repository
            .execute_policy(state.actor, command)
            .await?,
    ))
}
pub(super) async fn assign_task(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<AssignmentBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let task_id = path_id(path)?;
    let body = json_body(body)?;
    let command = Command::Assign {
        task_id,
        context: context(
            body.operation_id,
            body.expected_revision,
            body.acting_assignment_id,
        ),
        expected_attempt_id: body.expected_attempt_id,
        assignee: principal(&body.assignee_principal_id)?,
        assignee_responsibility_id: body.assignee_responsibility_id,
        reason: body.reason,
    };
    Ok(Json(state.repository.execute(state.actor, command).await?))
}
