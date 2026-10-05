use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    routing::get,
};
use document_application::{
    ApplicationError, EditManifest, EditManifestPurpose, EditManifestRepository,
    EditManifestRequest, EditManifestService, VerifiedActorContext,
};
use document_domain::{DocumentId, DocumentVersionId};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiProblem},
    identity::IdentityAdapter,
    limits::ORDINARY_OPERATION_TIMEOUT,
    router::{StartupError, protect_routes},
    timeout::with_operation_timeout,
    trace::TraceContext,
};

/// Independently testable read family, also included in the ordinary read router.
pub fn edit_manifest_router<R: EditManifestRepository + 'static>(
    repository: Arc<R>,
    identity_adapter: Arc<dyn IdentityAdapter>,
) -> Result<Router, StartupError> {
    protect_routes(
        with_operation_timeout(edit_manifest_routes(repository), ORDINARY_OPERATION_TIMEOUT),
        Some(identity_adapter),
    )
}

pub(crate) fn edit_manifest_routes<R: EditManifestRepository + 'static>(
    repository: Arc<R>,
) -> Router {
    Router::new()
        .route(
            "/v1/documents/{document_id}/versions/{version_id}/edit-manifest",
            get(get_manifest::<R>),
        )
        .with_state(repository)
}

#[derive(Deserialize)]
struct Params {
    purpose: EditManifestPurpose,
}

async fn get_manifest<R: EditManifestRepository + 'static>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, version_id)): Path<(String, String)>,
    query: Result<Query<Params>, QueryRejection>,
) -> Result<Json<EditManifest>, ApiError> {
    let path = format!("/v1/documents/{document_id}/versions/{version_id}/edit-manifest");
    let problem =
        |error| ApiError::from(ApiProblem::from_application(error, &path, &trace.trace_id));
    let Query(params) = query.map_err(|_| {
        problem(ApplicationError::Validation(
            "invalid editing purpose".into(),
        ))
    })?;
    let document_id = Uuid::parse_str(&document_id)
        .map_err(|_| problem(ApplicationError::Validation("invalid documentId".into())))?;
    let source_version_id = Uuid::parse_str(&version_id)
        .map_err(|_| problem(ApplicationError::Validation("invalid versionId".into())))?;
    EditManifestService::new(repository)
        .read(
            &ctx,
            EditManifestRequest {
                document_id: DocumentId::from_uuid(document_id),
                source_version_id: DocumentVersionId::from_uuid(source_version_id),
                purpose: params.purpose,
            },
        )
        .await
        .map(Json)
        .map_err(problem)
}
