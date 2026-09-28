//! Authoritative Source discovery and retention capabilities.

use serde::{Deserialize, Serialize};

use crate::id::SourceId;
use crate::resource::ResourceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiscoveryMode {
    LocalDirectory,
    LocalContentSearch,
    RemoteEnumeration,
    RemoteQuery,
    DirectAddress,
    LiveOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EnumerationSemantics {
    Complete,
    Partial,
    QueryOnly,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RetentionMode {
    PersistentResource,
    PersistentDiscoveryMetadata,
    CacheWithExpiry,
    SessionOnly,
    NoRetention,
}

/// Estimated work by dimension. A missing dimension is unknown, not zero.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CostProfile {
    pub expected_latency_ms: Option<u64>,
    pub cpu_ms: Option<u64>,
    pub peak_memory_bytes: Option<u64>,
    pub io_read_bytes: Option<u64>,
    pub io_write_bytes: Option<u64>,
    pub network_bytes: Option<u64>,
    pub llm_input_tokens: Option<u64>,
    pub llm_output_tokens: Option<u64>,
    pub remote_calls: Option<u64>,
    pub monetary_cost_minor_units: Option<i64>,
    pub monetary_cost_currency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverableSource {
    pub source_id: SourceId,
    pub source_type: String,
    pub resource_types: Vec<ResourceKind>,
    pub business_domains: Vec<String>,
    pub concept_refs: Vec<String>,
    pub discovery_modes: Vec<DiscoveryMode>,
    pub discovery_capabilities: Vec<String>,
    pub enumeration_semantics: EnumerationSemantics,
    pub authority_scope: Option<String>,
    pub provenance: Option<String>,
    pub access_model: Option<String>,
    pub retention_mode: RetentionMode,
    pub freshness_policy: Option<String>,
    pub latency_profile: Option<CostProfile>,
    pub cost_profile: Option<CostProfile>,
    pub availability_state: Option<String>,
}

impl DiscoverableSource {
    pub fn new(
        source_id: SourceId,
        source_type: impl Into<String>,
        enumeration_semantics: EnumerationSemantics,
        retention_mode: RetentionMode,
    ) -> Self {
        Self {
            source_id,
            source_type: source_type.into(),
            resource_types: Vec::new(),
            business_domains: Vec::new(),
            concept_refs: Vec::new(),
            discovery_modes: Vec::new(),
            discovery_capabilities: Vec::new(),
            enumeration_semantics,
            authority_scope: None,
            provenance: None,
            access_model: None,
            retention_mode,
            freshness_policy: None,
            latency_profile: None,
            cost_profile: None,
            availability_state: None,
        }
    }

    /// Capability and retention are separate: NO_RETENTION still permits live reads.
    pub fn supports(&self, mode: DiscoveryMode) -> bool {
        self.discovery_modes.contains(&mode)
    }
}
