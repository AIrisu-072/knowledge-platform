//! Current-capability targeted Probe execution for a blocking unknown gap.

use search_core::discovery::{CandidateIdentityClass, FederatedCandidate, InformationGap};

use crate::error::SearchError;
use crate::materialization::{MaterializationService, ProbeBudget, ProbeRequest, ProbeResult};
use crate::ports::{
    AccessDecision, CurrentCandidateAccessEvaluatorPort, CurrentSourcePolicyPort,
    ProbeCapabilityCatalogPort, ProbePort,
};

/// The budget is supplied by trusted Discovery/session code, never by a
/// candidate, public request, or provider response.
/// In v0 the identity class and stable candidate ID are supplied by a trusted
/// Source adapter and revalidated at call time. Accepting caller self-reported
/// identity would require a Source-owned target token instead.
pub struct ProbeExecutionInput {
    pub candidate: FederatedCandidate,
    pub gap: InformationGap,
    pub facet: String,
    pub access_context: String,
    pub budget: ProbeBudget,
}

pub struct ProbeExecutionService;

impl ProbeExecutionService {
    pub async fn execute(
        port: &dyn ProbePort,
        access_evaluator: &dyn CurrentCandidateAccessEvaluatorPort,
        capability_catalog: &dyn ProbeCapabilityCatalogPort,
        source_policy: &dyn CurrentSourcePolicyPort,
        input: &ProbeExecutionInput,
    ) -> Result<ProbeResult, SearchError> {
        if input.access_context.is_empty() {
            return Err(SearchError::InvalidRequest(
                "current probe access is not authorized".into(),
            ));
        }
        // Nothing about Source or capability may be queried before current
        // candidate access is allowed. All denial paths use one response.
        if !matches!(
            access_evaluator
                .evaluate(&input.candidate, &input.access_context)
                .await,
            Ok(AccessDecision::Allowed)
        ) {
            return Err(SearchError::InvalidRequest(
                "current probe access is not authorized".into(),
            ));
        }
        if input.candidate.identity_class == CandidateIdentityClass::EphemeralCandidate
            && input.candidate.resource_ref.is_none()
        {
            return Ok(ProbeResult::unsupported(
                "ephemeral-target-has-no-resource-id",
            ));
        }
        if input.candidate.candidate_id.is_empty()
            || !input.gap.blocking
            || input.facet.is_empty()
            || input.facet != input.gap.required_fact
        {
            return Err(SearchError::InvalidRequest(
                "probe requires an exact blocking gap".into(),
            ));
        }

        let Some(current) = capability_catalog
            .for_candidate(&input.candidate, &input.facet, &input.access_context)
            .await?
        else {
            return Ok(ProbeResult::unsupported("no-current-probe-capability"));
        };
        if current.candidate_id != input.candidate.candidate_id
            || current.resource_ref != input.candidate.resource_ref
            || current.facet != input.facet
            || current.capability.source_ref != input.candidate.source_ref
        {
            return Err(SearchError::InvalidRequest(
                "current probe capability does not match the target".into(),
            ));
        }

        let policy = source_policy
            .for_candidate(&input.candidate, &input.access_context)
            .await?
            .ok_or_else(|| {
                SearchError::InvalidRequest("current Source policy is unknown".into())
            })?;
        if !policy.probe_allowed {
            return Err(SearchError::InvalidRequest(
                "probe is not permitted by current Source policy".into(),
            ));
        }

        let request = ProbeRequest {
            candidate: input.candidate.clone(),
            gap: input.gap.clone(),
            facet: input.facet.clone(),
            access_context: input.access_context.clone(),
            access: AccessDecision::Allowed,
            provider_allows_probe: policy.probe_allowed,
            provider_permission: policy.provider_permission,
            retention_mode: policy.retention_mode,
            budget: input.budget.clone(),
            capability: current.capability,
        };
        MaterializationService::probe(port, access_evaluator, source_policy, &request).await
    }
}
