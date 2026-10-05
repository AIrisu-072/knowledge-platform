//! Typed Source coverage preflight for Document metadata versus body search.

use search_application::SearchError;
use search_application::content_scope::{BodySearchSpec, DiscoveryScope};
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
    /// `BodyRequired` runs only with a trusted request-scoped `BodySearchSpec`.
    /// A missing, malformed or unbound spec, or a spec on a metadata request,
    /// is a blocking gap with no retrieval rather than a partial result.
    pub async fn discover(
        service: &DiscoveryService<'_>,
        request: DiscoveryRequest,
        coverage: DocumentCoverageRequirement,
        body: Option<BodySearchSpec>,
    ) -> Result<DiscoveryResult, SearchError> {
        match (coverage, body) {
            (DocumentCoverageRequirement::TitleAndPermittedMetadata, None) => {
                service.discover(request).await
            }
            (DocumentCoverageRequirement::TitleAndPermittedMetadata, Some(_)) => {
                Ok(blocked(request, "document.body.scope_mismatch"))
            }
            (DocumentCoverageRequirement::BodyRequired, None) => {
                Ok(blocked(request, "document.body.search_spec_missing"))
            }
            (DocumentCoverageRequirement::BodyRequired, Some(spec)) => {
                if spec.validate().is_err() {
                    return Ok(blocked(request, "document.body.search_spec_invalid"));
                }
                // A selector may bind only a Claim this request itself requires.
                if spec
                    .exact_text_claim
                    .as_ref()
                    .is_some_and(|claim| !request.need.required_claims.contains(claim))
                {
                    return Ok(blocked(request, "document.body.selector_unbound"));
                }
                service
                    .discover_with_content_scope(request, DiscoveryScope::BodyRequired(spec))
                    .await
            }
        }
    }
}

fn blocked(request: DiscoveryRequest, gap: &str) -> DiscoveryResult {
    DiscoveryResult {
        discovery_evaluation_id: request.temporal_context.evaluation_id,
        need: request.need,
        qualified_resources: Vec::new(),
        evidence_set: Vec::new(),
        evidence_sufficiency: EvidenceSufficiency::Unresolved,
        unresolved_gaps: vec![InformationGap::new(
            gap,
            GapReason::UnsupportedCoverage,
            true,
        )],
        rejected_candidates: Vec::new(),
        source_trace: Vec::new(),
        retrieval_trace: Vec::new(),
        qualification_trace: Vec::new(),
    }
}
