//! E (P2-07): Vector over the durable Document Source.
//!
//! The maintainer builds the Vector generation of the Source's current READY
//! P1 bundle by Unit segment — one authority scope per Document Source,
//! persistent retention — and publishes it through the P2-06 lifecycle CAS.
//! A Vector segment whose Unit segment and authority are unchanged is listed
//! as it is; a new one reuses stored values by cache key and embeds the rest.
//! Each segment is checked once per process (manifest v2, SD-T11 5), so a
//! build reads only the Units of segments it has not checked. A hit is
//! resolved against the loaded generation's own Units and the actor's
//! current Document Read before it can become a candidate.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use search_application::SearchError;
use search_application::ports::{AccessDecision, BoxFuture, CurrentAccessEvaluatorPort};
use search_application::scoped::AuthorizedSourceScope;
use search_application::search_core::id::{ProjectionGenerationId, SourceId};
use search_application::search_core::knowledge_unit::{
    ContentPartRef, EmbeddingCacheKey, KnowledgeUnit, ResourceVersionRef, VectorAuthorityInput,
    VectorHitRef, restamp_source_snapshot,
};
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_application::search_core::source::{DiscoverableSource, RetentionMode};
use search_application::search_core::vector::{
    BoundEmbedding, EmbeddingModelId, VectorActivationPolicy, VectorEntryRef,
    VectorManifestHeader, VectorManifestUnit, VectorProjectionManifest, VectorSegmentCheck,
    VectorStorageKind, VectorUnitCoverage, check_segment,
};
use search_application::vector::{
    CurrentSourceUnit, EmbeddingProvider, SourceUnitState, TrustedVectorQuery, VectorActivation,
    VectorActivationPort, VectorBuildOutcome, VectorGenerationPort, VectorIndexPort,
    VectorRetrievalBatch, VectorRetriever, VectorSourceResolverPort,
};
use search_extraction_core::{BodyCoverage, ItemOperationState};
use search_source_document::{BodyItemEntry, DocumentCurrentAccessAdapter, GenerationBundleReceipt};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::payload::PgPayloadStore;
use crate::vector_store::{PgVectorGenerations, PgVectorIndex, cache_digest, segment_digest_for};

/// The one authority scope of a Document Source's embeddings: Read is
/// checked per hit, the scope only partitions purge and epochs.
pub fn document_scope_key(source: SourceId) -> String {
    format!("document-source:{}", source.as_uuid())
}

pub const DOCUMENT_RETENTION_LEASE: &str = "document-persistent";

/// The pre-registered calibration's similarity floor for the pinned E5 model
/// (MIRACL ja dev calibration queries; see the E measurement report).
pub const DEFAULT_SIMILARITY_FLOOR: f32 = 0.89;

/// Unit segments read per batch by a build, so the Unit text of one batch at
/// most is held at a time.
const BUILD_BATCH: usize = 256;

fn hex(bytes: &[u8; 32]) -> String {
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

fn failed(what: &str, error: impl std::fmt::Debug) -> SearchError {
    SearchError::SourceUnavailable(format!("vector: {what}: {error:?}"))
}

/// The pinned authority of one indexed Unit of `key`'s bundle.
fn unit_authority(key: ProjectionGenerationKey, unit: &KnowledgeUnit) -> VectorAuthorityInput {
    VectorAuthorityInput {
        generation: key,
        version: (*unit.version).clone(),
        part: (*unit.part).clone(),
        authoritative_representation_ref: unit.provenance.authoritative_representation_ref.clone(),
        raw: unit.provenance.raw.clone(),
        profile: unit.provenance.profile.clone(),
        authority_scope_key: document_scope_key(key.source_id),
        retention_lease_id: DOCUMENT_RETENTION_LEASE.into(),
        lifetime_scope_id: String::new(),
        retention_mode: RetentionMode::PersistentResource,
        lease_expires_at: None,
    }
}

/// Whether an item's Units are indexed, and with which coverage.
fn indexed_coverage(
    operation: ItemOperationState,
    coverage: Option<&BodyCoverage>,
) -> Option<VectorUnitCoverage> {
    if operation != ItemOperationState::Completed {
        return None;
    }
    match coverage? {
        BodyCoverage::Supported => Some(VectorUnitCoverage::Complete),
        BodyCoverage::Partial { .. } => Some(VectorUnitCoverage::PartialValidated),
        BodyCoverage::Unsupported { .. } => None,
    }
}

/// Every Document Source registered with Vector uses the one selected model.
pub struct RegisteredVectorActivation {
    enabled: BTreeMap<SourceId, VectorActivation>,
}

impl RegisteredVectorActivation {
    pub fn new(
        sources: impl IntoIterator<Item = SourceId>,
        provider: &dyn EmbeddingProvider,
    ) -> Self {
        Self {
            enabled: sources
                .into_iter()
                .map(|source| {
                    (
                        source,
                        VectorActivation {
                            policy: VectorActivationPolicy::EligibleOptIn,
                            model: provider.spec().clone(),
                        },
                    )
                })
                .collect(),
        }
    }
}

impl VectorActivationPort for RegisteredVectorActivation {
    fn activation<'a>(&'a self, source: SourceId) -> BoxFuture<'a, Option<VectorActivation>> {
        Box::pin(async move { Ok(self.enabled.get(&source).cloned()) })
    }
}

/// The Vector services shared by every request and build.
#[derive(Clone)]
pub struct VectorServices {
    pub provider: Arc<dyn EmbeddingProvider>,
    pub index: Arc<PgVectorIndex>,
    pub generations: Arc<PgVectorGenerations>,
    pub activations: Arc<dyn VectorActivationPort>,
}

impl VectorServices {
    fn model(&self) -> Result<EmbeddingModelId, SearchError> {
        self.provider
            .spec()
            .validate_and_id()
            .map_err(|error| failed("model", error))
    }
}

fn cache_key(model: &EmbeddingModelId, item: &VectorManifestUnit) -> EmbeddingCacheKey {
    EmbeddingCacheKey {
        embedding_model_id: model.to_string(),
        unit_id: item.unit.unit_id,
        text_sha256: item.unit.text_sha256,
        profile: item.unit.provenance.profile.clone(),
        source_id: item.unit.version.source_id,
        authority_scope_key: item.authority.authority_scope_key.clone(),
        retention_lease_id: item.authority.retention_lease_id.clone(),
        lifetime_scope_id: item.authority.lifetime_scope_id.clone(),
    }
}

/// One indexed item of a bundle: its Unit and Vector segment digests.
struct IndexedItem {
    unit_segment: String,
    vector_segment: String,
    unit_count: u64,
    coverage: VectorUnitCoverage,
}

/// A bundle's Vector header and its indexed items in manifest order, read
/// from segment summaries without the Units.
struct BundleItems {
    header: VectorManifestHeader,
    items: Vec<IndexedItem>,
}

/// How a pass treats a segment whose Vector segment is not stored.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Missing {
    /// Store it from stored values and fresh embeddings.
    Embed,
    /// Stop: the stage being checked is incomplete.
    Fail,
}

/// Builds and recovers the Vector generation of the current P1 bundle.
pub struct VectorMaintainer {
    pool: PgPool,
    source: DiscoverableSource,
    services: VectorServices,
    /// Checked segments by Vector segment digest. A check names no
    /// generation, so it holds for every bundle that lists the segment.
    checks: Mutex<HashMap<String, Arc<VectorSegmentCheck>>>,
}

impl VectorMaintainer {
    pub fn new(pool: PgPool, source: DiscoverableSource, services: VectorServices) -> Self {
        Self {
            pool,
            source,
            services,
            checks: Mutex::new(HashMap::new()),
        }
    }

    async fn current_key(&self) -> Result<Option<ProjectionGenerationKey>, SearchError> {
        let current: Option<Option<Uuid>> = sqlx::query_scalar(
            "SELECT current_generation_id FROM search_source_coordination WHERE source_id=$1",
        )
        .bind(self.source.source_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| failed("current", error))?;
        Ok(current.flatten().map(|generation| ProjectionGenerationKey {
            source_id: self.source.source_id,
            generation_id: ProjectionGenerationId::from_uuid(generation),
        }))
    }

    /// The Vector header of `key`'s READY bundle and its indexed items.
    async fn bundle(
        &self,
        key: ProjectionGenerationKey,
        model: &EmbeddingModelId,
    ) -> Result<BundleItems, SearchError> {
        let row: (serde_json::Value, serde_json::Value) = sqlx::query_as(
            "SELECT g.projection_manifest, r.receipt_dto FROM search_generation g \
             JOIN search_generation_receipt r USING (source_id, generation_id) \
             WHERE g.source_id=$1 AND g.generation_id=$2 AND g.state='READY'",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(|error| failed("bundle", error))?;
        let manifest: ProjectionGenerationManifest =
            serde_json::from_value(row.0.get("manifest").cloned().unwrap_or_default())
                .map_err(|error| failed("manifest", error))?;
        let receipt: GenerationBundleReceipt =
            serde_json::from_value(row.1.get("bundle").cloned().unwrap_or_default())
                .map_err(|error| failed("receipt", error))?;
        if manifest.key() != key || receipt.key != key {
            return Err(failed("bundle key", key));
        }
        let summary = PgPayloadStore::new(self.pool.clone())
            .restore_without_units(&manifest)
            .await
            .map_err(|error| failed("payload", error))?;
        if summary.coverage.items.len() != summary.units.segments.len() {
            return Err(failed("segment list", summary.units.segments.len()));
        }
        let header = VectorManifestHeader {
            bundle_key: key,
            body_receipt_digest: hex(&receipt.composite_digest),
            source_snapshot: manifest.source_snapshot.clone(),
            authority_scope_key: document_scope_key(key.source_id),
            retention_lease_id: DOCUMENT_RETENTION_LEASE.into(),
            lease_expires_at: None,
            source_declares_persistent_embedding_permission: true,
            nonindexed_retention_unit_ids: vec![],
            lexical_analyzer_revision: receipt.lexical_schema_version.clone(),
            graph_schema_revision: search_graph::GRAPH_SCHEMA_VERSION.into(),
        };
        let items = summary
            .coverage
            .items
            .iter()
            .zip(summary.units.segments)
            .filter_map(|(item, unit_segment)| {
                let coverage = indexed_coverage(item.operation, item.coverage.as_ref())?;
                Some(IndexedItem {
                    vector_segment: segment_digest_for(
                        model,
                        &unit_segment,
                        &header.authority_scope_key,
                        &header.retention_lease_id,
                        "",
                    ),
                    unit_segment,
                    unit_count: u64::from(item.unit_count),
                    coverage,
                })
            })
            .collect();
        Ok(BundleItems { header, items })
    }

    /// The indexed Units of a batch of items, read from their Unit segments
    /// and bound to `header`'s bundle.
    async fn batch_units(
        &self,
        header: &VectorManifestHeader,
        batch: &[&IndexedItem],
    ) -> Result<Vec<Vec<VectorManifestUnit>>, SearchError> {
        let mut digests: Vec<String> = batch.iter().map(|item| item.unit_segment.clone()).collect();
        digests.sort();
        digests.dedup();
        let segments = PgPayloadStore::new(self.pool.clone())
            .unit_segments(&digests)
            .await
            .map_err(|error| failed("unit segments", error))?;
        batch
            .iter()
            .map(|item| {
                let mut entry: BodyItemEntry = segments
                    .get(&item.unit_segment)
                    .cloned()
                    .ok_or_else(|| failed("unit segment", ()))?;
                if entry.units.len() as u64 != item.unit_count {
                    return Err(failed("unit count", item.unit_count));
                }
                restamp_source_snapshot(&mut entry.units, &header.source_snapshot);
                Ok(entry
                    .units
                    .into_iter()
                    .map(|unit| VectorManifestUnit {
                        authority: unit_authority(header.bundle_key, &unit),
                        unit,
                        coverage: item.coverage,
                    })
                    .collect())
            })
            .collect()
    }

    /// The entries of a new Vector segment: stored values by cache key, the
    /// rest embedded; the segment is stored before it is returned.
    async fn embed_segment(
        &self,
        model: &EmbeddingModelId,
        header: &VectorManifestHeader,
        segment: &str,
        units: &[VectorManifestUnit],
    ) -> Result<Vec<VectorEntryRef>, SearchError> {
        let spec = self.services.provider.spec();
        let index = self.services.index.as_ref();
        let keys = units
            .iter()
            .map(|item| cache_digest(&cache_key(model, item)))
            .collect::<Result<Vec<_>, _>>()?;
        let stored = index.cached_values(model, &keys).await?;
        let mut missing = Vec::new();
        let mut embeddings = Vec::with_capacity(units.len());
        for (item, key) in units.iter().zip(&keys) {
            match stored.get(key) {
                Some(values) => embeddings.push(Some(
                    BoundEmbedding::new(spec, &item.unit, &item.authority, values.clone())
                        .map_err(|error| failed("rebinding", error))?,
                )),
                None => {
                    missing.push(item.clone());
                    embeddings.push(None);
                }
            }
        }
        let mut fresh = self
            .services
            .provider
            .embed_units(&missing)
            .await?
            .into_iter();
        if fresh.len() != missing.len() {
            return Err(failed("embedding count", missing.len()));
        }
        let embeddings = embeddings
            .into_iter()
            .zip(units)
            .map(|(embedding, item)| match embedding {
                Some(embedding) => Ok(embedding),
                None => {
                    let embedding = fresh.next().ok_or_else(|| failed("embedding", ()))?;
                    embedding
                        .validate_binding(spec, &item.unit, &item.authority)
                        .map_err(|error| failed("embedding binding", error))?;
                    Ok(embedding)
                }
            })
            .collect::<Result<Vec<_>, SearchError>>()?;
        index
            .put_segment(segment, model, &header.authority_scope_key, &embeddings)
            .await?;
        Ok(embeddings.iter().map(BoundEmbedding::entry_ref).collect())
    }

    /// The check of every item's segment, in item order. Segments this
    /// process checked already are not read again; the rest are read by
    /// batch, their Vector segment stored if `missing` allows, and checked.
    /// `None` when a segment fails its check or may not be stored.
    async fn segment_checks(
        &self,
        model: &EmbeddingModelId,
        bundle: &BundleItems,
        missing: Missing,
    ) -> Result<Option<Vec<VectorSegmentCheck>>, SearchError> {
        let index = self.services.index.as_ref();
        let digests: Vec<String> = bundle
            .items
            .iter()
            .map(|item| item.vector_segment.clone())
            .collect();
        let existing = index.existing_segments(&digests).await?;
        let pending: Vec<&IndexedItem> = {
            let mut checks = self.checks.lock().map_err(|_| failed("lock", ()))?;
            // A check outlives its segment only until the segment is gone.
            checks.retain(|digest, _| existing.contains(digest));
            let mut seen = HashSet::new();
            bundle
                .items
                .iter()
                .filter(|item| {
                    !checks.contains_key(&item.vector_segment)
                        && seen.insert(item.vector_segment.as_str())
                })
                .collect()
        };
        if missing == Missing::Fail
            && pending
                .iter()
                .any(|item| !existing.contains(&item.vector_segment))
        {
            return Ok(None);
        }
        for batch in pending.chunks(BUILD_BATCH) {
            let units = self.batch_units(&bundle.header, batch).await?;
            let stored: Vec<String> = batch
                .iter()
                .filter(|item| existing.contains(&item.vector_segment))
                .map(|item| item.vector_segment.clone())
                .collect();
            let mut stored = index
                .listed_entries(bundle.header.bundle_key, model, &stored)
                .await?;
            for (item, units) in batch.iter().zip(units) {
                let entries = match stored.remove(&item.vector_segment) {
                    Some(entries) => entries,
                    None => {
                        self.embed_segment(model, &bundle.header, &item.vector_segment, &units)
                            .await?
                    }
                };
                let Ok(check) = check_segment(
                    model,
                    &bundle.header,
                    VectorStorageKind::Persistent,
                    &units,
                    &entries,
                ) else {
                    return Ok(None);
                };
                self.checks
                    .lock()
                    .map_err(|_| failed("lock", ()))?
                    .insert(item.vector_segment.clone(), Arc::new(check));
            }
        }
        let checks = self.checks.lock().map_err(|_| failed("lock", ()))?;
        Ok(bundle
            .items
            .iter()
            .map(|item| checks.get(&item.vector_segment).map(|check| (**check).clone()))
            .collect())
    }

    /// Builds the current bundle's Vector generation unless it is published
    /// already; `None` when the Source has no Vector or no current bundle.
    pub async fn ensure_current(&self) -> Result<Option<VectorBuildOutcome>, SearchError> {
        if self
            .services
            .activations
            .activation(self.source.source_id)
            .await?
            .is_none_or(|activation| activation.policy != VectorActivationPolicy::EligibleOptIn)
        {
            return Ok(None);
        }
        let Some(key) = self.current_key().await? else {
            return Ok(None);
        };
        let model = self.services.model()?;
        if self
            .services
            .generations
            .pin_current(key, &model)
            .await?
            .is_some()
        {
            return Ok(None);
        }
        // The epoch is read before the input is captured.
        let scope_epoch = self
            .services
            .generations
            .scope_epoch(&document_scope_key(key.source_id))
            .await?;
        let bundle = self.bundle(key, &model).await?;
        self.build(&model, &bundle, scope_epoch).await.map(Some)
    }

    /// One build of `bundle` by segment: every segment is checked, the stage
    /// lists them in order, and the manifest composes their checks.
    async fn build(
        &self,
        model: &EmbeddingModelId,
        bundle: &BundleItems,
        scope_epoch: u64,
    ) -> Result<VectorBuildOutcome, SearchError> {
        let spec = self.services.provider.spec();
        let index = self.services.index.as_ref();
        let Some(checks) = self.segment_checks(model, bundle, Missing::Embed).await? else {
            return Ok(VectorBuildOutcome::Rejected);
        };
        let segments: Vec<String> = bundle
            .items
            .iter()
            .map(|item| item.vector_segment.clone())
            .collect();
        let descriptor = index
            .stage_listed(bundle.header.bundle_key, model, &segments)
            .await?;
        let checked = VectorProjectionManifest::stage_segments(
            spec,
            &bundle.header,
            descriptor.clone(),
            &checks,
            VectorStorageKind::Persistent,
            OffsetDateTime::now_utc(),
        )
        .and_then(|manifest| {
            manifest
                .validate_segments(&bundle.header, &checks)
                .map(|receipt| (manifest, receipt))
        });
        let (manifest, receipt) = match checked {
            Ok(checked) => checked,
            Err(_) => {
                index.discard(&descriptor).await?;
                return Ok(VectorBuildOutcome::Rejected);
            }
        };
        let published = match self
            .services
            .generations
            .publish_if_current(&manifest, scope_epoch)
            .await
        {
            Ok(published) => published,
            Err(error) => {
                // The unpublished stage never outlives a failed publication.
                let _ = index.discard(&descriptor).await;
                return Err(error);
            }
        };
        if !published {
            index.discard(&descriptor).await?;
            return Ok(VectorBuildOutcome::LostCas);
        }
        Ok(VectorBuildOutcome::Published(Box::new(receipt)))
    }

    /// The Units of the current generation that a build would embed: their
    /// Vector segment is not stored and neither is their value. Each comes
    /// with the cache digest its value is stored under, for values the same
    /// model computed elsewhere (a validation import).
    pub async fn unstored_units(&self) -> Result<Vec<(String, String)>, SearchError> {
        let Some(key) = self.current_key().await? else {
            return Ok(vec![]);
        };
        let model = self.services.model()?;
        let bundle = self.bundle(key, &model).await?;
        let store = self.services.index.as_ref();
        let digests: Vec<String> = bundle
            .items
            .iter()
            .map(|item| item.vector_segment.clone())
            .collect();
        let existing = store.existing_segments(&digests).await?;
        let pending: Vec<&IndexedItem> = bundle
            .items
            .iter()
            .filter(|item| !existing.contains(&item.vector_segment))
            .collect();
        let mut out = Vec::new();
        for batch in pending.chunks(BUILD_BATCH) {
            let mut batch_units = Vec::new();
            for units in self.batch_units(&bundle.header, batch).await? {
                for item in units {
                    batch_units.push((cache_digest(&cache_key(&model, &item))?, item.unit.text));
                }
            }
            let keys: Vec<String> = batch_units.iter().map(|(key, _)| key.clone()).collect();
            let stored = store.cached_values(&model, &keys).await?;
            out.extend(
                batch_units
                    .into_iter()
                    .filter(|(key, _)| !stored.contains_key(key)),
            );
        }
        Ok(out)
    }

    /// Restart: keep only published generations of the current bundle whose
    /// stage lists its segments and whose manifest the segment checks
    /// validate; withdraw the rest and discard every unpublished stage.
    pub async fn recover(&self) -> Result<(), SearchError> {
        let model = self.services.model()?;
        let current = match self.current_key().await? {
            Some(key) => Some(self.bundle(key, &model).await?),
            None => None,
        };
        let index = self.services.index.as_ref();
        let generations = self.services.generations.as_ref();
        let now = OffsetDateTime::now_utc();
        let mut live = Vec::new();
        for manifest in generations.published().await? {
            let valid = match &current {
                Some(bundle)
                    if bundle.header.bundle_key == manifest.bundle_key
                        && manifest.model_id == model
                        && manifest.lease_expires_at.is_none_or(|expiry| expiry > now) =>
                {
                    let expected: Vec<&str> = bundle
                        .items
                        .iter()
                        .map(|item| item.vector_segment.as_str())
                        .collect();
                    let listed = index.listed_segments(&manifest.index.index_digest).await;
                    match listed {
                        Ok(listed) if listed.iter().map(String::as_str).eq(expected) => self
                            .segment_checks(&model, bundle, Missing::Fail)
                            .await
                            .ok()
                            .flatten()
                            .is_some_and(|checks| {
                                manifest.validate_segments(&bundle.header, &checks).is_ok()
                            }),
                        _ => false,
                    }
                }
                _ => false,
            };
            if valid {
                live.push(manifest.index.clone());
            } else {
                generations.withdraw(&manifest).await?;
            }
        }
        for stage in index.stages().await? {
            if !live.contains(&stage) {
                index.discard(&stage).await?;
            }
        }
        Ok(())
    }
}

/// Unit segments kept resolved per generation before they are dropped.
const RESOLVED_SEGMENTS: usize = 256;

/// A loaded generation's indexed items and their Unit segments, for hit
/// resolution without holding every Unit (stage 3): a hit's Unit is read
/// from its item's segment, verified, when a query first needs it.
pub struct VectorUnitSegments {
    key: ProjectionGenerationKey,
    source_snapshot: String,
    pool: Option<PgPool>,
    /// Indexed items in manifest order: Version, Part, Unit segment digest.
    items: Vec<(ResourceVersionRef, ContentPartRef, String)>,
    resolved: Mutex<HashMap<String, Arc<BodyItemEntry>>>,
}

/// The manifest order of an item (Resource, part ordinal, path, native ID).
fn item_order<'a>(
    version: &ResourceVersionRef,
    part: &'a ContentPartRef,
) -> (
    search_application::search_core::id::ResourceId,
    u32,
    &'a str,
    &'a str,
) {
    (
        version.resource_id,
        part.ordinal,
        part.logical_path.as_str(),
        part.source_native_part_id.as_str(),
    )
}

impl VectorUnitSegments {
    /// No Unit resolves: for a host without Vector retrieval.
    pub fn none(key: ProjectionGenerationKey) -> Self {
        Self {
            key,
            source_snapshot: String::new(),
            pool: None,
            items: vec![],
            resolved: Default::default(),
        }
    }

    /// The indexed items of a restored summary, whose coverage items and
    /// segment digests are in the same manifest order.
    pub fn of_summary(
        pool: PgPool,
        summary: &crate::payload::RestoredSummaryV1,
    ) -> Result<Self, SearchError> {
        if summary.coverage.items.len() != summary.units.segments.len() {
            return Err(failed("segment list", summary.units.segments.len()));
        }
        let items = summary
            .coverage
            .items
            .iter()
            .zip(&summary.units.segments)
            .filter(|(item, _)| indexed_coverage(item.operation, item.coverage.as_ref()).is_some())
            .map(|(item, digest)| (item.version.clone(), item.part.clone(), digest.clone()))
            .collect();
        Ok(Self {
            key: summary.units.key,
            source_snapshot: summary.units.source_snapshot.clone(),
            pool: Some(pool),
            items,
            resolved: Default::default(),
        })
    }

    /// The Unit `hit` names, with its pinned authority, if it is one of the
    /// generation's indexed Units.
    async fn unit(
        &self,
        hit: &VectorHitRef,
    ) -> Result<Option<(KnowledgeUnit, VectorAuthorityInput)>, SearchError> {
        let Some(pool) = &self.pool else {
            return Ok(None);
        };
        let wanted = item_order(&hit.version, &hit.part);
        let Ok(at) = self
            .items
            .binary_search_by(|(version, part, _)| item_order(version, part).cmp(&wanted))
        else {
            return Ok(None);
        };
        let (version, part, digest) = &self.items[at];
        if *version != hit.version || *part != hit.part {
            return Ok(None);
        }
        let cached = self
            .resolved
            .lock()
            .map_err(|_| failed("lock", ()))?
            .get(digest)
            .cloned();
        let entry = match cached {
            Some(entry) => entry,
            None => {
                let mut read = PgPayloadStore::new(pool.clone())
                    .unit_segments(std::slice::from_ref(digest))
                    .await
                    .map_err(|error| failed("unit segment", error))?;
                let entry = Arc::new(
                    read.remove(digest)
                        .ok_or_else(|| failed("unit segment", ()))?,
                );
                let mut resolved = self.resolved.lock().map_err(|_| failed("lock", ()))?;
                if resolved.len() >= RESOLVED_SEGMENTS {
                    resolved.clear();
                }
                resolved.insert(digest.clone(), entry.clone());
                entry
            }
        };
        let Some(unit) = entry.units.iter().find(|unit| unit.unit_id == hit.unit_id) else {
            return Ok(None);
        };
        let mut unit = unit.clone();
        Arc::make_mut(&mut unit.provenance).source_snapshot = self.source_snapshot.clone();
        let authority = unit_authority(self.key, &unit);
        Ok(Some((unit, authority)))
    }
}

/// Resolves hits against the loaded generation and the actor's current Read.
pub struct DocumentVectorResolver {
    key: ProjectionGenerationKey,
    units: Arc<VectorUnitSegments>,
    access: Arc<DocumentCurrentAccessAdapter>,
    binding: String,
}

impl VectorSourceResolverPort for DocumentVectorResolver {
    fn current_unit<'a>(
        &'a self,
        scope: &'a AuthorizedSourceScope,
        hit: &'a VectorHitRef,
    ) -> BoxFuture<'a, SourceUnitState> {
        Box::pin(async move {
            if hit.generation != self.key
                || scope.source_id() != self.key.source_id
                || self.units.key != self.key
            {
                return Ok(SourceUnitState::NotCurrent);
            }
            let Some((unit, authority)) = self.units.unit(hit).await? else {
                return Ok(SourceUnitState::NotCurrent);
            };
            let read: AccessDecision = CurrentAccessEvaluatorPort::evaluate(
                self.access.as_ref(),
                unit.version.resource_id,
                &self.binding,
            )
            .await
            .unwrap_or(AccessDecision::Unknown);
            Ok(SourceUnitState::Current(Box::new(CurrentSourceUnit {
                unit,
                authority,
                read,
            })))
        })
    }
}

/// One actor's Vector retrieval over the loaded generation.
pub trait ActorVectorPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        scope: &'a AuthorizedSourceScope,
        generation: ProjectionGenerationKey,
        text: &'a str,
        window: usize,
    ) -> BoxFuture<'a, VectorRetrievalBatch>;
}

pub struct DocumentActorVector {
    services: VectorServices,
    resolver: DocumentVectorResolver,
}

impl DocumentActorVector {
    pub fn new(
        services: VectorServices,
        key: ProjectionGenerationKey,
        units: Arc<VectorUnitSegments>,
        access: Arc<DocumentCurrentAccessAdapter>,
        binding: String,
    ) -> Self {
        Self {
            services,
            resolver: DocumentVectorResolver {
                key,
                units,
                access,
                binding,
            },
        }
    }
}

impl ActorVectorPort for DocumentActorVector {
    fn retrieve<'a>(
        &'a self,
        scope: &'a AuthorizedSourceScope,
        generation: ProjectionGenerationKey,
        text: &'a str,
        window: usize,
    ) -> BoxFuture<'a, VectorRetrievalBatch> {
        Box::pin(async move {
            let query = TrustedVectorQuery::compile(
                scope,
                self.services.activations.as_ref(),
                text,
                window,
            )
            .await?;
            VectorRetriever {
                provider: self.services.provider.as_ref(),
                index: self.services.index.as_ref(),
                generations: self.services.generations.as_ref(),
                resolver: &self.resolver,
            }
            .retrieve(generation, &query, OffsetDateTime::now_utc())
            .await
        })
    }
}
