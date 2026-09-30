use std::sync::Arc;

use axum::extract::multipart::MultipartRejection;
use axum::extract::rejection::QueryRejection;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use document_application::{
    ApplicationError, AuthorizationScope, Clock, CreateDocumentCommand, CreateDocumentResult,
    CreateOutcomeProbe, CreateOutcomeRecoveryService, CreateOutcomeRepository, DocumentRepository,
    DocumentService, FileStorage, IdGenerator, VerifiedActorContext,
};
use document_domain::{DocumentId, DocumentVersionId, FileId, FolderId, MediaType, Metadata};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use uuid::Uuid;

use crate::error::{ApiError, ApiProblem, ErrorCode};
use crate::identity::IdentityAdapter;
use crate::limits::UploadLimits;
use crate::multipart::{MultipartFailure, parse_initial_upload, request_header_bytes};
use crate::router::{StartupError, protect_routes};
use crate::trace::TraceContext;

pub trait CreateApiRepository:
    AuthorizationScope + DocumentRepository + CreateOutcomeRepository + Send + Sync + 'static
{
}

impl<T> CreateApiRepository for T where
    T: AuthorizationScope + DocumentRepository + CreateOutcomeRepository + Send + Sync + 'static
{
}

struct CreateState<I, C, F, R> {
    ids: Arc<I>,
    clock: Arc<C>,
    storage: Arc<F>,
    repository: Arc<R>,
    limits: UploadLimits,
}

impl<I, C, F, R> Clone for CreateState<I, C, F, R> {
    fn clone(&self) -> Self {
        Self {
            ids: self.ids.clone(),
            clock: self.clock.clone(),
            storage: self.storage.clone(),
            repository: self.repository.clone(),
            limits: self.limits,
        }
    }
}

pub fn create_router<I, C, F, R>(
    ids: Arc<I>,
    clock: Arc<C>,
    storage: Arc<F>,
    repository: Arc<R>,
    identity_adapter: Arc<dyn IdentityAdapter>,
) -> Result<Router, StartupError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    R: CreateApiRepository,
{
    create_router_with_limits(
        ids,
        clock,
        storage,
        repository,
        identity_adapter,
        UploadLimits::PRODUCTION,
    )
}

/// Builds the same router with a profile no looser than the production profile.
/// This is used by deterministic boundary tests and stricter deployments.
pub fn create_router_with_limits<I, C, F, R>(
    ids: Arc<I>,
    clock: Arc<C>,
    storage: Arc<F>,
    repository: Arc<R>,
    identity_adapter: Arc<dyn IdentityAdapter>,
    limits: UploadLimits,
) -> Result<Router, StartupError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    R: CreateApiRepository,
{
    let total_bytes = limits.total_bytes;
    let state = CreateState {
        ids,
        clock,
        storage,
        repository,
        limits,
    };
    let routes = Router::new()
        .route("/v1/documents", post(create_document::<I, C, F, R>))
        .route(
            "/v1/document-creation-outcomes/{document_id}",
            get(recover_create::<I, C, F, R>),
        )
        .with_state(state)
        .layer(DefaultBodyLimit::max(total_bytes));
    protect_routes(routes, Some(identity_adapter))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateDocumentRequest {
    folder_id: Uuid,
    title: String,
    document_metadata: Map<String, serde_json::Value>,
    version_metadata: Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateOutcomeQuery {
    document_version_id: Uuid,
    file_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateDocumentResultDto {
    document_id: Uuid,
    document_version_id: Uuid,
    file_id: Uuid,
}

async fn create_document<I, C, F, R>(
    State(state): State<CreateState<I, C, F, R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<(StatusCode, Json<CreateDocumentResultDto>), ApiError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    R: CreateApiRepository,
{
    const PATH: &str = "/v1/documents";
    if request_header_bytes(&headers) > state.limits.header_bytes {
        return Err(problem(
            ApplicationError::Validation("request headers exceed limit".into()),
            PATH,
            &trace,
        ));
    }
    let multipart = multipart.map_err(|_| {
        problem(
            ApplicationError::Validation("invalid multipart request".into()),
            PATH,
            &trace,
        )
    })?;
    let upload = parse_initial_upload(multipart, state.limits)
        .await
        .map_err(|failure| multipart_problem(failure, PATH, &trace))?;
    let request: CreateDocumentRequest =
        serde_json::from_slice(&upload.request_json).map_err(|_| {
            problem(
                ApplicationError::Validation("invalid create request".into()),
                PATH,
                &trace,
            )
        })?;
    let media_type = MediaType::new(upload.media_type)
        .map_err(|_| ApiProblem::new(ErrorCode::UnsupportedMediaType, PATH, &trace.trace_id))?;
    let scoped = Arc::new(state.repository.with_verified_actor(ctx.clone()));
    let result = DocumentService::new(state.ids, state.clock, state.storage, scoped)
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(request.folder_id),
            title: request.title,
            document_metadata: Metadata::from_map(request.document_metadata),
            version_metadata: Metadata::from_map(request.version_metadata),
            principal: ctx.principal().clone(),
            original_filename: upload.original_filename,
            media_type,
            content: upload.content,
        })
        .await
        .map_err(|error| problem(error, PATH, &trace))?;
    Ok((StatusCode::CREATED, Json(result_dto(result))))
}

async fn recover_create<I, C, F, R>(
    State(state): State<CreateState<I, C, F, R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    query: Result<Query<CreateOutcomeQuery>, QueryRejection>,
) -> Result<Json<CreateDocumentResultDto>, ApiError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    R: CreateApiRepository,
{
    let path = format!("/v1/document-creation-outcomes/{document_id}");
    let document_id = Uuid::parse_str(&document_id)
        .map(DocumentId::from_uuid)
        .map_err(|_| {
            problem(
                ApplicationError::Validation("invalid documentId".into()),
                &path,
                &trace,
            )
        })?;
    let Query(query) = query.map_err(|_| {
        problem(
            ApplicationError::Validation("invalid create outcome query".into()),
            &path,
            &trace,
        )
    })?;
    let probe = CreateOutcomeProbe {
        document_id,
        document_version_id: DocumentVersionId::from_uuid(query.document_version_id),
        file_id: FileId::from_uuid(query.file_id),
    };
    match CreateOutcomeRecoveryService::new(state.repository)
        .recover(&ctx, probe)
        .await
        .map_err(|error| problem(error, &path, &trace))?
    {
        Some(result) => Ok(Json(result_dto(result))),
        None => Err(ApiProblem::new(ErrorCode::DocumentNotFound, &path, &trace.trace_id).into()),
    }
}

fn multipart_problem(failure: MultipartFailure, path: &str, trace: &TraceContext) -> ApiError {
    match failure {
        MultipartFailure::Validation => problem(
            ApplicationError::Validation("invalid multipart request".into()),
            path,
            trace,
        ),
        MultipartFailure::UnsupportedMediaType => {
            ApiProblem::new(ErrorCode::UnsupportedMediaType, path, &trace.trace_id).into()
        }
        MultipartFailure::Internal => {
            ApiProblem::new(ErrorCode::Internal, path, &trace.trace_id).into()
        }
    }
}

fn problem(error: ApplicationError, path: &str, trace: &TraceContext) -> ApiError {
    ApiProblem::from_application(error, path, &trace.trace_id).into()
}

fn result_dto(result: CreateDocumentResult) -> CreateDocumentResultDto {
    CreateDocumentResultDto {
        document_id: result.document_id().as_uuid(),
        document_version_id: result.document_version_id().as_uuid(),
        file_id: result.file_id().as_uuid(),
    }
}
