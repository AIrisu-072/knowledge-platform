//! Boundary fakes for the P2-06 Vector contract. They force failure
//! boundaries (dropped stage entries, moved P1 pointers, foreign hits,
//! Unknown Read); they are not quality evidence.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use search_application::SearchError;
use search_application::ports::{AccessDecision, BoxFuture};
use search_application::scoped::{AuthorizedSourceScope, CurrentSourceVisibilityPort};
use search_application::vector::{
    CurrentSourceUnit, EmbeddingProvider, PinnedVectorGeneration, SourceUnitState,
    TrustedVectorQuery, VectorGenerationPort, VectorIndexPort, VectorSourceResolverPort,
};
use search_core::id::{ProjectionGenerationId, ResourceId, SourceId};
use search_core::knowledge_unit::{
    ContentPartRef, ExtractionProfileId, FormatId, KnowledgeUnit, NativeLocator, RawBinding,
    ResourceVersionRef, UnitId, UnitKind, UnitProvenance, VectorAuthorityInput, VectorHitRef,
    text_sha256,
};
use search_core::projection::ProjectionGenerationKey;
use search_core::source::RetentionMode;
use search_core::vector::{
    BoundEmbedding, EmbeddingModelId, EmbeddingModelSpec, QueryEmbedding, RankedVectorHit,
    VectorEntryRef, VectorIndexDescriptor, VectorManifestInput, VectorManifestUnit, VectorMetric,
    VectorNormalization, VectorPrecision, VectorProjectionManifest, VectorUnitCoverage,
};
use uuid::Uuid;

use crate::api::ApiWorld;

pub fn digest(byte: u8) -> String {
    format!("sha256:{}", format!("{byte:02x}").repeat(32))
}

pub fn spec(revision: &str) -> EmbeddingModelSpec {
    EmbeddingModelSpec {
        model_name: "synthetic/multilingual-small".into(),
        model_revision: revision.into(),
        weights_sha256: [1; 32],
        tokenizer_revision: "commit-def456".into(),
        tokenizer_files_sha256: [2; 32],
        tokenizer_config_sha256: [3; 32],
        unicode_preprocessing: "NFC".into(),
        input_preprocessing: "unit-text-v1".into(),
        query_template: "query: {text}".into(),
        passage_template: "passage: {text}".into(),
        pooling: "masked-mean".into(),
        attention_masking: "exclude-padding".into(),
        max_tokens: 512,
        chunking: "one-unit-no-neighbor".into(),
        truncation: "right-at-512".into(),
        dimension: 3,
        precision: VectorPrecision::F32,
        normalization: VectorNormalization::UnitL2,
        metric: VectorMetric::Dot,
        runtime_family: "rust-cpu".into(),
        runtime_build: "build-789".into(),
        native_binary_sha256: Some([4; 32]),
        deterministic_config_sha256: [5; 32],
    }
}

pub fn generation(source: SourceId, number: u128) -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: source,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(number)),
    }
}

/// One Unit of `parent`'s current Version in Part `part`.
pub fn unit(source: SourceId, parent: u128, part: u32, text: &str, profile: u8) -> KnowledgeUnit {
    let version = ResourceVersionRef {
        source_id: source,
        resource_id: ResourceId::from_uuid(Uuid::from_u128(parent)),
        source_native_version: "version-1".into(),
    };
    let part = ContentPartRef {
        source_native_part_id: format!("part-{part}"),
        logical_path: format!("part-{part}.txt"),
        ordinal: part,
    };
    let locator = NativeLocator::Text {
        line_start: 0,
        line_end: 1,
    };
    let profile = ExtractionProfileId::parse(&digest(profile)).unwrap();
    KnowledgeUnit {
        unit_id: UnitId::derive(&version, &part, &profile, &locator, 0).unwrap(),
        version,
        part,
        parent_unit_id: None,
        ordinal: 0,
        kind: UnitKind::PlainText,
        text_sha256: text_sha256(text),
        text: text.into(),
        locator,
        provenance: UnitProvenance {
            source_snapshot: "snapshot-1".into(),
            authoritative_representation_ref: "representation-1".into(),
            raw: RawBinding {
                sha256: [7; 32],
                size_bytes: 1024,
                media_type: "text/plain".into(),
            },
            detected_format: FormatId::Text,
            archive_inner_format: None,
            profile,
            parser_build_id: "reader-1".into(),
        },
    }
}

pub fn authority(
    unit: &KnowledgeUnit,
    generation: ProjectionGenerationKey,
    scope: &str,
) -> VectorAuthorityInput {
    VectorAuthorityInput {
        generation,
        version: unit.version.clone(),
        part: unit.part.clone(),
        authoritative_representation_ref: unit.provenance.authoritative_representation_ref.clone(),
        raw: unit.provenance.raw.clone(),
        profile: unit.provenance.profile.clone(),
        authority_scope_key: scope.into(),
        retention_lease_id: "lease-1".into(),
        lifetime_scope_id: String::new(),
        retention_mode: RetentionMode::PersistentResource,
        lease_expires_at: None,
    }
}

pub fn input(
    units: &[KnowledgeUnit],
    generation: ProjectionGenerationKey,
    scope: &str,
) -> VectorManifestInput {
    VectorManifestInput {
        bundle_key: generation,
        body_receipt_digest: digest(10),
        source_snapshot: "snapshot-1".into(),
        authority_scope_key: scope.into(),
        retention_lease_id: "lease-1".into(),
        lease_expires_at: None,
        source_declares_persistent_embedding_permission: true,
        units: units
            .iter()
            .map(|unit| VectorManifestUnit {
                unit: unit.clone(),
                authority: authority(unit, generation, scope),
                coverage: VectorUnitCoverage::Complete,
            })
            .collect(),
        nonindexed_retention_unit_ids: vec![],
        lexical_analyzer_revision: "lexical-v1".into(),
        graph_schema_revision: "graph-v1".into(),
    }
}

/// A deterministic unit vector from the text; similar text, similar vector.
pub fn vector_of(text: &str) -> Vec<f32> {
    let mut values = [1.0_f32, 0.0, 0.0];
    if text.contains("alpha") {
        values = [0.9, 0.1, 0.0];
    }
    if text.contains("beta") {
        values = [0.1, 0.9, 0.0];
    }
    if text.contains("gamma") {
        values = [0.0, 0.1, 0.9];
    }
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    values.iter().map(|value| value / norm).collect()
}

pub struct Provider {
    pub spec: EmbeddingModelSpec,
    pub embedded_units: AtomicUsize,
    /// Answer queries with another model's embedding.
    pub forge_query_model: Option<EmbeddingModelSpec>,
}

impl Provider {
    pub fn new(spec: EmbeddingModelSpec) -> Self {
        Self {
            spec,
            embedded_units: AtomicUsize::new(0),
            forge_query_model: None,
        }
    }
    pub fn embedded(&self) -> usize {
        self.embedded_units.load(Ordering::SeqCst)
    }
}

impl EmbeddingProvider for Provider {
    fn spec(&self) -> &EmbeddingModelSpec {
        &self.spec
    }
    fn embed_units<'a>(
        &'a self,
        units: &'a [VectorManifestUnit],
    ) -> BoxFuture<'a, Vec<BoundEmbedding>> {
        Box::pin(async move {
            self.embedded_units.fetch_add(units.len(), Ordering::SeqCst);
            units
                .iter()
                .map(|item| {
                    BoundEmbedding::new(
                        &self.spec,
                        &item.unit,
                        &item.authority,
                        vector_of(&item.unit.text),
                    )
                    .map_err(|error| SearchError::OperationFailed(error.to_string()))
                })
                .collect()
        })
    }
    fn embed_query<'a>(&'a self, query: &'a TrustedVectorQuery) -> BoxFuture<'a, QueryEmbedding> {
        Box::pin(async move {
            let spec = self.forge_query_model.as_ref().unwrap_or(&self.spec);
            QueryEmbedding::new(spec, vector_of(query.text()))
                .map_err(|error| SearchError::OperationFailed(error.to_string()))
        })
    }
}

#[derive(Default)]
pub struct Index {
    stages: Mutex<BTreeMap<String, (VectorIndexDescriptor, Vec<BoundEmbedding>)>>,
    counter: AtomicUsize,
    /// Leave the last entry out of the next stage (a partial write).
    pub drop_one: Mutex<bool>,
    /// Answer searches with a hit of another generation.
    pub foreign_generation: Mutex<Option<ProjectionGenerationKey>>,
}

impl Index {
    pub fn stage_count(&self) -> usize {
        self.stages.lock().unwrap().len()
    }
    pub fn entry_count(&self) -> usize {
        self.stages
            .lock()
            .unwrap()
            .values()
            .map(|(_, entries)| entries.len())
            .sum()
    }
}

impl VectorIndexPort for Index {
    fn stage<'a>(
        &'a self,
        _bundle: ProjectionGenerationKey,
        _model: &'a EmbeddingModelId,
        embeddings: &'a [BoundEmbedding],
    ) -> BoxFuture<'a, VectorIndexDescriptor> {
        Box::pin(async move {
            let number = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
            let descriptor = VectorIndexDescriptor {
                engine: "exact".into(),
                engine_build: "build-1".into(),
                parameters_digest: digest(11),
                index_receipt_digest: digest(12),
                index_digest: format!("sha256:{number:064x}"),
            };
            let mut stored = embeddings.to_vec();
            if std::mem::take(&mut *self.drop_one.lock().unwrap()) {
                stored.pop();
            }
            self.stages.lock().unwrap().insert(
                descriptor.index_digest.clone(),
                (descriptor.clone(), stored),
            );
            Ok(descriptor)
        })
    }
    fn staged_entries<'a>(
        &'a self,
        index: &'a VectorIndexDescriptor,
    ) -> BoxFuture<'a, Vec<VectorEntryRef>> {
        Box::pin(async move {
            Ok(self
                .stages
                .lock()
                .unwrap()
                .get(&index.index_digest)
                .map(|(_, entries)| entries.iter().map(BoundEmbedding::entry_ref).collect())
                .unwrap_or_default())
        })
    }
    fn stages<'a>(&'a self) -> BoxFuture<'a, Vec<VectorIndexDescriptor>> {
        Box::pin(async move {
            Ok(self
                .stages
                .lock()
                .unwrap()
                .values()
                .map(|(descriptor, _)| descriptor.clone())
                .collect())
        })
    }
    fn search<'a>(
        &'a self,
        pin: &'a PinnedVectorGeneration,
        query: &'a QueryEmbedding,
        window: usize,
    ) -> BoxFuture<'a, Vec<RankedVectorHit>> {
        Box::pin(async move {
            let stages = self.stages.lock().unwrap();
            let Some((_, entries)) = stages.get(&pin.manifest().index.index_digest) else {
                return Ok(vec![]);
            };
            let mut scored: Vec<(f32, VectorHitRef)> = entries
                .iter()
                .map(|embedding| {
                    let score = embedding
                        .values()
                        .iter()
                        .zip(query.values())
                        .map(|(a, b)| a * b)
                        .sum();
                    (score, embedding.entry_ref().hit)
                })
                .collect();
            scored.sort_by(|left, right| {
                right
                    .0
                    .total_cmp(&left.0)
                    .then_with(|| left.1.unit_id.cmp(&right.1.unit_id))
            });
            let foreign = *self.foreign_generation.lock().unwrap();
            Ok(scored
                .into_iter()
                .take(window)
                .enumerate()
                .map(|(index, (score, mut hit))| {
                    if let Some(generation) = foreign {
                        hit.generation = generation;
                    }
                    RankedVectorHit {
                        hit,
                        model_id: pin.manifest().model_id.clone(),
                        rank: index + 1,
                        raw_similarity: score,
                    }
                })
                .collect())
        })
    }
    fn discard<'a>(&'a self, index: &'a VectorIndexDescriptor) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.stages.lock().unwrap().remove(&index.index_digest);
            Ok(())
        })
    }
    fn purge_scope<'a>(&'a self, authority_scope_key: &'a str) -> BoxFuture<'a, usize> {
        Box::pin(async move {
            let mut removed = 0;
            for (_, entries) in self.stages.lock().unwrap().values_mut() {
                let before = entries.len();
                entries.retain(|embedding| {
                    embedding.entry_ref().cache_key.authority_scope_key != authority_scope_key
                });
                removed += before - entries.len();
            }
            Ok(removed)
        })
    }
}

/// The P1 current pointer per Source and the published Vector manifests.
#[derive(Default)]
pub struct Generations {
    pub p1_current: Mutex<BTreeMap<SourceId, ProjectionGenerationKey>>,
    published: Mutex<Vec<VectorProjectionManifest>>,
}

impl Generations {
    pub fn set_p1(&self, key: ProjectionGenerationKey) {
        self.p1_current.lock().unwrap().insert(key.source_id, key);
    }
    pub fn publish_raw(&self, manifest: VectorProjectionManifest) {
        self.published.lock().unwrap().push(manifest);
    }
}

impl VectorGenerationPort for Generations {
    fn publish_if_current<'a>(
        &'a self,
        manifest: &'a VectorProjectionManifest,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            if self
                .p1_current
                .lock()
                .unwrap()
                .get(&manifest.bundle_key.source_id)
                != Some(&manifest.bundle_key)
            {
                return Ok(false);
            }
            let mut published = self.published.lock().unwrap();
            published.retain(|existing| {
                existing.bundle_key != manifest.bundle_key || existing.model_id != manifest.model_id
            });
            published.push(manifest.clone());
            Ok(true)
        })
    }
    fn pin_current<'a>(
        &'a self,
        bundle: ProjectionGenerationKey,
        model: &'a EmbeddingModelId,
    ) -> BoxFuture<'a, Option<PinnedVectorGeneration>> {
        Box::pin(async move {
            Ok(self
                .published
                .lock()
                .unwrap()
                .iter()
                .find(|manifest| manifest.bundle_key == bundle && manifest.model_id == *model)
                .cloned()
                .map(PinnedVectorGeneration::new))
        })
    }
    fn published<'a>(&'a self) -> BoxFuture<'a, Vec<VectorProjectionManifest>> {
        Box::pin(async move { Ok(self.published.lock().unwrap().clone()) })
    }
    fn withdraw<'a>(&'a self, manifest: &'a VectorProjectionManifest) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.published
                .lock()
                .unwrap()
                .retain(|existing| existing.manifest_digest != manifest.manifest_digest);
            Ok(())
        })
    }
}

/// The Source's current Units with a per-parent Read decision.
#[derive(Default)]
pub struct Resolver {
    pub units: Mutex<BTreeMap<UnitId, (KnowledgeUnit, VectorAuthorityInput)>>,
    pub read: Mutex<BTreeMap<ResourceId, AccessDecision>>,
    pub not_current: Mutex<Vec<UnitId>>,
    pub failing: Mutex<bool>,
}

impl Resolver {
    pub fn current(&self, input: &VectorManifestInput) {
        let mut units = self.units.lock().unwrap();
        units.clear();
        for item in &input.units {
            units.insert(
                item.unit.unit_id,
                (item.unit.clone(), item.authority.clone()),
            );
        }
    }
    pub fn read(&self, parent: u128, decision: AccessDecision) {
        self.read
            .lock()
            .unwrap()
            .insert(ResourceId::from_uuid(Uuid::from_u128(parent)), decision);
    }
}

impl VectorSourceResolverPort for Resolver {
    fn current_unit<'a>(
        &'a self,
        _scope: &'a AuthorizedSourceScope,
        hit: &'a VectorHitRef,
    ) -> BoxFuture<'a, SourceUnitState> {
        Box::pin(async move {
            if *self.failing.lock().unwrap() {
                return Err(SearchError::SourceUnavailable("timeout".into()));
            }
            if self.not_current.lock().unwrap().contains(&hit.unit_id) {
                return Ok(SourceUnitState::NotCurrent);
            }
            let Some((unit, authority)) = self.units.lock().unwrap().get(&hit.unit_id).cloned()
            else {
                return Ok(SourceUnitState::NotCurrent);
            };
            let read = self
                .read
                .lock()
                .unwrap()
                .get(&unit.version.resource_id)
                .copied()
                .unwrap_or(AccessDecision::Allowed);
            Ok(SourceUnitState::Current(Box::new(CurrentSourceUnit {
                unit,
                authority,
                read,
            })))
        })
    }
}

/// A real authorized scope for the world's first Document Source.
pub async fn scope(world: &ApiWorld) -> AuthorizedSourceScope {
    let visibility = world.visibility();
    let handle = world
        .actor("tenant-a", "reader", &visibility, &[world.document])
        .await;
    let actor =
        search_application::scoped::AccessContextAuthorityPort::resolve(&world.authority, &handle)
            .await
            .unwrap()
            .unwrap();
    visibility
        .bind_source(&actor, world.document)
        .await
        .unwrap()
        .unwrap()
}
