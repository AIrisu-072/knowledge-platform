//! Candidate-group contrast reuses deterministic applicability gates.

use serde::{Deserialize, Serialize};

use crate::applicability::{
    ApplicabilityEvaluation, ApplicabilityState, Discriminator, evaluate_applicability,
};
use crate::discovery::FederatedCandidate;
use crate::fact::FactSet;
use crate::id::ResourceId;
use crate::predicate::ConceptResolver;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContrastSet {
    pub contrast_set_id: String,
    pub members: Vec<ResourceId>,
    pub discriminators: Vec<Discriminator>,
}

impl ContrastSet {
    pub fn new(
        contrast_set_id: impl Into<String>,
        members: Vec<ResourceId>,
        discriminators: Vec<Discriminator>,
    ) -> Self {
        Self {
            contrast_set_id: contrast_set_id.into(),
            members,
            discriminators,
        }
    }
}

pub fn resolve_contrast(
    contrast_set: &ContrastSet,
    candidate: &FederatedCandidate,
    facts: &FactSet,
    concepts: &dyn ConceptResolver,
) -> ApplicabilityEvaluation {
    let mut result =
        evaluate_applicability(candidate, facts, &contrast_set.discriminators, concepts);
    if !candidate
        .resource_ref
        .is_some_and(|resource| contrast_set.members.contains(&resource))
    {
        result.state = ApplicabilityState::Invalid;
        result.reasons.push("candidate outside contrast set".into());
    }
    result
}
