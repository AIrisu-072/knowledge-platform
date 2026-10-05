//! P1-A02 BodyOnly lexical contracts. A Unit hit reference is copied unchanged
//! through retrieval records so qualification and evidence can re-verify it.

use search_core::discovery::FederatedCandidate;
use search_core::id::{ClaimId, ResourceId};
use search_core::knowledge_unit::{
    ContentPartRef, ExtractionProfileId, RawBinding, ResourceVersionRef, TextSpan, UnitId,
};
use search_core::projection::ProjectionGenerationKey;

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
