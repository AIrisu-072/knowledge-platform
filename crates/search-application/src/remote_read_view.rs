//! P4-11: explicit-key composite read view for one Discovery evaluation.
//!
//! Every generation key the evaluation reads is registered explicitly: a
//! durable pin when `pin_current` returns it, a sealed remote generation when
//! the evaluation seals one. A key that was never registered reads nothing;
//! there is no fallback by generation UUID. Remote reads go through the
//! generation's lease-guarded store and return owned copies. A remote Source
//! gets a server-owned, non-sensitive concept view and server-owned Claim
//! selectors; provider data never becomes a selector or a concept relation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, RwLock};

use search_core::assertion::Assertion;
use search_core::discovery::FederatedCandidate;
use search_core::id::{ClaimId, ResourceId, SourceId};
use search_core::predicate::{ConceptResolver, TruthValue, TypedValue};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};

use crate::SearchError;
use crate::ports::{
    AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort, ConceptRegistryPort,
    EvidenceResolverPort, GenerationReadPort, ResolvedAssertionEvidence, SealedRemoteRetrieverPort,
};
use crate::remote_evidence::resolved_evidence;
use crate::remote_generation::{REMOTE_CLAIM_SUBJECT, RemoteEvaluationGeneration};
use crate::remote_lease::{
    GuardedRemoteStore, LeaseClock, LeaseState, RemoteLease, RemoteOwner, RemoteOwnerGate,
};
use crate::retrieval::RetrievalAction;

const GENERATION: &str = "generation";

/// The remote retriever ID a planned Discovery action is sealed under. The
/// planner's `source:kind` ID uses a character remote IDs do not allow.
pub(crate) fn sealed_retriever_id(retriever_id: &str) -> String {
    retriever_id.replace(':', ".")
}

/// Server-owned remote Claim selectors: ClaimId → predicate and optional
/// expected value, always about `REMOTE_CLAIM_SUBJECT`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteClaimSelectors {
    selectors: BTreeMap<ClaimId, (String, Option<TypedValue>)>,
}

impl RemoteClaimSelectors {
    pub fn new(selectors: Vec<(ClaimId, String, Option<TypedValue>)>) -> Self {
        Self {
            selectors: selectors
                .into_iter()
                .map(|(claim, predicate, expected)| (claim, (predicate, expected)))
                .collect(),
        }
    }
}

/// No remote (or unpinned) concept relation is proven: every question other
/// than identity is unknown.
struct UnknownConcepts;

impl ConceptResolver for UnknownConcepts {
    fn same_concept(&self, left: &str, right: &str) -> TruthValue {
        if left == right {
            TruthValue::True
        } else {
            TruthValue::Unknown
        }
    }
    fn is_a(&self, _child: &str, _parent: &str) -> TruthValue {
        TruthValue::Unknown
    }
    fn descendant_of(&self, _child: &str, _ancestor: &str) -> TruthValue {
        TruthValue::Unknown
    }
}

struct RemoteEntry {
    owner: RemoteOwner,
    store: GuardedRemoteStore<Arc<RemoteEvaluationGeneration>>,
}

pub struct CompositeEvaluationReadView<'a> {
    durable: &'a dyn GenerationReadPort,
    durable_concepts: &'a dyn ConceptRegistryPort,
    durable_selectors: &'a dyn ClaimSelectorPort,
    durable_assertions: &'a dyn AssertionStorePort,
    durable_evidence: &'a dyn EvidenceResolverPort,
    remote_selectors: &'a RemoteClaimSelectors,
    gate: &'a dyn RemoteOwnerGate,
    clock: Arc<dyn LeaseClock>,
    remote: RwLock<BTreeMap<ProjectionGenerationKey, Arc<RemoteEntry>>>,
    remote_sources: RwLock<BTreeMap<SourceId, ProjectionGenerationKey>>,
    durable_keys: RwLock<BTreeSet<ProjectionGenerationKey>>,
}

impl fmt::Debug for CompositeEvaluationReadView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CompositeEvaluationReadView(<evaluation-owned>)")
    }
}

fn unavailable() -> SearchError {
    SearchError::SourceUnavailable("evaluation read view unavailable".into())
}

#[allow(clippy::too_many_arguments)]
impl<'a> CompositeEvaluationReadView<'a> {
    pub fn new(
        durable: &'a dyn GenerationReadPort,
        durable_concepts: &'a dyn ConceptRegistryPort,
        durable_selectors: &'a dyn ClaimSelectorPort,
        durable_assertions: &'a dyn AssertionStorePort,
        durable_evidence: &'a dyn EvidenceResolverPort,
        remote_selectors: &'a RemoteClaimSelectors,
        gate: &'a dyn RemoteOwnerGate,
        clock: Arc<dyn LeaseClock>,
    ) -> Self {
        Self {
            durable,
            durable_concepts,
            durable_selectors,
            durable_assertions,
            durable_evidence,
            remote_selectors,
            gate,
            clock,
            remote: RwLock::new(BTreeMap::new()),
            remote_sources: RwLock::new(BTreeMap::new()),
            durable_keys: RwLock::new(BTreeSet::new()),
        }
    }

    /// Registers one sealed generation under its own lease. A Source has at
    /// most one remote generation per evaluation; a key never collides.
    pub async fn register_remote(
        &self,
        generation: RemoteEvaluationGeneration,
        lease: RemoteLease,
    ) -> Result<ProjectionGenerationKey, SearchError> {
        let key = generation.key();
        {
            let remote = self.remote.read().map_err(|_| unavailable())?;
            let sources = self.remote_sources.read().map_err(|_| unavailable())?;
            let durable = self.durable_keys.read().map_err(|_| unavailable())?;
            if remote.contains_key(&key)
                || sources.contains_key(&key.source_id)
                || durable
                    .iter()
                    .any(|pinned| pinned.source_id == key.source_id)
            {
                return Err(SearchError::OperationFailed(
                    "one Source has one generation domain per evaluation".into(),
                ));
            }
        }
        let owner = RemoteOwner::for_evaluation(generation.context());
        let store = GuardedRemoteStore::new(owner.clone(), lease, self.clock.clone(), 1);
        store.open()?;
        store
            .write(&owner, GENERATION, Arc::new(generation), self.gate)
            .await?;
        self.remote
            .write()
            .map_err(|_| unavailable())?
            .insert(key, Arc::new(RemoteEntry { owner, store }));
        self.remote_sources
            .write()
            .map_err(|_| unavailable())?
            .insert(key.source_id, key);
        Ok(key)
    }

    /// Ends the evaluation: every remote lease closes and nothing is readable.
    pub fn close(&self) {
        if let Ok(remote) = self.remote.read() {
            for entry in remote.values() {
                entry.store.close();
            }
        }
    }

    /// Source revoked mid-evaluation: its generation is gone at once.
    pub fn revoke_source(&self, source: SourceId) {
        let key = self
            .remote_sources
            .read()
            .ok()
            .and_then(|sources| sources.get(&source).copied());
        if let Some(key) = key
            && let Ok(remote) = self.remote.read()
            && let Some(entry) = remote.get(&key)
        {
            entry.store.revoke();
        }
    }

    /// Whether every remote lease of this evaluation has ended.
    pub fn is_closed(&self) -> bool {
        self.remote
            .read()
            .map(|remote| {
                remote.values().all(|entry| {
                    matches!(
                        entry.store.state(),
                        LeaseState::Closed | LeaseState::Revoked | LeaseState::Expired
                    )
                })
            })
            .unwrap_or(false)
    }

    pub fn is_remote(&self, key: ProjectionGenerationKey) -> bool {
        self.remote
            .read()
            .map(|remote| remote.contains_key(&key))
            .unwrap_or(false)
    }

    /// The remote key pinned for a Source in this evaluation, if any.
    pub fn remote_key(&self, source: SourceId) -> Option<ProjectionGenerationKey> {
        self.remote_sources
            .read()
            .ok()
            .and_then(|sources| sources.get(&source).copied())
    }

    async fn remote_generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<Option<Arc<RemoteEvaluationGeneration>>, SearchError> {
        let entry = self
            .remote
            .read()
            .map_err(|_| unavailable())?
            .get(&key)
            .cloned();
        match entry {
            None => Ok(None),
            Some(entry) => entry.store.read(&entry.owner, GENERATION, self.gate).await,
        }
    }

    fn durable_registered(&self, key: ProjectionGenerationKey) -> bool {
        self.durable_keys
            .read()
            .map(|keys| keys.contains(&key))
            .unwrap_or(false)
    }
}

impl GenerationReadPort for CompositeEvaluationReadView<'_> {
    fn pin_current<'b>(
        &'b self,
        source_id: SourceId,
    ) -> BoxFuture<'b, Option<ProjectionGenerationManifest>> {
        Box::pin(async move {
            if let Some(key) = self.remote_key(source_id) {
                return Ok(self
                    .remote_generation(key)
                    .await?
                    .map(|generation| generation.manifest().clone()));
            }
            let manifest = self.durable.pin_current(source_id).await?;
            if let Some(manifest) = &manifest {
                self.durable_keys
                    .write()
                    .map_err(|_| unavailable())?
                    .insert(manifest.key());
            }
            Ok(manifest)
        })
    }

    fn resource_at<'b>(
        &'b self,
        key: ProjectionGenerationKey,
        resource_id: ResourceId,
    ) -> BoxFuture<'b, Option<CompiledResourceProjection>> {
        Box::pin(async move {
            if self.is_remote(key) {
                return Ok(self
                    .remote_generation(key)
                    .await?
                    .and_then(|generation| generation.projection(resource_id).cloned()));
            }
            if self.durable_registered(key) {
                return self.durable.resource_at(key, resource_id).await;
            }
            Ok(None)
        })
    }
}

impl ClaimSelectorPort for CompositeEvaluationReadView<'_> {
    fn selector_for<'b>(
        &'b self,
        generation: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'b, Option<ClaimSelector>> {
        Box::pin(async move {
            if self.is_remote(generation) {
                return Ok(self.remote_selectors.selectors.get(&claim_id).map(
                    |(predicate, expected)| ClaimSelector {
                        claim_id,
                        subject_ref: REMOTE_CLAIM_SUBJECT.into(),
                        predicate: predicate.clone(),
                        expected_value: expected.clone(),
                    },
                ));
            }
            if self.durable_registered(generation) {
                return self
                    .durable_selectors
                    .selector_for(generation, claim_id)
                    .await;
            }
            Ok(None)
        })
    }
}

impl AssertionStorePort for CompositeEvaluationReadView<'_> {
    fn assertions_for<'b>(
        &'b self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        predicate: &'b str,
    ) -> BoxFuture<'b, Vec<Assertion>> {
        Box::pin(async move {
            if self.is_remote(generation) {
                return Ok(self
                    .remote_generation(generation)
                    .await?
                    .map(|sealed| sealed.assertions(resource_ref, predicate))
                    .unwrap_or_default());
            }
            if self.durable_registered(generation) {
                return self
                    .durable_assertions
                    .assertions_for(generation, resource_ref, predicate)
                    .await;
            }
            Ok(vec![])
        })
    }
}

impl EvidenceResolverPort for CompositeEvaluationReadView<'_> {
    fn resolve<'b>(
        &'b self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        evidence_ref: &'b str,
    ) -> BoxFuture<'b, Option<ResolvedAssertionEvidence>> {
        Box::pin(async move {
            if self.is_remote(generation) {
                return Ok(self
                    .remote_generation(generation)
                    .await?
                    .and_then(|sealed| {
                        sealed
                            .evidence(resource_ref, evidence_ref)
                            .map(|verified| resolved_evidence(generation, resource_ref, verified))
                    }));
            }
            if self.durable_registered(generation) {
                return self
                    .durable_evidence
                    .resolve(generation, resource_ref, evidence_ref)
                    .await;
            }
            Ok(None)
        })
    }
}

impl ConceptRegistryPort for CompositeEvaluationReadView<'_> {
    fn pin_view<'b>(
        &'b self,
        generation: ProjectionGenerationKey,
    ) -> BoxFuture<'b, Arc<dyn ConceptResolver + Send + Sync>> {
        Box::pin(async move {
            if self.durable_registered(generation) {
                return self.durable_concepts.pin_view(generation).await;
            }
            let resolver: Arc<dyn ConceptResolver + Send + Sync> = Arc::new(UnknownConcepts);
            Ok(resolver)
        })
    }

    fn same_concept<'b>(
        &'b self,
        generation: ProjectionGenerationKey,
        left: &'b str,
        right: &'b str,
    ) -> BoxFuture<'b, TruthValue> {
        Box::pin(async move {
            if self.durable_registered(generation) {
                return self
                    .durable_concepts
                    .same_concept(generation, left, right)
                    .await;
            }
            Ok(UnknownConcepts.same_concept(left, right))
        })
    }

    fn is_a<'b>(
        &'b self,
        generation: ProjectionGenerationKey,
        child: &'b str,
        parent: &'b str,
    ) -> BoxFuture<'b, TruthValue> {
        Box::pin(async move {
            if self.durable_registered(generation) {
                return self.durable_concepts.is_a(generation, child, parent).await;
            }
            Ok(UnknownConcepts.is_a(child, parent))
        })
    }
}

impl SealedRemoteRetrieverPort for CompositeEvaluationReadView<'_> {
    fn retrieve<'b>(
        &'b self,
        action: &'b RetrievalAction,
        key: ProjectionGenerationKey,
    ) -> BoxFuture<'b, Vec<FederatedCandidate>> {
        Box::pin(async move {
            if action.source_id != key.source_id || self.remote_key(action.source_id) != Some(key) {
                return Err(SearchError::InvalidRequest(
                    "remote action is not bound to its Source's sealed generation".into(),
                ));
            }
            let sealed = self.remote_generation(key).await?.ok_or_else(unavailable)?;
            sealed
                .candidates(&sealed_retriever_id(&action.retriever_id))
                .ok_or_else(|| {
                    SearchError::SourceUnavailable("remote action did not complete".into())
                })
        })
    }
}
