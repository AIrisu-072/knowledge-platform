//! Typed Source coverage preflight for Document metadata versus unavailable body extraction.

use search_application::SearchError;
use search_application::discovery_service::DiscoveryService;
use search_core::discovery::{DiscoveryRequest, DiscoveryResult, GapReason, InformationGap};
use search_core::evidence::EvidenceSufficiency;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentCoverageRequirement {
    TitleAndPermittedMetadata,
    BodyRequired,
}

pub struct DocumentCoveragePreflight;

impl DocumentCoveragePreflight {
    pub async fn discover(
        service: &DiscoveryService<'_>,
        request: DiscoveryRequest,
        coverage: DocumentCoverageRequirement,
    ) -> Result<DiscoveryResult, SearchError> {
        match coverage {
            DocumentCoverageRequirement::TitleAndPermittedMetadata => {
                service.discover(request).await
            }
            DocumentCoverageRequirement::BodyRequired => Ok(DiscoveryResult {
                discovery_evaluation_id: request.temporal_context.evaluation_id,
                need: request.need,
                qualified_resources: Vec::new(),
                evidence_set: Vec::new(),
                evidence_sufficiency: EvidenceSufficiency::Unresolved,
                unresolved_gaps: vec![InformationGap::new(
                    "document.body.search_extraction",
                    GapReason::UnsupportedCoverage,
                    true,
                )],
                rejected_candidates: Vec::new(),
                source_trace: Vec::new(),
                retrieval_trace: Vec::new(),
                qualification_trace: Vec::new(),
            }),
        }
    }
}
