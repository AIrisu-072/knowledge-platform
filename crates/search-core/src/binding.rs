//! Session-stable bindings; rediscovery never silently replaces representation identity.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::id::{
    BindingId, LogicalResourceId, RepresentationId, ResourceVersionId, SourceId, UsageProfileId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingMode {
    SnapshotPinned,
    RemoteVersionPinned,
    SessionSnapshot,
    LiveReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevalidationMarker {
    Required,
    NotRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalResourceBinding {
    pub binding_id: BindingId,
    pub logical_resource_ref: LogicalResourceId,
    pub usage_profile_ref: Option<UsageProfileId>,
    pub qualification_evidence_ref: String,
    pub bound_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepresentationBinding {
    pub binding_id: BindingId,
    pub logical_resource_ref: LogicalResourceId,
    pub representation_ref: RepresentationId,
    pub resource_version_ref: Option<ResourceVersionId>,
    pub source_ref: SourceId,
    pub provider_ref: Option<String>,
    pub schema_digest: Option<String>,
    pub content_digest: Option<String>,
    pub binding_mode: BindingMode,
    pub revalidation_marker: RevalidationMarker,
    pub bound_at: OffsetDateTime,
}

impl RepresentationBinding {
    pub fn new(
        binding_id: BindingId,
        logical_resource_ref: LogicalResourceId,
        representation_ref: RepresentationId,
        source_ref: SourceId,
        binding_mode: BindingMode,
        bound_at: OffsetDateTime,
    ) -> Self {
        Self {
            binding_id,
            logical_resource_ref,
            representation_ref,
            resource_version_ref: None,
            source_ref,
            provider_ref: None,
            schema_digest: None,
            content_digest: None,
            binding_mode,
            revalidation_marker: if binding_mode == BindingMode::LiveReference {
                RevalidationMarker::Required
            } else {
                RevalidationMarker::NotRequired
            },
            bound_at,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.binding_mode == BindingMode::LiveReference
            && self.revalidation_marker != RevalidationMarker::Required
        {
            return Err("live reference requires current revalidation");
        }
        if self.binding_mode == BindingMode::RemoteVersionPinned
            && self.resource_version_ref.is_none()
        {
            return Err("remote version binding requires version identity");
        }
        if self.binding_mode == BindingMode::SnapshotPinned
            && self.resource_version_ref.is_none()
            && self.content_digest.is_none()
        {
            return Err("snapshot binding requires version or content digest");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBindingSet {
    bindings: BTreeMap<BindingId, RepresentationBinding>,
}

impl SessionBindingSet {
    pub fn bind_if_absent(
        &mut self,
        binding: RepresentationBinding,
    ) -> Result<&RepresentationBinding, &'static str> {
        binding.validate()?;
        Ok(self.bindings.entry(binding.binding_id).or_insert(binding))
    }

    pub fn get(&self, binding_id: BindingId) -> Option<&RepresentationBinding> {
        self.bindings.get(&binding_id)
    }
}
