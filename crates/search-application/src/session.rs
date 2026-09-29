//! Session-owned bindings and transient materialization state.

use std::collections::{BTreeMap, btree_map::Entry};

use search_core::binding::{RepresentationBinding, SessionBindingSet};
use search_core::id::{BindingId, ResourceId, SourceId};
use search_core::materialization::{MaterializationState, ProbeOutcome};
use search_core::projection::ProjectionGenerationKey;

use crate::error::SearchError;
use crate::materialization::{MaterializationService, ProbeRequest, ProbeResult};
use crate::ports::{
    BoxFuture, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
    CurrentMaterializationStatePort, CurrentSourcePolicyPort, MaterializationReceipt,
    MaterializationRequest, MaterializerPort, PersistableMaterializationRecord, ProbePort,
};

/// Source identity is part of every session lookup, including identical UUIDs
/// discovered through another Source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BoundResourceKey {
    pub source_ref: SourceId,
    pub resource_ref: ResourceId,
}

impl BoundResourceKey {
    pub const fn new(source_ref: SourceId, resource_ref: ResourceId) -> Self {
        Self {
            source_ref,
            resource_ref,
        }
    }
}

struct WorkingResource {
    generation: ProjectionGenerationKey,
    candidate_id: String,
    binding: RepresentationBinding,
    state: MaterializationState,
    probe: Option<ProbeResult>,
    receipt: Option<MaterializationReceipt>,
}

/// Nothing in this type is serializable. In particular, a NO_RETENTION body
/// stays in memory for the lifetime of this working set only.
///
/// ```compile_fail
/// let set = search_application::session::SessionWorkingSet::default();
/// let _serialized = serde_json::to_string(&set).unwrap();
/// ```
#[derive(Default)]
pub struct SessionWorkingSet {
    bindings: SessionBindingSet,
    binding_targets: BTreeMap<BindingId, BoundResourceKey>,
    resources: BTreeMap<BoundResourceKey, WorkingResource>,
}

impl SessionWorkingSet {
    /// Repeated discovery may confirm a binding, but cannot silently replace
    /// its representation, version, digests, Source or original generation.
    pub fn bind_resource(
        &mut self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        candidate_id: impl Into<String>,
        binding: RepresentationBinding,
    ) -> Result<&RepresentationBinding, SearchError> {
        let candidate_id = candidate_id.into();
        if generation.source_id != binding.source_ref || candidate_id.is_empty() {
            return Err(SearchError::InvalidRequest(
                "session binding lacks a matching Source or candidate identity".into(),
            ));
        }
        binding
            .validate()
            .map_err(|reason| SearchError::InvalidRequest(reason.into()))?;
        let key = BoundResourceKey::new(binding.source_ref, resource_ref);
        if self.resources.get(&key).is_some_and(|existing| {
            existing.binding != binding || existing.candidate_id != candidate_id
        }) {
            return Err(SearchError::InvalidRequest(
                "rediscovery conflicts with the session binding".into(),
            ));
        }
        if self
            .binding_targets
            .get(&binding.binding_id)
            .is_some_and(|existing| *existing != key)
        {
            return Err(SearchError::InvalidRequest(
                "BindingId already targets another resource".into(),
            ));
        }
        self.bindings
            .bind_if_absent(binding.clone())
            .map_err(|reason| SearchError::InvalidRequest(reason.into()))?;
        match self.resources.entry(key) {
            Entry::Occupied(existing) => Ok(&existing.into_mut().binding),
            Entry::Vacant(slot) => {
                self.binding_targets.insert(binding.binding_id, key);
                Ok(&slot
                    .insert(WorkingResource {
                        generation,
                        candidate_id,
                        binding,
                        state: MaterializationState::ReferenceOnly,
                        probe: None,
                        receipt: None,
                    })
                    .binding)
            }
        }
    }

    pub fn bound(&self, key: BoundResourceKey) -> Option<&RepresentationBinding> {
        self.resources.get(&key).map(|entry| &entry.binding)
    }

    pub fn pinned_generation(&self, key: BoundResourceKey) -> Option<ProjectionGenerationKey> {
        self.resources.get(&key).map(|entry| entry.generation)
    }

    fn exact_entry(
        &self,
        key: BoundResourceKey,
        binding: &RepresentationBinding,
    ) -> Option<&WorkingResource> {
        self.resources
            .get(&key)
            .filter(|entry| entry.binding == *binding)
    }

    pub fn state(
        &self,
        key: BoundResourceKey,
        binding: &RepresentationBinding,
    ) -> Option<MaterializationState> {
        self.exact_entry(key, binding).map(|entry| entry.state)
    }

    /// Durable conversion is delegated to C7's policy-verified receipt.
    pub fn durable_record(
        &self,
        key: BoundResourceKey,
        binding: &RepresentationBinding,
    ) -> Result<Option<PersistableMaterializationRecord>, SearchError> {
        let entry = self.exact_entry(key, binding).ok_or_else(|| {
            SearchError::InvalidRequest("unknown session representation binding".into())
        })?;
        entry
            .receipt
            .as_ref()
            .map(MaterializationReceipt::to_session_store_record)
            .transpose()
    }

    /// ProbeResult constructors are public for Source adapters. They cannot be
    /// injected into this state: only C7's validated service result is recorded.
    pub async fn probe_and_record(
        &mut self,
        binding_id: BindingId,
        port: &dyn ProbePort,
        access_evaluator: &dyn CurrentCandidateAccessEvaluatorPort,
        source_policy: &dyn CurrentSourcePolicyPort,
        request: &ProbeRequest,
    ) -> Result<&ProbeResult, SearchError> {
        let key = *self.binding_targets.get(&binding_id).ok_or_else(|| {
            SearchError::InvalidRequest("unknown session representation binding".into())
        })?;
        let entry = self
            .resources
            .get(&key)
            .expect("binding target is registered");
        if request.candidate.source_ref != key.source_ref
            || request.candidate.resource_ref != Some(key.resource_ref)
            || request.candidate.candidate_id != entry.candidate_id
            || request.candidate.logical_resource_ref != Some(entry.binding.logical_resource_ref)
        {
            return Err(SearchError::InvalidRequest(
                "probe target differs from the bound candidate".into(),
            ));
        }
        let result =
            MaterializationService::probe(port, access_evaluator, source_policy, request).await?;
        let entry = self
            .resources
            .get_mut(&key)
            .expect("binding target is registered");
        if matches!(
            result.outcome(),
            ProbeOutcome::Found | ProbeOutcome::NotFoundByProbe
        ) {
            entry.state = entry.state.max(MaterializationState::Probed);
        }
        entry.probe = Some(result);
        Ok(entry.probe.as_ref().expect("just recorded"))
    }

    /// Updates the working stage only after C7 validates the current Source
    /// grant, exact bound state, receipt identity and returned bytes.
    pub async fn materialize(
        &mut self,
        port: &dyn MaterializerPort,
        access_evaluator: &dyn CurrentAccessEvaluatorPort,
        source_policy: &dyn CurrentSourcePolicyPort,
        request: &MaterializationRequest,
    ) -> Result<&MaterializationReceipt, SearchError> {
        let key = BoundResourceKey::new(request.binding.source_ref, request.resource_ref);
        if self.exact_entry(key, &request.binding).is_none() {
            return Err(SearchError::InvalidRequest(
                "unknown session representation binding".into(),
            ));
        }
        let receipt = MaterializationService::materialize(
            self,
            port,
            access_evaluator,
            source_policy,
            request,
        )
        .await?;
        let entry = self
            .resources
            .get_mut(&key)
            .expect("exact binding was checked");
        entry.state = receipt.achieved_state();
        entry.receipt = Some(receipt);
        Ok(entry.receipt.as_ref().expect("just recorded"))
    }
}

impl CurrentMaterializationStatePort for SessionWorkingSet {
    fn for_resource<'a>(
        &'a self,
        source_ref: SourceId,
        resource_ref: ResourceId,
        binding: &'a RepresentationBinding,
        access_context: &'a str,
    ) -> BoxFuture<'a, Option<MaterializationState>> {
        Box::pin(async move {
            if access_context.is_empty() || binding.source_ref != source_ref {
                return Ok(None);
            }
            Ok(self.state(BoundResourceKey::new(source_ref, resource_ref), binding))
        })
    }
}
