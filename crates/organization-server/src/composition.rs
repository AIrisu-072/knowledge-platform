use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use tower::ServiceExt;

#[derive(Clone)]
struct Routes {
    work: Router,
    document: Router,
}
/// Forward intact requests to their existing authority. No Document business logic.
pub fn compose_routes(work: Router, document: Router) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/tasks") }))
        .fallback(dispatch)
        .with_state(Routes { work, document })
}
async fn dispatch(State(routes): State<Routes>, request: Request<Body>) -> Response {
    let path = request.uri().path();
    let selected = if path == "/v1/organization" || path.starts_with("/v1/organization/") {
        routes.work
    } else {
        routes.document
    };
    match selected.oneshot(request).await {
        Ok(response) => response.into_response(),
        Err(never) => match never {},
    }
}
