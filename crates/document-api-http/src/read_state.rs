use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::put;
use axum::{Extension, Json, Router};
use document_application::{
    MarkVersionRead, ReadStateRepository, ReadStateService, VerifiedActorContext,
};
use document_domain::DocumentVersionId;
use serde::Serialize;
use uuid::Uuid;

use crate::error::ApiError;
use crate::management::{document_id_value, format_time, problem, validation};
use crate::trace::TraceContext;

pub(crate) fn read_state_routes<R: ReadStateRepository + Send + Sync + 'static>() -> Router<Arc<R>>
{
    Router::new().route(
        "/v1/documents/{document_id}/versions/{version_id}/read-state",
        put(mark_version_read::<R>),
    )
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
