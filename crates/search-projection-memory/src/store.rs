use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use search_application::SearchError;
use search_application::ports::{
    AssertionStorePort, BoxFuture, ConceptRegistryPort, ProjectionGenerationStore,
    SemanticRegistrySnapshot,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_core::assertion::Assertion;
use search_core::authority::AuthorityResolution;
use search_core::id::{ProjectionGenerationId, RelationId, ResourceId, SourceId};
use search_core::predicate::{ConceptResolver, TruthValue, TypedValue};
use search_core::profile::FacetState;
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::relation::TypedRelationInstance;
use search_core::source::RetentionMode;
use sha2::{Digest, Sha256};

fn invalid(message: impl Into<String>) -> SearchError {
    SearchError::OperationFailed(message.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Building,
    Validated,
    Current,
    Retired,
    Failed,
}

impl Phase {
    fn is_published(self) -> bool {
        matches!(self, Self::Current | Self::Retired)
    }
}

#[derive(Debug, Clone)]
pub(super) struct Segment {
    manifest: ProjectionGenerationManifest,
    phase: Phase,
    pub(super) resources: BTreeMap<ResourceId, CompiledResourceProjection>,
    registry: Option<SemanticRegistrySnapshot>,
    /// Only a replacement of a carried resource may share its ID with the
    /// initial incremental map; two stage calls with one ID are an error.
    staged_ids: BTreeSet<ResourceId>,
    retired_ids: BTreeSet<ResourceId>,
}

#[derive(Debug, Default)]
struct SourceState {
    generations: BTreeMap<ProjectionGenerationId, Segment>,
    current: Option<ProjectionGenerationId>,
}

#[derive(Debug, Default)]
pub(super) struct State {
    sources: BTreeMap<SourceId, SourceState>,
}

/// A lock covers both segment validation state and the source-local current
/// pointer. Published segments are never mutated or discarded by this store.
#[derive(Debug, Clone, Default)]
pub struct MemoryProjectionStore {
    state: Arc<RwLock<State>>,
}

impl MemoryProjectionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) fn read(&self) -> Result<std::sync::RwLockReadGuard<'_, State>, SearchError> {
        self.state
            .read()
            .map_err(|_| invalid("projection store lock poisoned"))
    }

    fn write(&self) -> Result<std::sync::RwLockWriteGuard<'_, State>, SearchError> {
        self.state
            .write()
            .map_err(|_| invalid("projection store lock poisoned"))
    }

    pub(super) fn published(
        state: &State,
        key: ProjectionGenerationKey,
    ) -> Result<&Segment, SearchError> {
        let segment = state
            .sources
            .get(&key.source_id)
            .and_then(|source| source.generations.get(&key.generation_id))
            .ok_or_else(|| invalid("unknown projection generation"))?;
        if !segment.phase.is_published() {
            return Err(invalid("projection generation is not published"));
        }
        Ok(segment)
    }

    /// Atomically publish a validated generation only if the Source current
    /// pointer is still the one observed before an authoritative read began.
    /// A stale builder must not rewind a newer generation.
    pub fn publish_generation_if_current(
        &self,
        key: ProjectionGenerationKey,
        expected_current: Option<ProjectionGenerationKey>,
    ) -> Result<bool, SearchError> {
        if expected_current.is_some_and(|expected| expected.source_id != key.source_id) {
            return Err(invalid("expected current belongs to another source"));
        }
        let mut state = self.write()?;
        let source = state
            .sources
            .get_mut(&key.source_id)
            .ok_or_else(|| invalid("unknown projection source"))?;
        if source.current != expected_current.map(|expected| expected.generation_id) {
            return Ok(false);
        }
        if source
            .generations
            .get(&key.generation_id)
            .map(|segment| segment.phase)
            != Some(Phase::Validated)
        {
            return Err(invalid("generation is not validated"));
        }
        if let Some(previous) = source.current {
            source
                .generations
                .get_mut(&previous)
                .expect("current generation must exist")
                .phase = Phase::Retired;
        }
        source
            .generations
            .get_mut(&key.generation_id)
            .expect("validated generation must exist")
            .phase = Phase::Current;
        source.current = Some(key.generation_id);
        Ok(true)
    }
}

fn base_compatible(
    base: &ProjectionGenerationManifest,
    next: &ProjectionGenerationManifest,
) -> bool {
    base.source_id == next.source_id
        && base.projection_schema_version == next.projection_schema_version
        && base.lens_version == next.lens_version
        && base.semantic_registry_version == next.semantic_registry_version
        && base.analyzer_version == next.analyzer_version
        && base.embedding_model_version == next.embedding_model_version
        && base.graph_schema_version == next.graph_schema_version
        && base.coverage == next.coverage
}

fn validate_registry(registry: &SemanticRegistrySnapshot) -> Result<(), SearchError> {
    if registry.version.is_empty() || registry.concepts.iter().any(String::is_empty) {
        return Err(invalid(
            "semantic registry contains an empty version or concept",
        ));
    }
    for (left, right) in registry.synonyms.iter().chain(registry.is_a.iter()) {
        if !registry.concepts.contains(left) || !registry.concepts.contains(right) {
            return Err(invalid("semantic edge refers to an unknown concept"));
        }
    }
    Ok(())
}

fn validate_segment(segment: &Segment) -> Result<(), SearchError> {
    if segment.resources.len() as u64 != segment.manifest.resource_count {
        return Err(invalid("projection resource count does not match manifest"));
    }
    let registry = segment
        .registry
        .as_ref()
        .ok_or_else(|| invalid("semantic registry was not staged"))?;
    validate_registry(registry)?;
    if registry.version != segment.manifest.semantic_registry_version {
        return Err(invalid("semantic registry version does not match manifest"));
    }
    let mut all_relations: BTreeMap<RelationId, &TypedRelationInstance> = BTreeMap::new();
    for (&id, projection) in &segment.resources {
        if projection.manifest != segment.manifest
            || projection.directory.resource_ref != id
            || projection.structured.resource_ref != id
            || projection.temporal.resource_ref != id
            || projection.access.resource_ref != id
        {
            return Err(invalid("projection resource identity or manifest mismatch"));
        }
        if !matches!(
            projection.retention_mode,
            RetentionMode::PersistentResource | RetentionMode::PersistentDiscoveryMetadata
        ) {
            return Err(invalid(
                "projection retention mode forbids persistent state",
            ));
        }
        let mut local_relations = BTreeSet::new();
        for relation in &projection.relations {
            if relation
                .participants
                .iter()
                .any(|participant| segment.retired_ids.contains(&participant.resource_ref))
            {
                return Err(invalid("projection relation references retired resource"));
            }
            if relation.validate().is_err()
                || !relation
                    .participants
                    .iter()
                    .any(|participant| participant.resource_ref == id)
                || !local_relations.insert(relation.relation_id)
            {
                return Err(invalid("invalid or duplicate projection relation"));
            }
            if let Some(existing) = all_relations.insert(relation.relation_id, relation)
                && canonical_relation(existing)? != canonical_relation(relation)?
            {
                return Err(invalid("relation ID has conflicting definitions"));
            }
        }
    }
    if segment
        .manifest
        .relation_count
        .is_some_and(|count| count != all_relations.len() as u64)
    {
        return Err(invalid("projection relation count does not match manifest"));
    }
    let resources: Vec<_> = segment.resources.values().cloned().collect();
    let actual_digest = generation_digest(segment.manifest.source_id, &resources, registry)?;
    if actual_digest != segment.manifest.digest {
        return Err(invalid("projection digest does not match manifest"));
    }
    Ok(())
}

/// SHA-256 canonicalization v1: prefix and source UUID, then length-prefixed
/// JSON of registry (sorted known concepts, normalized undirected synonyms,
/// directed IS_A), then resources ordered by ResourceId. Each resource hashes
/// retention, Directory, Structured, Temporal, Access and relations sorted by
/// RelationId, with each relation's participants sorted by role and ResourceId.
/// The embedded generation manifest, including its digest,
/// generation ID, snapshot and build time, is deliberately excluded: a full
/// and incremental build of the same logical projection have one digest.
/// Collection fields with set semantics are sorted without changing ordered
/// TypedValue::List contents. Staging applies the same normalization to the
/// published payload.
pub fn generation_digest(
    source_id: SourceId,
    resources: &[CompiledResourceProjection],
    registry: &SemanticRegistrySnapshot,
) -> Result<String, SearchError> {
    validate_registry(registry)?;
    let registry = canonical_registry(registry);
    let mut by_id = BTreeMap::new();
    for projection in resources {
        if projection.manifest.source_id != source_id {
            return Err(invalid("projection source does not match digest source"));
        }
        let projection = canonical_projection(projection)?;
        if by_id
            .insert(projection.directory.resource_ref, projection)
            .is_some()
        {
            return Err(invalid("duplicate resource in projection digest"));
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(b"search-projection-memory:v1\n");
    hasher.update(source_id.as_uuid().as_bytes());
    let registry_bytes = serde_json::to_vec(&(
        &registry.version,
        &registry.concepts,
        &registry.synonyms,
        &registry.is_a,
    ))
    .map_err(|error| invalid(format!("semantic registry serialization failed: {error}")))?;
    hash_frame(&mut hasher, &registry_bytes);
    for (id, projection) in by_id {
        hasher.update(id.as_uuid().as_bytes());
        let bytes = serde_json::to_vec(&(
            &projection.retention_mode,
            &projection.directory,
            &projection.structured,
            &projection.temporal,
            &projection.access,
            &projection.relations,
        ))
        .map_err(|error| invalid(format!("projection serialization failed: {error}")))?;
        hash_frame(&mut hasher, &bytes);
    }
    let mut digest = String::from("sha256:");
    for byte in hasher.finalize() {
        use std::fmt::Write;
        write!(&mut digest, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(digest)
}

fn canonical_registry(registry: &SemanticRegistrySnapshot) -> SemanticRegistrySnapshot {
    let mut canonical = registry.clone();
    canonical.synonyms = registry
        .synonyms
        .iter()
        .map(|(left, right)| {
            if left <= right {
                (left.clone(), right.clone())
            } else {
                (right.clone(), left.clone())
            }
        })
        .collect();
    canonical
}

fn canonical_typed_value(value: &mut TypedValue) -> Result<(), SearchError> {
    match value {
        TypedValue::List(values) => {
            for member in values.iter_mut() {
                canonical_typed_value(member)?;
            }
        }
        TypedValue::Set(values) => {
            for member in values.iter_mut() {
                canonical_typed_value(member)?;
            }
            sort_typed_values(values)?;
        }
        _ => {}
    }
    Ok(())
}

fn sort_typed_values(values: &mut Vec<TypedValue>) -> Result<(), SearchError> {
    let mut keyed = Vec::with_capacity(values.len());
    for value in std::mem::take(values) {
        let key = serde_json::to_vec(&value)
            .map_err(|error| invalid(format!("typed value serialization failed: {error}")))?;
        keyed.push((key, value));
    }
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    *values = keyed.into_iter().map(|(_, value)| value).collect();
    Ok(())
}

fn canonical_relation(
    relation: &TypedRelationInstance,
) -> Result<TypedRelationInstance, SearchError> {
    let mut relation = relation.clone();
    relation.participants.sort();
    relation.evidence_refs.sort();
    for value in relation.qualifiers.values_mut() {
        canonical_typed_value(value)?;
    }
    Ok(relation)
}

fn canonical_projection(
    projection: &CompiledResourceProjection,
) -> Result<CompiledResourceProjection, SearchError> {
    let mut projection = projection.clone();
    projection.directory.aliases.sort();
    projection.structured.concept_refs.sort();
    for facet in projection.structured.typed_facets.values_mut() {
        if let FacetState::Known(value) = facet {
            canonical_typed_value(value)?;
        }
    }
    for assertion in &mut projection.structured.assertions {
        canonical_typed_value(&mut assertion.value)?;
        assertion.evidence_refs.sort();
    }
    let mut assertions = Vec::with_capacity(projection.structured.assertions.len());
    for assertion in std::mem::take(&mut projection.structured.assertions) {
        let key = serde_json::to_vec(&assertion)
            .map_err(|error| invalid(format!("assertion serialization failed: {error}")))?;
        assertions.push((key, assertion));
    }
    assertions.sort_by(|left, right| left.0.cmp(&right.0));
    projection.structured.assertions = assertions
        .into_iter()
        .map(|(_, assertion)| assertion)
        .collect();
    for resolution in projection.structured.authority_resolutions.values_mut() {
        match resolution {
            AuthorityResolution::Resolved(value) => canonical_typed_value(value)?,
            AuthorityResolution::Conflict(conflict) => {
                for value in &mut conflict.values {
                    canonical_typed_value(value)?;
                }
                sort_typed_values(&mut conflict.values)?;
            }
            AuthorityResolution::Unresolved => {}
        }
    }
    projection.relations = projection
        .relations
        .iter()
        .map(canonical_relation)
        .collect::<Result<Vec<_>, _>>()?;
    projection
        .relations
        .sort_by_key(|relation| relation.relation_id);
    Ok(projection)
}

fn hash_frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

impl ProjectionGenerationStore for MemoryProjectionStore {
    fn begin_generation<'a>(&'a self, input: PersistableGenerationManifest) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let manifest = input.into_inner();
            let mut state = self.write()?;
            let source = state.sources.entry(manifest.source_id).or_default();
            if source.generations.contains_key(&manifest.generation_id) {
                return Err(invalid("projection generation already exists"));
            }
            source.generations.insert(
                manifest.generation_id,
                Segment {
                    manifest,
                    phase: Phase::Building,
                    resources: BTreeMap::new(),
                    registry: None,
                    staged_ids: BTreeSet::new(),
                    retired_ids: BTreeSet::new(),
                },
            );
            Ok(())
        })
    }

    fn begin_incremental_generation<'a>(
        &'a self,
        input: PersistableGenerationManifest,
        base: ProjectionGenerationKey,
        retired: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let manifest = input.into_inner();
            if base.source_id != manifest.source_id {
                return Err(invalid("incremental base belongs to another source"));
            }
            let mut state = self.write()?;
            let source = state
                .sources
                .get_mut(&manifest.source_id)
                .ok_or_else(|| invalid("incremental base source is unknown"))?;
            if source.generations.contains_key(&manifest.generation_id) {
                return Err(invalid("projection generation already exists"));
            }
            let base_segment = source
                .generations
                .get(&base.generation_id)
                .ok_or_else(|| invalid("incremental base generation is unknown"))?;
            if !base_segment.phase.is_published()
                || !base_compatible(&base_segment.manifest, &manifest)
            {
                return Err(invalid(
                    "incremental base is not published or version-compatible",
                ));
            }
            if !retired
                .iter()
                .all(|id| base_segment.resources.contains_key(id))
            {
                return Err(invalid("retired resource is absent from incremental base"));
            }
            let mut resources = base_segment.resources.clone();
            for id in &retired {
                resources.remove(id);
            }
            for projection in resources.values_mut() {
                projection.manifest = manifest.clone();
            }
            source.generations.insert(
                manifest.generation_id,
                Segment {
                    manifest,
                    phase: Phase::Building,
                    resources,
                    registry: base_segment.registry.clone(),
                    staged_ids: BTreeSet::new(),
                    retired_ids: retired,
                },
            );
            Ok(())
        })
    }

    fn stage_resource<'a>(&'a self, input: PersistableResourceProjection) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let projection = canonical_projection(&input.into_inner())?;
            let key = projection.manifest.key();
            let id = projection.directory.resource_ref;
            let mut state = self.write()?;
            let segment = state
                .sources
                .get_mut(&key.source_id)
                .and_then(|source| source.generations.get_mut(&key.generation_id))
                .ok_or_else(|| invalid("unknown projection generation"))?;
            if segment.phase != Phase::Building {
                return Err(invalid("generation staging is frozen"));
            }
            if projection.manifest != segment.manifest {
                return Err(invalid("projection manifest mismatch"));
            }
            if segment.retired_ids.contains(&id) {
                return Err(invalid("retired resource cannot be restaged"));
            }
            if projection.structured.resource_ref != id
                || projection.temporal.resource_ref != id
                || projection.access.resource_ref != id
            {
                return Err(invalid("projection resource identity mismatch"));
            }
            if !matches!(
                projection.retention_mode,
                RetentionMode::PersistentResource | RetentionMode::PersistentDiscoveryMetadata
            ) {
                return Err(invalid(
                    "projection retention mode forbids persistent state",
                ));
            }
            if !segment.staged_ids.insert(id) {
                return Err(invalid("resource was staged twice"));
            }
            segment.resources.insert(id, projection);
            Ok(())
        })
    }

    fn stage_concept_registry<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        registry: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            validate_registry(&registry)?;
            let registry = canonical_registry(&registry);
            let mut state = self.write()?;
            let source = state
                .sources
                .get_mut(&key.source_id)
                .ok_or_else(|| invalid("unknown projection source"))?;
            let segment = source
                .generations
                .get(&key.generation_id)
                .ok_or_else(|| invalid("unknown projection generation"))?;
            if segment.phase != Phase::Building {
                return Err(invalid("generation staging is frozen"));
            }
            if registry.version != segment.manifest.semantic_registry_version {
                return Err(invalid("semantic registry version mismatch"));
            }
            if segment.registry.is_some() {
                return Err(invalid("semantic registry already staged or inherited"));
            }
            if source.generations.values().any(|existing| {
                existing.phase != Phase::Failed
                    && existing.registry.as_ref().is_some_and(|staged| {
                        staged.version == registry.version && staged != &registry
                    })
            }) {
                return Err(invalid("semantic registry version has different contents"));
            }
            source
                .generations
                .get_mut(&key.generation_id)
                .expect("checked generation must exist")
                .registry = Some(registry);
            Ok(())
        })
    }

    fn validate_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.write()?;
            let segment = state
                .sources
                .get_mut(&key.source_id)
                .and_then(|source| source.generations.get_mut(&key.generation_id))
                .ok_or_else(|| invalid("unknown projection generation"))?;
            if segment.phase != Phase::Building {
                return Err(invalid("only building generations can be validated"));
            }
            match validate_segment(segment) {
                Ok(()) => {
                    segment.phase = Phase::Validated;
                    Ok(())
                }
                Err(error) => {
                    segment.phase = Phase::Failed;
                    Err(error)
                }
            }
        })
    }

    fn publish_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.write()?;
            let source = state
                .sources
                .get_mut(&key.source_id)
                .ok_or_else(|| invalid("unknown projection source"))?;
            if source
                .generations
                .get(&key.generation_id)
                .map(|segment| segment.phase)
                != Some(Phase::Validated)
            {
                return Err(invalid("generation is not validated"));
            }
            if let Some(previous) = source.current {
                source
                    .generations
                    .get_mut(&previous)
                    .expect("current generation must exist")
                    .phase = Phase::Retired;
            }
            source
                .generations
                .get_mut(&key.generation_id)
                .expect("validated generation must exist")
                .phase = Phase::Current;
            source.current = Some(key.generation_id);
            Ok(())
        })
    }

    fn fail_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.write()?;
            let segment = state
                .sources
                .get_mut(&key.source_id)
                .and_then(|source| source.generations.get_mut(&key.generation_id))
                .ok_or_else(|| invalid("unknown projection generation"))?;
            if !matches!(segment.phase, Phase::Building | Phase::Validated) {
                return Err(invalid("published or failed generation cannot be failed"));
            }
            segment.phase = Phase::Failed;
            Ok(())
        })
    }

    fn pin_current<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        Box::pin(async move {
            let state = self.read()?;
            Ok(state
                .sources
                .get(&source_id)
                .and_then(|source| source.current.and_then(|id| source.generations.get(&id)))
                .map(|segment| segment.manifest.clone()))
        })
    }

    fn resource_at<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        resource_id: ResourceId,
    ) -> BoxFuture<'a, Option<CompiledResourceProjection>> {
        Box::pin(async move {
            let state = self.read()?;
            Ok(Self::published(&state, key)?
                .resources
                .get(&resource_id)
                .cloned())
        })
    }
}

impl AssertionStorePort for MemoryProjectionStore {
    fn assertions_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        predicate: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        Box::pin(async move {
            let state = self.read()?;
            let segment = Self::published(&state, generation)?;
            Ok(segment
                .resources
                .get(&resource_ref)
                .map(|resource| {
                    resource
                        .structured
                        .assertions
                        .iter()
                        .filter(|assertion| assertion.predicate == predicate)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default())
        })
    }
}

impl ConceptRegistryPort for MemoryProjectionStore {
    fn pin_view<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Arc<dyn ConceptResolver + Send + Sync>> {
        Box::pin(async move {
            let state = self.read()?;
            let registry = Self::published(&state, generation)?
                .registry
                .as_ref()
                .ok_or_else(|| invalid("published generation has no semantic registry"))?;
            Ok(Arc::new(PinnedRegistry {
                registry: registry.clone(),
            }) as Arc<dyn ConceptResolver + Send + Sync>)
        })
    }

    fn same_concept<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        left: &'a str,
        right: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        Box::pin(async move {
            let state = self.read()?;
            let registry = Self::published(&state, generation)?
                .registry
                .as_ref()
                .ok_or_else(|| invalid("published generation has no semantic registry"))?;
            Ok(concept_reachable(registry, left, right, false))
        })
    }

    fn is_a<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        child: &'a str,
        parent: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        Box::pin(async move {
            let state = self.read()?;
            let registry = Self::published(&state, generation)?
                .registry
                .as_ref()
                .ok_or_else(|| invalid("published generation has no semantic registry"))?;
            Ok(concept_reachable(registry, child, parent, true))
        })
    }
}

struct PinnedRegistry {
    registry: SemanticRegistrySnapshot,
}

impl ConceptResolver for PinnedRegistry {
    fn same_concept(&self, left: &str, right: &str) -> TruthValue {
        concept_reachable(&self.registry, left, right, false)
    }

    fn is_a(&self, child: &str, parent: &str) -> TruthValue {
        concept_reachable(&self.registry, child, parent, true)
    }

    fn descendant_of(&self, child: &str, ancestor: &str) -> TruthValue {
        concept_reachable(&self.registry, child, ancestor, true)
    }
}

fn concept_reachable(
    registry: &SemanticRegistrySnapshot,
    start: &str,
    target: &str,
    hierarchy: bool,
) -> TruthValue {
    if !registry.concepts.contains(start) || !registry.concepts.contains(target) {
        return TruthValue::Unknown;
    }
    let mut visited = BTreeSet::from([start]);
    let mut frontier = vec![start];
    while let Some(node) = frontier.pop() {
        if node == target {
            return TruthValue::True;
        }
        for (left, right) in &registry.synonyms {
            let next = if left == node {
                Some(right.as_str())
            } else if right == node {
                Some(left.as_str())
            } else {
                None
            };
            if let Some(next) = next
                && visited.insert(next)
            {
                frontier.push(next);
            }
        }
        if hierarchy {
            for (child, parent) in &registry.is_a {
                if child == node && visited.insert(parent.as_str()) {
                    frontier.push(parent);
                }
            }
        }
    }
    TruthValue::False
}
