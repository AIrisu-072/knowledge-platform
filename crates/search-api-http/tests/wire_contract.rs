//! P5-06: identity, Problem, DTO and the four handlers on the wire.

#[path = "../../search-application/tests/support/api.rs"]
mod api;
#[path = "support/backend.rs"]
mod backend;
#[path = "../../search-application/tests/support/search_corpus.rs"]
mod corpus;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use backend::{Backend, Tokens};
use search_api_http::auth::{
    AuthConfigurationError, SearchAuthChallengePort, SearchAuthSchemeBinding, StaticBearerChallenge,
};
use search_api_http::problem::ProblemCode;
use search_api_http::router::{
    SearchApiBackend, SearchOperation, SearchRouterConfig, StartupError, build_search_router,
};
use search_application::scoped::AccessContextHandle;
use serde_json::{Value, json};
use tower::ServiceExt;

struct Harness {
    backend: &'static Backend,
    tokens: Arc<Tokens>,
    router: Router,
}

async fn harness(timeout: Duration) -> Harness {
    let backend = Backend::new().await;
    let tokens = Arc::new(Tokens::default());
    let reader = backend.handle("reader").await;
    tokens
        .0
        .lock()
        .unwrap()
        .insert("reader-token".into(), reader);
    let backend_object: Arc<dyn SearchApiBackend> = Arc::new(backend);
    let router = build_search_router(SearchRouterConfig {
        backend: backend_object,
        credentials: Some(tokens.clone()),
        auth: Some(SearchAuthSchemeBinding::bearer(Arc::new(
            StaticBearerChallenge,
        ))),
        operation_timeout: timeout,
    })
    .unwrap();
    Harness {
        backend,
        tokens,
        router,
    }
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

async fn send(router: &Router, request: Request<Body>) -> Reply {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

fn post(path: &str, token: Option<&str>, body: impl Into<Body>) -> Request<Body> {
    let mut builder = Request::post(path).header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(body.into()).unwrap()
}

fn get(path: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

fn search_body() -> String {
    json!({"query": "規程", "coverage": "titleAndPermittedMetadata"}).to_string()
}

fn discover_body() -> String {
    json!({
        "need": {
            "purpose": "find a rule",
            "requiredResourceTypes": ["knowledge"],
            "requiredClaimIds": ["00000000-0000-0000-0000-000000000051"]
        },
        "coverage": "titleAndPermittedMetadata"
    })
    .to_string()
}

fn resource_path() -> String {
    format!("/v1/resources/{}", corpus::rid(11).as_uuid())
}

fn assert_problem(reply: &Reply, code: ProblemCode) {
    let (status, code_text, title, detail) = code.registry();
    assert_eq!(reply.status.as_u16(), status, "{code_text}");
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "application/problem+json"
    );
    assert_eq!(reply.headers[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(reply.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(reply.body["type"], "about:blank");
    assert_eq!(reply.body["status"], status);
    assert_eq!(reply.body["code"], code_text);
    assert_eq!(reply.body["title"], title);
    assert_eq!(reply.body["detail"], detail);
    assert!(reply.body.get("instance").is_none());
    let trace = reply.body["trace_id"].as_str().unwrap();
    assert_eq!(uuid::Uuid::parse_str(trace).unwrap().get_version_num(), 4);
    assert_eq!(
        reply.headers.get(header::WWW_AUTHENTICATE).is_some(),
        code == ProblemCode::AuthenticationRequired
    );
}

#[tokio::test]
async fn all_four_routes_reject_self_principal_and_share_one_verified_scope() {
    let harness = harness(Duration::from_secs(5)).await;
    let spoof = |mut request: Request<Body>| {
        request
            .headers_mut()
            .insert("x-search-principal", "admin".parse().unwrap());
        request
    };
    for request in [
        post("/v1/search", Some("reader-token"), search_body()),
        post("/v1/discover", Some("reader-token"), discover_body()),
        get(&resource_path(), Some("reader-token")),
        get("/v1/sources", Some("reader-token")),
    ] {
        let reply = send(&harness.router, spoof(request)).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        assert_eq!(reply.headers[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(reply.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    }
    // Every route resolved the one verified actor, never the header.
    let seen = harness.backend.seen.lock().unwrap().clone();
    assert_eq!(
        seen.iter()
            .map(|(operation, _)| *operation)
            .collect::<Vec<_>>(),
        SearchOperation::ALL.to_vec()
    );
    assert!(seen.iter().all(|(_, principal)| principal == "reader"));
    // A self-asserted principal or tenant in the body is an unknown field.
    for body in [
        json!({"query": "規程", "coverage": "titleAndPermittedMetadata", "principal": "admin"}),
        json!({"query": "規程", "coverage": "titleAndPermittedMetadata", "tenant": "tenant-b"}),
    ] {
        let reply = send(
            &harness.router,
            post("/v1/search", Some("reader-token"), body.to_string()),
        )
        .await;
        assert_problem(&reply, ProblemCode::ValidationFailed);
    }
}

#[tokio::test]
async fn missing_invalid_revoked_token_401_with_scheme_challenge() {
    let harness = harness(Duration::from_secs(5)).await;
    let revoked: AccessContextHandle = harness.backend.handle("leaver").await;
    harness
        .tokens
        .0
        .lock()
        .unwrap()
        .insert("leaver-token".into(), revoked.clone());
    harness.backend.world.authority.revoke(&revoked).unwrap();
    let basic = Request::get("/v1/sources")
        .header(header::AUTHORIZATION, "Basic cmVhZGVy")
        .body(Body::empty())
        .unwrap();
    for request in [
        get("/v1/sources", None),
        get("/v1/sources", Some("unknown-token")),
        basic,
        get("/v1/sources", Some("leaver-token")),
        post("/v1/search", Some("leaver-token"), search_body()),
    ] {
        let reply = send(&harness.router, request).await;
        assert_problem(&reply, ProblemCode::AuthenticationRequired);
        assert_eq!(
            reply.headers[header::WWW_AUTHENTICATE],
            "Bearer realm=\"search\""
        );
    }
}

#[tokio::test]
async fn authenticated_operation_denied_403() {
    let harness = harness(Duration::from_secs(5)).await;
    harness
        .backend
        .denied
        .lock()
        .unwrap()
        .insert(("reader".into(), "discover"));
    let reply = send(
        &harness.router,
        post("/v1/discover", Some("reader-token"), discover_body()),
    )
    .await;
    assert_problem(&reply, ProblemCode::Forbidden);
    // Other operations of the same actor are unaffected.
    let reply = send(&harness.router, get("/v1/sources", Some("reader-token"))).await;
    assert_eq!(reply.status, StatusCode::OK);
}

#[tokio::test]
async fn resolver_outage_identity_503() {
    let harness = harness(Duration::from_secs(5)).await;
    let reply = send(&harness.router, get("/v1/sources", Some("outage"))).await;
    assert_problem(&reply, ProblemCode::IdentityUnavailable);
}

struct Challenges(Vec<String>);
impl SearchAuthChallengePort for Challenges {
    fn challenges(&self, _: SearchOperation) -> Vec<String> {
        self.0.clone()
    }
}

#[tokio::test]
async fn invalid_empty_unwired_challenge_rejects_startup() {
    let backend = Backend::new().await;
    let config = |credentials: bool, auth: Option<SearchAuthSchemeBinding>| SearchRouterConfig {
        backend: Arc::new(backend),
        credentials: credentials.then(|| Arc::new(Tokens::default()) as Arc<_>),
        auth,
        operation_timeout: Duration::from_secs(5),
    };
    let binding = |values: &[&str]| {
        Some(SearchAuthSchemeBinding::bearer(Arc::new(Challenges(
            values.iter().map(|value| (*value).to_owned()).collect(),
        ))))
    };
    let error = |config| build_search_router(config).err().unwrap();
    assert_eq!(
        error(config(false, binding(&["Bearer realm=\"search\""]))),
        StartupError::CredentialVerifierUnwired
    );
    assert_eq!(error(config(true, None)), StartupError::ChallengeUnwired);
    assert_eq!(
        error(config(true, binding(&[]))),
        StartupError::Challenge(AuthConfigurationError::EmptyChallenge)
    );
    for invalid in [
        "Bearer realm=\"other\"",
        "Bearer realm=\"search\"\r\nX-Injected: 1",
        "Bearer realm=\"search\", error=\"tenant-a\"",
        "",
    ] {
        assert_eq!(
            error(config(true, binding(&[invalid]))),
            StartupError::Challenge(AuthConfigurationError::InvalidChallenge),
            "{invalid:?}"
        );
    }
    let mut wrong_scheme = SearchAuthSchemeBinding::bearer(Arc::new(StaticBearerChallenge));
    wrong_scheme.security_scheme = "basic".into();
    assert_eq!(
        error(config(true, Some(wrong_scheme))),
        StartupError::Challenge(AuthConfigurationError::InvalidScheme)
    );
    assert!(build_search_router(config(true, binding(&["Bearer realm=\"search\""]))).is_ok());
}

/// `(status, title, detail)` per code, read from the registry file itself.
fn registry() -> BTreeMap<String, (u16, String, String)> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/errors/search-api-error-registry.yaml"
    ))
    .unwrap();
    let mut codes = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut entry = (0u16, String::new(), String::new());
    let mut in_codes = false;
    for line in text.lines() {
        if line == "codes:" {
            in_codes = true;
            continue;
        }
        if !in_codes {
            continue;
        }
        if let Some(name) = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' '))
        {
            if let Some(code) = current.take() {
                codes.insert(code, entry.clone());
            }
            current = Some(name.trim_end_matches(':').to_owned());
            entry = (0, String::new(), String::new());
        } else if let Some(value) = line.trim().strip_prefix("status: ") {
            entry.0 = value.parse().unwrap();
        } else if let Some(value) = line.trim().strip_prefix("title: ") {
            entry.1 = value.to_owned();
        } else if let Some(value) = line.trim().strip_prefix("detail: ") {
            entry.2 = value.to_owned();
        } else if !line.starts_with(' ') && !line.is_empty() {
            break;
        }
    }
    if let Some(code) = current {
        codes.insert(code, entry);
    }
    codes
}

#[tokio::test]
async fn all_problem_status_headers_schema_and_fixed_detail_match_registry() {
    let registry = registry();
    assert_eq!(registry.len(), ProblemCode::ALL.len());
    for code in ProblemCode::ALL {
        let (status, name, title, detail) = code.registry();
        assert_eq!(
            registry[name],
            (status, title.to_owned(), detail.to_owned()),
            "{name}"
        );
        let challenge =
            search_api_http::auth::ValidatedChallenge::parse("Bearer realm=\"search\"").unwrap();
        let response =
            search_api_http::problem::problem(code, uuid::Uuid::new_v4(), Some(&challenge), &[]);
        let status_code = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        assert_problem(
            &Reply {
                status: status_code,
                headers,
                body: serde_json::from_slice(&bytes).unwrap(),
            },
            code,
        );
    }
}

#[tokio::test]
async fn internal_deadline_503_only_real_upstream_wait_504() {
    let harness = harness(Duration::from_millis(300)).await;
    harness.backend.stall.store(true, Ordering::SeqCst);
    let reply = send(&harness.router, get("/v1/sources", Some("reader-token"))).await;
    assert_problem(&reply, ProblemCode::ServiceUnavailable);
    assert_eq!(
        reply.body["detail"],
        "The service could not complete the operation."
    );
    harness.backend.stall.store(false, Ordering::SeqCst);
    harness
        .backend
        .upstream_timeout
        .store(true, Ordering::SeqCst);
    let reply = send(&harness.router, get("/v1/sources", Some("reader-token"))).await;
    assert_problem(&reply, ProblemCode::UpstreamTimeout);
}

#[tokio::test]
async fn unknown_json_field_422_malformed_json_400() {
    let harness = harness(Duration::from_secs(5)).await;
    let router = &harness.router;
    let unknown = json!({"query": "規程", "coverage": "titleAndPermittedMetadata", "routing": {}});
    assert_problem(
        &send(
            router,
            post("/v1/search", Some("reader-token"), unknown.to_string()),
        )
        .await,
        ProblemCode::ValidationFailed,
    );
    assert_problem(
        &send(
            router,
            post("/v1/search", Some("reader-token"), "{\"query\":"),
        )
        .await,
        ProblemCode::MalformedRequest,
    );
    // Out-of-range fields carry public pointers only.
    let reply = send(
        router,
        post(
            "/v1/search",
            Some("reader-token"),
            json!({"query": "規程", "coverage": "titleAndPermittedMetadata", "pageSize": 101})
                .to_string(),
        ),
    )
    .await;
    assert_problem(&reply, ProblemCode::ValidationFailed);
    assert_eq!(reply.body["errors"][0]["pointer"], "/pageSize");
    // Media type, body and header bounds.
    let text = Request::post("/v1/search")
        .header(header::CONTENT_TYPE, "text/plain")
        .header(header::AUTHORIZATION, "Bearer reader-token")
        .body(Body::from(search_body()))
        .unwrap();
    assert_problem(&send(router, text).await, ProblemCode::UnsupportedMediaType);
    let large = format!(
        "{{\"query\":\"{}\",\"coverage\":\"titleAndPermittedMetadata\"}}",
        "x".repeat(17 * 1024)
    );
    assert_problem(
        &send(router, post("/v1/search", Some("reader-token"), large)).await,
        ProblemCode::PayloadTooLarge,
    );
    let mut headers = get("/v1/sources", Some("reader-token"));
    headers
        .headers_mut()
        .insert("x-padding", "p".repeat(17 * 1024).parse().unwrap());
    assert_problem(
        &send(router, headers).await,
        ProblemCode::RequestHeadersTooLarge,
    );
    assert_problem(
        &send(
            router,
            get("/v1/resources/NOT-A-UUID", Some("reader-token")),
        )
        .await,
        ProblemCode::ValidationFailed,
    );
    assert_problem(
        &send(
            router,
            get("/v1/sources?tenant=tenant-b", Some("reader-token")),
        )
        .await,
        ProblemCode::ValidationFailed,
    );
}
