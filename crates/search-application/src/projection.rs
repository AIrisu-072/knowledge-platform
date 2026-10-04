//! Pure projection compilation and publish guards; storage is an adapter concern.

use std::collections::{BTreeMap, BTreeSet};

use search_core::assertion::Assertion;
use search_core::authority::AuthorityResolution;
use search_core::fact::FactSet;
use search_core::id::{ProjectionGenerationId, SourceId};
use search_core::predicate::{
    ConceptResolver, Operand, PredicateEvaluator, PredicateExpr, TruthValue, TypedValue,
};
use search_core::profile::{DiscoveryLens, FacetState};
use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection, ProjectionGenerationKey,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_core::relation::TypedRelationInstance;
use search_core::resource::DiscoverableResource;
use search_core::source::{DiscoverableSource, RetentionMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionInput {
    pub resource: DiscoverableResource,
    pub source: DiscoverableSource,
    pub lens: DiscoveryLens,
    pub source_snapshot: String,
    pub projection_schema_version: String,
    pub semantic_registry_version: String,
    /// Source-supplied display title; not inferred from a body or DSI evidence.
    pub title: Option<String>,
    pub typed_facets: BTreeMap<String, FacetState<TypedValue>>,
    /// Pre-resolution evidence, retained alongside the resolved facet state.
    pub assertions: Vec<Assertion>,
    pub authority_resolutions: BTreeMap<String, AuthorityResolution>,
    /// Resolved instances for `resource.relation_ids`; IDs alone are insufficient.
    pub relations: Vec<TypedRelationInstance>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectionError {
    #[error("projection Source does not match generation Source")]
    SourceMismatch,
    #[error("projection Lens version does not match generation Lens version")]
    LensVersionMismatch,
    #[error("projection Source snapshot does not match generation Source snapshot")]
    SourceSnapshotMismatch,
    #[error("projection schema version does not match generation schema version")]
    SchemaVersionMismatch,
    #[error("projection semantic registry version does not match generation registry version")]
    SemanticRegistryVersionMismatch,
    #[error("projection Lens resource type does not match Resource")]
    LensResourceTypeMismatch,
    #[error("typed facet `{facet}` contradicts authority resolution")]
    InconsistentFacetAuthority { facet: String },
    #[error("high-signal facet `{facet}` contradicts authority resolution")]
    InconsistentHighSignalFacetAuthority { facet: String },
    #[error("high-signal facet `{facet}` contradicts typed facet")]
    InconsistentFacetProjections { facet: String },
    #[error("projection relation is missing, extraneous, or invalid")]
    InvalidRelation,
    #[error("Source retention policy does not permit persistent projection state")]
    PersistenceDenied,
    #[error("generation must be validated before publish")]
    GenerationNotValidated,
    #[error("invalid projection generation state transition")]
    InvalidGenerationTransition,
    #[error("projection generation was not begun")]
    UnknownGeneration,
    #[error("projection generation was already begun")]
    DuplicateGeneration,
}

pub struct ProjectionCompiler;

impl ProjectionCompiler {
    pub fn compile_resource(
        manifest: &ProjectionGenerationManifest,
        input: &ProjectionInput,
    ) -> Result<CompiledResourceProjection, ProjectionError> {
        let identity = &input.resource.identity;
        if identity.source_id != manifest.source_id
            || input.source.source_id != manifest.source_id
            || input
                .lens
                .source_scope
                .is_some_and(|source| source != manifest.source_id)
        {
            return Err(ProjectionError::SourceMismatch);
        }
        if input.lens.lens_version != manifest.lens_version {
            return Err(ProjectionError::LensVersionMismatch);
        }
        if input.source_snapshot != manifest.source_snapshot {
            return Err(ProjectionError::SourceSnapshotMismatch);
        }
        if input.projection_schema_version != manifest.projection_schema_version {
            return Err(ProjectionError::SchemaVersionMismatch);
        }
        if input.semantic_registry_version != manifest.semantic_registry_version {
            return Err(ProjectionError::SemanticRegistryVersionMismatch);
        }
        if input.lens.resource_type != identity.resource_type
            || input.resource.body.kind() != identity.resource_type
        {
            return Err(ProjectionError::LensResourceTypeMismatch);
        }
        for (facet, state) in &input.typed_facets {
            if input
                .authority_resolutions
                .get(facet)
                .is_some_and(|resolution| {
                    !facet_matches_resolution(state, resolution, typed_values_semantically_equal)
                })
            {
                return Err(ProjectionError::InconsistentFacetAuthority {
                    facet: facet.clone(),
                });
            }
        }

        let expected: BTreeSet<_> = input.resource.relation_ids.iter().copied().collect();
        let supplied: BTreeSet<_> = input
            .relations
            .iter()
            .map(|relation| relation.relation_id)
            .collect();
        if expected.len() != input.resource.relation_ids.len()
            || supplied.len() != input.relations.len()
            || expected != supplied
            || input.relations.iter().any(|relation| {
                relation.validate().is_err()
                    || !relation
                        .participants
                        .iter()
                        .any(|participant| participant.resource_ref == identity.resource_id)
            })
        {
            return Err(ProjectionError::InvalidRelation);
        }

        let mut relations = input.relations.clone();
        relations.sort_by_key(|relation| relation.relation_id);
        let selected_high_signal_facets: BTreeMap<_, _> = input
            .lens
            .high_signal_facets
            .iter()
            .filter_map(|facet| {
                input
                    .resource
                    .discovery_profile
                    .high_signal_facets
                    .get(facet)
                    .map(|state| (facet.clone(), state.clone()))
            })
            .collect();
        for (facet, state) in &selected_high_signal_facets {
            if input.authority_resolutions.get(facet).is_some_and(|resolution| {
                !facet_matches_resolution(state, resolution, |signal, resolved| {
                    matches!(resolved, TypedValue::String(value) if signal == value)
                })
            }) {
                return Err(ProjectionError::InconsistentHighSignalFacetAuthority {
                    facet: facet.clone(),
                });
            }
            if input
                .typed_facets
                .get(facet)
                .is_some_and(|typed| !selected_and_typed_facets_agree(state, typed))
            {
                return Err(ProjectionError::InconsistentFacetProjections {
                    facet: facet.clone(),
                });
            }
        }
        Ok(CompiledResourceProjection {
            manifest: manifest.clone(),
            retention_mode: input.source.retention_mode,
            directory: DirectoryProjection {
                resource_ref: identity.resource_id,
                resource_version: identity.resource_version,
                kind: identity.resource_type,
                canonical_name: input.resource.discovery_profile.canonical_name.clone(),
                title: input.title.clone(),
                aliases: input.resource.discovery_profile.aliases.clone(),
            },
            structured: StructuredProjection {
                resource_ref: identity.resource_id,
                concept_refs: input.resource.discovery_profile.concept_refs.clone(),
                high_signal_facets: selected_high_signal_facets,
                typed_facets: input.typed_facets.clone(),
                assertions: input.assertions.clone(),
                authority_resolutions: input.authority_resolutions.clone(),
            },
            temporal: TemporalProjection {
                resource_ref: identity.resource_id,
                valid_from: identity.valid_from,
                valid_to: identity.valid_to,
                profile: input.resource.temporal_profile.clone(),
            },
            access: AccessProjection {
                resource_ref: identity.resource_id,
                access_scope: identity.access_scope.clone(),
                source_access_model: input.source.access_model.clone(),
            },
            relations,
        })
    }
}

struct NoConceptResolution;

impl ConceptResolver for NoConceptResolution {
    fn same_concept(&self, _left: &str, _right: &str) -> TruthValue {
        TruthValue::Error
    }

    fn is_a(&self, _child: &str, _parent: &str) -> TruthValue {
        TruthValue::Error
    }

    fn descendant_of(&self, _child: &str, _ancestor: &str) -> TruthValue {
        TruthValue::Error
    }
}

fn typed_values_semantically_equal(left: &TypedValue, right: &TypedValue) -> bool {
    // Eq uses the same value semantics as authority resolution, including decimal scales.
    PredicateEvaluator::evaluate(
        &PredicateExpr::Eq(Operand::Value(left.clone()), Operand::Value(right.clone())),
        &FactSet::default(),
        &NoConceptResolution,
    ) == TruthValue::True
}

fn facet_matches_resolution<T>(
    state: &FacetState<T>,
    resolution: &AuthorityResolution,
    values_equal: impl FnOnce(&T, &TypedValue) -> bool,
) -> bool {
    // Unresolved has no rankable value: Unknown and structural NotApplicable
    // are both possible, while Known and Conflict require matching evidence.
    match (state, resolution) {
        (FacetState::Known(value), AuthorityResolution::Resolved(resolved)) => {
            values_equal(value, resolved)
        }
        (FacetState::Unknown | FacetState::NotApplicable, AuthorityResolution::Unresolved)
        | (FacetState::Conflict, AuthorityResolution::Conflict(_)) => true,
        _ => false,
    }
}

fn selected_and_typed_facets_agree(
    high_signal: &FacetState<String>,
    typed: &FacetState<TypedValue>,
) -> bool {
    match (high_signal, typed) {
        (FacetState::Known(signal), FacetState::Known(TypedValue::String(value))) => {
            signal == value
        }
        (FacetState::Unknown, FacetState::Unknown)
        | (FacetState::NotApplicable, FacetState::NotApplicable)
        | (FacetState::Conflict, FacetState::Conflict) => true,
        // A display string has no defined conversion from other TypedValue kinds.
        _ => false,
    }
}

fn permits_persistent_state(retention_mode: RetentionMode) -> bool {
    matches!(
        retention_mode,
        RetentionMode::PersistentResource | RetentionMode::PersistentDiscoveryMetadata
    )
}

/// The only input accepted by a persistent generation store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistableGenerationManifest(ProjectionGenerationManifest);

impl PersistableGenerationManifest {
    pub fn manifest(&self) -> &ProjectionGenerationManifest {
        &self.0
    }

    pub fn into_inner(self) -> ProjectionGenerationManifest {
        self.0
    }
}

impl TryFrom<(ProjectionGenerationManifest, &DiscoverableSource)>
    for PersistableGenerationManifest
{
    type Error = ProjectionError;

    fn try_from(
        (manifest, source): (ProjectionGenerationManifest, &DiscoverableSource),
    ) -> Result<Self, Self::Error> {
        if manifest.source_id != source.source_id {
            return Err(ProjectionError::SourceMismatch);
        }
        if !permits_persistent_state(source.retention_mode) {
            return Err(ProjectionError::PersistenceDenied);
        }
        Ok(Self(manifest))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistableResourceProjection(CompiledResourceProjection);

impl PersistableResourceProjection {
    pub fn projection(&self) -> &CompiledResourceProjection {
        &self.0
    }

    pub fn into_inner(self) -> CompiledResourceProjection {
        self.0
    }
}

impl TryFrom<CompiledResourceProjection> for PersistableResourceProjection {
    type Error = ProjectionError;

    fn try_from(projection: CompiledResourceProjection) -> Result<Self, Self::Error> {
        if !permits_persistent_state(projection.retention_mode) {
            return Err(ProjectionError::PersistenceDenied);
        }
        Ok(Self(projection))
    }
}

/// Publish lifecycle is derived application state, never Resource business truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionPublishState {
    Building,
    Validated,
    Current,
    /// Previously published; immutable and still readable by an existing pin.
    Retired,
    Failed,
}

#[derive(Debug, Clone)]
struct GenerationRecord {
    manifest: ProjectionGenerationManifest,
    state: ProjectionPublishState,
}

/// Reusable transition guard; an adapter must make its publish switch atomic.
#[derive(Debug, Clone)]
pub struct GenerationPublication {
    source_id: SourceId,
    records: BTreeMap<ProjectionGenerationId, GenerationRecord>,
    current: Option<ProjectionGenerationId>,
}

impl GenerationPublication {
    pub fn new(source_id: SourceId) -> Self {
        Self {
            source_id,
            records: BTreeMap::new(),
            current: None,
        }
    }

    pub fn begin(&mut self, manifest: ProjectionGenerationManifest) -> Result<(), ProjectionError> {
        if manifest.source_id != self.source_id {
            return Err(ProjectionError::SourceMismatch);
        }
        if self.records.contains_key(&manifest.generation_id) {
            return Err(ProjectionError::DuplicateGeneration);
        }
        self.records.insert(
            manifest.generation_id,
            GenerationRecord {
                manifest,
                state: ProjectionPublishState::Building,
            },
        );
        Ok(())
    }

    pub fn validate(&mut self, key: ProjectionGenerationKey) -> Result<(), ProjectionError> {
        let record = self.record_mut(key)?;
        if record.state != ProjectionPublishState::Building {
            return Err(ProjectionError::InvalidGenerationTransition);
        }
        record.state = ProjectionPublishState::Validated;
        Ok(())
    }

    pub fn publish(&mut self, key: ProjectionGenerationKey) -> Result<(), ProjectionError> {
        let record = self.record_mut(key)?;
        if record.state != ProjectionPublishState::Validated {
            return Err(ProjectionError::GenerationNotValidated);
        }
        if let Some(previous) = self.current {
            self.records
                .get_mut(&previous)
                .expect("current generation must exist")
                .state = ProjectionPublishState::Retired;
        }
        self.records
            .get_mut(&key.generation_id)
            .expect("validated generation must exist")
            .state = ProjectionPublishState::Current;
        self.current = Some(key.generation_id);
        Ok(())
    }

    pub fn fail(&mut self, key: ProjectionGenerationKey) -> Result<(), ProjectionError> {
        let record = self.record_mut(key)?;
        if !matches!(
            record.state,
            ProjectionPublishState::Building | ProjectionPublishState::Validated
        ) {
            return Err(ProjectionError::InvalidGenerationTransition);
        }
        record.state = ProjectionPublishState::Failed;
        Ok(())
    }

    pub fn current(&self) -> Option<ProjectionGenerationKey> {
        self.current.map(|generation_id| ProjectionGenerationKey {
            source_id: self.source_id,
            generation_id,
        })
    }

    /// A copied manifest pins all subsequent reads to the same source-local key.
    pub fn pin_current(&self) -> Option<ProjectionGenerationManifest> {
        self.current
            .and_then(|generation_id| self.records.get(&generation_id))
            .map(|record| record.manifest.clone())
    }

    pub fn state(&self, key: ProjectionGenerationKey) -> Option<ProjectionPublishState> {
        if key.source_id != self.source_id {
            return None;
        }
        self.records
            .get(&key.generation_id)
            .map(|record| record.state)
    }

    fn record_mut(
        &mut self,
        key: ProjectionGenerationKey,
    ) -> Result<&mut GenerationRecord, ProjectionError> {
        if key.source_id != self.source_id {
            return Err(ProjectionError::SourceMismatch);
        }
        self.records
            .get_mut(&key.generation_id)
            .ok_or(ProjectionError::UnknownGeneration)
    }
}
