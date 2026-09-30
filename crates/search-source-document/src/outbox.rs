//! Rebuildable Document indexing from authoritative read-side snapshots.
//! This adapter never acknowledges or edits the generic Document outbox.

use std::collections::BTreeSet;
use std::sync::Arc;

use search_application::SearchError;
use search_application::indexing_service::{
    DocumentIndexingPort, DocumentSourceEvent, IndexingOutcome,
};
use search_application::ports::{
    BoxFuture, LexicalQuery, LexicalRetrieverPort, ProjectionGenerationStore,
    SemanticRegistrySnapshot,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection, ProjectionCompiler,
};
use search_core::discovery::{DiscoveryRequest, FederatedCandidate};
use search_core::id::{ProjectionGenerationId, ResourceId, SourceId};
use search_core::observation::Coverage;
use search_core::profile::DiscoveryLens;
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::source::DiscoverableSource;
use search_projection_memory::{MemoryProjectionStore, generation_digest};
use search_tantivy::{LexicalBuildInput, LexicalIndexError, TantivyLexicalIndex};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::postgres::{DocumentOutboxSnapshot, PostgresDocumentSnapshotReader};
use crate::translate::DocumentSourceTranslator;

/// Deliberately separate from `outbox_events.delivered_at`: a durable Search
/// receipt implementation can be supplied without claiming the delivery worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexingReceipt {
    pub digest: String,
    pub generation: ProjectionGenerationKey,
}

pub trait IndexingReceiptStore: Send + Sync {
    fn get<'a>(&'a self, event_id: Uuid) -> BoxFuture<'a, Option<IndexingReceipt>>;
    fn put<'a>(&'a self, event_id: Uuid, receipt: IndexingReceipt) -> BoxFuture<'a, ()>;
}

/// One call must return both tiers and an empty-state token from the same
/// authoritative Source snapshot.
pub trait DocumentOutboxReader: Send + Sync {
    fn enumerate_snapshot<'a>(&'a self) -> BoxFuture<'a, DocumentOutboxSnapshot>;
}

impl DocumentOutboxReader for PostgresDocumentSnapshotReader {
    fn enumerate_snapshot<'a>(&'a self) -> BoxFuture<'a, DocumentOutboxSnapshot> {
        Box::pin(async move {
            self.enumerate_outbox_snapshot()
                .await
                .map_err(|error| SearchError::SourceUnavailable(error.to_string()))
        })
    }
}

/// A runtime owns both the projection pointer and the lexical generations
/// reachable through it. Clones must share both parts. Conditional publication
/// must compare and switch the Source pointer atomically across all indexers;
/// a rejected comparison or error must leave that pointer unchanged.
pub trait DocumentIndexRuntime: Send + Sync {
    fn pin_current<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>>;
    fn begin_generation<'a>(&'a self, manifest: PersistableGenerationManifest)
    -> BoxFuture<'a, ()>;
    fn stage_concept_registry<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        registry: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()>;
    fn stage_resource<'a>(&'a self, resource: PersistableResourceProjection) -> BoxFuture<'a, ()>;
    fn validate_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()>;
    fn publish_if_current<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        expected_current: Option<ProjectionGenerationKey>,
    ) -> BoxFuture<'a, bool>;
    fn fail_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()>;
    fn build_lexical_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        input: LexicalBuildInput,
    ) -> Result<(), LexicalIndexError>;
    fn discard_lexical_generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<bool, LexicalIndexError>;
}

/// The in-process Document index is one clonable unit. No constructor accepts
/// an existing projection store and an unrelated lexical index.
#[derive(Clone, Default)]
pub struct MemoryDocumentIndexRuntime {
    store: MemoryProjectionStore,
    lexical: Arc<TantivyLexicalIndex>,
}

impl MemoryDocumentIndexRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn projection_reader(&self) -> DocumentProjectionReader {
        DocumentProjectionReader(self.store.clone())
    }

    pub fn lexical_reader(&self) -> DocumentLexicalReader {
        DocumentLexicalReader(self.lexical.clone())
    }

    /// Drop a rebuildable lexical segment after detected loss/corruption; the
    /// caller must reconcile the Source before serving that generation again.
    pub fn discard_lexical_generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<bool, LexicalIndexError> {
        self.lexical.discard_generation(key)
    }
}

#[derive(Clone)]
pub struct DocumentProjectionReader(MemoryProjectionStore);

impl DocumentProjectionReader {
    pub async fn pin_current(
        &self,
        source_id: SourceId,
    ) -> Result<Option<ProjectionGenerationManifest>, SearchError> {
        ProjectionGenerationStore::pin_current(&self.0, source_id).await
    }

    pub async fn resource_at(
        &self,
        key: ProjectionGenerationKey,
        resource_id: ResourceId,
    ) -> Result<Option<CompiledResourceProjection>, SearchError> {
        ProjectionGenerationStore::resource_at(&self.0, key, resource_id).await
    }
}

fn read_only_projection<'a>() -> BoxFuture<'a, ()> {
    Box::pin(async {
        Err(SearchError::OperationFailed(
            "Document projection reader cannot mutate generations".into(),
        ))
    })
}

// D8 currently accepts ProjectionGenerationStore for generation pinning. This
// facade satisfies that query port while denying every mutation, so a caller
// cannot publish a projection without its runtime-owned lexical generation.
impl ProjectionGenerationStore for DocumentProjectionReader {
    fn begin_generation<'a>(
        &'a self,
        _manifest: PersistableGenerationManifest,
    ) -> BoxFuture<'a, ()> {
        read_only_projection()
    }

    fn begin_incremental_generation<'a>(
        &'a self,
        _manifest: PersistableGenerationManifest,
        _base: ProjectionGenerationKey,
        _retired: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()> {
        read_only_projection()
    }

    fn stage_resource<'a>(&'a self, _resource: PersistableResourceProjection) -> BoxFuture<'a, ()> {
        read_only_projection()
    }

    fn stage_concept_registry<'a>(
        &'a self,
        _key: ProjectionGenerationKey,
        _registry: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        read_only_projection()
    }

    fn validate_generation<'a>(&'a self, _key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        read_only_projection()
    }

    fn publish_generation<'a>(&'a self, _key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        read_only_projection()
    }

    fn fail_generation<'a>(&'a self, _key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        read_only_projection()
    }

    fn pin_current<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        ProjectionGenerationStore::pin_current(&self.0, source_id)
    }

    fn resource_at<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        resource_id: ResourceId,
    ) -> BoxFuture<'a, Option<CompiledResourceProjection>> {
        ProjectionGenerationStore::resource_at(&self.0, key, resource_id)
    }
}

#[derive(Clone)]
pub struct DocumentLexicalReader(Arc<TantivyLexicalIndex>);

impl LexicalRetrieverPort for DocumentLexicalReader {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        LexicalRetrieverPort::retrieve(self.0.as_ref(), generation, request, query)
    }
}

impl DocumentIndexRuntime for MemoryDocumentIndexRuntime {
    fn pin_current<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        ProjectionGenerationStore::pin_current(&self.store, source_id)
    }

    fn begin_generation<'a>(
        &'a self,
        manifest: PersistableGenerationManifest,
    ) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::begin_generation(&self.store, manifest)
    }

    fn stage_concept_registry<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        registry: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::stage_concept_registry(&self.store, key, registry)
    }

    fn stage_resource<'a>(&'a self, resource: PersistableResourceProjection) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::stage_resource(&self.store, resource)
    }

    fn validate_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::validate_generation(&self.store, key)
    }

    fn publish_if_current<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        expected_current: Option<ProjectionGenerationKey>,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.store
                .publish_generation_if_current(key, expected_current)
        })
    }

    fn fail_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::fail_generation(&self.store, key)
    }

    fn build_lexical_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        input: LexicalBuildInput,
    ) -> Result<(), LexicalIndexError> {
        self.lexical.build_generation(manifest, source, input)
    }

    fn discard_lexical_generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<bool, LexicalIndexError> {
        self.lexical.discard_generation(key)
    }
}

pub struct DocumentIndexingConfig {
    pub source: DiscoverableSource,
    pub lens: DiscoveryLens,
    pub projection_schema_version: String,
    pub analyzer_version: String,
    pub semantic_registry: SemanticRegistrySnapshot,
}

/// The full Source is re-read for every relevant trigger. This also covers
/// Folder and AccessPolicy events whose affected Documents are not in the
/// transport-neutral event envelope.
pub struct DocumentOutboxIndexer<R, E, T = MemoryDocumentIndexRuntime> {
    reader: R,
    config: DocumentIndexingConfig,
    runtime: T,
    receipts: E,
    gate: Mutex<()>,
}

impl<R, E, T: DocumentIndexRuntime> DocumentOutboxIndexer<R, E, T> {
    pub fn new(reader: R, config: DocumentIndexingConfig, runtime: T, receipts: E) -> Self {
        Self {
            reader,
            config,
            runtime,
            receipts,
            gate: Mutex::new(()),
        }
    }
}

impl<R: DocumentOutboxReader, E: IndexingReceiptStore, T: DocumentIndexRuntime>
    DocumentOutboxIndexer<R, E, T>
{
    async fn reconcile(
        &self,
        event_id: Option<Uuid>,
        full_rebuild: bool,
    ) -> Result<IndexingOutcome, SearchError> {
        // Serialize this instance. The shared store's conditional pointer
        // switch protects independent indexers using the same Source.
        let _guard = self.gate.lock().await;
        for _ in 0..3 {
            if let Some(outcome) = self.reconcile_once(event_id, full_rebuild).await? {
                return Ok(outcome);
            }
        }
        Err(SearchError::OperationFailed(
            "Document Source changed during three generation builds".into(),
        ))
    }

    async fn reconcile_once(
        &self,
        event_id: Option<Uuid>,
        full_rebuild: bool,
    ) -> Result<Option<IndexingOutcome>, SearchError> {
        let source_id = self.config.source.source_id;
        let expected_current = self
            .runtime
            .pin_current(source_id)
            .await?
            .map(|item| item.key());
        let snapshot = self.reader.enumerate_snapshot().await?;
        let source_snapshot = one_source_snapshot(&snapshot)?;
        let live_count = snapshot.live.len();
        let records = snapshot.live.into_iter().chain(snapshot.historical);
        let translator = DocumentSourceTranslator::new(
            self.config.source.clone(),
            self.config.lens.clone(),
            self.config.projection_schema_version.clone(),
            self.config.semantic_registry.version.clone(),
        );
        let mut manifest = ProjectionGenerationManifest {
            source_id,
            generation_id: ProjectionGenerationId::from_uuid(Uuid::now_v7()),
            projection_schema_version: self.config.projection_schema_version.clone(),
            lens_version: self.config.lens.lens_version,
            semantic_registry_version: self.config.semantic_registry.version.clone(),
            analyzer_version: Some(self.config.analyzer_version.clone()),
            embedding_model_version: None,
            graph_schema_version: None,
            source_snapshot: source_snapshot.clone(),
            resource_count: live_count as u64,
            relation_count: Some(0),
            coverage: Coverage::CompleteEnumeration,
            digest: String::new(),
            built_at: OffsetDateTime::now_utc(),
        };
        let mut projections = Vec::with_capacity(live_count);
        let mut lexical_documents = Vec::with_capacity(live_count);
        for record in records {
            let translation = translator.translate_record(record).map_err(|error| {
                SearchError::OperationFailed(format!("Document translation failed: {error}"))
            })?;
            let inputs = match translation {
                crate::translate::DocumentSourceTranslation::Live(inputs) => inputs,
                // Historical and authoring views require separate tiers; they
                // cannot enter the normal Live generation.
                crate::translate::DocumentSourceTranslation::Historical { .. }
                | crate::translate::DocumentSourceTranslation::Authoring { .. } => continue,
            };
            projections.push(
                ProjectionCompiler::compile_resource(&manifest, &inputs.projection).map_err(
                    |error| {
                        SearchError::OperationFailed(format!("Document projection failed: {error}"))
                    },
                )?,
            );
            lexical_documents.push(inputs.lexical_document);
        }
        manifest.relation_count = Some(
            projections
                .iter()
                .map(|projection| projection.relations.len() as u64)
                .sum(),
        );
        manifest.digest =
            generation_digest(source_id, &projections, &self.config.semantic_registry)?;
        let current = self.runtime.pin_current(source_id).await?;
        if let Some(current) = current.filter(|current| {
            !full_rebuild
                && current.digest == manifest.digest
                && current.projection_schema_version == manifest.projection_schema_version
                && current.lens_version == manifest.lens_version
                && current.semantic_registry_version == manifest.semantic_registry_version
                && current.analyzer_version == manifest.analyzer_version
                && current.embedding_model_version == manifest.embedding_model_version
                && current.graph_schema_version == manifest.graph_schema_version
                && current.coverage == manifest.coverage
        }) {
            let key = current.key();
            let duplicate = if let Some(event_id) = event_id {
                let previous = self.receipts.get(event_id).await?;
                let duplicate = previous.as_ref().is_some_and(|receipt| {
                    receipt.digest == manifest.digest && receipt.generation == key
                });
                if !duplicate {
                    self.receipts
                        .put(
                            event_id,
                            IndexingReceipt {
                                digest: manifest.digest,
                                generation: key,
                            },
                        )
                        .await?;
                }
                duplicate
            } else {
                false
            };
            return Ok(Some(if duplicate {
                IndexingOutcome::Duplicate(key)
            } else {
                IndexingOutcome::Unchanged(key)
            }));
        }
        for projection in &mut projections {
            projection.manifest = manifest.clone();
        }
        let key = manifest.key();
        let persistent =
            PersistableGenerationManifest::try_from((manifest.clone(), &self.config.source))
                .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
        self.runtime.begin_generation(persistent).await?;
        let staged = async {
            self.runtime
                .stage_concept_registry(key, self.config.semantic_registry.clone())
                .await?;
            for projection in projections {
                let persistent = PersistableResourceProjection::try_from(projection)
                    .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
                self.runtime.stage_resource(persistent).await?;
            }
            self.runtime.validate_generation(key).await
        }
        .await;
        if let Err(error) = staged {
            let _ = self.runtime.fail_generation(key).await;
            return Err(error);
        }
        let lexical_input = LexicalBuildInput::new(
            source_id,
            source_snapshot,
            manifest.projection_schema_version.clone(),
            manifest.lens_version,
            lexical_documents,
        );
        if let Err(error) = self.runtime.build_lexical_generation(
            manifest.clone(),
            &self.config.source,
            lexical_input,
        ) {
            let _ = self.runtime.fail_generation(key).await;
            return Err(SearchError::OperationFailed(format!(
                "Document lexical build failed: {error}"
            )));
        }
        let published = self.runtime.publish_if_current(key, expected_current).await;
        if !matches!(&published, Ok(true)) {
            let _ = self.runtime.fail_generation(key).await;
            self.runtime
                .discard_lexical_generation(key)
                .map_err(|error| {
                    SearchError::OperationFailed(format!(
                        "Document lexical rollback failed: {error}"
                    ))
                })?;
            match published {
                Ok(false) => return Ok(None),
                Err(error) => return Err(error),
                Ok(true) => unreachable!(),
            }
        }
        if let Some(event_id) = event_id {
            self.receipts
                .put(
                    event_id,
                    IndexingReceipt {
                        digest: manifest.digest,
                        generation: key,
                    },
                )
                .await?;
        }
        Ok(Some(IndexingOutcome::Published(key)))
    }
}

impl<R: DocumentOutboxReader, E: IndexingReceiptStore, T: DocumentIndexRuntime> DocumentIndexingPort
    for DocumentOutboxIndexer<R, E, T>
{
    fn refresh<'a>(&'a self, event: DocumentSourceEvent) -> BoxFuture<'a, IndexingOutcome> {
        Box::pin(async move { self.reconcile(Some(event.event_id), false).await })
    }

    fn rebuild<'a>(&'a self) -> BoxFuture<'a, IndexingOutcome> {
        Box::pin(async move { self.reconcile(None, true).await })
    }
}

fn one_source_snapshot(snapshot: &DocumentOutboxSnapshot) -> Result<String, SearchError> {
    if snapshot.source_snapshot.trim().is_empty()
        || snapshot
            .live
            .iter()
            .chain(&snapshot.historical)
            .any(|record| record.snapshot.source_snapshot != snapshot.source_snapshot)
    {
        return Err(SearchError::SourceUnavailable(
            "Document outbox enumeration crossed Source snapshots".into(),
        ));
    }
    Ok(snapshot.source_snapshot.clone())
}
