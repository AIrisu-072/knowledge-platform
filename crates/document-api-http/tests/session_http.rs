use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_api_http::read::session_router;
use document_application::{
    IdentityPresentation, IdentityPresentationResolution, IdentityPresentationResolutionError,
    IdentityPresentationResolver, IdentityRef, InvocationKind, VerifiedActorContext,
};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;

#[derive(Clone)]
struct FixedIdentity(VerifiedActorContext);

impl IdentityAdapter for FixedIdentity {
    fn resolve<'a>(
        &'a self,
        _request: &'a IdentityRequestContext,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedActorContext, document_application::IdentityResolutionError>> + Send + 'a>,
    > {
        let verified = self.0.clone();
        Box::pin(async move { Ok(verified) })
    }
}

#[derive(Clone)]
struct FixedPresentation {
    calls: Arc<AtomicUsize>,
    requested: Arc<Mutex<Vec<Vec<IdentityRef>>>>,
    result: Result<Vec<IdentityPresentation>, IdentityPresentationResolutionError>,
}

impl IdentityPresentationResolver for FixedPresentation {
    fn resolve_batch<'a>(
        &'a self,
        refs: &'a [IdentityRef],
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Vec<IdentityPresentation>, IdentityPresentationResolutionError>>
                + Send
                + 'a,
        >,
    > {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requested.lock().unwrap().push(refs.to_vec());
        let result = self.result.clone();
        Box::pin(async move { result })
    }
}

fn verified_actor() -> VerifiedActorContext {
    let principal = PrincipalRef::new("trusted-directory", "verified-user-7").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![PolicySubject::new(
            PolicySubjectKind::Principal,
            "trusted-directory",
            "verified-user-7",
        )
        .unwrap()],
        OffsetDateTime::now_utc() + Duration::minutes(30),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

async fn get_session(router: axum::Router) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/session?principalId=forged-query-user")
                .header("x-principal-id", "forged-header-user")
                .header("x-identity-provider", "forged-provider")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn session_uses_only_verified_actor_for_principal_and_identity_presentation() {
    let actor = verified_actor();
    let expected_expiry = actor.valid_until().format(&time::format_description::well_known::Rfc3339).unwrap();
    let expected_ref = IdentityRef::from_principal(actor.principal());
    let resolver = FixedPresentation {
        calls: Arc::new(AtomicUsize::new(0)),
        requested: Arc::new(Mutex::new(Vec::new())),
        result: Ok(vec![IdentityPresentation {
            reference: expected_ref.clone(),
            display_name: Some("Verified User".into()),
            secondary_text: Some("Operations".into()),
            resolution: IdentityPresentationResolution::Resolved,
        }]),
    };
    let router = session_router(Arc::new(FixedIdentity(actor)), Arc::new(resolver.clone())).unwrap();

    let (status, body) = get_session(router).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["principal"]["identityProvider"], "trusted-directory");
    assert_eq!(body["principal"]["principalId"], "verified-user-7");
    assert_eq!(body["presentation"]["ref"]["subjectId"], "verified-user-7");
    assert_eq!(body["presentation"]["displayName"], "Verified User");
    assert_eq!(body["presentation"]["resolution"], "resolved");
    assert_eq!(body["invocationKind"], "human_interactive");
    assert_eq!(body["expiresAt"], expected_expiry);
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(resolver.requested.lock().unwrap().as_slice(), &[vec![expected_ref]]);
}

#[tokio::test]
async fn unavailable_session_presentation_keeps_the_verified_identity_and_returns_success() {
    let actor = verified_actor();
    let resolver = FixedPresentation {
        calls: Arc::new(AtomicUsize::new(0)),
        requested: Arc::new(Mutex::new(Vec::new())),
        result: Err(IdentityPresentationResolutionError::Unavailable),
    };
    let router = session_router(Arc::new(FixedIdentity(actor)), Arc::new(resolver)).unwrap();

    let (status, body) = get_session(router).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["principal"]["principalId"], "verified-user-7");
    assert_eq!(body["presentation"]["ref"]["subjectId"], "verified-user-7");
    assert_eq!(body["presentation"]["displayName"], Value::Null);
    assert_eq!(body["presentation"]["resolution"], "unavailable");
}
