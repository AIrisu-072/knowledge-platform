use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::routing::post;
use axum::{Extension, Json, Router};
use document_application::{
    ApplicationError, AuthorizationScope, AuthorizedDocumentService, CancelScheduleCommand, Clock,
    DocumentPublishRepository, DocumentRepository, EndDocumentPublicationCommand, FileStorage,
    IdGenerator, PublicationEndOperationId, PublicationEndRepository,
    PublicationScheduleRepository, PublishDocumentCommand, PublishDocumentResult,
    PublishOperationId, SchedulePublishCommand, SchedulePublishResult, SemanticInspectionExecutor,
    SemanticInspectionRepository, VerifiedActorContext, VersionOperationId, VersioningRepository,
    WithdrawVersionCommand, WithdrawVersionResult,
};
use document_domain::DocumentVersionId;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::error::ApiError;
use crate::identity::IdentityAdapter;
use crate::limits::{MAX_JSON_BODY_BYTES, ORDINARY_OPERATION_TIMEOUT};
use crate::management::{document_id_value, format_time, problem, validation};
use crate::router::{StartupError, protect_routes};
use crate::timeout::with_operation_timeout;
use crate::trace::TraceContext;

pub trait PublicationApiRepository:
    AuthorizationScope
    + DocumentRepository
    + VersioningRepository
    + SemanticInspectionRepository
    + DocumentPublishRepository
    + PublicationScheduleRepository
    + PublicationEndRepository
    + Send
    + Sync
    + 'static
{
}

impl<T> PublicationApiRepository for T where
    T: AuthorizationScope
        + DocumentRepository
        + VersioningRepository
        + SemanticInspectionRepository
        + DocumentPublishRepository
        + PublicationScheduleRepository
        + PublicationEndRepository
        + Send
        + Sync
        + 'static
{
}

struct PublicationState<I, C, F, E, R> {
    ids: Arc<I>,
    clock: Arc<C>,
    storage: Arc<F>,
    executor: Arc<E>,
    repository: Arc<R>,
}

impl<I, C, F, E, R> Clone for PublicationState<I, C, F, E, R> {
    fn clone(&self) -> Self {
        Self {
            ids: self.ids.clone(),
            clock: self.clock.clone(),
            storage: self.storage.clone(),
            executor: self.executor.clone(),
            repository: self.repository.clone(),
        }
    }
}

pub fn publication_router<I, C, F, E, R>(
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
    R: PublicationApiRepository,
{
    let state = PublicationState {
        ids,
        clock,
        storage,
        executor,
        repository,
    };
    let routes = Router::new()
        .route(
            "/v1/documents/{document_id}/versions/{version_action}",
            post(version_action::<I, C, F, E, R>),
        )
        .route(
            "/v1/documents/{document_action}",
            post(document_action::<I, C, F, E, R>),
        )
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES));
    protect_routes(
        with_operation_timeout(routes, ORDINARY_OPERATION_TIMEOUT),
        Some(identity_adapter),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishRequest {
    operation_id: Uuid,
    expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WithdrawRequest {
    operation_id: Uuid,
    expected_revision: i64,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScheduleRequest {
    operation_id: Uuid,
    expected_revision: i64,
    scheduled_publish_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelRequest {
    operation_id: Uuid,
    publish_operation_id: Uuid,
    expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EndPublicationRequest {
    operation_id: Uuid,
    expected_revision: i64,
    expected_current_version_id: Uuid,
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum LifecycleResultDto {
    Publish(PublishResultDto),
    Withdraw(WithdrawResultDto),
    Schedule(ScheduleResultDto),
    Cancel(CancelResultDto),
    End(EndPublicationResultDto),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishResultDto {
    publish_operation_id: Uuid,
    document_id: Uuid,
    document_version_id: Uuid,
    resulting_document_revision: i64,
    published_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WithdrawResultDto {
    operation_id: Uuid,
    document_id: Uuid,
    target_version_id: Uuid,
    former_current_version_id: Option<Uuid>,
    resulting_current_version_id: Option<Uuid>,
    resulting_revision: i64,
    restoration_withheld_reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleResultDto {
    publish_operation_id: Uuid,
    document_id: Uuid,
    target_version_id: Uuid,
    accepted_revision: i64,
    scheduled_publish_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CancelResultDto {
    operation_id: Uuid,
    publish_operation_id: Uuid,
    document_id: Uuid,
    target_version_id: Uuid,
    resulting_revision: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EndPublicationResultDto {
    operation_id: Uuid,
    document_id: Uuid,
    former_current_version_id: Uuid,
    resulting_current_version_id: Option<Uuid>,
    resulting_document_revision: i64,
    ended_at: String,
}

#[derive(Debug, Clone, Copy)]
enum VersionAction {
    Publish,
    Withdraw,
    Schedule,
    Cancel,
}

async fn version_action<I, C, F, E, R>(
    State(state): State<PublicationState<I, C, F, E, R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path((document_id, version_action)): Path<(String, String)>,
    payload: Result<Json<Value>, JsonRejection>,
) -> Result<Json<LifecycleResultDto>, ApiError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    E: SemanticInspectionExecutor + 'static,
    R: PublicationApiRepository,
{
    let raw_path = format!("/v1/documents/{document_id}/versions/{version_action}");
    let (version_id, action) =
        parse_version_action(&version_action).map_err(|error| problem(error, &raw_path, &trace))?;
    let document_id =
        document_id_value(&document_id).map_err(|error| problem(error, &raw_path, &trace))?;
    let version_id =
        version_id_value(version_id).map_err(|error| problem(error, &raw_path, &trace))?;
    let value = payload
        .map(|Json(value)| value)
        .map_err(|_| problem(validation("invalid JSON request"), &raw_path, &trace))?;
    let service = AuthorizedDocumentService::new(
        state.ids,
        state.clock,
        state.storage,
        state.executor,
        state.repository,
    );
    let result = match action {
        VersionAction::Publish => {
            let request: PublishRequest = parse_payload(value, &raw_path, &trace)?;
            let command = PublishDocumentCommand::new(
                publish_operation_id(request.operation_id)
                    .map_err(|error| problem(error, &raw_path, &trace))?,
                document_id,
                version_id,
                request.expected_revision,
                ctx.principal().clone(),
            )
            .map_err(|error| problem(error, &raw_path, &trace))?;
            LifecycleResultDto::Publish(publish_dto(
                service
                    .publish_document(&ctx, command)
                    .await
                    .map_err(|error| problem(error, &raw_path, &trace))?,
            ))
        }
        VersionAction::Withdraw => {
            let request: WithdrawRequest = parse_payload(value, &raw_path, &trace)?;
            let command = WithdrawVersionCommand::new(
                version_operation_id(request.operation_id)
                    .map_err(|error| problem(error, &raw_path, &trace))?,
                document_id,
                version_id,
                request.expected_revision,
                ctx.principal().clone(),
                request.reason,
            )
            .map_err(|error| problem(error, &raw_path, &trace))?;
            LifecycleResultDto::Withdraw(withdraw_dto(
                service
                    .withdraw_version(&ctx, command)
                    .await
                    .map_err(|error| problem(error, &raw_path, &trace))?,
            ))
        }
        VersionAction::Schedule => {
            let request: ScheduleRequest = parse_payload(value, &raw_path, &trace)?;
            let scheduled_publish_at =
                OffsetDateTime::parse(&request.scheduled_publish_at, &Rfc3339).map_err(|_| {
                    problem(validation("invalid scheduledPublishAt"), &raw_path, &trace)
                })?;
            let command = SchedulePublishCommand::new(
                publish_operation_id(request.operation_id)
                    .map_err(|error| problem(error, &raw_path, &trace))?,
                document_id,
                version_id,
                request.expected_revision,
                ctx.principal().clone(),
                scheduled_publish_at,
            )
            .map_err(|error| problem(error, &raw_path, &trace))?;
            LifecycleResultDto::Schedule(schedule_dto(
                service
                    .schedule_publish(&ctx, command)
                    .await
                    .map_err(|error| problem(error, &raw_path, &trace))?,
            ))
        }
        VersionAction::Cancel => {
            let request: CancelRequest = parse_payload(value, &raw_path, &trace)?;
            let command = CancelScheduleCommand::new(
                version_operation_id(request.operation_id)
                    .map_err(|error| problem(error, &raw_path, &trace))?,
                publish_operation_id(request.publish_operation_id)
                    .map_err(|error| problem(error, &raw_path, &trace))?,
                document_id,
                version_id,
                request.expected_revision,
                ctx.principal().clone(),
            )
            .map_err(|error| problem(error, &raw_path, &trace))?;
            let result = service
                .cancel_schedule(&ctx, command)
                .await
                .map_err(|error| problem(error, &raw_path, &trace))?;
            LifecycleResultDto::Cancel(CancelResultDto {
                operation_id: result.operation_id.as_uuid(),
                publish_operation_id: result.publish_operation_id.as_uuid(),
                document_id: result.document_id.as_uuid(),
                target_version_id: result.target_version_id.as_uuid(),
                resulting_revision: result.resulting_revision,
            })
        }
    };
    Ok(Json(result))
}

async fn document_action<I, C, F, E, R>(
    State(state): State<PublicationState<I, C, F, E, R>>,
    Extension(ctx): Extension<VerifiedActorContext>,
    Extension(trace): Extension<TraceContext>,
    Path(document_action): Path<String>,
    payload: Result<Json<Value>, JsonRejection>,
) -> Result<Json<LifecycleResultDto>, ApiError>
where
    I: IdGenerator + 'static,
    C: Clock + 'static,
    F: FileStorage + 'static,
    E: SemanticInspectionExecutor + 'static,
    R: PublicationApiRepository,
{
    let raw_path = format!("/v1/documents/{document_action}");
    let document_id = document_action
        .strip_suffix(":end-publication")
        .ok_or_else(|| validation("publication end path must end with :end-publication"))
        .and_then(document_id_value)
        .map_err(|error| problem(error, &raw_path, &trace))?;
    let value = payload
        .map(|Json(value)| value)
        .map_err(|_| problem(validation("invalid JSON request"), &raw_path, &trace))?;
    let request: EndPublicationRequest = parse_payload(value, &raw_path, &trace)?;
    let command = EndDocumentPublicationCommand::new(
        PublicationEndOperationId::try_from_uuid(request.operation_id)
            .map_err(|error| problem(error, &raw_path, &trace))?,
        document_id,
        request.expected_revision,
        DocumentVersionId::from_uuid(request.expected_current_version_id),
        ctx.principal().clone(),
        request.reason,
    )
    .map_err(|error| problem(error, &raw_path, &trace))?;
    let result = AuthorizedDocumentService::new(
        state.ids,
        state.clock,
        state.storage,
        state.executor,
        state.repository,
    )
    .end_document_publication(&ctx, command)
    .await
    .map_err(|error| problem(error, &raw_path, &trace))?;
    Ok(Json(LifecycleResultDto::End(EndPublicationResultDto {
        operation_id: result.operation_id().as_uuid(),
        document_id: result.document_id().as_uuid(),
        former_current_version_id: result.former_current_version_id().as_uuid(),
        resulting_current_version_id: result.resulting_current_version_id().map(|id| id.as_uuid()),
        resulting_document_revision: result.resulting_document_revision(),
        ended_at: format_time(result.ended_at()),
    })))
}

fn parse_version_action(value: &str) -> Result<(&str, VersionAction), ApplicationError> {
    for (suffix, action) in [
        (":publish", VersionAction::Publish),
        (":withdraw", VersionAction::Withdraw),
        (":schedule-publication", VersionAction::Schedule),
        (":cancel-publication-schedule", VersionAction::Cancel),
    ] {
        if let Some(version_id) = value.strip_suffix(suffix) {
            return Ok((version_id, action));
        }
    }
    Err(validation("unknown publication lifecycle action"))
}

fn parse_payload<T: DeserializeOwned>(
    value: Value,
    path: &str,
    trace: &TraceContext,
) -> Result<T, ApiError> {
    serde_json::from_value(value)
        .map_err(|_| problem(validation("invalid lifecycle request"), path, trace))
}

fn version_id_value(value: &str) -> Result<DocumentVersionId, ApplicationError> {
    Uuid::parse_str(value)
        .map(DocumentVersionId::from_uuid)
        .map_err(|_| validation("invalid versionId"))
}

fn publish_operation_id(value: Uuid) -> Result<PublishOperationId, ApplicationError> {
    PublishOperationId::try_from_uuid(value)
}

fn version_operation_id(value: Uuid) -> Result<VersionOperationId, ApplicationError> {
    VersionOperationId::try_from_uuid(value)
}

fn publish_dto(result: PublishDocumentResult) -> PublishResultDto {
    PublishResultDto {
        publish_operation_id: result.publish_operation_id().as_uuid(),
        document_id: result.document_id().as_uuid(),
        document_version_id: result.document_version_id().as_uuid(),
        resulting_document_revision: result.resulting_document_revision(),
        published_at: format_time(result.published_at()),
    }
}

fn withdraw_dto(result: WithdrawVersionResult) -> WithdrawResultDto {
    WithdrawResultDto {
        operation_id: result.operation_id.as_uuid(),
        document_id: result.document_id.as_uuid(),
        target_version_id: result.target_version_id.as_uuid(),
        former_current_version_id: result.former_current_version_id.map(|id| id.as_uuid()),
        resulting_current_version_id: result.resulting_current_version_id.map(|id| id.as_uuid()),
        resulting_revision: result.resulting_revision,
        restoration_withheld_reason: result.restoration_withheld_reason,
    }
}

fn schedule_dto(result: SchedulePublishResult) -> ScheduleResultDto {
    ScheduleResultDto {
        publish_operation_id: result.publish_operation_id.as_uuid(),
        document_id: result.document_id.as_uuid(),
        target_version_id: result.target_version_id.as_uuid(),
        accepted_revision: result.accepted_revision,
        scheduled_publish_at: format_time(result.scheduled_publish_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ApiProblem;
    use document_domain::DocumentId;

    #[test]
    fn commit_unknown_requires_exact_retry() {
        let problem = ApiProblem::from_application(
            ApplicationError::PublishCommitOutcomeUnknown {
                publish_operation_id: PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                document_id: DocumentId::from_uuid(Uuid::now_v7()),
                document_version_id: DocumentVersionId::from_uuid(Uuid::now_v7()),
            },
            "/v1/documents/id/versions/id:publish",
            "trace",
        );
        assert_eq!(problem.status, 503);
        assert!(problem.retryable);
        assert_eq!(problem.exact_retry, Some(true));
    }
}
