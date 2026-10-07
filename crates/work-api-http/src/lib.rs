#![forbid(unsafe_code)]
//! Work HTTP transport. The composition root injects a process-fixed verified actor.
mod agent;
mod evidence;
mod organization;
use agent::*;
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, Request, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use evidence::*;
use organization::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use work_application::{AgentDispatchPort, WorkRepository};
use work_domain::*;

const MAX_JSON_BYTES: usize = 1024 * 1024;
#[derive(Clone)]
struct ApiState {
    repository: Arc<dyn WorkRepository>,
    actor: VerifiedActor,
    agent_dispatch: Option<Arc<dyn AgentDispatchPort>>,
}
/// Header/body/query values cannot change identity or grant acting responsibility.
pub fn router(repository: Arc<dyn WorkRepository>, actor: VerifiedActor) -> Router {
    build_router(repository, actor, None)
}
pub fn router_with_agent(
    repository: Arc<dyn WorkRepository>,
    actor: VerifiedActor,
    dispatch: Arc<dyn AgentDispatchPort>,
) -> Router {
    build_router(repository, actor, Some(dispatch))
}
fn build_router(
    repository: Arc<dyn WorkRepository>,
    actor: VerifiedActor,
    agent_dispatch: Option<Arc<dyn AgentDispatchPort>>,
) -> Router {
    Router::new()
        .route("/v1/organization/session", get(session))
        .route("/v1/organization/units", get(units))
        .route("/v1/organization/roles", get(roles))
        .route(
            "/v1/organization/role-assignments",
            get(role_assignments).post(create_role_assignment),
        )
        .route(
            "/v1/organization/role-assignments/{id}/revoke",
            post(revoke_role_assignment),
        )
        .route(
            "/v1/organization/delegations",
            get(delegations).post(create_delegation),
        )
        .route(
            "/v1/organization/delegations/{id}/revoke",
            post(revoke_delegation),
        )
        .route("/v1/organization/tasks/{id}/assignment", post(assign_task))
        .route("/v1/organization/tasks", get(list_tasks))
        .route("/v1/organization/tasks/{id}", get(task))
        .route(
            "/v1/organization/tasks/{id}/working-artifacts",
            get(artifacts).post(create_artifact),
        )
        .route(
            "/v1/organization/working-artifacts/{id}",
            get(artifact).put(update_artifact),
        )
        .route(
            "/v1/organization/tasks/{id}/evidence",
            get(list_evidence).post(register_evidence),
        )
        .route("/v1/organization/evidence/{id}", get(evidence))
        .route(
            "/v1/organization/tasks/{id}/findings",
            get(list_findings).post(register_finding),
        )
        .route("/v1/organization/findings/{id}", get(finding))
        .route(
            "/v1/organization/findings/{id}/decisions",
            get(list_decisions).post(record_decision),
        )
        .route(
            "/v1/organization/tasks/{id}/agent-executions",
            post(request_agent_execution),
        )
        .route(
            "/v1/organization/agent-executions/{id}",
            get(agent_execution),
        )
        .route(
            "/v1/organization/agent-executions/{id}/result",
            get(agent_result),
        )
        .route(
            "/v1/organization/agent-executions/{id}/cancel",
            post(cancel_agent_execution),
        )
        .route("/v1/organization/tasks/{id}/claim", post(claim))
        .route("/v1/organization/tasks/{id}/submit", post(submit))
        .route("/v1/organization/tasks/{id}/return", post(return_task))
        .route("/v1/organization/tasks/{id}/actions", post(workflow_action))
        .route(
            "/v1/organization/return-instructions/{id}",
            get(return_instruction),
        )
        .route("/v1/organization/handoff-snapshots/{id}", get(snapshot))
        .route("/v1/organization/operations/{id}", get(recover))
        .fallback(|| async { Problem(WorkError::WorkItemNotFound) })
        .method_not_allowed_fallback(|| async { Problem(WorkError::ValidationFailed) })
        .with_state(ApiState {
            repository,
            actor,
            agent_dispatch,
        })
        .layer(DefaultBodyLimit::max(MAX_JSON_BYTES))
        .layer(middleware::from_fn(transport_boundary))
}
async fn transport_boundary(request: Request, next: Next) -> Response {
    let forbidden = [
        "authorization",
        "x-principal-id",
        "x-actor-id",
        "x-user-id",
        "x-groups",
        "x-role",
        "x-acting-principal",
        "x-organization-profile",
    ];
    let header_bytes: usize = request
        .headers()
        .iter()
        .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
        .sum();
    let has_query = request.uri().query().is_some_and(|query| !query.is_empty());
    let path = request.uri().path();
    let collection_query = request.method() == axum::http::Method::GET
        && (path == "/v1/organization/tasks"
            || (path.starts_with("/v1/organization/tasks/")
                && (path.ends_with("/evidence") || path.ends_with("/findings")))
            || (path.starts_with("/v1/organization/findings/") && path.ends_with("/decisions")));
    let response = if header_bytes > 16 * 1024
        || forbidden
            .iter()
            .any(|name| request.headers().contains_key(*name))
        || (has_query && !collection_query)
    {
        Problem(WorkError::ValidationFailed).into_response()
    } else {
        next.run(request).await
    };
    let (parts, body) = response.into_parts();
    let mut response = match axum::body::to_bytes(body, MAX_JSON_BYTES).await {
        Ok(bytes) => Response::from_parts(parts, axum::body::Body::from(bytes)),
        Err(_) => Problem(WorkError::DependencyUnavailable).into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQuery {
    view: Option<TaskView>,
    limit: Option<usize>,
    cursor: Option<String>,
    /// Requested projection scope; re-resolved against the current policy.
    acting_assignment_id: Option<Uuid>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}
async fn list_tasks(
    State(state): State<ApiState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Page<TaskSummary>>, Problem> {
    let Query(query) = query.map_err(|_| Problem(WorkError::ValidationFailed))?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(Problem(WorkError::ValidationFailed));
    }
    if query.cursor.is_some() {
        return Err(Problem(WorkError::CursorStale));
    }
    let items = state
        .repository
        .list_tasks_in(
            state.actor,
            query.view.unwrap_or(TaskView::Context),
            query.acting_assignment_id,
        )
        .await?;
    // This fixed two-step PoC has no pagination token implementation: never silently truncate.
    if items.len() > limit {
        return Err(Problem(WorkError::ValidationFailed));
    }
    Ok(Json(Page {
        items,
        next_cursor: None,
    }))
}
fn path_id(path: Result<Path<Uuid>, PathRejection>) -> Result<Uuid, Problem> {
    path.map(|Path(id)| id)
        .map_err(|_| Problem(WorkError::ValidationFailed))
}
fn json_body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, Problem> {
    body.map(|Json(value)| value)
        .map_err(|_| Problem(WorkError::ValidationFailed))
}
async fn task(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<TaskDetail>, Problem> {
    Ok(Json(
        state.repository.task(state.actor, path_id(path)?).await?,
    ))
}
async fn artifacts(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Page<WorkingArtifact>>, Problem> {
    Ok(Json(Page {
        items: state
            .repository
            .task(state.actor, path_id(path)?)
            .await?
            .working_artifacts,
        next_cursor: None,
    }))
}
async fn artifact(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<WorkingArtifact>, Problem> {
    Ok(Json(
        state
            .repository
            .artifact(state.actor, path_id(path)?)
            .await?,
    ))
}
async fn snapshot(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<HandoffSnapshot>, Problem> {
    Ok(Json(
        state
            .repository
            .snapshot(state.actor, path_id(path)?)
            .await?,
    ))
}
async fn return_instruction(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<ReturnInstruction>, Problem> {
    Ok(Json(
        state
            .repository
            .return_instruction(state.actor, path_id(path)?)
            .await?,
    ))
}
async fn recover(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<MutationResult>, Problem> {
    Ok(Json(
        state
            .repository
            .recover(state.actor, path_id(path)?)
            .await?,
    ))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    value: TextValue,
}
impl DraftBody {
    fn command(self, task_id: Uuid, artifact_id: Option<Uuid>) -> Command {
        Command::SaveDraft {
            task_id,
            artifact_id,
            context: CommandContext {
                operation_id: self.operation_id,
                expected_revision: self.expected_revision,
                acting_assignment_id: self.acting_assignment_id,
            },
            value: self.value,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmitBody {
    #[serde(default)]
    expected_attempt_id: Option<Uuid>,
    #[serde(default)]
    evidence_revision_refs: Vec<RevisionRef>,
    #[serde(default)]
    finding_revision_refs: Vec<RevisionRef>,
    #[serde(default)]
    decision_revision_refs: Vec<RevisionRef>,
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    artifacts: Vec<ArtifactSelection>,
}
async fn create_artifact(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<DraftBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let command = json_body(body)?.command(path_id(path)?, None);
    Ok(Json(state.repository.execute(state.actor, command).await?))
}
async fn update_artifact(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<DraftBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let id = path_id(path)?;
    let body = json_body(body)?;
    let artifact = state.repository.artifact(state.actor, id).await?;
    Ok(Json(
        state
            .repository
            .execute(state.actor, body.command(artifact.task_id, Some(id)))
            .await?,
    ))
}
async fn claim(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<CommandContext>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let command = Command::Claim {
        task_id: path_id(path)?,
        context: json_body(body)?,
    };
    Ok(Json(state.repository.execute(state.actor, command).await?))
}
async fn submit(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<SubmitBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let body = json_body(body)?;
    let command = Command::Submit {
        task_id: path_id(path)?,
        context: CommandContext {
            operation_id: body.operation_id,
            expected_revision: body.expected_revision,
            acting_assignment_id: body.acting_assignment_id,
        },
        artifacts: body.artifacts,
        expected_attempt_id: body.expected_attempt_id,
        evidence_revision_refs: body.evidence_revision_refs,
        finding_revision_refs: body.finding_revision_refs,
        decision_revision_refs: body.decision_revision_refs,
    };
    Ok(Json(state.repository.execute(state.actor, command).await?))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReturnBody {
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_attempt_id: Uuid,
    previous_submission_id: Uuid,
    target_task_id: Uuid,
    transition_id: Uuid,
    reason: String,
}
async fn return_task(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<ReturnBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let body = json_body(body)?;
    let command = Command::Return {
        task_id: path_id(path)?,
        context: CommandContext {
            operation_id: body.operation_id,
            expected_revision: body.expected_revision,
            acting_assignment_id: body.acting_assignment_id,
        },
        expected_attempt_id: body.expected_attempt_id,
        previous_submission_id: body.previous_submission_id,
        target_task_id: body.target_task_id,
        transition_id: body.transition_id,
        reason: body.reason,
    };
    Ok(Json(state.repository.execute(state.actor, command).await?))
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkflowAction {
    Complete,
    Hold,
    Resume,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowActionBody {
    action: WorkflowAction,
    operation_id: Uuid,
    expected_revision: i64,
    acting_assignment_id: Uuid,
    expected_attempt_id: Uuid,
    definition_action_id: Uuid,
}
async fn workflow_action(
    State(state): State<ApiState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<WorkflowActionBody>, JsonRejection>,
) -> Result<Json<MutationResult>, Problem> {
    let body = json_body(body)?;
    let task_id = path_id(path)?;
    let context = CommandContext {
        operation_id: body.operation_id,
        expected_revision: body.expected_revision,
        acting_assignment_id: body.acting_assignment_id,
    };
    let expected_attempt_id = body.expected_attempt_id;
    let definition_action_id = body.definition_action_id;
    let command = match body.action {
        WorkflowAction::Complete => Command::Complete {
            task_id,
            context,
            expected_attempt_id,
            definition_action_id,
        },
        WorkflowAction::Hold => Command::Hold {
            task_id,
            context,
            expected_attempt_id,
            definition_action_id,
        },
        WorkflowAction::Resume => Command::Resume {
            task_id,
            context,
            expected_attempt_id,
            definition_action_id,
        },
    };
    Ok(Json(state.repository.execute(state.actor, command).await?))
}
struct Problem(WorkError);
impl From<WorkError> for Problem {
    fn from(error: WorkError) -> Self {
        Self(error)
    }
}
impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let status = match self.0 {
            WorkError::ValidationFailed => StatusCode::UNPROCESSABLE_ENTITY,
            WorkError::Forbidden => StatusCode::FORBIDDEN,
            WorkError::EvidenceNotFound
            | WorkError::FindingNotFound
            | WorkError::WorkItemNotFound
            | WorkError::OrganizationRecordNotFound
            | WorkError::WorkArtifactNotFound => StatusCode::NOT_FOUND,
            WorkError::RevisionConflict
            | WorkError::OperationConflict
            | WorkError::WorkAssignmentConflict
            | WorkError::WorkContextStale
            | WorkError::AgentResultNotReady
            | WorkError::HandoffNotReady
            | WorkError::CursorStale => StatusCode::CONFLICT,
            WorkError::DependencyUnavailable | WorkError::CommitOutcomeUnknown => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            WorkError::IntegrityViolation => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = serde_json::json!({"type":"about:blank","title":status.canonical_reason().unwrap_or("Request failed"),"status":status.as_u16(),"code":self.0,"traceId":Uuid::now_v7()});
        let mut response = (status, Json(body)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
