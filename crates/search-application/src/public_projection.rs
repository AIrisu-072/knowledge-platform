//! P5-04: the safe public projection of one Discovery result.
//!
//! Built only from the gated result inside the disclosure callback. It
//! carries canonical IDs, registered generic codes and currently authorized
//! scalar values only: no locator, candidate ID, Graph path, raw score,
//! provider label, private gap fact or qualification trace string. Required
//! evidence that is unassessed or a Partial body coverage gap is never
//! reported as sufficient, and every evidence item keeps its Claim state.

use std::collections::BTreeMap;

use search_core::applicability::ApplicabilityState;
use search_core::discovery::{DiscoveryResult, GapReason};
use search_core::evidence::{ClaimState, EvidenceRole, EvidenceSufficiency};
use search_core::id::{ClaimId, DiscoveryEvaluationId, NeedId, ResourceId, SourceId};
use search_core::predicate::TypedValue;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::search_query::{PublicGapView, public_gaps};

pub const MAX_QUALIFIED: usize = 50;
pub const MAX_EVIDENCE: usize = 64;
pub const MAX_TRACE: usize = 64;
pub const MAX_REJECTED: usize = 200;
pub const MAX_VALUE_CODE_POINTS: usize = 320;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicApplicability {
    Applicable,
    Excluded,
    Unresolved,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicSufficiency {
    Sufficient,
    Insufficient,
    Unresolved,
    Conflicted,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicClaimState {
    Supported,
    Absent,
    Unknown,
    Conflicted,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicEvidenceRole {
    Primary,
    Corroborating,
    Contradicting,
    Contextual,
    Derived,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PublicEvidenceValue {
    Text(String),
    Integer(i64),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completeness {
    Complete,
    Bounded,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompletenessReason {
    BudgetExhausted,
    SourceInterrupted,
    RequiredEvidenceUnevaluated,
    BodyCoverageIncomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceStage {
    Routing,
    Retrieval,
    Qualification,
    Evidence,
    Disclosure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceOutcome {
    Completed,
    Bounded,
    Interrupted,
    Skipped,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicQualified {
    pub resource_id: ResourceId,
    pub source_id: SourceId,
    pub applicability: PublicApplicability,
    pub matched_condition_codes: Vec<String>,
    pub resolved_discriminator_codes: Vec<String>,
    pub evidence_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublicEvidence {
    pub id: Uuid,
    pub claim_id: ClaimId,
    pub state: PublicClaimState,
    pub role: PublicEvidenceRole,
    pub source_id: Option<SourceId>,
    pub value: Option<PublicEvidenceValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicTraceEntry {
    pub stage: TraceStage,
    pub outcome: TraceOutcome,
    pub visible_source_id: Option<SourceId>,
    pub count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryEvaluationView {
    pub need_id: NeedId,
    pub evaluation_id: DiscoveryEvaluationId,
    pub qualified: Vec<PublicQualified>,
    pub sufficiency: PublicSufficiency,
    pub evidence: Vec<PublicEvidence>,
    pub gaps: Vec<PublicGapView>,
    pub rejected_reason_codes: Vec<&'static str>,
    pub completeness: Completeness,
    pub completeness_reasons: Vec<CompletenessReason>,
    pub trace: Vec<PublicTraceEntry>,
    pub trace_id: Uuid,
}

/// A stable canonical ID for one evidence reference of one evaluation.
fn evidence_id(evaluation: DiscoveryEvaluationId, reference: &str) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(b"search-public-evidence:v1");
    hasher.update(evaluation.as_uuid().as_bytes());
    hasher.update((reference.len() as u64).to_be_bytes());
    hasher.update(reference.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Only registered-looking codes survive; free text never does.
fn public_codes(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter(|value| {
            let bytes = value.as_bytes();
            !bytes.is_empty()
                && bytes.len() <= 64
                && bytes[0].is_ascii_uppercase()
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
        })
        .take(64)
        .cloned()
        .collect()
}

fn value(value: &TypedValue) -> Option<PublicEvidenceValue> {
    match value {
        TypedValue::String(text) | TypedValue::ConceptRef(text)
            if text.chars().count() <= MAX_VALUE_CODE_POINTS =>
        {
            Some(PublicEvidenceValue::Text(text.clone()))
        }
        TypedValue::Integer(number) => i64::try_from(*number)
            .ok()
            .map(PublicEvidenceValue::Integer),
        TypedValue::Bool(flag) => Some(PublicEvidenceValue::Bool(*flag)),
        _ => None,
    }
}

const fn state(state: ClaimState) -> PublicClaimState {
    match state {
        ClaimState::Supported => PublicClaimState::Supported,
        ClaimState::Absent => PublicClaimState::Absent,
        ClaimState::Unknown => PublicClaimState::Unknown,
        ClaimState::Conflicted => PublicClaimState::Conflicted,
        ClaimState::Invalid => PublicClaimState::Invalid,
    }
}

const fn role(role: EvidenceRole) -> PublicEvidenceRole {
    match role {
        EvidenceRole::Primary => PublicEvidenceRole::Primary,
        EvidenceRole::Corroborating => PublicEvidenceRole::Corroborating,
        EvidenceRole::Contradicting => PublicEvidenceRole::Contradicting,
        EvidenceRole::Contextual => PublicEvidenceRole::Contextual,
        EvidenceRole::Derived => PublicEvidenceRole::Derived,
    }
}

pub fn project_discovery(result: &DiscoveryResult, trace_id: Uuid) -> DiscoveryEvaluationView {
    let evaluation = result.discovery_evaluation_id;
    let qualified: Vec<PublicQualified> = result
        .qualified_resources
        .iter()
        .filter_map(|item| {
            Some(PublicQualified {
                resource_id: item.resource_ref,
                source_id: item.source_ref?,
                applicability: match item.applicability {
                    ApplicabilityState::Applicable => PublicApplicability::Applicable,
                    ApplicabilityState::Excluded => PublicApplicability::Excluded,
                    ApplicabilityState::Unresolved => PublicApplicability::Unresolved,
                    _ => PublicApplicability::Invalid,
                },
                matched_condition_codes: public_codes(&item.matched_conditions),
                resolved_discriminator_codes: public_codes(&item.resolved_discriminators),
                evidence_ids: item
                    .evidence_refs
                    .iter()
                    .take(MAX_EVIDENCE)
                    .map(|reference| evidence_id(evaluation, reference))
                    .collect(),
            })
        })
        .take(MAX_QUALIFIED)
        .collect();
    let mut evidence = Vec::new();
    for claim in &result.evidence_set {
        let claim_value = claim.value.as_ref().and_then(value);
        if claim.evidence_refs.is_empty() {
            evidence.push(PublicEvidence {
                id: evidence_id(evaluation, &format!("claim:{}", claim.claim_id.as_uuid())),
                claim_id: claim.claim_id,
                state: state(claim.state),
                role: PublicEvidenceRole::Contextual,
                source_id: None,
                value: None,
            });
            continue;
        }
        for (index, reference) in claim.evidence_refs.iter().enumerate() {
            let key = reference
                .evidence_ref
                .clone()
                .unwrap_or_else(|| format!("claim:{}:{index}", claim.claim_id.as_uuid()));
            evidence.push(PublicEvidence {
                id: evidence_id(evaluation, &key),
                claim_id: claim.claim_id,
                state: state(claim.state),
                role: role(reference.role),
                source_id: Some(reference.source_ref),
                value: claim_value.clone(),
            });
        }
    }
    evidence.truncate(MAX_EVIDENCE);
    let gaps = public_gaps(&result.unresolved_gaps);
    let mut reasons = Vec::new();
    for gap in &result.unresolved_gaps {
        let fact = gap.required_fact.as_str();
        let reason = if fact == "max_discovery_actions_reached" {
            Some(CompletenessReason::BudgetExhausted)
        } else if fact.starts_with("document.body.coverage") {
            Some(CompletenessReason::BodyCoverageIncomplete)
        } else if fact.starts_with("claim:") {
            Some(CompletenessReason::RequiredEvidenceUnevaluated)
        } else if gap.reason == GapReason::Availability && gap.blocking {
            Some(CompletenessReason::SourceInterrupted)
        } else {
            None
        };
        if let Some(reason) = reason
            && !reasons.contains(&reason)
        {
            reasons.push(reason);
        }
    }
    reasons.sort();
    let completeness = if reasons.contains(&CompletenessReason::SourceInterrupted) {
        Completeness::Interrupted
    } else if reasons.contains(&CompletenessReason::BudgetExhausted) {
        Completeness::Bounded
    } else {
        Completeness::Complete
    };
    // Only counts per currently visible contributing Source.
    let mut per_source: BTreeMap<SourceId, u32> = BTreeMap::new();
    for item in &qualified {
        *per_source.entry(item.source_id).or_default() += 1;
    }
    let mut trace: Vec<PublicTraceEntry> = per_source
        .into_iter()
        .map(|(source, count)| PublicTraceEntry {
            stage: TraceStage::Qualification,
            outcome: TraceOutcome::Completed,
            visible_source_id: Some(source),
            count: Some(count),
        })
        .collect();
    trace.push(PublicTraceEntry {
        stage: TraceStage::Evidence,
        outcome: if completeness == Completeness::Complete {
            TraceOutcome::Completed
        } else {
            TraceOutcome::Interrupted
        },
        visible_source_id: None,
        count: Some(evidence.len() as u32),
    });
    trace.truncate(MAX_TRACE);
    DiscoveryEvaluationView {
        need_id: result.need.need_id,
        evaluation_id: evaluation,
        qualified,
        sufficiency: match result.evidence_sufficiency {
            EvidenceSufficiency::Sufficient => PublicSufficiency::Sufficient,
            EvidenceSufficiency::Insufficient => PublicSufficiency::Insufficient,
            EvidenceSufficiency::Unresolved => PublicSufficiency::Unresolved,
            EvidenceSufficiency::Conflicted => PublicSufficiency::Conflicted,
            EvidenceSufficiency::Invalid => PublicSufficiency::Invalid,
        },
        evidence,
        gaps,
        rejected_reason_codes: result
            .rejected_candidates
            .iter()
            .take(MAX_REJECTED)
            .map(|_| "HARD_GATE_REJECTED")
            .collect(),
        completeness,
        completeness_reasons: reasons,
        trace,
        trace_id,
    }
}
