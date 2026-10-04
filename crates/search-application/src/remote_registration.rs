//! Server-owned remote Source registration and a visibility-gated catalog.

use std::collections::HashSet;

use search_core::id::SourceId;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};

use crate::SearchError;
use crate::scoped::{RegistrationRevision, TenantId, VisibilityRevision};

fn invalid_registration() -> SearchError {
    SearchError::InvalidRequest("invalid remote Source registration".into())
}

/// An origin fixed by operator configuration. Transport independently enforces
/// HTTPS, DNS/address policy and TLS verification in production.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredEndpoint {
    scheme: String,
    host: String,
    port: u16,
    base_path: String,
}

impl RegisteredEndpoint {
    pub(crate) fn assert_canonical_schema_v1(&self) {
        let Self {
            scheme,
            host,
            port,
            base_path,
        } = self;
        let _ = (scheme, host, port, base_path);
    }

    pub fn new(
        scheme: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        base_path: impl Into<String>,
    ) -> Result<Self, SearchError> {
        let endpoint = Self {
            scheme: scheme.into(),
            host: host.into(),
            port,
            base_path: base_path.into(),
        };
        endpoint.validate()?;
        Ok(endpoint)
    }

    fn validate(&self) -> Result<(), SearchError> {
        if !matches!(self.scheme.as_str(), "https" | "http")
            || self.host.is_empty()
            || self.host.len() > 253
            || !self
                .host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
            || self.port == 0
            || !self.base_path.starts_with('/')
            || self.base_path.len() > 1024
            || self.base_path.contains("..")
            || self.base_path.bytes().any(|byte| {
                matches!(byte, b'?' | b'#' | b'@' | b'\\' | b'%') || byte.is_ascii_control()
            })
        {
            return Err(invalid_registration());
        }
        Ok(())
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn base_path(&self) -> &str {
        &self.base_path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentAccessContract {
    PerItem,
    PublicReadWithFieldPolicy,
}

/// Transport and protocol bounds supplied by the operator, never a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteRegistrationLimits {
    pub call_millis: u64,
    pub evaluation_millis: u64,
    pub max_request_bytes: usize,
    pub max_decoded_response_bytes: usize,
    pub max_hits_per_page: usize,
    pub max_pages_or_requests: usize,
    pub max_hits: usize,
    pub max_actions: usize,
    pub max_native_id_bytes: usize,
    pub max_cursor_bytes: usize,
    pub max_json_depth: usize,
}

impl RemoteRegistrationLimits {
    pub(crate) fn assert_canonical_schema_v1(&self) {
        let Self {
            call_millis,
            evaluation_millis,
            max_request_bytes,
            max_decoded_response_bytes,
            max_hits_per_page,
            max_pages_or_requests,
            max_hits,
            max_actions,
            max_native_id_bytes,
            max_cursor_bytes,
            max_json_depth,
        } = self;
        let _ = (
            call_millis,
            evaluation_millis,
            max_request_bytes,
            max_decoded_response_bytes,
            max_hits_per_page,
            max_pages_or_requests,
            max_hits,
            max_actions,
            max_native_id_bytes,
            max_cursor_bytes,
            max_json_depth,
        );
    }

    pub const fn synthetic_canary() -> Self {
        Self {
            call_millis: 2_000,
            evaluation_millis: 5_000,
            max_request_bytes: 16 * 1024,
            max_decoded_response_bytes: 1024 * 1024,
            max_hits_per_page: 100,
            max_pages_or_requests: 32,
            max_hits: 3_200,
            max_actions: 4,
            max_native_id_bytes: 512,
            max_cursor_bytes: 1024,
            max_json_depth: 32,
        }
    }

    fn valid(self) -> bool {
        self.call_millis > 0
            && self.evaluation_millis >= self.call_millis
            && self.max_request_bytes > 0
            && self.max_decoded_response_bytes > 0
            && self.max_hits_per_page > 0
            && self.max_pages_or_requests > 0
            && self.max_hits > 0
            && self.max_actions > 0
            && self.max_native_id_bytes > 0
            && self.max_cursor_bytes > 0
            && self.max_json_depth > 0
    }
}

/// Input for trusted composition-root configuration. It is deliberately not
/// deserializable from HTTP or provider responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRemoteRegistrationConfig {
    pub tenant: TenantId,
    pub source_id: SourceId,
    pub provider_kind: String,
    pub endpoint: RegisteredEndpoint,
    pub supported_modes: Vec<DiscoveryMode>,
    pub enumeration_semantics: EnumerationSemantics,
    pub authority_predicates: Vec<String>,
    pub allowed_resource_kinds: Vec<ResourceKind>,
    pub current_access_contract: CurrentAccessContract,
    pub retention_mode: RetentionMode,
    pub freshness_policy: Option<String>,
    pub canonical_upstream_lineage: String,
    pub limits: RemoteRegistrationLimits,
    pub registration_revision: RegistrationRevision,
    pub visibility_revision: VisibilityRevision,
}

/// Fixed registration. Neither SourceId nor authority fields can be taken
/// from a provider response because no provider conversion API exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSourceRegistration {
    tenant: TenantId,
    source_id: SourceId,
    provider_kind: String,
    endpoint: RegisteredEndpoint,
    supported_modes: Vec<DiscoveryMode>,
    enumeration_semantics: EnumerationSemantics,
    authority_predicates: Vec<String>,
    allowed_resource_kinds: Vec<ResourceKind>,
    current_access_contract: CurrentAccessContract,
    retention_mode: RetentionMode,
    freshness_policy: Option<String>,
    canonical_upstream_lineage: String,
    limits: RemoteRegistrationLimits,
    registration_revision: RegistrationRevision,
    visibility_revision: VisibilityRevision,
}

impl RemoteSourceRegistration {
    pub fn from_server_config(config: ServerRemoteRegistrationConfig) -> Result<Self, SearchError> {
        if config.source_id.as_uuid().is_nil()
            || config.provider_kind.is_empty()
            || config.provider_kind.len() > 128
            || !config
                .provider_kind
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || config.canonical_upstream_lineage.is_empty()
            || config.canonical_upstream_lineage.len() > 256
            || config.supported_modes.is_empty()
            || config.supported_modes.iter().any(|mode| {
                !matches!(
                    mode,
                    DiscoveryMode::RemoteEnumeration
                        | DiscoveryMode::RemoteQuery
                        | DiscoveryMode::DirectAddress
                        | DiscoveryMode::LiveOnly
                )
            })
            || config.supported_modes.iter().collect::<HashSet<_>>().len()
                != config.supported_modes.len()
            || !config.limits.valid()
        {
            return Err(invalid_registration());
        }
        config.endpoint.validate()?;
        Ok(Self {
            tenant: config.tenant,
            source_id: config.source_id,
            provider_kind: config.provider_kind,
            endpoint: config.endpoint,
            supported_modes: config.supported_modes,
            enumeration_semantics: config.enumeration_semantics,
            authority_predicates: config.authority_predicates,
            allowed_resource_kinds: config.allowed_resource_kinds,
            current_access_contract: config.current_access_contract,
            retention_mode: config.retention_mode,
            freshness_policy: config.freshness_policy,
            canonical_upstream_lineage: config.canonical_upstream_lineage,
            limits: config.limits,
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

    pub fn provider_kind(&self) -> &str {
        &self.provider_kind
    }

    pub fn endpoint(&self) -> &RegisteredEndpoint {
        &self.endpoint
    }

    pub fn supported_modes(&self) -> &[DiscoveryMode] {
        &self.supported_modes
    }

    pub const fn enumeration_semantics(&self) -> EnumerationSemantics {
        self.enumeration_semantics
    }

    pub fn authority_predicates(&self) -> &[String] {
        &self.authority_predicates
    }

    pub fn allowed_resource_kinds(&self) -> &[ResourceKind] {
        &self.allowed_resource_kinds
    }

    pub const fn current_access_contract(&self) -> CurrentAccessContract {
        self.current_access_contract
    }

    pub const fn retention_mode(&self) -> RetentionMode {
        self.retention_mode
    }

    pub fn freshness_policy(&self) -> Option<&str> {
        self.freshness_policy.as_deref()
    }

    pub fn canonical_upstream_lineage(&self) -> &str {
        &self.canonical_upstream_lineage
    }

    pub const fn limits(&self) -> RemoteRegistrationLimits {
        self.limits
    }

    pub const fn registration_revision(&self) -> RegistrationRevision {
        self.registration_revision
    }

    pub const fn visibility_revision(&self) -> VisibilityRevision {
        self.visibility_revision
    }

    pub(crate) fn discoverable_source(&self) -> DiscoverableSource {
        let mut source = DiscoverableSource::new(
            self.source_id,
            self.provider_kind.clone(),
            self.enumeration_semantics,
            self.retention_mode,
        );
        source.resource_types = self.allowed_resource_kinds.clone();
        source.discovery_modes = self.supported_modes.clone();
        source.authority_scope = Some(self.authority_predicates.join(","));
        source.provenance = Some(self.canonical_upstream_lineage.clone());
        source.access_model = Some(
            match self.current_access_contract {
                CurrentAccessContract::PerItem => "per-item",
                CurrentAccessContract::PublicReadWithFieldPolicy => "public-read-with-field-policy",
            }
            .into(),
        );
        source.freshness_policy = self.freshness_policy.clone();
        source
    }
}

impl RemoteSourceRegistration {
    /// Keeps canonical v1 encoding exhaustive when a server-owned DTO field is
    /// added. An added field requires an explicit codec version update.
    pub(crate) fn assert_canonical_schema_v1(&self) {
        let Self {
            tenant,
            source_id,
            provider_kind,
            endpoint,
            supported_modes,
            enumeration_semantics,
            authority_predicates,
            allowed_resource_kinds,
            current_access_contract,
            retention_mode,
            freshness_policy,
            canonical_upstream_lineage,
            limits,
            registration_revision,
            visibility_revision,
        } = self;
        let _ = (
            tenant,
            source_id,
            provider_kind,
            endpoint,
            supported_modes,
            enumeration_semantics,
            authority_predicates,
            allowed_resource_kinds,
            current_access_contract,
            retention_mode,
            freshness_policy,
            canonical_upstream_lineage,
            limits,
            registration_revision,
            visibility_revision,
        );
    }

    pub(crate) fn same_definition_ignoring_visibility(&self, other: &Self) -> bool {
        let mut left = self.clone();
        left.visibility_revision = other.visibility_revision;
        left == *other
    }
}

// Compatibility facade for existing P4 trust callers. The implementation and
// one union ledger live in source_registration; no Remote-only production
// desired-set constructor remains.
pub use crate::source_registration::{
    RegistrationActivation, SourceRegistrationCatalog as RemoteRegistrationCatalog,
    SourceRegistrationLedgerPort, SyntheticRegistrationLedger, TrustedVisibleRegistry,
};
