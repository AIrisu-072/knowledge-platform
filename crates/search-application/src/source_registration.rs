//! Source-neutral, host-owned registration and synthetic contract ledger.
//!
//! The in-memory authority and ledger below are test fixtures. Production must
//! provide a durable implementation of the same port and a complete host-owned
//! snapshot for *each* namespace before exposing any actor-facing route.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use search_core::id::SourceId;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use sha2::{Digest, Sha256};

use crate::SearchError;
use crate::ports::{AccessDecision, BoxFuture};
use crate::remote_registration::{CurrentAccessContract, RemoteSourceRegistration};
use crate::scoped::{
    AccessContextAuthorityPort, CurrentSourceVisibilityPort, RegistrationRevision,
    ScopedSourceRegistryPort, TenantId, TrustedSearchScope, VisibilityRevision,
    VisibleCatalogSnapshot, VisibleSourceRegistration, check_actor_current,
};

fn invalid_registration() -> SearchError {
    SearchError::InvalidRequest("invalid Source registration".into())
}

fn unavailable() -> SearchError {
    SearchError::OperationFailed("registration authority unavailable".into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceKind {
    Document,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RegistrationNamespace {
    Document,
    Remote,
}

impl RegistrationNamespace {
    const fn kind(self) -> SourceKind {
        match self {
            Self::Document => SourceKind::Document,
            Self::Remote => SourceKind::Remote,
        }
    }
}

/// Non-secret host binding identifier. URLs, paths and credentials are excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentAdapterRef(String);

impl DocumentAdapterRef {
    pub fn new(value: impl Into<String>) -> Result<Self, SearchError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(invalid_registration());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Capabilities reported by a connected, trusted Document adapter. The host
/// supplies this port, not an HTTP caller or a Document provider response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedDocumentAdapterCapabilities {
    binding: DocumentAdapterRef,
    resource_kinds: Vec<ResourceKind>,
    local_modes: Vec<DiscoveryMode>,
    enumeration_semantics: Vec<EnumerationSemantics>,
    retention_modes: Vec<RetentionMode>,
}

impl ConnectedDocumentAdapterCapabilities {
    pub fn new(
        binding: DocumentAdapterRef,
        resource_kinds: Vec<ResourceKind>,
        local_modes: Vec<DiscoveryMode>,
        enumeration_semantics: Vec<EnumerationSemantics>,
        retention_modes: Vec<RetentionMode>,
    ) -> Result<Self, SearchError> {
        if resource_kinds.is_empty()
            || local_modes.is_empty()
            || enumeration_semantics.is_empty()
            || retention_modes.is_empty()
            || !unique(&resource_kinds)
            || !unique(&local_modes)
            || !unique(&enumeration_semantics)
            || !unique(&retention_modes)
            || local_modes.iter().any(|mode| {
                !matches!(
                    mode,
                    DiscoveryMode::LocalDirectory | DiscoveryMode::LocalContentSearch
                )
            })
        {
            return Err(invalid_registration());
        }
        Ok(Self {
            binding,
            resource_kinds,
            local_modes,
            enumeration_semantics,
            retention_modes,
        })
    }
}

fn unique<T: Eq + std::hash::Hash>(values: &[T]) -> bool {
    values.iter().collect::<HashSet<_>>().len() == values.len()
}

pub trait DocumentAdapterCapabilityPort: Send + Sync {
    /// Return `None` unless this binding is actually connected and usable now.
    fn connected_capabilities<'a>(
        &'a self,
        binding: &'a DocumentAdapterRef,
    ) -> BoxFuture<'a, Option<ConnectedDocumentAdapterCapabilities>>;
}

/// Issued only after the trusted adapter port reports a matching live binding.
#[derive(Debug, Clone)]
pub struct ConnectedDocumentAdapterWitness {
    capabilities: ConnectedDocumentAdapterCapabilities,
}

impl ConnectedDocumentAdapterWitness {
    pub async fn from_connected_port(
        port: &dyn DocumentAdapterCapabilityPort,
        binding: &DocumentAdapterRef,
    ) -> Result<Option<Self>, SearchError> {
        let capabilities = port.connected_capabilities(binding).await?;
        capabilities
            .map(|capabilities| {
                if &capabilities.binding != binding {
                    return Err(invalid_registration());
                }
                Ok(Self { capabilities })
            })
            .transpose()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerDocumentRegistrationConfig {
    pub tenant: TenantId,
    pub source_id: SourceId,
    pub document_adapter_ref: DocumentAdapterRef,
    pub allowed_resource_kinds: Vec<ResourceKind>,
    pub supported_modes: Vec<DiscoveryMode>,
    pub enumeration_semantics: EnumerationSemantics,
    pub retention_mode: RetentionMode,
    pub registration_revision: RegistrationRevision,
    pub visibility_revision: VisibilityRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSourceRegistration {
    tenant: TenantId,
    source_id: SourceId,
    document_adapter_ref: DocumentAdapterRef,
    allowed_resource_kinds: Vec<ResourceKind>,
    supported_modes: Vec<DiscoveryMode>,
    enumeration_semantics: EnumerationSemantics,
    retention_mode: RetentionMode,
    registration_revision: RegistrationRevision,
    visibility_revision: VisibilityRevision,
}

impl DocumentSourceRegistration {
    pub fn from_server_config(
        config: ServerDocumentRegistrationConfig,
        witness: &ConnectedDocumentAdapterWitness,
    ) -> Result<Self, SearchError> {
        let connected = &witness.capabilities;
        if config.source_id.as_uuid().is_nil()
            || config.document_adapter_ref != connected.binding
            || config.allowed_resource_kinds.is_empty()
            || config.supported_modes.is_empty()
            || !unique(&config.allowed_resource_kinds)
            || !unique(&config.supported_modes)
            || config
                .allowed_resource_kinds
                .iter()
                .any(|kind| !connected.resource_kinds.contains(kind))
            || config.supported_modes.iter().any(|mode| {
                !matches!(
                    mode,
                    DiscoveryMode::LocalDirectory | DiscoveryMode::LocalContentSearch
                ) || !connected.local_modes.contains(mode)
            })
            || !connected
                .enumeration_semantics
                .contains(&config.enumeration_semantics)
            || !connected.retention_modes.contains(&config.retention_mode)
        {
            return Err(invalid_registration());
        }
        Ok(Self {
            tenant: config.tenant,
            source_id: config.source_id,
            document_adapter_ref: config.document_adapter_ref,
            allowed_resource_kinds: config.allowed_resource_kinds,
            supported_modes: config.supported_modes,
            enumeration_semantics: config.enumeration_semantics,
            retention_mode: config.retention_mode,
            registration_revision: config.registration_revision,
            visibility_revision: config.visibility_revision,
        })
    }

    pub fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    pub const fn source_id(&self) -> SourceId {
        self.source_id
    }

    pub fn document_adapter_ref(&self) -> &DocumentAdapterRef {
        &self.document_adapter_ref
    }

    pub fn allowed_resource_kinds(&self) -> &[ResourceKind] {
        &self.allowed_resource_kinds
    }

    pub fn supported_modes(&self) -> &[DiscoveryMode] {
        &self.supported_modes
    }

    pub const fn enumeration_semantics(&self) -> EnumerationSemantics {
        self.enumeration_semantics
    }

    pub const fn retention_mode(&self) -> RetentionMode {
        self.retention_mode
    }

    pub const fn registration_revision(&self) -> RegistrationRevision {
        self.registration_revision
    }

    pub const fn visibility_revision(&self) -> VisibilityRevision {
        self.visibility_revision
    }

    fn discoverable_source(&self) -> DiscoverableSource {
        let mut source = DiscoverableSource::new(
            self.source_id,
            "document",
            self.enumeration_semantics,
            self.retention_mode,
        );
        source.resource_types = self.allowed_resource_kinds.clone();
        source.discovery_modes = self.supported_modes.clone();
        source
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
// The frozen source-neutral port carries the full, typed Remote DTO by value.
// Boxing this variant would change the shared application contract.
#[allow(clippy::large_enum_variant)]
pub enum SourceRegistration {
    Document(DocumentSourceRegistration),
    Remote(RemoteSourceRegistration),
}

impl SourceRegistration {
    pub fn tenant(&self) -> &TenantId {
        match self {
            Self::Document(value) => value.tenant(),
            Self::Remote(value) => value.tenant(),
        }
    }

    pub const fn source_id(&self) -> SourceId {
        match self {
            Self::Document(value) => value.source_id(),
            Self::Remote(value) => value.source_id(),
        }
    }

    pub const fn kind(&self) -> SourceKind {
        match self {
            Self::Document(_) => SourceKind::Document,
            Self::Remote(_) => SourceKind::Remote,
        }
    }

    pub const fn registration_revision(&self) -> RegistrationRevision {
        match self {
            Self::Document(value) => value.registration_revision(),
            Self::Remote(value) => value.registration_revision(),
        }
    }

    pub const fn visibility_revision(&self) -> VisibilityRevision {
        match self {
            Self::Document(value) => value.visibility_revision(),
            Self::Remote(value) => value.visibility_revision(),
        }
    }

    pub fn discoverable_source(&self) -> DiscoverableSource {
        match self {
            Self::Document(value) => value.discoverable_source(),
            Self::Remote(value) => value.discoverable_source(),
        }
    }

    pub fn authority_descriptor(&self) -> SourceAuthorityDescriptor {
        SourceAuthorityDescriptor {
            tenant: self.tenant().clone(),
            source_id: self.source_id(),
            kind: self.kind(),
            registration_revision: self.registration_revision(),
            visibility_revision: self.visibility_revision(),
        }
    }

    fn same_definition_ignoring_visibility(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Document(left), Self::Document(right)) => {
                let mut normalized = left.clone();
                normalized.visibility_revision = right.visibility_revision;
                normalized == *right
            }
            (Self::Remote(left), Self::Remote(right)) => {
                left.same_definition_ignoring_visibility(right)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAuthorityDescriptor {
    tenant: TenantId,
    source_id: SourceId,
    kind: SourceKind,
    registration_revision: RegistrationRevision,
    visibility_revision: VisibilityRevision,
}

impl SourceAuthorityDescriptor {
    pub fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    pub const fn source_id(&self) -> SourceId {
        self.source_id
    }

    pub const fn kind(&self) -> SourceKind {
        self.kind
    }

    pub const fn registration_revision(&self) -> RegistrationRevision {
        self.registration_revision
    }

    pub const fn visibility_revision(&self) -> VisibilityRevision {
        self.visibility_revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistrationActivation(u64);

impl RegistrationActivation {
    pub fn from_persisted(value: u64) -> Result<Self, SearchError> {
        (value > 0)
            .then_some(Self(value))
            .ok_or_else(invalid_registration)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    fn next(self) -> Result<Self, SearchError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(invalid_registration)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistrationSetRevision(u64);

impl RegistrationSetRevision {
    pub fn new(value: u64) -> Result<Self, SearchError> {
        (value > 0)
            .then_some(Self(value))
            .ok_or_else(invalid_registration)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RegistrationSetDigest([u8; 32]);

impl fmt::Debug for RegistrationSetDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RegistrationSetDigest(<sha256>)")
    }
}

impl RegistrationSetDigest {
    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

struct CanonicalFrame(Vec<u8>);

impl CanonicalFrame {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn field(&mut self, tag: u8, bytes: &[u8]) -> Result<(), SearchError> {
        let length = u64::try_from(bytes.len()).map_err(|_| invalid_registration())?;
        self.0.push(tag);
        self.0.extend_from_slice(&length.to_be_bytes());
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    fn text(&mut self, tag: u8, value: &str) -> Result<(), SearchError> {
        self.field(tag, value.as_bytes())
    }

    fn byte(&mut self, tag: u8, value: u8) -> Result<(), SearchError> {
        self.field(tag, &[value])
    }

    fn number(&mut self, tag: u8, value: u64) -> Result<(), SearchError> {
        self.field(tag, &value.to_be_bytes())
    }

    fn usize(&mut self, tag: u8, value: usize) -> Result<(), SearchError> {
        self.number(
            tag,
            u64::try_from(value).map_err(|_| invalid_registration())?,
        )
    }

    fn optional_text(&mut self, tag: u8, value: Option<&str>) -> Result<(), SearchError> {
        let mut nested = Self::new();
        match value {
            None => nested.byte(1, 0)?,
            Some(value) => {
                nested.byte(1, 1)?;
                nested.text(2, value)?;
            }
        }
        self.field(tag, &nested.0)
    }

    fn vector<T>(
        &mut self,
        tag: u8,
        values: &[T],
        mut encode: impl FnMut(&T) -> Result<Vec<u8>, SearchError>,
    ) -> Result<(), SearchError> {
        let mut nested = Self::new();
        nested.usize(1, values.len())?;
        for value in values {
            nested.field(2, &encode(value)?)?;
        }
        self.field(tag, &nested.0)
    }
}

const fn mode_tag(value: DiscoveryMode) -> u8 {
    match value {
        DiscoveryMode::LocalDirectory => 1,
        DiscoveryMode::LocalContentSearch => 2,
        DiscoveryMode::RemoteEnumeration => 3,
        DiscoveryMode::RemoteQuery => 4,
        DiscoveryMode::DirectAddress => 5,
        DiscoveryMode::LiveOnly => 6,
    }
}

const fn enumeration_tag(value: EnumerationSemantics) -> u8 {
    match value {
        EnumerationSemantics::Complete => 1,
        EnumerationSemantics::Partial => 2,
        EnumerationSemantics::QueryOnly => 3,
        EnumerationSemantics::None => 4,
    }
}

const fn retention_tag(value: RetentionMode) -> u8 {
    match value {
        RetentionMode::PersistentResource => 1,
        RetentionMode::PersistentDiscoveryMetadata => 2,
        RetentionMode::CacheWithExpiry => 3,
        RetentionMode::SessionOnly => 4,
        RetentionMode::NoRetention => 5,
    }
}

const fn resource_kind_tag(value: ResourceKind) -> u8 {
    match value {
        ResourceKind::Knowledge => 1,
        ResourceKind::Document => 2,
        ResourceKind::FolderPlacement => 3,
        ResourceKind::Semantic => 4,
        ResourceKind::Capability => 5,
        ResourceKind::AgentSkill => 6,
        ResourceKind::Workflow => 7,
        ResourceKind::Policy => 8,
    }
}

fn encode_registration(registration: &SourceRegistration) -> Result<Vec<u8>, SearchError> {
    let mut frame = CanonicalFrame::new();
    frame.text(1, registration.tenant().as_str())?;
    frame.byte(
        2,
        match registration.kind() {
            SourceKind::Document => 1,
            SourceKind::Remote => 2,
        },
    )?;
    frame.field(3, registration.source_id().as_uuid().as_bytes())?;
    frame.number(4, registration.registration_revision().get())?;
    frame.number(5, registration.visibility_revision().get())?;
    match registration {
        SourceRegistration::Document(value) => {
            let DocumentSourceRegistration {
                tenant,
                source_id,
                document_adapter_ref,
                allowed_resource_kinds,
                supported_modes,
                enumeration_semantics,
                retention_mode,
                registration_revision,
                visibility_revision,
            } = value;
            let _ = (
                tenant,
                source_id,
                document_adapter_ref,
                allowed_resource_kinds,
                supported_modes,
                enumeration_semantics,
                retention_mode,
                registration_revision,
                visibility_revision,
            );
            frame.text(10, value.document_adapter_ref().as_str())?;
            frame.vector(11, value.allowed_resource_kinds(), |item| {
                Ok(vec![resource_kind_tag(*item)])
            })?;
            frame.vector(12, value.supported_modes(), |item| {
                Ok(vec![mode_tag(*item)])
            })?;
            frame.byte(13, enumeration_tag(value.enumeration_semantics()))?;
            frame.byte(14, retention_tag(value.retention_mode()))?;
        }
        SourceRegistration::Remote(value) => {
            value.assert_canonical_schema_v1();
            frame.text(20, value.provider_kind())?;
            let endpoint = value.endpoint();
            endpoint.assert_canonical_schema_v1();
            frame.text(21, endpoint.scheme())?;
            frame.text(22, endpoint.host())?;
            frame.field(23, &endpoint.port().to_be_bytes())?;
            frame.text(24, endpoint.base_path())?;
            frame.vector(25, value.supported_modes(), |item| {
                Ok(vec![mode_tag(*item)])
            })?;
            frame.byte(26, enumeration_tag(value.enumeration_semantics()))?;
            frame.vector(27, value.authority_predicates(), |item| {
                Ok(item.as_bytes().to_vec())
            })?;
            frame.vector(28, value.allowed_resource_kinds(), |item| {
                Ok(vec![resource_kind_tag(*item)])
            })?;
            frame.byte(
                29,
                match value.current_access_contract() {
                    CurrentAccessContract::PerItem => 1,
                    CurrentAccessContract::PublicReadWithFieldPolicy => 2,
                },
            )?;
            frame.byte(30, retention_tag(value.retention_mode()))?;
            frame.optional_text(31, value.freshness_policy())?;
            frame.text(32, value.canonical_upstream_lineage())?;
            let limits = value.limits();
            limits.assert_canonical_schema_v1();
            frame.number(33, limits.call_millis)?;
            frame.number(34, limits.evaluation_millis)?;
            frame.usize(35, limits.max_request_bytes)?;
            frame.usize(36, limits.max_decoded_response_bytes)?;
            frame.usize(37, limits.max_hits_per_page)?;
            frame.usize(38, limits.max_pages_or_requests)?;
            frame.usize(39, limits.max_hits)?;
            frame.usize(40, limits.max_actions)?;
            frame.usize(41, limits.max_native_id_bytes)?;
            frame.usize(42, limits.max_cursor_bytes)?;
            frame.usize(43, limits.max_json_depth)?;
        }
    }
    Ok(frame.0)
}

fn canonical_digest(
    namespace: RegistrationNamespace,
    registrations: &BTreeMap<SourceId, SourceRegistration>,
) -> Result<RegistrationSetDigest, SearchError> {
    let mut frame = CanonicalFrame::new();
    frame.text(
        1,
        match namespace {
            RegistrationNamespace::Document => "document-desired-set:v1",
            RegistrationNamespace::Remote => "remote-desired-set:v1",
        },
    )?;
    frame.usize(2, registrations.len())?;
    let mut ordered: Vec<_> = registrations.iter().collect();
    ordered.sort_by_key(|(id, _)| *id.as_uuid().as_bytes());
    for (id, registration) in ordered {
        if *id != registration.source_id() || registration.kind() != namespace.kind() {
            return Err(invalid_registration());
        }
        frame.field(3, &encode_registration(registration)?)?;
    }
    Ok(RegistrationSetDigest(Sha256::digest(&frame.0).into()))
}

fn collect_unique(
    namespace: RegistrationNamespace,
    registrations: Vec<SourceRegistration>,
) -> Result<BTreeMap<SourceId, SourceRegistration>, SearchError> {
    let mut unique = BTreeMap::new();
    for registration in registrations {
        let id = registration.source_id();
        if id.as_uuid().is_nil()
            || registration.kind() != namespace.kind()
            || unique.insert(id, registration).is_some()
        {
            return Err(invalid_registration());
        }
    }
    Ok(unique)
}

/// A complete inventory captured from the trusted host registration authority.
/// A host adapter must enumerate every tenant in the namespace before calling
/// this constructor. It is not a request/provider DTO or a tenant-local map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRegistrationSnapshot {
    namespace: RegistrationNamespace,
    deployment_revision: RegistrationSetRevision,
    set_digest: RegistrationSetDigest,
    registrations: BTreeMap<SourceId, SourceRegistration>,
}

impl HostRegistrationSnapshot {
    pub fn from_complete_host_inventory(
        namespace: RegistrationNamespace,
        deployment_revision: RegistrationSetRevision,
        registrations: Vec<SourceRegistration>,
    ) -> Result<Self, SearchError> {
        let registrations = collect_unique(namespace, registrations)?;
        let set_digest = canonical_digest(namespace, &registrations)?;
        Ok(Self {
            namespace,
            deployment_revision,
            set_digest,
            registrations,
        })
    }
}

/// Supplied by the trusted composition root and independently read again by
/// the ledger immediately before it commits a namespace replacement.
pub trait HostRegistrationSnapshotPort: Send + Sync {
    fn snapshot<'a>(
        &'a self,
        namespace: RegistrationNamespace,
    ) -> BoxFuture<'a, HostRegistrationSnapshot>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteDesiredRegistrations {
    namespace: RegistrationNamespace,
    deployment_revision: RegistrationSetRevision,
    set_digest: RegistrationSetDigest,
    registrations: BTreeMap<SourceId, SourceRegistration>,
}

impl CompleteDesiredRegistrations {
    pub async fn capture(
        host: &dyn HostRegistrationSnapshotPort,
        namespace: RegistrationNamespace,
    ) -> Result<Self, SearchError> {
        let snapshot = host.snapshot(namespace).await?;
        if snapshot.namespace != namespace
            || canonical_digest(namespace, &snapshot.registrations)? != snapshot.set_digest
        {
            return Err(invalid_registration());
        }
        Ok(Self {
            namespace,
            deployment_revision: snapshot.deployment_revision,
            set_digest: snapshot.set_digest,
            registrations: snapshot.registrations,
        })
    }

    pub const fn namespace(&self) -> RegistrationNamespace {
        self.namespace
    }

    pub const fn deployment_revision(&self) -> RegistrationSetRevision {
        self.deployment_revision
    }

    pub const fn set_digest(&self) -> RegistrationSetDigest {
        self.set_digest
    }

    pub fn len(&self) -> usize {
        self.registrations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.registrations.is_empty()
    }

    pub fn registrations(&self) -> &BTreeMap<SourceId, SourceRegistration> {
        &self.registrations
    }
}

/// Explicit synthetic host fixture. Its inventory is independent from a
/// candidate desired set and can advance between capture and ledger commit.
#[derive(Debug, Default)]
pub struct SyntheticHostRegistrationAuthority {
    snapshots: RwLock<BTreeMap<RegistrationNamespace, HostRegistrationSnapshot>>,
}

impl SyntheticHostRegistrationAuthority {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(
        &self,
        namespace: RegistrationNamespace,
        deployment_revision: RegistrationSetRevision,
        registrations: Vec<SourceRegistration>,
    ) -> Result<(), SearchError> {
        let snapshot = HostRegistrationSnapshot::from_complete_host_inventory(
            namespace,
            deployment_revision,
            registrations,
        )?;
        let mut stored = self.snapshots.write().map_err(|_| unavailable())?;
        if stored
            .get(&namespace)
            .is_some_and(|old| old.deployment_revision >= deployment_revision)
        {
            return Err(invalid_registration());
        }
        stored.insert(namespace, snapshot);
        Ok(())
    }
}

impl HostRegistrationSnapshotPort for SyntheticHostRegistrationAuthority {
    fn snapshot<'a>(
        &'a self,
        namespace: RegistrationNamespace,
    ) -> BoxFuture<'a, HostRegistrationSnapshot> {
        Box::pin(async move {
            self.snapshots
                .read()
                .map_err(|_| unavailable())?
                .get(&namespace)
                .cloned()
                .ok_or_else(unavailable)
        })
    }
}

/// One host-owned ledger for both kinds. Production implementations must make
/// each reconcile atomic with all-tenant owner, kind, DTO and activation state.
pub trait SourceRegistrationLedgerPort: Send + Sync {
    fn reconcile<'a>(
        &'a self,
        desired: &'a CompleteDesiredRegistrations,
    ) -> BoxFuture<'a, BTreeMap<SourceId, RegistrationActivation>>;

    fn is_current<'a>(
        &'a self,
        registration: &'a SourceRegistration,
        activation: RegistrationActivation,
    ) -> BoxFuture<'a, bool>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyntheticLedgerEntry {
    last: SourceRegistration,
    activation: RegistrationActivation,
    active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NamespaceReceipt {
    deployment_revision: RegistrationSetRevision,
    set_digest: RegistrationSetDigest,
    registrations: BTreeMap<SourceId, SourceRegistration>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyntheticLedgerStateSnapshot {
    entries: BTreeMap<SourceId, SyntheticLedgerEntry>,
    namespaces: BTreeMap<RegistrationNamespace, NamespaceReceipt>,
}

impl SyntheticLedgerStateSnapshot {
    pub fn activation(&self, source: SourceId) -> Option<RegistrationActivation> {
        self.entries.get(&source).map(|entry| entry.activation)
    }
}

/// Contract fixture only; recreating this object does not provide disk or
/// multi-replica durability. It holds a separate host inventory for comparison.
#[derive(Debug)]
pub struct SyntheticRegistrationLedger {
    host: Arc<SyntheticHostRegistrationAuthority>,
    state: RwLock<SyntheticLedgerStateSnapshot>,
    fail_after_commit: AtomicBool,
}

impl Default for SyntheticRegistrationLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntheticRegistrationLedger {
    pub fn new() -> Self {
        Self::with_host(Arc::new(SyntheticHostRegistrationAuthority::new()))
    }

    pub fn with_host(host: Arc<SyntheticHostRegistrationAuthority>) -> Self {
        Self {
            host,
            state: RwLock::new(SyntheticLedgerStateSnapshot::default()),
            fail_after_commit: AtomicBool::new(false),
        }
    }

    pub fn host_for_synthetic(&self) -> Arc<SyntheticHostRegistrationAuthority> {
        self.host.clone()
    }

    pub fn state_for_testing(&self) -> Result<SyntheticLedgerStateSnapshot, SearchError> {
        Ok(self.state.read().map_err(|_| unavailable())?.clone())
    }

    pub fn fail_after_commit_once(&self) {
        self.fail_after_commit.store(true, Ordering::Release);
    }
}

fn candidate_matches_host(
    desired: &CompleteDesiredRegistrations,
    host: &HostRegistrationSnapshot,
) -> Result<bool, SearchError> {
    let desired_digest = canonical_digest(desired.namespace, &desired.registrations)?;
    let host_digest = canonical_digest(host.namespace, &host.registrations)?;
    Ok(desired.namespace == host.namespace
        && desired.deployment_revision == host.deployment_revision
        && desired.set_digest == desired_digest
        && host.set_digest == host_digest
        && desired.set_digest == host.set_digest
        && desired.registrations == host.registrations)
}

impl SourceRegistrationLedgerPort for SyntheticRegistrationLedger {
    fn reconcile<'a>(
        &'a self,
        desired: &'a CompleteDesiredRegistrations,
    ) -> BoxFuture<'a, BTreeMap<SourceId, RegistrationActivation>> {
        Box::pin(async move {
            // First independent host read. The second read remains locked until
            // after the synthetic commit, modelling the DB serial boundary.
            let first = self.host.snapshot(desired.namespace).await?;
            if !candidate_matches_host(desired, &first)? {
                return Err(invalid_registration());
            }
            let mut state = self.state.write().map_err(|_| unavailable())?;
            let host_guard = self.host.snapshots.read().map_err(|_| unavailable())?;
            let second = host_guard.get(&desired.namespace).ok_or_else(unavailable)?;
            if !candidate_matches_host(desired, second)? {
                return Err(invalid_registration());
            }
            if let Some(prior) = state.namespaces.get(&desired.namespace) {
                if desired.deployment_revision < prior.deployment_revision
                    || (desired.deployment_revision == prior.deployment_revision
                        && (desired.set_digest != prior.set_digest
                            || desired.registrations != prior.registrations))
                {
                    return Err(invalid_registration());
                }
                if desired.deployment_revision == prior.deployment_revision {
                    return desired
                        .registrations
                        .keys()
                        .map(|id| {
                            let entry = state.entries.get(id).ok_or_else(unavailable)?;
                            if !entry.active || entry.last != desired.registrations[id] {
                                return Err(unavailable());
                            }
                            Ok((*id, entry.activation))
                        })
                        .collect();
                }
            }

            let mut next = state.clone();
            for (id, registration) in &desired.registrations {
                if *id != registration.source_id()
                    || registration.kind() != desired.namespace.kind()
                {
                    return Err(invalid_registration());
                }
                if let Some(previous) = next.entries.get(id)
                    && (registration.tenant() != previous.last.tenant()
                        || registration.kind() != previous.last.kind()
                        || registration.registration_revision()
                            < previous.last.registration_revision()
                        || registration.visibility_revision() < previous.last.visibility_revision()
                        || (registration.registration_revision()
                            == previous.last.registration_revision()
                            && !registration.same_definition_ignoring_visibility(&previous.last))
                        || (!previous.active
                            && registration.registration_revision()
                                == previous.last.registration_revision()
                            && registration.visibility_revision()
                                == previous.last.visibility_revision()))
                {
                    return Err(invalid_registration());
                }
            }
            for (id, entry) in &mut next.entries {
                if entry.last.kind() == desired.namespace.kind()
                    && !desired.registrations.contains_key(id)
                    && entry.active
                {
                    entry.activation = entry.activation.next()?;
                    entry.active = false;
                }
            }
            let mut activations = BTreeMap::new();
            for (id, registration) in &desired.registrations {
                let activation = match next.entries.get_mut(id) {
                    Some(entry) => {
                        if !entry.active || entry.last != *registration {
                            entry.activation = entry.activation.next()?;
                        }
                        entry.last = registration.clone();
                        entry.active = true;
                        entry.activation
                    }
                    None => {
                        let activation = RegistrationActivation(1);
                        next.entries.insert(
                            *id,
                            SyntheticLedgerEntry {
                                last: registration.clone(),
                                activation,
                                active: true,
                            },
                        );
                        activation
                    }
                };
                activations.insert(*id, activation);
            }
            next.namespaces.insert(
                desired.namespace,
                NamespaceReceipt {
                    deployment_revision: desired.deployment_revision,
                    set_digest: desired.set_digest,
                    registrations: desired.registrations.clone(),
                },
            );
            *state = next;
            drop(host_guard);
            if self.fail_after_commit.swap(false, Ordering::AcqRel) {
                return Err(SearchError::CompletionUnknown);
            }
            Ok(activations)
        })
    }

    fn is_current<'a>(
        &'a self,
        registration: &'a SourceRegistration,
        activation: RegistrationActivation,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let state = self.state.read().map_err(|_| unavailable())?;
            Ok(matches!(state.entries.get(&registration.source_id()),
                Some(entry) if entry.active
                    && entry.activation == activation
                    && entry.last == *registration
                    && entry.last.tenant() == registration.tenant()
                    && entry.last.kind() == registration.kind()
                    && entry.last.registration_revision() == registration.registration_revision()
                    && entry.last.visibility_revision() == registration.visibility_revision()
            ))
        })
    }
}

/// Local union projection of the authoritative ledger. It cannot establish
/// owner identity or current registration without the ledger port.
///
/// A caller cannot turn a partial Remote `Vec` directly into an actor-visible
/// catalog. Both namespace snapshots must come from the host authority.
///
/// ```compile_fail
/// use search_application::source_registration::SourceRegistrationCatalog;
/// use search_application::remote_registration::RemoteSourceRegistration;
/// async fn bypass() {
///     let _ = SourceRegistrationCatalog::try_new_synthetic(
///         Vec::<RemoteSourceRegistration>::new(),
///     ).await;
/// }
/// ```
///
/// ```compile_fail
/// use std::sync::Arc;
/// use search_application::source_registration::{SourceRegistrationCatalog, SyntheticRegistrationLedger};
/// use search_application::remote_registration::RemoteSourceRegistration;
/// async fn bypass(ledger: Arc<SyntheticRegistrationLedger>) {
///     let _ = SourceRegistrationCatalog::try_new_synthetic_with_ledger(
///         ledger, Vec::<RemoteSourceRegistration>::new(),
///     ).await;
/// }
/// ```
///
/// ```compile_fail
/// use search_application::source_registration::SourceRegistrationCatalog;
/// use search_application::remote_registration::RemoteSourceRegistration;
/// async fn bypass(catalog: &SourceRegistrationCatalog) {
///     let _ = catalog.replace_synthetic_checked(
///         Vec::<RemoteSourceRegistration>::new(),
///     ).await;
/// }
/// ```
pub struct SourceRegistrationCatalog {
    ledger: Arc<dyn SourceRegistrationLedgerPort>,
    registrations: RwLock<BTreeMap<SourceId, (SourceRegistration, RegistrationActivation)>>,
    updating: AtomicBool,
    uncertain: AtomicBool,
}

impl fmt::Debug for SourceRegistrationCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SourceRegistrationCatalog(<host-owned>)")
    }
}

fn receipt_projection(
    desired: &CompleteDesiredRegistrations,
    activations: BTreeMap<SourceId, RegistrationActivation>,
) -> Result<BTreeMap<SourceId, (SourceRegistration, RegistrationActivation)>, SearchError> {
    if desired.registrations.len() != activations.len()
        || desired.registrations.keys().ne(activations.keys())
    {
        return Err(invalid_registration());
    }
    Ok(desired
        .registrations
        .iter()
        .map(|(id, registration)| (*id, (registration.clone(), activations[id])))
        .collect())
}

impl SourceRegistrationCatalog {
    /// Production startup requires both complete namespace snapshots and one
    /// durable host ledger. No actor-facing catalog escapes a partial startup.
    pub async fn try_new(
        ledger: Arc<dyn SourceRegistrationLedgerPort>,
        document: &CompleteDesiredRegistrations,
        remote: &CompleteDesiredRegistrations,
    ) -> Result<Self, SearchError> {
        if document.namespace != RegistrationNamespace::Document
            || remote.namespace != RegistrationNamespace::Remote
        {
            return Err(invalid_registration());
        }
        let document_receipts = ledger.reconcile(document).await?;
        let remote_receipts = ledger.reconcile(remote).await?;
        let mut registrations = receipt_projection(document, document_receipts)?;
        for (id, entry) in receipt_projection(remote, remote_receipts)? {
            if registrations.insert(id, entry).is_some() {
                return Err(invalid_registration());
            }
        }
        for (registration, activation) in registrations.values() {
            if !ledger.is_current(registration, *activation).await? {
                return Err(unavailable());
            }
        }
        Ok(Self {
            ledger,
            registrations: RwLock::new(registrations),
            updating: AtomicBool::new(false),
            uncertain: AtomicBool::new(false),
        })
    }

    /// Host-ledger reconcile succeeds before the local projection changes.
    /// Any unknown/error outcome closes every local current gate until a fresh
    /// authoritative reconcile verifies and repairs the projection.
    pub async fn replace_checked(
        &self,
        desired: &CompleteDesiredRegistrations,
    ) -> Result<(), SearchError> {
        if canonical_digest(desired.namespace, &desired.registrations)? != desired.set_digest {
            return Err(invalid_registration());
        }
        if self
            .updating
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(unavailable());
        }
        struct ResetUpdate<'a>(&'a AtomicBool);
        impl Drop for ResetUpdate<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = ResetUpdate(&self.updating);
        self.uncertain.store(true, Ordering::Release);
        let activations = self.ledger.reconcile(desired).await?;
        let replacement = receipt_projection(desired, activations)?;
        for (registration, activation) in replacement.values() {
            if !self.ledger.is_current(registration, *activation).await? {
                return Err(unavailable());
            }
        }
        let mut local = self.registrations.write().map_err(|_| unavailable())?;
        let mut next = local.clone();
        next.retain(|_, (registration, _)| registration.kind() != desired.namespace.kind());
        for (id, entry) in replacement {
            if next.insert(id, entry).is_some() {
                return Err(invalid_registration());
            }
        }
        *local = next;
        self.uncertain.store(false, Ordering::Release);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.registrations.read().map_or(0, |entries| entries.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Operator inspection only; request paths use ScopedSourceRegistryPort.
    pub fn get_for_server(&self, source: SourceId) -> Option<SourceRegistration> {
        self.registrations
            .read()
            .ok()?
            .get(&source)
            .map(|(registration, _)| registration.clone())
    }

    fn ready(&self) -> Result<(), SearchError> {
        if self.updating.load(Ordering::Acquire) || self.uncertain.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        Ok(())
    }

    fn values(&self) -> Result<Vec<SourceRegistration>, SearchError> {
        self.ready()?;
        let local = self.registrations.read().map_err(|_| unavailable())?;
        Ok(local
            .values()
            .map(|(registration, _)| registration.clone())
            .collect())
    }

    /// Both checked and synthetic visibility adapters use this exact gate.
    pub(crate) async fn current_activation(
        &self,
        tenant: &TenantId,
        source: SourceId,
        registration_revision: RegistrationRevision,
        visibility_revision: VisibilityRevision,
    ) -> Result<Option<RegistrationActivation>, SearchError> {
        if self.updating.load(Ordering::Acquire) || self.uncertain.load(Ordering::Acquire) {
            return Ok(None);
        }
        let (registration, activation) = {
            let local = self.registrations.read().map_err(|_| unavailable())?;
            let Some((registration, activation)) = local.get(&source) else {
                return Ok(None);
            };
            (registration.clone(), *activation)
        };
        if registration.tenant() != tenant
            || registration.registration_revision() != registration_revision
            || registration.visibility_revision() != visibility_revision
            || !self.ledger.is_current(&registration, activation).await?
            || self.updating.load(Ordering::Acquire)
            || self.uncertain.load(Ordering::Acquire)
        {
            return Ok(None);
        }
        let local = self.registrations.read().map_err(|_| unavailable())?;
        Ok(local
            .get(&source)
            .filter(|(current, current_activation)| {
                current == &registration && *current_activation == activation
            })
            .map(|_| activation))
    }
}

/// Registry view built from one union catalog and the existing actor/Source
/// mint. No Document-specific visibility decision is introduced.
pub struct TrustedVisibleRegistry<'a> {
    authority: &'a dyn AccessContextAuthorityPort,
    visibility: &'a dyn CurrentSourceVisibilityPort,
    catalog: &'a SourceRegistrationCatalog,
}

impl<'a> TrustedVisibleRegistry<'a> {
    pub fn new(
        authority: &'a dyn AccessContextAuthorityPort,
        visibility: &'a dyn CurrentSourceVisibilityPort,
        catalog: &'a SourceRegistrationCatalog,
    ) -> Self {
        Self {
            authority,
            visibility,
            catalog,
        }
    }
}

impl ScopedSourceRegistryPort for TrustedVisibleRegistry<'_> {
    fn visible_sources<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
    ) -> BoxFuture<'a, VisibleCatalogSnapshot> {
        Box::pin(async move {
            check_actor_current(self.authority, actor).await?;
            let mut visible = Vec::new();
            for registration in self.catalog.values()? {
                if registration.tenant() != actor.tenant() {
                    continue;
                }
                let Some(scope) = self
                    .visibility
                    .bind_source(actor, registration.source_id())
                    .await?
                else {
                    continue;
                };
                if scope.actor() != actor || scope.source_id() != registration.source_id() {
                    return Err(SearchError::OperationFailed(
                        "trusted scope unavailable".into(),
                    ));
                }
                if scope.registration_revision() != registration.registration_revision()
                    || scope.visibility_revision() != registration.visibility_revision()
                {
                    // This source changed after the enumeration snapshot.
                    continue;
                }
                if self.visibility.current(&scope).await? != AccessDecision::Allowed {
                    continue;
                }
                if self
                    .catalog
                    .current_activation(
                        actor.tenant(),
                        scope.source_id(),
                        scope.registration_revision(),
                        scope.visibility_revision(),
                    )
                    .await?
                    != Some(scope.registration_activation())
                {
                    continue;
                }
                visible.push(VisibleSourceRegistration::new(scope, registration)?);
            }
            check_actor_current(self.authority, actor).await?;
            self.catalog.ready()?;
            Ok(VisibleCatalogSnapshot::unstamped(visible))
        })
    }
}
