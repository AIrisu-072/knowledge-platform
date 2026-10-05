//! P1-B02/B03: body-ready generations through the outbox indexer — same-key
//! bundle sealing, unchanged detection, body-only rebuild and the pre-publication
//! Source re-read.

#[path = "support/body.rs"]
mod body_support;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use body_support::{InProcessExtractor, Mode, registry};
use document_application::{
    ContentReader, FileStorage, StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
};
use document_domain::{
    DocumentId, DocumentVersionId, FileId, FolderId, LifecycleState, StorageKey, Title,
};
use search_application::indexing_service::{
    DocumentIndexingService, DocumentSourceEvent, IndexingOutcome,
};
use search_application::ports::{BoxFuture, SemanticRegistrySnapshot};
use search_core::id::SourceId;
use search_core::knowledge_unit::{ContentPartRef, RawBinding};
use search_core::profile::DiscoveryLens;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_source_document::{
    AuthoritativeItemBinding, DocumentAccessProjectionInput, DocumentBodyExtractor,
    DocumentIndexRuntime, DocumentIndexingConfig, DocumentOutboxIndexer, DocumentOutboxReader,
    DocumentOutboxSnapshot, DocumentSourceSnapshot, DsiReadState, IndexingReceipt,
    IndexingReceiptStore, MemoryDocumentIndexRuntime, PermittedDocumentMetadata,
    VersionSnapshotRecord,
};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

const SOURCE: u128 = 70;

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(SOURCE))
}

/// Raw bytes by storage key; tests rewrite them to model Source changes.
#[derive(Clone, Default)]
struct KeyedStorage(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);

impl KeyedStorage {
    fn put(&self, key: &str, bytes: &[u8]) {
        self.0.lock().unwrap().insert(key.into(), bytes.to_vec());
    }
}

impl FileStorage for KeyedStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        unreachable!("read-only fixture")
    }

    async fn open(&self, key: &StorageKey) -> Result<ContentReader, StorageError> {
        let bytes = self
            .0
            .lock()
            .unwrap()
            .get(key.as_str())
            .cloned()
            .ok_or(StorageError::Unavailable)?;
        Ok(Box::pin(Cursor::new(bytes)))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

/// Returns queued snapshots one per enumeration, then repeats the last.
#[derive(Clone)]
struct QueuedReader(Arc<Mutex<Vec<DocumentOutboxSnapshot>>>);

impl QueuedReader {
    fn new(snapshot: DocumentOutboxSnapshot) -> Self {
        Self(Arc::new(Mutex::new(vec![snapshot])))
    }

    fn replace(&self, snapshots: Vec<DocumentOutboxSnapshot>) {
        *self.0.lock().unwrap() = snapshots;
    }
}

impl DocumentOutboxReader for QueuedReader {
    fn enumerate_snapshot<'a>(&'a self) -> BoxFuture<'a, DocumentOutboxSnapshot> {
        Box::pin(async move {
            let mut queue = self.0.lock().unwrap();
            let next = if queue.len() > 1 {
                queue.remove(0)
            } else {
                queue[0].clone()
            };
            Ok(next)
        })
    }
}

#[derive(Default, Clone)]
struct MemoryReceipts(Arc<Mutex<BTreeMap<Uuid, IndexingReceipt>>>);

impl IndexingReceiptStore for MemoryReceipts {
    fn get<'a>(&'a self, event_id: Uuid) -> BoxFuture<'a, Option<IndexingReceipt>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().get(&event_id).cloned()) })
    }

    fn put<'a>(&'a self, event_id: Uuid, receipt: IndexingReceipt) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().insert(event_id, receipt);
            Ok(())
        })
    }
}

fn config() -> DocumentIndexingConfig {
    let mut source = DiscoverableSource::new(
        source_id(),
        "document-platform",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentResource,
    );
    source.resource_types.push(ResourceKind::Knowledge);
    source.discovery_modes.push(DiscoveryMode::LocalDirectory);
    source
        .discovery_modes
        .push(DiscoveryMode::LocalContentSearch);
    source.access_model = Some("document-current-check".into());
    DocumentIndexingConfig {
        source,
        lens: DiscoveryLens {
            lens_id: "document-version".into(),
            lens_version: 1,
            resource_type: ResourceKind::Knowledge,
            domain_scope: None,
            source_scope: Some(source_id()),
            identity_fields: vec!["document_version_id".into()],
            high_signal_facets: vec!["document_type".into()],
            searchable_fields: vec!["title".into(), "permitted_metadata".into()],
            applicability_fields: vec![],
            temporal_fields: vec![],
            relation_fields: vec![],
            extraction_policy: None,
            projection_policy: None,
        },
        projection_schema_version: "schema-1".into(),
        analyzer_version: "tantivy-default-0.26.2".into(),
        semantic_registry: SemanticRegistrySnapshot::new("registry-1"),
    }
}

fn binding(
    key: &str,
    raw: &[u8],
    media: &str,
    ordinal: u32,
    seed: u128,
) -> AuthoritativeItemBinding {
    let content_item_id = Uuid::from_u128(seed);
    AuthoritativeItemBinding {
        content_item_id,
        part: ContentPartRef {
            source_native_part_id: content_item_id.to_string(),
            logical_path: format!("本文/part-{ordinal}"),
            ordinal,
        },
        representation_id: Uuid::from_u128(seed + 1),
        file_id: FileId::from_uuid(Uuid::from_u128(seed + 2)),
        raw: RawBinding {
            sha256: Sha256::digest(raw).into(),
            size_bytes: raw.len() as u64,
            media_type: media.into(),
        },
        storage_key: StorageKey::new(key).unwrap(),
    }
}

fn snapshot(token: &str, items: Vec<AuthoritativeItemBinding>) -> DocumentOutboxSnapshot {
    let at = |second| OffsetDateTime::from_unix_timestamp(second).unwrap();
    let version = DocumentVersionId::from_uuid(Uuid::from_u128(20));
    let record = VersionSnapshotRecord {
        snapshot: DocumentSourceSnapshot {
            source_snapshot: token.into(),
            document_id: DocumentId::from_uuid(Uuid::from_u128(10)),
            document_version_id: version,
            current_version_id: Some(version),
            publication_end: None,
            lifecycle_state: LifecycleState::Published,
            title: Title::new("規程").unwrap(),
            metadata: PermittedDocumentMetadata {
                document_type: Some("policy".into()),
                category: None,
            },
            folder_id: FolderId::from_uuid(Uuid::from_u128(30)),
            created_at: at(100),
            published_at: Some(at(200)),
            withdrawn_at: None,
            effective_from: None,
            effective_to: None,
            access: DocumentAccessProjectionInput {
                access_scope: Some("scope".into()),
            },
            dsi: None,
        },
        document_revision: 2,
        access_revision: 1,
        dsi_state: DsiReadState::UnknownMissing,
        authoritative_items: items,
    };
    DocumentOutboxSnapshot {
        source_snapshot: token.into(),
        live: vec![record],
        historical: Vec::new(),
    }
}

fn event(id: u128) -> DocumentSourceEvent {
    DocumentSourceEvent {
        event_id: Uuid::from_u128(id),
        event_type: "DocumentVersionPublished".into(),
        aggregate_id: Uuid::from_u128(10),
        occurred_at: OffsetDateTime::from_unix_timestamp(300).unwrap(),
    }
}

struct Harness {
    storage: KeyedStorage,
    reader: QueuedReader,
    runtime: MemoryDocumentIndexRuntime,
    service: DocumentIndexingService<DocumentOutboxIndexer<QueuedReader, MemoryReceipts>>,
}

fn harness(initial: DocumentOutboxSnapshot, storage: KeyedStorage, body: bool) -> Harness {
    let reader = QueuedReader::new(initial);
    let runtime = MemoryDocumentIndexRuntime::new();
    let mut indexer = DocumentOutboxIndexer::new(
        reader.clone(),
        config(),
        runtime.clone(),
        MemoryReceipts::default(),
    );
    if body {
        indexer = indexer.with_body_extractor(Arc::new(DocumentBodyExtractor::new(
            source_id(),
            storage.clone(),
            InProcessExtractor::new(Mode::Honest),
            registry(),
        )));
    }
    Harness {
        storage,
        reader,
        runtime,
        service: DocumentIndexingService::new(indexer),
    }
}

fn with_body(harness: Harness) -> Harness {
    let storage = harness.storage.clone();
    let reader = harness.reader.clone();
    let runtime = harness.runtime.clone();
    let indexer = DocumentOutboxIndexer::new(
        reader.clone(),
        config(),
        runtime.clone(),
        MemoryReceipts::default(),
    )
    .with_body_extractor(Arc::new(DocumentBodyExtractor::new(
        source_id(),
        storage.clone(),
        InProcessExtractor::new(Mode::Honest),
        registry(),
    )));
    Harness {
        storage,
        reader,
        runtime,
        service: DocumentIndexingService::new(indexer),
    }
}

fn two_items(storage: &KeyedStorage, text: &str) -> Vec<AuthoritativeItemBinding> {
    let first = format!("{text}\n").into_bytes();
    let second = "同文。,x\n".as_bytes().to_vec();
    storage.put("objects/a", &first);
    storage.put("objects/b", &second);
    vec![
        binding("objects/a", &first, "text/plain", 0, 100),
        binding("objects/b", &second, "text/csv", 1, 200),
    ]
}

async fn published(
    harness: &Harness,
    id: u128,
) -> search_core::projection::ProjectionGenerationKey {
    match harness.service.handle(event(id)).await.unwrap() {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected publication, got {other:?}"),
    }
}

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
