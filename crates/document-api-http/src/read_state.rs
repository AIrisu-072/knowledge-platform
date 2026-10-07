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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use document_application::{InvocationKind, RepositoryError};
    use document_domain::{DocumentId, PolicySubject, PolicySubjectKind, PrincipalRef};
    use serde_json::{Value, json};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use time::{Duration, OffsetDateTime};
    use tower::ServiceExt;

    #[derive(Default)]
    struct FakeRepository {
        calls: AtomicUsize,
        error: Option<RepositoryError>,
    }

    impl ReadStateRepository for FakeRepository {
        async fn mark_version_read(
            &self,
            ctx: &VerifiedActorContext,
            command: MarkVersionRead,
        ) -> Result<document_application::ReadStateResult, RepositoryError> {
            Ok(document_application::ReadStateResult {
                principal: ctx.principal().clone(),
                document_version_id: command.document_version_id,
                first_read_at: OffsetDateTime::UNIX_EPOCH,
                inserted: false,
            })
        }
    }

    impl CurrentReadStateRepository for FakeRepository {
        async fn get_current_read_state(
            &self,
            _: &VerifiedActorContext,
            document_id: DocumentId,
            document_version_id: DocumentVersionId,
        ) -> Result<CurrentReadState, RepositoryError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            Ok(CurrentReadState {
                document_id,
                document_version_id,
                state: CurrentReadProjection::default(),
            })
        }

        async fn mutate_read_state(
            &self,
            _: &VerifiedActorContext,
            command: ReadStateMutation,
        ) -> Result<ReadStateMutationResult, RepositoryError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            Ok(ReadStateMutationResult {
                operation_id: command.operation_id,
                document_id: command.document_id,
                document_version_id: command.document_version_id,
                kind: command.kind,
                expected_read_state_revision: command.expected_read_state_revision,
                changed: true,
                occurred_at: OffsetDateTime::UNIX_EPOCH,
                resulting_read_state: CurrentReadProjection {
                    first_read_at: Some(OffsetDateTime::UNIX_EPOCH),
                    needs_recheck: command.kind == ReadStateMutationKind::Reset,
                    read_state_revision: command.expected_read_state_revision + 1,
                },
            })
        }
    }

    fn router(repository: Arc<FakeRepository>, kind: InvocationKind) -> Router {
        let context = VerifiedActorContext::from_trusted_adapter(
            PrincipalRef::new("test-idp", "alice").unwrap(),
            vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "alice").unwrap()],
            OffsetDateTime::now_utc() + Duration::hours(1),
            kind,
            None,
        ).unwrap();
        read_state_routes::<FakeRepository>().with_state(repository)
            .layer(Extension(context))
            .layer(Extension(TraceContext::from_traceparent(None)))
    }

    const PATH: &str = "/v1/documents/00000000-0000-4000-8000-000000000001/versions/00000000-0000-4000-8000-000000000002/read-state";

    async fn request(router: Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value, Option<String>) {
        let mut request = Request::builder().method(method).uri(path);
        let body = match body {
            Some(value) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let response = router.oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let cache = response.headers().get(header::CACHE_CONTROL).map(|value| value.to_str().unwrap().to_owned());
        let body = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap();
        (status, body, cache)
    }

    #[tokio::test]
    async fn human_get_is_no_store_and_agent_get_does_not_call_repository() {
        let repository = Arc::new(FakeRepository::default());
        let (status, body, cache) = request(router(repository.clone(), InvocationKind::HumanInteractive), "GET", PATH, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["readStateRevision"], 0);
        assert_eq!(body["isRead"], false);
        assert_eq!(body["needsRecheck"], false);
        assert_eq!(body["firstReadAt"], Value::Null);
        assert_eq!(cache.as_deref(), Some("private, no-store"));
        let (status, body, _) = request(router(repository.clone(), InvocationKind::Agent), "GET", PATH, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "FORBIDDEN");
        assert_eq!(repository.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn strict_mutation_body_rejects_claims_kind_reason_fraction_and_unsafe_revision() {
        let repository = Arc::new(FakeRepository::default());
        let id = Uuid::now_v7();
        for body in [
            json!({"operationId": id, "expectedReadStateRevision": 0, "principal": "mallory"}),
            json!({"operationId": id, "expectedReadStateRevision": 0, "invocationKind": "human_interactive"}),
            json!({"operationId": id, "expectedReadStateRevision": 0, "kind": "VIEW"}),
            json!({"operationId": id, "expectedReadStateRevision": 0, "reason": "arbitrary"}),
            json!({"operationId": Uuid::nil(), "expectedReadStateRevision": 0}),
            json!({"operationId": id, "expectedReadStateRevision": 0.5}),
            json!({"operationId": id, "expectedReadStateRevision": -1}),
            json!({"operationId": id, "expectedReadStateRevision": 9007199254740992_i64}),
        ] {
            let (status, body, _) = request(router(repository.clone(), InvocationKind::HumanInteractive), "POST", &format!("{PATH}/view"), Some(body)).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(body["code"], "VALIDATION_FAILED");
        }
        assert_eq!(repository.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn repository_errors_keep_authorization_priority_and_unknown_exact_retry() {
        for (error, expected_status, code) in [
            (RepositoryError::Forbidden, 403, "FORBIDDEN"),
            (RepositoryError::DocumentNotFound, 404, "DOCUMENT_NOT_FOUND"),
            (RepositoryError::StaleVersion, 409, "STALE_VERSION"),
            (RepositoryError::ReadStateRevisionConflict, 409, "REVISION_CONFLICT"),
            (RepositoryError::OperationConflict, 409, "OPERATION_CONFLICT"),
            (RepositoryError::CommitOutcomeUnknown, 503, "COMMIT_OUTCOME_UNKNOWN"),
        ] {
            let repository = Arc::new(FakeRepository { error: Some(error), ..FakeRepository::default() });
            let (status, body, _) = request(router(repository, InvocationKind::HumanInteractive), "POST", &format!("{PATH}/reset"), Some(json!({"operationId": Uuid::now_v7(), "expectedReadStateRevision": 1}))).await;
            assert_eq!(status.as_u16(), expected_status);
            assert_eq!(body["code"], code);
            if code == "COMMIT_OUTCOME_UNKNOWN" {
                assert_eq!(body["retryable"], true);
                assert_eq!(body["exactRetry"], true);
            }
            if code == "REVISION_CONFLICT" {
                assert!(body["detail"].as_str().unwrap().contains("read-state"));
            }
        }
    }

    #[tokio::test]
    async fn legacy_put_keeps_four_fields_and_new_receipt_has_fixed_identity() {
        let (status, body, _) = request(router(Arc::new(FakeRepository::default()), InvocationKind::HumanInteractive), "PUT", PATH, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_object().unwrap().len(), 4);
        assert_eq!(body["inserted"], false);
        let id = Uuid::now_v7();
        let (status, body, _) = request(router(Arc::new(FakeRepository::default()), InvocationKind::HumanInteractive), "POST", &format!("{PATH}/view"), Some(json!({"operationId": id, "expectedReadStateRevision": 0}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["operationId"], id.to_string());
        assert_eq!(body["kind"], "VIEW");
        assert_eq!(body["resultingReadState"]["isRead"], true);
        assert_eq!(body.as_object().unwrap().len(), 8);
    }
}
