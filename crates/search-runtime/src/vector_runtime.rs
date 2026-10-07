//! E (P2-07): Vector over the durable Document Source.
//!
//! The maintainer builds the Vector generation of the Source's current READY
//! P1 bundle from its stored Unit manifest — one authority scope per Document
//! Source, persistent retention — reusing stored embeddings whose cache key
//! is unchanged, and publishes it through the P2-06 lifecycle CAS. A hit is
//! resolved against the loaded generation's own Units and the actor's
//! current Document Read before it can become a candidate.

use std::collections::BTreeMap;
use std::sync::Arc;

use search_application::SearchError;
use search_application::ports::{AccessDecision, BoxFuture, CurrentAccessEvaluatorPort};
use search_application::scoped::AuthorizedSourceScope;
use search_application::search_core::id::{ProjectionGenerationId, SourceId};
use search_application::search_core::knowledge_unit::{
    EmbeddingCacheKey, KnowledgeUnit, UnitId, VectorAuthorityInput, VectorHitRef,
};
use search_application::search_core::projection::ProjectionGenerationKey;
use search_application::search_core::source::{DiscoverableSource, RetentionMode};
use search_application::search_core::vector::{
    BoundEmbedding, EmbeddingModelId, VectorActivationPolicy, VectorManifestInput,
    VectorManifestUnit, VectorStorageKind, VectorUnitCoverage,
};
use search_application::vector::{
    CurrentSourceUnit, EmbeddingProvider, SourceUnitState, TrustedVectorQuery, VectorActivation,
    VectorActivationPort, VectorBuildOutcome, VectorGenerationPort, VectorLifecycle,
    VectorRetrievalBatch, VectorRetriever, VectorSourceResolverPort,
};
use search_extraction_core::{BodyCoverage, ItemOperationState};
use search_source_document::{
    BodyUnitManifest, DocumentCurrentAccessAdapter, GenerationBundleReceipt,
};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::payload::PgPayloadStore;
use crate::vector_store::{PgVectorGenerations, PgVectorIndex};

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

/// The indexed Units of one stored bundle with their pinned authority.
pub fn manifest_units(
    key: ProjectionGenerationKey,
    units: &BodyUnitManifest,
) -> Vec<VectorManifestUnit> {
    let scope = document_scope_key(key.source_id);
    units
        .entries
        .iter()
        .filter(|entry| entry.operation == ItemOperationState::Completed)
        .filter_map(|entry| {
            let coverage = match entry.coverage.as_ref()? {
                BodyCoverage::Supported => VectorUnitCoverage::Complete,
                BodyCoverage::Partial { .. } => VectorUnitCoverage::PartialValidated,
                BodyCoverage::Unsupported { .. } => return None,
            };
            Some((entry, coverage))
        })
        .flat_map(|(entry, coverage)| {
            let scope = scope.clone();
            entry.units.iter().map(move |unit| VectorManifestUnit {
                unit: unit.clone(),
                authority: VectorAuthorityInput {
                    generation: key,
                    version: (*unit.version).clone(),
                    part: (*unit.part).clone(),
                    authoritative_representation_ref: unit
                        .provenance
                        .authoritative_representation_ref
                        .clone(),
                    raw: unit.provenance.raw.clone(),
                    profile: unit.provenance.profile.clone(),
                    authority_scope_key: scope.clone(),
                    retention_lease_id: DOCUMENT_RETENTION_LEASE.into(),
                    lifetime_scope_id: String::new(),
                    retention_mode: RetentionMode::PersistentResource,
                    lease_expires_at: None,
                },
                coverage,
            })
        })
        .collect()
}

pub fn manifest_input(
    key: ProjectionGenerationKey,
    source_snapshot: &str,
    units: &BodyUnitManifest,
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

    async fn input(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<VectorManifestInput, SearchError> {
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
        Ok(manifest_input(
            key,
            &manifest.source_snapshot,
            &restored.unit_manifest,
            &receipt,
        ))
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
        let input = self.input(key).await?;
        // Reuse stored vectors whose cache key is unchanged, re-bound here.
        let stored = self
            .services
            .index
            .values_by_cache_key(self.source.source_id, &model)
            .await?;
        let spec = self.services.provider.spec();
        let previous: Vec<BoundEmbedding> = input
            .units
            .iter()
            .filter_map(|item| {
                let values = stored.get(&cache_key(&model, item))?;
                BoundEmbedding::new(spec, &item.unit, &item.authority, values.clone()).ok()
            })
            .collect();
        let lifecycle = VectorLifecycle {
            provider: self.services.provider.as_ref(),
            index: self.services.index.as_ref(),
            generations: self.services.generations.as_ref(),
            activations: self.services.activations.as_ref(),
        };
        lifecycle
            .build_at(
                &input,
                &previous,
                VectorStorageKind::Persistent,
                OffsetDateTime::now_utc(),
                scope_epoch,
            )
            .await
            .map(Some)
    }

    /// Restart: keep only published generations of the current bundle that
    /// still validate; discard every other stage.
    pub async fn recover(&self) -> Result<(), SearchError> {
        let inputs = match self.current_key().await? {
            Some(key) => vec![self.input(key).await?],
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

/// A loaded generation's Units, keyed for hit resolution.
pub type VectorUnits = BTreeMap<UnitId, (KnowledgeUnit, VectorAuthorityInput)>;

pub fn vector_units(key: ProjectionGenerationKey, units: &BodyUnitManifest) -> VectorUnits {
    manifest_units(key, units)
        .into_iter()
        .map(|item| (item.unit.unit_id, (item.unit, item.authority)))
        .collect()
}

/// Resolves hits against the loaded generation and the actor's current Read.
pub struct DocumentVectorResolver {
    key: ProjectionGenerationKey,
    units: Arc<VectorUnits>,
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
            if hit.generation != self.key || scope.source_id() != self.key.source_id {
                return Ok(SourceUnitState::NotCurrent);
            }
            let Some((unit, authority)) = self.units.get(&hit.unit_id) else {
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
                unit: unit.clone(),
                authority: authority.clone(),
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
        units: Arc<VectorUnits>,
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
