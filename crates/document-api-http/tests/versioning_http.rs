#[path = "../../document-repository-postgres/tests/support/versioning.rs"]
mod support;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use document_api_http::error::ApiProblem;
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::versioning::versioning_router;
use document_application::{
    ApplicationError, BootstrapRootPolicy, IdentityResolutionError, InvocationKind,
    VerifiedActorContext, VersionOperationId,
};
use document_domain::{Action, PolicyGrant, PolicySubject, PolicySubjectKind};
use serde_json::{Value, json};
use support::{TestIds, actor, fixture, install_new_current};
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
struct FixedIdentity(VerifiedActorContext);

impl IdentityAdapter for FixedIdentity {
    fn resolve<'a>(
        &'a self,
        _request: &'a IdentityRequestContext,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedActorContext, IdentityResolutionError>> + Send + 'a>,
    > {
        Box::pin(async { Ok(self.0.clone()) })
    }
}

fn context() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        actor(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn full_grant() -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap(),
        [
            Action::Read,
            Action::ReadHistory,
            Action::Write,
            Action::Publish,
            Action::Administer,
        ],
    )
    .unwrap()
}

#[derive(Clone)]
struct Part {
    name: &'static str,
    part_id: Option<String>,
    content_type: &'static str,
    body: Vec<u8>,
}

fn request_part(value: Value) -> Part {
    Part {
        name: "request",
        part_id: None,
        content_type: "application/json",
        body: serde_json::to_vec(&value).unwrap(),
    }
}

fn file_part(part_id: &str, byte: u8) -> Part {
    Part {
        name: "files",
        part_id: Some(part_id.to_owned()),
        content_type: "application/octet-stream",
        body: vec![byte; 3],
    }
}

fn multipart(parts: &[Part], terminated: bool) -> (String, Vec<u8>) {
    let boundary = "versioning-boundary";
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let filename = if part.name == "files" {
            "; filename=\"transport-name.bin\""
        } else {
            ""
        };
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"{filename}\r\n",
                part.name
            )
            .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {}\r\n", part.content_type).as_bytes());
        if let Some(part_id) = &part.part_id {
            body.extend_from_slice(format!("X-Part-Id: {part_id}\r\n").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(&part.body);
        body.extend_from_slice(b"\r\n");
    }
    if terminated {
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    }
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn send_multipart(
    router: Router,
    method: Method,
    uri: &str,
    parts: &[Part],
    terminated: bool,
) -> (StatusCode, Value) {
    let (content_type, body) = multipart(parts, terminated);
    let response = router
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    response_json(response).await
}

async fn send_json(router: Router, uri: &str, value: &Value) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(value).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    response_json(response).await
}

async fn response_json(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "expected JSON response: {error}; body={}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, value)
}

fn operation_id(value: u8) -> Uuid {
    Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8000-{value:012x}")).unwrap()
}

fn version_request(
    operation_id: Uuid,
    target_version_id: Uuid,
    expected_revision: i64,
    file_id: Uuid,
    part_id: &str,
    title: &str,
) -> Value {
    json!({
        "operationId": operation_id,
        "targetVersionId": target_version_id,
        "expectedRevision": expected_revision,
        "title": title,
        "items": [{
            "logicalPath": "primary",
            "ordinal": 0,
            "fileId": file_id,
            "partId": part_id,
            "mediaType": "text/plain",
            "originalFilename": "authoritative.txt",
            "renditions": []
        }]
    })
}

#[tokio::test]
async fn create_update_replay_and_rebase_preserve_manifest_binding() {
    let f = fixture().await;
    let bootstrap =
        document_repository_postgres::PostgresDocumentRepository::new_with_bootstrap_actor(
            f.pool.clone(),
            actor(),
        );
    bootstrap
        .initialize_root_policy(&context(), vec![full_grant()])
        .await
        .unwrap();
    let api = versioning_router(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        f.executor.clone(),
        f.repository.clone(),
        Arc::new(FixedIdentity(context())),
    )
    .unwrap();

    let target_id = Uuid::now_v7();
    let create_operation = operation_id(111);
    let create_file = Uuid::now_v7();
    let create_request = version_request(
        create_operation,
        target_id,
        1,
        create_file,
        "authoritative",
        "Replacement",
    );
    let create_parts = [
        file_part("authoritative", 2),
        request_part(create_request.clone()),
    ];
    let create_uri = format!("/v1/documents/{}/versions", f.document_id.as_uuid());
    let (status, created) =
        send_multipart(api.clone(), Method::POST, &create_uri, &create_parts, true).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["targetVersionId"], target_id.to_string());
    assert_eq!(created["resultingRevision"], 2);

    let (status, replayed) =
        send_multipart(api.clone(), Method::POST, &create_uri, &create_parts, true).await;
    assert_eq!(status, StatusCode::CREATED, "{replayed}");
    assert_eq!(replayed, created);

    let changed_request = version_request(
        create_operation,
        target_id,
        1,
        Uuid::now_v7(),
        "changed",
        "Different payload",
    );
    let (status, problem) = send_multipart(
        api.clone(),
        Method::POST,
        &create_uri,
        &[request_part(changed_request), file_part("changed", 3)],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");

    let update_operation = operation_id(112);
    let update_file = Uuid::now_v7();
    let update_request = version_request(
        update_operation,
        target_id,
        2,
        update_file,
        "update",
        "Replacement revised",
    );
    let update_uri = format!(
        "/v1/documents/{}/versions/{target_id}",
        f.document_id.as_uuid()
    );
    let (status, updated) = send_multipart(
        api.clone(),
        Method::PUT,
        &update_uri,
        &[request_part(update_request), file_part("update", 4)],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["resultingRevision"], 3);

    let new_current = install_new_current(&f, "New current", 5).await;
    let rebase_uri = format!(
        "/v1/documents/{}/versions/{target_id}:rebase",
        f.document_id.as_uuid()
    );
    let (status, rebased) = send_json(
        api,
        &rebase_uri,
        &json!({"operationId": operation_id(113), "expectedRevision": 4}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rebased}");
    assert_eq!(rebased["baseVersionId"], new_current.as_uuid().to_string());
    assert_eq!(rebased["resultingRevision"], 5);
}

#[tokio::test]
async fn malformed_binding_path_and_disconnect_fail_before_version_commit() {
    let f = fixture().await;
    let bootstrap =
        document_repository_postgres::PostgresDocumentRepository::new_with_bootstrap_actor(
            f.pool.clone(),
            actor(),
        );
    bootstrap
        .initialize_root_policy(&context(), vec![full_grant()])
        .await
        .unwrap();
    let api = versioning_router(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        f.executor.clone(),
        f.repository.clone(),
        Arc::new(FixedIdentity(context())),
    )
    .unwrap();
    let document_id = f.document_id.as_uuid();
    let target_id = Uuid::now_v7();
    let request = version_request(
        operation_id(121),
        target_id,
        1,
        Uuid::now_v7(),
        "primary",
        "Replacement",
    );
    let create_uri = format!("/v1/documents/{document_id}/versions");
    let cases = [
        ("missing binary", vec![request_part(request.clone())]),
        (
            "duplicate binary",
            vec![
                request_part(request.clone()),
                file_part("primary", 2),
                file_part("primary", 2),
            ],
        ),
        (
            "unknown binary",
            vec![
                request_part(request.clone()),
                file_part("primary", 2),
                file_part("unknown", 2),
            ],
        ),
    ];
    for (name, parts) in cases {
        let (status, problem) =
            send_multipart(api.clone(), Method::POST, &create_uri, &parts, true).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{name}: {problem}"
        );
        assert_eq!(problem["code"], "VALIDATION_FAILED", "{name}");
    }

    let (status, problem) = send_multipart(
        api.clone(),
        Method::POST,
        &create_uri,
        &[request_part(request.clone()), file_part("primary", 2)],
        false,
    )
    .await;
    assert!(status.is_client_error(), "{problem}");

    let mut invalid_media = version_request(
        operation_id(122),
        Uuid::now_v7(),
        1,
        Uuid::now_v7(),
        "invalid-media",
        "Invalid media",
    );
    invalid_media["items"][0]["mediaType"] = json!("not-a-media-type");
    let (status, problem) = send_multipart(
        api.clone(),
        Method::POST,
        &create_uri,
        &[request_part(invalid_media), file_part("invalid-media", 2)],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{problem}");

    let mut parser_disagreement = version_request(
        operation_id(123),
        Uuid::now_v7(),
        1,
        Uuid::now_v7(),
        "parser-disagreement",
        "Parser disagreement",
    );
    parser_disagreement["items"][0]["mediaType"] = json!("application/pdf");
    let (status, problem) = send_multipart(
        api.clone(),
        Method::POST,
        &create_uri,
        &[
            request_part(parser_disagreement),
            file_part("parser-disagreement", 2),
        ],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{problem}");
    assert_eq!(problem["code"], "INTEGRITY_VIOLATION");

    let mismatched_path = format!("/v1/documents/{document_id}/versions/{}", Uuid::now_v7());
    let (status, problem) = send_multipart(
        api,
        Method::PUT,
        &mismatched_path,
        &[request_part(request), file_part("primary", 2)],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");

    let version_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_versions WHERE document_id = $1")
            .bind(document_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(version_count, 1);
}

#[test]
fn version_commit_unknown_is_exact_retry_only() {
    let operation_id = VersionOperationId::try_from_uuid(operation_id(131)).unwrap();
    let problem = ApiProblem::from_application(
        ApplicationError::VersionCommitOutcomeUnknown {
            operation_id,
            document_id: document_domain::DocumentId::from_uuid(Uuid::now_v7()),
            document_version_id: document_domain::DocumentVersionId::from_uuid(Uuid::now_v7()),
        },
        "/v1/documents/id/versions/id",
        "trace",
    );
    assert_eq!(problem.status, 503);
    assert!(problem.retryable);
    assert_eq!(problem.exact_retry, Some(true));
    assert!(problem.recovery.is_none());
}
