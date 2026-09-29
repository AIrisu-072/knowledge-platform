//! Backend-neutral retrieval hits and already evaluated hard-gate results.

use search_core::applicability::{ApplicabilityEvaluation, ApplicabilityState};
use search_core::discovery::{FederatedCandidate, InformationGap, RejectedCandidate};
use search_core::id::LogicalResourceId;
use search_core::identity::IdentityEvidence;
use search_core::projection::ProjectionGenerationKey;

/// Structured and current-access checks are evaluated upstream. An unknown
/// check must arrive as `Unresolved`, with its gap when one is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardGateEvaluation {
    pub state: ApplicabilityState,
    pub reasons: Vec<String>,
    pub gaps: Vec<InformationGap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateHardGates {
    pub applicability: ApplicabilityEvaluation,
    pub structured: HardGateEvaluation,
    pub access: HardGateEvaluation,
}

/// The caller supplies hits in its routed retriever order and each retriever's
/// own rank order. Scores are optional trace data, never fusion weights.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedCandidateHit {
    pub candidate: FederatedCandidate,
    pub hard_gates: CandidateHardGates,
    /// Evidence for this candidate's claimed `logical_resource_ref`, rather
    /// than evidence that two display names or source labels happen to match.
    pub identity_evidence: Vec<IdentityEvidence>,
    pub raw_score: Option<f64>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrieverRankList {
    pub retriever_id: String,
    /// Source-local immutable generation used to produce every hit in this list.
    pub generation: ProjectionGenerationKey,
    pub hits: Vec<RankedCandidateHit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrieverHitTrace {
    pub retriever_id: String,
    pub generation: ProjectionGenerationKey,
    /// One-based rank in the original retriever list, before hard-gate removal.
    pub rank: usize,
    pub raw_score: Option<f64>,
    pub evidence_refs: Vec<String>,
}

/// One representation/retrieval hit remains intact after grouping. The
/// candidate carries its locator, provenance, signals and retrieval trace.
#[derive(Debug, Clone, PartialEq)]
pub struct RepresentationHit {
    pub candidate: FederatedCandidate,
    pub hard_gates: CandidateHardGates,
    pub identity_evidence: Vec<IdentityEvidence>,
    pub trace: RetrieverHitTrace,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FusedCandidate {
    /// Set only when strong, non-conflicting evidence resolves the mapping.
    pub logical_resource_ref: Option<LogicalResourceId>,
    pub hits: Vec<RepresentationHit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingCandidateHit {
    /// Retained so a later probe can resolve the unknown gate.
    pub hit: RepresentationHit,
    pub gaps: Vec<InformationGap>,
    pub reason_trace: Vec<String>,
}

/// One probe target can retain several unresolved retrieval hits and their
/// distinct gaps, avoiding duplicate expensive probes for known identities.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingCandidateGroup {
    pub logical_resource_ref: Option<LogicalResourceId>,
    pub hits: Vec<PendingCandidateHit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RejectedCandidateHit {
    pub rejection: RejectedCandidate,
    /// Preserve Source identity, candidate provenance and the original
    /// retriever rank, score and evidence for audit and later re-evaluation.
    pub hit: RepresentationHit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateFederationResult {
    pub ranked: Vec<FusedCandidate>,
    pub pending: Vec<PendingCandidateGroup>,
    pub rejected: Vec<RejectedCandidateHit>,
}
