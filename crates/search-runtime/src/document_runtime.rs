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
//!
//! B6: when the Source already has a READY current generation, the target
//! is registered INCREMENTAL instead. The Graph copies the base and applies
//! one closure-proved delta, while the payload and lexical directory are the
//! complete new bundle, so the published generation is self-contained. The
//! choice is made before registration; a target that fails after it is
//! aborted and the Source's next build is full.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use document_domain::DocumentId;
use search_application::SearchError;
use search_application::graph_generation::{
    BuildGuardHandle, ClosureBasis, GraphBatchCursor, GraphBatchPhase, GraphGenerationReceipt,
    GraphIncrementalDelta, GraphRelationClosureProof, GraphResourceRecord, GraphSourceMapping,
};
use search_application::indexing_service::DocumentSourceEvent;
use search_application::ports::{
    BoxFuture, SearchCompletionOutcome, SearchDeliveryFence, SemanticRegistrySnapshot,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_application::search_core::id::{
    ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
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
use crate::full_guard::{EventCandidateHandle, FullGuardTtl, ManualBuildHandle};
use crate::gc::PgGenerationGc;
use crate::generation_registration::{FullBuildRequest, GenerationError, PgGenerationRegistrar};
use crate::lexical_artifact::LexicalArtifactStore;
use crate::payload::{PgPayloadStore, ProjectionPayloadV1, StoredBundleV1};
use crate::ready::{ReadyCoordinator, VerifiedBundle};

fn failed(what: &str, error: impl Debug) -> SearchError {
    SearchError::OperationFailed(format!("durable Document {what}: {error:?}"))
}

/// A registration refused because a delivery or Source fence moved on is a
/// lost fence, not an indexing failure.
fn registration_error(error: GenerationError) -> SearchError {
    match error {
        GenerationError::Lost => SearchError::FenceLost,
        other => failed("registration", other),
    }
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
    delivery: Option<(DocumentSourceEvent, SearchDeliveryFence)>,
    handle: Option<Registered>,
    verified: Option<VerifiedBundle>,
}

/// The P7 target: MANUAL for a rebuild, EVENT for a fenced delivery.
#[derive(Clone)]
enum Registered {
    Manual(Arc<ManualBuildHandle>),
    Event(Arc<EventCandidateHandle>),
    /// B6: an INCREMENTAL target under its Graph build guard.
    Incremental(BuildGuardHandle),
}

impl Registered {
    fn graph_target(
        &self,
    ) -> Option<search_application::graph_generation::RegisteredFullBuildHandle> {
        match self {
            Self::Manual(handle) => handle.graph_target(),
            Self::Event(handle) => handle.graph_target(),
            Self::Incremental(_) => None,
        }
    }
}

/// Graph rows copied from the base per batch.
const GRAPH_COPY_BATCH: u32 = 1_000;

fn without_relations(record: &GraphResourceRecord) -> GraphResourceRecord {
    GraphResourceRecord {
        attached_relations: Vec::new(),
        ..record.clone()
    }
}

/// The closure-proved delta from a base Graph to the target plan. Every
/// Resource whose own row or any incident relation changed is affected, and
/// every relation incident to an affected Resource is removed from the copy
/// and written again when it remains, so the closure holds by construction.
fn incremental_delta(
    base: &GraphGenerationReceipt,
    base_records: &[GraphResourceRecord],
    base_relations: &[TypedRelationInstance],
    target_snapshot: &str,
    plan: &GraphPlan,
) -> GraphIncrementalDelta {
    let old: BTreeMap<RelationId, &TypedRelationInstance> = base_relations
        .iter()
        .map(|relation| (relation.relation_id, relation))
        .collect();
    let new: BTreeMap<RelationId, &TypedRelationInstance> = plan
        .relations
        .iter()
        .map(|relation| (relation.relation_id, relation))
        .collect();
    let touched: BTreeSet<RelationId> = old
        .iter()
        .filter(|(id, relation)| new.get(id) != Some(relation))
        .map(|(id, _)| *id)
        .chain(new.keys().filter(|id| !old.contains_key(id)).copied())
        .collect();
    let participants = |relation: &TypedRelationInstance| -> Vec<ResourceId> {
        relation
            .participants
            .iter()
            .map(|participant| participant.resource_ref)
            .collect()
    };
    let in_touched: BTreeSet<ResourceId> = touched
        .iter()
        .flat_map(|id| {
            old.get(id)
                .map(|relation| participants(relation))
                .into_iter()
                .chain(new.get(id).map(|relation| participants(relation)))
                .flatten()
        })
        .collect();
    let base_rows: BTreeMap<ResourceId, GraphResourceRecord> = base_records
        .iter()
        .map(|record| (record.resource_ref, without_relations(record)))
        .collect();
    let target_ids: BTreeSet<ResourceId> = plan
        .records
        .iter()
        .map(|record| record.resource_ref)
        .collect();
    let changed_resources: Vec<GraphResourceRecord> = plan
        .records
        .iter()
        .filter(|record| {
            base_rows.get(&record.resource_ref) != Some(&without_relations(record))
                || in_touched.contains(&record.resource_ref)
        })
        .cloned()
        .collect();
    let retired_resources: Vec<ResourceId> = base_rows
        .keys()
        .filter(|id| !target_ids.contains(id))
        .copied()
        .collect();
    let affected: BTreeSet<ResourceId> = changed_resources
        .iter()
        .map(|record| record.resource_ref)
        .chain(retired_resources.iter().copied())
        .collect();
    let incident: BTreeSet<RelationId> = old
        .iter()
        .filter(|(_, relation)| {
            relation
                .participants
                .iter()
                .any(|participant| affected.contains(&participant.resource_ref))
        })
        .map(|(id, _)| *id)
        .collect();
    let replacement_relations: Vec<TypedRelationInstance> = new
        .iter()
        .filter(|(id, _)| touched.contains(id) || incident.contains(id))
        .map(|(_, relation)| (*relation).clone())
        .collect();
    let changed_relation_ids: Vec<RelationId> = replacement_relations
        .iter()
        .map(|relation| relation.relation_id)
        .filter(|id| old.contains_key(id))
        .collect();
    let retired_relation_ids: Vec<RelationId> = old
        .keys()
        .filter(|id| !new.contains_key(id))
        .copied()
        .collect();
    let old_relation_ids: BTreeSet<RelationId> = incident
        .iter()
        .chain(&changed_relation_ids)
        .chain(&retired_relation_ids)
        .copied()
        .collect();
    GraphIncrementalDelta {
        proof: GraphRelationClosureProof {
            base_snapshot: base.source_snapshot.clone(),
            target_snapshot: target_snapshot.to_owned(),
            affected_resources: affected.into_iter().collect(),
            old_relation_ids: old_relation_ids.into_iter().collect(),
            new_relation_ids: replacement_relations
                .iter()
                .map(|relation| relation.relation_id)
                .collect(),
            basis: ClosureBasis::CompleteEnumeration,
        },
        changed_resources,
        retired_resources,
        changed_relation_ids,
        retired_relation_ids,
        replacement_relations,
        target_source_mapping_digest: plan.mapping_digest.clone(),
    }
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
    /// Set after an incremental target failed: the next build is full.
    next_full: AtomicBool,
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
            next_full: AtomicBool::new(false),
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
        let gc = PgGenerationGc::new(self.pool.clone(), &self.lexical_root);
        match handle {
            Registered::Manual(handle) => gc.abort_manual(&handle).await,
            Registered::Event(handle) => gc.abort_event(&handle).await,
            Registered::Incremental(handle) => gc.abort_incremental(&handle).await,
        }
        .map_err(|error| failed("abort", error))?;
        Ok(true)
    }

    /// The READY current base and its Graph rows, when the target may be
    /// built incrementally from it.
    async fn incremental_base(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<
        Option<(
            GraphGenerationReceipt,
            Vec<GraphResourceRecord>,
            Vec<TypedRelationInstance>,
        )>,
        SearchError,
    > {
        if self.next_full.swap(false, Ordering::SeqCst) {
            return Ok(None);
        }
        let Some(base) = self.current_key(key.source_id).await? else {
            return Ok(None);
        };
        if base == key {
            return Ok(None);
        }
        let manifest = self.stored_manifest(base).await?;
        // A base whose Graph no longer recovers is rebuilt in full.
        Ok(PostgresGraphStore::new(self.pool.clone())
            .recover_rows(base, &manifest.digest)
            .await
            .ok())
    }

    /// Copies the base, applies the delta and settles the bundle READY.
    #[allow(clippy::too_many_arguments)]
    async fn finish_incremental(
        &self,
        handle: BuildGuardHandle,
        manifest: &ProjectionGenerationManifest,
        delta: &GraphIncrementalDelta,
        lexical: ArtifactReceipt,
        resources: Vec<CompiledResourceProjection>,
        registry: SemanticRegistrySnapshot,
        graph: &GraphPlan,
        unit_manifest: BodyUnitManifest,
        coverage: BodyCoverageArtifact,
    ) -> Result<VerifiedBundle, SearchError> {
        let key = manifest.key();
        let store = PostgresGraphStore::new(self.pool.clone());
        let mut position = GraphBatchCursor {
            target_key: key,
            phase: GraphBatchPhase::Copy,
            committed_sequence: 0,
        };
        loop {
            let next = store
                .copy_batch(&handle, &position, GRAPH_COPY_BATCH)
                .await
                .map_err(|error| failed("graph copy", error))?;
            if next == position {
                break;
            }
            position = next;
        }
        store
            .verify_copy(&handle)
            .await
            .map_err(|error| failed("graph copy verification", error))?;
        let size = delta.changed_resources.len()
            + delta.retired_resources.len()
            + delta.changed_relation_ids.len()
            + delta.retired_relation_ids.len()
            + delta.replacement_relations.len();
        store
            .apply_delta_batch(
                &handle,
                delta,
                &GraphBatchCursor {
                    target_key: key,
                    phase: GraphBatchPhase::Delta,
                    committed_sequence: 0,
                },
                u32::try_from(size.max(1)).map_err(|_| failed("graph delta size", size))?,
            )
            .await
            .map_err(|error| failed("graph delta", error))?;
        let artifacts = self.lexical();
        artifacts
            .finalize(
                manifest,
                &self.source,
                &artifacts.staging_dir(key),
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
        let verified =
            ReadyCoordinator::new(self.pool.clone(), &self.lexical_root, self.source.clone())
                .ready_incremental(&handle)
                .await
                .map_err(|error| failed("READY", error))?;
        if verified.receipt() != &receipt {
            return Err(failed("READY receipt", key));
        }
        Ok(verified)
    }

    async fn settle(&self, key: ProjectionGenerationKey) -> Result<VerifiedBundle, SearchError> {
        let (manifest, registry, resources, lexical, graph, unit_manifest, coverage, delivery) =
            self.with(key, |pending| {
                Ok((
                    pending.manifest.clone(),
                    pending.registry.clone(),
                    pending.resources.clone(),
                    pending.lexical,
                    pending.graph.clone(),
                    pending.unit_manifest.clone(),
                    pending.coverage.clone(),
                    pending.delivery.clone(),
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
        let request = FullBuildRequest {
            manifest: manifest.clone(),
            expected_snapshot: manifest.source_snapshot.clone(),
        };
        if let Some((base, base_records, base_relations)) = self.incremental_base(key).await? {
            let delta = incremental_delta(
                &base,
                &base_records,
                &base_relations,
                &manifest.source_snapshot,
                &graph,
            );
            let registered = match &delivery {
                Some((event, fence)) => {
                    self.registrar
                        .register_incremental_event(
                            event,
                            *fence,
                            &base,
                            &request,
                            &graph.mapping_digest,
                            self.guard_ttl,
                        )
                        .await
                }
                None => {
                    self.registrar
                        .register_incremental(
                            &base,
                            &request,
                            &graph.mapping_digest,
                            self.guard_ttl,
                        )
                        .await
                }
            };
            match registered {
                Ok(handle) => {
                    self.with(key, |pending| {
                        pending.handle = Some(Registered::Incremental(handle));
                        Ok(())
                    })?;
                    let verified = match self
                        .finish_incremental(
                            handle,
                            &manifest,
                            &delta,
                            lexical,
                            resources,
                            registry,
                            &graph,
                            unit_manifest,
                            coverage,
                        )
                        .await
                    {
                        Ok(verified) => verified,
                        Err(error) => {
                            self.next_full.store(true, Ordering::SeqCst);
                            return Err(error);
                        }
                    };
                    self.with(key, |pending| {
                        pending.verified = Some(verified.clone());
                        Ok(())
                    })?;
                    return Ok(verified);
                }
                // The base retired between the read and the registration.
                Err(GenerationError::Lost) => {}
                Err(error) => return Err(registration_error(error)),
            }
        }
        let handle = match &delivery {
            Some((event, fence)) => Registered::Event(Arc::new(
                self.registrar
                    .register_event_with_graph(
                        event,
                        *fence,
                        &request,
                        &graph.mapping_digest,
                        self.guard_ttl,
                    )
                    .await
                    .map_err(registration_error)?,
            )),
            None => Registered::Manual(Arc::new(
                self.registrar
                    .register_manual_with_graph(&request, &graph.mapping_digest, self.guard_ttl)
                    .await
                    .map_err(registration_error)?,
            )),
        };
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
        let coordinator =
            ReadyCoordinator::new(self.pool.clone(), &self.lexical_root, self.source.clone());
        let verified = match &handle {
            Registered::Manual(handle) => coordinator.ready_manual(handle).await,
            Registered::Event(handle) => coordinator.ready_event(handle).await,
            Registered::Incremental(handle) => coordinator.ready_incremental(handle).await,
        }
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

    fn bind_delivery<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        event: &'a DocumentSourceEvent,
        fence: SearchDeliveryFence,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.with(key, |pending| {
                pending.delivery = Some((event.clone(), fence));
                Ok(())
            })
        })
    }

    fn settled<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&key);
            }
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
                    "durable Document publication needs a READY MANUAL bundle".into(),
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
            let outcome = match handle {
                Registered::Manual(handle) => {
                    publication
                        .publish_manual(&handle, &verified, &current)
                        .await
                }
                Registered::Incremental(handle) => {
                    publication
                        .publish_incremental_manual(&handle, &verified, &current)
                        .await
                }
                Registered::Event(_) => {
                    return Err(SearchError::OperationFailed(
                        "durable Document publication needs a READY MANUAL bundle".into(),
                    ));
                }
            }
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
