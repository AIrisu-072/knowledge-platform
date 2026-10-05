use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::multipart::MultipartRejection;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{post, put};
use axum::{Extension, Json, Router};
use document_application::{
    ApplicationError, AuthorizationScope, Clock, CreateVersionCommand, DocumentRepository,
    DocumentVersionService, FileStorage, IdGenerator, RebaseWorkingVersionCommand,
    SemanticInspectionExecutor, SemanticInspectionRepository, UpdateWorkingVersionCommand,
    VerifiedActorContext, VersionOperationId, VersionOperationResult, VersioningItemInput,
    VersioningPreflight, VersioningRenditionInput, VersioningRepository,
};
use document_domain::{DocumentId, DocumentVersionId, FileId, LogicalPath, MediaType, Title};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiProblem, ErrorCode};
use crate::identity::IdentityAdapter;
use crate::limits::{
    MAX_JSON_BODY_BYTES, MULTIPART_OPERATION_TIMEOUT, ORDINARY_OPERATION_TIMEOUT, UploadLimits,
};
use crate::multipart::{
    MultipartFailure, VersionUpload, parse_version_upload, request_header_bytes, valid_media_type,
};
use crate::router::{StartupError, protect_routes};
use crate::timeout::with_operation_timeout;
use crate::trace::TraceContext;

pub trait VersioningApiRepository:
    AuthorizationScope
    + DocumentRepository
    + VersioningRepository
    + SemanticInspectionRepository
    + Send
    + Sync
    + 'static
{
}

impl<T> VersioningApiRepository for T where
    T: AuthorizationScope
        + DocumentRepository
        + VersioningRepository
        + SemanticInspectionRepository
        + Send
        + Sync
        + 'static
{
}

struct VersioningState<I, C, F, E, R> {
    ids: Arc<I>,
    clock: Arc<C>,
    storage: Arc<F>,
    executor: Arc<E>,
    repository: Arc<R>,
    limits: UploadLimits,
}

impl<I, C, F, E, R> Clone for VersioningState<I, C, F, E, R> {
    fn clone(&self) -> Self {
        Self {
            ids: self.ids.clone(),
            clock: self.clock.clone(),
            storage: self.storage.clone(),
            executor: self.executor.clone(),
            repository: self.repository.clone(),
            limits: self.limits,
        }
    }
}

pub fn versioning_router<I, C, F, E, R>(
    ids: Arc<I>,
    clock: Arc<C>,
    storage: Arc<F>,
    executor: Arc<E>,
    repository: Arc<R>,
    identity_adapter: Arc<dyn IdentityAdapter>,
) -> Result<Router, StartupError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    E: SemanticInspectionExecutor + 'static,
    R: VersioningApiRepository,
{
    let state = VersioningState {
        ids,
        clock,
        storage,
        executor,
        repository,
        limits: UploadLimits::PRODUCTION,
    };
    let multipart = Router::new()
        .route(
            "/v1/documents/{document_id}/versions",
            post(create_version::<I, C, F, E, R>),
        )
        .route(
            "/v1/documents/{document_id}/versions/{version_id}",
            put(update_version::<I, C, F, E, R>),
        )
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(state.limits.total_bytes));
    let rebase = Router::new()
        .route(
            "/v1/documents/{document_id}/versions/{version_id}",
            post(rebase_version::<I, C, F, E, R>),
        )
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES));
    protect_routes(
        with_operation_timeout(multipart, MULTIPART_OPERATION_TIMEOUT)
            .merge(with_operation_timeout(rebase, ORDINARY_OPERATION_TIMEOUT)),
        Some(identity_adapter),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VersionWriteRequest {
    operation_id: Uuid,
    target_version_id: Uuid,
    expected_revision: i64,
    title: String,
    items: Vec<VersionItemRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VersionItemRequest {
    logical_path: String,
    ordinal: u32,
    file_id: Uuid,
    part_id: String,
    media_type: String,
    original_filename: String,
    #[serde(default)]
    renditions: Vec<VersionRenditionRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VersionRenditionRequest {
    file_id: Uuid,
    part_id: String,
    media_type: String,
    original_filename: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RebaseRequest {
    operation_id: Uuid,
    expected_revision: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionMutationResultDto {
    operation_id: Uuid,
    document_id: Uuid,
    target_version_id: Uuid,
    version_no: i64,
    base_version_id: Option<Uuid>,
    resulting_revision: i64,
}

async fn create_version<I, C, F, E, R>(
    State(state): State<VersioningState<I, C, F, E, R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<(StatusCode, Json<VersionMutationResultDto>), ApiError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    E: SemanticInspectionExecutor + 'static,
    R: VersioningApiRepository,
{
    let path = format!("/v1/documents/{document_id}/versions");
    let document_id =
        document_id_value(&document_id).map_err(|error| problem(error, &path, &trace))?;
    let (request, upload) = receive_write(&state, &path, &trace, headers, multipart).await?;
    let prepared = prepare_manifest(&state, &ctx, &request, upload, &path, &trace).await?;
    let command = CreateVersionCommand::new(
        operation_id(request.operation_id).map_err(|error| problem(error, &path, &trace))?,
        document_id,
        DocumentVersionId::from_uuid(request.target_version_id),
        request.expected_revision,
        ctx.principal().clone(),
    )
    .map_err(|error| problem(error, &path, &trace))?;
    let result = version_service(&state, &ctx)
        .create_version(command, prepared)
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    Ok((StatusCode::CREATED, Json(result_dto(result))))
}

async fn update_version<I, C, F, E, R>(
    State(state): State<VersioningState<I, C, F, E, R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, version_id)): Path<(String, String)>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Json<VersionMutationResultDto>, ApiError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    E: SemanticInspectionExecutor + 'static,
    R: VersioningApiRepository,
{
    let path = format!("/v1/documents/{document_id}/versions/{version_id}");
    let document_id =
        document_id_value(&document_id).map_err(|error| problem(error, &path, &trace))?;
    let path_version =
        version_id_value(&version_id).map_err(|error| problem(error, &path, &trace))?;
    let (request, upload) = receive_write(&state, &path, &trace, headers, multipart).await?;
    let target = DocumentVersionId::from_uuid(request.target_version_id);
    if target != path_version {
        return Err(problem(
            validation("targetVersionId does not match path"),
            &path,
            &trace,
        ));
    }
    let prepared = prepare_manifest(&state, &ctx, &request, upload, &path, &trace).await?;
    let command = UpdateWorkingVersionCommand::new(
        operation_id(request.operation_id).map_err(|error| problem(error, &path, &trace))?,
        document_id,
        target,
        request.expected_revision,
        ctx.principal().clone(),
    )
    .map_err(|error| problem(error, &path, &trace))?;
    let result = version_service(&state, &ctx)
        .update_working(command, prepared)
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(result_dto(result)))
}

async fn rebase_version<I, C, F, E, R>(
    State(state): State<VersioningState<I, C, F, E, R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, version_id_and_action)): Path<(String, String)>,
    payload: Result<Json<RebaseRequest>, JsonRejection>,
) -> Result<Json<VersionMutationResultDto>, ApiError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    E: SemanticInspectionExecutor + 'static,
    R: VersioningApiRepository,
{
    let version_id = version_id_and_action
        .strip_suffix(":rebase")
        .ok_or_else(|| validation("rebase path must end with :rebase"))
        .map_err(|error| {
            problem(
                error,
                &format!("/v1/documents/{document_id}/versions/{version_id_and_action}"),
                &trace,
            )
        })?;
    let path = format!("/v1/documents/{document_id}/versions/{version_id}:rebase");
    let Json(request) =
        payload.map_err(|_| problem(validation("invalid JSON request"), &path, &trace))?;
    let command = RebaseWorkingVersionCommand::new(
        operation_id(request.operation_id).map_err(|error| problem(error, &path, &trace))?,
        document_id_value(&document_id).map_err(|error| problem(error, &path, &trace))?,
        version_id_value(version_id).map_err(|error| problem(error, &path, &trace))?,
        request.expected_revision,
        ctx.principal().clone(),
    )
    .map_err(|error| problem(error, &path, &trace))?;
    let result = version_service(&state, &ctx)
        .rebase_working(command)
        .await
        .map_err(|error| problem(error, &path, &trace))?;
    Ok(Json(result_dto(result)))
}

async fn receive_write<I, C, F, E, R>(
    state: &VersioningState<I, C, F, E, R>,
    path: &str,
    trace: &TraceContext,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<(VersionWriteRequest, VersionUpload), ApiError> {
    if request_header_bytes(&headers) > state.limits.header_bytes {
        return Err(problem(
            validation("request headers exceed limit"),
            path,
            trace,
        ));
    }
    let multipart =
        multipart.map_err(|_| problem(validation("invalid multipart request"), path, trace))?;
    let upload = parse_version_upload(multipart, state.limits)
        .await
        .map_err(|failure| multipart_problem(failure, path, trace))?;
    let request: VersionWriteRequest = serde_json::from_slice(&upload.request_json)
        .map_err(|_| problem(validation("invalid version request"), path, trace))?;
    Ok((request, upload))
}

async fn prepare_manifest<I, C, F, E, R>(
    state: &VersioningState<I, C, F, E, R>,
    ctx: &VerifiedActorContext,
    request: &VersionWriteRequest,
    upload: VersionUpload,
    path: &str,
    trace: &TraceContext,
) -> Result<document_application::PreparedManifest, ApiError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    E: SemanticInspectionExecutor + 'static,
    R: VersioningApiRepository,
{
    let (title, items) = bind_manifest(request, upload, state.limits)
        .map_err(|failure| build_problem(failure, path, trace))?;
    let repository = Arc::new(state.repository.with_verified_actor(ctx.clone()));
    VersioningPreflight::new(
        repository,
        state.storage.clone(),
        state.executor.clone(),
        state.clock.clone(),
    )
    .prepare_dsi_v0(title, items)
    .await
    .map_err(|error| problem(error, path, trace))
}

enum BuildFailure {
    Validation,
    UnsupportedMediaType,
}

fn bind_manifest(
    request: &VersionWriteRequest,
    mut upload: VersionUpload,
    limits: UploadLimits,
) -> Result<(Title, Vec<VersioningItemInput>), BuildFailure> {
    let title = Title::new(request.title.clone()).map_err(|_| BuildFailure::Validation)?;
    let mut part_ids = BTreeSet::new();
    let mut file_ids = BTreeSet::new();
    let mut items = Vec::with_capacity(request.items.len());
    for item in &request.items {
        validate_binding(
            &item.part_id,
            &item.original_filename,
            &item.media_type,
            item.file_id,
            limits,
            &mut part_ids,
            &mut file_ids,
        )?;
        let content = upload
            .files
            .remove(&item.part_id)
            .ok_or(BuildFailure::Validation)?;
        let mut renditions = Vec::with_capacity(item.renditions.len());
        for rendition in &item.renditions {
            validate_binding(
                &rendition.part_id,
                &rendition.original_filename,
                &rendition.media_type,
                rendition.file_id,
                limits,
                &mut part_ids,
                &mut file_ids,
            )?;
            renditions.push(VersioningRenditionInput::new(
                FileId::from_uuid(rendition.file_id),
                media_type(&rendition.media_type)?,
                rendition.original_filename.clone(),
                upload
                    .files
                    .remove(&rendition.part_id)
                    .ok_or(BuildFailure::Validation)?,
            ));
        }
        items.push(
            VersioningItemInput::new(
                LogicalPath::new(&item.logical_path).map_err(|_| BuildFailure::Validation)?,
                item.ordinal,
                FileId::from_uuid(item.file_id),
                media_type(&item.media_type)?,
                item.original_filename.clone(),
                content,
            )
            .with_renditions(renditions),
        );
    }
    if !upload.files.is_empty() {
        return Err(BuildFailure::Validation);
    }
    Ok((title, items))
}

#[allow(clippy::too_many_arguments)]
fn validate_binding(
    part_id: &str,
    original_filename: &str,
    media_type: &str,
    file_id: Uuid,
    limits: UploadLimits,
    part_ids: &mut BTreeSet<String>,
    file_ids: &mut BTreeSet<Uuid>,
) -> Result<(), BuildFailure> {
    if part_id.trim().is_empty()
        || part_id.len() > limits.filename_bytes
        || original_filename.trim().is_empty()
        || original_filename.len() > limits.filename_bytes
        || !part_ids.insert(part_id.to_owned())
        || !file_ids.insert(file_id)
    {
        return Err(BuildFailure::Validation);
    }
    if !valid_media_type(media_type) {
        return Err(BuildFailure::UnsupportedMediaType);
    }
    Ok(())
}

fn media_type(value: &str) -> Result<MediaType, BuildFailure> {
    if !valid_media_type(value) {
        return Err(BuildFailure::UnsupportedMediaType);
    }
    MediaType::new(value).map_err(|_| BuildFailure::UnsupportedMediaType)
}

fn version_service<I, C, F, E, R>(
    state: &VersioningState<I, C, F, E, R>,
    ctx: &VerifiedActorContext,
) -> DocumentVersionService<I, C, F, E, R>
where
    I: IdGenerator,
    C: Clock,
    F: FileStorage,
    E: SemanticInspectionExecutor,
    R: VersioningApiRepository,
{
    DocumentVersionService::new(
        state.ids.clone(),
        state.clock.clone(),
        state.storage.clone(),
        state.executor.clone(),
        Arc::new(state.repository.with_verified_actor(ctx.clone())),
    )
}

fn multipart_problem(failure: MultipartFailure, path: &str, trace: &TraceContext) -> ApiError {
    match failure {
        MultipartFailure::Validation => {
            problem(validation("invalid multipart request"), path, trace)
        }
        MultipartFailure::UnsupportedMediaType => {
            ApiProblem::new(ErrorCode::UnsupportedMediaType, path, &trace.trace_id).into()
        }
        MultipartFailure::Internal => {
            ApiProblem::new(ErrorCode::Internal, path, &trace.trace_id).into()
        }
    }
}

fn build_problem(failure: BuildFailure, path: &str, trace: &TraceContext) -> ApiError {
    match failure {
        BuildFailure::Validation => problem(validation("invalid version manifest"), path, trace),
        BuildFailure::UnsupportedMediaType => {
            ApiProblem::new(ErrorCode::UnsupportedMediaType, path, &trace.trace_id).into()
        }
    }
}

fn document_id_value(value: &str) -> Result<DocumentId, ApplicationError> {
    Uuid::parse_str(value)
        .map(DocumentId::from_uuid)
        .map_err(|_| validation("invalid documentId"))
}

fn version_id_value(value: &str) -> Result<DocumentVersionId, ApplicationError> {
    Uuid::parse_str(value)
        .map(DocumentVersionId::from_uuid)
        .map_err(|_| validation("invalid versionId"))
}

fn operation_id(value: Uuid) -> Result<VersionOperationId, ApplicationError> {
    VersionOperationId::try_from_uuid(value)
}

fn validation(message: &str) -> ApplicationError {
    ApplicationError::Validation(message.to_owned())
}

fn problem(error: ApplicationError, path: &str, trace: &TraceContext) -> ApiError {
    ApiProblem::from_application(error, path, &trace.trace_id).into()
}

fn result_dto(result: VersionOperationResult) -> VersionMutationResultDto {
    VersionMutationResultDto {
        operation_id: result.operation_id().as_uuid(),
        document_id: result.document_id().as_uuid(),
        target_version_id: result.target_version_id().as_uuid(),
        version_no: result.version_no(),
        base_version_id: result.base_version_id().map(|id| id.as_uuid()),
        resulting_revision: result.resulting_revision(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_working_update_result_serializes_an_explicit_null_base() {
        let result = VersionOperationResult::from_persisted(
            VersionOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
            DocumentId::from_uuid(Uuid::from_u128(1)),
            DocumentVersionId::from_uuid(Uuid::from_u128(2)),
            1,
            None,
            1,
        );
        let value = serde_json::to_value(result_dto(result)).unwrap();
        assert!(value.as_object().unwrap().contains_key("baseVersionId"));
        assert_eq!(value["baseVersionId"], serde_json::Value::Null);
        assert_eq!(value["versionNo"], 1);
    }
}
