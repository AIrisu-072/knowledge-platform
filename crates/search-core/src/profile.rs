//! Discovery metadata and reusable Lens selection.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::id::{ResourceId, SourceId};
use crate::resource::ResourceKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FacetState<T> {
    Known(T),
    Unknown,
    NotApplicable,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryProfile {
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub concept_refs: Vec<String>,
    pub intents: Vec<String>,
    pub high_signal_facets: BTreeMap<String, FacetState<String>>,
    pub positive_signals: Vec<String>,
    pub negative_signals: Vec<String>,
    pub confusable_with: Vec<ResourceId>,
    pub distinguished_by: Vec<String>,
    pub projection_version: u32,
}

impl DiscoveryProfile {
    pub fn new(canonical_name: impl Into<String>) -> Self {
        Self {
            canonical_name: canonical_name.into(),
            aliases: Vec::new(),
            concept_refs: Vec::new(),
            intents: Vec::new(),
            high_signal_facets: BTreeMap::new(),
            positive_signals: Vec::new(),
            negative_signals: Vec::new(),
            confusable_with: Vec::new(),
            distinguished_by: Vec::new(),
            projection_version: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryLens {
    pub lens_id: String,
    pub lens_version: u32,
    pub resource_type: ResourceKind,
    pub domain_scope: Option<String>,
    pub source_scope: Option<SourceId>,
    pub identity_fields: Vec<String>,
    pub high_signal_facets: Vec<String>,
    pub searchable_fields: Vec<String>,
    pub applicability_fields: Vec<String>,
    pub temporal_fields: Vec<String>,
    pub relation_fields: Vec<String>,
    pub extraction_policy: Option<String>,
    pub projection_policy: Option<String>,
}
