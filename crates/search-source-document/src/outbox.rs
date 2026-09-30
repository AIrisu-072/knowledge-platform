//! Rebuildable Document indexing from authoritative read-side snapshots.
//! This adapter never acknowledges or edits the generic Document outbox.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use document_domain::DocumentId;
use search_application::SearchError;
use search_application::indexing_service::{
    DocumentIndexingPort, DocumentSourceEvent, IndexingOutcome,
};
use search_application::ports::{
    AccessDecision, AssertionStorePort, BoxFuture, ConceptRegistryPort, CurrentAccessEvaluatorPort,
    DirectoryRetrieverPort, GraphRetrievalResult, HyperGraphRetrieverPort, LexicalQuery,
    LexicalRetrieverPort, ProjectionGenerationStore, SemanticRegistrySnapshot,
    StructuredFacetFilter, StructuredRetrievalHit, StructuredRetrieverPort,
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
use search_tantivy::{LexicalBuildInput, LexicalIndexError, TantivyLexicalIndex};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::evidence::{append_authoritative_assertions, expose_generation_locators};
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
        }
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
        self.lexical.build_generation(manifest, source, input)
    }

    fn discard_lexical_generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<bool, LexicalIndexError> {
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
        let owners = validate_graph_ownership(&projections, ownership)?;
        let key = manifest.key();
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
        Ok(())
    }

    fn discard_graph_generation(&self, key: ProjectionGenerationKey) -> Result<bool, SearchError> {
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
    async fn cleanup_unpublished(&self, key: ProjectionGenerationKey) -> Result<(), SearchError> {
        let mut failures = Vec::new();
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
