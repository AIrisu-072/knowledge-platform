//! The four Search routes over one backend.
//!
//! Order per request: header bound → media type and body bound → Bearer
//! credential → verified actor → operation authorization → closed JSON
//! parse → complete visible catalog → core service → final disclosure gate,
//! inside which the bounded JSON body is produced exactly once. The whole
//! pipeline runs under the internal operation deadline (503); only a real
//! upstream wait reported by the backend is a 504.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Path, Request, State};
use axum::http::request::Parts;
use axum::http::{Method, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use search_application::SearchError;
use search_application::api_cursor::CursorHandle;
use search_application::api_scope::{ApiError, SearchOperationContext};
use search_application::discover_route::DiscoverInput;
use search_application::remote_disclosure::{CurrentDisclosureAccessPort, TransientDisclosure};
use search_application::resource_read::ResourceView;
use search_application::scoped::{AccessContextHandle, VisibleCatalogSnapshot};
use search_application::search_query::{SearchInput, SearchResultView};
use search_application::source_browse::SourceView;
use search_core::discovery::DiscoveryResult;
use search_core::id::ResourceId;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::auth::{
    AuthConfigurationError, SearchAuthSchemeBinding, SearchCredentialVerifierPort,
    ValidatedChallenge, bearer_token,
};
use crate::dto::{self, DiscoveryInputDto, SearchQueryDto};
use crate::limits::{
    DEFAULT_PAGE_SIZE, POST_JSON_BODY_BYTES, REQUEST_HEADER_BYTES, SUCCESS_BODY_BYTES,
    header_bytes, is_json,
};
use crate::problem::{FieldError, NO_SNIFF, PRIVATE_NO_STORE, ProblemCode, problem};
use crate::send::{ConnectionLeases, leased_response};

pub type ApiFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ApiError>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOperation {
    Search,
    Discover,
    Resource,
    Sources,
}

impl SearchOperation {
    pub const ALL: [Self; 4] = [Self::Search, Self::Discover, Self::Resource, Self::Sources];
}

/// The core services behind the routes, wired by the host runtime.
pub trait SearchApiBackend: Send + Sync + 'static {
    fn authenticate<'a>(
        &'a self,
        handle: &'a AccessContextHandle,
        deadline: Instant,
    ) -> ApiFuture<'a, SearchOperationContext>;

    /// Whether the authenticated actor may use this operation at all.
    fn authorize<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        operation: SearchOperation,
    ) -> ApiFuture<'a, ()>;

    fn visible<'a>(
        &'a self,
        context: &'a SearchOperationContext,
    ) -> ApiFuture<'a, VisibleCatalogSnapshot>;

    fn search<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        input: SearchInput,
    ) -> ApiFuture<'a, TransientDisclosure<SearchResultView>>;

    fn discover<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        input: DiscoverInput,
    ) -> ApiFuture<'a, TransientDisclosure<DiscoveryResult>>;

    fn resource<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        resource_id: ResourceId,
    ) -> ApiFuture<'a, TransientDisclosure<ResourceView>>;

    fn sources<'a>(
        &'a self,
        context: &'a SearchOperationContext,
        snapshot: &'a VisibleCatalogSnapshot,
        page_size: usize,
        cursor: Option<CursorHandle>,
    ) -> ApiFuture<'a, TransientDisclosure<SourceView>>;

    /// The final actor/Source/item/field gate.
    fn gate(&self) -> &dyn CurrentDisclosureAccessPort;
}

pub struct SearchRouterConfig {
    pub backend: Arc<dyn SearchApiBackend>,
    pub credentials: Option<Arc<dyn SearchCredentialVerifierPort>>,
    pub auth: Option<SearchAuthSchemeBinding>,
    pub operation_timeout: Duration,
}

impl fmt::Debug for SearchRouterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SearchRouterConfig(<wired>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupError {
    CredentialVerifierUnwired,
    ChallengeUnwired,
    Challenge(AuthConfigurationError),
    InvalidTimeout,
}

struct AppState {
    backend: Arc<dyn SearchApiBackend>,
    credentials: Arc<dyn SearchCredentialVerifierPort>,
    challenge: ValidatedChallenge,
    timeout: Duration,
}

/// Refuses to start without a verifier, a valid challenge and a deadline.
pub fn build_search_router(config: SearchRouterConfig) -> Result<Router, StartupError> {
    let credentials = config
        .credentials
        .ok_or(StartupError::CredentialVerifierUnwired)?;
    let challenge = config
        .auth
        .ok_or(StartupError::ChallengeUnwired)?
        .validate()
        .map_err(StartupError::Challenge)?;
    if config.operation_timeout.is_zero() {
        return Err(StartupError::InvalidTimeout);
    }
    let state = Arc::new(AppState {
        backend: config.backend,
        credentials,
        challenge,
        timeout: config.operation_timeout,
    });
    Ok(Router::new()
        .route("/v1/search", post(search))
        .route("/v1/discover", post(discover))
        .route("/v1/resources/{resourceId}", get(resource))
        .route("/v1/sources", get(sources))
        .with_state(state))
}

async fn search(State(state): State<Arc<AppState>>, request: Request) -> Response {
    route(state, SearchOperation::Search, request, None).await
}

async fn discover(State(state): State<Arc<AppState>>, request: Request) -> Response {
    route(state, SearchOperation::Discover, request, None).await
}

async fn resource(
    State(state): State<Arc<AppState>>,
    Path(resource_id): Path<String>,
    request: Request,
) -> Response {
    route(state, SearchOperation::Resource, request, Some(resource_id)).await
}

async fn sources(State(state): State<Arc<AppState>>, request: Request) -> Response {
    route(state, SearchOperation::Sources, request, None).await
}

/// A request failure: one registry code and optional public field errors.
struct Failure(ProblemCode, Vec<FieldError>);

impl From<ApiError> for Failure {
    fn from(error: ApiError) -> Self {
        Self(error.into(), vec![])
    }
}

impl From<ProblemCode> for Failure {
    fn from(code: ProblemCode) -> Self {
        Self(code, vec![])
    }
}

async fn route(
    state: Arc<AppState>,
    operation: SearchOperation,
    request: Request,
    resource_id: Option<String>,
) -> Response {
    let trace = Uuid::new_v4();
    let (parts, body) = request.into_parts();
    let outcome = match bounded_body(&parts, body).await {
        Err(failure) => Err(failure),
        Ok(bytes) => {
            match tokio::time::timeout(
                state.timeout,
                pipeline(&state, operation, &parts, bytes, resource_id, trace),
            )
            .await
            {
                Ok(outcome) => outcome,
                // The internal operation deadline is never an upstream 504.
                Err(_) => Err(ProblemCode::ServiceUnavailable.into()),
            }
        }
    };
    match outcome {
        // On the socket server the body owns a lease until the send completes.
        Ok((bytes, evaluation_closed)) => match parts.extensions.get::<ConnectionLeases>() {
            Some(leases) => leased_response(bytes, leases.open(evaluation_closed)),
            None => success(bytes),
        },
        Err(Failure(code, errors)) => problem(code, trace, Some(&state.challenge), &errors),
    }
}

async fn bounded_body(parts: &Parts, body: Body) -> Result<Bytes, Failure> {
    if header_bytes(&parts.headers) > REQUEST_HEADER_BYTES {
        return Err(ProblemCode::RequestHeadersTooLarge.into());
    }
    if parts.method != Method::POST {
        return Ok(Bytes::new());
    }
    if !is_json(&parts.headers) {
        return Err(ProblemCode::UnsupportedMediaType.into());
    }
    to_bytes(body, POST_JSON_BODY_BYTES)
        .await
        .map_err(|_| ProblemCode::PayloadTooLarge.into())
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Failure> {
    serde_json::from_slice(bytes).map_err(|error| match error.classify() {
        serde_json::error::Category::Data => ProblemCode::ValidationFailed.into(),
        _ => ProblemCode::MalformedRequest.into(),
    })
}

fn validation(errors: Vec<FieldError>) -> Failure {
    Failure(ProblemCode::ValidationFailed, errors)
}

/// Canonical lowercase hyphenated UUID only.
fn canonical_uuid(value: &str) -> Option<Uuid> {
    let uuid = Uuid::parse_str(value).ok()?;
    (uuid.hyphenated().to_string() == value).then_some(uuid)
}

fn source_query(parts: &Parts) -> Result<(usize, Option<CursorHandle>), Failure> {
    let mut page_size = DEFAULT_PAGE_SIZE;
    let mut cursor = None;
    let mut seen_page_size = false;
    let mut seen_cursor = false;
    for pair in parts.uri.query().unwrap_or_default().split('&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        match name {
            "pageSize" if !seen_page_size => {
                seen_page_size = true;
                page_size = value
                    .parse()
                    .ok()
                    .filter(|size| (1..=100).contains(size))
                    .ok_or_else(|| {
                        validation(vec![FieldError {
                            pointer: "/pageSize",
                            code: "OUT_OF_RANGE",
                        }])
                    })?;
            }
            "cursor" if !seen_cursor => {
                seen_cursor = true;
                cursor = Some(CursorHandle::parse(value).ok_or_else(|| {
                    validation(vec![FieldError {
                        pointer: "/cursor",
                        code: "INVALID_FORMAT",
                    }])
                })?);
            }
            _ => return Err(ProblemCode::ValidationFailed.into()),
        }
    }
    Ok((page_size, cursor))
}

fn gated(error: SearchError) -> Failure {
    // A final-gate refusal or an unserializable view never sends a partial body.
    let _ = error;
    ProblemCode::ServiceUnavailable.into()
}

async fn pipeline(
    state: &AppState,
    operation: SearchOperation,
    parts: &Parts,
    body: Bytes,
    resource_id: Option<String>,
    trace: Uuid,
) -> Result<(Vec<u8>, bool), Failure> {
    let token = bearer_token(&parts.headers).ok_or(ProblemCode::AuthenticationRequired)?;
    let handle = state
        .credentials
        .verify(token)
        .await
        .map_err(|_| ProblemCode::IdentityUnavailable)?
        .ok_or(ProblemCode::AuthenticationRequired)?;
    let deadline = Instant::now() + state.timeout;
    let backend = &state.backend;
    let context = backend.authenticate(&handle, deadline).await?;
    backend.authorize(&context, operation).await?;
    let gate = backend.gate();
    let mut bytes: Option<Vec<u8>> = None;
    let evaluation_closed;
    let encode = |slot: &mut Option<Vec<u8>>, encoded: serde_json::Result<Vec<u8>>| {
        *slot = Some(encoded.map_err(|_| SearchError::OperationFailed("encode".into()))?);
        Ok(())
    };
    match operation {
        SearchOperation::Search => {
            let input = parse::<SearchQueryDto>(&body)?
                .into_input()
                .map_err(validation)?;
            let snapshot = backend.visible(&context).await?;
            let mut disclosure = backend.search(&context, &snapshot, input).await?;
            evaluation_closed = disclosure.evaluation_closed();
            disclosure
                .disclose_with(gate, |view| {
                    encode(&mut bytes, dto::search_page(view, trace))
                })
                .await
                .map_err(gated)?;
        }
        SearchOperation::Discover => {
            let input = parse::<DiscoveryInputDto>(&body)?
                .into_input()
                .map_err(validation)?;
            let snapshot = backend.visible(&context).await?;
            let mut disclosure = backend.discover(&context, &snapshot, input).await?;
            evaluation_closed = disclosure.evaluation_closed();
            disclosure
                .with_disclosure(gate, |view| {
                    encode(&mut bytes, dto::discovery_evaluation(&view.public(trace)))
                })
                .await
                .map_err(gated)?;
        }
        SearchOperation::Resource => {
            let id = resource_id
                .as_deref()
                .and_then(canonical_uuid)
                .ok_or_else(|| {
                    validation(vec![FieldError {
                        pointer: "/resourceId",
                        code: "INVALID_FORMAT",
                    }])
                })?;
            let snapshot = backend.visible(&context).await?;
            let mut disclosure = backend
                .resource(&context, &snapshot, ResourceId::from_uuid(id))
                .await?;
            evaluation_closed = disclosure.evaluation_closed();
            disclosure
                .disclose_with(gate, |view| {
                    encode(&mut bytes, dto::resource_detail(view, trace))
                })
                .await
                .map_err(gated)?;
        }
        SearchOperation::Sources => {
            let (page_size, cursor) = source_query(parts)?;
            let snapshot = backend.visible(&context).await?;
            let mut disclosure = backend
                .sources(&context, &snapshot, page_size, cursor)
                .await?;
            evaluation_closed = disclosure.evaluation_closed();
            disclosure
                .disclose_with(gate, |view| {
                    encode(&mut bytes, dto::source_page(view, trace))
                })
                .await
                .map_err(gated)?;
        }
    }
    let bytes = bytes.ok_or(ProblemCode::ServiceUnavailable)?;
    if bytes.len() > SUCCESS_BODY_BYTES {
        return Err(ProblemCode::ServiceUnavailable.into());
    }
    Ok((bytes, evaluation_closed))
}

fn success(bytes: Vec<u8>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, PRIVATE_NO_STORE)
        .header(header::X_CONTENT_TYPE_OPTIONS, NO_SNIFF)
        .body(Body::from(bytes))
        .unwrap_or_else(|_| {
            let mut fallback = Response::new(Body::empty());
            *fallback.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
            fallback
        })
}
