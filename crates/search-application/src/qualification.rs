//! Qualification orchestration depends on contract ports, never a graph backend.

use search_core::applicability::{ApplicabilityEvaluation, Discriminator, evaluate_applicability};
use search_core::fact::FactSet;
use search_core::graph::GraphTraversalPlan;
use search_core::predicate::ConceptResolver;
use search_core::projection::ProjectionGenerationKey;

use crate::error::SearchError;
use crate::ports::{GraphRetrievalResult, HyperGraphRetrieverPort};

pub struct QualificationService;

impl QualificationService {
    /// Fetch raw adapter output for the executor. Per-hit data is untrusted
    /// until the executor has checked current candidate and path access.
    pub(crate) async fn candidates_from_graph(
        retriever: &dyn HyperGraphRetrieverPort,
        generation: ProjectionGenerationKey,
        plan: &GraphTraversalPlan,
    ) -> Result<GraphRetrievalResult, SearchError> {
        plan.validate()
            .map_err(|reason| SearchError::InvalidRequest(reason.into()))?;
        retriever.retrieve(generation, plan).await
    }

    pub fn qualify_candidate(
        candidate: &search_core::discovery::FederatedCandidate,
        facts: &FactSet,
        discriminators: &[Discriminator],
        concepts: &dyn ConceptResolver,
    ) -> ApplicabilityEvaluation {
        evaluate_applicability(candidate, facts, discriminators, concepts)
    }
}
