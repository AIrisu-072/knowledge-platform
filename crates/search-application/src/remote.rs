//! Provider-neutral, Source-scoped remote operation contracts.
//!
//! This slice carries identity hints and observations only. Sealed projections,
//! lease-owned content, transport and disclosure are separate integration steps.

use search_core::materialization::MaterializationState;
use search_core::observation::{Coverage, Presence};
use search_core::predicate::TypedValue;
use search_core::resource::ResourceKind;
use search_core::source::DiscoveryMode;
use std::collections::BTreeMap;
use std::fmt;
use uuid::Uuid;

use crate::SearchError;
use crate::ports::{AccessDecision, BoxFuture, CurrentSourcePolicy};
use crate::remote_observation::SourceSnapshotProof;
use crate::remote_registration::RemoteSourceRegistration;
use crate::retrieval::{LiveInput, OpaqueNativeId, RemoteQueryInput};
use crate::scoped::{
    AccessContextAuthorityPort, AuthorizedSourceScope, CurrentSourceVisibilityPort,
    TrustedDiscoveryBinding, VisibleSourceRegistration, verify_discovery_binding,
};
use crate::source_registration::SourceRegistration;

pub(crate) fn invalid_remote() -> SearchError {
    SearchError::InvalidRequest("remote contract unavailable".into())
}

/// Combines existing authority-issued bindings with a catalog-authenticated
/// visible registration; raw server configuration cannot substitute its contents.
///
/// ```compile_fail
/// use search_application::remote::TrustedRemoteContext;
/// use search_application::remote_registration::RemoteSourceRegistration;
/// use search_application::scoped::{TrustedDiscoveryBinding, AuthorizedSourceScope};
/// fn forge(binding: TrustedDiscoveryBinding, source_scope: AuthorizedSourceScope,
///          registration: RemoteSourceRegistration) -> TrustedRemoteContext {
///     TrustedRemoteContext { binding, source_scope, registration }
/// }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct TrustedRemoteContext {
    binding: TrustedDiscoveryBinding,
    source_scope: AuthorizedSourceScope,
    registration: RemoteSourceRegistration,
}

impl fmt::Debug for TrustedRemoteContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TrustedRemoteContext(<opaque>)")
    }
}

impl TrustedRemoteContext {
    pub async fn bind(
        binding: TrustedDiscoveryBinding,
        visible: &VisibleSourceRegistration,
        authority: &dyn AccessContextAuthorityPort,
        visibility: &dyn CurrentSourceVisibilityPort,
    ) -> Result<Self, SearchError> {
        let SourceRegistration::Remote(registration) = visible.registration() else {
            return Err(invalid_remote());
        };
        let context = Self {
            binding,
            source_scope: visible.scope().clone(),
            registration: registration.clone(),
        };
        context
            .check_current(registration, authority, visibility)
            .await?;
        Ok(context)
    }

    pub fn binding(&self) -> &TrustedDiscoveryBinding {
        &self.binding
    }
    pub fn source_scope(&self) -> &AuthorizedSourceScope {
        &self.source_scope
    }
    pub fn registration(&self) -> &RemoteSourceRegistration {
        &self.registration
    }

    pub(crate) fn matches_registration(&self, registration: &RemoteSourceRegistration) -> bool {
        self.binding.actor().is_live()
            && self.binding.actor() == self.source_scope.actor()
            && self.binding.actor().tenant() == registration.tenant()
            && self.source_scope.source_id() == registration.source_id()
            && self.source_scope.registration_revision() == registration.registration_revision()
            && self.source_scope.visibility_revision() == registration.visibility_revision()
            && &self.registration == registration
    }

    pub(crate) async fn check_current(
        &self,
        registration: &RemoteSourceRegistration,
        authority: &dyn AccessContextAuthorityPort,
        visibility: &dyn CurrentSourceVisibilityPort,
    ) -> Result<(), SearchError> {
        if !self.matches_registration(registration) {
            return Err(invalid_remote());
        }
        verify_discovery_binding(
            authority,
            &self.binding,
            &self.binding.actor().access_handle().to_opaque_string(),
            self.binding.evaluation(),
        )
        .await?;
        if visibility.current(&self.source_scope).await? != AccessDecision::Allowed {
            return Err(invalid_remote());
        }
        // A visibility await may overlap revocation of the actor.
        verify_discovery_binding(
            authority,
            &self.binding,
            &self.binding.actor().access_handle().to_opaque_string(),
            self.binding.evaluation(),
        )
        .await?;
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct OpaqueCursor(String);
impl fmt::Debug for OpaqueCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpaqueCursor(<opaque>)")
    }
}
impl OpaqueCursor {
    pub fn new(value: impl Into<String>) -> Result<Self, SearchError> {
        let value = value.into();
        if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
            return Err(invalid_remote());
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOperationKind {
    Enumerate,
    Query,
    Lookup,
    Live,
}
impl RemoteOperationKind {
    pub const fn discovery_mode(self) -> DiscoveryMode {
        match self {
            Self::Enumerate => DiscoveryMode::RemoteEnumeration,
            Self::Query => DiscoveryMode::RemoteQuery,
            Self::Lookup => DiscoveryMode::DirectAddress,
            Self::Live => DiscoveryMode::LiveOnly,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteOperation {
    Enumerate { cursor: Option<OpaqueCursor> },
    Query { input: RemoteQueryInput },
    Lookup { native_id: OpaqueNativeId },
    Live { input: LiveInput },
}
impl RemoteOperation {
    pub const fn kind(&self) -> RemoteOperationKind {
        match self {
            Self::Enumerate { .. } => RemoteOperationKind::Enumerate,
            Self::Query { .. } => RemoteOperationKind::Query,
            Self::Lookup { .. } => RemoteOperationKind::Lookup,
            Self::Live { .. } => RemoteOperationKind::Live,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRemoteAction {
    context: TrustedRemoteContext,
    retriever_id: String,
    operation: RemoteOperation,
}
impl PlannedRemoteAction {
    pub fn new(
        context: &TrustedRemoteContext,
        retriever_id: impl Into<String>,
        operation: RemoteOperation,
    ) -> Result<Self, SearchError> {
        let retriever_id = retriever_id.into();
        if retriever_id.is_empty()
            || retriever_id.len() > 128
            || !retriever_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            || !context.matches_registration(&context.registration)
            || !context
                .registration
                .supported_modes()
                .contains(&operation.kind().discovery_mode())
        {
            return Err(invalid_remote());
        }
        let limits = context.registration.limits();
        let (query, native) = match &operation {
            RemoteOperation::Enumerate { cursor } => {
                if cursor
                    .as_ref()
                    .is_some_and(|v| v.as_str().len() > limits.max_cursor_bytes)
                {
                    return Err(invalid_remote());
                }
                (None, None)
            }
            RemoteOperation::Query { input } => (Some(input), None),
            RemoteOperation::Lookup { native_id } => (None, Some(native_id)),
            RemoteOperation::Live { input } => (input.query_input(), input.native_id()),
        };
        if query.is_some_and(|v| v.window() > limits.max_hits_per_page)
            || native.is_some_and(|v| v.as_str().len() > limits.max_native_id_bytes)
        {
            return Err(invalid_remote());
        }
        Ok(Self {
            context: context.clone(),
            retriever_id,
            operation,
        })
    }
    pub fn retriever_id(&self) -> &str {
        &self.retriever_id
    }
    pub fn operation(&self) -> &RemoteOperation {
        &self.operation
    }
    pub(crate) fn matches_context(&self, context: &TrustedRemoteContext) -> bool {
        &self.context == context
    }
}

/// The adapter checks this before any transport call. Result batch snapshot
/// compatibility is checked separately, before a generation can be sealed.
pub fn validate_remote_batch(
    context: &TrustedRemoteContext,
    actions: &[PlannedRemoteAction],
) -> Result<(), SearchError> {
    let mut ids = std::collections::BTreeSet::new();
    if actions.is_empty()
        || actions.len() > context.registration.limits().max_actions
        || !context.matches_registration(&context.registration)
        || actions
            .iter()
            .any(|action| !action.matches_context(context) || !ids.insert(action.retriever_id()))
    {
        return Err(invalid_remote());
    }
    Ok(())
}

/// One provider field value and the provider's own provenance label. Both are
/// untrusted: the label is never evidence and never selects authority.
#[derive(Clone, PartialEq, Eq)]
pub struct UntrustedFieldValue {
    pub(crate) value: TypedValue,
    pub(crate) provenance: Option<String>,
}
impl fmt::Debug for UntrustedFieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UntrustedFieldValue(<untrusted>)")
    }
}

/// Provider identity hints, deliberately without SourceId, ResourceId,
/// candidate ID, evidence role, upstream authority, or a fetch destination.
#[derive(Clone, PartialEq, Eq)]
pub struct UntrustedRemoteHit {
    native_id: Option<OpaqueNativeId>,
    version: Option<String>,
    digest: Option<String>,
    kind: Option<ResourceKind>,
    title: Option<String>,
    fields: BTreeMap<String, UntrustedFieldValue>,
}
impl fmt::Debug for UntrustedRemoteHit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UntrustedRemoteHit(<untrusted>)")
    }
}
impl UntrustedRemoteHit {
    pub fn new(
        native_id: Option<OpaqueNativeId>,
        version: Option<String>,
        digest: Option<String>,
    ) -> Result<Self, SearchError> {
        for value in [&version, &digest].into_iter().flatten() {
            if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
                return Err(invalid_remote());
            }
        }
        Ok(Self {
            native_id,
            version,
            digest,
            kind: None,
            title: None,
            fields: BTreeMap::new(),
        })
    }

    /// Bounded, untrusted projection fields. Names are lowercase field keys;
    /// a duplicate name is malformed rather than silently overwritten.
    pub fn with_projection(
        mut self,
        kind: Option<ResourceKind>,
        title: Option<String>,
        fields: Vec<(String, TypedValue, Option<String>)>,
    ) -> Result<Self, SearchError> {
        let bounded = |value: &str, max: usize| {
            !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
        };
        if title.as_deref().is_some_and(|title| !bounded(title, 1024)) || fields.len() > 64 {
            return Err(invalid_remote());
        }
        let mut map = BTreeMap::new();
        for (name, value, provenance) in fields {
            if !bounded(&name, 128)
                || !name.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.')
                })
                || provenance
                    .as_deref()
                    .is_some_and(|label| !bounded(label, 256))
                || map
                    .insert(name, UntrustedFieldValue { value, provenance })
                    .is_some()
            {
                return Err(invalid_remote());
            }
        }
        self.kind = kind;
        self.title = title;
        self.fields = map;
        Ok(self)
    }

    pub fn native_id(&self) -> Option<&OpaqueNativeId> {
        self.native_id.as_ref()
    }
    pub const fn kind(&self) -> Option<ResourceKind> {
        self.kind
    }
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    pub(crate) fn fields(&self) -> &BTreeMap<String, UntrustedFieldValue> {
        &self.fields
    }
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemotePage {
    Unpaged,
    Enumeration {
        requested: Option<OpaqueCursor>,
        next: Option<OpaqueCursor>,
        terminal: bool,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteResponseStatus {
    Success,
    Partial,
    NotFound,
    Forbidden,
    Timeout,
    Unavailable,
    Malformed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteUnknownReason {
    Partial,
    NotFound,
    Denied,
    Timeout,
    Unavailable,
    Malformed,
    UnverifiedSnapshot,
    SnapshotIncompatible,
    Unsupported,
}

/// Untrusted provider data. Reported totals never determine coverage.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteResponseInput {
    pub(crate) status: RemoteResponseStatus,
    pub(crate) page: RemotePage,
    pub(crate) hits: Vec<UntrustedRemoteHit>,
    reported_total: Option<u64>,
}
impl fmt::Debug for RemoteResponseInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RemoteResponseInput(<untrusted>)")
    }
}
impl RemoteResponseInput {
    pub fn new(
        status: RemoteResponseStatus,
        page: RemotePage,
        hits: Vec<UntrustedRemoteHit>,
        reported_total: Option<u64>,
    ) -> Result<Self, SearchError> {
        if hits.len() > 100 {
            return Err(invalid_remote());
        }
        Ok(Self {
            status,
            page,
            hits,
            reported_total,
        })
    }
    pub const fn status(&self) -> RemoteResponseStatus {
        self.status
    }
    pub fn page(&self) -> &RemotePage {
        &self.page
    }
    pub fn hits(&self) -> &[UntrustedRemoteHit] {
        &self.hits
    }
    pub const fn reported_total(&self) -> Option<u64> {
        self.reported_total
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteActionResponse {
    pub(crate) action: PlannedRemoteAction,
    pub(crate) proof: SourceSnapshotProof,
    pub(crate) coverage: Coverage,
    pub(crate) page: RemotePage,
    pub(crate) hits: Vec<UntrustedRemoteHit>,
}
impl RemoteActionResponse {
    pub fn retriever_id(&self) -> &str {
        self.action.retriever_id()
    }
    pub const fn operation(&self) -> RemoteOperationKind {
        self.action.operation.kind()
    }
    pub fn source_snapshot_proof(&self) -> &SourceSnapshotProof {
        &self.proof
    }
    pub const fn coverage(&self) -> Coverage {
        self.coverage
    }
    pub fn hits(&self) -> &[UntrustedRemoteHit] {
        &self.hits
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteActionOutcome {
    Completed(Box<RemoteActionResponse>),
    Unknown {
        retriever_id: String,
        operation: RemoteOperationKind,
        reason: RemoteUnknownReason,
    },
}
impl RemoteActionOutcome {
    /// A miss has no absence authority. Only VerifiedAbsence supplies Absent.
    pub fn presence(&self, native_id: &OpaqueNativeId) -> Presence {
        match self {
            Self::Completed(response)
                if response
                    .hits
                    .iter()
                    .any(|hit| hit.native_id.as_ref() == Some(native_id)) =>
            {
                Presence::Present
            }
            _ => Presence::Unknown,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct EvaluationLeaseId(Uuid);
impl EvaluationLeaseId {
    /// This is only an owner identifier, not a live lease or an access grant.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}
impl Default for EvaluationLeaseId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Debug for EvaluationLeaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EvaluationLeaseId(<opaque>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RemoteIdentity {
    scope: AuthorizedSourceScope,
    native_id: OpaqueNativeId,
}
impl fmt::Debug for RemoteIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RemoteIdentity(<opaque>)")
    }
}
impl RemoteIdentity {
    pub fn new(
        context: &TrustedRemoteContext,
        native_id: OpaqueNativeId,
    ) -> Result<Self, SearchError> {
        if !context.matches_registration(&context.registration)
            || native_id.as_str().len() > context.registration.limits().max_native_id_bytes
        {
            return Err(invalid_remote());
        }
        Ok(Self {
            scope: context.source_scope.clone(),
            native_id,
        })
    }
    pub fn source_scope(&self) -> &AuthorizedSourceScope {
        &self.scope
    }
    pub fn native_id(&self) -> &OpaqueNativeId {
        &self.native_id
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteAccessTarget {
    SourceScope,
    Resource(RemoteIdentity),
}
#[derive(Clone, PartialEq, Eq)]
pub struct PinnedRemoteTarget {
    identity: RemoteIdentity,
    snapshot: SourceSnapshotProof,
    version: Option<String>,
    digest: Option<String>,
}
impl fmt::Debug for PinnedRemoteTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PinnedRemoteTarget(<opaque>)")
    }
}
impl PinnedRemoteTarget {
    /// Pins only an identity actually observed in this verified response. This
    /// is not qualification, a content permission, or a materialization receipt.
    pub fn from_response(
        context: &TrustedRemoteContext,
        response: &RemoteActionResponse,
        native_id: &OpaqueNativeId,
    ) -> Result<Self, SearchError> {
        if !response.action.matches_context(context) || !response.proof.matches_context(context) {
            return Err(invalid_remote());
        }
        let mut matches = response
            .hits
            .iter()
            .filter(|hit| hit.native_id.as_ref() == Some(native_id));
        let hit = matches.next().ok_or_else(invalid_remote)?;
        if matches.next().is_some() {
            return Err(invalid_remote());
        }
        if hit.version.is_none() && hit.digest.is_none() {
            return Err(invalid_remote());
        }
        Ok(Self {
            identity: RemoteIdentity::new(context, native_id.clone())?,
            snapshot: response.proof.clone(),
            version: hit.version.clone(),
            digest: hit.digest.clone(),
        })
    }
    /// A sealed batch's own staged identity, version and digest.
    pub(crate) fn from_parts(
        identity: RemoteIdentity,
        snapshot: SourceSnapshotProof,
        version: Option<String>,
        digest: Option<String>,
    ) -> Self {
        Self {
            identity,
            snapshot,
            version,
            digest,
        }
    }
    pub fn identity(&self) -> &RemoteIdentity {
        &self.identity
    }
    pub fn snapshot(&self) -> &SourceSnapshotProof {
        &self.snapshot
    }
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }
}

/// Observation metadata only. Payload ownership and materialization execution
/// remain unqualified until the lease/binding integration slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteReadOutcome {
    Observed {
        target: Box<PinnedRemoteTarget>,
        state: MaterializationState,
    },
    Unknown(RemoteUnknownReason),
}

/// Trusted, host-wired adapter boundary. Implementations must validate the
/// batch/current binding before and after awaits and must not mutate Resource
/// lifecycle state from failures. No provider is allowed to select this port.
pub trait RemoteSourcePort: Send + Sync {
    fn execute_batch<'a>(
        &'a self,
        context: &'a TrustedRemoteContext,
        actions: &'a [PlannedRemoteAction],
    ) -> BoxFuture<'a, Vec<RemoteActionOutcome>>;
    fn current_access<'a>(
        &'a self,
        context: &'a TrustedRemoteContext,
        target: &'a RemoteAccessTarget,
    ) -> BoxFuture<'a, AccessDecision>;
    fn current_policy<'a>(
        &'a self,
        context: &'a TrustedRemoteContext,
        target: &'a RemoteIdentity,
    ) -> BoxFuture<'a, CurrentSourcePolicy>;
    fn probe_or_materialize<'a>(
        &'a self,
        context: &'a TrustedRemoteContext,
        target: &'a PinnedRemoteTarget,
        stage: MaterializationState,
    ) -> BoxFuture<'a, RemoteReadOutcome>;
}
