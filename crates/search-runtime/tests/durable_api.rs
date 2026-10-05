//! B4: the four API routes read the Document Source's durable P7 generation.
//! The outbox indexer publishes READY bundles into real PostgreSQL and a real
//! lexical directory; the API read model follows the current pointer,
//! re-verifies each new key before serving it, and refuses a key whose
//! artifacts no longer verify.

mod support;

#[path = "support/api.rs"]
mod api;
#[path = "../../search-source-document/tests/support/body.rs"]
mod body_support;
#[path = "../../search-source-document/tests/support/document_discovery.rs"]
mod discovery_support;
#[path = "support/durable.rs"]
mod durable;
#[path = "support/registration.rs"]
mod registration;

use std::sync::Arc;

use api::{Host, call, discovery_config, durable as api_durable};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use document_application::{DocumentAccessCheckService, InvocationKind, VerifiedActorContext};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef as DocumentPrincipal};
use durable::{Durable, publish};
use search_api_http::router::ApiFuture;
use search_application::api_scope::ApiError;
use search_application::indexing_service::IndexingOutcome;
use search_application::retrieval::{RetrievalInputs, RetrieverSupport};
use search_application::scoped::TrustedSearchScope;
use search_application::search_core::projection::ProjectionGenerationKey;
use search_application::source_registration::HostRegistrationSnapshotPort;
use search_runtime::api::build_search_api_runtime;
use search_runtime::durable_read::{
    DocumentActorAccess, DocumentActorAccessPort, DurableDocumentPorts, DurableDocumentReadModel,
};
use search_runtime::lexical_artifact::LexicalArtifactStore;
use search_source_document::{DocumentApiRead, DocumentCurrentAccessAdapter};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

/// The host's Document identity mapping: a Search principal is the Document
/// principal of the same name at the test IdP.
struct HostDocumentAccess {
    durable_source: search_application::search_core::id::SourceId,
    pool: sqlx::PgPool,
    repository: Arc<document_repository_postgres::PostgresDocumentRepository>,
}

impl DocumentActorAccessPort for HostDocumentAccess {
    fn for_actor<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
    ) -> ApiFuture<'a, DocumentActorAccess> {
        Box::pin(async move {
            let principal = actor.principal().as_str();
            let document_actor = VerifiedActorContext::from_trusted_adapter(
                DocumentPrincipal::new("test-idp", principal)
                    .map_err(|_| ApiError::IdentityUnavailable)?,
                vec![
                    PolicySubject::new(PolicySubjectKind::Principal, "test-idp", principal)
                        .map_err(|_| ApiError::IdentityUnavailable)?,
                ],
                OffsetDateTime::now_utc() + time::Duration::minutes(10),
                InvocationKind::HumanInteractive,
                None,
            )
            .map_err(|_| ApiError::IdentityUnavailable)?;
            let binding = actor.access_handle().to_opaque_string();
            let access = Arc::new(DocumentCurrentAccessAdapter::new(
                self.durable_source,
                self.pool.clone(),
                DocumentAccessCheckService::new(self.repository.clone()),
                document_actor,
                binding.clone(),
            ));
            let read = Arc::new(DocumentApiRead::new(
                self.durable_source,
                self.pool.clone(),
                access.clone(),
                binding,
            ));
            Ok(DocumentActorAccess {
                access,
                resource_locator: read.clone(),
                resource_reader: read,
            })
        })
    }
}

async fn index(durable: &Durable, document: Uuid) -> ProjectionGenerationKey {
    let event = discovery_support::event("DocumentVersionPublished", document);
    match durable.indexer().await.handle(event).await.unwrap() {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected a published durable generation: {other:?}"),
    }
}

fn post(path: &str, token: &str, body: &Value) -> Request<Body> {
    Request::post(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn get(path: &str, token: &str) -> Request<Body> {
    Request::get(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn items(body: &Value) -> Vec<Value> {
    body["items"]
        .as_array()
        .unwrap_or_else(|| panic!("{body}"))
        .clone()
}

#[tokio::test]
async fn four_routes_follow_the_verified_durable_current_and_refuse_drift() {
    let durable = Durable::start_with(true).await;
    let tokyo = publish(&durable.pool, &durable.storage, "東京 規程 本文").await;
    index(&durable, tokyo).await;

    let host = Host::new();
    host.grants.grant("tenant-a", "editor", durable.source_id);
    host.login("editor-token", "tenant-a", "editor");
    let model = Arc::new(DurableDocumentReadModel::new(
        durable.pool.clone(),
        &durable.lexical_root,
        durable.source(),
    ));
    let ports = Arc::new(DurableDocumentPorts::new(
        model,
        Arc::new(HostDocumentAccess {
            durable_source: durable.source_id,
            pool: durable.pool.clone(),
            repository: durable.repository.clone(),
        }),
    ));
    let snapshot: Arc<dyn HostRegistrationSnapshotPort> = durable.host.clone();
    let runtime = build_search_api_runtime(
        host.config(
            snapshot,
            None,
            discovery_config(RetrieverSupport::default(), RetrievalInputs::default()),
        ),
        api_durable(&durable.pool, ports),
        host.identity(),
    )
    .await
    .unwrap();
    let router = runtime.router();
    let search = |query: &str, coverage: &str| {
        post(
            "/v1/search",
            "editor-token",
            &json!({"query": query, "coverage": coverage}),
        )
    };

    // Title, metadata and the body come from the stored bundle.
    let found = call(&router, search("規程", "titleAndPermittedMetadata")).await;
    assert_eq!(found.status, StatusCode::OK, "{}", found.body);
    let first = items(&found.body);
    assert_eq!(first.len(), 1, "{}", found.body);
    let body_hit = call(&router, search("東京", "bodyRequired")).await;
    assert_eq!(body_hit.status, StatusCode::OK, "{}", body_hit.body);
    assert_eq!(items(&body_hit.body).len(), 1, "{}", body_hit.body);
    let resource = first[0]["resourceId"].as_str().unwrap().to_owned();
    let read = call(
        &router,
        get(&format!("/v1/resources/{resource}"), "editor-token"),
    )
    .await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    let sources = call(&router, get("/v1/sources", "editor-token")).await;
    assert_eq!(sources.status, StatusCode::OK, "{}", sources.body);
    assert!(
        sources
            .body
            .to_string()
            .contains(&durable.source_id.as_uuid().to_string()),
        "{}",
        sources.body
    );

    // A newer publication moves the pointer; the next request serves it.
    let osaka = publish(&durable.pool, &durable.storage, "大阪 規程 本文").await;
    index(&durable, osaka).await;
    let moved = call(&router, search("大阪", "bodyRequired")).await;
    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);
    assert_eq!(items(&moved.body).len(), 1, "{}", moved.body);
    let both = call(&router, search("規程", "titleAndPermittedMetadata")).await;
    assert_eq!(items(&both.body).len(), 2, "{}", both.body);

    // A new current key whose sealed lexical directory is gone never serves.
    let nagoya = publish(&durable.pool, &durable.storage, "名古屋 規程 本文").await;
    let drifted = index(&durable, nagoya).await;
    let lexical = LexicalArtifactStore::new(&durable.lexical_root, durable.pool.clone());
    std::fs::remove_dir_all(lexical.final_dir(drifted)).unwrap();
    let refused = call(&router, search("規程", "titleAndPermittedMetadata")).await;
    assert_eq!(
        refused.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        refused.body
    );
}
