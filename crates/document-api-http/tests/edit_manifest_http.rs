//! Pure HTTP route validation: never connects to the lazy PostgreSQL pool.
use std::{future::Future, pin::Pin, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use document_api_http::{
    identity::{IdentityAdapter, IdentityRequestContext},
    read::read_router,
};
use document_application::{IdentityResolutionError, InvocationKind, VerifiedActorContext};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use document_repository_postgres::PostgresDocumentRepository;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;

struct FixedIdentity;
impl IdentityAdapter for FixedIdentity {
    fn resolve<'a>(
        &'a self,
        _: &'a IdentityRequestContext,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedActorContext, IdentityResolutionError>> + Send + 'a>,
    > {
        Box::pin(async {
            Ok(VerifiedActorContext::from_trusted_adapter(
                PrincipalRef::new("test-idp", "editor").unwrap(),
                vec![
                    PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap(),
                ],
                OffsetDateTime::now_utc() + Duration::hours(1),
                InvocationKind::Agent,
                None,
            )
            .unwrap())
        })
    }
}

#[tokio::test]
async fn edit_manifest_rejects_history_missing_and_unknown_purpose_before_storage() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    let router = read_router(
        Arc::new(PostgresDocumentRepository::new(pool)),
        Arc::new(FixedIdentity),
    )
    .unwrap();
    for query in ["", "?purpose=history", "?purpose=unknown"] {
        let response = router.clone().oneshot(Request::builder()
            .uri(format!("/v1/documents/00000000-0000-4000-8000-000000000001/versions/00000000-0000-4000-8000-000000000002/edit-manifest{query}"))
            .body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{query}"
        );
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap())
                .unwrap();
        assert_eq!(value["code"], "VALIDATION_FAILED");
    }
}

struct ManifestRepository {
    denied: bool,
    calls: std::sync::atomic::AtomicUsize,
}
impl document_application::EditManifestRepository for ManifestRepository {
    async fn get_edit_manifest(
        &self,
        _: &VerifiedActorContext,
        request: document_application::EditManifestRequest,
    ) -> Result<document_application::EditManifest, document_application::RepositoryError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.denied {
            return Err(document_application::RepositoryError::DocumentVersionNotFound);
        }
        use document_application::{
            EditManifest, EditManifestItem, EditManifestRepresentation, EditManifestRole,
        };
        Ok(EditManifest {
            document_id: request.document_id,
            source_version_id: request.source_version_id,
            document_revision: 27,
            purpose: request.purpose,
            title: "Exact source".into(),
            items: vec![EditManifestItem {
                content_item_id: uuid::Uuid::from_u128(6),
                logical_path: "appendix".into(),
                ordinal: 4,
                representations: vec![
                    EditManifestRepresentation {
                        representation_id: uuid::Uuid::from_u128(8),
                        role: EditManifestRole::Authoritative,
                        file_id: uuid::Uuid::from_u128(10),
                        original_filename: "../source\\unmodified\nname.txt".into(),
                        media_type: "text/plain".into(),
                        size_bytes: 23,
                    },
                    EditManifestRepresentation {
                        representation_id: uuid::Uuid::from_u128(7),
                        role: EditManifestRole::Rendition,
                        file_id: uuid::Uuid::from_u128(11),
                        original_filename: "preview.pdf".into(),
                        media_type: "application/pdf".into(),
                        size_bytes: 24,
                    },
                ],
            }],
        })
    }
}

#[tokio::test]
async fn edit_manifest_serializes_the_exact_snapshot_without_display_name_sanitization() {
    let repository = Arc::new(ManifestRepository {
        denied: false,
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let router = document_api_http::edit_manifest::edit_manifest_router(
        repository.clone(),
        Arc::new(FixedIdentity),
    )
    .unwrap();
    let response = router.oneshot(Request::builder()
        .uri("/v1/documents/00000000-0000-4000-8000-000000000001/versions/00000000-0000-4000-8000-000000000002/edit-manifest?purpose=authoring")
        .body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "documentId": "00000000-0000-4000-8000-000000000001",
            "sourceVersionId": "00000000-0000-4000-8000-000000000002",
            "documentRevision": 27, "purpose": "authoring", "title": "Exact source",
            "items": [{"contentItemId": uuid::Uuid::from_u128(6), "logicalPath":"appendix", "ordinal":4,
                "representations":[
                    {"representationId":uuid::Uuid::from_u128(8), "role":"authoritative", "fileId":uuid::Uuid::from_u128(10), "originalFilename":"../source\\unmodified\nname.txt", "mediaType":"text/plain", "sizeBytes":23},
                    {"representationId":uuid::Uuid::from_u128(7), "role":"rendition", "fileId":uuid::Uuid::from_u128(11), "originalFilename":"preview.pdf", "mediaType":"application/pdf", "sizeBytes":24},
                ]}]
        })
    );
    assert_eq!(
        repository.calls.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn edit_manifest_does_not_retry_an_unauthorized_source_as_history() {
    let repository = Arc::new(ManifestRepository {
        denied: true,
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let router = document_api_http::edit_manifest::edit_manifest_router(
        repository.clone(),
        Arc::new(FixedIdentity),
    )
    .unwrap();
    let response = router.oneshot(Request::builder()
        .uri("/v1/documents/00000000-0000-4000-8000-000000000001/versions/00000000-0000-4000-8000-000000000002/edit-manifest?purpose=published")
        .body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap();
    assert_eq!(value["code"], "DOCUMENT_VERSION_NOT_FOUND");
    assert!(value.get("items").is_none());
    assert_eq!(
        repository.calls.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}
