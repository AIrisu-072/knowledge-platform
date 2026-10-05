//! P6-S04..S06: a Document outbox event, delivered under a live outbox lease
//! and Source lease, reaches a current READY bundle and a Search receipt only
//! through the atomic P7 completion port; the generic row is never acked by
//! Search; cancellation and lost fences leave the current pointer alone; the
//! bridge maps routes and outcomes to runner decisions.

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
use std::sync::atomic::{AtomicBool, Ordering};

use durable::*;
use outbox_delivery::runner::{ClaimPermit, DeliveryContext, DeliveryHandler};
use outbox_delivery::{DeliveryDecision, DeliveryEnvelope, DeliveryFuture, ErrorCode, FenceResult};
use search_application::SearchError;
use search_application::indexing_service::{DocumentSourceEvent, IndexingOutcome};
use search_application::ports::{BoxFuture, SearchDeliveryFence, SearchSourceLease, SourceFence};
use search_source_document::{
    AuthoritativeItemBinding, BodyItemExtractor, DocumentSearchDeliveryHandler,
    ExtractedItemResult, VersionSnapshotRecord,
};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

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
         VALUES ($1,$2,'Document',$3,'{\"secret\":\"never-forwarded\"}',$4,clock_timestamp(), \
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

async fn scalar(pool: &PgPool, statement: &'static str) -> i64 {
    sqlx::query_scalar(statement).fetch_one(pool).await.unwrap()
}

async fn current(pool: &PgPool) -> Option<Uuid> {
    sqlx::query_scalar("SELECT current_generation_id FROM search_source_coordination")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn fenced_published_unchanged_and_duplicate_use_atomic_completion_only() {
    let durable = Durable::start().await;
    let document = publish(&durable.pool, &durable.storage, "東京の規程本文").await;
    let indexer = durable.indexer_with(durable.extractor(), true);
    let live = || Arc::new(AtomicBool::new(false));

    let (first, first_fence) = deliver(&durable.pool, durable.source_id, document, 1).await;
    let key = match indexer
        .handle_delivery(first.clone(), first_fence, live())
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected a fenced publication: {other:?}"),
    };
    assert_eq!(
        current(&durable.pool).await,
        Some(key.generation_id.as_uuid())
    );
    // The Search receipt and the pointer committed; the generic row is not acked.
    assert_eq!(
        scalar(&durable.pool, "SELECT count(*) FROM search_index_receipts").await,
        1
    );
    assert_eq!(
        scalar(
            &durable.pool,
            "SELECT count(*) FROM outbox_events WHERE delivered_at IS NOT NULL"
        )
        .await,
        0
    );

    // A later event on the same snapshot re-validates and reuses the current bundle.
    let (second, second_fence) = deliver(&durable.pool, durable.source_id, document, 1).await;
    assert_eq!(
        indexer
            .handle_delivery(second, second_fence, live())
            .await
            .unwrap(),
        IndexingOutcome::Unchanged(key)
    );
    // Redelivery of the first event is a duplicate, never a second publish.
    assert_eq!(
        indexer
            .handle_delivery(first, first_fence, live())
            .await
            .unwrap(),
        IndexingOutcome::Duplicate(key)
    );
    assert_eq!(
        scalar(&durable.pool, "SELECT count(*) FROM search_index_receipts").await,
        2
    );
    assert_eq!(
        scalar(&durable.pool, "SELECT count(*) FROM search_generation").await,
        1
    );
}

/// Sets the runner's cancellation flag while the body is being extracted.
struct Cancelling {
    inner: Arc<dyn BodyItemExtractor>,
    cancel: Arc<AtomicBool>,
}

impl BodyItemExtractor for Cancelling {
    fn parser_build_id(&self) -> &str {
        self.inner.parser_build_id()
    }

    fn extract<'a>(
        &'a self,
        record: &'a VersionSnapshotRecord,
        item: &'a AuthoritativeItemBinding,
    ) -> BoxFuture<'a, ExtractedItemResult> {
        self.cancel.store(true, Ordering::Release);
        self.inner.extract(record, item)
    }
}

#[tokio::test]
async fn cancelled_build_or_lost_fence_never_publishes_or_deletes_current() {
    let durable = Durable::start().await;
    let first = publish(&durable.pool, &durable.storage, "東京の規程本文").await;
    let indexer = durable.indexer_with(durable.extractor(), true);
    let (event, fence) = deliver(&durable.pool, durable.source_id, first, 1).await;
    let key = match indexer
        .handle_delivery(event, fence, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected a fenced publication: {other:?}"),
    };

    // The Source changes; the build is cancelled midway.
    let second = publish(&durable.pool, &durable.storage, "大阪の規程本文").await;
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelling = durable.indexer_with(
        Arc::new(Cancelling {
            inner: durable.extractor(),
            cancel: cancel.clone(),
        }),
        true,
    );
    let (event, fence) = deliver(&durable.pool, durable.source_id, second, 2).await;
    assert!(matches!(
        cancelling.handle_delivery(event, fence, cancel).await,
        Err(SearchError::FenceLost)
    ));
    assert_eq!(
        current(&durable.pool).await,
        Some(key.generation_id.as_uuid())
    );
    assert_eq!(
        scalar(&durable.pool, "SELECT count(*) FROM search_generation").await,
        1
    );

    // A Source lease that moved on (lost fence): no publication, no receipt.
    let (event, mut fence) = deliver(&durable.pool, durable.source_id, second, 3).await;
    fence.source.epoch = 2;
    let receipts = scalar(&durable.pool, "SELECT count(*) FROM search_index_receipts").await;
    assert!(matches!(
        indexer
            .handle_delivery(event, fence, Arc::new(AtomicBool::new(false)))
            .await,
        Err(SearchError::FenceLost)
    ));
    assert_eq!(
        current(&durable.pool).await,
        Some(key.generation_id.as_uuid())
    );
    assert_eq!(
        scalar(&durable.pool, "SELECT count(*) FROM search_index_receipts").await,
        receipts
    );
    assert_eq!(
        scalar(&durable.pool, "SELECT count(*) FROM search_generation").await,
        1
    );
}

#[derive(Clone)]
struct Permit(SourceFence);

impl ClaimPermit for Permit {
    fn preflight(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async { Ok(FenceResult::Updated) })
    }
    fn renew(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async { Ok(FenceResult::Updated) })
    }
    fn release(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async { Ok(FenceResult::Updated) })
    }
}

impl SearchSourceLease for Permit {
    fn fence(&self) -> SourceFence {
        self.0
    }
}

fn envelope(
    event: &DocumentSourceEvent,
    event_type: &str,
    aggregate_type: &str,
) -> DeliveryEnvelope {
    DeliveryEnvelope {
        event_id: event.event_id,
        event_type: event_type.into(),
        aggregate_type: aggregate_type.into(),
        aggregate_id: event.aggregate_id,
        payload: serde_json::json!({"secret": "never-forwarded"}),
        occurred_at: event.occurred_at,
    }
}

fn context(fence: SearchDeliveryFence) -> DeliveryContext {
    DeliveryContext {
        attempt: 1,
        outbox_token: fence.outbox_token,
        outbox_deadline: OffsetDateTime::now_utc() + time::Duration::hours(1),
        cancel: Arc::new(AtomicBool::new(false)),
        trace: None,
    }
}

#[tokio::test]
async fn bridge_routes_document_folder_policy_and_leaves_failures_unacked() {
    let durable = Durable::start().await;
    let document = publish(&durable.pool, &durable.storage, "東京の規程本文").await;
    let handler = DocumentSearchDeliveryHandler::new(
        durable.indexer_with(durable.extractor(), true),
        durable.source_id,
    );

    let (event, fence) = deliver(&durable.pool, durable.source_id, document, 1).await;
    let applied = handler
        .deliver(
            envelope(&event, "DocumentVersionPublished", "Document"),
            context(fence),
            Permit(fence.source),
        )
        .await;
    assert_eq!(applied, DeliveryDecision::Applied);

    // Known event, wrong aggregate: invalid. Unknown event: unsupported.
    let (other, other_fence) = deliver(&durable.pool, durable.source_id, document, 1).await;
    for (event_type, aggregate, expected) in [
        (
            "DocumentVersionPublished",
            "Folder",
            ErrorCode::InvalidEnvelope,
        ),
        ("DocumentArchived", "Document", ErrorCode::UnsupportedEvent),
    ] {
        assert_eq!(
            handler
                .deliver(
                    envelope(&other, event_type, aggregate),
                    context(other_fence),
                    Permit(other_fence.source),
                )
                .await,
            DeliveryDecision::Terminal(expected)
        );
    }
    // A Folder event on the same snapshot reuses the current bundle.
    assert_eq!(
        handler
            .deliver(
                envelope(&other, "FolderMoved", "Folder"),
                context(other_fence),
                Permit(other_fence.source),
            )
            .await,
        DeliveryDecision::Applied
    );

    // A stale Source lease is a retryable failure and writes nothing.
    let (late, late_fence) = deliver(&durable.pool, durable.source_id, document, 2).await;
    let receipts = scalar(&durable.pool, "SELECT count(*) FROM search_index_receipts").await;
    let stale = SourceFence {
        epoch: 1,
        ..late_fence.source
    };
    assert_eq!(
        handler
            .deliver(
                envelope(&late, "DocumentVersionPublished", "Document"),
                context(late_fence),
                Permit(stale),
            )
            .await,
        DeliveryDecision::Retryable(ErrorCode::SourceUnavailable)
    );
    assert_eq!(
        scalar(&durable.pool, "SELECT count(*) FROM search_index_receipts").await,
        receipts
    );
    assert_eq!(
        scalar(
            &durable.pool,
            "SELECT count(*) FROM outbox_events WHERE delivered_at IS NOT NULL"
        )
        .await,
        0
    );
}
