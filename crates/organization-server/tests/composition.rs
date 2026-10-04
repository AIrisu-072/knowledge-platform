use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::get,
};
use organization_server::compose_routes;
use tower::ServiceExt;

#[tokio::test]
async fn organization_routes_preserve_document_requests_and_task_landing() {
    let work = Router::new().route("/v1/organization/session", get(|| async { "organization" }));
    let document = Router::new().route("/v1/documents", get(|| async { "original-document" }));
    let app = compose_routes(work, document);
    for (path, expected) in [
        ("/v1/organization/session", "organization"),
        ("/v1/documents", "original-document"),
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(to_bytes(response.into_body(), 100).await.unwrap(), expected);
    }
    let root = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(root.status(), StatusCode::SEE_OTHER);
    assert_eq!(root.headers()["location"], "/tasks");
}
