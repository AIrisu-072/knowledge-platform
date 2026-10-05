//! Server-issued Search identity and Source visibility boundaries.
//!
//! The synthetic adapters are for an in-process, trusted harness. A production
//! composition root must connect an authenticated identity resolver and a
//! current Source visibility evaluator to the same ports.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use search_core::id::{DiscoveryEvaluationId, SessionId, SourceId};
use search_core::source::DiscoverableSource;
use uuid::Uuid;

use crate::SearchError;
use crate::ports::{AccessDecision, BoxFuture};
use crate::source_registration::{
    RegistrationActivation, SourceRegistration, SourceRegistrationCatalog,
};

fn unavailable() -> SearchError {
    SearchError::InvalidRequest("trusted scope unavailable".into())
}

fn nonempty_identity(value: &str) -> Result<(), SearchError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(SearchError::InvalidRequest(
            "invalid server identity".into(),
        ));
    }
    Ok(())
}

/// Tenant identity is Search-local; it is not a Document Domain principal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TenantId(String);

impl TenantId {
    pub fn new(value: impl Into<String>) -> Result<Self, SearchError> {
        let value = value.into();
        nonempty_identity(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrincipalRef(String);

impl PrincipalRef {
    pub fn new(value: impl Into<String>) -> Result<Self, SearchError> {
        let value = value.into();
        nonempty_identity(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// This handle is issued by the in-process authority; callers cannot build one.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccessContextHandle(String);

impl fmt::Debug for AccessContextHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccessContextHandle(<opaque>)")
    }
}

impl AccessContextHandle {
    /// Opaque compatibility value for the existing `DiscoveryRequest.access_context`.
    pub fn to_opaque_string(&self) -> String {
        self.0.clone()
    }
}

macro_rules! revision {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            pub fn new(value: u64) -> Result<Self, SearchError> {
                if value == 0 {
                    return Err(SearchError::InvalidRequest(
                        "revision must be positive".into(),
                    ));
                }
                Ok(Self(value))
            }

            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

revision!(AccessRevision);
revision!(RegistrationRevision);
revision!(VisibilityRevision);

/// Only an authority adapter inside this crate can mint the actor scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedSearchScope {
    tenant: TenantId,
    principal: PrincipalRef,
    session: Option<SessionId>,
    access_handle: AccessContextHandle,
    access_revision: AccessRevision,
    issued_at: Instant,
    deadline: Instant,
}

impl TrustedSearchScope {
    pub fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    pub fn principal(&self) -> &PrincipalRef {
        &self.principal
    }

    pub const fn session(&self) -> Option<SessionId> {
        self.session
    }

    pub fn access_handle(&self) -> &AccessContextHandle {
        &self.access_handle
    }

    pub const fn access_revision(&self) -> AccessRevision {
        self.access_revision
    }

    pub const fn issued_at(&self) -> Instant {
        self.issued_at
    }

    pub const fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn is_live(&self) -> bool {
        Instant::now() < self.deadline
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedDiscoveryBinding {
    actor: TrustedSearchScope,
    evaluation: DiscoveryEvaluationId,
}

impl TrustedDiscoveryBinding {
    pub fn actor(&self) -> &TrustedSearchScope {
        &self.actor
    }

    pub const fn evaluation(&self) -> DiscoveryEvaluationId {
        self.evaluation
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedSourceScope {
    actor: TrustedSearchScope,
    source: SourceId,
    registration_revision: RegistrationRevision,
    visibility_revision: VisibilityRevision,
    registration_activation: RegistrationActivation,
}

impl AuthorizedSourceScope {
    pub fn actor(&self) -> &TrustedSearchScope {
        &self.actor
    }

    pub const fn source_id(&self) -> SourceId {
        self.source
    }

    pub const fn registration_revision(&self) -> RegistrationRevision {
        self.registration_revision
    }

    pub const fn visibility_revision(&self) -> VisibilityRevision {
        self.visibility_revision
    }

    pub const fn registration_activation(&self) -> RegistrationActivation {
        self.registration_activation
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleSourceRegistration {
    scope: AuthorizedSourceScope,
    registration: SourceRegistration,
}

impl VisibleSourceRegistration {
    pub(crate) fn new(
        scope: AuthorizedSourceScope,
        registration: SourceRegistration,
    ) -> Result<Self, SearchError> {
        let descriptor = registration.authority_descriptor();
        if scope.actor.tenant != *descriptor.tenant()
            || scope.source != descriptor.source_id()
            || scope.registration_revision != descriptor.registration_revision()
            || scope.visibility_revision != descriptor.visibility_revision()
        {
            return Err(unavailable());
        }
        Ok(Self {
            scope,
            registration,
        })
    }

    pub fn scope(&self) -> &AuthorizedSourceScope {
        &self.scope
    }

    pub fn registration(&self) -> &SourceRegistration {
        &self.registration
    }

    pub fn discoverable_source(&self) -> DiscoverableSource {
        self.registration.discoverable_source()
    }
}

/// Present only when the registry has an authoritative stable visibility
/// epoch/digest. No stamp means no Search or SourcePage cursor authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleSetStamp([u8; 32]);

impl VisibleSetStamp {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Digest of one actor's complete visible set: every Source with its
    /// registration and visibility revisions and its ledger activation. Only
    /// a registry with an authoritative stable visibility epoch may attach
    /// it to a snapshot; the union catalog view does not.
    pub fn of(actor: &TrustedSearchScope, entries: &[VisibleSourceRegistration]) -> Self {
        use sha2::{Digest, Sha256};
        let mut parts: Vec<[u8; 40]> = entries
            .iter()
            .map(|entry| {
                let mut part = [0u8; 40];
                part[..16].copy_from_slice(entry.scope.source.as_uuid().as_bytes());
                part[16..24]
                    .copy_from_slice(&entry.scope.registration_revision.get().to_be_bytes());
                part[24..32].copy_from_slice(&entry.scope.visibility_revision.get().to_be_bytes());
                part[32..40]
                    .copy_from_slice(&entry.scope.registration_activation.get().to_be_bytes());
                part
            })
            .collect();
        parts.sort();
        let mut hasher = Sha256::new();
        hasher.update(b"search-visible-set:v1");
        for field in [actor.tenant.as_str(), actor.principal.as_str()] {
            hasher.update((field.len() as u64).to_be_bytes());
            hasher.update(field.as_bytes());
        }
        hasher.update(actor.access_revision.get().to_be_bytes());
        for part in parts {
            hasher.update(part);
        }
        Self(hasher.finalize().into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleCatalogSnapshot {
    entries: Vec<VisibleSourceRegistration>,
    continuation_stamp: Option<VisibleSetStamp>,
}

impl VisibleCatalogSnapshot {
    /// A trusted registry may use this only after complete enumeration and
    /// current checks; no cursor can be issued from this result.
    pub fn unstamped(entries: Vec<VisibleSourceRegistration>) -> Self {
        Self {
            entries,
            continuation_stamp: None,
        }
    }

    /// A complete enumeration with an authoritative visible-set stamp, the
    /// only snapshot a continuation cursor can be bound to.
    pub fn stamped(entries: Vec<VisibleSourceRegistration>, stamp: VisibleSetStamp) -> Self {
        Self {
            entries,
            continuation_stamp: Some(stamp),
        }
    }

    pub fn entries(&self) -> &[VisibleSourceRegistration] {
        &self.entries
    }

    pub fn continuation_stamp(&self) -> Option<&VisibleSetStamp> {
        self.continuation_stamp.as_ref()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::ops::Index<usize> for VisibleCatalogSnapshot {
    type Output = VisibleSourceRegistration;

    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessBindingState {
    Current,
    Denied,
    Unknown,
}

pub trait AccessContextAuthorityPort: Send + Sync {
    fn resolve<'a>(
        &'a self,
        handle: &'a AccessContextHandle,
    ) -> BoxFuture<'a, Option<TrustedSearchScope>>;

    fn bind_discovery<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        evaluation: DiscoveryEvaluationId,
    ) -> BoxFuture<'a, Option<TrustedDiscoveryBinding>>;

    fn current<'a>(&'a self, actor: &'a TrustedSearchScope) -> BoxFuture<'a, AccessBindingState>;
}

/// A Source gate must check both the current visibility grant and the current
/// registration/activation from the host ledger on every bind and current read.
pub trait CurrentSourceVisibilityPort: Send + Sync {
    fn bind_source<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<AuthorizedSourceScope>>;

    fn current<'a>(&'a self, scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessDecision>;
}

/// Only this actor-scoped port supplies routing-visible registrations.
pub trait ScopedSourceRegistryPort: Send + Sync {
    fn visible_sources<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
    ) -> BoxFuture<'a, VisibleCatalogSnapshot>;
}

/// A descriptor returned only by a host-configured, authenticated resolver.
/// It is data for the checked adapter; callers cannot pass it to a scope mint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedActorDescriptor {
    tenant: TenantId,
    principal: PrincipalRef,
    session: Option<SessionId>,
    access_revision: AccessRevision,
    issued_at: Instant,
    deadline: Instant,
}

impl VerifiedActorDescriptor {
    pub fn new(
        tenant: TenantId,
        principal: PrincipalRef,
        session: Option<SessionId>,
        access_revision: AccessRevision,
        issued_at: Instant,
        deadline: Instant,
    ) -> Result<Self, SearchError> {
        if issued_at > Instant::now() || deadline <= issued_at {
            return Err(unavailable());
        }
        Ok(Self {
            tenant,
            principal,
            session,
            access_revision,
            issued_at,
            deadline,
        })
    }
}

/// Host trust boundary: verify the raw opaque handle against the configured
/// identity provider, including actor, session, revision and expiry. The HTTP
/// request cannot choose this port or supply a descriptor directly.
pub trait VerifiedActorResolverPort: Send + Sync {
    fn resolve_verified<'a>(
        &'a self,
        raw_handle: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedActorDescriptor>>;
}

/// The only general issuer for production wiring; it mints after the trusted
/// resolver has verified the raw handle on every resolve/current check.
pub struct CheckedAuthorityAdapter<'a> {
    resolver: &'a dyn VerifiedActorResolverPort,
}

impl<'a> CheckedAuthorityAdapter<'a> {
    pub fn new(resolver: &'a dyn VerifiedActorResolverPort) -> Self {
        Self { resolver }
    }

    pub async fn authenticate_handle(
        &self,
        raw_handle: &str,
    ) -> Result<Option<TrustedSearchScope>, SearchError> {
        if raw_handle.is_empty()
            || raw_handle.len() > 512
            || !raw_handle.is_ascii()
            || raw_handle
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Ok(None);
        }
        let descriptor = self.resolver.resolve_verified(raw_handle).await?;
        Ok(descriptor
            .filter(|descriptor| {
                descriptor.issued_at <= Instant::now() && Instant::now() < descriptor.deadline
            })
            .map(|descriptor| TrustedSearchScope {
                tenant: descriptor.tenant,
                principal: descriptor.principal,
                session: descriptor.session,
                access_handle: AccessContextHandle(raw_handle.into()),
                access_revision: descriptor.access_revision,
                issued_at: descriptor.issued_at,
                deadline: descriptor.deadline,
            }))
    }
}

impl AccessContextAuthorityPort for CheckedAuthorityAdapter<'_> {
    fn resolve<'a>(
        &'a self,
        handle: &'a AccessContextHandle,
    ) -> BoxFuture<'a, Option<TrustedSearchScope>> {
        Box::pin(async move { self.authenticate_handle(&handle.0).await })
    }

    fn bind_discovery<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        evaluation: DiscoveryEvaluationId,
    ) -> BoxFuture<'a, Option<TrustedDiscoveryBinding>> {
        Box::pin(async move {
            if evaluation.as_uuid().is_nil()
                || self.current(actor).await? != AccessBindingState::Current
            {
                return Ok(None);
            }
            Ok(Some(TrustedDiscoveryBinding {
                actor: actor.clone(),
                evaluation,
            }))
        })
    }

    fn current<'a>(&'a self, actor: &'a TrustedSearchScope) -> BoxFuture<'a, AccessBindingState> {
        Box::pin(async move {
            Ok(
                if self
                    .authenticate_handle(&actor.access_handle.0)
                    .await?
                    .as_ref()
                    == Some(actor)
                {
                    AccessBindingState::Current
                } else {
                    AccessBindingState::Denied
                },
            )
        })
    }
}

/// A current grant from the host-configured visibility policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSourceGrant {
    tenant: TenantId,
    source: SourceId,
    registration_revision: RegistrationRevision,
    visibility_revision: VisibilityRevision,
}

impl VerifiedSourceGrant {
    pub fn new(
        tenant: TenantId,
        source: SourceId,
        registration_revision: RegistrationRevision,
        visibility_revision: VisibilityRevision,
    ) -> Self {
        Self {
            tenant,
            source,
            registration_revision,
            visibility_revision,
        }
    }
}

/// Host trust boundary: evaluate present Source visibility for this actor.
pub trait VerifiedSourceVisibilityPort: Send + Sync {
    fn grant_for<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<VerifiedSourceGrant>>;
}

pub struct CheckedSourceVisibilityAdapter<'a> {
    verifier: &'a dyn VerifiedSourceVisibilityPort,
    catalog: &'a SourceRegistrationCatalog,
}

impl<'a> CheckedSourceVisibilityAdapter<'a> {
    pub fn new(
        verifier: &'a dyn VerifiedSourceVisibilityPort,
        catalog: &'a SourceRegistrationCatalog,
    ) -> Self {
        Self { verifier, catalog }
    }
}

impl CurrentSourceVisibilityPort for CheckedSourceVisibilityAdapter<'_> {
    fn bind_source<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<AuthorizedSourceScope>> {
        Box::pin(async move {
            if !actor.is_live() {
                return Ok(None);
            }
            let grant = self.verifier.grant_for(actor, source).await?;
            let Some(grant) =
                grant.filter(|grant| grant.tenant == actor.tenant && grant.source == source)
            else {
                return Ok(None);
            };
            let Some(registration_activation) = self
                .catalog
                .current_activation(
                    &actor.tenant,
                    source,
                    grant.registration_revision,
                    grant.visibility_revision,
                )
                .await?
            else {
                return Ok(None);
            };
            Ok(Some(AuthorizedSourceScope {
                actor: actor.clone(),
                source,
                registration_revision: grant.registration_revision,
                visibility_revision: grant.visibility_revision,
                registration_activation,
            }))
        })
    }

    fn current<'a>(&'a self, scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if !scope.actor.is_live() {
                return Ok(AccessDecision::Denied);
            }
            let grant = self.verifier.grant_for(&scope.actor, scope.source).await?;
            let registration_current = self
                .catalog
                .current_activation(
                    &scope.actor.tenant,
                    scope.source,
                    scope.registration_revision,
                    scope.visibility_revision,
                )
                .await?
                == Some(scope.registration_activation);
            Ok(match grant {
                Some(grant)
                    if grant.tenant == scope.actor.tenant
                        && grant.source == scope.source
                        && grant.registration_revision == scope.registration_revision
                        && grant.visibility_revision == scope.visibility_revision
                        && registration_current =>
                {
                    AccessDecision::Allowed
                }
                _ => AccessDecision::Denied,
            })
        })
    }
}

pub async fn check_actor_current(
    authority: &dyn AccessContextAuthorityPort,
    actor: &TrustedSearchScope,
) -> Result<(), SearchError> {
    if !actor.is_live() {
        return Err(unavailable());
    }
    let resolved = authority.resolve(actor.access_handle()).await?;
    if resolved.as_ref() != Some(actor)
        || authority.current(actor).await? != AccessBindingState::Current
    {
        return Err(unavailable());
    }
    Ok(())
}

/// Verify the complete actor/evaluation/request binding before any Source port.
pub async fn verify_discovery_binding(
    authority: &dyn AccessContextAuthorityPort,
    binding: &TrustedDiscoveryBinding,
    request_access_context: &str,
    requested_evaluation: DiscoveryEvaluationId,
) -> Result<(), SearchError> {
    if binding.evaluation != requested_evaluation
        || binding.actor.access_handle.to_opaque_string() != request_access_context
    {
        return Err(unavailable());
    }
    check_actor_current(authority, &binding.actor).await?;
    let rebound = authority
        .bind_discovery(&binding.actor, requested_evaluation)
        .await?;
    if rebound.as_ref() != Some(binding) {
        return Err(unavailable());
    }
    Ok(())
}

/// Entry helper used before a registry read. It also rejects malformed output
/// from a registry adapter instead of routing one arbitrary duplicate winner.
pub async fn prepare_actor_visible_sources(
    authority: &dyn AccessContextAuthorityPort,
    registry: &dyn ScopedSourceRegistryPort,
    actor: &TrustedSearchScope,
) -> Result<VisibleCatalogSnapshot, SearchError> {
    check_actor_current(authority, actor).await?;
    let visible = registry.visible_sources(actor).await?;
    let mut ids = std::collections::BTreeSet::new();
    for entry in visible.entries() {
        let descriptor = entry.registration.authority_descriptor();
        if entry.scope.actor != *actor
            || entry.scope.actor.tenant != *descriptor.tenant()
            || entry.scope.source != descriptor.source_id()
            || entry.scope.registration_revision != descriptor.registration_revision()
            || entry.scope.visibility_revision != descriptor.visibility_revision()
            || !ids.insert(entry.scope.source)
        {
            return Err(unavailable());
        }
    }
    check_actor_current(authority, actor).await?;
    Ok(visible)
}

/// Discovery-specific evaluation binding over the shared four-route actor
/// and catalog gate. Other routes use `prepare_actor_visible_sources` directly.
pub async fn prepare_visible_sources(
    authority: &dyn AccessContextAuthorityPort,
    registry: &dyn ScopedSourceRegistryPort,
    binding: &TrustedDiscoveryBinding,
    request_access_context: &str,
    requested_evaluation: DiscoveryEvaluationId,
) -> Result<VisibleCatalogSnapshot, SearchError> {
    verify_discovery_binding(
        authority,
        binding,
        request_access_context,
        requested_evaluation,
    )
    .await?;
    prepare_actor_visible_sources(authority, registry, &binding.actor).await
}

#[derive(Debug, Clone)]
struct SyntheticActorRecord {
    tenant: TenantId,
    principal: PrincipalRef,
    session: Option<SessionId>,
    access_revision: AccessRevision,
    issued_at: Instant,
    deadline: Instant,
    active: bool,
}

/// Synthetic trusted authority for the in-process harness. A real identity
/// resolver must authenticate before it writes an actor record.
#[derive(Debug, Default)]
pub struct SyntheticAuthorityAdapter {
    records: RwLock<BTreeMap<AccessContextHandle, SyntheticActorRecord>>,
}

impl SyntheticAuthorityAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Trusted harness setup, never a request/provider DTO conversion.
    pub fn issue_verified_identity(
        &self,
        tenant: TenantId,
        principal: PrincipalRef,
        session: Option<SessionId>,
        access_revision: AccessRevision,
        lifetime: Duration,
    ) -> Result<AccessContextHandle, SearchError> {
        let issued_at = Instant::now();
        let deadline = issued_at.checked_add(lifetime).ok_or_else(unavailable)?;
        if deadline <= issued_at {
            return Err(unavailable());
        }
        let mut records = self
            .records
            .write()
            .map_err(|_| SearchError::OperationFailed("authority unavailable".into()))?;
        let handle = AccessContextHandle(Uuid::now_v7().to_string());
        if records.contains_key(&handle) {
            return Err(SearchError::OperationFailed("handle collision".into()));
        }
        records.insert(
            handle.clone(),
            SyntheticActorRecord {
                tenant,
                principal,
                session,
                access_revision,
                issued_at,
                deadline,
                active: true,
            },
        );
        Ok(handle)
    }

    pub fn replace_access_revision(
        &self,
        handle: &AccessContextHandle,
        next: AccessRevision,
    ) -> Result<(), SearchError> {
        let mut records = self
            .records
            .write()
            .map_err(|_| SearchError::OperationFailed("authority unavailable".into()))?;
        let record = records.get_mut(handle).ok_or_else(unavailable)?;
        if next <= record.access_revision {
            return Err(unavailable());
        }
        record.access_revision = next;
        Ok(())
    }

    pub fn revoke(&self, handle: &AccessContextHandle) -> Result<(), SearchError> {
        let mut records = self
            .records
            .write()
            .map_err(|_| SearchError::OperationFailed("authority unavailable".into()))?;
        records.get_mut(handle).ok_or_else(unavailable)?.active = false;
        Ok(())
    }
}

impl AccessContextAuthorityPort for SyntheticAuthorityAdapter {
    fn resolve<'a>(
        &'a self,
        handle: &'a AccessContextHandle,
    ) -> BoxFuture<'a, Option<TrustedSearchScope>> {
        Box::pin(async move {
            let records = self
                .records
                .read()
                .map_err(|_| SearchError::OperationFailed("authority unavailable".into()))?;
            Ok(records
                .get(handle)
                .filter(|record| record.active && Instant::now() < record.deadline)
                .map(|record| TrustedSearchScope {
                    tenant: record.tenant.clone(),
                    principal: record.principal.clone(),
                    session: record.session,
                    access_handle: handle.clone(),
                    access_revision: record.access_revision,
                    issued_at: record.issued_at,
                    deadline: record.deadline,
                }))
        })
    }

    fn bind_discovery<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        evaluation: DiscoveryEvaluationId,
    ) -> BoxFuture<'a, Option<TrustedDiscoveryBinding>> {
        Box::pin(async move {
            if evaluation.as_uuid().is_nil()
                || self.current(actor).await? != AccessBindingState::Current
            {
                return Ok(None);
            }
            Ok(Some(TrustedDiscoveryBinding {
                actor: actor.clone(),
                evaluation,
            }))
        })
    }

    fn current<'a>(&'a self, actor: &'a TrustedSearchScope) -> BoxFuture<'a, AccessBindingState> {
        Box::pin(async move {
            Ok(
                if self.resolve(&actor.access_handle).await?.as_ref() == Some(actor) {
                    AccessBindingState::Current
                } else {
                    AccessBindingState::Denied
                },
            )
        })
    }
}

#[derive(Debug, Clone)]
struct SyntheticSourceGrant {
    tenant: TenantId,
    registration_revision: RegistrationRevision,
    visibility_revision: VisibilityRevision,
    active: bool,
}

/// Mutable synthetic visibility policy; the catalog still validates tenant and revisions.
#[derive(Debug)]
pub struct SyntheticVisibilityAdapter<'a> {
    catalog: &'a SourceRegistrationCatalog,
    grants: RwLock<BTreeMap<(SourceId, PrincipalRef), SyntheticSourceGrant>>,
}

impl<'a> SyntheticVisibilityAdapter<'a> {
    pub fn new(catalog: &'a SourceRegistrationCatalog) -> Self {
        Self {
            catalog,
            grants: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn grant(
        &self,
        tenant: TenantId,
        principal: PrincipalRef,
        source: SourceId,
        registration_revision: RegistrationRevision,
        visibility_revision: VisibilityRevision,
    ) -> Result<(), SearchError> {
        let mut grants = self
            .grants
            .write()
            .map_err(|_| SearchError::OperationFailed("visibility unavailable".into()))?;
        let key = (source, principal);
        if let Some(previous) = grants.get(&key)
            && (previous.tenant != tenant
                || registration_revision < previous.registration_revision
                || visibility_revision < previous.visibility_revision
                || (!previous.active && visibility_revision <= previous.visibility_revision))
        {
            return Err(unavailable());
        }
        grants.insert(
            key,
            SyntheticSourceGrant {
                tenant,
                registration_revision,
                visibility_revision,
                active: true,
            },
        );
        Ok(())
    }

    pub fn revoke(&self, principal: &PrincipalRef, source: SourceId) -> Result<(), SearchError> {
        let mut grants = self
            .grants
            .write()
            .map_err(|_| SearchError::OperationFailed("visibility unavailable".into()))?;
        grants
            .get_mut(&(source, principal.clone()))
            .ok_or_else(unavailable)?
            .active = false;
        Ok(())
    }
}

impl CurrentSourceVisibilityPort for SyntheticVisibilityAdapter<'_> {
    fn bind_source<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<AuthorizedSourceScope>> {
        Box::pin(async move {
            if !actor.is_live() {
                return Ok(None);
            }
            let grant = {
                let grants = self
                    .grants
                    .read()
                    .map_err(|_| SearchError::OperationFailed("visibility unavailable".into()))?;
                grants.get(&(source, actor.principal.clone())).cloned()
            };
            let Some(grant) = grant.filter(|grant| grant.active && grant.tenant == actor.tenant)
            else {
                return Ok(None);
            };
            let Some(registration_activation) = self
                .catalog
                .current_activation(
                    &actor.tenant,
                    source,
                    grant.registration_revision,
                    grant.visibility_revision,
                )
                .await?
            else {
                return Ok(None);
            };
            Ok(Some(AuthorizedSourceScope {
                actor: actor.clone(),
                source,
                registration_revision: grant.registration_revision,
                visibility_revision: grant.visibility_revision,
                registration_activation,
            }))
        })
    }

    fn current<'a>(&'a self, scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if !scope.actor.is_live() {
                return Ok(AccessDecision::Denied);
            }
            let grant = {
                let grants = self
                    .grants
                    .read()
                    .map_err(|_| SearchError::OperationFailed("visibility unavailable".into()))?;
                grants
                    .get(&(scope.source, scope.actor.principal.clone()))
                    .cloned()
            };
            let registration_current = self
                .catalog
                .current_activation(
                    &scope.actor.tenant,
                    scope.source,
                    scope.registration_revision,
                    scope.visibility_revision,
                )
                .await?
                == Some(scope.registration_activation);
            Ok(match grant {
                Some(grant)
                    if grant.active
                        && grant.tenant == scope.actor.tenant
                        && grant.registration_revision == scope.registration_revision
                        && grant.visibility_revision == scope.visibility_revision
                        && registration_current =>
                {
                    AccessDecision::Allowed
                }
                _ => AccessDecision::Denied,
            })
        })
    }
}
