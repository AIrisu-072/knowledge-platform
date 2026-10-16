//! P1-A02 BodyOnly lexical contracts. A Unit hit reference is copied unchanged
//! through retrieval records so qualification and evidence can re-verify it.

use std::time::{Duration, Instant};

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
    /// A bounded plain-text window of the Unit around the matched literal
    /// (at most `MAX_EXCERPT_CHARS` code points). It is disclosed only as the
    /// body snippet of a hit that passed the final current-access gate.
    pub excerpt: Option<String>,
}

/// The longest body snippet the Search API may disclose.
pub const MAX_EXCERPT_CHARS: usize = 320;

/// The window of `text` around `span` with at most `MAX_EXCERPT_CHARS` code
/// points, the match kept whole when it fits, line breaks as spaces.
pub fn excerpt_around(text: &str, span: &TextSpan) -> Option<String> {
    let start = text.get(..span.start_byte as usize)?.chars().count();
    let matched = text
        .get(span.start_byte as usize..span.end_byte as usize)?
        .chars()
        .count();
    let chars: Vec<char> = text.chars().collect();
    let room = MAX_EXCERPT_CHARS.saturating_sub(matched);
    // Half of the spare room before the match; a short tail shifts it back.
    let mut from = start.saturating_sub(room / 2);
    let to = (from + MAX_EXCERPT_CHARS).min(chars.len());
    if to - from < MAX_EXCERPT_CHARS {
        from = to.saturating_sub(MAX_EXCERPT_CHARS);
    }
    let window: String = chars[from..to]
        .iter()
        .map(|c| if c.is_control() { ' ' } else { *c })
        .collect();
    let trimmed = window.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
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

/// Handle of one published body bundle, as pinned for this evaluation. The
/// owning Source re-checks it against its own published receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedBodyBundle {
    pub generation: ProjectionGenerationKey,
    pub composite_digest: [u8; 32],
}

/// Finite limits of one exact absence scan. Reaching any limit is `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactScanBudget {
    pub max_visible_items: u64,
    pub max_units: u64,
    pub max_text_bytes: u64,
    pub deadline: Instant,
}

impl ExactScanBudget {
    /// Initial request limits: 1024 items, 100000 Units, 64 MiB, 2 s.
    pub fn initial(now: Instant) -> Self {
        Self {
            max_visible_items: 1024,
            max_units: 100_000,
            max_text_bytes: 64 * 1024 * 1024,
            deadline: now + Duration::from_secs(2),
        }
    }
}

/// Receipt of a completed, finite, Source-owned scan: the literal occurs in
/// no Unit of any visible item of one current parent Version. Only a
/// registered Source port issues it; it carries bindings, never body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactTextNegativeProof {
    generation: ProjectionGenerationKey,
    bundle_digest: [u8; 32],
    source_snapshot: String,
    claim_id: ClaimId,
    parent: ResourceVersionRef,
    document_revision: i64,
    access_revision: i64,
    exact_text_sha256: [u8; 32],
    visible_item_bindings_digest: [u8; 32],
    scanned_unit_bindings_digest: [u8; 32],
    visible_item_count: u64,
    scanned_unit_count: u64,
}

/// Fields of a negative proof, as computed by the issuing Source port.
pub struct NegativeProofFields {
    pub generation: ProjectionGenerationKey,
    pub bundle_digest: [u8; 32],
    pub source_snapshot: String,
    pub claim_id: ClaimId,
    pub parent: ResourceVersionRef,
    pub document_revision: i64,
    pub access_revision: i64,
    pub exact_text_sha256: [u8; 32],
    pub visible_item_bindings_digest: [u8; 32],
    pub scanned_unit_bindings_digest: [u8; 32],
    pub visible_item_count: u64,
    pub scanned_unit_count: u64,
}

impl ExactTextNegativeProof {
    /// Issued only by a Source-owned absence port after its verified scan.
    pub fn issue(fields: NegativeProofFields) -> Self {
        Self {
            generation: fields.generation,
            bundle_digest: fields.bundle_digest,
            source_snapshot: fields.source_snapshot,
            claim_id: fields.claim_id,
            parent: fields.parent,
            document_revision: fields.document_revision,
            access_revision: fields.access_revision,
            exact_text_sha256: fields.exact_text_sha256,
            visible_item_bindings_digest: fields.visible_item_bindings_digest,
            scanned_unit_bindings_digest: fields.scanned_unit_bindings_digest,
            visible_item_count: fields.visible_item_count,
            scanned_unit_count: fields.scanned_unit_count,
        }
    }

    pub fn predicate(&self) -> &'static str {
        CONTAINS_EXACT_PREDICATE
    }
    pub fn generation(&self) -> ProjectionGenerationKey {
        self.generation
    }
    pub fn bundle_digest(&self) -> [u8; 32] {
        self.bundle_digest
    }
    pub fn source_snapshot(&self) -> &str {
        &self.source_snapshot
    }
    pub fn claim_id(&self) -> ClaimId {
        self.claim_id
    }
    pub fn parent(&self) -> &ResourceVersionRef {
        &self.parent
    }
    pub fn revisions(&self) -> (i64, i64) {
        (self.document_revision, self.access_revision)
    }
    pub fn exact_text_sha256(&self) -> [u8; 32] {
        self.exact_text_sha256
    }
    pub fn binding_digests(&self) -> ([u8; 32], [u8; 32]) {
        (
            self.visible_item_bindings_digest,
            self.scanned_unit_bindings_digest,
        )
    }
    pub fn counts(&self) -> (u64, u64) {
        (self.visible_item_count, self.scanned_unit_count)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExactTextAbsenceOutcome {
    ProvenAbsent(Box<ExactTextNegativeProof>),
    /// Internal integrity/recall signal; never a hit or evidence by itself.
    MatchFound,
    /// Blocking gap with a non-disclosing `required_fact`.
    Unknown(InformationGap),
}

/// Finite exact negative proof for one selector parent. Lexical no-hit only
/// starts the scan; it is never evidence of absence.
pub trait SourceExactTextAbsencePort: Send + Sync {
    fn pin_body<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Option<PinnedBodyBundle>>;

    fn verify_absence<'a>(
        &'a self,
        request: &'a DiscoveryRequest,
        pinned: &'a PinnedBodyBundle,
        selector: &'a ExactTextSelector,
        budget: ExactScanBudget,
    ) -> BoxFuture<'a, ExactTextAbsenceOutcome>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, needle: &str) -> TextSpan {
        let start = text.find(needle).unwrap() as u32;
        TextSpan::new(text, start, start + needle.len() as u32).unwrap()
    }

    #[test]
    fn excerpt_keeps_the_match_inside_a_bounded_window() {
        let text = format!("{}東京の本文{}", "前".repeat(500), "後".repeat(500));
        let excerpt = excerpt_around(&text, &span(&text, "東京の本文")).unwrap();
        assert!(excerpt.contains("東京の本文"));
        assert_eq!(excerpt.chars().count(), MAX_EXCERPT_CHARS);
        // Near the end the window shifts back instead of shrinking.
        let tail = format!("{}終わりの語", "前".repeat(500));
        let excerpt = excerpt_around(&tail, &span(&tail, "終わりの語")).unwrap();
        assert!(excerpt.ends_with("終わりの語"));
        assert_eq!(excerpt.chars().count(), MAX_EXCERPT_CHARS);
        // Short text is returned whole, line breaks as spaces.
        let short = "一行目\n東京\n三行目";
        assert_eq!(
            excerpt_around(short, &span(short, "東京")).unwrap(),
            "一行目 東京 三行目"
        );
    }
}
