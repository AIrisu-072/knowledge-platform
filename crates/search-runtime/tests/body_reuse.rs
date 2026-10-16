//! A delivery reads only items whose version, part, representation or raw
//! bytes changed; unchanged items keep the published Units of the current
//! bundle. A manual rebuild still reads every item.

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

use durable::*;
use search_application::indexing_service::IndexingOutcome;
use search_source_document::DocumentBodyExtractor;

#[tokio::test]
async fn unchanged_items_are_not_extracted_again() {
    let durable = Durable::start().await;
    let body = Arc::new(DocumentBodyExtractor::new(
        durable.source_id,
        durable.storage.clone(),
        body_support::InProcessExtractor::new(body_support::Mode::Honest),
        body_support::registry(),
    ));
    let indexer = durable.indexer_with(body.clone(), false);
    let tokyo = publish(&durable.pool, &durable.storage, "東京 規程 本文").await;
    let first = indexer
        .handle(discovery_support::event("DocumentVersionPublished", tokyo))
        .await
        .unwrap();
    assert!(matches!(first, IndexingOutcome::Published(_)), "{first:?}");
    // Worker requests per read item (extraction and its locator check).
    let per_item = body.extractor().calls();
    assert!(per_item >= 1);

    // A second Document: only its item is read; Tokyo's Units are reused.
    let osaka = publish(&durable.pool, &durable.storage, "大阪 規程 本文").await;
    let second = indexer
        .handle(discovery_support::event("DocumentVersionPublished", osaka))
        .await
        .unwrap();
    assert!(
        matches!(second, IndexingOutcome::Published(_)),
        "{second:?}"
    );
    assert_eq!(body.extractor().calls(), 2 * per_item);

    // Tokyo stood for its Units by its stored segment: the new generation
    // binds that segment, which still holds Tokyo's text.
    let current_payloads = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT u.payload FROM search_source_coordination c \
             JOIN search_generation_segment g \
               ON g.source_id = c.source_id AND g.generation_id = c.current_generation_id \
             JOIN search_unit_segment u ON u.segment_digest = g.segment_digest \
             WHERE c.source_id = $1 ORDER BY g.ordinal",
        )
        .bind(durable.source_id.as_uuid())
        .fetch_all(&durable.pool)
        .await
        .unwrap()
    };
    let payloads = current_payloads().await;
    assert_eq!(payloads.len(), 2);
    assert!(payloads.iter().any(|payload| payload.contains("東京")));
    assert!(payloads.iter().any(|payload| payload.contains("大阪")));

    // A third Document: both earlier items stand by their stored segments.
    let nagoya = publish(&durable.pool, &durable.storage, "名古屋 規程 本文").await;
    let third = indexer
        .handle(discovery_support::event("DocumentVersionPublished", nagoya))
        .await
        .unwrap();
    assert!(matches!(third, IndexingOutcome::Published(_)), "{third:?}");
    assert_eq!(body.extractor().calls(), 3 * per_item);
    assert_eq!(current_payloads().await.len(), 3);

    // Nothing changed: the bundle is unchanged and nothing is read.
    let again = indexer
        .handle(discovery_support::event("DocumentVersionPublished", nagoya))
        .await
        .unwrap();
    assert!(
        matches!(
            again,
            IndexingOutcome::Unchanged(_) | IndexingOutcome::Duplicate(_)
        ),
        "{again:?}"
    );
    assert_eq!(body.extractor().calls(), 3 * per_item);

    // The manual rebuild reads every item again.
    let rebuilt = indexer.rebuild().await.unwrap();
    assert!(
        matches!(
            rebuilt,
            IndexingOutcome::Published(_) | IndexingOutcome::Unchanged(_)
        ),
        "{rebuilt:?}"
    );
    assert_eq!(body.extractor().calls(), 6 * per_item);
}
