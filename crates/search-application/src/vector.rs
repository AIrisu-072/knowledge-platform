//! P2-06: the provider-neutral Vector application contract.
//!
//! Vector is an optional retriever behind `VectorActivationPolicy`, which
//! defaults to `Disabled`; no adapter is wired until a measured adoption
//! decision selects one. A query is compiled by the server for one
//! authorized Source scope and the Source's registered model, so a caller
//! chooses neither actor, Source, model nor an unbounded window. Every index
//! hit goes through the owning Source before it becomes a candidate: the
//! current Live Version, Part, representation, raw bytes, profile, text,
//! model and generation are rechecked; Denied and stale hits are suppressed
//! without identity; an Unknown makes the batch unavailable instead of
//! proving absence. Units fold to their parent Version once and raw
//! similarity stays in the retriever's own trace. Building is embed → stage
//! → two-way validation → CAS publish: a failed stage or a lost CAS keeps
//! the previous pointer and discards only the unpublished stage. Embedding
//! bytes are reused only under an identical P1 cache key and are always
//! re-bound to the destination generation.

use std::collections::BTreeSet;
use std::fmt;

use search_core::discovery::{
    CandidateIdentityClass, FederatedCandidate, GapReason, InformationGap,
};
use search_core::id::{ResourceId, SourceId};
use search_core::knowledge_unit::{
    EmbeddingCacheKey, KnowledgeUnit, VectorAuthorityInput, VectorHitRef,
};
use search_core::projection::ProjectionGenerationKey;
use search_core::vector::{
    BoundEmbedding, EmbeddingModelId, EmbeddingModelSpec, QueryEmbedding, RankedVectorHit,
    VectorActivationPolicy, VectorEntryRef, VectorIndexDescriptor, VectorManifestInput,
    VectorManifestUnit, VectorProjectionManifest, VectorStageReceipt, VectorStorageKind,
};
use time::OffsetDateTime;

use crate::SearchError;
use crate::candidate::{CandidateHardGates, RankedCandidateHit, RetrieverRankList};
use crate::ports::{AccessDecision, BoxFuture};
use crate::scoped::AuthorizedSourceScope;

pub const MAX_VECTOR_QUERY_BYTES: usize = 2_048;
pub const MAX_VECTOR_WINDOW: usize = 100;

fn invalid(message: &'static str) -> SearchError {
    SearchError::InvalidRequest(message.into())
}

/// A Vector query the server compiled for one authorized Source scope.
#[derive(Clone)]
pub struct TrustedVectorQuery {
    scope: AuthorizedSourceScope,
    model_id: EmbeddingModelId,
    text: String,
    window: usize,
}

impl fmt::Debug for TrustedVectorQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TrustedVectorQuery(<redacted>)")
    }
}

/// One Source's registered Vector activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorActivation {
    pub policy: VectorActivationPolicy,
    pub model: EmbeddingModelSpec,
}

/// Host wiring of each Source's registered activation; never a request
/// value. A Source without one is `Disabled`.
pub trait VectorActivationPort: Send + Sync {
    fn activation<'a>(&'a self, source: SourceId) -> BoxFuture<'a, Option<VectorActivation>>;
}

/// The registered model of an opted-in Source, or the Disabled refusal.
async fn active_model(
    activations: &dyn VectorActivationPort,
    source: SourceId,
) -> Result<EmbeddingModelSpec, SearchError> {
    match activations.activation(source).await? {
        Some(activation) if activation.policy == VectorActivationPolicy::EligibleOptIn => {
            Ok(activation.model)
        }
        _ => Err(invalid("Vector is disabled for this Source")),
    }
}

impl TrustedVectorQuery {
    /// The planner's only constructor: the policy and model are the scope
    /// Source's registered activation.
    pub async fn compile(
        scope: &AuthorizedSourceScope,
        activations: &dyn VectorActivationPort,
        text: &str,
        window: usize,
    ) -> Result<Self, SearchError> {
        let model = active_model(activations, scope.source_id()).await?;
        let text = text.trim();
        if text.is_empty()
            || text.len() > MAX_VECTOR_QUERY_BYTES
            || window == 0
            || window > MAX_VECTOR_WINDOW
        {
            return Err(invalid("Vector query out of bounds"));
        }
        let model_id = model
            .validate_and_id()
            .map_err(|_| invalid("Vector model"))?;
        Ok(Self {
            scope: scope.clone(),
            model_id,
            text: text.to_owned(),
            window,
        })
    }

    pub fn scope(&self) -> &AuthorizedSourceScope {
        &self.scope
    }

    pub fn model_id(&self) -> &EmbeddingModelId {
        &self.model_id
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn window(&self) -> usize {
        self.window
    }
}

/// A registered model's inference. Adapters stay outside core/application.
pub trait EmbeddingProvider: Send + Sync {
    fn spec(&self) -> &EmbeddingModelSpec;

    /// One bound embedding per Unit, in input order.
    fn embed_units<'a>(
        &'a self,
        units: &'a [VectorManifestUnit],
    ) -> BoxFuture<'a, Vec<BoundEmbedding>>;

    fn embed_query<'a>(&'a self, query: &'a TrustedVectorQuery) -> BoxFuture<'a, QueryEmbedding>;
}

/// A published, READY Vector generation pinned for one P1 bundle key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedVectorGeneration {
    manifest: VectorProjectionManifest,
}

impl PinnedVectorGeneration {
    pub fn new(manifest: VectorProjectionManifest) -> Self {
        Self { manifest }
    }

    pub fn manifest(&self) -> &VectorProjectionManifest {
        &self.manifest
    }

    pub fn key(&self) -> ProjectionGenerationKey {
        self.manifest.bundle_key
    }
}

pub trait VectorIndexPort: Send + Sync {
    /// Writes an isolated, unpublished stage and describes it.
    fn stage<'a>(
        &'a self,
        bundle: ProjectionGenerationKey,
        model: &'a EmbeddingModelId,
        embeddings: &'a [BoundEmbedding],
    ) -> BoxFuture<'a, VectorIndexDescriptor>;

    /// The entries actually present in a stage, read back for validation.
    fn staged_entries<'a>(
        &'a self,
        index: &'a VectorIndexDescriptor,
    ) -> BoxFuture<'a, Vec<VectorEntryRef>>;

    /// Every stage the index holds, published or not.
    fn stages<'a>(&'a self) -> BoxFuture<'a, Vec<VectorIndexDescriptor>>;

    fn search<'a>(
        &'a self,
        pin: &'a PinnedVectorGeneration,
        query: &'a QueryEmbedding,
        window: usize,
    ) -> BoxFuture<'a, Vec<RankedVectorHit>>;

    fn discard<'a>(&'a self, index: &'a VectorIndexDescriptor) -> BoxFuture<'a, ()>;

    /// Removes every entry of one authority scope; returns how many.
    fn purge_scope<'a>(&'a self, authority_scope_key: &'a str) -> BoxFuture<'a, usize>;
}

pub trait VectorGenerationPort: Send + Sync {
    /// CAS: publishes only while the manifest's P1 bundle key is still its
    /// Source's current key and its authority scope is still at
    /// `scope_epoch`; otherwise nothing changes.
    fn publish_if_current<'a>(
        &'a self,
        manifest: &'a VectorProjectionManifest,
        scope_epoch: u64,
    ) -> BoxFuture<'a, bool>;

    /// Advances on every purge of the scope (revocation, expiry, scope
    /// change, cancellation).
    fn scope_epoch<'a>(&'a self, authority_scope_key: &'a str) -> BoxFuture<'a, u64>;

    fn advance_scope_epoch<'a>(&'a self, authority_scope_key: &'a str) -> BoxFuture<'a, u64>;

    fn pin_current<'a>(
        &'a self,
        bundle: ProjectionGenerationKey,
        model: &'a EmbeddingModelId,
    ) -> BoxFuture<'a, Option<PinnedVectorGeneration>>;

    /// The published manifests a restart finds.
    fn published<'a>(&'a self) -> BoxFuture<'a, Vec<VectorProjectionManifest>>;

    fn withdraw<'a>(&'a self, manifest: &'a VectorProjectionManifest) -> BoxFuture<'a, ()>;
}

/// The owning Source's current Unit with its current Read decision.
#[derive(Debug, Clone)]
pub struct CurrentSourceUnit {
    pub unit: KnowledgeUnit,
    pub authority: VectorAuthorityInput,
    pub read: AccessDecision,
}

/// The owning Source's current view of the Unit a hit names.
#[derive(Debug, Clone)]
pub enum SourceUnitState {
    Current(Box<CurrentSourceUnit>),
    /// Another Version is current, publication ended, or the Part changed.
    NotCurrent,
}

pub trait VectorSourceResolverPort: Send + Sync {
    fn current_unit<'a>(
        &'a self,
        scope: &'a AuthorizedSourceScope,
        hit: &'a VectorHitRef,
    ) -> BoxFuture<'a, SourceUnitState>;
}

/// A hit the Source just confirmed; only this module can make one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceValidatedVectorCandidate {
    parent: ResourceId,
    hit: VectorHitRef,
    rank: usize,
}

impl SourceValidatedVectorCandidate {
    pub fn parent(&self) -> ResourceId {
        self.parent
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorResolution {
    Visible(Box<SourceValidatedVectorCandidate>),
    /// Denied or no longer current: nothing about the hit leaves.
    Suppressed,
    /// Unknown Read or an unavailable Source: never absence.
    Unavailable,
}

/// Resolves one index hit against the pinned generation and its Source.
pub async fn resolve_hit(
    resolver: &dyn VectorSourceResolverPort,
    spec: &EmbeddingModelSpec,
    pin: &PinnedVectorGeneration,
    scope: &AuthorizedSourceScope,
    ranked: &RankedVectorHit,
) -> VectorResolution {
    // A hit from another model or generation is an index fault: fail closed.
    if ranked.model_id != pin.manifest().model_id
        || ranked.hit.generation != pin.key()
        || ranked.hit.version.source_id != scope.source_id()
    {
        return VectorResolution::Unavailable;
    }
    match resolver.current_unit(scope, &ranked.hit).await {
        Err(_) => VectorResolution::Unavailable,
        Ok(SourceUnitState::NotCurrent) => VectorResolution::Suppressed,
        Ok(SourceUnitState::Current(current)) => {
            if ranked
                .validate(spec, &current.unit, &current.authority)
                .is_err()
            {
                return VectorResolution::Suppressed;
            }
            match current.read {
                AccessDecision::Allowed => {
                    VectorResolution::Visible(Box::new(SourceValidatedVectorCandidate {
                        parent: current.unit.version.resource_id,
                        hit: ranked.hit.clone(),
                        rank: ranked.rank,
                    }))
                }
                AccessDecision::Denied => VectorResolution::Suppressed,
                AccessDecision::Unknown => VectorResolution::Unavailable,
            }
        }
    }
}

/// Parent candidates in first-hit order and the retriever-internal trace.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorRetrievalBatch {
    generation: Option<ProjectionGenerationKey>,
    candidates: Vec<FederatedCandidate>,
    /// `(rank, raw similarity)` of visible hits only; never fused.
    trace: Vec<(usize, f32)>,
    unavailable: bool,
}

impl VectorRetrievalBatch {
    fn unavailable() -> Self {
        Self {
            generation: None,
            candidates: vec![],
            trace: vec![],
            unavailable: true,
        }
    }

    pub fn candidates(&self) -> &[FederatedCandidate] {
        &self.candidates
    }

    pub fn trace(&self) -> &[(usize, f32)] {
        &self.trace
    }

    pub fn is_unavailable(&self) -> bool {
        self.unavailable
    }

    /// A blocking, ID-free gap when Vector could not run completely.
    pub fn gap(&self) -> Option<InformationGap> {
        self.unavailable
            .then(|| InformationGap::new("vector_unavailable", GapReason::Availability, true))
    }

    /// The hard-gated S1 list; raw similarity never becomes a fusion score.
    pub fn rank_list(
        &self,
        retriever_id: &str,
        gates: impl Fn(&FederatedCandidate) -> CandidateHardGates,
    ) -> Option<RetrieverRankList> {
        if self.unavailable {
            return None;
        }
        let generation = self.generation?;
        Some(RetrieverRankList {
            retriever_id: retriever_id.to_owned(),
            generation,
            hits: self
                .candidates
                .iter()
                .map(|candidate| RankedCandidateHit {
                    candidate: candidate.clone(),
                    hard_gates: gates(candidate),
                    identity_evidence: vec![],
                    raw_score: None,
                    evidence_refs: vec![],
                })
                .collect(),
        })
    }
}

/// The Discovery executor's Vector port: one Source's candidates for the
/// pinned P1 bundle `generation`. The host binds it to the actor's
/// authorized scope of that Source and to the Source's registered model.
pub trait VectorExecutionPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        text: &'a str,
        window: usize,
    ) -> BoxFuture<'a, VectorRetrievalBatch>;
}

/// The Vector façade over the trusted ports.
pub struct VectorRetriever<'a> {
    pub provider: &'a dyn EmbeddingProvider,
    pub index: &'a dyn VectorIndexPort,
    pub generations: &'a dyn VectorGenerationPort,
    pub resolver: &'a dyn VectorSourceResolverPort,
}

impl VectorRetriever<'_> {
    /// `bundle` is the P1 key this evaluation already pinned for the Source.
    pub async fn retrieve(
        &self,
        bundle: ProjectionGenerationKey,
        query: &TrustedVectorQuery,
        now: OffsetDateTime,
    ) -> Result<VectorRetrievalBatch, SearchError> {
        let spec = self.provider.spec();
        if query.scope().source_id() != bundle.source_id
            || spec.validate_and_id().ok().as_ref() != Some(query.model_id())
        {
            return Err(invalid("Vector query binding"));
        }
        let Some(pin) = self
            .generations
            .pin_current(bundle, query.model_id())
            .await?
        else {
            // No READY generation for this exact P1 key: skip, never absence.
            return Ok(VectorRetrievalBatch::unavailable());
        };
        if pin.key() != bundle
            || pin.manifest().model_id != *query.model_id()
            || pin
                .manifest()
                .lease_expires_at
                .is_some_and(|expiry| expiry <= now)
        {
            return Ok(VectorRetrievalBatch::unavailable());
        }
        let embedded = self.provider.embed_query(query).await?;
        if embedded.model_id() != query.model_id() {
            return Err(invalid("Vector query model"));
        }
        let hits = self.index.search(&pin, &embedded, query.window()).await?;
        if hits.len() > query.window()
            || hits
                .iter()
                .enumerate()
                .any(|(position, hit)| hit.rank != position + 1)
        {
            return Err(invalid("Vector window or rank order"));
        }
        let mut batch = VectorRetrievalBatch {
            generation: Some(bundle),
            candidates: vec![],
            trace: vec![],
            unavailable: false,
        };
        let mut parents = BTreeSet::new();
        for ranked in &hits {
            match resolve_hit(self.resolver, spec, &pin, query.scope(), ranked).await {
                VectorResolution::Visible(candidate) => {
                    batch.trace.push((candidate.rank, ranked.raw_similarity));
                    if parents.insert(candidate.parent) {
                        let mut federated = FederatedCandidate::new(
                            format!(
                                "{}:{}",
                                bundle.source_id.as_uuid(),
                                candidate.parent.as_uuid()
                            ),
                            CandidateIdentityClass::DurableResource,
                            bundle.source_id,
                            "vector",
                        );
                        federated.resource_ref = Some(candidate.parent);
                        batch.candidates.push(federated);
                    }
                }
                VectorResolution::Suppressed => {}
                VectorResolution::Unavailable => batch.unavailable = true,
            }
        }
        Ok(batch)
    }
}

impl crate::ports::VectorRetrieverPort for VectorRetriever<'_> {
    fn retrieve<'a>(
        &'a self,
        bundle: ProjectionGenerationKey,
        query: &'a TrustedVectorQuery,
        now: OffsetDateTime,
    ) -> BoxFuture<'a, VectorRetrievalBatch> {
        Box::pin(VectorRetriever::retrieve(self, bundle, query, now))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorBuildOutcome {
    Published(Box<VectorStageReceipt>),
    /// The staged index did not hold exactly the manifest's entries.
    Rejected,
    /// The P1 bundle moved on, or the scope was purged, before publication.
    LostCas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VectorRecovery {
    pub kept: usize,
    pub withdrawn: usize,
    pub orphans_discarded: usize,
}

/// Builds, publishes, recovers and purges Vector generations.
pub struct VectorLifecycle<'a> {
    pub provider: &'a dyn EmbeddingProvider,
    pub index: &'a dyn VectorIndexPort,
    pub generations: &'a dyn VectorGenerationPort,
    pub activations: &'a dyn VectorActivationPort,
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

impl VectorLifecycle<'_> {
    /// One build of the complete canonical Unit set at `input`'s P1 key.
    /// `previous` embeddings are reused only under the identical cache key
    /// and are re-bound to this generation; everything else is embedded.
    pub async fn build(
        &self,
        input: &VectorManifestInput,
        previous: &[BoundEmbedding],
        storage: VectorStorageKind,
        now: OffsetDateTime,
    ) -> Result<VectorBuildOutcome, SearchError> {
        let spec = self.provider.spec();
        let model = spec
            .validate_and_id()
            .map_err(|_| invalid("Vector model"))?;
        let registered = active_model(self.activations, input.bundle_key.source_id).await?;
        if registered.validate_and_id().ok().as_ref() != Some(&model) {
            return Err(invalid("Vector model is not the registered one"));
        }
        let scope_epoch = self
            .generations
            .scope_epoch(&input.authority_scope_key)
            .await?;
        let nonindexed: BTreeSet<_> = input
            .nonindexed_retention_unit_ids
            .iter()
            .copied()
            .collect();
        let mut embeddings = Vec::new();
        let mut missing = Vec::new();
        for item in input
            .units
            .iter()
            .filter(|item| !nonindexed.contains(&item.unit.unit_id))
        {
            let key = cache_key(&model, item);
            match previous
                .iter()
                .find(|embedding| embedding.entry_ref().cache_key == key)
            {
                Some(reused) => embeddings.push(
                    BoundEmbedding::new(
                        spec,
                        &item.unit,
                        &item.authority,
                        reused.values().to_vec(),
                    )
                    .map_err(|_| invalid("Vector rebinding"))?,
                ),
                None => missing.push(item.clone()),
            }
        }
        let fresh = self.provider.embed_units(&missing).await?;
        if fresh.len() != missing.len() {
            return Err(invalid("Vector embedding count"));
        }
        for (item, embedding) in missing.iter().zip(&fresh) {
            embedding
                .validate_binding(spec, &item.unit, &item.authority)
                .map_err(|_| invalid("Vector embedding binding"))?;
        }
        embeddings.extend(fresh);
        let entries: Vec<VectorEntryRef> =
            embeddings.iter().map(BoundEmbedding::entry_ref).collect();
        let descriptor = self
            .index
            .stage(input.bundle_key, &model, &embeddings)
            .await?;
        let staged = match self.index.staged_entries(&descriptor).await {
            Ok(staged) => staged,
            Err(error) => {
                let _ = self.index.discard(&descriptor).await;
                return Err(error);
            }
        };
        let manifest = match VectorProjectionManifest::stage(
            spec,
            input,
            descriptor.clone(),
            &entries,
            storage,
            now,
        )
        .and_then(|manifest| {
            manifest
                .validate_against(input, &staged)
                .map(|receipt| (manifest, receipt))
        }) {
            Ok(checked) => checked,
            Err(_) => {
                self.index.discard(&descriptor).await?;
                return Ok(VectorBuildOutcome::Rejected);
            }
        };
        if !self
            .generations
            .publish_if_current(&manifest.0, scope_epoch)
            .await?
        {
            self.index.discard(&descriptor).await?;
            return Ok(VectorBuildOutcome::LostCas);
        }
        Ok(VectorBuildOutcome::Published(Box::new(manifest.1)))
    }

    /// Restart: keep only published manifests whose index still validates
    /// against the P1 input they name; discard stages nothing published.
    pub async fn recover(
        &self,
        inputs: &[VectorManifestInput],
        now: OffsetDateTime,
    ) -> Result<VectorRecovery, SearchError> {
        let mut recovery = VectorRecovery::default();
        let mut live = Vec::new();
        for manifest in self.generations.published().await? {
            let expired = manifest
                .lease_expires_at
                .is_some_and(|expiry| expiry <= now);
            let valid = match inputs
                .iter()
                .find(|input| input.bundle_key == manifest.bundle_key)
            {
                Some(input) if !expired => self
                    .index
                    .staged_entries(&manifest.index)
                    .await
                    .is_ok_and(|staged| manifest.validate_against(input, &staged).is_ok()),
                _ => false,
            };
            if valid {
                recovery.kept += 1;
                live.push(manifest.index.clone());
            } else {
                self.generations.withdraw(&manifest).await?;
                recovery.withdrawn += 1;
            }
        }
        for stage in self.index.stages().await? {
            if !live.contains(&stage) {
                self.index.discard(&stage).await?;
                recovery.orphans_discarded += 1;
            }
        }
        Ok(recovery)
    }

    /// Revocation, expiry, scope change or cancellation of one authority
    /// scope: withdraw its generations and purge its entries.
    pub async fn purge(&self, authority_scope_key: &str) -> Result<usize, SearchError> {
        // First: no build that began before the purge can publish after it.
        self.generations
            .advance_scope_epoch(authority_scope_key)
            .await?;
        for manifest in self.generations.published().await? {
            if manifest.authority_scope_key == authority_scope_key {
                self.generations.withdraw(&manifest).await?;
            }
        }
        self.index.purge_scope(authority_scope_key).await
    }
}
