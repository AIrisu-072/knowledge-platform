use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::extract::rejection::JsonRejection;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use document_api_http::error::{ApiError, ApiProblem, ErrorCode};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::limits::{
    DIFF_OPERATION_TIMEOUT, DOWNLOAD_IDLE_TIMEOUT, MAX_JSON_BODY_BYTES, MAX_JSON_RESPONSE_BYTES,
    MAX_REQUEST_HEADER_BYTES, MULTIPART_OPERATION_TIMEOUT, ORDINARY_OPERATION_TIMEOUT,
};
use document_api_http::router::{protect_routes, protect_routes_with_observer};
use document_api_http::timeout::with_operation_timeout;
use document_api_http::trace::{HttpObservation, HttpObservationSink, TraceContext};
use document_application::{IdentityResolutionError, InvocationKind, VerifiedActorContext};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use serde_json::{Value, json};
use time::{Duration as TimeDuration, OffsetDateTime};
use tower::ServiceExt;

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

fn actor() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        PrincipalRef::new("test-idp", "reader").unwrap(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "reader").unwrap()],
        OffsetDateTime::now_utc() + TimeDuration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

#[derive(Default)]
struct RecordingObserver(Mutex<Vec<HttpObservation>>);

impl HttpObservationSink for RecordingObserver {
    fn record(&self, observation: HttpObservation) {
        self.0.lock().unwrap().push(observation);
    }
}

async fn forbidden(Extension(trace): Extension<TraceContext>) -> axum::response::Response {
    ApiProblem::new(ErrorCode::Forbidden, "/v1/probe/{value}", &trace.trace_id).into_response()
}

#[tokio::test]
async fn observations_use_route_templates_and_omit_request_secrets() {
    let observer = Arc::new(RecordingObserver::default());
    let router = protect_routes_with_observer(
        Router::new().route("/v1/probe/{value}", post(forbidden)),
        Some(Arc::new(FixedIdentity(actor()))),
        observer.clone(),
    )
    .unwrap();
    let trace_id = "0123456789abcdef0123456789abcdef";
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/probe/private-document?token=query-secret")
                .header("traceparent", format!("00-{trace_id}-0123456789abcdef-01"))
                .header(header::AUTHORIZATION, "Bearer credential-secret")
                .header(header::COOKIE, "session=cookie-secret")
                .body(Body::from("document-body-secret"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let observations = observer.0.lock().unwrap();
    assert_eq!(observations.len(), 1);
    let observation = &observations[0];
    assert_eq!(observation.trace_id, trace_id);
    assert_eq!(observation.route_template, "/v1/probe/{value}");
    assert_eq!(observation.method, "POST");
    assert_eq!(observation.status, 403);
    assert_eq!(observation.error_code, Some(ErrorCode::Forbidden));
    assert_eq!(observation.invocation_kind, Some("human_interactive"));
    let recorded = format!("{observation:?}");
    for secret in [
        "private-document",
        "query-secret",
        "credential-secret",
        "cookie-secret",
        "document-body-secret",
    ] {
        assert!(!recorded.contains(secret), "recorded secret: {secret}");
    }
}

struct DropSignal(Arc<AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn operation_timeout_is_finite_cancels_pending_work_and_preserves_known_commit_unknown() {
    let dropped = Arc::new(AtomicBool::new(false));
    let signal = dropped.clone();
    let slow = Router::new().route(
        "/slow",
        get(move || {
            let signal = signal.clone();
            async move {
                let _drop_signal = DropSignal(signal);
                std::future::pending::<()>().await;
                StatusCode::OK
            }
        }),
    );
    let router = protect_routes(
        with_operation_timeout(slow, Duration::from_millis(20)),
        Some(Arc::new(FixedIdentity(actor()))),
    )
    .unwrap();
    let response = router
        .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["code"], "TIMEOUT");
    assert!(dropped.load(Ordering::SeqCst));

    let known = Router::new().route(
        "/commit",
        post(|Extension(trace): Extension<TraceContext>| async move {
            ApiProblem::new(ErrorCode::CommitOutcomeUnknown, "/commit", &trace.trace_id)
                .into_response()
        }),
    );
    let router = protect_routes(
        with_operation_timeout(known, Duration::from_millis(100)),
        Some(Arc::new(FixedIdentity(actor()))),
    )
    .unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commit")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["code"], "COMMIT_OUTCOME_UNKNOWN");
}

async fn json_probe(
    Extension(trace): Extension<TraceContext>,
    payload: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    payload
        .map_err(|_| ApiProblem::new(ErrorCode::ValidationFailed, "/json", &trace.trace_id).into())
}

#[tokio::test]
async fn production_request_and_response_limits_accept_exact_and_reject_one_over() {
    assert_eq!(MAX_JSON_BODY_BYTES, 1024 * 1024);
    assert_eq!(MAX_REQUEST_HEADER_BYTES, 32 * 1024);
    assert_eq!(MAX_JSON_RESPONSE_BYTES, 32 * 1024 * 1024);

    let router = protect_routes(
        Router::new()
            .route("/json", post(json_probe))
            .route(
                "/response/{bytes}",
                get(
                    |axum::extract::Path(bytes): axum::extract::Path<usize>| async move {
                        axum::response::Response::builder()
                            .header(header::CONTENT_TYPE, "application/json")
                            .body(Body::from(vec![b' '; bytes]))
                            .unwrap()
                    },
                ),
            )
            .route("/headers", get(|| async { Json(json!({"ok": true})) })),
        Some(Arc::new(FixedIdentity(actor()))),
    )
    .unwrap();

    let exact_json = format!(
        "{{\"v\":\"{}\"}}",
        "x".repeat(MAX_JSON_BODY_BYTES - "{\"v\":\"\"}".len())
    );
    assert_eq!(exact_json.len(), MAX_JSON_BODY_BYTES);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/json")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(exact_json.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/json")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!("{exact_json} ")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let header_name = "x-padding";
    let exact_header_value = "x".repeat(MAX_REQUEST_HEADER_BYTES - header_name.len() - 4);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/headers")
                .header(header_name, &exact_header_value)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/headers")
                .header(header_name, format!("{exact_header_value}x"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/response/{MAX_JSON_RESPONSE_BYTES}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .oneshot(
            Request::builder()
                .uri(format!("/response/{}", MAX_JSON_RESPONSE_BYTES + 1))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["code"], "INTERNAL");
}

#[tokio::test]
async fn same_origin_default_has_no_cross_origin_credentials_and_keeps_security_headers() {
    let router = protect_routes(
        Router::new().route("/secure", get(|| async { "ok" })),
        Some(Arc::new(FixedIdentity(actor()))),
    )
    .unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .uri("/secure")
                .header(header::ORIGIN, "https://cross-origin.invalid")
                .header(header::COOKIE, "session=credential")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .is_none()
    );
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
}

#[test]
fn operation_timeout_profile_is_finite_and_keeps_worker_inside_api_budget() {
    assert_eq!(ORDINARY_OPERATION_TIMEOUT, Duration::from_secs(30));
    assert_eq!(MULTIPART_OPERATION_TIMEOUT, Duration::from_secs(120));
    assert_eq!(DIFF_OPERATION_TIMEOUT, Duration::from_secs(45));
    assert_eq!(DOWNLOAD_IDLE_TIMEOUT, Duration::from_secs(30));
    assert!(DIFF_OPERATION_TIMEOUT > document_diff_runner::MAX_WALL_TIMEOUT);
}
