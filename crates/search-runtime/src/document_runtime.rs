//! P3-D01 with the P7-09 production wiring: a durable `DocumentIndexRuntime`
//! for the existing Document outbox indexer.
//!
//! The indexer compiles projections, lexical input, body Units and Graph
//! relations from one Source snapshot exactly as before. This runtime keeps
//! them per key until the bundle is complete, then registers the MANUAL
//! target with its Graph mapping commitment, finalizes the lexical directory,
//! stores the payloads, stages the Graph rows from the snapshot-bound
//! Document mapping and settles READY in one commit. Publication is the P7
//! pointer CAS. A failed or lost key is aborted by its own guard holder. No
//! Graph ownership is read from RAM after READY; only the build in progress
//! is held in memory and nothing of it is authority.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use document_domain::DocumentId;
use search_application::SearchError;
use search_application::graph_generation::{GraphResourceRecord, GraphSourceMapping};
use search_application::ports::{BoxFuture, SearchCompletionOutcome, SemanticRegistrySnapshot};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_application::search_core::id::{ProjectionGenerationId, ResourceId, SourceId};
use search_application::search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_application::search_core::relation::TypedRelationInstance;
use search_application::search_core::source::DiscoverableSource;
use search_graph::PostgresGraphStore;
use search_projection_memory::generation_digest;
use search_source_document::{
    ArtifactReceipt, BodyCoverageArtifact, BodyUnitManifest, DocumentIndexRuntime,
    DocumentOutboxSnapshot, GenerationBundleReceipt, compute_bundle_receipt,
    document_graph_records, graph_receipt, graph_relations,
};
use search_tantivy::{LexicalBuildInput, LexicalIndexError, TantivyLexicalIndex};
use sqlx::PgPool;
use uuid::Uuid;

use crate::event_completion::PgPublication;
use crate::full_guard::{FullGuardTtl, ManualBuildHandle};
use crate::gc::PgGenerationGc;
use crate::generation_registration::{FullBuildRequest, PgGenerationRegistrar};
use crate::lexical_artifact::LexicalArtifactStore;
use crate::payload::{PgPayloadStore, ProjectionPayloadV1, StoredBundleV1};
use crate::ready::{ReadyCoordinator, VerifiedBundle};

fn failed(what: &str, error: impl Debug) -> SearchError {
    SearchError::OperationFailed(format!("durable Document {what}: {error:?}"))
}

/// One build in progress. Only the registered handle and the verified
/// bundle come from the database; everything else is the indexer's input.
#[derive(Default)]
struct Pending {
    manifest: Option<ProjectionGenerationManifest>,
    snapshot: Option<DocumentOutboxSnapshot>,
    registry: Option<SemanticRegistrySnapshot>,
    resources: Vec<CompiledResourceProjection>,
    lexical: Option<ArtifactReceipt>,
    graph: Option<GraphPlan>,
    unit_manifest: Option<BodyUnitManifest>,
    coverage: Option<BodyCoverageArtifact>,
    handle: Option<Arc<ManualBuildHandle>>,
    verified: Option<VerifiedBundle>,
}

#[derive(Clone)]
struct GraphPlan {
    records: Vec<GraphResourceRecord>,
    relations: Vec<TypedRelationInstance>,
    mapping_digest: String,
    owners: Vec<(ResourceId, DocumentId)>,
}

pub struct PgDocumentIndexRuntime {
    pool: PgPool,
    lexical_root: PathBuf,
    source: DiscoverableSource,
    registrar: PgGenerationRegistrar,
    guard_ttl: FullGuardTtl,
    pending: Mutex<BTreeMap<ProjectionGenerationKey, Pending>>,
}

impl PgDocumentIndexRuntime {
    pub fn new(
        pool: PgPool,
        lexical_root: impl Into<PathBuf>,
        source: DiscoverableSource,
        registrar: PgGenerationRegistrar,
        guard_ttl: FullGuardTtl,
    ) -> Self {
        Self {
            pool,
            lexical_root: lexical_root.into(),
            source,
            registrar,
            guard_ttl,
            pending: Mutex::new(BTreeMap::new()),
        }
    }

    fn with<T>(
        &self,
        key: ProjectionGenerationKey,
        f: impl FnOnce(&mut Pending) -> Result<T, SearchError>,
    ) -> Result<T, SearchError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| SearchError::OperationFailed("durable runtime lock poisoned".into()))?;
        let entry = pending
            .get_mut(&key)
            .ok_or_else(|| SearchError::OperationFailed("unknown durable Document build".into()))?;
        f(entry)
    }

    fn lexical(&self) -> LexicalArtifactStore {
        LexicalArtifactStore::new(&self.lexical_root, self.pool.clone())
    }

    async fn stored_manifest(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<ProjectionGenerationManifest, SearchError> {
        let dto: serde_json::Value = sqlx::query_scalar(
            "SELECT projection_manifest FROM search_generation \
             WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(|error| failed("manifest read", error))?;
        let manifest: ProjectionGenerationManifest =
            serde_json::from_value(dto.get("manifest").cloned().unwrap_or_default())
                .map_err(|error| failed("manifest decode", error))?;
        if manifest.key() != key {
            return Err(failed("manifest key", key));
        }
        Ok(manifest)
    }

    async fn current_key(
        &self,
        source_id: SourceId,
    ) -> Result<Option<ProjectionGenerationKey>, SearchError> {
        let current: Option<Option<Uuid>> = sqlx::query_scalar(
            "SELECT current_generation_id FROM search_source_coordination WHERE source_id=$1",
        )
        .bind(source_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| failed("current read", error))?;
        Ok(current.flatten().map(|generation| ProjectionGenerationKey {
            source_id,
            generation_id: ProjectionGenerationId::from_uuid(generation),
        }))
    }

    /// The guard holder gives up a build; the files and rows go with it.
    async fn abort(&self, key: ProjectionGenerationKey) -> Result<bool, SearchError> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| SearchError::OperationFailed("durable runtime lock poisoned".into()))?
            .remove(&key);
        let _ = std::fs::remove_dir_all(self.lexical().staging_dir(key));
        let Some(handle) = pending.and_then(|pending| pending.handle) else {
            return Ok(false);
        };
        PgGenerationGc::new(self.pool.clone(), &self.lexical_root)
            .abort_manual(&handle)
            .await
            .map_err(|error| failed("abort", error))?;
        Ok(true)
    }

    async fn settle(&self, key: ProjectionGenerationKey) -> Result<VerifiedBundle, SearchError> {
        let (manifest, registry, resources, lexical, graph, unit_manifest, coverage) =
            self.with(key, |pending| {
                Ok((
                    pending.manifest.clone(),
                    pending.registry.clone(),
                    pending.resources.clone(),
                    pending.lexical,
                    pending.graph.clone(),
                    pending.unit_manifest.clone(),
                    pending.coverage.clone(),
                ))
            })?;
        let (
            Some(manifest),
            Some(registry),
            Some(lexical),
            Some(graph),
            Some(unit_manifest),
            Some(coverage),
        ) = (manifest, registry, lexical, graph, unit_manifest, coverage)
        else {
            return Err(SearchError::OperationFailed(
                "durable Document build needs every artifact and a body bundle".into(),
            ));
        };
        let handle = Arc::new(
            self.registrar
                .register_manual_with_graph(
                    &FullBuildRequest {
                        manifest: manifest.clone(),
                        expected_snapshot: manifest.source_snapshot.clone(),
                    },
                    &graph.mapping_digest,
                    self.guard_ttl,
                )
                .await
                .map_err(|error| failed("registration", error))?,
        );
        self.with(key, |pending| {
            pending.handle = Some(handle.clone());
            Ok(())
        })?;
        let store = self.lexical();
        store
            .finalize(
                &manifest,
                &self.source,
                &store.staging_dir(key),
                &lexical,
                &unit_manifest,
            )
            .await
            .map_err(|error| failed("lexical finalize", error))?;
        let receipt = compute_bundle_receipt(
            key,
            &manifest.source_snapshot,
            &manifest.digest,
            &unit_manifest,
            &coverage,
            lexical,
            graph_receipt(key, &resources, &graph.owners)?,
        )
        .map_err(|error| failed("bundle receipt", error))?;
        PgPayloadStore::new(self.pool.clone())
            .store(&StoredBundleV1 {
                manifest: manifest.clone(),
                projection: ProjectionPayloadV1 {
                    resources,
                    registry,
                },
                unit_manifest,
                coverage,
                receipt: receipt.clone(),
            })
            .await
            .map_err(|error| failed("payload", error))?;
        let target = handle
            .graph_target()
            .ok_or_else(|| failed("graph target", key))?;
        PostgresGraphStore::new(self.pool.clone())
            .stage_full(&target, &graph.records, &graph.relations)
            .await
            .map_err(|error| failed("graph stage", error))?;
        let verified =
            ReadyCoordinator::new(self.pool.clone(), &self.lexical_root, self.source.clone())
                .ready_manual(&handle)
                .await
                .map_err(|error| failed("READY", error))?;
        if verified.receipt() != &receipt {
            return Err(failed("READY receipt", key));
        }
        self.with(key, |pending| {
            pending.verified = Some(verified.clone());
            Ok(())
        })?;
        Ok(verified)
    }
}

impl DocumentIndexRuntime for PgDocumentIndexRuntime {
    fn pin_current<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        Box::pin(async move {
            match self.current_key(source_id).await? {
                Some(key) => Ok(Some(self.stored_manifest(key).await?)),
                None => Ok(None),
            }
        })
    }

    fn begin_generation<'a>(
        &'a self,
        manifest: PersistableGenerationManifest,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let manifest = manifest.manifest().clone();
            if manifest.source_id != self.source.source_id {
                return Err(failed("Source", manifest.source_id));
            }
            let mut pending = self.pending.lock().map_err(|_| {
                SearchError::OperationFailed("durable runtime lock poisoned".into())
            })?;
            if pending.contains_key(&manifest.key()) {
                return Err(failed("duplicate build", manifest.key()));
            }
            pending.insert(
                manifest.key(),
                Pending {
                    manifest: Some(manifest),
                    ..Pending::default()
                },
            );
            Ok(())
        })
    }

    fn bind_source_snapshot<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        snapshot: &'a DocumentOutboxSnapshot,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.with(key, |pending| {
                pending.snapshot = Some(snapshot.clone());
                Ok(())
            })
        })
    }

    fn stage_concept_registry<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        registry: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.with(key, |pending| {
                pending.registry = Some(registry);
                Ok(())
            })
        })
    }

    fn stage_resource<'a>(&'a self, resource: PersistableResourceProjection) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let projection = resource.projection().clone();
            self.with(projection.manifest.key(), |pending| {
                pending.resources.push(projection);
                Ok(())
            })
        })
    }

    fn validate_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.with(key, |pending| {
                let (Some(manifest), Some(registry)) = (&pending.manifest, &pending.registry)
                else {
                    return Err(failed("generation", "manifest or registry missing"));
                };
                if pending.resources.len() as u64 != manifest.resource_count
                    || generation_digest(manifest.source_id, &pending.resources, registry)?
                        != manifest.digest
                {
                    return Err(failed("generation digest", key));
                }
                Ok(())
            })
        })
    }

    fn publish_if_current<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        expected_current: Option<ProjectionGenerationKey>,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let (handle, verified) = self.with(key, |pending| {
                Ok((pending.handle.clone(), pending.verified.clone()))
            })?;
            let (Some(handle), Some(verified)) = (handle, verified) else {
                return Err(SearchError::OperationFailed(
                    "durable Document publication needs a READY bundle".into(),
                ));
            };
            let publication = PgPublication::new(self.pool.clone());
            let current = publication
                .current(key.source_id)
                .await
                .map_err(|error| failed("current", error))?;
            if current.key != expected_current {
                return Ok(false);
            }
            let outcome = publication
                .publish_manual(&handle, &verified, &current)
                .await
                .map_err(|error| failed("publication", error))?;
            if outcome != SearchCompletionOutcome::Published(key) {
                return Ok(false);
            }
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&key);
            }
            Ok(true)
        })
    }

    fn fail_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.abort(key).await.map(|_| ()) })
    }

    fn discard_projection_generation<'a>(
        &'a self,
        key: ProjectionGenerationKey,
    ) -> BoxFuture<'a, bool> {
        Box::pin(self.abort(key))
    }

    fn build_lexical_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        input: LexicalBuildInput,
    ) -> Result<(), LexicalIndexError> {
        let key = manifest.key();
        let logical = search_tantivy::lexical_input_digest(&input)?;
        let dir = self.lexical().staging_dir(key);
        TantivyLexicalIndex::new().build_generation_at(manifest, source, input, &dir)?;
        self.with(key, |pending| {
            pending.lexical = Some(ArtifactReceipt {
                key,
                digest: logical.digest,
                count: logical.count,
            });
            Ok(())
        })
        .map_err(|_| LexicalIndexError::UnknownGeneration)
    }

    fn discard_lexical_generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<bool, LexicalIndexError> {
        Ok(std::fs::remove_dir_all(self.lexical().staging_dir(key)).is_ok())
    }

    fn build_graph_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        _source: &DiscoverableSource,
        projections: Vec<CompiledResourceProjection>,
        ownership: Vec<(ResourceId, DocumentId)>,
    ) -> Result<(), SearchError> {
        let key = manifest.key();
        let snapshot = self
            .with(key, |pending| Ok(pending.snapshot.clone()))?
            .ok_or_else(|| failed("graph", "no bound Source snapshot"))?;
        let (records, mapping_digest) =
            document_graph_records(manifest.source_id, &snapshot, &projections)?;
        // The indexer's structural owners must be the snapshot-bound ones.
        let derived: BTreeSet<(ResourceId, Uuid)> = records
            .iter()
            .filter_map(|record| match &record.mapping {
                GraphSourceMapping::Document { document_id }
                | GraphSourceMapping::FolderPlacement { document_id, .. } => {
                    Some((record.resource_ref, *document_id))
                }
                _ => None,
            })
            .collect();
        let given: BTreeSet<(ResourceId, Uuid)> = ownership
            .iter()
            .map(|(resource, document)| (*resource, document.as_uuid()))
            .collect();
        if derived != given || given.len() != ownership.len() {
            return Err(failed("graph owners", key));
        }
        let relations = graph_relations(&records);
        self.with(key, |pending| {
            pending.graph = Some(GraphPlan {
                records,
                relations,
                mapping_digest,
                owners: ownership,
            });
            Ok(())
        })
    }

    fn discard_graph_generation(&self, _key: ProjectionGenerationKey) -> Result<bool, SearchError> {
        // The Graph rows are removed by the guard holder's abort.
        Ok(false)
    }

    fn stage_body_unit_manifest<'a>(&'a self, manifest: BodyUnitManifest) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.with(manifest.key, |pending| {
                pending.unit_manifest = Some(manifest);
                Ok(())
            })
        })
    }

    fn stage_body_coverage<'a>(&'a self, artifact: BodyCoverageArtifact) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.with(artifact.key, |pending| {
                pending.coverage = Some(artifact);
                Ok(())
            })
        })
    }

    fn validate_bundle<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        projection_digest: String,
    ) -> BoxFuture<'a, GenerationBundleReceipt> {
        Box::pin(async move {
            let expected = self.with(key, |pending| {
                Ok(pending.manifest.as_ref().map(|m| m.digest.clone()))
            })?;
            if expected.as_deref() != Some(projection_digest.as_str()) {
                return Err(failed("projection digest", key));
            }
            Ok(self.settle(key).await?.receipt().clone())
        })
    }

    fn pin_current_bundle<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<(ProjectionGenerationManifest, GenerationBundleReceipt)>> {
        Box::pin(async move {
            let Some(key) = self.current_key(source_id).await? else {
                return Ok(None);
            };
            let dto: Option<serde_json::Value> = sqlx::query_scalar(
                "SELECT receipt_dto FROM search_generation_receipt \
                 WHERE source_id=$1 AND generation_id=$2",
            )
            .bind(key.source_id.as_uuid())
            .bind(key.generation_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| failed("receipt read", error))?;
            let Some(dto) = dto else {
                return Ok(None);
            };
            let receipt: GenerationBundleReceipt =
                serde_json::from_value(dto.get("bundle").cloned().unwrap_or_default())
                    .map_err(|error| failed("receipt decode", error))?;
            Ok(Some((self.stored_manifest(key).await?, receipt)))
        })
    }

    fn discard_body_generation<'a>(&'a self, _key: ProjectionGenerationKey) -> BoxFuture<'a, bool> {
        Box::pin(async { Ok(false) })
    }
}
