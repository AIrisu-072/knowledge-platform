//! Rebuildable, source-local projection records and immutable generation identity.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::assertion::Assertion;
use crate::authority::AuthorityResolution;
use crate::id::{ProjectionGenerationId, ResourceId, ResourceVersionId, SourceId};
use crate::observation::Coverage;
use crate::predicate::TypedValue;
use crate::profile::FacetState;
use crate::relation::TypedRelationInstance;
use crate::resource::ResourceKind;
use crate::source::RetentionMode;
use crate::temporal::TemporalDiscoveryProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjectionFamily {
    Directory,
    Structured,
    Lexical,
    Vector,
    Temporal,
    HyperGraph,
    Access,
}

impl ProjectionFamily {
    pub const ALL: [Self; 7] = [
        Self::Directory,
        Self::Structured,
        Self::Lexical,
        Self::Vector,
        Self::Temporal,
        Self::HyperGraph,
        Self::Access,
    ];
}

/// A generation ID is only meaningful together with its owning Source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProjectionGenerationKey {
    pub source_id: SourceId,
    pub generation_id: ProjectionGenerationId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionGenerationManifest {
    pub source_id: SourceId,
    pub generation_id: ProjectionGenerationId,
    pub projection_schema_version: String,
    pub lens_version: u32,
    pub semantic_registry_version: String,
    pub analyzer_version: Option<String>,
    pub embedding_model_version: Option<String>,
    pub graph_schema_version: Option<String>,
    /// Opaque, immutable Source snapshot identifier; never a live cursor.
    pub source_snapshot: String,
    pub resource_count: u64,
    pub relation_count: Option<u64>,
    pub coverage: Coverage,
    pub digest: String,
    pub built_at: OffsetDateTime,
}

impl ProjectionGenerationManifest {
    pub const fn key(&self) -> ProjectionGenerationKey {
        ProjectionGenerationKey {
            source_id: self.source_id,
            generation_id: self.generation_id,
        }
    }
}

/// Lightweight card fields only; source content and full body are outside C1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryProjection {
    pub resource_ref: ResourceId,
    pub resource_version: Option<ResourceVersionId>,
    pub kind: ResourceKind,
    pub canonical_name: String,
    pub title: Option<String>,
    pub aliases: Vec<String>,
}

/// Keep four-state facets and their underlying authority evidence distinct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredProjection {
    pub resource_ref: ResourceId,
    pub concept_refs: Vec<String>,
    pub high_signal_facets: BTreeMap<String, FacetState<String>>,
    pub typed_facets: BTreeMap<String, FacetState<TypedValue>>,
    pub assertions: Vec<Assertion>,
    pub authority_resolutions: BTreeMap<String, AuthorityResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalProjection {
    pub resource_ref: ResourceId,
    pub valid_from: Option<OffsetDateTime>,
    pub valid_to: Option<OffsetDateTime>,
    pub profile: TemporalDiscoveryProfile,
}

/// Prefilter metadata only. Final authorization uses the current Source policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessProjection {
    pub resource_ref: ResourceId,
    pub access_scope: Option<String>,
    pub source_access_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledResourceProjection {
    pub manifest: ProjectionGenerationManifest,
    pub retention_mode: RetentionMode,
    pub directory: DirectoryProjection,
    pub structured: StructuredProjection,
    pub temporal: TemporalProjection,
    pub access: AccessProjection,
    /// Canonical n-ary relations, sorted by RelationId for rebuild equivalence.
    pub relations: Vec<TypedRelationInstance>,
}
