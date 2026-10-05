use search_application::ports::{
    BoxFuture, DirectoryRetrieverPort, StructuredFacetFilter, StructuredFacetOutcome,
    StructuredRetrievalHit, StructuredRetrieverPort,
};
use search_core::discovery::{CandidateIdentityClass, DiscoveryRequest, FederatedCandidate};
use search_core::fact::FactSet;
use search_core::predicate::{
    ConceptResolver, Operand, PredicateEvaluator, PredicateExpr, TruthValue,
};
use search_core::profile::FacetState;
use search_core::projection::{CompiledResourceProjection, ProjectionGenerationKey};

use crate::store::MemoryProjectionStore;

fn candidate(
    key: ProjectionGenerationKey,
    projection: &CompiledResourceProjection,
    method: &str,
) -> FederatedCandidate {
    let id = projection.directory.resource_ref;
    let mut candidate = FederatedCandidate::new(
        format!("{}:{}", key.source_id.as_uuid(), id.as_uuid()),
        CandidateIdentityClass::DurableResource,
        key.source_id,
        method,
    );
    candidate.resource_ref = Some(id);
    candidate.retrieval_trace_ref = Some(format!(
        "{}:{}",
        key.source_id.as_uuid(),
        key.generation_id.as_uuid()
    ));
    // AccessProjection is only prefilter metadata. This candidate never claims
    // final authorization; CurrentAccessEvaluatorPort owns that decision.
    candidate
}

fn relevant_kind(projection: &CompiledResourceProjection, request: &DiscoveryRequest) -> bool {
    !matches!(
        projection.directory.kind,
        search_core::resource::ResourceKind::Document
            | search_core::resource::ResourceKind::FolderPlacement
    ) && (request.need.required_resource_types.is_empty()
        || request
            .need
            .required_resource_types
            .contains(&projection.directory.kind))
}

impl DirectoryRetrieverPort for MemoryProjectionStore {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        Box::pin(async move {
            let state = self.read()?;
            let segment = Self::published(&state, generation)?;
            Ok(segment
                .resources
                .values()
                .filter(|projection| relevant_kind(projection, request))
                .map(|projection| candidate(generation, projection, "directory"))
                .collect())
        })
    }
}

impl StructuredRetrieverPort for MemoryProjectionStore {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        hard_filters: &'a [StructuredFacetFilter],
    ) -> BoxFuture<'a, Vec<StructuredRetrievalHit>> {
        Box::pin(async move {
            let state = self.read()?;
            let segment = Self::published(&state, generation)?;
            Ok(segment
                .resources
                .values()
                .filter(|projection| relevant_kind(projection, request))
                .map(|projection| StructuredRetrievalHit {
                    candidate: candidate(generation, projection, "structured"),
                    outcomes: hard_filters
                        .iter()
                        .map(|filter| facet_outcome(projection, filter))
                        .collect(),
                })
                .collect())
        })
    }
}

struct NoConceptResolution;

impl ConceptResolver for NoConceptResolution {
    fn same_concept(&self, _left: &str, _right: &str) -> TruthValue {
        TruthValue::Error
    }
    fn is_a(&self, _child: &str, _parent: &str) -> TruthValue {
        TruthValue::Error
    }
    fn descendant_of(&self, _child: &str, _ancestor: &str) -> TruthValue {
        TruthValue::Error
    }
}

fn facet_outcome(
    projection: &CompiledResourceProjection,
    filter: &StructuredFacetFilter,
) -> StructuredFacetOutcome {
    match projection.structured.typed_facets.get(&filter.facet) {
        None | Some(FacetState::Unknown) => StructuredFacetOutcome::Unknown,
        Some(FacetState::NotApplicable) => StructuredFacetOutcome::NotApplicable,
        Some(FacetState::Conflict) => StructuredFacetOutcome::Conflict,
        Some(FacetState::Known(actual)) => {
            let truth = PredicateEvaluator::evaluate(
                &PredicateExpr::Eq(
                    Operand::Value(actual.clone()),
                    Operand::Value(filter.expected.clone()),
                ),
                &FactSet::default(),
                &NoConceptResolution,
            );
            match truth {
                TruthValue::True => StructuredFacetOutcome::Match,
                TruthValue::False => StructuredFacetOutcome::Mismatch,
                TruthValue::Unknown | TruthValue::Error => StructuredFacetOutcome::Unknown,
            }
        }
    }
}
