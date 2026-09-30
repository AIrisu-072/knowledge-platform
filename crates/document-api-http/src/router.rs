use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use http_body::Body as _;

use crate::error::{ApiProblem, ErrorCode};
use crate::identity::{IdentityAdapter, IdentityRequestContext};
use crate::limits::{MAX_JSON_BODY_BYTES, MAX_JSON_RESPONSE_BYTES, MAX_REQUEST_HEADER_BYTES};
use crate::multipart::request_header_bytes;
use crate::trace::{
    HttpObservationSink, TraceContext, TracingObservationSink, attach_trace, observe_request,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupError {
    IdentityAdapterMissing,
}

/// No production default adapter exists. The composition root must supply one.
pub fn protect_routes(
    routes: Router,
    identity_adapter: Option<Arc<dyn IdentityAdapter>>,
) -> Result<Router, StartupError> {
    protect_routes_with_observer(routes, identity_adapter, Arc::new(TracingObservationSink))
}

pub fn protect_routes_with_observer(
    routes: Router,
    identity_adapter: Option<Arc<dyn IdentityAdapter>>,
    observer: Arc<dyn HttpObservationSink>,
) -> Result<Router, StartupError> {
    let identity_adapter = identity_adapter.ok_or(StartupError::IdentityAdapterMissing)?;
    Ok(routes
        .layer(middleware::from_fn_with_state(observer, observe_request))
        .layer(middleware::from_fn_with_state(
            identity_adapter,
            authenticate,
        ))
        .layer(middleware::from_fn(enforce_request_headers))
        .layer(middleware::from_fn(security_headers))
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(middleware::from_fn(attach_trace)))
}

async fn enforce_request_headers(request: Request, next: Next) -> Response {
    if request_header_bytes(request.headers()) > MAX_REQUEST_HEADER_BYTES {
        let trace_id = request
            .extensions()
            .get::<TraceContext>()
            .map(|trace| trace.trace_id.as_str())
            .unwrap_or("");
        return ApiProblem::new(ErrorCode::ValidationFailed, request.uri().path(), trace_id)
            .into_response();
    }
    next.run(request).await
}

async fn authenticate(
    State(adapter): State<Arc<dyn IdentityAdapter>>,
    mut request: Request,
    next: Next,
) -> Response {
    let identity_request = IdentityRequestContext::from_headers(request.headers());
    let actor = match adapter.resolve(&identity_request).await {
        Ok(actor) if actor.ensure_current().is_ok() => actor,
        Ok(_) | Err(document_application::IdentityResolutionError::InvalidIdentity) => {
            return problem_response(&request, ErrorCode::AuthenticationRequired);
        }
        Err(document_application::IdentityResolutionError::Unavailable) => {
            return problem_response(&request, ErrorCode::IdentityUnavailable);
        }
    };
    request.extensions_mut().insert(actor);
    next.run(request).await
}

fn problem_response(request: &Request, code: ErrorCode) -> Response {
    let trace_id = request
        .extensions()
        .get::<TraceContext>()
        .map(|context| context.trace_id.as_str())
        .unwrap_or("");
    axum::response::IntoResponse::into_response(ApiProblem::new(
        code,
        request.uri().path(),
        trace_id,
    ))
}

async fn security_headers(request: Request, next: Next) -> Response {
    let instance = request.uri().path().to_owned();
    let trace_id = request
        .extensions()
        .get::<TraceContext>()
        .map(|trace| trace.trace_id.clone())
        .unwrap_or_default();
    let response = next.run(request).await;
    let json_response = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.starts_with("application/json") || value.starts_with("application/problem+json")
        });
    let too_large = json_response
        && response
            .body()
            .size_hint()
            .upper()
            .is_none_or(|bytes| bytes > MAX_JSON_RESPONSE_BYTES as u64);
    let mut response = if too_large {
        ApiProblem::new(ErrorCode::Internal, &instance, &trace_id).into_response()
    } else {
        response
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}
