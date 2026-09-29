use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use document_api_http::error::{ApiProblem, ErrorCode};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::router::{StartupError, protect_routes};
use document_api_http::validation::SchemaRegistry;
use document_application::{
    ApplicationError, IdentityResolutionError, InvocationKind, VerifiedActorContext,
};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;

#[derive(Clone)]
struct FixtureIdentity {
    outcome: Result<VerifiedActorContext, IdentityResolutionError>,
}

impl IdentityAdapter for FixtureIdentity {
    fn resolve<'a>(
        &'a self,
        _request: &'a IdentityRequestContext,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedActorContext, IdentityResolutionError>> + Send + 'a>,
    > {
        Box::pin(async { self.outcome.clone() })
    }
}

fn actor() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        PrincipalRef::new("test-idp", "user-1").unwrap(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "user-1").unwrap()],
        OffsetDateTime::now_utc() + Duration::minutes(5),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn fixture_router(outcome: Result<VerifiedActorContext, IdentityResolutionError>) -> Router {
    protect_routes(
        Router::new().route("/v1/probe", get(|| async { "ok" })),
        Some(Arc::new(FixtureIdentity { outcome })),
    )
    .unwrap()
}

async fn request_json(router: Router, path: &str) -> (StatusCode, Value, axum::http::HeaderMap) {
    let response = router
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body, headers)
}

#[tokio::test]
async fn identity_is_required_and_adapter_failures_are_closed() {
    assert!(matches!(
        protect_routes(Router::new(), None),
        Err(StartupError::IdentityAdapterMissing)
    ));
    let (status, body, headers) = request_json(
        fixture_router(Err(IdentityResolutionError::InvalidIdentity)),
        "/v1/probe",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "AUTHENTICATION_REQUIRED");
    assert_eq!(body["status"], 401);
    assert_eq!(headers[header::CONTENT_TYPE], "application/problem+json");
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
    assert!(headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());

    let (status, body, _) = request_json(
        fixture_router(Err(IdentityResolutionError::Unavailable)),
        "/v1/probe",
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "IDENTITY_UNAVAILABLE");
}

#[tokio::test]
async fn verified_actor_and_trace_are_attached_without_trusting_payload() {
    let response = fixture_router(Ok(actor()))
        .oneshot(
            Request::builder()
                .uri("/v1/probe")
                .header(
                    "traceparent",
                    "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["trace-id"],
        "0123456789abcdef0123456789abcdef"
    );
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
}

#[test]
fn application_errors_map_to_stable_sanitized_problem() {
    let forbidden = ApiProblem::from_application(
        ApplicationError::Forbidden,
        "/v1/documents/hidden",
        "0123456789abcdef0123456789abcdef",
    );
    assert_eq!(forbidden.status, 403);
    assert_eq!(forbidden.code, ErrorCode::Forbidden);
    let internal = ApiProblem::from_application(
        ApplicationError::Internal("secret-sql-storage-locator".into()),
        "/v1/documents",
        "0123456789abcdef0123456789abcdef",
    );
    let json = serde_json::to_string(&internal).unwrap();
    assert_eq!(internal.status, 500);
    assert_eq!(internal.code, ErrorCode::Internal);
    assert!(!json.contains("secret-sql-storage-locator"));
    assert_eq!(
        ApiProblem::from_application(ApplicationError::Conflict, "/", "t").status,
        409
    );
    assert_eq!(
        ApiProblem::from_application(ApplicationError::Validation("bad".into()), "/", "t").status,
        422
    );
}

#[test]
fn schema_compiles_at_startup_and_rejects_actor_claims() {
    let registry = SchemaRegistry::compile([(
        "mutation",
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"reason": {"type": "string"}},
            "required": ["reason"],
            "additionalProperties": false
        }),
    )])
    .unwrap();
    assert!(
        registry
            .validate_request("mutation", &json!({"reason": "review"}))
            .is_ok()
    );
    assert!(
        registry
            .validate_request("mutation", &json!({"reason": 4}))
            .is_err()
    );
    assert!(
        registry
            .validate_request(
                "mutation",
                &json!({"reason": "x", "principalId": "attacker"})
            )
            .is_err()
    );
    assert!(SchemaRegistry::compile([("broken", json!({"type": "unknown"}))]).is_err());
}
