//! Backend-neutral, asynchronous ports for Source-owned reads and derived Search state.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use search_core::assertion::Assertion;
use search_core::binding::RepresentationBinding;
use search_core::discovery::{DiscoveryRequest, FederatedCandidate};
use search_core::evidence::EvidenceRole;
use search_core::graph::{GraphPathEvidence, GraphTraversalPlan};
use search_core::id::{ClaimId, ResourceId, SourceId};
use search_core::materialization::{MaterializationState, ProviderContentPermission};
use search_core::predicate::{ConceptResolver, TruthValue, TypedValue};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::resource::ResourceKind;
use search_core::source::DiscoverableSource;
use search_core::source::RetentionMode;

use crate::error::SearchError;
use crate::materialization::{
    MaterializationBudget, ProbeCapability, ProbeRequest, ProbeResult, ResourceCostEstimate,
};
use crate::projection::{PersistableGenerationManifest, PersistableResourceProjection};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SearchError>> + Send + 'a>>;

#[path = "evidence_resolution.rs"]
mod evidence_resolution;
pub use evidence_resolution::{assemble_resource_claims, assess_claim_evidence};

#[path = "probe_execution.rs"]
mod probe_execution;
pub use probe_execution::{ProbeExecutionInput, ProbeExecutionService};

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

/// A trusted ClaimId lookup, pinned to the same generation as Assertion reads.
/// DiscoveryRequest carries only IDs and cannot supply or reinterpret selectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimSelector {
    pub claim_id: ClaimId,
    pub subject_ref: String,
    pub predicate: String,
    pub expected_value: Option<TypedValue>,
}

pub trait ClaimSelectorPort: Send + Sync {
    fn selector_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>>;
}

/// Provenance resolved by the owning Source adapter from an opaque Assertion ref.
/// The application checks every identity field before it becomes Claim evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssertionEvidence {
    pub generation: ProjectionGenerationKey,
    pub source_id: SourceId,
    pub resource_id: ResourceId,
    pub evidence_ref: String,
    pub upstream_origin: String,
    pub role: EvidenceRole,
    pub citation_chain: Vec<String>,
    pub content_digest: Option<String>,
    pub is_summary: bool,
}

pub trait EvidenceResolverPort: Send + Sync {
    fn resolve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>>;
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
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>>;
}

/// Explicit lexical input; DiscoveryRequest is an intent/evidence request,
/// not a free-text query. The list limit is a retrieval window, not a
/// Discovery completion condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalQuery {
    pub text: String,
    pub limit: usize,
}

impl LexicalQuery {
    pub fn new(text: impl Into<String>, limit: usize) -> Self {
        Self {
            text: text.into(),
            limit,
        }
    }
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
        generation: ProjectionGenerationKey,
        plan: &'a GraphTraversalPlan,
    ) -> BoxFuture<'a, GraphRetrievalResult>;
}

/// The generation and all relation-local path evidence travel with candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRetrievalResult {
    pub generation: ProjectionGenerationKey,
    pub hits: Vec<GraphRetrievalHit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRetrievalHit {
    pub candidate: FederatedCandidate,
    pub paths: Vec<GraphPathEvidence>,
}

/// The Source adapter must revalidate candidate access, current capability,
/// provider grant, and execution budget at call time after application preflight.
pub trait ProbePort: Send + Sync {
    fn probe<'a>(&'a self, request: &'a ProbeRequest) -> BoxFuture<'a, ProbeResult>;
}

/// A current Source-owned capability bound to the exact candidate and facet.
/// It is never copied from a retrieval projection or public Discovery request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentProbeCapability {
    pub candidate_id: String,
    pub resource_ref: Option<ResourceId>,
    pub facet: String,
    pub capability: ProbeCapability,
}

pub trait ProbeCapabilityCatalogPort: Send + Sync {
    fn for_candidate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        facet: &'a str,
        access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentProbeCapability>>;
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

/// Evaluates SourceId plus the full candidate identity, including remote
/// candidates with no ResourceId. Unknown must fail closed at the caller.
pub trait CurrentCandidateAccessEvaluatorPort: Send + Sync {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision>;
}

/// A current, target-specific grant from the authoritative Source boundary.
/// The caller must not construct this from the public request or a projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentSourcePolicy {
    pub resource_kind: ResourceKind,
    pub provider_permission: ProviderContentPermission,
    pub retention_mode: RetentionMode,
    pub probe_allowed: bool,
}

pub trait CurrentSourcePolicyPort: Send + Sync {
    fn for_candidate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>>;

    fn for_resource<'a>(
        &'a self,
        source_ref: SourceId,
        resource_ref: ResourceId,
        binding: &'a RepresentationBinding,
        access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>>;
}

/// The session-owned working state for this exact bound representation.
/// A missing or failed lookup cannot authorize a materialization transition.
pub trait CurrentMaterializationStatePort: Send + Sync {
    fn for_resource<'a>(
        &'a self,
        source_ref: SourceId,
        resource_ref: ResourceId,
        binding: &'a RepresentationBinding,
        access_context: &'a str,
    ) -> BoxFuture<'a, Option<MaterializationState>>;
}

/// The Source adapter receives the authorized target stage and the bounds that
/// the application validated. It must not broaden either permission or stage.
pub struct MaterializationRequest {
    pub resource_ref: ResourceId,
    pub binding: RepresentationBinding,
    /// Caller assertion; checked against the session-owned current state at I/O time.
    pub current_state: MaterializationState,
    pub requested_state: MaterializationState,
    pub access_context: String,
    pub access: AccessDecision,
    pub provider_permission: ProviderContentPermission,
    pub retention_mode: RetentionMode,
    pub estimate: ResourceCostEstimate,
    pub budget: MaterializationBudget,
    pub allow_direct_full: bool,
}

pub struct MaterializationReceipt {
    pub(crate) resource_ref: ResourceId,
    pub(crate) binding: RepresentationBinding,
    pub(crate) requested_state: MaterializationState,
    pub(crate) achieved_state: MaterializationState,
    pub(crate) retention_mode: RetentionMode,
    pub(crate) content_digest: Option<String>,
    pub(crate) locator: Option<String>,
    // Runtime bytes deliberately have no Serialize implementation.
    pub(crate) content: Option<Vec<u8>>,
    /// Only the application can stamp a checked, current Source grant.
    pub(crate) policy_verified: bool,
}

impl MaterializationReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resource_ref: ResourceId,
        binding: RepresentationBinding,
        requested_state: MaterializationState,
        achieved_state: MaterializationState,
        retention_mode: RetentionMode,
        content_digest: Option<String>,
        locator: Option<String>,
        content: Option<Vec<u8>>,
    ) -> Self {
        Self {
            resource_ref,
            binding,
            requested_state,
            achieved_state,
            retention_mode,
            content_digest,
            locator,
            content,
            policy_verified: false,
        }
    }

    pub const fn achieved_state(&self) -> MaterializationState {
        self.achieved_state
    }

    pub fn content(&self) -> Option<&[u8]> {
        self.content.as_deref()
    }

    /// Low-cardinality telemetry; neither content, locator nor digest enters it.
    pub fn trace(&self) -> MaterializationTrace {
        MaterializationTrace {
            requested_state: self.requested_state,
            achieved_state: self.achieved_state,
            retention_mode: self.retention_mode,
            content_bytes: self.content.as_ref().map_or(0, Vec::len),
        }
    }

    /// Produces a durable Session Working Store record without body bytes.
    /// SESSION_ONLY, NO_RETENTION and expiry-bound content require a separate
    /// non-durable/expiry-aware store contract and cannot use this conversion.
    pub fn to_session_store_record(&self) -> Result<PersistableMaterializationRecord, SearchError> {
        if !self.policy_verified {
            return Err(SearchError::InvalidRequest(
                "current Source retention policy was not verified".into(),
            ));
        }
        match self.retention_mode {
            RetentionMode::PersistentResource => {}
            RetentionMode::PersistentDiscoveryMetadata
                if self.achieved_state <= MaterializationState::Metadata => {}
            _ => {
                return Err(SearchError::InvalidRequest(
                    "retention forbids durable materialization record".into(),
                ));
            }
        }
        Ok(PersistableMaterializationRecord {
            resource_ref: self.resource_ref,
            binding: self.binding.clone(),
            achieved_state: self.achieved_state,
            content_digest: self.content_digest.clone(),
            locator: self.locator.clone(),
        })
    }
}

impl std::fmt::Debug for MaterializationReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MaterializationReceipt")
            .field("trace", &self.trace())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializationTrace {
    pub requested_state: MaterializationState,
    pub achieved_state: MaterializationState,
    pub retention_mode: RetentionMode,
    pub content_bytes: usize,
}

/// Durable metadata created only after the current Source grant is checked.
/// External Session Working Store code cannot bypass that check with a literal.
///
/// ```compile_fail
/// use search_application::ports::PersistableMaterializationRecord;
/// use search_application::search_core::id::ResourceId;
/// use search_application::search_core::materialization::MaterializationState;
///
/// let _forged = PersistableMaterializationRecord {
///     resource_ref: ResourceId::from_uuid("00000000-0000-0000-0000-000000000002".parse().unwrap()),
///     binding: unreachable!(),
///     achieved_state: MaterializationState::FullContent,
///     content_digest: Some("digest".into()),
///     locator: Some("locator".into()),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistableMaterializationRecord {
    resource_ref: ResourceId,
    binding: RepresentationBinding,
    achieved_state: MaterializationState,
    content_digest: Option<String>,
    locator: Option<String>,
}

impl PersistableMaterializationRecord {
    pub const fn resource_ref(&self) -> ResourceId {
        self.resource_ref
    }

    pub const fn binding(&self) -> &RepresentationBinding {
        &self.binding
    }

    pub const fn achieved_state(&self) -> MaterializationState {
        self.achieved_state
    }

    pub fn content_digest(&self) -> Option<&str> {
        self.content_digest.as_deref()
    }

    pub fn locator(&self) -> Option<&str> {
        self.locator.as_deref()
    }
}

/// The Source adapter must revalidate resource access, exact representation
/// identity and bytes/locator, provider permission, retention, and budget at
/// execution time; application preflight cannot prove the actual remote read.
pub trait MaterializerPort: Send + Sync {
    fn materialize<'a>(
        &'a self,
        request: &'a MaterializationRequest,
    ) -> BoxFuture<'a, MaterializationReceipt>;
}
