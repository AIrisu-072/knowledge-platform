//! P1-A02 BodyOnly lexical contracts. A Unit hit reference is copied unchanged
//! through retrieval records so qualification and evidence can re-verify it.

use search_core::assertion::Assertion;
use search_core::discovery::{DiscoveryRequest, FederatedCandidate, InformationGap};
use search_core::id::{ClaimId, ResourceId};
use search_core::knowledge_unit::{
    ContentPartRef, ExtractionProfileId, RawBinding, ResourceVersionRef, TextSpan, UnitId,
};
use search_core::projection::ProjectionGenerationKey;

use crate::ports::{BoxFuture, ResolvedAssertionEvidence};

/// The only predicate a body Unit may support in v1: the normalized literal
/// occurs contiguously inside one verified Unit of one current parent Version.
pub const CONTAINS_EXACT_PREDICATE: &str = "document.body.contains_exact";

/// One searchable Unit that literally contains the normalized query text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeUnitHitRef {
    pub generation: ProjectionGenerationKey,
    pub parent_resource: ResourceId,
    pub version: ResourceVersionRef,
    pub part: ContentPartRef,
    pub authoritative_representation_ref: String,
    pub unit_id: UnitId,
    /// UTF-8 byte range of the matched literal inside the Unit text.
    pub span: TextSpan,
    pub text_sha256: [u8; 32],
    pub raw: RawBinding,
    pub profile: ExtractionProfileId,
    /// Encoded native locator; meaningful only to the owning Source.
    pub opaque_locator: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalHit {
    pub candidate: FederatedCandidate,
    /// `None` when the index matched tokens but no exact literal span exists.
    pub unit_hit: Option<KnowledgeUnitHitRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalRetrievalBatch {
    pub hits: Vec<LexicalHit>,
    /// True only when every matching Unit of the pinned generation was seen.
    pub exhausted_matching_units: bool,
}

/// Trusted mapping from a required Claim to one exact body phrase of one parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactTextSelector {
    pub claim_id: ClaimId,
    pub parent_resource: ResourceId,
    pub predicate: String,
    pub expected_exact_text: String,
}

/// A Source-verified exact span: the owning Source re-read the raw bytes,
/// rebuilt the same Unit and confirmed current access before returning it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedExtractedTextEvidence {
    /// `origin == AssertionOrigin::Extracted`; subject is the parent Version.
    pub assertion: Assertion,
    pub resolved: ResolvedAssertionEvidence,
    pub matched_span: TextSpan,
}

/// Trusted selector registry and Source-owned verification of one Unit hit.
/// Any change, uncertainty or mismatch yields `None`, never a weaker claim.
pub trait ExactTextEvidencePort: Send + Sync {
    fn selector_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ExactTextSelector>>;

    fn resolve_hit<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        hit: &'a KnowledgeUnitHitRef,
        selector: &'a ExactTextSelector,
    ) -> BoxFuture<'a, Option<VerifiedExtractedTextEvidence>>;
}

/// Body coverage of the pinned generation as seen by the caller's current
/// Read. Denied items contribute nothing, not even a count; an undecidable
/// access check collapses into one blocking gap without item identities.
pub trait BodyCoverageGapPort: Send + Sync {
    fn coverage_gaps<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        generation: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Vec<InformationGap>>;
}
