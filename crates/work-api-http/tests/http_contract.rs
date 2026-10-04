use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use work_application::{WorkFuture, WorkRepository};
use work_domain::*;
struct Unavailable;
impl WorkRepository for Unavailable {
    fn list_tasks(&self, _: VerifiedActor, _: TaskView) -> WorkFuture<'_, Vec<TaskSummary>> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn task(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, TaskDetail> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn artifact(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, WorkingArtifact> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn snapshot(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, HandoffSnapshot> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn execute(&self, _: VerifiedActor, _: Command) -> WorkFuture<'_, MutationResult> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
    fn recover(&self, _: VerifiedActor, _: Uuid) -> WorkFuture<'_, MutationResult> {
        Box::pin(async { Err(WorkError::DependencyUnavailable) })
    }
}
fn app() -> axum::Router {
    work_api_http::router(Arc::new(Unavailable), VerifiedActor::Sales01)
}
#[tokio::test]
async fn startup_identity_is_returned_and_capabilities_are_explicitly_unavailable() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/v1/organization/session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["principalId"], "sales-01");
    assert_eq!(body["capabilities"]["fileUpload"], false);
}
#[tokio::test]
async fn identity_override_headers_and_query_are_rejected_instead_of_switching_actor() {
    for request in [
        Request::builder()
            .uri("/v1/organization/session")
            .header("x-principal-id", "office-01")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .uri("/v1/organization/session?principalId=office-01")
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
#[tokio::test]
async fn unrecognized_body_fields_are_safe_problem_details() {
    let body = serde_json::json!({"operationId":Uuid::now_v7(), "expectedRevision":0,"actingAssignmentId":SALES_ASSIGNMENT_ID,"principalId":"office-01"});
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/organization/tasks/{SALES_TASK_ID}/claim"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/problem+json"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["code"], "VALIDATION_FAILED");
    assert!(!body.to_string().contains("office-01"));
}
#[tokio::test]
async fn repository_outage_is_not_an_empty_task_list() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/v1/organization/tasks?view=queue")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["code"], "DEPENDENCY_UNAVAILABLE");
}
#[tokio::test]
async fn oversized_body_and_unknown_routes_use_closed_errors() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/organization/tasks/{SALES_TASK_ID}/claim"))
                .header("content-type", "application/json")
                .body(Body::from(" ".repeat(1_048_577)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/problem+json"
    );
}
