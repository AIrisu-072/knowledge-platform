//! Typed Resource identity and family, separate from display names and projections.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::id::{RelationId, ResourceId, ResourceVersionId, SourceId, UsageProfileId};
use crate::profile::DiscoveryProfile;
use crate::temporal::TemporalDiscoveryProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    Knowledge,
    Semantic,
    Capability,
    AgentSkill,
    Workflow,
    Policy,
}

/// The family contract; source content and Search Extraction remain separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceBody {
    Knowledge,
    Semantic,
    Capability,
    AgentSkill,
    Workflow,
    Policy,
}

impl ResourceBody {
    pub const fn kind(self) -> ResourceKind {
        match self {
            Self::Knowledge => ResourceKind::Knowledge,
            Self::Semantic => ResourceKind::Semantic,
            Self::Capability => ResourceKind::Capability,
            Self::AgentSkill => ResourceKind::AgentSkill,
            Self::Workflow => ResourceKind::Workflow,
            Self::Policy => ResourceKind::Policy,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceIdentity {
    pub resource_id: ResourceId,
    pub resource_type: ResourceKind,
    pub resource_version: Option<ResourceVersionId>,
    pub source_id: SourceId,
    pub source_native_id: Option<String>,
    pub provenance: Option<String>,
    pub valid_from: Option<OffsetDateTime>,
    pub valid_to: Option<OffsetDateTime>,
    pub access_scope: Option<String>,
    pub integrity_digest: Option<String>,
}

impl ResourceIdentity {
    pub fn new(resource_id: ResourceId, resource_type: ResourceKind, source_id: SourceId) -> Self {
        Self {
            resource_id,
            resource_type,
            resource_version: None,
            source_id,
            source_native_id: None,
            provenance: None,
            valid_from: None,
            valid_to: None,
            access_scope: None,
            integrity_digest: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverableResource {
    pub identity: ResourceIdentity,
    pub body: ResourceBody,
    pub usage_profile_ids: Vec<UsageProfileId>,
    pub discovery_profile: DiscoveryProfile,
    pub temporal_profile: TemporalDiscoveryProfile,
    pub relation_ids: Vec<RelationId>,
}

impl DiscoverableResource {
    pub fn new(
        identity: ResourceIdentity,
        body: ResourceBody,
        discovery_profile: DiscoveryProfile,
    ) -> Self {
        Self {
            identity,
            body,
            usage_profile_ids: Vec::new(),
            discovery_profile,
            temporal_profile: TemporalDiscoveryProfile::default(),
            relation_ids: Vec::new(),
        }
    }
}
