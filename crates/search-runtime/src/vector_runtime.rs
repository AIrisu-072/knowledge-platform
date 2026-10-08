//! E (P2-07): Vector over the durable Document Source.
//!
//! The maintainer builds the Vector generation of the Source's current READY
//! P1 bundle from its stored Unit manifest — one authority scope per Document
//! Source, persistent retention — reusing stored embeddings whose cache key
//! is unchanged, and publishes it through the P2-06 lifecycle CAS. A hit is
//! resolved against the loaded generation's own Units and the actor's
//! current Document Read before it can become a candidate.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use search_application::SearchError;
use search_application::ports::{AccessDecision, BoxFuture, CurrentAccessEvaluatorPort};
use search_application::scoped::AuthorizedSourceScope;
use search_application::search_core::id::{ProjectionGenerationId, SourceId};
use search_application::search_core::knowledge_unit::{
    ContentPartRef, EmbeddingCacheKey, KnowledgeUnit, ResourceVersionRef, VectorAuthorityInput,
    VectorHitRef,
};
use search_application::search_core::projection::ProjectionGenerationKey;
use search_application::search_core::source::{DiscoverableSource, RetentionMode};
use search_application::search_core::vector::{
    BoundEmbedding, EmbeddingModelId, VectorActivationPolicy, VectorManifestInput,
    VectorManifestUnit, VectorProjectionManifest, VectorStorageKind, VectorUnitCoverage,
};
use search_application::vector::{
    CurrentSourceUnit, EmbeddingProvider, SourceUnitState, TrustedVectorQuery, VectorActivation,
    VectorActivationPort, VectorBuildOutcome, VectorGenerationPort, VectorIndexPort,
    VectorLifecycle, VectorRetrievalBatch, VectorRetriever, VectorSourceResolverPort,
};
use search_extraction_core::{BodyCoverage, ItemOperationState};
use search_source_document::{
    BodyItemEntry, BodyUnitManifest, DocumentCurrentAccessAdapter, GenerationBundleReceipt,
};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::payload::{PgPayloadStore, stored_segment_digest};
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

fn hex(bytes: &[u8; 32]) -> String {
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

fn failed(what: &str, error: impl std::fmt::Debug) -> SearchError {
    SearchError::SourceUnavailable(format!("vector: {what}: {error:?}"))
}

/// The items of a stored bundle whose Units are indexed, with their coverage.
fn indexed_items(
    units: &BodyUnitManifest,
) -> impl Iterator<Item = (&BodyItemEntry, VectorUnitCoverage)> {
    units.entries.iter().filter_map(|entry| {
        Some((
            entry,
            indexed_coverage(entry.operation, entry.coverage.as_ref())?,
        ))
    })
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

/// The indexed Units of one stored bundle with their pinned authority. The
/// Units are moved, so a large bundle is never held twice.
pub fn manifest_units(
    key: ProjectionGenerationKey,
    units: BodyUnitManifest,
) -> Vec<VectorManifestUnit> {
    units
        .entries
        .into_iter()
        .filter_map(|entry| {
            let coverage = indexed_coverage(entry.operation, entry.coverage.as_ref())?;
            Some((entry.units, coverage))
        })
        .flat_map(|(units, coverage)| {
            units.into_iter().map(move |unit| VectorManifestUnit {
                authority: unit_authority(key, &unit),
                unit,
                coverage,
            })
        })
        .collect()
}

pub fn manifest_input(
    key: ProjectionGenerationKey,
    source_snapshot: &str,
    units: BodyUnitManifest,
    receipt: &GenerationBundleReceipt,
) -> VectorManifestInput {
    VectorManifestInput {
        bundle_key: key,
        body_receipt_digest: hex(&receipt.composite_digest),
        source_snapshot: source_snapshot.to_owned(),
        authority_scope_key: document_scope_key(key.source_id),
        retention_lease_id: DOCUMENT_RETENTION_LEASE.into(),
        lease_expires_at: None,
        source_declares_persistent_embedding_permission: true,
        units: manifest_units(key, units),
        nonindexed_retention_unit_ids: vec![],
        lexical_analyzer_revision: receipt.lexical_schema_version.clone(),
        graph_schema_revision: search_graph::GRAPH_SCHEMA_VERSION.into(),
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

/// The Vector segment digest of each item of `input`.
fn vector_segments(
    model: &EmbeddingModelId,
    input: &VectorManifestInput,
    items: &[(String, Range<usize>)],
) -> Vec<String> {
    items
        .iter()
        .map(|(unit_segment, _)| {
            segment_digest_for(
                model,
                unit_segment,
                &input.authority_scope_key,
                &input.retention_lease_id,
                "",
            )
        })
        .collect()
}

/// Builds and recovers the Vector generation of the current P1 bundle.
pub struct VectorMaintainer {
    pool: PgPool,
    source: DiscoverableSource,
    services: VectorServices,
}

impl VectorMaintainer {
    pub fn new(pool: PgPool, source: DiscoverableSource, services: VectorServices) -> Self {
        Self {
            pool,
            source,
            services,
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

    /// The comparison input of `key` and, per indexed item, its Unit segment
    /// digest and the range of its Units in the input.
    async fn input(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<(VectorManifestInput, Vec<(String, Range<usize>)>), SearchError> {
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
        let manifest: search_application::search_core::projection::ProjectionGenerationManifest =
            serde_json::from_value(row.0.get("manifest").cloned().unwrap_or_default())
                .map_err(|error| failed("manifest", error))?;
        let receipt: GenerationBundleReceipt =
            serde_json::from_value(row.1.get("bundle").cloned().unwrap_or_default())
                .map_err(|error| failed("receipt", error))?;
        if manifest.key() != key || receipt.key != key {
            return Err(failed("bundle key", key));
        }
        let restored = PgPayloadStore::new(self.pool.clone())
            .restore(&manifest)
            .await
            .map_err(|error| failed("payload", error))?;
        let mut items = Vec::new();
        let mut at = 0;
        for (entry, _) in indexed_items(&restored.unit_manifest) {
            let digest =
                stored_segment_digest(entry).map_err(|error| failed("segment digest", error))?;
            items.push((digest, at..at + entry.units.len()));
            at += entry.units.len();
        }
        let input = manifest_input(
            key,
            &manifest.source_snapshot,
            restored.unit_manifest,
            &receipt,
        );
        if input.units.len() != at {
            return Err(failed("indexed Units", at));
        }
        Ok((input, items))
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
        let (input, items) = self.input(key).await?;
        self.build(&input, &items, scope_epoch).await.map(Some)
    }

    /// One build of `input` by Unit segment (stage 3): a Vector segment
    /// whose Unit segment and authority are unchanged is listed as it is;
    /// a new one reuses stored values by cache key and embeds the rest.
    async fn build(
        &self,
        input: &VectorManifestInput,
        items: &[(String, Range<usize>)],
        scope_epoch: u64,
    ) -> Result<VectorBuildOutcome, SearchError> {
        let spec = self.services.provider.spec();
        let model = self.services.model()?;
        if !input.nonindexed_retention_unit_ids.is_empty() {
            return Err(failed("nonindexed retention Units", input.bundle_key));
        }
        let index = self.services.index.as_ref();
        let segments = vector_segments(&model, input, items);
        let existing = index.existing_segments(&segments).await?;
        for ((_, range), segment) in items.iter().zip(&segments) {
            if existing.contains(segment) {
                continue;
            }
            let units = &input.units[range.clone()];
            let keys = units
                .iter()
                .map(|item| cache_digest(&cache_key(&model, item)))
                .collect::<Result<Vec<_>, _>>()?;
            let stored = index.cached_values(&model, &keys).await?;
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
                .put_segment(segment, &model, &input.authority_scope_key, &embeddings)
                .await?;
        }
        let (descriptor, staged) = index
            .stage_segments(input.bundle_key, &model, &segments)
            .await?;
        let checked = VectorProjectionManifest::stage(
            spec,
            input,
            descriptor.clone(),
            &staged,
            VectorStorageKind::Persistent,
            OffsetDateTime::now_utc(),
        )
        .and_then(|manifest| {
            manifest
                .validate_against(input, &staged)
                .map(|receipt| (manifest, receipt))
        });
        drop(staged);
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
        let (input, items) = self.input(key).await?;
        let store = self.services.index.as_ref();
        let segments = vector_segments(&model, &input, &items);
        let existing = store.existing_segments(&segments).await?;
        let mut pending = Vec::new();
        for ((_, range), segment) in items.iter().zip(&segments) {
            if existing.contains(segment) {
                continue;
            }
            for item in &input.units[range.clone()] {
                pending.push((
                    cache_digest(&cache_key(&model, item))?,
                    item.unit.text.clone(),
                ));
            }
        }
        let keys: Vec<String> = pending.iter().map(|(key, _)| key.clone()).collect();
        let stored = store.cached_values(&model, &keys).await?;
        pending.retain(|(key, _)| !stored.contains_key(key));
        Ok(pending)
    }

    /// Restart: keep only published generations of the current bundle that
    /// still validate; discard every other stage.
    pub async fn recover(&self) -> Result<(), SearchError> {
        let inputs = match self.current_key().await? {
            Some(key) => vec![self.input(key).await?.0],
            None => vec![],
        };
        let lifecycle = VectorLifecycle {
            provider: self.services.provider.as_ref(),
            index: self.services.index.as_ref(),
            generations: self.services.generations.as_ref(),
            activations: self.services.activations.as_ref(),
        };
        lifecycle
            .recover(&inputs, OffsetDateTime::now_utc())
            .await?;
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
    resolved: std::sync::Mutex<std::collections::HashMap<String, Arc<BodyItemEntry>>>,
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
