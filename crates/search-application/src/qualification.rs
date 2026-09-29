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
    pub async fn candidates_from_graph(
        retriever: &dyn HyperGraphRetrieverPort,
        generation: ProjectionGenerationKey,
        plan: &GraphTraversalPlan,
    ) -> Result<GraphRetrievalResult, SearchError> {
        plan.validate()
            .map_err(|reason| SearchError::InvalidRequest(reason.into()))?;
        let result = retriever.retrieve(generation, plan).await?;
        if result.generation != generation {
            return Err(SearchError::OperationFailed(
                "graph retriever returned another generation".into(),
            ));
        }
        for hit in &result.hits {
            if hit.candidate.source_ref != generation.source_id {
                return Err(SearchError::OperationFailed(
                    "graph retriever returned another Source".into(),
                ));
            }
            if hit.paths.is_empty() || hit.paths.iter().any(|path| path.steps.is_empty()) {
                return Err(SearchError::OperationFailed(
                    "graph retriever returned a hit without path evidence".into(),
                ));
            }
        }
        Ok(result)
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
