use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;

use crate::error::{ApiProblem, ErrorCode};
use crate::identity::{IdentityAdapter, IdentityRequestContext};
use crate::trace::{TraceContext, attach_trace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupError {
    IdentityAdapterMissing,
}

/// No production default adapter exists. The composition root must supply one.
pub fn protect_routes(
    routes: Router,
    identity_adapter: Option<Arc<dyn IdentityAdapter>>,
) -> Result<Router, StartupError> {
    let identity_adapter = identity_adapter.ok_or(StartupError::IdentityAdapterMissing)?;
    Ok(routes
        .layer(middleware::from_fn_with_state(
            identity_adapter,
            authenticate,
        ))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(attach_trace)))
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
    let mut response = next.run(request).await;
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
