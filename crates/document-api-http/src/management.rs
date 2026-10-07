use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{patch, post, put};
use axum::{Extension, Json, Router};
use document_application::{
    AccessPolicyService, ApplicationError, CurrentReadStateRepository, DocumentManagementService,
    FolderService, ManagementCommand, ManagementMutationResult, ManagementOperationId,
    ManagementRepository, ReadStateRepository, VerifiedActorContext,
};
use document_domain::{
    Action, DocumentId, FolderId, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind,
    PolicyTarget, ResourceRef,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::error::{ApiError, ApiProblem};
use crate::identity::IdentityAdapter;
use crate::limits::ORDINARY_OPERATION_TIMEOUT;
use crate::router::{StartupError, protect_routes};
use crate::timeout::with_operation_timeout;
use crate::trace::TraceContext;

pub trait ManagementApiRepository:
    ManagementRepository + ReadStateRepository + CurrentReadStateRepository + Send + Sync + 'static
{
}

impl<T> ManagementApiRepository for T where
    T: ManagementRepository
        + ReadStateRepository
        + CurrentReadStateRepository
        + Send
        + Sync
        + 'static
{
}

pub fn management_router<R: ManagementApiRepository>(
    repository: Arc<R>,
    identity_adapter: Arc<dyn IdentityAdapter>,
) -> Result<Router, StartupError> {
    let routes = Router::new()
        .route(
            "/v1/documents/{document_id}/metadata",
            patch(update_document_metadata::<R>),
        )
        .route("/v1/documents/{document_id}", post(move_document::<R>))
        .route("/v1/folders", post(create_folder::<R>))
        .route(
            "/v1/folders/{folder_id}",
            patch(rename_folder::<R>).post(move_folder::<R>),
        )
        .route(
            "/v1/documents/{document_id}/access-policy",
            put(set_document_policy::<R>),
        )
        .route(
            "/v1/folders/{folder_id}/access-policy",
            put(set_folder_policy::<R>),
        )
        .merge(crate::read_state::read_state_routes::<R>())
        .with_state(repository);
    protect_routes(
        with_operation_timeout(routes, ORDINARY_OPERATION_TIMEOUT),
        Some(identity_adapter),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetadataPatchRequest {
    operation_id: Uuid,
    expected_document_revision: i64,
    set: BTreeMap<String, Value>,
    unset: Vec<String>,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MoveDocumentRequest {
    operation_id: Uuid,
    from_folder_id: Uuid,
    to_folder_id: Uuid,
    expected_document_revision: i64,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateFolderRequest {
    operation_id: Uuid,
    folder_id: Uuid,
    parent_folder_id: Uuid,
    expected_parent_revision: i64,
    name: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RenameFolderRequest {
    operation_id: Uuid,
    expected_folder_revision: i64,
    name: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MoveFolderRequest {
    operation_id: Uuid,
    from_parent_id: Uuid,
    to_parent_id: Uuid,
    expected_folder_revision: i64,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "mode",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum SetAccessPolicyRequest {
    Inherit {
        operation_id: Uuid,
        expected_policy_revision: i64,
        reason: String,
    },
    Explicit {
        operation_id: Uuid,
        expected_policy_revision: i64,
        reason: String,
        grants: Vec<PolicyGrantRequest>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyGrantRequest {
    subject_kind: PolicySubjectKindRequest,
    identity_provider: String,
    subject_id: String,
    actions: Vec<ActionRequest>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PolicySubjectKindRequest {
    Principal,
    Group,
    Role,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ActionRequest {
    Read,
    ReadHistory,
    Write,
    Publish,
    Administer,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MutationResultDto {
    operation_id: Uuid,
    resource_id: Uuid,
    resulting_revision: i64,
    changed: bool,
    occurred_at: String,
}

async fn update_document_metadata<R: ManagementRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    payload: Result<Json<MetadataPatchRequest>, JsonRejection>,
) -> Result<Json<MutationResultDto>, ApiError> {
    let path = format!("/v1/documents/{document_id}/metadata");
    let request = payload_value(payload, &path, &trace)?;
    let unset = unique_strings(request.unset).map_err(|error| problem(error, &path, &trace))?;
    let command = ManagementCommand::UpdateDocumentMetadata {
        operation_id: operation_id(request.operation_id)
            .map_err(|error| problem(error, &path, &trace))?,
        document_id: document_id_value(&document_id)
            .map_err(|error| problem(error, &path, &trace))?,
        expected_document_revision: request.expected_document_revision,
        set: request.set,
        unset,
        reason: request.reason,
    };
    DocumentManagementService::new(repository)
        .update_document_metadata(&ctx, command)
        .await
        .map(mutation_dto)
        .map(Json)
        .map_err(|error| problem(error, &path, &trace))
}

async fn move_document<R: ManagementRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id_and_action): Path<String>,
    payload: Result<Json<MoveDocumentRequest>, JsonRejection>,
) -> Result<Json<MutationResultDto>, ApiError> {
    let document_id = document_id_and_action
        .strip_suffix(":move")
        .ok_or_else(|| validation("document move path must end with :move"))
        .map_err(|error| {
            problem(
                error,
                &format!("/v1/documents/{document_id_and_action}"),
                &trace,
            )
        })?;
    let path = format!("/v1/documents/{document_id}:move");
    let request = payload_value(payload, &path, &trace)?;
    let command = ManagementCommand::MoveDocument {
        operation_id: operation_id(request.operation_id)
            .map_err(|error| problem(error, &path, &trace))?,
        document_id: document_id_value(document_id)
            .map_err(|error| problem(error, &path, &trace))?,
        from_folder_id: FolderId::from_uuid(request.from_folder_id),
        to_folder_id: FolderId::from_uuid(request.to_folder_id),
        expected_document_revision: request.expected_document_revision,
        reason: request.reason,
    };
    DocumentManagementService::new(repository)
        .move_document(&ctx, command)
        .await
        .map(mutation_dto)
        .map(Json)
        .map_err(|error| problem(error, &path, &trace))
}

async fn create_folder<R: ManagementRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    payload: Result<Json<CreateFolderRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<MutationResultDto>), ApiError> {
    let path = "/v1/folders";
    let request = payload_value(payload, path, &trace)?;
    let command = ManagementCommand::CreateFolder {
        operation_id: operation_id(request.operation_id)
            .map_err(|error| problem(error, path, &trace))?,
        folder_id: FolderId::from_uuid(request.folder_id),
        parent_folder_id: FolderId::from_uuid(request.parent_folder_id),
        expected_parent_revision: request.expected_parent_revision,
        name: request.name,
        reason: request.reason,
    };
    FolderService::new(repository)
        .create_folder(&ctx, command)
        .await
        .map(mutation_dto)
        .map(|result| (StatusCode::CREATED, Json(result)))
        .map_err(|error| problem(error, path, &trace))
}

async fn rename_folder<R: ManagementRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(folder_id): Path<String>,
    payload: Result<Json<RenameFolderRequest>, JsonRejection>,
) -> Result<Json<MutationResultDto>, ApiError> {
    let path = format!("/v1/folders/{folder_id}");
    let request = payload_value(payload, &path, &trace)?;
    let command = ManagementCommand::RenameFolder {
        operation_id: operation_id(request.operation_id)
            .map_err(|error| problem(error, &path, &trace))?,
        folder_id: folder_id_value(&folder_id).map_err(|error| problem(error, &path, &trace))?,
        expected_folder_revision: request.expected_folder_revision,
        name: request.name,
        reason: request.reason,
    };
    FolderService::new(repository)
        .rename_folder(&ctx, command)
        .await
        .map(mutation_dto)
        .map(Json)
        .map_err(|error| problem(error, &path, &trace))
}

async fn move_folder<R: ManagementRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(folder_id_and_action): Path<String>,
    payload: Result<Json<MoveFolderRequest>, JsonRejection>,
) -> Result<Json<MutationResultDto>, ApiError> {
    let folder_id = folder_id_and_action
        .strip_suffix(":move")
        .ok_or_else(|| validation("folder move path must end with :move"))
        .map_err(|error| {
            problem(
                error,
                &format!("/v1/folders/{folder_id_and_action}"),
                &trace,
            )
        })?;
    let path = format!("/v1/folders/{folder_id}:move");
    let request = payload_value(payload, &path, &trace)?;
    let command = ManagementCommand::MoveFolder {
        operation_id: operation_id(request.operation_id)
            .map_err(|error| problem(error, &path, &trace))?,
        folder_id: folder_id_value(folder_id).map_err(|error| problem(error, &path, &trace))?,
        from_parent_id: FolderId::from_uuid(request.from_parent_id),
        to_parent_id: FolderId::from_uuid(request.to_parent_id),
        expected_folder_revision: request.expected_folder_revision,
        reason: request.reason,
    };
    FolderService::new(repository)
        .move_folder(&ctx, command)
        .await
        .map(mutation_dto)
        .map(Json)
        .map_err(|error| problem(error, &path, &trace))
}

async fn set_document_policy<R: ManagementRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_id): Path<String>,
    payload: Result<Json<SetAccessPolicyRequest>, JsonRejection>,
) -> Result<Json<MutationResultDto>, ApiError> {
    let path = format!("/v1/documents/{document_id}/access-policy");
    let target = PolicyTarget::Document(
        document_id_value(&document_id).map_err(|error| problem(error, &path, &trace))?,
    );
    set_policy(repository, ctx, trace, path, target, payload).await
}

async fn set_folder_policy<R: ManagementRepository>(
    State(repository): State<Arc<R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(folder_id): Path<String>,
    payload: Result<Json<SetAccessPolicyRequest>, JsonRejection>,
) -> Result<Json<MutationResultDto>, ApiError> {
    let path = format!("/v1/folders/{folder_id}/access-policy");
    let target = PolicyTarget::Folder(
        folder_id_value(&folder_id).map_err(|error| problem(error, &path, &trace))?,
    );
    set_policy(repository, ctx, trace, path, target, payload).await
}

async fn set_policy<R: ManagementRepository>(
    repository: Arc<R>,
    ctx: VerifiedActorContext,
    trace: TraceContext,
    path: String,
    target: PolicyTarget,
    payload: Result<Json<SetAccessPolicyRequest>, JsonRejection>,
) -> Result<Json<MutationResultDto>, ApiError> {
    let request = payload_value(payload, &path, &trace)?;
    let (raw_operation_id, expected_policy_revision, reason, mode) = match request {
        SetAccessPolicyRequest::Inherit {
            operation_id,
            expected_policy_revision,
            reason,
        } => (
            operation_id,
            expected_policy_revision,
            reason,
            PolicyMode::Inherit,
        ),
        SetAccessPolicyRequest::Explicit {
            operation_id,
            expected_policy_revision,
            reason,
            grants,
        } => (
            operation_id,
            expected_policy_revision,
            reason,
            PolicyMode::Explicit(
                grants
                    .into_iter()
                    .map(policy_grant)
                    .collect::<Result<_, _>>()
                    .map_err(|error| problem(error, &path, &trace))?,
            ),
        ),
    };
    let command = ManagementCommand::SetAccessPolicy {
        operation_id: operation_id(raw_operation_id)
            .map_err(|error| problem(error, &path, &trace))?,
        target,
        expected_policy_revision,
        mode,
        reason,
    };
    AccessPolicyService::new(repository)
        .set_access_policy(&ctx, command)
        .await
        .map(mutation_dto)
        .map(Json)
        .map_err(|error| problem(error, &path, &trace))
}

fn policy_grant(request: PolicyGrantRequest) -> Result<PolicyGrant, ApplicationError> {
    let kind = match request.subject_kind {
        PolicySubjectKindRequest::Principal => PolicySubjectKind::Principal,
        PolicySubjectKindRequest::Group => PolicySubjectKind::Group,
        PolicySubjectKindRequest::Role => PolicySubjectKind::Role,
    };
    let subject = PolicySubject::new(kind, request.identity_provider, request.subject_id)
        .map_err(|_| validation("invalid policy subject"))?;
    let actions = request.actions.into_iter().map(|action| match action {
        ActionRequest::Read => Action::Read,
        ActionRequest::ReadHistory => Action::ReadHistory,
        ActionRequest::Write => Action::Write,
        ActionRequest::Publish => Action::Publish,
        ActionRequest::Administer => Action::Administer,
    });
    PolicyGrant::new(subject, actions).map_err(|_| validation("invalid policy grant"))
}

fn unique_strings(values: Vec<String>) -> Result<BTreeSet<String>, ApplicationError> {
    let count = values.len();
    let unique: BTreeSet<_> = values.into_iter().collect();
    if unique.len() != count {
        return Err(validation("unset entries must be unique"));
    }
    Ok(unique)
}

fn payload_value<T>(
    payload: Result<Json<T>, JsonRejection>,
    path: &str,
    trace: &TraceContext,
) -> Result<T, ApiError> {
    payload
        .map(|Json(value)| value)
        .map_err(|_| problem(validation("invalid JSON request"), path, trace))
}

pub(crate) fn problem(error: ApplicationError, path: &str, trace: &TraceContext) -> ApiError {
    ApiProblem::from_application(error, path, &trace.trace_id).into()
}

pub(crate) fn validation(message: &str) -> ApplicationError {
    ApplicationError::Validation(message.to_owned())
}

pub(crate) fn document_id_value(value: &str) -> Result<DocumentId, ApplicationError> {
    Uuid::parse_str(value)
        .map(DocumentId::from_uuid)
        .map_err(|_| validation("invalid documentId"))
}

pub(crate) fn folder_id_value(value: &str) -> Result<FolderId, ApplicationError> {
    Uuid::parse_str(value)
        .map(FolderId::from_uuid)
        .map_err(|_| validation("invalid folderId"))
}

fn operation_id(value: Uuid) -> Result<ManagementOperationId, ApplicationError> {
    ManagementOperationId::try_from_uuid(value)
}

fn mutation_dto(value: ManagementMutationResult) -> MutationResultDto {
    let resource_id = match value.resource {
        ResourceRef::Document(id) => id.as_uuid(),
        ResourceRef::Folder(id) => id.as_uuid(),
        ResourceRef::AccessPolicy(id) => id.as_uuid(),
    };
    MutationResultDto {
        operation_id: value.operation_id.as_uuid(),
        resource_id,
        resulting_revision: value.resulting_revision,
        changed: value.changed,
        occurred_at: format_time(value.occurred_at),
    }
}

pub(crate) fn format_time(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .expect("OffsetDateTime always formats as RFC3339")
}
