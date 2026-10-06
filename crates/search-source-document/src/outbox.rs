//! Rebuildable Document indexing from authoritative read-side snapshots.
//! This adapter never acknowledges or edits the generic Document outbox.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use document_domain::DocumentId;
use search_application::SearchError;
use search_application::body_ports::LexicalRetrievalBatch;
use search_application::indexing_service::{
    DocumentIndexingPort, DocumentSourceEvent, IndexingOutcome,
};
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, CompleteEventRequest, CompletionMode,
    ConceptRegistryPort, CurrentAccessEvaluatorPort, CurrentGenerationSnapshot,
    DirectoryRetrieverPort, FencedDocumentIndexingPort, GraphRetrievalResult,
    HyperGraphRetrieverPort, LexicalQuery, LexicalRetrieverPort, ProjectionGenerationStore,
    SearchCompletionOutcome, SearchDeliveryFence, SearchEventCompletionPort,
    SemanticRegistrySnapshot, StructuredFacetFilter, StructuredRetrievalHit,
    StructuredRetrieverPort,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection, ProjectionCompiler,
};
use search_core::assertion::Assertion;
use search_core::discovery::{DiscoveryRequest, FederatedCandidate};
use search_core::graph::GraphTraversalPlan;
use search_core::id::{ProjectionGenerationId, RelationId, ResourceId, SourceId};
use search_core::observation::Coverage;
use search_core::predicate::{ConceptResolver, TruthValue};
use search_core::profile::DiscoveryLens;
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::relation::TypedRelationInstance;
use search_core::resource::ResourceKind;
use search_core::source::DiscoverableSource;
use search_graph_memory::MemoryGraphRetriever;
use search_projection_memory::{MemoryProjectionStore, generation_digest};
use search_tantivy::{
    LexicalBuildInput, LexicalIndexError, TantivyLexicalIndex, lexical_input_digest,
};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::body_bundle::{BundleRegistry, PublishedBody, graph_receipt};
use crate::body_manifest::{
    ArtifactReceipt, BodyCoverageArtifact, BodyItemEntry, BodyUnitManifest,
    GenerationBundleReceipt, coverage_receipt, profile_set_digest, unit_manifest_receipt,
    validate_manifest,
};
use crate::evidence::{append_authoritative_assertions, expose_generation_locators};
use crate::extraction::BodyItemExtractor;
use crate::postgres::{
    DocumentCurrentAccessAdapter, DocumentOutboxSnapshot, PostgresDocumentSnapshotReader,
};
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
    fn discard_projection_generation<'a>(
        &'a self,
        key: ProjectionGenerationKey,
    ) -> BoxFuture<'a, bool>;
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
    fn build_graph_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        projections: Vec<CompiledResourceProjection>,
        ownership: Vec<(ResourceId, DocumentId)>,
    ) -> Result<(), SearchError>;
    fn discard_graph_generation(&self, key: ProjectionGenerationKey) -> Result<bool, SearchError>;

    /// Stage the body Unit manifest of a same-key bundle (P1-B02).
    fn stage_body_unit_manifest<'a>(&'a self, _manifest: BodyUnitManifest) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(body_bundles_unsupported()) })
    }

    fn stage_body_coverage<'a>(&'a self, _artifact: BodyCoverageArtifact) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(body_bundles_unsupported()) })
    }

    /// Recompute all receipts and seal the actual lexical Unit documents.
    fn validate_bundle<'a>(
        &'a self,
        _key: ProjectionGenerationKey,
        _projection_digest: String,
    ) -> BoxFuture<'a, GenerationBundleReceipt> {
        Box::pin(async { Err(body_bundles_unsupported()) })
    }

    /// The current projection generation together with its published bundle.
    /// `None` when the current generation is not body-ready.
    fn pin_current_bundle<'a>(
        &'a self,
        _source_id: SourceId,
    ) -> BoxFuture<'a, Option<(ProjectionGenerationManifest, GenerationBundleReceipt)>> {
        Box::pin(async { Ok(None) })
    }

    fn discard_body_generation<'a>(&'a self, _key: ProjectionGenerationKey) -> BoxFuture<'a, bool> {
        Box::pin(async { Ok(false) })
    }

    /// The authoritative snapshot behind a body-ready generation, for a
    /// durable runtime that commits the Graph mapping to it (P3-D01).
    fn bind_source_snapshot<'a>(
        &'a self,
        _key: ProjectionGenerationKey,
        _snapshot: &'a DocumentOutboxSnapshot,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    /// The delivery an event-origin build belongs to (P6-S04). A durable
    /// runtime registers the candidate as that event's EVENT target.
    fn bind_delivery<'a>(
        &'a self,
        _key: ProjectionGenerationKey,
        _event: &'a DocumentSourceEvent,
        _fence: SearchDeliveryFence,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    /// A fenced completion committed this key; its build state may go.
    fn settled<'a>(&'a self, _key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn body_bundles_unsupported() -> SearchError {
    SearchError::OperationFailed("this Document index runtime has no body bundles".into())
}

#[derive(Default)]
struct GraphOwnership {
    by_resource: BTreeMap<ResourceId, (DocumentId, BTreeSet<ProjectionGenerationKey>)>,
    by_generation: BTreeMap<ProjectionGenerationKey, BTreeSet<ResourceId>>,
}

/// All generation artifacts and structural ownership share one clone.
#[derive(Clone)]
pub struct MemoryDocumentIndexRuntime {
    store: MemoryProjectionStore,
    lexical: Arc<TantivyLexicalIndex>,
    graph: Arc<MemoryGraphRetriever>,
    graph_access: DocumentGraphAccessReader,
    ownership: Arc<RwLock<GraphOwnership>>,
    bundles: Arc<std::sync::Mutex<BundleRegistry>>,
}

impl MemoryDocumentIndexRuntime {
    pub fn new() -> Self {
        Self::make(None)
    }

    pub fn with_current_access(access: Arc<DocumentCurrentAccessAdapter>) -> Self {
        Self::make(Some(access))
    }

    fn make(access: Option<Arc<DocumentCurrentAccessAdapter>>) -> Self {
        let ownership = Arc::new(RwLock::new(GraphOwnership::default()));
        let graph_access = DocumentGraphAccessReader {
            access,
            ownership: ownership.clone(),
        };
        Self {
            store: MemoryProjectionStore::default(),
            lexical: Arc::new(TantivyLexicalIndex::default()),
            graph: Arc::new(MemoryGraphRetriever::new(Arc::new(graph_access.clone()))),
            graph_access,
            ownership,
            bundles: Arc::new(std::sync::Mutex::new(BundleRegistry::default())),
        }
    }

    fn bundles(&self) -> Result<std::sync::MutexGuard<'_, BundleRegistry>, SearchError> {
        self.bundles
            .lock()
            .map_err(|_| SearchError::OperationFailed("body bundle lock poisoned".into()))
    }

    pub fn projection_reader(&self) -> DocumentProjectionReader {
        DocumentProjectionReader(self.store.clone())
    }

    pub fn lexical_reader(&self) -> DocumentLexicalReader {
        DocumentLexicalReader(self.lexical.clone())
    }

    pub fn graph_reader(&self) -> DocumentGraphReader {
        DocumentGraphReader(self.graph.clone())
    }

    pub fn graph_access_reader(&self) -> DocumentGraphAccessReader {
        self.graph_access.clone()
    }

    /// Artifacts of a published body bundle; `None` for an unpublished or
    /// projection-only generation.
    pub fn published_body(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<Option<PublishedBody>, SearchError> {
        Ok(self.bundles()?.published_body(key))
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

impl Default for MemoryDocumentIndexRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct DocumentGraphAccessReader {
    access: Option<Arc<DocumentCurrentAccessAdapter>>,
    ownership: Arc<RwLock<GraphOwnership>>,
}

impl CurrentAccessEvaluatorPort for DocumentGraphAccessReader {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            let Some(access) = &self.access else {
                return Ok(AccessDecision::Unknown);
            };
            let owner = self
                .ownership
                .read()
                .map_err(|_| SearchError::OperationFailed("graph ownership lock poisoned".into()))?
                .by_resource
                .get(&resource_ref)
                .map(|(document, _)| *document);
            if let Some(document) = owner {
                access
                    .evaluate_owned_document(document, access_context)
                    .await
            } else {
                access.evaluate(resource_ref, access_context).await
            }
        })
    }
}

#[derive(Clone)]
pub struct DocumentGraphReader(Arc<MemoryGraphRetriever>);

impl HyperGraphRetrieverPort for DocumentGraphReader {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        plan: &'a GraphTraversalPlan,
    ) -> BoxFuture<'a, GraphRetrievalResult> {
        HyperGraphRetrieverPort::retrieve(self.0.as_ref(), generation, plan)
    }
}

#[derive(Clone)]
pub struct DocumentProjectionReader(MemoryProjectionStore);

impl DocumentProjectionReader {
    /// A reader over a store hydrated from durable READY generations.
    pub fn over(store: MemoryProjectionStore) -> Self {
        Self(store)
    }

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

// Discovery accepts ProjectionGenerationStore for generation pinning. This
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

impl ConceptRegistryPort for DocumentProjectionReader {
    fn pin_view<'a>(
        &'a self,
        key: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Arc<dyn ConceptResolver + Send + Sync>> {
        ConceptRegistryPort::pin_view(&self.0, key)
    }

    fn same_concept<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        left: &'a str,
        right: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        ConceptRegistryPort::same_concept(&self.0, key, left, right)
    }

    fn is_a<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        child: &'a str,
        parent: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        ConceptRegistryPort::is_a(&self.0, key, child, parent)
    }
}

impl DirectoryRetrieverPort for DocumentProjectionReader {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        DirectoryRetrieverPort::retrieve(&self.0, generation, request)
    }
}

impl StructuredRetrieverPort for DocumentProjectionReader {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        hard_filters: &'a [StructuredFacetFilter],
    ) -> BoxFuture<'a, Vec<StructuredRetrievalHit>> {
        StructuredRetrieverPort::retrieve(&self.0, generation, request, hard_filters)
    }
}

impl AssertionStorePort for DocumentProjectionReader {
    fn assertions_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        predicate: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        Box::pin(async move {
            let mut assertions =
                AssertionStorePort::assertions_for(&self.0, generation, resource_ref, predicate)
                    .await?;
            for assertion in &mut assertions {
                expose_generation_locators(assertion, generation, resource_ref);
            }
            Ok(assertions)
        })
    }
}

#[derive(Clone)]
pub struct DocumentLexicalReader(Arc<TantivyLexicalIndex>);

impl DocumentLexicalReader {
    /// A reader over generations reopened from sealed lexical directories.
    pub fn over(index: Arc<TantivyLexicalIndex>) -> Self {
        Self(index)
    }
}

impl LexicalRetrieverPort for DocumentLexicalReader {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        LexicalRetrieverPort::retrieve(self.0.as_ref(), generation, request, query)
    }

    fn retrieve_body<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, LexicalRetrievalBatch> {
        LexicalRetrieverPort::retrieve_body(self.0.as_ref(), generation, request, query)
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
            // Re-verify the validated bundle and switch the pointer under one lock.
            let mut bundles = self.bundles()?;
            let bundle = bundles.publishable(key)?;
            let switched = self
                .store
                .publish_generation_if_current(key, expected_current)?;
            if switched && bundle.is_some() {
                bundles.mark_published(key);
            }
            Ok(switched)
        })
    }

    fn fail_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::fail_generation(&self.store, key)
    }

    fn discard_projection_generation<'a>(
        &'a self,
        key: ProjectionGenerationKey,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.store.discard_unpublished_generation(key) })
    }

    fn build_lexical_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        input: LexicalBuildInput,
    ) -> Result<(), LexicalIndexError> {
        let key = manifest.key();
        let digest = lexical_input_digest(&input)?;
        self.lexical.build_generation(manifest, source, input)?;
        self.bundles()
            .and_then(|mut bundles| {
                bundles.record_lexical(ArtifactReceipt {
                    key,
                    digest: digest.digest,
                    count: digest.count,
                })
            })
            .map_err(|_| LexicalIndexError::DuplicateGeneration)
    }

    fn discard_lexical_generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<bool, LexicalIndexError> {
        if let Ok(mut bundles) = self.bundles() {
            bundles.forget_lexical(key);
        }
        self.lexical.discard_generation(key)
    }

    fn build_graph_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        projections: Vec<CompiledResourceProjection>,
        ownership: Vec<(ResourceId, DocumentId)>,
    ) -> Result<(), SearchError> {
        validate_document_graph(&projections)?;
        let key = manifest.key();
        let receipt = graph_receipt(key, &projections, &ownership)?;
        let owners = validate_graph_ownership(&projections, ownership)?;
        let mut map = self
            .ownership
            .write()
            .map_err(|_| SearchError::OperationFailed("graph ownership lock poisoned".into()))?;
        if map.by_generation.contains_key(&key) {
            return Err(SearchError::OperationFailed(
                "graph generation ownership already registered".into(),
            ));
        }
        for (id, document) in &owners {
            if map
                .by_resource
                .get(id)
                .is_some_and(|(known, _)| known != document)
            {
                return Err(SearchError::OperationFailed(
                    "graph auxiliary ResourceId owner collision".into(),
                ));
            }
        }
        self.graph
            .build_generation(manifest, source, projections)
            .map_err(|error| {
                SearchError::OperationFailed(format!("Document graph build failed: {error}"))
            })?;
        for (&id, &document) in &owners {
            map.by_resource
                .entry(id)
                .or_insert_with(|| (document, BTreeSet::new()))
                .1
                .insert(key);
        }
        map.by_generation.insert(key, owners.into_keys().collect());
        self.bundles()?.record_graph(receipt)
    }

    fn discard_graph_generation(&self, key: ProjectionGenerationKey) -> Result<bool, SearchError> {
        self.bundles()?.forget_graph(key);
        let mut map = self
            .ownership
            .write()
            .map_err(|_| SearchError::OperationFailed("graph ownership lock poisoned".into()))?;
        let discarded = self.graph.discard_generation(key).map_err(|error| {
            SearchError::OperationFailed(format!("Document graph rollback failed: {error}"))
        })?;
        if let Some(ids) = map.by_generation.remove(&key) {
            for id in ids {
                if let Some((_, generations)) = map.by_resource.get_mut(&id) {
                    generations.remove(&key);
                    if generations.is_empty() {
                        map.by_resource.remove(&id);
                    }
                }
            }
        }
        Ok(discarded)
    }

    fn stage_body_unit_manifest<'a>(&'a self, manifest: BodyUnitManifest) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.bundles()?.stage_manifest(manifest) })
    }

    fn stage_body_coverage<'a>(&'a self, artifact: BodyCoverageArtifact) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.bundles()?.stage_coverage(artifact) })
    }

    fn validate_bundle<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        projection_digest: String,
    ) -> BoxFuture<'a, GenerationBundleReceipt> {
        Box::pin(async move {
            let documents = self.lexical.enumerate_unit_docs(key).map_err(|error| {
                SearchError::OperationFailed(format!("body bundle: lexical seal: {error}"))
            })?;
            self.bundles()?.validate(key, &projection_digest, documents)
        })
    }

    fn pin_current_bundle<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<(ProjectionGenerationManifest, GenerationBundleReceipt)>> {
        Box::pin(async move {
            let Some(manifest) =
                ProjectionGenerationStore::pin_current(&self.store, source_id).await?
            else {
                return Ok(None);
            };
            let receipt = self.bundles()?.published(manifest.key());
            Ok(receipt.map(|receipt| (manifest, receipt)))
        })
    }

    fn discard_body_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.bundles()?.discard(key) })
    }
}

fn validate_graph_ownership(
    projections: &[CompiledResourceProjection],
    ownership: Vec<(ResourceId, DocumentId)>,
) -> Result<BTreeMap<ResourceId, DocumentId>, SearchError> {
    let expected: BTreeSet<_> = projections
        .iter()
        .filter(|projection| {
            matches!(
                projection.directory.kind,
                ResourceKind::Document | ResourceKind::FolderPlacement
            )
        })
        .map(|projection| projection.directory.resource_ref)
        .collect();
    let mut owners = BTreeMap::new();
    for (id, document) in ownership {
        if !expected.contains(&id) || owners.insert(id, document).is_some() {
            return Err(SearchError::OperationFailed(
                "invalid Document graph auxiliary owner".into(),
            ));
        }
    }
    if owners.len() != expected.len() {
        return Err(SearchError::OperationFailed(
            "Document graph auxiliary owner is missing".into(),
        ));
    }
    Ok(owners)
}

/// D8 Source invariant: every participant is a compiled Resource of this
/// generation and carries the same canonical relation definition.
fn validate_document_graph(projections: &[CompiledResourceProjection]) -> Result<(), SearchError> {
    let resources: BTreeSet<_> = projections
        .iter()
        .map(|projection| projection.directory.resource_ref)
        .collect();
    if resources.len() != projections.len() {
        return Err(SearchError::OperationFailed(
            "duplicate Document graph participant".into(),
        ));
    }
    let mut relations: BTreeMap<RelationId, (TypedRelationInstance, BTreeSet<ResourceId>)> =
        BTreeMap::new();
    for projection in projections {
        let resource = projection.directory.resource_ref;
        for relation in &projection.relations {
            if relation.validate().is_err()
                || !relation
                    .participants
                    .iter()
                    .any(|participant| participant.resource_ref == resource)
            {
                return Err(SearchError::OperationFailed(
                    "invalid Document graph relation attachment".into(),
                ));
            }
            let entry = relations
                .entry(relation.relation_id)
                .or_insert_with(|| (relation.clone(), BTreeSet::new()));
            if entry.0 != *relation || !entry.1.insert(resource) {
                return Err(SearchError::OperationFailed(
                    "conflicting Document graph relation attachment".into(),
                ));
            }
        }
    }
    for (relation, attached) in relations.values() {
        let participants: BTreeSet<_> = relation
            .participants
            .iter()
            .map(|participant| participant.resource_ref)
            .collect();
        if participants != *attached || !participants.is_subset(&resources) {
            return Err(SearchError::OperationFailed(
                "Document graph relation has a missing generation participant".into(),
            ));
        }
    }
    Ok(())
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
    body: Option<Arc<dyn BodyItemExtractor>>,
    completion: Option<Arc<dyn SearchEventCompletionPort>>,
}

/// One fenced delivery: the event, both live leases, the runner's
/// cancellation and the only port allowed to complete it.
struct Delivery {
    event: DocumentSourceEvent,
    fence: SearchDeliveryFence,
    cancel: Arc<AtomicBool>,
    completion: Arc<dyn SearchEventCompletionPort>,
}

impl Delivery {
    fn check(&self) -> Result<(), SearchError> {
        if self.cancel.load(Ordering::Acquire) {
            Err(SearchError::FenceLost)
        } else {
            Ok(())
        }
    }
}

/// A committed fenced outcome; `Retry` rebuilds from a new snapshot.
fn fenced_outcome(
    outcome: SearchCompletionOutcome,
) -> Result<Option<IndexingOutcome>, SearchError> {
    match outcome {
        SearchCompletionOutcome::Published(key) => Ok(Some(IndexingOutcome::Published(key))),
        SearchCompletionOutcome::Unchanged(key) => Ok(Some(IndexingOutcome::Unchanged(key))),
        SearchCompletionOutcome::Duplicate(key) => Ok(Some(IndexingOutcome::Duplicate(key))),
        SearchCompletionOutcome::Retry => Ok(None),
        SearchCompletionOutcome::Lost => Err(SearchError::FenceLost),
    }
}

impl<R, E, T: DocumentIndexRuntime> DocumentOutboxIndexer<R, E, T> {
    pub fn new(reader: R, config: DocumentIndexingConfig, runtime: T, receipts: E) -> Self {
        Self {
            reader,
            config,
            runtime,
            receipts,
            gate: Mutex::new(()),
            body: None,
            completion: None,
        }
    }

    /// P6-S04: an event-origin build completes only through this atomic
    /// port, never through the legacy receipt store or pointer switch.
    pub fn with_fenced_completion(
        mut self,
        completion: Arc<dyn SearchEventCompletionPort>,
    ) -> Self {
        self.completion = Some(completion);
        self
    }

    /// Build body-ready (P1) generations: every Live authoritative item is
    /// extracted, sealed into the same-key bundle and published together.
    pub fn with_body_extractor(mut self, extractor: Arc<dyn BodyItemExtractor>) -> Self {
        self.body = Some(extractor);
        self
    }
}

/// Every Live item binding that must still hold right before publication.
fn live_binding_fingerprint(snapshot: &DocumentOutboxSnapshot) -> Vec<String> {
    let mut rows: Vec<String> = snapshot
        .live
        .iter()
        .map(|record| {
            format!(
                "{}|{}|{:?}|{}|{}|{:?}|{:?}",
                record.snapshot.document_id.as_uuid(),
                record.snapshot.document_version_id.as_uuid(),
                record.snapshot.current_version_id.map(|id| id.as_uuid()),
                record.document_revision,
                record.access_revision,
                record.snapshot.publication_end,
                record.authoritative_items,
            )
        })
        .collect();
    rows.sort();
    rows
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl<R: DocumentOutboxReader, E: IndexingReceiptStore, T: DocumentIndexRuntime>
    DocumentOutboxIndexer<R, E, T>
{
    /// Extract every Live authoritative item exactly once and validate the result
    /// against the same Source snapshot.
    async fn body_bundle(
        &self,
        extractor: &dyn BodyItemExtractor,
        snapshot: &DocumentOutboxSnapshot,
        key: ProjectionGenerationKey,
    ) -> Result<(BodyUnitManifest, BodyCoverageArtifact), SearchError> {
        let source_id = self.config.source.source_id;
        let mut entries = Vec::new();
        for record in &snapshot.live {
            for item in &record.authoritative_items {
                let result = extractor.extract(record, item).await?;
                entries.push(BodyItemEntry::from_extracted(
                    source_id,
                    record,
                    item,
                    extractor.parser_build_id(),
                    result,
                ));
            }
        }
        entries.sort_by(|left, right| {
            (
                left.version.resource_id,
                left.part.ordinal,
                &left.part.logical_path,
                &left.part.source_native_part_id,
            )
                .cmp(&(
                    right.version.resource_id,
                    right.part.ordinal,
                    &right.part.logical_path,
                    &right.part.source_native_part_id,
                ))
        });
        let manifest = BodyUnitManifest {
            key,
            source_snapshot: snapshot.source_snapshot.clone(),
            entries,
        };
        let coverage = validate_manifest(&manifest, snapshot)
            .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
        Ok((manifest, coverage))
    }

    async fn cleanup_unpublished(&self, key: ProjectionGenerationKey) -> Result<(), SearchError> {
        let mut failures = Vec::new();
        if let Err(error) = self.runtime.discard_body_generation(key).await {
            failures.push(format!("body bundle cleanup: {error}"));
        }
        if let Err(error) = self.runtime.discard_graph_generation(key) {
            failures.push(format!("graph cleanup: {error}"));
        }
        if let Err(error) = self.runtime.discard_lexical_generation(key) {
            failures.push(format!("lexical cleanup: {error}"));
        }
        if let Err(error) = self.runtime.discard_projection_generation(key).await {
            failures.push(format!("projection cleanup: {error}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(SearchError::OperationFailed(format!(
                "Document generation cleanup failed: {}",
                failures.join("; ")
            )))
        }
    }

    async fn cleanup_after_error(
        &self,
        key: ProjectionGenerationKey,
        error: SearchError,
    ) -> SearchError {
        match self.cleanup_unpublished(key).await {
            Ok(()) => error,
            Err(cleanup) => SearchError::OperationFailed(format!("{error}; {cleanup}")),
        }
    }

    async fn reconcile(
        &self,
        event_id: Option<Uuid>,
        full_rebuild: bool,
        delivery: Option<Delivery>,
    ) -> Result<IndexingOutcome, SearchError> {
        // Serialize this instance. The shared store's conditional pointer
        // switch protects independent indexers using the same Source.
        let _guard = self.gate.lock().await;
        for _ in 0..3 {
            if let Some(outcome) = self
                .reconcile_once(event_id, full_rebuild, delivery.as_ref())
                .await?
            {
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
        delivery: Option<&Delivery>,
    ) -> Result<Option<IndexingOutcome>, SearchError> {
        let source_id = self.config.source.source_id;
        let expected_current = self
            .runtime
            .pin_current(source_id)
            .await?
            .map(|item| item.key());
        let fenced_current: Option<CurrentGenerationSnapshot> = match delivery {
            Some(delivery) => Some(delivery.completion.current_snapshot(source_id).await?),
            None => None,
        };
        let snapshot = self.reader.enumerate_snapshot().await?;
        if let Some(delivery) = delivery {
            delivery.check()?;
        }
        let source_snapshot = one_source_snapshot(&snapshot)?;
        let body_snapshot = self.body.as_ref().map(|_| snapshot.clone());
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
            graph_schema_version: Some("typed-nary-v1".into()),
            source_snapshot: source_snapshot.clone(),
            resource_count: 0,
            relation_count: Some(0),
            coverage: Coverage::CompleteEnumeration,
            digest: String::new(),
            built_at: OffsetDateTime::now_utc(),
        };
        let mut projections = Vec::with_capacity(live_count);
        let mut lexical_documents = Vec::with_capacity(live_count);
        let mut ownership = Vec::with_capacity(live_count * 2);
        for record in records {
            let document_id = record.snapshot.document_id;
            let observed_at = record
                .snapshot
                .published_at
                .unwrap_or(record.snapshot.created_at);
            let translation = translator.translate_record(record).map_err(|error| {
                SearchError::OperationFailed(format!("Document translation failed: {error}"))
            })?;
            let mut inputs = match translation {
                crate::translate::DocumentSourceTranslation::Live(inputs) => inputs,
                // Historical and authoring views require separate tiers; they
                // cannot enter the normal Live generation.
                crate::translate::DocumentSourceTranslation::Historical { .. }
                | crate::translate::DocumentSourceTranslation::Authoring { .. } => continue,
            };
            append_authoritative_assertions(&mut inputs.projection, document_id, observed_at);
            projections.push(
                ProjectionCompiler::compile_resource(&manifest, &inputs.projection).map_err(
                    |error| {
                        SearchError::OperationFailed(format!("Document projection failed: {error}"))
                    },
                )?,
            );
            for auxiliary in &inputs.auxiliary_projections {
                ownership.push((auxiliary.resource.identity.resource_id, document_id));
                projections.push(
                    ProjectionCompiler::compile_resource(&manifest, auxiliary).map_err(
                        |error| {
                            SearchError::OperationFailed(format!(
                                "Document auxiliary projection failed: {error}"
                            ))
                        },
                    )?,
                );
            }
            lexical_documents.push(inputs.lexical_document);
        }
        manifest.resource_count = projections.len() as u64;
        manifest.relation_count = Some(
            projections
                .iter()
                .flat_map(|projection| {
                    projection
                        .relations
                        .iter()
                        .map(|relation| relation.relation_id)
                })
                .collect::<BTreeSet<_>>()
                .len() as u64,
        );
        manifest.digest =
            generation_digest(source_id, &projections, &self.config.semantic_registry)?;
        let body = match (&self.body, &body_snapshot) {
            (Some(extractor), Some(snapshot)) => Some(
                self.body_bundle(extractor.as_ref(), snapshot, manifest.key())
                    .await?,
            ),
            _ => None,
        };
        let current = self.runtime.pin_current(source_id).await?;
        // A projection-only generation is never body-ready; a body-ready one is
        // unchanged only when its published bundle has the same body digests.
        let mut current_bundle = None;
        let body_unchanged = match &body {
            None => true,
            Some((unit_manifest, coverage)) => {
                match self.runtime.pin_current_bundle(source_id).await? {
                    Some((pinned, receipt))
                        if current
                            .as_ref()
                            .is_some_and(|current| current.key() == pinned.key()) =>
                    {
                        let unchanged = receipt.unit_manifest.digest
                            == unit_manifest_receipt(unit_manifest)
                                .map_err(|error| SearchError::OperationFailed(error.to_string()))?
                                .digest
                            && receipt.body_coverage.digest
                                == coverage_receipt(coverage)
                                    .map_err(|error| {
                                        SearchError::OperationFailed(error.to_string())
                                    })?
                                    .digest
                            && receipt.profile_set_digest
                                == profile_set_digest(unit_manifest).map_err(|error| {
                                    SearchError::OperationFailed(error.to_string())
                                })?;
                        current_bundle = Some(receipt);
                        unchanged
                    }
                    _ => false,
                }
            }
        };
        if let Some(current) = current.filter(|current| {
            body_unchanged
                && !full_rebuild
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
            if let (Some(delivery), Some(snapshot)) = (delivery, &fenced_current) {
                // The current READY bundle is re-validated by the port.
                if snapshot.key != Some(key) {
                    return Ok(None);
                }
                let outcome = delivery
                    .completion
                    .complete_event_if_current(CompleteEventRequest {
                        fence: delivery.fence,
                        expected_current: snapshot.clone(),
                        candidate: key,
                        manifest_digest: snapshot.manifest_digest.clone().unwrap_or_default(),
                        bundle_digest: snapshot.bundle_digest.clone().unwrap_or_default(),
                        mode: CompletionMode::ReuseCurrent,
                    })
                    .await?;
                return fenced_outcome(outcome);
            }
            let digest = match &current_bundle {
                Some(receipt) => format!("bundle:{}", hex(&receipt.composite_digest)),
                None => manifest.digest.clone(),
            };
            let duplicate = if let Some(event_id) = event_id {
                let previous = self.receipts.get(event_id).await?;
                let duplicate = previous
                    .as_ref()
                    .is_some_and(|receipt| receipt.digest == digest && receipt.generation == key);
                if !duplicate {
                    self.receipts
                        .put(
                            event_id,
                            IndexingReceipt {
                                digest,
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
            if let Some(snapshot) = &body_snapshot {
                self.runtime.bind_source_snapshot(key, snapshot).await?;
            }
            if let Some(delivery) = delivery {
                self.runtime
                    .bind_delivery(key, &delivery.event, delivery.fence)
                    .await?;
                delivery.check()?;
            }
            self.runtime
                .stage_concept_registry(key, self.config.semantic_registry.clone())
                .await?;
            for projection in &projections {
                let persistent = PersistableResourceProjection::try_from(projection.clone())
                    .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
                self.runtime.stage_resource(persistent).await?;
            }
            self.runtime.validate_generation(key).await
        }
        .await;
        if let Err(error) = staged {
            return Err(self.cleanup_after_error(key, error).await);
        }
        let mut lexical_input = LexicalBuildInput::new(
            source_id,
            source_snapshot,
            manifest.projection_schema_version.clone(),
            manifest.lens_version,
            lexical_documents,
        );
        if let Some((unit_manifest, _)) = &body {
            lexical_input = lexical_input.with_body_units(
                unit_manifest
                    .entries
                    .iter()
                    .flat_map(|entry| entry.units.iter().cloned())
                    .collect(),
            );
        }
        if let Err(error) = self.runtime.build_lexical_generation(
            manifest.clone(),
            &self.config.source,
            lexical_input,
        ) {
            return Err(self
                .cleanup_after_error(
                    key,
                    SearchError::OperationFailed(format!("Document lexical build failed: {error}")),
                )
                .await);
        }
        if let Err(error) = self.runtime.build_graph_generation(
            manifest.clone(),
            &self.config.source,
            projections,
            ownership,
        ) {
            return Err(self.cleanup_after_error(key, error).await);
        }
        let mut bundle_receipt = None;
        if let Some((unit_manifest, coverage)) = body {
            let validated = async {
                self.runtime.stage_body_unit_manifest(unit_manifest).await?;
                self.runtime.stage_body_coverage(coverage).await?;
                self.runtime
                    .validate_bundle(key, manifest.digest.clone())
                    .await
            }
            .await;
            match validated {
                Ok(receipt) => bundle_receipt = Some(receipt),
                Err(error) => return Err(self.cleanup_after_error(key, error).await),
            }
            // Re-read the Source right before publication; any binding change
            // discards this key and rebuilds from the new snapshot.
            let reread = self.reader.enumerate_snapshot().await;
            let unchanged = match (&reread, &body_snapshot) {
                (Ok(again), Some(first)) => {
                    live_binding_fingerprint(again) == live_binding_fingerprint(first)
                }
                _ => false,
            };
            if !unchanged {
                self.cleanup_unpublished(key).await?;
                reread?;
                return Ok(None);
            }
        }
        if let (Some(delivery), Some(snapshot)) = (delivery, fenced_current) {
            let completion = async {
                delivery.check()?;
                let receipt = bundle_receipt.as_ref().ok_or_else(|| {
                    SearchError::OperationFailed(
                        "fenced Document completion needs a body-ready bundle".into(),
                    )
                })?;
                delivery
                    .completion
                    .complete_event_if_current(CompleteEventRequest {
                        fence: delivery.fence,
                        expected_current: snapshot,
                        candidate: key,
                        manifest_digest: manifest.digest.clone(),
                        bundle_digest: format!("sha256:{}", hex(&receipt.composite_digest)),
                        mode: CompletionMode::PublishCandidate,
                    })
                    .await
            }
            .await;
            return match completion {
                Ok(SearchCompletionOutcome::Published(published)) if published == key => {
                    self.runtime.settled(key).await?;
                    Ok(Some(IndexingOutcome::Published(key)))
                }
                Ok(outcome) => {
                    // A lost or stale candidate is removed by its guard holder.
                    self.cleanup_unpublished(key).await?;
                    fenced_outcome(outcome)
                }
                Err(error) => Err(self.cleanup_after_error(key, error).await),
            };
        }
        let published = self.runtime.publish_if_current(key, expected_current).await;
        match published {
            Ok(true) => {}
            Ok(false) => {
                self.cleanup_unpublished(key).await.map_err(|error| {
                    SearchError::OperationFailed(format!(
                        "Document generation publication CAS rejected; {error}"
                    ))
                })?;
                return Ok(None);
            }
            Err(error) => return Err(self.cleanup_after_error(key, error).await),
        }
        if let Some(event_id) = event_id {
            let digest = match &bundle_receipt {
                Some(receipt) => format!("bundle:{}", hex(&receipt.composite_digest)),
                None => manifest.digest,
            };
            // A receipt write failure after publication keeps the published
            // bundle; the same event retries and completes the receipt.
            self.receipts
                .put(
                    event_id,
                    IndexingReceipt {
                        digest,
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
        Box::pin(async move { self.reconcile(Some(event.event_id), false, None).await })
    }

    fn rebuild<'a>(&'a self) -> BoxFuture<'a, IndexingOutcome> {
        Box::pin(async move { self.reconcile(None, true, None).await })
    }
}

impl<R: DocumentOutboxReader, E: IndexingReceiptStore, T: DocumentIndexRuntime>
    FencedDocumentIndexingPort for DocumentOutboxIndexer<R, E, T>
{
    fn refresh_fenced<'a>(
        &'a self,
        event: DocumentSourceEvent,
        fence: SearchDeliveryFence,
        cancel: Arc<AtomicBool>,
    ) -> BoxFuture<'a, IndexingOutcome> {
        Box::pin(async move {
            let completion = self.completion.clone().ok_or_else(|| {
                SearchError::OperationFailed("fenced completion is not configured".into())
            })?;
            if fence.event_id != event.event_id
                || fence.source.source_id != self.config.source.source_id
            {
                return Err(SearchError::InvalidRequest(
                    "delivery fence does not match the event or Source".into(),
                ));
            }
            self.reconcile(
                Some(event.event_id),
                false,
                Some(Delivery {
                    event,
                    fence,
                    cancel,
                    completion,
                }),
            )
            .await
        })
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

/// Participant access of the actors currently reading one durable Graph,
/// keyed by each request's opaque access-context binding.
#[derive(Default)]
struct GraphActors {
    owners: BTreeMap<ResourceId, DocumentId>,
    bound: RwLock<BTreeMap<String, (Arc<DocumentCurrentAccessAdapter>, usize)>>,
}

impl CurrentAccessEvaluatorPort for GraphActors {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            let access = self
                .bound
                .read()
                .map_err(|_| SearchError::OperationFailed("graph actor lock poisoned".into()))?
                .get(access_context)
                .map(|(access, _)| access.clone());
            // A context no live request bound is never evaluated.
            let Some(access) = access else {
                return Ok(AccessDecision::Unknown);
            };
            match self.owners.get(&resource_ref) {
                Some(document) => {
                    access
                        .evaluate_owned_document(*document, access_context)
                        .await
                }
                None => access.evaluate(resource_ref, access_context).await,
            }
        })
    }
}

/// One durable generation's Graph, loaded once and read by many actors.
/// Each request enters with its own Document access; its participant checks
/// use only that access, and leaving drops the binding.
pub struct DurableDocumentGraph {
    graph: Arc<MemoryGraphRetriever>,
    actors: Arc<GraphActors>,
}

impl DurableDocumentGraph {
    pub fn load(
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        projections: Vec<CompiledResourceProjection>,
        ownership: Vec<(ResourceId, DocumentId)>,
    ) -> Result<Self, SearchError> {
        validate_document_graph(&projections)?;
        let owners = validate_graph_ownership(&projections, ownership)?;
        let actors = Arc::new(GraphActors {
            owners,
            bound: RwLock::default(),
        });
        let graph = Arc::new(MemoryGraphRetriever::new(actors.clone()));
        graph
            .build_generation(manifest, source, projections)
            .map_err(|error| {
                SearchError::OperationFailed(format!("Document graph load failed: {error}"))
            })?;
        Ok(Self { graph, actors })
    }

    pub fn reader(&self) -> DocumentGraphReader {
        DocumentGraphReader(self.graph.clone())
    }

    /// Binds `access` to `binding` for one request.
    pub fn enter(
        &self,
        binding: String,
        access: Arc<DocumentCurrentAccessAdapter>,
    ) -> Result<DocumentGraphActorAccess, SearchError> {
        let mut bound = self
            .actors
            .bound
            .write()
            .map_err(|_| SearchError::OperationFailed("graph actor lock poisoned".into()))?;
        let entry = bound.entry(binding.clone()).or_insert((access, 0));
        entry.1 += 1;
        Ok(DocumentGraphActorAccess {
            actors: self.actors.clone(),
            binding,
        })
    }
}

/// One request's Graph participant access; dropping it leaves the Graph.
pub struct DocumentGraphActorAccess {
    actors: Arc<GraphActors>,
    binding: String,
}

impl CurrentAccessEvaluatorPort for DocumentGraphActorAccess {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if access_context != self.binding {
                return Ok(AccessDecision::Unknown);
            }
            CurrentAccessEvaluatorPort::evaluate(self.actors.as_ref(), resource_ref, access_context)
                .await
        })
    }
}

impl Drop for DocumentGraphActorAccess {
    fn drop(&mut self) {
        if let Ok(mut bound) = self.actors.bound.write()
            && let Some(entry) = bound.get_mut(&self.binding)
        {
            entry.1 -= 1;
            if entry.1 == 0 {
                bound.remove(&self.binding);
            }
        }
    }
}
