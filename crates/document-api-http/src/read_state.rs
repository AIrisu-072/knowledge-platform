use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json, Router};
use document_application::{
    CurrentReadProjection, CurrentReadState, CurrentReadStateRepository, CurrentReadStateService,
    MarkVersionRead, ReadStateMutation, ReadStateMutationKind, ReadStateMutationResult,
    ReadStateOperationId, ReadStateRepository, ReadStateService, VerifiedActorContext,
};
use document_domain::DocumentVersionId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::management::{document_id_value, format_time, problem, validation};
use crate::trace::TraceContext;

pub(crate) fn read_state_routes<R: ReadStateRepository + CurrentReadStateRepository + Send + Sync + 'static>() -> Router<Arc<R>> {
    Router::new()
        .route(
            "/v1/documents/{document_id}/versions/{version_id}/read-state",
            get(current_state::<R>).put(mark_version_read::<R>),
        )
        .route(
            "/v1/documents/{document_id}/versions/{version_id}/read-state/view",
            post(record_view::<R>),
        )
        .route(
            "/v1/documents/{document_id}/versions/{version_id}/read-state/reset",
            post(reset_state::<R>),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadStateMutationRequest {
    operation_id: Uuid,
    expected_read_state_revision: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentReadProjectionDto {
    first_read_at: Option<String>,
    needs_recheck: bool,
    read_state_revision: i64,
    is_read: bool,
}

impl From<CurrentReadProjection> for CurrentReadProjectionDto {
    fn from(state: CurrentReadProjection) -> Self {
        Self {
            first_read_at: state.first_read_at.map(format_time),
            needs_recheck: state.needs_recheck,
            read_state_revision: state.read_state_revision,
            is_read: state.is_read(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentReadStateDto {
    document_id: Uuid,
    version_id: Uuid,
    #[serde(flatten)]
    state: CurrentReadProjectionDto,
}

impl From<CurrentReadState> for CurrentReadStateDto {
    fn from(value: CurrentReadState) -> Self {
        Self {
            document_id: value.document_id.as_uuid(),
            version_id: value.document_version_id.as_uuid(),
            state: value.state.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadStateMutationResultDto {
    operation_id: Uuid,
    document_id: Uuid,
    version_id: Uuid,
    kind: &'static str,
    expected_read_state_revision: i64,
    changed: bool,
    occurred_at: String,
    resulting_read_state: CurrentReadProjectionDto,
}

impl From<ReadStateMutationResult> for ReadStateMutationResultDto {
    fn from(value: ReadStateMutationResult) -> Self {
        Self {
            operation_id: value.operation_id.as_uuid(),
            document_id: value.document_id.as_uuid(),
            version_id: value.document_version_id.as_uuid(),
            kind: value.kind.as_str(),
            expected_read_state_revision: value.expected_read_state_revision,
            changed: value.changed,
            occurred_at: format_time(value.occurred_at),
            resulting_read_state: value.resulting_read_state.into(),
        }
    }
}

fn no_store(value: impl Serialize) -> Response {
    let mut response = Json(value).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

fn target(document_id: &str, version_id: &str) -> Result<(document_domain::DocumentId, DocumentVersionId), document_application::ApplicationError> {
    Ok((
        document_id_value(document_id)?,
        Uuid::parse_str(version_id).map(DocumentVersionId::from_uuid).map_err(|_| validation("invalid versionId"))?,
    ))
}

async fn current_state<R: CurrentReadStateRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, version_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let path = format!("/v1/documents/{document_id}/versions/{version_id}/read-state");
    let (document_id, version_id) = target(&document_id, &version_id).map_err(|error| problem(error, &path, &trace))?;
    let result = CurrentReadStateService::new(repository).get_current_read_state(&ctx, document_id, version_id).await.map_err(|error| problem(error, &path, &trace))?;
    Ok(no_store(CurrentReadStateDto::from(result)))
}

async fn record_view<R: CurrentReadStateRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(ids): Path<(String, String)>,
    body: Result<Json<ReadStateMutationRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    mutate(repository, ctx, trace, ids, body, ReadStateMutationKind::View).await
}

async fn reset_state<R: CurrentReadStateRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(ids): Path<(String, String)>,
    body: Result<Json<ReadStateMutationRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    mutate(repository, ctx, trace, ids, body, ReadStateMutationKind::Reset).await
}

async fn mutate<R: CurrentReadStateRepository>(
    repository: Arc<R>,
    ctx: VerifiedActorContext,
    trace: TraceContext,
    (document_id, version_id): (String, String),
    body: Result<Json<ReadStateMutationRequest>, JsonRejection>,
    kind: ReadStateMutationKind,
) -> Result<Response, ApiError> {
    let suffix = match kind {
        ReadStateMutationKind::View => "view",
        ReadStateMutationKind::Reset => "reset",
    };
    let path = format!("/v1/documents/{document_id}/versions/{version_id}/read-state/{suffix}");
    let (document_id, document_version_id) = target(&document_id, &version_id).map_err(|error| problem(error, &path, &trace))?;
    let Json(body) = body.map_err(|_| problem(validation("invalid read-state mutation JSON"), &path, &trace))?;
    let operation_id = ReadStateOperationId::try_from_uuid(body.operation_id).map_err(|error| problem(error, &path, &trace))?;
    let result = CurrentReadStateService::new(repository).mutate_read_state(&ctx, ReadStateMutation {
        operation_id,
        document_id,
        document_version_id,
        expected_read_state_revision: body.expected_read_state_revision,
        kind,
    }).await.map_err(|error| problem(error, &path, &trace))?;
    Ok(no_store(ReadStateMutationResultDto::from(result)))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadStateResultDto {
    document_id: Uuid,
    version_id: Uuid,
    first_read_at: String,
    inserted: bool,
}

async fn mark_version_read<R: ReadStateRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, version_id)): Path<(String, String)>,
) -> Result<Json<ReadStateResultDto>, ApiError> {
    let path = format!("/v1/documents/{document_id}/versions/{version_id}/read-state");
    let document_id =
        document_id_value(&document_id).map_err(|error| problem(error, &path, &trace))?;
    let version_id = Uuid::parse_str(&version_id)
        .map(DocumentVersionId::from_uuid)
        .map_err(|_| problem(validation("invalid versionId"), &path, &trace))?;
    ReadStateService::new(repository)
        .mark_version_read(
            &ctx,
            MarkVersionRead {
                document_id,
                document_version_id: version_id,
            },
        )
        .await
        .map(|result| {
            Json(ReadStateResultDto {
                document_id: document_id.as_uuid(),
                version_id: result.document_version_id.as_uuid(),
                first_read_at: format_time(result.first_read_at),
                inserted: result.inserted,
            })
        })
        .map_err(|error| problem(error, &path, &trace))
}
