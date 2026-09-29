use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use search_application::ports::CurrentAccessEvaluatorPort;
use search_core::id::{RelationId, ResourceId};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
    TemporalProjection,
};
use search_core::relation::TypedRelationInstance;
use search_core::source::{DiscoverableSource, RetentionMode};

#[derive(Debug, thiserror::Error)]
pub enum GraphIndexError {
    #[error("graph generation belongs to another Source")]
    SourceMismatch,
    #[error("graph schema version is unsupported")]
    UnsupportedSchemaVersion,
    #[error("Source retention forbids persistent graph state")]
    PersistenceDenied,
    #[error("compiled projection does not match graph manifest or Resource")]
    ProjectionMismatch,
    #[error("graph resource count does not match generation manifest")]
    ResourceCountMismatch,
    #[error("graph relation count does not match generation manifest")]
    RelationCountMismatch,
    #[error("graph relation is invalid or conflicts with the same relation ID")]
    InvalidRelation,
    #[error("graph generation already exists")]
    DuplicateGeneration,
    #[error("graph index lock is poisoned")]
    LockPoisoned,
}

#[derive(Clone)]
pub(crate) struct ResourceMetadata {
    pub temporal: TemporalProjection,
}

pub(crate) struct GenerationIndex {
    pub resources: BTreeMap<ResourceId, ResourceMetadata>,
    pub relations: BTreeMap<RelationId, TypedRelationInstance>,
    pub incidence: BTreeMap<(ResourceId, String), Vec<RelationId>>,
}

pub struct MemoryGraphRetriever {
    pub(crate) generations: RwLock<BTreeMap<ProjectionGenerationKey, Arc<GenerationIndex>>>,
    pub(crate) access: Arc<dyn CurrentAccessEvaluatorPort>,
}

impl MemoryGraphRetriever {
    pub fn new(access: Arc<dyn CurrentAccessEvaluatorPort>) -> Self {
        Self {
            generations: RwLock::new(BTreeMap::new()),
            access,
        }
    }

    /// Build a whole, immutable Source generation before making it visible.
    /// A later generation never mutates one already pinned by an evaluation.
    pub fn build_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        projections: Vec<CompiledResourceProjection>,
    ) -> Result<(), GraphIndexError> {
        let key = manifest.key();
        if source.source_id != key.source_id {
            return Err(GraphIndexError::SourceMismatch);
        }
        if !matches!(
            source.retention_mode,
            RetentionMode::PersistentResource | RetentionMode::PersistentDiscoveryMetadata
        ) {
            return Err(GraphIndexError::PersistenceDenied);
        }
        if manifest.graph_schema_version.as_deref() != Some("typed-nary-v1") {
            return Err(GraphIndexError::UnsupportedSchemaVersion);
        }
        if projections.len() as u64 != manifest.resource_count {
            return Err(GraphIndexError::ResourceCountMismatch);
        }
        if self
            .generations
            .read()
            .map_err(|_| GraphIndexError::LockPoisoned)?
            .contains_key(&key)
        {
            return Err(GraphIndexError::DuplicateGeneration);
        }

        let mut resources = BTreeMap::new();
        let mut relations = BTreeMap::new();
        for projection in projections {
            let id = projection.directory.resource_ref;
            if projection.manifest != manifest
                || projection.structured.resource_ref != id
                || projection.temporal.resource_ref != id
                || projection.access.resource_ref != id
                || !matches!(
                    projection.retention_mode,
                    RetentionMode::PersistentResource | RetentionMode::PersistentDiscoveryMetadata
                )
            {
                return Err(GraphIndexError::ProjectionMismatch);
            }
            if resources
                .insert(
                    id,
                    ResourceMetadata {
                        temporal: projection.temporal,
                    },
                )
                .is_some()
            {
                return Err(GraphIndexError::ProjectionMismatch);
            }
            let mut local_ids = BTreeSet::new();
            for mut relation in projection.relations {
                if relation.validate().is_err()
                    || !relation
                        .participants
                        .iter()
                        .any(|participant| participant.resource_ref == id)
                    || !local_ids.insert(relation.relation_id)
                {
                    return Err(GraphIndexError::InvalidRelation);
                }
                relation.participants.sort();
                if relation
                    .participants
                    .windows(2)
                    .any(|pair| pair[0] == pair[1])
                {
                    return Err(GraphIndexError::InvalidRelation);
                }
                relation.evidence_refs.sort();
                relation.evidence_refs.dedup();
                if let Some(existing) = relations.insert(relation.relation_id, relation.clone())
                    && existing != relation
                {
                    return Err(GraphIndexError::InvalidRelation);
                }
            }
        }
        if manifest
            .relation_count
            .is_some_and(|count| count != relations.len() as u64)
        {
            return Err(GraphIndexError::RelationCountMismatch);
        }
        let mut incidence: BTreeMap<(ResourceId, String), Vec<RelationId>> = BTreeMap::new();
        for (&relation_id, relation) in &relations {
            for participant in &relation.participants {
                incidence
                    .entry((participant.resource_ref, participant.role.clone()))
                    .or_default()
                    .push(relation_id);
            }
        }
        let generation = Arc::new(GenerationIndex {
            resources,
            relations,
            incidence,
        });
        let mut generations = self
            .generations
            .write()
            .map_err(|_| GraphIndexError::LockPoisoned)?;
        match generations.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(generation);
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                Err(GraphIndexError::DuplicateGeneration)
            }
        }
    }
}
