//! E (P2-07): Vector over the durable Document Source, end to end through
//! the public Discover route. The maintainer builds and publishes the
//! current bundle's Vector generation into PostgreSQL; Discover with a query
//! plans the Vector retriever and every hit is resolved against the loaded
//! generation's Units and the actor's current Document Read. A deterministic
//! character-bigram embedding stands in for the pinned model (CI has no
//! model files); the adapter's real-model check is `search-vector-adapter`.

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
use search_application::ports::BoxFuture;
use search_application::retrieval::{RetrievalInputs, RetrieverSupport};
use search_application::scoped::TrustedSearchScope;
use search_application::search_core::vector::{
    BoundEmbedding, EmbeddingModelSpec, QueryEmbedding, VectorManifestUnit,
};
use search_application::source_registration::HostRegistrationSnapshotPort;
use search_application::vector::{EmbeddingProvider, TrustedVectorQuery, VectorBuildOutcome};
use search_runtime::api::build_search_api_runtime;
use search_runtime::durable_read::{
    DocumentActorAccess, DocumentActorAccessPort, DurableDocumentPorts, DurableDocumentReadModel,
};
use search_runtime::vector_runtime::{RegisteredVectorActivation, VectorMaintainer, VectorServices};
use search_runtime::vector_store::{PgVectorGenerations, PgVectorIndex};
use search_source_document::{DocumentApiRead, DocumentCurrentAccessAdapter};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

const DIMENSION: usize = 384;

/// Deterministic bag-of-character-bigrams embedding.
struct BigramProvider {
    spec: EmbeddingModelSpec,
}

impl BigramProvider {
    fn new() -> Self {
        let mut spec = search_vector_adapter::e5_small_spec();
        spec.model_name = "test/character-bigram-hash".into();
        Self { spec }
    }

    fn embed(text: &str) -> Vec<f32> {
        let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
        let mut values = vec![0f32; DIMENSION];
        for pair in chars.windows(2) {
            let hash = pair
                .iter()
                .fold(2_166_136_261u32, |h, c| (h ^ *c as u32).wrapping_mul(16_777_619));
            values[hash as usize % DIMENSION] += 1.0;
        }
        if values.iter().all(|value| *value == 0.0) {
            values[0] = 1.0;
        }
        let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
        values.iter().map(|value| value / norm).collect()
    }
}

impl EmbeddingProvider for BigramProvider {
    fn spec(&self) -> &EmbeddingModelSpec {
        &self.spec
    }

    fn embed_units<'a>(
        &'a self,
        units: &'a [VectorManifestUnit],
    ) -> BoxFuture<'a, Vec<BoundEmbedding>> {
        Box::pin(async move {
            Ok(units
                .iter()
                .map(|item| {
                    BoundEmbedding::new(&self.spec, &item.unit, &item.authority, Self::embed(&item.unit.text))
                        .unwrap()
                })
                .collect())
        })
    }

    fn embed_query<'a>(&'a self, query: &'a TrustedVectorQuery) -> BoxFuture<'a, QueryEmbedding> {
        Box::pin(async move { Ok(QueryEmbedding::new(&self.spec, Self::embed(query.text())).unwrap()) })
    }
}

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

async fn index(durable: &Durable, document: Uuid) {
    let event = discovery_support::event("DocumentVersionPublished", document);
    match durable.indexer().await.handle(event).await.unwrap() {
        IndexingOutcome::Published(_) | IndexingOutcome::Unchanged(_) => {}
        other => panic!("expected a published durable generation: {other:?}"),
    }
}

async fn version(durable: &Durable, document: Uuid) -> String {
    let id: Uuid =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id=$1")
            .bind(document)
            .fetch_one(&durable.pool)
            .await
            .unwrap();
    id.to_string()
}

fn qualified(body: &Value) -> Vec<String> {
    body["qualifiedResources"]
        .as_array()
        .unwrap_or_else(|| panic!("{body}"))
        .iter()
        .map(|item| item["resourceId"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn discover_ranks_vector_candidates_through_the_owning_source() {
    let durable = Durable::start_with(true).await;
    let tokyo = publish(&durable.pool, &durable.storage, "東京本社の就業規程と休暇の申請").await;
    let rooms = publish(&durable.pool, &durable.storage, "会議室の予約手順と利用時間").await;
    index(&durable, tokyo).await;
    index(&durable, rooms).await;

    let provider: Arc<dyn EmbeddingProvider> = Arc::new(BigramProvider::new());
    let services = VectorServices {
        provider: provider.clone(),
        index: Arc::new(PgVectorIndex::new(durable.pool.clone(), 0.2)),
        generations: Arc::new(PgVectorGenerations::new(durable.pool.clone())),
        activations: Arc::new(RegisteredVectorActivation::new(
            [durable.source_id],
            provider.as_ref(),
        )),
    };
    let maintainer = VectorMaintainer::new(durable.pool.clone(), durable.source(), services.clone());
    assert!(matches!(
        maintainer.ensure_current().await.unwrap(),
        Some(VectorBuildOutcome::Published(_))
    ));
    // The published generation is not built again.
    assert!(maintainer.ensure_current().await.unwrap().is_none());

    let host = Host::new();
    for principal in ["editor", "outsider"] {
        host.grants.grant("tenant-a", principal, durable.source_id);
        host.login(&format!("{principal}-token"), "tenant-a", principal);
    }
    let ports = Arc::new(
        DurableDocumentPorts::new(
            Arc::new(DurableDocumentReadModel::new(
                durable.pool.clone(),
                &durable.lexical_root,
                durable.source(),
            )),
            Arc::new(HostDocumentAccess {
                durable_source: durable.source_id,
                pool: durable.pool.clone(),
                repository: durable.repository.clone(),
            }),
        )
        .with_vector(services.clone()),
    );
    // Only the Vector retriever runs, so every qualified Resource is a
    // Vector candidate.
    let mut config = discovery_config(RetrieverSupport::default(), RetrievalInputs::default());
    config.retriever_support = RetrieverSupport::default();
    let snapshot: Arc<dyn HostRegistrationSnapshotPort> = durable.host.clone();
    let runtime = build_search_api_runtime(
        host.config(snapshot, None, config),
        api_durable(&durable.pool, ports),
        host.identity(),
    )
    .await
    .unwrap();
    let router = runtime.router();
    let discover = |token: &str, query: &str| {
        Request::post("/v1/discover")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "need": {
                        "purpose": "find the rule",
                        "requiredResourceTypes": ["knowledge"],
                        "requiredClaimIds": [Uuid::now_v7().to_string()],
                    },
                    "query": query,
                    "coverage": "titleAndPermittedMetadata",
                })
                .to_string(),
            ))
            .unwrap()
    };

    let leave = call(&router, discover("editor-token", "休暇の申請")).await;
    assert_eq!(leave.status, StatusCode::OK, "{}", leave.body);
    assert_eq!(qualified(&leave.body)[0], version(&durable, tokyo).await, "{}", leave.body);
    let booking = call(&router, discover("editor-token", "会議室の予約")).await;
    assert_eq!(qualified(&booking.body)[0], version(&durable, rooms).await, "{}", booking.body);
    // Below the similarity floor nothing is a candidate.
    let unrelated = call(&router, discover("editor-token", "ｘｙｚｗ")).await;
    assert!(qualified(&unrelated.body).is_empty(), "{}", unrelated.body);
    // Without Document Read, no Vector hit leaves the Source.
    let outsider = call(&router, discover("outsider-token", "休暇の申請")).await;
    assert_eq!(outsider.status, StatusCode::OK, "{}", outsider.body);
    assert!(qualified(&outsider.body).is_empty(), "{}", outsider.body);

    // A newer bundle without its Vector generation yet: unavailable, never
    // absence, until the maintainer publishes it.
    let osaka = publish(&durable.pool, &durable.storage, "大阪支社の休暇の申請").await;
    index(&durable, osaka).await;
    let pending = call(&router, discover("editor-token", "休暇の申請")).await;
    assert!(qualified(&pending.body).is_empty(), "{}", pending.body);
    assert!(
        pending.body["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["blocking"] == true),
        "{}",
        pending.body
    );
    assert!(maintainer.ensure_current().await.unwrap().is_some());
    let rebuilt = call(&router, discover("editor-token", "休暇の申請")).await;
    let found = qualified(&rebuilt.body);
    assert!(found.contains(&version(&durable, tokyo).await), "{}", rebuilt.body);
    assert!(found.contains(&version(&durable, osaka).await), "{}", rebuilt.body);
}
