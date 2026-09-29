//! Backend-neutral, asynchronous ports for Source-owned reads and derived Search state.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use search_core::assertion::Assertion;
use search_core::binding::RepresentationBinding;
use search_core::discovery::{DiscoveryRequest, FederatedCandidate};
use search_core::fact::FactSet;
use search_core::graph::GraphTraversalPlan;
use search_core::id::{ResourceId, SourceId};
use search_core::predicate::{ConceptResolver, TruthValue, TypedValue};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::source::DiscoverableSource;

use crate::error::SearchError;
use crate::projection::{PersistableGenerationManifest, PersistableResourceProjection};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SearchError>> + Send + 'a>>;

pub trait SourceRegistryPort: Send + Sync {
    fn get_source<'a>(&'a self, source_id: SourceId) -> BoxFuture<'a, Option<DiscoverableSource>>;
    fn list_sources<'a>(&'a self) -> BoxFuture<'a, Vec<DiscoverableSource>>;
}

pub trait AssertionStorePort: Send + Sync {
    fn assertions_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        predicate: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>>;
}

pub trait ConceptRegistryPort: Send + Sync {
    /// An evaluation keeps this immutable synchronous view for predicate
    /// evaluation, even after the current generation advances.
    fn pin_view<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Arc<dyn ConceptResolver + Send + Sync>>;

    fn same_concept<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        left: &'a str,
        right: &'a str,
    ) -> BoxFuture<'a, TruthValue>;
    fn is_a<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        child: &'a str,
        parent: &'a str,
    ) -> BoxFuture<'a, TruthValue>;
}

/// Concept edges are staged with the generation, rather than expanded into
/// each resource. Edges must refer to explicitly known concepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRegistrySnapshot {
    pub version: String,
    pub concepts: BTreeSet<String>,
    /// Undirected equivalence edges.
    pub synonyms: BTreeSet<(String, String)>,
    /// Directed (child, parent) edges.
    pub is_a: BTreeSet<(String, String)>,
}

impl SemanticRegistrySnapshot {
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            concepts: BTreeSet::new(),
            synonyms: BTreeSet::new(),
            is_a: BTreeSet::new(),
        }
    }
}

/// A source-local generation is immutable after validation. Publishing must
/// atomically switch the current key; previous keys remain readable for pinned
/// evaluations. Unvalidated or failed keys must never become current.
pub trait ProjectionGenerationStore: Send + Sync {
    fn begin_generation<'a>(&'a self, manifest: PersistableGenerationManifest)
    -> BoxFuture<'a, ()>;

    /// Copy an already published, compatible source-local generation, then
    /// replace changed resources and remove retired IDs before validation.
    fn begin_incremental_generation<'a>(
        &'a self,
        manifest: PersistableGenerationManifest,
        base: ProjectionGenerationKey,
        retired: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()>;

    fn stage_resource<'a>(&'a self, projection: PersistableResourceProjection)
    -> BoxFuture<'a, ()>;

    fn stage_concept_registry<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        registry: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()>;

    fn validate_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()>;
    fn publish_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()>;
    fn fail_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()>;

    fn pin_current<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>>;

    fn resource_at<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        resource_id: ResourceId,
    ) -> BoxFuture<'a, Option<CompiledResourceProjection>>;
}

pub trait DirectoryRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

pub trait StructuredRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        hard_filters: &'a [StructuredFacetFilter],
    ) -> BoxFuture<'a, Vec<StructuredRetrievalHit>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredFacetFilter {
    pub facet: String,
    pub expected: TypedValue,
}

impl StructuredFacetFilter {
    pub fn eq(facet: impl Into<String>, expected: TypedValue) -> Self {
        Self {
            facet: facet.into(),
            expected,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredFacetOutcome {
    Match,
    Mismatch,
    Unknown,
    NotApplicable,
    Conflict,
}

/// A hard mismatch is explicit; UNKNOWN/NA/CONFLICT remain available for
/// targeted qualification and must not be silently filtered out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredRetrievalHit {
    pub candidate: FederatedCandidate,
    pub outcomes: Vec<StructuredFacetOutcome>,
}

pub trait LexicalRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

pub trait VectorRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

pub trait HyperGraphRetrieverPort: Send + Sync {
    fn retrieve<'a>(
        &'a self,
        plan: &'a GraphTraversalPlan,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

pub trait ProbePort: Send + Sync {
    fn probe<'a>(&'a self, candidate: &'a FederatedCandidate) -> BoxFuture<'a, FactSet>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    Allowed,
    Denied,
    Unknown,
}

pub trait CurrentAccessEvaluatorPort: Send + Sync {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializationReceipt {
    pub resource_ref: ResourceId,
    pub content_digest: Option<String>,
    pub locator: Option<String>,
}

pub trait MaterializerPort: Send + Sync {
    fn materialize<'a>(
        &'a self,
        binding: &'a RepresentationBinding,
        access_context: &'a str,
    ) -> BoxFuture<'a, MaterializationReceipt>;
}
