//! P4-08: a `SESSION_ONLY` remote working set behind the owner gate.
//!
//! The wrapper owns one `SessionWorkingSet` for one trusted session owner
//! with an absolute and an idle deadline. Every bind, read, probe and
//! materialization checks the owner, the lease and the current gate first and
//! returns owned values only; close or expiry drops the whole working set.
//! `NO_RETENTION` and every other mode never enter a session.

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use search_core::binding::RepresentationBinding;
use search_core::id::{BindingId, ResourceId};
use search_core::materialization::MaterializationState;
use search_core::projection::ProjectionGenerationKey;
use search_core::source::RetentionMode;

use crate::SearchError;
use crate::materialization::{ProbeRequest, ProbeResult};
use crate::ports::{
    CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort, CurrentSourcePolicyPort,
    MaterializationRequest, MaterializerPort, ProbePort,
};
use crate::remote::TrustedRemoteContext;
use crate::remote_lease::{
    LeaseClock, LeaseState, OwnerDecision, RemoteLease, RemoteOwner, RemoteOwnerGate,
    lease_unavailable,
};
use crate::session::{BoundResourceKey, SessionWorkingSet};

pub struct ScopedSessionWorkingSet {
    owner: RemoteOwner,
    lease: RemoteLease,
    clock: Arc<dyn LeaseClock>,
    state: LeaseState,
    last_used: Instant,
    inner: Option<SessionWorkingSet>,
}

impl fmt::Debug for ScopedSessionWorkingSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ScopedSessionWorkingSet(<session-owned>)")
    }
}

impl ScopedSessionWorkingSet {
    /// Only a `SESSION_ONLY` Source of a trusted session opens a working set.
    pub fn open(
        context: &TrustedRemoteContext,
        lease: RemoteLease,
        clock: Arc<dyn LeaseClock>,
    ) -> Result<Self, SearchError> {
        if context.registration().retention_mode() != RetentionMode::SessionOnly
            || lease.idle_timeout.is_none()
        {
            return Err(lease_unavailable());
        }
        let owner = RemoteOwner::for_session(context)?;
        let now = clock.now();
        Ok(Self {
            owner,
            lease,
            clock,
            state: LeaseState::Open,
            last_used: now,
            inner: Some(SessionWorkingSet::default()),
        })
    }

    pub const fn state(&self) -> LeaseState {
        self.state
    }

    fn end(&mut self, state: LeaseState) {
        self.inner = None;
        if !matches!(self.state, LeaseState::Closed | LeaseState::Expired) {
            self.state = state;
        }
    }

    fn live(&mut self, owner: &RemoteOwner) -> Result<(), SearchError> {
        if *owner != self.owner || self.state != LeaseState::Open || self.inner.is_none() {
            return Err(lease_unavailable());
        }
        let now = self.clock.now();
        if now >= self.lease.absolute_deadline
            || self
                .lease
                .idle_timeout
                .is_some_and(|idle| now.saturating_duration_since(self.last_used) >= idle)
        {
            self.end(LeaseState::Expired);
            return Err(lease_unavailable());
        }
        Ok(())
    }

    async fn gate(
        &mut self,
        owner: &RemoteOwner,
        gate: &dyn RemoteOwnerGate,
    ) -> Result<&mut SessionWorkingSet, SearchError> {
        self.live(owner)?;
        if !matches!(gate.current(owner).await, Ok(OwnerDecision::Current)) {
            self.end(LeaseState::Revoked);
            return Err(lease_unavailable());
        }
        self.live(owner)?;
        self.last_used = self.clock.now();
        self.inner.as_mut().ok_or_else(lease_unavailable)
    }

    pub async fn bind(
        &mut self,
        owner: &RemoteOwner,
        gate: &dyn RemoteOwnerGate,
        generation: ProjectionGenerationKey,
        resource: ResourceId,
        candidate_id: &str,
        binding: RepresentationBinding,
    ) -> Result<RepresentationBinding, SearchError> {
        let set = self.gate(owner, gate).await?;
        set.bind_resource(generation, resource, candidate_id, binding)
            .cloned()
    }

    pub async fn read(
        &mut self,
        owner: &RemoteOwner,
        gate: &dyn RemoteOwnerGate,
        key: BoundResourceKey,
    ) -> Result<Option<RepresentationBinding>, SearchError> {
        let set = self.gate(owner, gate).await?;
        Ok(set.bound(key).cloned())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn probe(
        &mut self,
        owner: &RemoteOwner,
        gate: &dyn RemoteOwnerGate,
        binding: BindingId,
        port: &dyn ProbePort,
        access: &dyn CurrentCandidateAccessEvaluatorPort,
        policy: &dyn CurrentSourcePolicyPort,
        request: &ProbeRequest,
    ) -> Result<ProbeResult, SearchError> {
        let set = self.gate(owner, gate).await?;
        set.probe_and_record(binding, port, access, policy, request)
            .await
            .cloned()
    }

    /// Only the achieved stage leaves; content stays session-owned.
    pub async fn materialize(
        &mut self,
        owner: &RemoteOwner,
        gate: &dyn RemoteOwnerGate,
        port: &dyn MaterializerPort,
        access: &dyn CurrentAccessEvaluatorPort,
        policy: &dyn CurrentSourcePolicyPort,
        request: &MaterializationRequest,
    ) -> Result<MaterializationState, SearchError> {
        let set = self.gate(owner, gate).await?;
        let state = set
            .materialize(port, access, policy, request)
            .await
            .map(|receipt| receipt.achieved_state())?;
        // The gate may have been revoked while the Source read was in flight.
        if !matches!(gate.current(owner).await, Ok(OwnerDecision::Current)) {
            self.end(LeaseState::Revoked);
            return Err(lease_unavailable());
        }
        Ok(state)
    }

    pub fn close(&mut self) {
        if self.state == LeaseState::Open {
            self.inner = None;
            self.state = LeaseState::Closed;
        }
    }
}
