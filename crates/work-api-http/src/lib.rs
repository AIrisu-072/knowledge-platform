#![forbid(unsafe_code)]
//! Work HTTP transport. The composition root injects a process-fixed verified actor.
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
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use work_application::WorkRepository;
use work_domain::*;

const MAX_JSON_BYTES: usize = 1024 * 1024;
#[derive(Clone)]
struct ApiState {
    repository: Arc<dyn WorkRepository>,
    actor: VerifiedActor,
}
/// Header/body/query values cannot change identity or grant acting responsibility.
pub fn router(repository: Arc<dyn WorkRepository>, actor: VerifiedActor) -> Router {
    Router::new()
        .route("/v1/organization/session", get(session))
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
        .route("/v1/organization/tasks/{id}/claim", post(claim))
        .route("/v1/organization/tasks/{id}/submit", post(submit))
        .route("/v1/organization/handoff-snapshots/{id}", get(snapshot))
        .route("/v1/organization/operations/{id}", get(recover))
        .fallback(|| async { Problem(WorkError::WorkItemNotFound) })
        .method_not_allowed_fallback(|| async { Problem(WorkError::ValidationFailed) })
        .with_state(ApiState { repository, actor })
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
    let mut response = if header_bytes > 16 * 1024
        || forbidden
            .iter()
            .any(|name| request.headers().contains_key(*name))
        || (has_query && request.uri().path() != "/v1/organization/tasks")
    {
        Problem(WorkError::ValidationFailed).into_response()
    } else {
        next.run(request).await
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
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}
async fn session(State(state): State<ApiState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "principalId":state.actor.principal_id(),
        "displayName":match state.actor { VerifiedActor::Sales01 => "営業担当（模擬）", VerifiedActor::Office01 => "事務担当（模擬）" },
        "actingAssignmentId":state.actor.assignment_id(),
        "capabilities":{"nativeWorkspace":false,"agent":false,"search":false,"fileUpload":false,"return":false}
    }))
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
        .list_tasks(state.actor, query.view.unwrap_or(TaskView::Context))
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
            WorkError::WorkItemNotFound | WorkError::WorkArtifactNotFound => StatusCode::NOT_FOUND,
            WorkError::RevisionConflict
            | WorkError::OperationConflict
            | WorkError::WorkAssignmentConflict
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
