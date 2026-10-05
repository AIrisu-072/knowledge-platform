//! Shared outbox-indexer harness for body-ready generations (P1-B03/A03/A04).
#![allow(dead_code, unused_imports)]

pub use std::collections::BTreeMap;
pub use std::io::Cursor;
pub use std::sync::{Arc, Mutex};

pub use super::body_support::{InProcessExtractor, Mode, registry};
pub use document_application::{
    ContentReader, FileStorage, StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
};
pub use document_domain::{
    DocumentId, DocumentVersionId, FileId, FolderId, LifecycleState, StorageKey, Title,
};
pub use search_application::indexing_service::{
    DocumentIndexingService, DocumentSourceEvent, IndexingOutcome,
};
pub use search_application::ports::{BoxFuture, SemanticRegistrySnapshot};
pub use search_core::id::SourceId;
pub use search_core::knowledge_unit::{ContentPartRef, RawBinding};
pub use search_core::profile::DiscoveryLens;
pub use search_core::resource::ResourceKind;
pub use search_core::source::{
    DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode,
};
pub use search_source_document::{
    AuthoritativeItemBinding, DocumentAccessProjectionInput, DocumentBodyExtractor,
    DocumentIndexRuntime, DocumentIndexingConfig, DocumentOutboxIndexer, DocumentOutboxReader,
    DocumentOutboxSnapshot, DocumentSourceSnapshot, DsiReadState, IndexingReceipt,
    IndexingReceiptStore, MemoryDocumentIndexRuntime, PermittedDocumentMetadata,
    VersionSnapshotRecord,
};
pub use sha2::{Digest, Sha256};
pub use time::OffsetDateTime;
pub use uuid::Uuid;

pub const SOURCE: u128 = 70;

pub fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(SOURCE))
}

/// Raw bytes by storage key; tests rewrite them to model Source changes.
#[derive(Clone, Default)]
pub struct KeyedStorage(pub Arc<Mutex<BTreeMap<String, Vec<u8>>>>);

impl KeyedStorage {
    pub fn put(&self, key: &str, bytes: &[u8]) {
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
pub struct QueuedReader(pub Arc<Mutex<Vec<DocumentOutboxSnapshot>>>);

impl QueuedReader {
    pub fn new(snapshot: DocumentOutboxSnapshot) -> Self {
        Self(Arc::new(Mutex::new(vec![snapshot])))
    }

    pub fn replace(&self, snapshots: Vec<DocumentOutboxSnapshot>) {
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
pub struct MemoryReceipts(pub Arc<Mutex<BTreeMap<Uuid, IndexingReceipt>>>);

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

pub fn config() -> DocumentIndexingConfig {
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

pub fn binding(
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

pub fn snapshot(token: &str, items: Vec<AuthoritativeItemBinding>) -> DocumentOutboxSnapshot {
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

pub fn event(id: u128) -> DocumentSourceEvent {
    DocumentSourceEvent {
        event_id: Uuid::from_u128(id),
        event_type: "DocumentVersionPublished".into(),
        aggregate_id: Uuid::from_u128(10),
        occurred_at: OffsetDateTime::from_unix_timestamp(300).unwrap(),
    }
}

pub struct Harness {
    pub storage: KeyedStorage,
    pub reader: QueuedReader,
    pub runtime: MemoryDocumentIndexRuntime,
    pub service: DocumentIndexingService<DocumentOutboxIndexer<QueuedReader, MemoryReceipts>>,
}

pub fn harness(initial: DocumentOutboxSnapshot, storage: KeyedStorage, body: bool) -> Harness {
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

pub fn with_body(harness: Harness) -> Harness {
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

pub fn two_items(storage: &KeyedStorage, text: &str) -> Vec<AuthoritativeItemBinding> {
    let first = format!("{text}\n").into_bytes();
    let second = "同文。,x\n".as_bytes().to_vec();
    storage.put("objects/a", &first);
    storage.put("objects/b", &second);
    vec![
        binding("objects/a", &first, "text/plain", 0, 100),
        binding("objects/b", &second, "text/csv", 1, 200),
    ]
}

pub async fn published(
    harness: &Harness,
    id: u128,
) -> search_core::projection::ProjectionGenerationKey {
    match harness.service.handle(event(id)).await.unwrap() {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected publication, got {other:?}"),
    }
}

/// Reads one Version from the reader's current snapshot, as the Source would.
impl search_source_document::CurrentVersionReader for QueuedReader {
    fn load_version<'a>(
        &'a self,
        version: DocumentVersionId,
    ) -> BoxFuture<'a, Option<VersionSnapshotRecord>> {
        Box::pin(async move {
            let queue = self.0.lock().unwrap();
            Ok(queue[0]
                .live
                .iter()
                .chain(&queue[0].historical)
                .find(|record| record.snapshot.document_version_id == version)
                .cloned())
        })
    }
}
