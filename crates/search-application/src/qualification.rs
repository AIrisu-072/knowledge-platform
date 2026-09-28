//! Qualification orchestration depends on contract ports, never a graph backend.

use search_core::applicability::{ApplicabilityEvaluation, Discriminator, evaluate_applicability};
use search_core::discovery::FederatedCandidate;
use search_core::fact::FactSet;
use search_core::graph::GraphTraversalPlan;
use search_core::predicate::ConceptResolver;

use crate::error::SearchError;
use crate::ports::HyperGraphRetrieverPort;

pub struct QualificationService;

impl QualificationService {
    pub async fn candidates_from_graph(
        retriever: &dyn HyperGraphRetrieverPort,
        plan: &GraphTraversalPlan,
    ) -> Result<Vec<FederatedCandidate>, SearchError> {
        plan.validate()
            .map_err(|reason| SearchError::InvalidRequest(reason.into()))?;
        retriever.retrieve(plan).await
    }

    pub fn qualify_candidate(
        candidate: &FederatedCandidate,
        facts: &FactSet,
        discriminators: &[Discriminator],
        concepts: &dyn ConceptResolver,
    ) -> ApplicabilityEvaluation {
        evaluate_applicability(candidate, facts, discriminators, concepts)
    }
}
