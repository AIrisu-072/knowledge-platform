#[path = "../../document-repository-postgres/tests/support/management.rs"]
mod support;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::read::read_router;
use document_application::{BootstrapRootPolicy, IdentityResolutionError, VerifiedActorContext};
use document_domain::{Action, PolicyGrant, PolicySubject, PolicySubjectKind};
use serde_json::Value;
use support::{context, fixture};
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

async fn get(router: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn root_discovery_and_query_validation_cross_the_http_boundary() {
    let f = fixture().await;
    let subject =
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap();
    f.repository
        .initialize_root_policy(
            &context(),
            vec![PolicyGrant::new(subject, [Action::Read, Action::Administer]).unwrap()],
        )
        .await
        .unwrap();
    let router = read_router(f.repository.clone(), Arc::new(FixedIdentity(context()))).unwrap();
    let (status, body) = get(router.clone(), "/v1/folders/root").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["folderId"], f.root_id.as_uuid().to_string());
    let (status, body) = get(router, "/v1/documents?view=published&pageSize=201").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "VALIDATION_FAILED");
}
