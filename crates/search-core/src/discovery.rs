//! Discovery contracts retain gaps, rejection reasons, and source trace.

use serde::{Deserialize, Serialize};

use crate::applicability::{ApplicabilityEvaluation, ApplicabilityState};
use crate::evidence::{Claim, EvidenceRequirement, EvidenceSufficiency};
use crate::id::{
    ClaimId, DiscoveryEvaluationId, GapId, LogicalResourceId, NeedId, ResourceId, SourceId,
    UsageProfileId,
};
use crate::intent::IntentSignature;
use crate::resource::ResourceKind;
use crate::temporal::TemporalEvaluationContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateIdentityClass {
    DurableResource,
    RemoteStableReference,
    EphemeralCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedCandidate {
    pub candidate_id: String,
    pub identity_class: CandidateIdentityClass,
    pub resource_ref: Option<ResourceId>,
    pub logical_resource_ref: Option<LogicalResourceId>,
    pub source_ref: SourceId,
    pub retrieval_method: String,
    pub locator: Option<String>,
    pub matched_signals: Vec<String>,
    pub materialization_state: Option<String>,
    pub provenance: Option<String>,
    pub retrieval_trace_ref: Option<String>,
}

impl FederatedCandidate {
    pub fn new(
        candidate_id: impl Into<String>,
        identity_class: CandidateIdentityClass,
        source_ref: SourceId,
        retrieval_method: impl Into<String>,
    ) -> Self {
        Self {
            candidate_id: candidate_id.into(),
            identity_class,
            resource_ref: None,
            logical_resource_ref: None,
            source_ref,
            retrieval_method: retrieval_method.into(),
            locator: None,
            matched_signals: Vec::new(),
            materialization_state: None,
            provenance: None,
            retrieval_trace_ref: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapReason {
    MissingFact,
    InsufficientEvidenceClass,
    Authority,
    Freshness,
    Corroboration,
    Conflict,
    Availability,
    UnsupportedCoverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InformationGap {
    pub gap_id: Option<GapId>,
    pub required_fact: String,
    pub reason: GapReason,
    pub blocking: bool,
    pub acceptable_evidence: Vec<String>,
}

impl InformationGap {
    pub fn new(required_fact: impl Into<String>, reason: GapReason, blocking: bool) -> Self {
        Self {
            gap_id: None,
            required_fact: required_fact.into(),
            reason,
            blocking,
            acceptable_evidence: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualifiedResource {
    pub resource_ref: ResourceId,
    /// The Source whose candidate qualified; absent only in older records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<SourceId>,
    pub usage_profile_ref: Option<UsageProfileId>,
    pub applicability: ApplicabilityState,
    pub matched_conditions: Vec<String>,
    pub resolved_discriminators: Vec<String>,
    pub remaining_nonblocking_unknowns: Vec<String>,
    pub contrast_resolution: Option<String>,
    pub evidence_refs: Vec<String>,
    pub qualification_trace: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedCandidate {
    pub candidate_id: String,
    pub state: ApplicabilityState,
    pub reason_trace: Vec<String>,
}

impl RejectedCandidate {
    pub fn from_evaluation(
        candidate: &FederatedCandidate,
        evaluation: &ApplicabilityEvaluation,
    ) -> Option<Self> {
        if evaluation.state == ApplicabilityState::Applicable {
            return None;
        }
        Some(Self {
            candidate_id: candidate.candidate_id.clone(),
            state: evaluation.state,
            reason_trace: evaluation.reasons.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryNeed {
    pub need_id: NeedId,
    pub intent_signature: IntentSignature,
    pub required_resource_types: Vec<ResourceKind>,
    pub required_claims: Vec<ClaimId>,
    pub authority_requirements: Vec<String>,
    pub freshness_requirements: Vec<String>,
    pub constraints: Vec<String>,
    pub completion_requirement: EvidenceRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryRequest {
    pub need: DiscoveryNeed,
    pub temporal_context: TemporalEvaluationContext,
    pub access_context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub discovery_evaluation_id: DiscoveryEvaluationId,
    pub need: DiscoveryNeed,
    pub qualified_resources: Vec<QualifiedResource>,
    pub evidence_set: Vec<Claim>,
    pub evidence_sufficiency: EvidenceSufficiency,
    pub unresolved_gaps: Vec<InformationGap>,
    pub rejected_candidates: Vec<RejectedCandidate>,
    pub source_trace: Vec<String>,
    pub retrieval_trace: Vec<String>,
    pub qualification_trace: Vec<String>,
}
