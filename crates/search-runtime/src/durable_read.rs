//! B4: the HTTP API reads the Document Source's durable P7 generation.
//!
//! The read model follows the Source's current pointer. When the pointer
//! moves, the new key is re-verified from every stored artifact (the P7-12
//! check: payload DTOs and composite digest, the sealed lexical directory,
//! the Graph rows and the pointer's two digests) before anything of it is
//! served. The verified projection is staged into a fresh in-memory store
//! and the sealed lexical directory is reopened, so each loaded generation
//! is self-contained and is dropped once no request holds it. A key that
//! fails re-verification is never served; the API reports the Source as
//! unavailable until a new key is published.
//!
//! B7: the verified Graph is loaded with the generation, its structural
//! owners taken from the verified Graph rows. Each request enters it with
//! its own Document access, so every traversed participant is checked for
//! that actor alone.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use document_domain::DocumentId;
use search_api_http::router::ApiFuture;
use search_application::api_scope::ApiError;
use search_application::graph_generation::GraphSourceMapping;
use search_application::ports::ProjectionGenerationStore;
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_application::resource_read::{CurrentResourceReadPort, ResourceLocatorPort};
use search_application::scoped::TrustedSearchScope;
use search_application::search_core::id::ProjectionGenerationId;
use search_application::search_core::id::ResourceId;
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_application::search_core::source::DiscoverableSource;
use search_graph::PostgresGraphStore;
use search_projection_memory::MemoryProjectionStore;
use search_source_document::{
    DocumentCurrentAccessAdapter, DocumentEvidenceCatalog, DocumentLexicalReader,
    DocumentProjectionReader, DurableDocumentGraph,
};
use search_tantivy::TantivyLexicalIndex;
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock, watch};
use uuid::Uuid;

use crate::api::{ActorPorts, ActorPortsFactory};
use crate::lexical_artifact::LexicalArtifactStore;
use crate::payload::{PgPayloadStore, RestoredPayloadV1};
use crate::recovery::{CurrentState, PgStartupRecovery};

/// Why a durable generation could not be loaded.
#[derive(Debug, Clone)]
pub enum DurableReadError {
    /// The current key failed re-verification; it must not be served.
    Unusable(ProjectionGenerationKey),
    /// The pointer moved while the key was loaded; the next read retries.
    Moved,
    Store(String),
}

impl fmt::Display for DurableReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unusable(_) => f.write_str("current generation failed re-verification"),
            Self::Moved => f.write_str("current generation moved while loading"),
            Self::Store(what) => write!(f, "durable read failed: {what}"),
        }
    }
}

impl std::error::Error for DurableReadError {}

fn store_error(what: &str, error: impl fmt::Debug) -> DurableReadError {
    DurableReadError::Store(format!("{what}: {error:?}"))
}

/// One verified generation, loaded for queries.
pub struct LoadedGeneration {
    key: ProjectionGenerationKey,
    store: MemoryProjectionStore,
    lexical: Arc<TantivyLexicalIndex>,
    graph: DurableDocumentGraph,
    /// E: the generation's indexed Units for Vector hit resolution.
    vector_units: Arc<crate::vector_runtime::VectorUnits>,
}

impl LoadedGeneration {
    pub fn key(&self) -> ProjectionGenerationKey {
        self.key
    }

    pub fn projection_reader(&self) -> DocumentProjectionReader {
        DocumentProjectionReader::over(self.store.clone())
    }

    pub fn lexical_reader(&self) -> DocumentLexicalReader {
        DocumentLexicalReader::over(self.lexical.clone())
    }

    pub fn graph(&self) -> &DurableDocumentGraph {
        &self.graph
    }
}

/// The Source's current durable generation as a read model.
pub struct DurableDocumentReadModel {
    pool: PgPool,
    lexical_root: PathBuf,
    source: DiscoverableSource,
    /// Whether loads keep the Units for Vector hit resolution.
    vector_units: bool,
    loaded: RwLock<Option<Arc<LoadedGeneration>>>,
    /// The key being loaded and the outcome of its load task. The load runs
    /// detached, so a request that gives up (e.g. at its operation deadline)
    /// does not cancel it; later requests wait for the same outcome.
    loading: Mutex<Option<(ProjectionGenerationKey, LoadOutcome)>>,
}

type LoadOutcome = watch::Receiver<Option<Result<Arc<LoadedGeneration>, DurableReadError>>>;

impl DurableDocumentReadModel {
    pub fn new(pool: PgPool, lexical_root: impl Into<PathBuf>, source: DiscoverableSource) -> Self {
        Self {
            pool,
            lexical_root: lexical_root.into(),
            source,
            vector_units: true,
            loaded: RwLock::new(None),
            loading: Mutex::new(None),
        }
    }

    /// For a host without Vector retrieval: loads do not keep a copy of every
    /// Unit, and a Vector hit never resolves to a current Unit.
    pub fn without_vector_units(mut self) -> Self {
        self.vector_units = false;
        self
    }

    /// A model over the same Source, for a detached load task.
    fn detached(&self) -> Self {
        Self {
            vector_units: self.vector_units,
            ..Self::new(
                self.pool.clone(),
                self.lexical_root.clone(),
                self.source.clone(),
            )
        }
    }

    async fn current_key(&self) -> Result<Option<ProjectionGenerationKey>, DurableReadError> {
        let current: Option<Option<Uuid>> = sqlx::query_scalar(
            "SELECT current_generation_id FROM search_source_coordination WHERE source_id=$1",
        )
        .bind(self.source.source_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| store_error("current read", error))?;
        Ok(current.flatten().map(|generation| ProjectionGenerationKey {
            source_id: self.source.source_id,
            generation_id: ProjectionGenerationId::from_uuid(generation),
        }))
    }

    async fn manifest(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<ProjectionGenerationManifest, DurableReadError> {
        let dto: serde_json::Value = sqlx::query_scalar(
            "SELECT projection_manifest FROM search_generation \
             WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(|error| store_error("manifest read", error))?;
        let manifest: ProjectionGenerationManifest =
            serde_json::from_value(dto.get("manifest").cloned().unwrap_or_default())
                .map_err(|error| store_error("manifest decode", error))?;
        if manifest.key() != key {
            return Err(store_error("manifest key", key));
        }
        Ok(manifest)
    }

    /// The generation that is current now, loaded and verified; `None`
    /// before the Source's first publication.
    pub async fn current(&self) -> Result<Option<Arc<LoadedGeneration>>, DurableReadError> {
        let Some(key) = self.current_key().await? else {
            return Ok(None);
        };
        if let Some(loaded) = self.loaded.read().await.as_ref()
            && loaded.key == key
        {
            return Ok(Some(loaded.clone()));
        }
        let mut outcome = {
            let mut loading = self.loading.lock().await;
            if let Some(loaded) = self.loaded.read().await.as_ref()
                && loaded.key == key
            {
                return Ok(Some(loaded.clone()));
            }
            match loading.as_ref() {
                Some((pending, outcome)) if *pending == key => outcome.clone(),
                _ => {
                    let (sender, outcome) = watch::channel(None);
                    let loader = self.detached();
                    tokio::spawn(async move {
                        let result = loader.load(key).await.map(Arc::new);
                        let _ = sender.send(Some(result));
                    });
                    *loading = Some((key, outcome.clone()));
                    outcome
                }
            }
        };
        let result = outcome
            .wait_for(Option::is_some)
            .await
            .map_err(|_| store_error("load task", key))?
            .clone()
            .ok_or_else(|| store_error("load task", key))?;
        let mut loading = self.loading.lock().await;
        if loading.as_ref().is_some_and(|(pending, _)| *pending == key) {
            *loading = None;
        }
        let loaded = result?;
        let mut current = self.loaded.write().await;
        if current.as_ref().is_none_or(|current| current.key != key) {
            *current = Some(loaded.clone());
        }
        Ok(Some(loaded))
    }

    async fn load(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<LoadedGeneration, DurableReadError> {
        let recovery =
            PgStartupRecovery::new(self.pool.clone(), &self.lexical_root, self.source.clone());
        match recovery
            .verify_current()
            .await
            .map_err(|error| store_error("re-verification", error))?
        {
            CurrentState::Verified(bundle) if bundle.key() == key => {}
            CurrentState::Unusable(unusable, _) if unusable == key => {
                return Err(DurableReadError::Unusable(key));
            }
            _ => return Err(DurableReadError::Moved),
        }
        let manifest = self.manifest(key).await?;
        let payloads = PgPayloadStore::new(self.pool.clone());
        // Without Vector retrieval no Unit text is needed: the payloads are
        // checked from per-segment summaries and the Units are never held.
        let (projection, vector_units) = if self.vector_units {
            let RestoredPayloadV1 {
                projection,
                unit_manifest,
                coverage: _,
            } = payloads
                .restore(&manifest)
                .await
                .map_err(|error| store_error("payload restore", error))?;
            let units = crate::vector_runtime::vector_units(key, &unit_manifest);
            (projection, units)
        } else {
            let restored = payloads
                .restore_without_units(&manifest)
                .await
                .map_err(|error| store_error("payload restore", error))?;
            (restored.projection, Default::default())
        };
        let vector_units = Arc::new(vector_units);

        // Structural owners come from the verified Graph rows, never RAM.
        let (_, records, _) = PostgresGraphStore::new(self.pool.clone())
            .recover_rows(key, &manifest.digest)
            .await
            .map_err(|error| store_error("graph rows", error))?;
        let owners: Vec<(ResourceId, DocumentId)> = records
            .iter()
            .filter_map(|record| match &record.mapping {
                GraphSourceMapping::Document { document_id }
                | GraphSourceMapping::FolderPlacement { document_id, .. } => {
                    Some((record.resource_ref, DocumentId::from_uuid(*document_id)))
                }
                _ => None,
            })
            .collect();
        let graph = DurableDocumentGraph::load(
            manifest.clone(),
            &self.source,
            projection.resources.clone(),
            owners,
        )
        .map_err(|error| store_error("graph load", error))?;

        let store = MemoryProjectionStore::new();
        let persistable = PersistableGenerationManifest::try_from((manifest.clone(), &self.source))
            .map_err(|error| store_error("manifest retention", error))?;
        store
            .begin_generation(persistable)
            .await
            .map_err(|error| store_error("stage manifest", error))?;
        store
            .stage_concept_registry(key, projection.registry)
            .await
            .map_err(|error| store_error("stage registry", error))?;
        for resource in projection.resources {
            let resource = PersistableResourceProjection::try_from(resource)
                .map_err(|error| store_error("resource retention", error))?;
            store
                .stage_resource(resource)
                .await
                .map_err(|error| store_error("stage resource", error))?;
        }
        store
            .validate_generation(key)
            .await
            .map_err(|error| store_error("validate", error))?;
        store
            .publish_generation(key)
            .await
            .map_err(|error| store_error("publish", error))?;

        let lexical = Arc::new(TantivyLexicalIndex::new());
        let artifacts = LexicalArtifactStore::new(&self.lexical_root, self.pool.clone());
        lexical
            .load_generation_at(&manifest, &self.source, &artifacts.final_dir(key))
            .map_err(|error| store_error("lexical reopen", error))?;
        Ok(LoadedGeneration {
            key,
            store,
            lexical,
            graph,
            vector_units,
        })
    }
}

/// The actor's current Document access and Resource reads, from the host
/// that maps a verified Search actor to its Document identity.
pub struct DocumentActorAccess {
    /// The actor's current Document access; Graph participants use it too.
    pub access: Arc<DocumentCurrentAccessAdapter>,
    pub resource_locator: Arc<dyn ResourceLocatorPort>,
    pub resource_reader: Arc<dyn CurrentResourceReadPort>,
}

pub trait DocumentActorAccessPort: Send + Sync {
    fn for_actor<'a>(&'a self, actor: &'a TrustedSearchScope)
    -> ApiFuture<'a, DocumentActorAccess>;
}

/// Production [`ActorPortsFactory`] over the durable Document read model.
pub struct DurableDocumentPorts {
    model: Arc<DurableDocumentReadModel>,
    access: Arc<dyn DocumentActorAccessPort>,
    vector: Option<crate::vector_runtime::VectorServices>,
}

impl DurableDocumentPorts {
    pub fn new(
        model: Arc<DurableDocumentReadModel>,
        access: Arc<dyn DocumentActorAccessPort>,
    ) -> Self {
        Self {
            model,
            access,
            vector: None,
        }
    }

    /// E: Vector retrieval over the loaded generation's published index.
    pub fn with_vector(mut self, services: crate::vector_runtime::VectorServices) -> Self {
        self.vector = Some(services);
        self
    }
}

impl ActorPortsFactory for DurableDocumentPorts {
    fn for_actor<'a>(&'a self, actor: &'a TrustedSearchScope) -> ApiFuture<'a, ActorPorts> {
        Box::pin(async move {
            let access = self.access.for_actor(actor).await?;
            let loaded = self
                .model
                .current()
                .await
                .map_err(|_| ApiError::DependencyUnavailable)?;
            // Before the first publication the Source has no generation and
            // every read sees an empty store.
            let (reader, lexical) = match &loaded {
                Some(loaded) => (loaded.projection_reader(), loaded.lexical_reader()),
                None => (
                    DocumentProjectionReader::over(MemoryProjectionStore::new()),
                    DocumentLexicalReader::over(Arc::new(TantivyLexicalIndex::new())),
                ),
            };
            let (hypergraph, graph_resource_access) = match &loaded {
                Some(loaded) => {
                    let entered = loaded
                        .graph()
                        .enter(
                            actor.access_handle().to_opaque_string(),
                            access.access.clone(),
                        )
                        .map_err(|_| ApiError::ServiceUnavailable)?;
                    (
                        Some(Arc::new(loaded.graph().reader())
                            as Arc<
                                dyn search_application::ports::HyperGraphRetrieverPort,
                            >),
                        Some(Arc::new(entered)
                            as Arc<
                                dyn search_application::ports::CurrentAccessEvaluatorPort,
                            >),
                    )
                }
                None => (None, None),
            };
            let vector = match (&loaded, &self.vector) {
                (Some(loaded), Some(services)) => {
                    Some(Arc::new(crate::vector_runtime::DocumentActorVector::new(
                        services.clone(),
                        loaded.key(),
                        loaded.vector_units.clone(),
                        access.access.clone(),
                        actor.access_handle().to_opaque_string(),
                    ))
                        as Arc<dyn crate::vector_runtime::ActorVectorPort>)
                }
                _ => None,
            };
            Ok(ActorPorts {
                generations: Arc::new(reader.clone()),
                concepts: Arc::new(reader.clone()),
                assertions: Arc::new(reader.clone()),
                evidence: Arc::new(DocumentEvidenceCatalog::new(reader.clone())),
                directory: Some(Arc::new(reader.clone())),
                structured: Some(Arc::new(reader)),
                lexical: Some(Arc::new(lexical)),
                hypergraph,
                graph_resource_access,
                vector,
                access: access.access,
                resource_locator: access.resource_locator,
                resource_reader: access.resource_reader,
            })
        })
    }
}
