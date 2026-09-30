use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};

use crate::error::{ApiProblem, ErrorCode};
use crate::trace::TraceContext;

pub fn with_operation_timeout(routes: Router, budget: Duration) -> Router {
    routes.layer(middleware::from_fn_with_state(budget, operation_timeout))
}

async fn operation_timeout(
    State(budget): State<Duration>,
    request: Request,
    next: Next,
) -> Response {
    let instance = request.uri().path().to_owned();
    let trace_id = request
        .extensions()
        .get::<TraceContext>()
        .map(|trace| trace.trace_id.clone())
        .unwrap_or_default();
    match tokio::time::timeout(budget, next.run(request)).await {
        Ok(response) => response,
        Err(_) => ApiProblem::new(ErrorCode::Timeout, &instance, &trace_id).into_response(),
    }
}
