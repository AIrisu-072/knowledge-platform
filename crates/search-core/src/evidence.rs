//! Claim-level evidence preserves contradiction and independent provenance.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::id::ClaimId;
use crate::predicate::TypedValue;
use crate::relation::RelationTemporalScope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceRole {
    Primary,
    Corroborating,
    Contradicting,
    Contextual,
    Derived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceReference {
    pub source_ref: String,
    pub upstream_origin: String,
    pub role: EvidenceRole,
    pub citation_chain: Vec<String>,
    pub content_digest: Option<String>,
    pub is_summary: bool,
}

impl EvidenceReference {
    pub fn new(
        source_ref: impl Into<String>,
        upstream_origin: impl Into<String>,
        role: EvidenceRole,
    ) -> Self {
        Self {
            source_ref: source_ref.into(),
            upstream_origin: upstream_origin.into(),
            role,
            citation_chain: Vec::new(),
            content_digest: None,
            is_summary: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClaimState {
    Supported,
    Absent,
    Unknown,
    Conflicted,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub claim_id: ClaimId,
    pub subject: Option<String>,
    pub predicate: Option<String>,
    pub value: Option<TypedValue>,
    pub temporal_scope: RelationTemporalScope,
    pub evidence_refs: Vec<EvidenceReference>,
    pub state: ClaimState,
}

impl Claim {
    pub fn new(claim_id: ClaimId, state: ClaimState) -> Self {
        Self {
            claim_id,
            subject: None,
            predicate: None,
            value: None,
            temporal_scope: RelationTemporalScope::default(),
            evidence_refs: Vec::new(),
            state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRequirement {
    pub required_claims: Vec<ClaimId>,
    pub authority_requirements: Vec<String>,
    pub freshness_requirements: Vec<String>,
    pub minimum_independent_sources: usize,
    pub contradiction_policy: String,
    pub completion_policy: String,
}

impl EvidenceRequirement {
    pub fn new(required_claims: Vec<ClaimId>) -> Self {
        Self {
            required_claims,
            authority_requirements: Vec::new(),
            freshness_requirements: Vec::new(),
            minimum_independent_sources: 1,
            contradiction_policy: "block".into(),
            completion_policy: "all-required".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceSufficiency {
    Sufficient,
    Insufficient,
    Unresolved,
    Conflicted,
    Invalid,
}

pub fn evaluate_evidence_sufficiency(
    requirement: &EvidenceRequirement,
    claims: &[Claim],
) -> EvidenceSufficiency {
    let mut insufficient = false;
    let mut unresolved = false;
    for claim_id in &requirement.required_claims {
        let matching: Vec<&Claim> = claims
            .iter()
            .filter(|claim| claim.claim_id == *claim_id)
            .collect();
        if matching.is_empty() {
            unresolved = true;
            continue;
        }
        if matching
            .iter()
            .any(|claim| claim.state == ClaimState::Invalid)
        {
            return EvidenceSufficiency::Invalid;
        }
        if matching
            .iter()
            .any(|claim| claim.state == ClaimState::Conflicted)
        {
            return EvidenceSufficiency::Conflicted;
        }
        let supported = matching
            .iter()
            .any(|claim| claim.state == ClaimState::Supported);
        let absent = matching
            .iter()
            .any(|claim| claim.state == ClaimState::Absent);
        if supported && absent {
            return EvidenceSufficiency::Conflicted;
        }
        let mut observed_value: Option<&TypedValue> = None;
        for claim in &matching {
            if let Some(value) = claim.value.as_ref() {
                if observed_value.is_some_and(|existing| existing != value) {
                    return EvidenceSufficiency::Conflicted;
                }
                observed_value = Some(value);
            }
        }
        if absent {
            insufficient = true;
            continue;
        }
        if !supported
            || matching
                .iter()
                .any(|claim| claim.state == ClaimState::Unknown)
        {
            unresolved = true;
            continue;
        }
        let evidence: Vec<&EvidenceReference> = matching
            .iter()
            .flat_map(|claim| claim.evidence_refs.iter())
            .collect();
        if evidence
            .iter()
            .any(|evidence| evidence.role == EvidenceRole::Contradicting)
        {
            return EvidenceSufficiency::Conflicted;
        }
        let has_primary = evidence
            .iter()
            .any(|evidence| evidence.role == EvidenceRole::Primary && !evidence.is_summary);
        let independent_origins: BTreeSet<&str> = evidence
            .iter()
            .filter(|evidence| {
                matches!(
                    evidence.role,
                    EvidenceRole::Primary | EvidenceRole::Corroborating
                ) && !evidence.is_summary
            })
            .map(|evidence| evidence.upstream_origin.as_str())
            .collect();
        if !has_primary || independent_origins.len() < requirement.minimum_independent_sources {
            insufficient = true;
        }
    }
    if !requirement.authority_requirements.is_empty()
        || !requirement.freshness_requirements.is_empty()
    {
        unresolved = true;
    }
    if insufficient {
        EvidenceSufficiency::Insufficient
    } else if unresolved {
        EvidenceSufficiency::Unresolved
    } else {
        EvidenceSufficiency::Sufficient
    }
}
