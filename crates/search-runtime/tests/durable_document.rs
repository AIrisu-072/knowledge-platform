//! P3-D01: the existing Document outbox indexer on the durable runtime. One
//! real Document Source snapshot becomes a READY, published P7 bundle whose
//! Graph rows carry the snapshot-bound Document mapping; a restarted runtime
//! with no RAM state sees the same current key and leaves it unchanged.

#[path = "../../search-source-document/tests/support/body.rs"]
mod body_support;
#[path = "../../search-source-document/tests/support/document_discovery.rs"]
mod discovery_support;
#[path = "support/registration.rs"]
mod registration;
mod support;

use document_application::DocumentAccessCheckService;
use search_application::graph_generation::GraphSourceMapping;
use search_application::indexing_service::IndexingOutcome;
use search_application::ports::AccessDecision;
use search_application::search_core::id::ResourceId;
use search_application::search_core::projection::ProjectionGenerationKey;
use search_graph::PostgresGraphStore;
use search_runtime::recovery::{CurrentState, PgStartupRecovery};
use search_source_document::{DocumentCurrentAccessAdapter, DocumentGenerationAccess};
use sqlx::PgPool;
use uuid::Uuid;

#[path = "support/durable.rs"]
mod durable;

use durable::*;

async fn graph_nodes(pool: &PgPool, key: ProjectionGenerationKey) -> Vec<ResourceId> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT resource_id FROM search_graph.resource WHERE source_id=$1 AND generation_id=$2 \
         ORDER BY resource_id",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_all(pool)
    .await
    .unwrap();
    ids.into_iter().map(ResourceId::from_uuid).collect()
}

#[tokio::test]
async fn document_outbox_rebuild_has_durable_same_key_graph_and_survives_restart() {
    let durable = Durable::start().await;
    let document = publish(&durable.pool, &durable.storage, "東京の規程本文").await;
    let event = || discovery_support::event("DocumentVersionPublished", document);

    let first = durable.indexer().await;
    let key = match first.handle(event()).await.unwrap() {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected a published durable generation: {other:?}"),
    };
    // The pointer, the bundle and the Graph all carry this one key.
    let report = PgStartupRecovery::new(
        durable.pool.clone(),
        &durable.lexical_root,
        durable.source(),
    )
    .startup(10)
    .await
    .unwrap();
    assert!(matches!(&report.current, CurrentState::Verified(bundle) if bundle.key() == key));

    // Every Graph node is a snapshot-bound Document mapping with current access.
    let access = DocumentCurrentAccessAdapter::new(
        durable.source_id,
        durable.pool.clone(),
        DocumentAccessCheckService::new(durable.repository.clone()),
        actor(),
        discovery_support::ACCESS_CONTEXT.into(),
    );
    let gate = DocumentGenerationAccess::new(
        PostgresGraphStore::new(durable.pool.clone()),
        &access,
        discovery_support::ACCESS_CONTEXT,
    );
    let store = PostgresGraphStore::new(durable.pool.clone());
    let nodes = graph_nodes(&durable.pool, key).await;
    assert_eq!(nodes.len(), 3, "Version, Document and folder placement");
    let mut kinds = Vec::new();
    for node in nodes {
        let stored = store.ready_resource(key, node).await.unwrap().unwrap();
        kinds.push(match stored.mapping {
            GraphSourceMapping::Version { .. } => "version",
            GraphSourceMapping::Document { .. } => "document",
            GraphSourceMapping::FolderPlacement { .. } => "folder",
            GraphSourceMapping::Registered { .. } => "registered",
        });
        assert_eq!(
            gate.evaluate_stored(key, &stored, discovery_support::ACCESS_CONTEXT)
                .await
                .unwrap(),
            AccessDecision::Allowed
        );
    }
    kinds.sort_unstable();
    assert_eq!(kinds, vec!["document", "folder", "version"]);

    // A restarted runtime sees the same current key and does not rebuild it.
    drop(first);
    let restarted = durable.indexer().await;
    assert_eq!(
        restarted.handle(event()).await.unwrap(),
        IndexingOutcome::Unchanged(key)
    );
    let generations: i64 = sqlx::query_scalar("SELECT count(*) FROM search_generation")
        .fetch_one(&durable.pool)
        .await
        .unwrap();
    assert_eq!(generations, 1);

    // A changed Source snapshot builds and publishes a new key.
    let second = publish(&durable.pool, &durable.storage, "大阪の規程本文").await;
    let next = match restarted
        .handle(discovery_support::event("DocumentVersionPublished", second))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(next) => next,
        other => panic!("expected a new durable generation: {other:?}"),
    };
    assert_ne!(next, key);
    assert_eq!(
        sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT current_generation_id FROM search_source_coordination WHERE source_id=$1"
        )
        .bind(durable.source_id.as_uuid())
        .fetch_one(&durable.pool)
        .await
        .unwrap(),
        Some(next.generation_id.as_uuid())
    );
}
