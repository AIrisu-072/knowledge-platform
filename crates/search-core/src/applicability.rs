//! Hard gates precede soft ranking; unknown is never silently excluded.

use serde::{Deserialize, Serialize};

use crate::discovery::{FederatedCandidate, GapReason, InformationGap, QualifiedResource};
use crate::fact::{FactOrigin, FactSet};
use crate::predicate::{ConceptResolver, PredicateEvaluator, PredicateExpr, TruthValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplicabilityState {
    Applicable,
    Excluded,
    Unresolved,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscriminatorImportance {
    Hard,
    Soft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MinimumFactEvidence {
    Any,
    NotInferred,
    AuthoritativeOrExplicit,
}

impl MinimumFactEvidence {
    fn accepts(self, origin: FactOrigin) -> bool {
        match self {
            Self::Any => true,
            Self::NotInferred => origin != FactOrigin::Inferred,
            Self::AuthoritativeOrExplicit => {
                matches!(origin, FactOrigin::Authoritative | FactOrigin::Explicit)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discriminator {
    pub facet: String,
    pub importance: DiscriminatorImportance,
    pub predicate: PredicateExpr,
    pub minimum_fact_evidence: MinimumFactEvidence,
}

impl Discriminator {
    pub fn new(
        facet: impl Into<String>,
        importance: DiscriminatorImportance,
        predicate: PredicateExpr,
    ) -> Self {
        Self {
            facet: facet.into(),
            importance,
            predicate,
            minimum_fact_evidence: MinimumFactEvidence::Any,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicabilityEvaluation {
    pub state: ApplicabilityState,
    pub matched_conditions: Vec<String>,
    pub resolved_discriminators: Vec<String>,
    pub remaining_nonblocking_unknowns: Vec<String>,
    pub gaps: Vec<InformationGap>,
    pub reasons: Vec<String>,
}

impl ApplicabilityEvaluation {
    pub fn qualify(&self, candidate: &FederatedCandidate) -> Option<QualifiedResource> {
        if self.state != ApplicabilityState::Applicable {
            return None;
        }
        Some(QualifiedResource {
            resource_ref: candidate.resource_ref?,
            usage_profile_ref: None,
            applicability: self.state,
            matched_conditions: self.matched_conditions.clone(),
            resolved_discriminators: self.resolved_discriminators.clone(),
            remaining_nonblocking_unknowns: self.remaining_nonblocking_unknowns.clone(),
            contrast_resolution: None,
            evidence_refs: Vec::new(),
            qualification_trace: self.reasons.clone(),
        })
    }
}

pub fn evaluate_applicability(
    _candidate: &FederatedCandidate,
    facts: &FactSet,
    discriminators: &[Discriminator],
    concepts: &dyn ConceptResolver,
) -> ApplicabilityEvaluation {
    let mut result = ApplicabilityEvaluation {
        state: ApplicabilityState::Applicable,
        matched_conditions: Vec::new(),
        resolved_discriminators: Vec::new(),
        remaining_nonblocking_unknowns: Vec::new(),
        gaps: Vec::new(),
        reasons: Vec::new(),
    };
    let mut hard_false = false;
    let mut hard_unknown = false;
    let mut invalid = false;
    for rule in discriminators {
        let insufficient_origin = facts
            .get(&rule.facet)
            .is_some_and(|fact| !rule.minimum_fact_evidence.accepts(fact.origin));
        let value = if insufficient_origin {
            TruthValue::Unknown
        } else {
            PredicateEvaluator::evaluate(&rule.predicate, facts, concepts)
        };
        match (rule.importance, value) {
            (_, TruthValue::Error) => {
                invalid = true;
                result
                    .reasons
                    .push(format!("{}: predicate error", rule.facet));
            }
            (DiscriminatorImportance::Hard, TruthValue::False) => {
                hard_false = true;
                result
                    .reasons
                    .push(format!("{}: hard mismatch", rule.facet));
            }
            (DiscriminatorImportance::Hard, TruthValue::Unknown) => {
                hard_unknown = true;
                result.gaps.push(InformationGap::new(
                    &rule.facet,
                    if insufficient_origin {
                        GapReason::InsufficientEvidenceClass
                    } else {
                        GapReason::MissingFact
                    },
                    true,
                ));
                result
                    .reasons
                    .push(format!("{}: hard unresolved", rule.facet));
            }
            (DiscriminatorImportance::Soft, TruthValue::Unknown) => {
                result
                    .remaining_nonblocking_unknowns
                    .push(rule.facet.clone());
                result
                    .reasons
                    .push(format!("{}: soft unresolved", rule.facet));
            }
            (_, TruthValue::True) => {
                result.matched_conditions.push(rule.facet.clone());
                result.resolved_discriminators.push(rule.facet.clone());
            }
            (DiscriminatorImportance::Soft, TruthValue::False) => {
                result.resolved_discriminators.push(rule.facet.clone());
                result
                    .reasons
                    .push(format!("{}: soft mismatch", rule.facet));
            }
        }
    }
    result.state = if invalid {
        ApplicabilityState::Invalid
    } else if hard_false {
        ApplicabilityState::Excluded
    } else if hard_unknown {
        ApplicabilityState::Unresolved
    } else {
        ApplicabilityState::Applicable
    };
    result
}
