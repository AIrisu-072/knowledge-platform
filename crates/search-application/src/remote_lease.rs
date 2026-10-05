//! P4-07: retention owner, lease and guarded handle.
//!
//! Every derived remote value (projection, selector, assertion, evidence,
//! Graph, probe, receipt, content, trace) lives in a store owned by one
//! `RemoteOwner` under one `RemoteLease`. Each read and write checks the
//! owner, the lease and the current actor/Source/retention gate, and again
//! after the gate's await, immediately before returning an owned copy. Close,
//! expiry and revocation clear every entry together and never reopen. No
//! borrowed reference, `Arc`, iterator or `Deref` leaves the guard.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use search_core::id::DiscoveryEvaluationId;
use search_core::source::RetentionMode;

use crate::SearchError;
use crate::ports::{AccessDecision, BoxFuture};
use crate::remote::TrustedRemoteContext;
use crate::scoped::{
    AccessBindingState, AccessContextAuthorityPort, AuthorizedSourceScope,
    CurrentSourceVisibilityPort, TrustedSearchScope,
};

pub(crate) fn lease_unavailable() -> SearchError {
    SearchError::SourceUnavailable("remote lease unavailable".into())
}

/// Injected time source, so expiry is testable without sleeping.
pub trait LeaseClock: Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemLeaseClock;

impl LeaseClock for SystemLeaseClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Whose data this is: actor, Source scope, evaluation (or session) and the
/// retention mode the Source was registered with.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteOwner {
    actor: TrustedSearchScope,
    source: AuthorizedSourceScope,
    evaluation: Option<DiscoveryEvaluationId>,
    retention_mode: RetentionMode,
}

impl fmt::Debug for RemoteOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RemoteOwner(<opaque>)")
    }
}

impl RemoteOwner {
    /// Owner of one Discovery evaluation's derived values.
    pub fn for_evaluation(context: &TrustedRemoteContext) -> Self {
        Self {
            actor: context.binding().actor().clone(),
            source: context.source_scope().clone(),
            evaluation: Some(context.binding().evaluation()),
            retention_mode: context.registration().retention_mode(),
        }
    }

    /// Owner of a trusted session; refused without a session.
    pub fn for_session(context: &TrustedRemoteContext) -> Result<Self, SearchError> {
        if context.binding().actor().session().is_none() {
            return Err(lease_unavailable());
        }
        Ok(Self {
            actor: context.binding().actor().clone(),
            source: context.source_scope().clone(),
            evaluation: None,
            retention_mode: context.registration().retention_mode(),
        })
    }

    pub fn actor(&self) -> &TrustedSearchScope {
        &self.actor
    }
    pub fn source(&self) -> &AuthorizedSourceScope {
        &self.source
    }
    pub const fn evaluation(&self) -> Option<DiscoveryEvaluationId> {
        self.evaluation
    }
    pub const fn retention_mode(&self) -> RetentionMode {
        self.retention_mode
    }
}

/// Absolute deadline, optional idle window and optional provider expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteLease {
    pub absolute_deadline: Instant,
    pub idle_timeout: Option<Duration>,
    pub provider_expiry: Option<Instant>,
}

/// Monotone: `Building -> Open -> Closing -> Closed`, or any state to
/// `Revoked`/`Expired`. The last three never reopen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseState {
    Building,
    Open,
    Closing,
    Closed,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerDecision {
    Current,
    Revoked,
}

/// The current actor/Source/item/field and retention gate for an owner.
pub trait RemoteOwnerGate: Send + Sync {
    fn current<'a>(&'a self, owner: &'a RemoteOwner) -> BoxFuture<'a, OwnerDecision>;
}

/// Gate over the trusted authority and Source visibility ports. A retention
/// change is a registration revision change, which the visibility port
/// refuses for the old scope.
pub struct ScopedOwnerGate<'a> {
    authority: &'a dyn AccessContextAuthorityPort,
    visibility: &'a dyn CurrentSourceVisibilityPort,
}

impl<'a> ScopedOwnerGate<'a> {
    pub fn new(
        authority: &'a dyn AccessContextAuthorityPort,
        visibility: &'a dyn CurrentSourceVisibilityPort,
    ) -> Self {
        Self {
            authority,
            visibility,
        }
    }
}

impl RemoteOwnerGate for ScopedOwnerGate<'_> {
    fn current<'b>(&'b self, owner: &'b RemoteOwner) -> BoxFuture<'b, OwnerDecision> {
        Box::pin(async move {
            if !owner.actor.is_live()
                || owner.actor != *owner.source.actor()
                || self.authority.current(&owner.actor).await? != AccessBindingState::Current
                || self.visibility.current(&owner.source).await? != AccessDecision::Allowed
            {
                return Ok(OwnerDecision::Revoked);
            }
            Ok(OwnerDecision::Current)
        })
    }
}

struct Inner<T> {
    state: LeaseState,
    last_used: Instant,
    entries: BTreeMap<String, T>,
}

/// One owner's bounded RAM store. Values leave only as owned clones.
pub struct GuardedRemoteStore<T> {
    owner: RemoteOwner,
    lease: RemoteLease,
    clock: Arc<dyn LeaseClock>,
    max_entries: usize,
    inner: Mutex<Inner<T>>,
}

impl<T> fmt::Debug for GuardedRemoteStore<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GuardedRemoteStore(<lease-owned>)")
    }
}

impl<T: Clone> GuardedRemoteStore<T> {
    pub fn new(
        owner: RemoteOwner,
        lease: RemoteLease,
        clock: Arc<dyn LeaseClock>,
        max_entries: usize,
    ) -> Self {
        let now = clock.now();
        Self {
            owner,
            lease,
            clock,
            max_entries,
            inner: Mutex::new(Inner {
                state: LeaseState::Building,
                last_used: now,
                entries: BTreeMap::new(),
            }),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner<T>>, SearchError> {
        self.inner.lock().map_err(|_| lease_unavailable())
    }

    pub fn owner(&self) -> &RemoteOwner {
        &self.owner
    }

    pub fn state(&self) -> LeaseState {
        self.lock()
            .map(|inner| inner.state)
            .unwrap_or(LeaseState::Revoked)
    }

    /// `Building -> Open` only.
    pub fn open(&self) -> Result<(), SearchError> {
        let mut inner = self.lock()?;
        if inner.state != LeaseState::Building {
            return Err(lease_unavailable());
        }
        inner.state = LeaseState::Open;
        inner.last_used = self.clock.now();
        Ok(())
    }

    /// Owner, state and every deadline. Expiry clears and is final.
    fn live(&self, inner: &mut Inner<T>, owner: &RemoteOwner) -> Result<(), SearchError> {
        if *owner != self.owner || inner.state != LeaseState::Open {
            return Err(lease_unavailable());
        }
        let now = self.clock.now();
        let expired = now >= self.lease.absolute_deadline
            || self.lease.provider_expiry.is_some_and(|at| now >= at)
            || self
                .lease
                .idle_timeout
                .is_some_and(|idle| now.saturating_duration_since(inner.last_used) >= idle);
        if expired {
            inner.state = LeaseState::Expired;
            inner.entries.clear();
            return Err(lease_unavailable());
        }
        Ok(())
    }

    async fn gate(
        &self,
        owner: &RemoteOwner,
        gate: &dyn RemoteOwnerGate,
    ) -> Result<(), SearchError> {
        self.live(&mut *self.lock()?, owner)?;
        let decision = gate.current(owner).await;
        if !matches!(decision, Ok(OwnerDecision::Current)) {
            self.revoke();
            return Err(lease_unavailable());
        }
        Ok(())
    }

    pub async fn write(
        &self,
        owner: &RemoteOwner,
        key: impl Into<String>,
        value: T,
        gate: &dyn RemoteOwnerGate,
    ) -> Result<(), SearchError> {
        self.gate(owner, gate).await?;
        let mut inner = self.lock()?;
        self.live(&mut inner, owner)?;
        let key = key.into();
        if !inner.entries.contains_key(&key) && inner.entries.len() >= self.max_entries {
            return Err(lease_unavailable());
        }
        inner.entries.insert(key, value);
        inner.last_used = self.clock.now();
        Ok(())
    }

    /// An owned copy, checked again after the gate's await.
    pub async fn read(
        &self,
        owner: &RemoteOwner,
        key: &str,
        gate: &dyn RemoteOwnerGate,
    ) -> Result<Option<T>, SearchError> {
        self.gate(owner, gate).await?;
        let mut inner = self.lock()?;
        self.live(&mut inner, owner)?;
        let value = inner.entries.get(key).cloned();
        inner.last_used = self.clock.now();
        Ok(value)
    }

    /// Normal end of the owner's use: every entry goes.
    pub fn close(&self) {
        if let Ok(mut inner) = self.inner.lock()
            && matches!(inner.state, LeaseState::Building | LeaseState::Open)
        {
            inner.state = LeaseState::Closing;
            inner.entries.clear();
            inner.state = LeaseState::Closed;
        }
    }

    /// Revocation, error or cancel: every derived entry goes together.
    pub fn revoke(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.entries.clear();
            if !matches!(inner.state, LeaseState::Closed | LeaseState::Expired) {
                inner.state = LeaseState::Revoked;
            }
        }
    }

    pub fn invalidate_owner(&self, owner: &RemoteOwner) {
        if *owner == self.owner {
            self.revoke();
        }
    }
}
