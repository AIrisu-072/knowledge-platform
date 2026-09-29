//! Budgeted Probe and progressive materialization at the Source boundary.

use std::fmt;
use std::io::{self, Write};

use search_core::discovery::{FederatedCandidate, InformationGap};
use search_core::fact::FactSet;
use search_core::id::{ResourceId, SourceId};
use search_core::materialization::{
    MaterializationPolicy, MaterializationState, ProbeCompletenessSemantics,
    ProbeExecutionLocation, ProbeOutcome, ProbeQueryMode, ProbeReturnType,
    ProviderContentPermission,
};
use search_core::observation::Coverage;
use search_core::resource::ResourceKind;

use crate::error::SearchError;
use crate::ports::{
    AccessDecision, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
    CurrentMaterializationStatePort, CurrentSourcePolicyPort, MaterializationReceipt,
    MaterializationRequest, MaterializerPort, ProbePort,
};

/// Unknown dimensions remain `None`; no action treats them as free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceCostEstimate {
    pub content_bytes: Option<u64>,
    pub latency_ms: Option<u64>,
    pub remote_calls: Option<u64>,
    pub monetary_cost_minor_units: Option<i64>,
    pub currency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializationBudget {
    pub max_content_bytes: u64,
    pub max_latency_ms: u64,
    pub max_remote_calls: u64,
    pub max_monetary_cost_minor_units: i64,
    pub currency: String,
    /// Direct full reads may bypass intermediate stages only below this limit.
    pub direct_full_max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeBudget {
    pub max_content_bytes: u64,
    pub max_latency_ms: u64,
    pub max_remote_calls: u64,
    pub max_monetary_cost_minor_units: i64,
    pub currency: String,
}

impl From<MaterializationBudget> for ProbeBudget {
    fn from(value: MaterializationBudget) -> Self {
        Self {
            max_content_bytes: value.max_content_bytes,
            max_latency_ms: value.max_latency_ms,
            max_remote_calls: value.max_remote_calls,
            max_monetary_cost_minor_units: value.max_monetary_cost_minor_units,
            currency: value.currency,
        }
    }
}

impl ResourceCostEstimate {
    fn fits(
        &self,
        max_content_bytes: u64,
        max_latency_ms: u64,
        max_remote_calls: u64,
        max_monetary_cost_minor_units: i64,
        currency: &str,
    ) -> bool {
        let (
            Some(content_bytes),
            Some(latency_ms),
            Some(remote_calls),
            Some(monetary_cost_minor_units),
            Some(estimated_currency),
        ) = (
            self.content_bytes,
            self.latency_ms,
            self.remote_calls,
            self.monetary_cost_minor_units,
            self.currency.as_deref(),
        )
        else {
            return false;
        };
        !currency.is_empty()
            && estimated_currency == currency
            && monetary_cost_minor_units >= 0
            && max_monetary_cost_minor_units >= 0
            && content_bytes <= max_content_bytes
            && latency_ms <= max_latency_ms
            && remote_calls <= max_remote_calls
            && monetary_cost_minor_units <= max_monetary_cost_minor_units
    }

    fn fits_materialization(&self, budget: &MaterializationBudget) -> bool {
        self.fits(
            budget.max_content_bytes,
            budget.max_latency_ms,
            budget.max_remote_calls,
            budget.max_monetary_cost_minor_units,
            &budget.currency,
        )
    }

    fn fits_probe(&self, budget: &ProbeBudget) -> bool {
        self.fits(
            budget.max_content_bytes,
            budget.max_latency_ms,
            budget.max_remote_calls,
            budget.max_monetary_cost_minor_units,
            &budget.currency,
        )
    }
}

pub struct MaterializationPlanInput {
    pub current: MaterializationState,
    pub policy: MaterializationPolicy,
    pub access: AccessDecision,
    pub provider_permission: ProviderContentPermission,
    pub has_unresolved_discriminator: bool,
    pub probe: Option<ProbePlanInput>,
    pub estimate: ResourceCostEstimate,
    pub budget: MaterializationBudget,
    pub allow_direct_full: bool,
}

pub struct ProbePlanInput {
    pub source_ref: SourceId,
    pub resource_kind: ResourceKind,
    pub facet: String,
    pub gap: InformationGap,
    pub capability: ProbeCapability,
    pub provider_allows_probe: bool,
    pub budget: ProbeBudget,
}

impl ProbePlanInput {
    fn is_eligible(&self) -> bool {
        self.provider_allows_probe
            && self
                .capability
                .covers(self.source_ref, self.resource_kind, &self.facet, &self.gap)
            && self.capability.estimated_cost.fits_probe(&self.budget)
    }
}

pub struct MaterializationPlanner;

impl MaterializationPlanner {
    pub fn next_state(input: &MaterializationPlanInput) -> MaterializationState {
        if input.access != AccessDecision::Allowed {
            return input.current;
        }
        let can_probe = input
            .probe
            .as_ref()
            .is_some_and(ProbePlanInput::is_eligible);
        let target = match input.policy {
            MaterializationPolicy::ReferenceOnly => input.current,
            MaterializationPolicy::InlineFull => {
                let skips_probe = input.current < MaterializationState::Probed;
                if (!skips_probe
                    || (input.allow_direct_full
                        && input
                            .estimate
                            .content_bytes
                            .is_some_and(|bytes| bytes <= input.budget.direct_full_max_bytes)))
                    && input
                        .provider_permission
                        .permits(MaterializationState::FullContent)
                    && input.estimate.fits_materialization(&input.budget)
                {
                    MaterializationState::FullContent
                } else {
                    MaterializationState::Metadata
                }
            }
            MaterializationPolicy::DiscriminativeFirst => {
                if input.has_unresolved_discriminator && can_probe {
                    MaterializationState::Probed
                } else {
                    MaterializationState::Metadata
                }
            }
            MaterializationPolicy::TargetedFragment => {
                if !input.has_unresolved_discriminator {
                    MaterializationState::Metadata
                } else if input
                    .provider_permission
                    .permits(MaterializationState::Fragment)
                    && input.estimate.fits_materialization(&input.budget)
                {
                    MaterializationState::Fragment
                } else if can_probe {
                    MaterializationState::Probed
                } else {
                    MaterializationState::Metadata
                }
            }
        };
        if target > input.current
            && target != MaterializationState::Probed
            && !input.estimate.fits_materialization(&input.budget)
        {
            return input.current;
        }
        if target == MaterializationState::Probed || input.provider_permission.permits(target) {
            input.current.max(target)
        } else {
            input.current
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeCapability {
    pub source_ref: SourceId,
    pub probe_type: String,
    pub supported_resource_types: Vec<ResourceKind>,
    pub supported_facets: Vec<String>,
    pub query_mode: ProbeQueryMode,
    pub return_types: Vec<ProbeReturnType>,
    pub completeness_semantics: ProbeCompletenessSemantics,
    pub location: ProbeExecutionLocation,
    pub coverage: Coverage,
    pub estimated_cost: ResourceCostEstimate,
}

impl ProbeCapability {
    fn covers(
        &self,
        source_ref: SourceId,
        resource_kind: ResourceKind,
        facet: &str,
        gap: &InformationGap,
    ) -> bool {
        self.source_ref == source_ref
            && !self.probe_type.is_empty()
            && self.location != ProbeExecutionLocation::None
            && self.supported_resource_types.contains(&resource_kind)
            && !facet.is_empty()
            && facet == gap.required_fact
            && self
                .supported_facets
                .iter()
                .any(|supported| supported == facet)
            && self.query_mode == ProbeQueryMode::FacetExact
            && self.return_types.contains(&ProbeReturnType::Facts)
            && self.completeness_semantics != ProbeCompletenessSemantics::Unknown
            && matches!(
                self.coverage,
                Coverage::QueryResult | Coverage::DirectLookup
            )
            && !(self.location == ProbeExecutionLocation::Provider
                && self.estimated_cost.remote_calls == Some(0))
    }
}

pub struct ProbeRequest {
    pub candidate: FederatedCandidate,
    pub gap: InformationGap,
    pub facet: String,
    pub access_context: String,
    pub access: AccessDecision,
    pub provider_allows_probe: bool,
    pub provider_permission: ProviderContentPermission,
    pub retention_mode: search_core::source::RetentionMode,
    pub budget: ProbeBudget,
    pub capability: ProbeCapability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeEvidence {
    pub source_ref: SourceId,
    pub candidate_id: String,
    pub resource_ref: Option<ResourceId>,
    pub facet: String,
    pub probe_type: String,
    pub query_mode: ProbeQueryMode,
    pub completeness_semantics: ProbeCompletenessSemantics,
    pub provenance: String,
    pub coverage: Coverage,
}

#[derive(Clone, PartialEq, Eq)]
enum ProbeResultKind {
    Found {
        facts: FactSet,
        evidence: ProbeEvidence,
    },
    NotFoundByProbe {
        evidence: ProbeEvidence,
    },
    Unsupported {
        reason_code: String,
    },
    Failed {
        reason_code: String,
    },
}

/// Only `Found` can carry Facts. A probe miss cannot become a false Fact or
/// an absence claim through this contract.
#[derive(Clone, PartialEq, Eq)]
pub struct ProbeResult(ProbeResultKind);

impl ProbeResult {
    pub fn found(facts: FactSet, evidence: ProbeEvidence) -> Self {
        Self(ProbeResultKind::Found { facts, evidence })
    }

    pub fn not_found_by_probe(evidence: ProbeEvidence) -> Self {
        Self(ProbeResultKind::NotFoundByProbe { evidence })
    }

    pub fn unsupported(reason_code: impl Into<String>) -> Self {
        Self(ProbeResultKind::Unsupported {
            reason_code: reason_code.into(),
        })
    }

    pub fn failed(reason_code: impl Into<String>) -> Self {
        Self(ProbeResultKind::Failed {
            reason_code: reason_code.into(),
        })
    }

    pub const fn outcome(&self) -> ProbeOutcome {
        match self.0 {
            ProbeResultKind::Found { .. } => ProbeOutcome::Found,
            ProbeResultKind::NotFoundByProbe { .. } => ProbeOutcome::NotFoundByProbe,
            ProbeResultKind::Unsupported { .. } => ProbeOutcome::Unsupported,
            ProbeResultKind::Failed { .. } => ProbeOutcome::Failed,
        }
    }

    pub fn facts(&self) -> Option<&FactSet> {
        match &self.0 {
            ProbeResultKind::Found { facts, .. } => Some(facts),
            _ => None,
        }
    }

    pub fn evidence(&self) -> Option<&ProbeEvidence> {
        match &self.0 {
            ProbeResultKind::Found { evidence, .. }
            | ProbeResultKind::NotFoundByProbe { evidence } => Some(evidence),
            _ => None,
        }
    }

    pub fn reason_code(&self) -> Option<&str> {
        match &self.0 {
            ProbeResultKind::Unsupported { reason_code }
            | ProbeResultKind::Failed { reason_code } => Some(reason_code),
            _ => None,
        }
    }

    /// Count the encoded response, including evidence and error reason, without
    /// allocating another copy of potentially large provider-supplied facts.
    fn fits_response_budget(&self, max_content_bytes: u64) -> bool {
        let mut writer = ResponseByteLimit {
            remaining: max_content_bytes,
        };
        match &self.0 {
            ProbeResultKind::Found { facts, evidence } => serde_json::to_writer(
                &mut writer,
                &(
                    "FOUND",
                    facts,
                    evidence.source_ref,
                    &evidence.candidate_id,
                    evidence.resource_ref,
                    &evidence.facet,
                    &evidence.probe_type,
                    evidence.query_mode,
                    evidence.completeness_semantics,
                    &evidence.provenance,
                    evidence.coverage,
                ),
            ),
            ProbeResultKind::NotFoundByProbe { evidence } => serde_json::to_writer(
                &mut writer,
                &(
                    "NOT_FOUND_BY_PROBE",
                    evidence.source_ref,
                    &evidence.candidate_id,
                    evidence.resource_ref,
                    &evidence.facet,
                    &evidence.probe_type,
                    evidence.query_mode,
                    evidence.completeness_semantics,
                    &evidence.provenance,
                    evidence.coverage,
                ),
            ),
            ProbeResultKind::Unsupported { reason_code } => {
                serde_json::to_writer(&mut writer, &("UNSUPPORTED", reason_code))
            }
            ProbeResultKind::Failed { reason_code } => {
                serde_json::to_writer(&mut writer, &("FAILED", reason_code))
            }
        }
        .is_ok()
    }
}

struct ResponseByteLimit {
    remaining: u64,
}

impl Write for ResponseByteLimit {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let len = u64::try_from(bytes.len())
            .map_err(|_| io::Error::other("probe response exceeds byte budget"))?;
        if len > self.remaining {
            return Err(io::Error::other("probe response exceeds byte budget"));
        }
        self.remaining -= len;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl fmt::Debug for ProbeResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProbeResult")
            .field("outcome", &self.outcome())
            .finish_non_exhaustive()
    }
}

pub struct MaterializationService;

impl MaterializationService {
    pub async fn probe(
        port: &dyn ProbePort,
        access_evaluator: &dyn CurrentCandidateAccessEvaluatorPort,
        source_policy: &dyn CurrentSourcePolicyPort,
        request: &ProbeRequest,
    ) -> Result<ProbeResult, SearchError> {
        if request.access != AccessDecision::Allowed
            || !request.provider_allows_probe
            || request.access_context.is_empty()
        {
            return Err(SearchError::InvalidRequest(
                "probe access is not authorized".into(),
            ));
        }
        if request.candidate.candidate_id.is_empty() {
            return Err(SearchError::InvalidRequest(
                "probe candidate identity is missing".into(),
            ));
        }
        if !request
            .capability
            .estimated_cost
            .fits_probe(&request.budget)
        {
            return Err(SearchError::InvalidRequest(
                "probe cost is unknown or exceeds budget".into(),
            ));
        }
        if !matches!(
            access_evaluator
                .evaluate(&request.candidate, &request.access_context)
                .await,
            Ok(AccessDecision::Allowed)
        ) {
            return Err(SearchError::InvalidRequest(
                "current probe access is not authorized".into(),
            ));
        }
        let current_policy = source_policy
            .for_candidate(&request.candidate, &request.access_context)
            .await?
            .ok_or_else(|| {
                SearchError::InvalidRequest("current Source policy is unknown".into())
            })?;
        if !current_policy.probe_allowed
            || !request.provider_allows_probe
            || request.provider_permission != current_policy.provider_permission
            || request.retention_mode != current_policy.retention_mode
        {
            return Err(SearchError::InvalidRequest(
                "probe is not permitted by current Source policy".into(),
            ));
        }
        if !request.capability.covers(
            request.candidate.source_ref,
            current_policy.resource_kind,
            &request.facet,
            &request.gap,
        ) {
            return Err(SearchError::InvalidRequest(
                "probe capability does not cover the gap".into(),
            ));
        }
        let result = port.probe(request).await?;
        if let Some(evidence) = result.evidence()
            && (evidence.source_ref != request.candidate.source_ref
                || evidence.candidate_id != request.candidate.candidate_id
                || evidence.resource_ref != request.candidate.resource_ref
                || evidence.facet != request.facet
                || evidence.probe_type != request.capability.probe_type
                || evidence.query_mode != request.capability.query_mode
                || evidence.completeness_semantics != request.capability.completeness_semantics
                || evidence.coverage != request.capability.coverage
                || evidence.provenance.is_empty())
        {
            return Err(SearchError::OperationFailed(
                "probe evidence is inconsistent".into(),
            ));
        }
        if result.outcome() == ProbeOutcome::Found
            && !result
                .facts()
                .is_some_and(|facts| facts.len() == 1 && facts.contains(&request.facet))
        {
            return Err(SearchError::OperationFailed(
                "probe facts do not match the targeted facet".into(),
            ));
        }
        if !result.fits_response_budget(request.budget.max_content_bytes) {
            return Err(SearchError::OperationFailed(
                "probe response exceeds byte budget".into(),
            ));
        }
        Ok(result)
    }

    pub async fn materialize(
        current_state_port: &dyn CurrentMaterializationStatePort,
        port: &dyn MaterializerPort,
        access_evaluator: &dyn CurrentAccessEvaluatorPort,
        source_policy: &dyn CurrentSourcePolicyPort,
        request: &MaterializationRequest,
    ) -> Result<MaterializationReceipt, SearchError> {
        if request.access != AccessDecision::Allowed || request.access_context.is_empty() {
            return Err(SearchError::InvalidRequest(
                "materialization access is not authorized".into(),
            ));
        }
        if request.binding.validate().is_err()
            || !request.provider_permission.permits(request.requested_state)
            || request.requested_state == MaterializationState::Probed
            || request.requested_state == MaterializationState::ReferenceOnly
        {
            return Err(SearchError::InvalidRequest(
                "materialization stage is not permitted".into(),
            ));
        }
        if !request.estimate.fits_materialization(&request.budget) {
            return Err(SearchError::InvalidRequest(
                "materialization cost is unknown or exceeds budget".into(),
            ));
        }
        if !matches!(
            access_evaluator
                .evaluate(request.resource_ref, &request.access_context)
                .await,
            Ok(AccessDecision::Allowed)
        ) {
            return Err(SearchError::InvalidRequest(
                "current materialization access is not authorized".into(),
            ));
        }
        let current_policy = source_policy
            .for_resource(
                request.binding.source_ref,
                request.resource_ref,
                &request.binding,
                &request.access_context,
            )
            .await?
            .ok_or_else(|| {
                SearchError::InvalidRequest("current Source policy is unknown".into())
            })?;
        if current_policy.provider_permission != request.provider_permission
            || current_policy.retention_mode != request.retention_mode
            || !current_policy
                .provider_permission
                .permits(request.requested_state)
        {
            return Err(SearchError::InvalidRequest(
                "materialization is not permitted by current Source policy".into(),
            ));
        }
        let current_state = match current_state_port
            .for_resource(
                request.binding.source_ref,
                request.resource_ref,
                &request.binding,
                &request.access_context,
            )
            .await
        {
            Ok(Some(state)) => state,
            Ok(None) | Err(_) => {
                return Err(SearchError::InvalidRequest(
                    "current materialization state is unavailable".into(),
                ));
            }
        };
        if current_state != request.current_state
            || !current_state.can_advance_to(request.requested_state)
        {
            return Err(SearchError::InvalidRequest(
                "materialization stage conflicts with current session state".into(),
            ));
        }
        let direct_full = request.requested_state == MaterializationState::FullContent
            && current_state < MaterializationState::Probed;
        if direct_full
            && (!request.allow_direct_full
                || !request
                    .estimate
                    .content_bytes
                    .is_some_and(|bytes| bytes <= request.budget.direct_full_max_bytes))
        {
            return Err(SearchError::InvalidRequest(
                "direct full materialization is not permitted".into(),
            ));
        }
        let mut receipt = port.materialize(request).await?;
        if receipt.resource_ref != request.resource_ref
            || receipt.binding != request.binding
            || receipt.retention_mode != request.retention_mode
            || receipt.requested_state != request.requested_state
            || !current_state.can_advance_to(receipt.achieved_state)
            || !receipt
                .achieved_state
                .can_advance_to(request.requested_state)
            || !request.provider_permission.permits(receipt.achieved_state)
            || receipt.achieved_state == MaterializationState::Probed
            || request
                .binding
                .content_digest
                .as_ref()
                .is_some_and(|digest| receipt.content_digest.as_deref() != Some(digest.as_str()))
        {
            return Err(SearchError::OperationFailed(
                "materialization receipt is inconsistent".into(),
            ));
        }
        let has_content = receipt.content.is_some();
        if has_content
            != matches!(
                receipt.achieved_state,
                MaterializationState::Fragment | MaterializationState::FullContent
            )
            || receipt.content.as_ref().is_some_and(|content| {
                content.len() as u64 > request.budget.max_content_bytes
                    || (direct_full && content.len() as u64 > request.budget.direct_full_max_bytes)
            })
        {
            return Err(SearchError::OperationFailed(
                "materialization content exceeds permitted stage or budget".into(),
            ));
        }
        receipt.policy_verified = true;
        Ok(receipt)
    }
}
