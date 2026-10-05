//! P1-B02/B03: body-ready generations through the outbox indexer — same-key
//! bundle sealing, unchanged detection, body-only rebuild and the pre-publication
//! Source re-read.

#[path = "support/body_index.rs"]
mod body_index;
#[path = "support/body.rs"]
mod body_support;

use body_index::*;

#[tokio::test]
async fn body_ready_generation_publishes_one_sealed_bundle() {
    let storage = KeyedStorage::default();
    let items = two_items(&storage, "東京");
    let harness = harness(snapshot("s1", items), storage, true);
    let key = published(&harness, 1).await;
    let (manifest, receipt) = harness
        .runtime
        .pin_current_bundle(source_id())
        .await
        .unwrap()
        .expect("body-ready current generation");
    assert_eq!(manifest.key(), key);
    assert_eq!(receipt.key, key);
    assert_eq!(receipt.unit_manifest.count, 3);
    assert_eq!(receipt.body_coverage.count, 2);
    assert_eq!(receipt.lexical_schema_version, "schema-2");
}

#[tokio::test]
async fn unchanged_body_is_a_noop_and_body_only_change_rebuilds() {
    let storage = KeyedStorage::default();
    let items = two_items(&storage, "東京");
    let harness = harness(snapshot("s1", items), storage.clone(), true);
    let first = published(&harness, 1).await;
    assert_eq!(
        harness.service.handle(event(2)).await.unwrap(),
        IndexingOutcome::Unchanged(first)
    );
    let (_, before) = harness
        .runtime
        .pin_current_bundle(source_id())
        .await
        .unwrap()
        .unwrap();
    // Same title and metadata (projection digest unchanged), new body bytes.
    let changed = two_items(&storage, "大阪");
    harness.reader.replace(vec![snapshot("s2", changed)]);
    let second = published(&harness, 3).await;
    assert_ne!(first, second);
    let (pinned, after) = harness
        .runtime
        .pin_current_bundle(source_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pinned.key(), second);
    assert_eq!(before.projection_digest, after.projection_digest);
    assert_ne!(before.composite_digest, after.composite_digest);
}

#[tokio::test]
async fn projection_only_generation_is_rebuilt_as_body_ready() {
    let storage = KeyedStorage::default();
    let items = two_items(&storage, "東京");
    let legacy = harness(snapshot("s1", items), storage, false);
    let first = published(&legacy, 1).await;
    assert!(
        legacy
            .runtime
            .pin_current_bundle(source_id())
            .await
            .unwrap()
            .is_none()
    );
    let upgraded = with_body(legacy);
    let second = published(&upgraded, 2).await;
    assert_ne!(first, second);
    assert!(
        upgraded
            .runtime
            .pin_current_bundle(source_id())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn retryable_raw_read_keeps_the_old_pointer() {
    let storage = KeyedStorage::default();
    let items = two_items(&storage, "東京");
    let harness = harness(snapshot("s1", items), storage.clone(), true);
    let first = published(&harness, 1).await;
    // The new snapshot binds a raw object the storage cannot serve right now.
    let mut changed = two_items(&storage, "東京");
    let missing = b"not stored\n";
    changed[0] = binding("objects/missing", missing, "text/plain", 0, 100);
    harness.reader.replace(vec![snapshot("s2", changed)]);
    assert!(harness.service.handle(event(2)).await.is_err());
    let (pinned, _) = harness
        .runtime
        .pin_current_bundle(source_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pinned.key(), first);
}

#[tokio::test]
async fn binding_change_before_publication_discards_and_rebuilds() {
    let storage = KeyedStorage::default();
    let original = two_items(&storage, "東京");
    let harness = harness(snapshot("s1", original.clone()), storage.clone(), true);
    // The re-read right before publication observes a different item binding.
    let changed_bytes = "名古屋\n".as_bytes();
    let mut changed = original.clone();
    changed[0] = binding("objects/a2", changed_bytes, "text/plain", 0, 100);
    storage.put("objects/a2", changed_bytes);
    harness.reader.replace(vec![
        snapshot("s1", original.clone()),
        snapshot("s2", changed.clone()),
        snapshot("s2", changed.clone()),
    ]);
    let key = published(&harness, 1).await;
    let (pinned, receipt) = harness
        .runtime
        .pin_current_bundle(source_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pinned.key(), key);
    assert_eq!(pinned.source_snapshot, "s2");
    assert_eq!(receipt.source_snapshot, "s2");
}
