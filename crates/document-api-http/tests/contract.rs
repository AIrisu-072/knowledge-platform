use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use document_api_http::api::{DocumentApiRouters, compose_document_api};
use tower::ServiceExt;

fn family(name: &'static str) -> Router {
    Router::new().fallback(move || async move { name })
}

#[tokio::test]
async fn every_openapi_operation_is_dispatched_to_exactly_one_handler_family() {
    let api = compose_document_api(DocumentApiRouters::new(
        family("read"),
        family("management"),
        family("create"),
        family("versioning"),
        family("publication"),
        family("file"),
        family("diff"),
    ));
    let id = "01890f7a-6f6e-7b0a-8000-000000000001";
    let cases = [
        (Method::GET, "/v1/documents".to_owned(), "read"),
        (Method::POST, "/v1/documents".to_owned(), "create"),
        (
            Method::GET,
            format!("/v1/document-creation-outcomes/{id}"),
            "create",
        ),
        (Method::GET, format!("/v1/documents/{id}"), "read"),
        (Method::GET, format!("/v1/documents/{id}/versions"), "read"),
        (
            Method::POST,
            format!("/v1/documents/{id}/versions"),
            "versioning",
        ),
        (
            Method::GET,
            format!("/v1/documents/{id}/versions/{id}"),
            "read",
        ),
        (
            Method::GET,
            format!("/v1/documents/{id}/versions/{id}/edit-manifest"),
            "read",
        ),
        (Method::GET, format!("/v1/documents/{id}/revisions"), "read"),
        (
            Method::GET,
            format!("/v1/documents/{id}/revisions/{id}"),
            "read",
        ),
        (
            Method::PUT,
            format!("/v1/documents/{id}/versions/{id}"),
            "versioning",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}/versions/{id}:rebase"),
            "versioning",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}/versions/{id}:publish"),
            "publication",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}/versions/{id}:withdraw"),
            "publication",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}/versions/{id}:schedule-publication"),
            "publication",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}/versions/{id}:cancel-publication-schedule"),
            "publication",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}:end-publication"),
            "publication",
        ),
        (
            Method::PATCH,
            format!("/v1/documents/{id}/metadata"),
            "management",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}:move"),
            "management",
        ),
        (
            Method::GET,
            format!("/v1/documents/{id}/access-policy"),
            "read",
        ),
        (
            Method::PUT,
            format!("/v1/documents/{id}/access-policy"),
            "management",
        ),
        (
            Method::PUT,
            format!("/v1/documents/{id}/versions/{id}/read-state"),
            "management",
        ),
        (
            Method::GET,
            format!("/v1/documents/{id}/versions/{id}/read-state"),
            "management",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}/versions/{id}/read-state/view"),
            "management",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}/versions/{id}/read-state/reset"),
            "management",
        ),
        (Method::GET, format!("/v1/documents/{id}/history"), "read"),
        (
            Method::GET,
            format!("/v1/documents/{id}/versions/{id}/files"),
            "read",
        ),
        (
            Method::GET,
            format!("/v1/documents/{id}/versions/{id}/files/{id}/{id}"),
            "file",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}/comparisons"),
            "diff",
        ),
        (
            Method::POST,
            format!("/v1/documents/{id}/revision-comparisons"),
            "diff",
        ),
        (Method::GET, "/v1/folders/root".to_owned(), "read"),
        (Method::GET, format!("/v1/folders/{id}/children"), "read"),
        (Method::POST, "/v1/folders".to_owned(), "management"),
        (Method::PATCH, format!("/v1/folders/{id}"), "management"),
        (Method::POST, format!("/v1/folders/{id}:move"), "management"),
        (
            Method::GET,
            format!("/v1/folders/{id}/access-policy"),
            "read",
        ),
        (
            Method::PUT,
            format!("/v1/folders/{id}/access-policy"),
            "management",
        ),
    ];

    assert_eq!(cases.len(), 37);
    for (method, uri, expected) in cases {
        let response = api
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(&uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{method} {uri}");
        let body = to_bytes(response.into_body(), 32).await.unwrap();
        assert_eq!(body.as_ref(), expected.as_bytes(), "{method} {uri}");
    }
}
