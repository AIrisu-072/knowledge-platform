//! B6: with a READY current generation, the next build is an INCREMENTAL
//! P7 bundle. Its Graph copies the base and applies one closure-proved
//! delta, its payload and lexical directory are the whole new bundle, and it
//! is published under its Graph build guard by both the manual and the fenced
//! outbox path. The published key re-verifies from storage like a full one.

#[path = "../../search-source-document/tests/support/body.rs"]
mod body_support;
#[path = "../../search-source-document/tests/support/document_discovery.rs"]
mod discovery_support;
#[path = "support/durable.rs"]
mod durable;
#[path = "support/registration.rs"]
mod registration;
mod support;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use durable::*;
use search_application::indexing_service::{DocumentSourceEvent, IndexingOutcome};
use search_application::ports::{SearchDeliveryFence, SourceFence};
use search_application::search_core::projection::ProjectionGenerationKey;
use search_runtime::recovery::{CurrentState, PgStartupRecovery};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

async fn build_kind(pool: &PgPool, key: ProjectionGenerationKey) -> (String, String) {
    sqlx::query_as(
        "SELECT build_kind, stage_origin FROM search_generation \
         WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn graph_guards(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM search_graph.build_guard")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn verified_current(durable: &Durable) -> ProjectionGenerationKey {
    match PgStartupRecovery::new(
        durable.pool.clone(),
        &durable.lexical_root,
        durable.source(),
    )
    .verify_current()
    .await
    .unwrap()
    {
        CurrentState::Verified(bundle) => bundle.key(),
        other => panic!("current must re-verify: {other:?}"),
    }
}

fn published(outcome: IndexingOutcome) -> ProjectionGenerationKey {
    match outcome {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected a published generation: {other:?}"),
    }
}

#[tokio::test]
async fn manual_rebuild_after_a_ready_current_is_an_incremental_bundle() {
    let durable = Durable::start().await;
    let event = |document| discovery_support::event("DocumentVersionPublished", document);
    let first = publish(&durable.pool, &durable.storage, "東京 規程 本文").await;
    let full = published(durable.indexer().await.handle(event(first)).await.unwrap());
    assert_eq!(build_kind(&durable.pool, full).await.0, "FULL");

    let second = publish(&durable.pool, &durable.storage, "大阪 規程 本文").await;
    let incremental = published(durable.indexer().await.handle(event(second)).await.unwrap());
    assert_ne!(incremental, full);
    assert_eq!(
        build_kind(&durable.pool, incremental).await,
        ("INCREMENTAL".into(), "MANUAL".into())
    );
    // Published under its Graph guard, which publication consumed.
    assert_eq!(graph_guards(&durable.pool).await, 0);
    assert_eq!(verified_current(&durable).await, incremental);

    // Another incremental generation builds on the incremental one.
    let third = publish(&durable.pool, &durable.storage, "名古屋 規程 本文").await;
    let next = published(durable.indexer().await.handle(event(third)).await.unwrap());
    assert_eq!(build_kind(&durable.pool, next).await.0, "INCREMENTAL");
    assert_eq!(verified_current(&durable).await, next);
}

/// One leased outbox row for `document` and the Source lease at `epoch`.
async fn deliver(
    pool: &PgPool,
    source: search_application::search_core::id::SourceId,
    document: Uuid,
    epoch: i64,
) -> (DocumentSourceEvent, SearchDeliveryFence) {
    let event = DocumentSourceEvent {
        event_id: Uuid::now_v7(),
        event_type: "DocumentVersionPublished".into(),
        aggregate_id: document,
        occurred_at: OffsetDateTime::UNIX_EPOCH,
    };
    let fence = SearchDeliveryFence {
        event_id: event.event_id,
        outbox_token: Uuid::now_v7(),
        source: SourceFence {
            source_id: source,
            owner_token: Uuid::from_u128(7_777),
            epoch,
        },
    };
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,payload, \
         occurred_at,available_at,lease_token,lease_owner,lease_expires_at) \
         VALUES ($1,$2,'Document',$3,'{}',$4,clock_timestamp(), \
         $5,$6,clock_timestamp()+interval '1 hour')",
    )
    .bind(event.event_id)
    .bind(&event.event_type)
    .bind(document)
    .bind(event.occurred_at)
    .bind(fence.outbox_token)
    .bind(Uuid::now_v7())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE search_source_coordination SET owner_token=$2, fence_epoch=$3, \
         lease_expires_at=clock_timestamp()+interval '1 hour' WHERE source_id=$1",
    )
    .bind(source.as_uuid())
    .bind(fence.source.owner_token)
    .bind(epoch)
    .execute(pool)
    .await
    .unwrap();
    (event, fence)
}

#[tokio::test]
async fn fenced_delivery_publishes_an_incremental_event_bundle_atomically() {
    let durable = Durable::start().await;
    let indexer = durable.indexer_with(durable.extractor(), true);
    let live = || Arc::new(AtomicBool::new(false));
    let first = publish(&durable.pool, &durable.storage, "東京 規程 本文").await;
    let (event, fence) = deliver(&durable.pool, durable.source_id, first, 1).await;
    let full = published(indexer.handle_delivery(event, fence, live()).await.unwrap());
    assert_eq!(
        build_kind(&durable.pool, full).await,
        ("FULL".into(), "EVENT".into())
    );

    let second = publish(&durable.pool, &durable.storage, "大阪 規程 本文").await;
    let (event, fence) = deliver(&durable.pool, durable.source_id, second, 1).await;
    let incremental = published(indexer.handle_delivery(event, fence, live()).await.unwrap());
    assert_eq!(
        build_kind(&durable.pool, incremental).await,
        ("INCREMENTAL".into(), "EVENT".into())
    );
    // The receipt, the pointer and the consumed guard committed together.
    let receipt: Uuid =
        sqlx::query_scalar("SELECT generation_id FROM search_index_receipts WHERE event_id=$1")
            .bind(fence.event_id)
            .fetch_one(&durable.pool)
            .await
            .unwrap();
    assert_eq!(receipt, incremental.generation_id.as_uuid());
    assert_eq!(graph_guards(&durable.pool).await, 0);
    assert_eq!(verified_current(&durable).await, incremental);
}
