use search_application::SearchError;
use search_application::materialization::{
    MaterializationBudget, MaterializationPlanInput, MaterializationPlanner,
    MaterializationService, ProbeBudget, ProbeCapability, ProbeEvidence, ProbePlanInput,
    ProbeRequest, ProbeResult, ResourceCostEstimate,
};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
    CurrentMaterializationStatePort, CurrentSourcePolicy, CurrentSourcePolicyPort,
    MaterializationReceipt, MaterializationRequest, MaterializerPort, ProbePort,
};
use search_core::binding::{BindingMode, RepresentationBinding};
use search_core::discovery::{
    CandidateIdentityClass, FederatedCandidate, GapReason, InformationGap,
};
use search_core::fact::{Fact, FactOrigin, FactSet};
use search_core::id::{
    BindingId, LogicalResourceId, RepresentationId, ResourceId, ResourceVersionId, SourceId,
};
use search_core::materialization::{
    MaterializationPolicy, MaterializationState, ProbeCompletenessSemantics,
    ProbeExecutionLocation, ProbeOutcome, ProbeQueryMode, ProbeReturnType,
    ProviderContentPermission,
};
use search_core::observation::Coverage;
use search_core::predicate::TypedValue;
use search_core::resource::ResourceKind;
use search_core::source::RetentionMode;
use std::sync::atomic::{AtomicUsize, Ordering};
use time::OffsetDateTime;
use uuid::Uuid;

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(1))
}

fn resource_id() -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(2))
}

fn candidate() -> FederatedCandidate {
    let mut candidate = FederatedCandidate::new(
        "candidate-1",
        CandidateIdentityClass::DurableResource,
        source_id(),
        "directory",
    );
    candidate.resource_ref = Some(resource_id());
    candidate
}

fn binding() -> RepresentationBinding {
    RepresentationBinding::new(
        BindingId::from_uuid(Uuid::from_u128(3)),
        LogicalResourceId::from_uuid(Uuid::from_u128(4)),
        RepresentationId::from_uuid(Uuid::from_u128(5)),
        source_id(),
        BindingMode::SessionSnapshot,
        OffsetDateTime::from_unix_timestamp(100).unwrap(),
    )
}

fn known_estimate() -> ResourceCostEstimate {
    ResourceCostEstimate {
        content_bytes: Some(12),
        latency_ms: Some(20),
        remote_calls: Some(1),
        monetary_cost_minor_units: Some(0),
        currency: Some("USD".into()),
    }
}

fn budget() -> MaterializationBudget {
    MaterializationBudget {
        max_content_bytes: 100,
        max_latency_ms: 100,
        max_remote_calls: 1,
        max_monetary_cost_minor_units: 10,
        currency: "USD".into(),
        direct_full_max_bytes: 16,
    }
}

struct FakeResourceAccess(AccessDecision);

impl CurrentAccessEvaluatorPort for FakeResourceAccess {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        _access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        assert_eq!(resource_ref, resource_id());
        Box::pin(async move { Ok(self.0) })
    }
}

struct FakeCurrentState(MaterializationState);

impl CurrentMaterializationStatePort for FakeCurrentState {
    fn for_resource<'a>(
        &'a self,
        source_ref: SourceId,
        resource_ref: ResourceId,
        binding: &'a RepresentationBinding,
        _access_context: &'a str,
    ) -> BoxFuture<'a, Option<MaterializationState>> {
        assert_eq!(source_ref, source_id());
        assert_eq!(resource_ref, resource_id());
        assert_eq!(binding.source_ref, source_ref);
        Box::pin(async move { Ok(Some(self.0)) })
    }
}

enum StateEvaluation {
    State(MaterializationState),
    Missing,
    Failed,
}

struct CountingCurrentState {
    evaluation: StateEvaluation,
    calls: AtomicUsize,
}

impl CurrentMaterializationStatePort for CountingCurrentState {
    fn for_resource<'a>(
        &'a self,
        _source_ref: SourceId,
        _resource_ref: ResourceId,
        _binding: &'a RepresentationBinding,
        _access_context: &'a str,
    ) -> BoxFuture<'a, Option<MaterializationState>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match self.evaluation {
                StateEvaluation::State(state) => Ok(Some(state)),
                StateEvaluation::Missing => Ok(None),
                StateEvaluation::Failed => {
                    Err(SearchError::SourceUnavailable("session unavailable".into()))
                }
            }
        })
    }
}

struct FakeCandidateAccess(AccessDecision);

impl CurrentCandidateAccessEvaluatorPort for FakeCandidateAccess {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        _access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        assert_eq!(candidate.source_ref, source_id());
        Box::pin(async move { Ok(self.0) })
    }
}

#[derive(Clone, Copy)]
enum AccessEvaluation {
    Decision(AccessDecision),
    SourceUnavailable,
    OperationFailed,
}

impl AccessEvaluation {
    fn result(self) -> Result<AccessDecision, SearchError> {
        match self {
            Self::Decision(decision) => Ok(decision),
            Self::SourceUnavailable => Err(SearchError::SourceUnavailable("not found".into())),
            Self::OperationFailed => Err(SearchError::OperationFailed("backend failed".into())),
        }
    }
}

struct VariableResourceAccess(AccessEvaluation);

impl CurrentAccessEvaluatorPort for VariableResourceAccess {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        _access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        assert_eq!(resource_ref, resource_id());
        Box::pin(async move { self.0.result() })
    }
}

struct VariableCandidateAccess(AccessEvaluation);

impl CurrentCandidateAccessEvaluatorPort for VariableCandidateAccess {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        _access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        assert_eq!(candidate.source_ref, source_id());
        Box::pin(async move { self.0.result() })
    }
}

struct FakeSourcePolicy(Option<CurrentSourcePolicy>);

impl CurrentSourcePolicyPort for FakeSourcePolicy {
    fn for_candidate<'a>(
        &'a self,
        _candidate: &'a FederatedCandidate,
        _access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        Box::pin(async move { Ok(self.0) })
    }

    fn for_resource<'a>(
        &'a self,
        source_ref: SourceId,
        resource_ref: ResourceId,
        binding: &'a RepresentationBinding,
        _access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        assert_eq!(source_ref, source_id());
        assert_eq!(resource_ref, resource_id());
        assert_eq!(binding.source_ref, source_ref);
        Box::pin(async move { Ok(self.0) })
    }
}

fn source_policy() -> FakeSourcePolicy {
    FakeSourcePolicy(Some(CurrentSourcePolicy {
        resource_kind: ResourceKind::Knowledge,
        provider_permission: ProviderContentPermission::FullContent,
        retention_mode: RetentionMode::NoRetention,
        probe_allowed: true,
    }))
}

struct CountingSourcePolicy {
    current: Option<CurrentSourcePolicy>,
    calls: AtomicUsize,
}

impl CurrentSourcePolicyPort for CountingSourcePolicy {
    fn for_candidate<'a>(
        &'a self,
        _candidate: &'a FederatedCandidate,
        _access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(self.current) })
    }

    fn for_resource<'a>(
        &'a self,
        _source_ref: SourceId,
        _resource_ref: ResourceId,
        _binding: &'a RepresentationBinding,
        _access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(self.current) })
    }
}

fn counting_source_policy(present: bool) -> CountingSourcePolicy {
    CountingSourcePolicy {
        current: source_policy().0.filter(|_| present),
        calls: AtomicUsize::new(0),
    }
}

fn probe_plan_input() -> ProbePlanInput {
    let request = probe_request();
    ProbePlanInput {
        source_ref: request.candidate.source_ref,
        resource_kind: ResourceKind::Knowledge,
        facet: request.facet,
        gap: request.gap,
        capability: request.capability,
        provider_allows_probe: true,
        budget: request.budget,
    }
}

#[test]
fn direct_full_requires_explicit_permission_and_known_bounded_cost() {
    let mut input = MaterializationPlanInput {
        current: MaterializationState::ReferenceOnly,
        policy: MaterializationPolicy::InlineFull,
        access: AccessDecision::Allowed,
        provider_permission: ProviderContentPermission::FullContent,
        has_unresolved_discriminator: true,
        probe: None,
        estimate: known_estimate(),
        budget: budget(),
        allow_direct_full: false,
    };
    assert_ne!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::FullContent
    );
    input.allow_direct_full = true;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::FullContent
    );
    input.estimate.monetary_cost_minor_units = None;
    assert_ne!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::FullContent
    );
    input.estimate = known_estimate();
    input.estimate.content_bytes = Some(17);
    assert_ne!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::FullContent
    );
    input.estimate = known_estimate();
    input.provider_permission = ProviderContentPermission::Fragment;
    assert_ne!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::FullContent
    );
    input.provider_permission = ProviderContentPermission::FullContent;
    input.access = AccessDecision::Unknown;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::ReferenceOnly
    );
    input.access = AccessDecision::Allowed;
    input.current = MaterializationState::Probed;
    input.estimate.content_bytes = Some(17);
    input.allow_direct_full = false;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::FullContent
    );
}

#[test]
fn policy_stages_are_monotonic_and_target_gaps() {
    assert!(MaterializationState::ReferenceOnly.can_advance_to(MaterializationState::FullContent));
    assert!(!MaterializationState::Fragment.can_advance_to(MaterializationState::Metadata));
    let mut input = MaterializationPlanInput {
        current: MaterializationState::Metadata,
        policy: MaterializationPolicy::DiscriminativeFirst,
        access: AccessDecision::Allowed,
        provider_permission: ProviderContentPermission::Fragment,
        has_unresolved_discriminator: true,
        probe: Some(probe_plan_input()),
        estimate: known_estimate(),
        budget: budget(),
        allow_direct_full: false,
    };
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Probed
    );
    input.policy = MaterializationPolicy::TargetedFragment;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Fragment
    );
    input.has_unresolved_discriminator = false;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.policy = MaterializationPolicy::ReferenceOnly;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.current = MaterializationState::ReferenceOnly;
    input.policy = MaterializationPolicy::InlineFull;
    input.estimate.latency_ms = None;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::ReferenceOnly
    );
}

#[test]
fn planner_only_probes_with_current_grant_matching_capability_and_bounded_cost() {
    let mut input = MaterializationPlanInput {
        current: MaterializationState::Metadata,
        policy: MaterializationPolicy::DiscriminativeFirst,
        access: AccessDecision::Allowed,
        provider_permission: ProviderContentPermission::Metadata,
        has_unresolved_discriminator: true,
        probe: Some(probe_plan_input()),
        estimate: known_estimate(),
        budget: budget(),
        allow_direct_full: false,
    };
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Probed
    );

    input.probe.as_mut().unwrap().provider_allows_probe = false;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.probe = Some(probe_plan_input());
    input
        .probe
        .as_mut()
        .unwrap()
        .capability
        .estimated_cost
        .remote_calls = None;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.probe = Some(probe_plan_input());
    input
        .probe
        .as_mut()
        .unwrap()
        .capability
        .estimated_cost
        .remote_calls = Some(0);
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.probe = Some(probe_plan_input());
    input.probe.as_mut().unwrap().budget.max_remote_calls = 0;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.probe = Some(probe_plan_input());
    input
        .probe
        .as_mut()
        .unwrap()
        .capability
        .supported_resource_types
        .clear();
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.probe = Some(probe_plan_input());
    input.probe.as_mut().unwrap().capability.query_mode = ProbeQueryMode::FreeText;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.probe = Some(probe_plan_input());
    input.probe.as_mut().unwrap().capability.return_types =
        vec![ProbeReturnType::CandidateReferences];
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.probe = Some(probe_plan_input());
    input
        .probe
        .as_mut()
        .unwrap()
        .capability
        .completeness_semantics = ProbeCompletenessSemantics::Unknown;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
    input.probe = Some(probe_plan_input());
    input.probe.as_mut().unwrap().gap.required_fact = "other".into();
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );

    input.probe = Some(probe_plan_input());
    input.policy = MaterializationPolicy::TargetedFragment;
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Probed
    );
    input
        .probe
        .as_mut()
        .unwrap()
        .capability
        .supported_facets
        .clear();
    assert_eq!(
        MaterializationPlanner::next_state(&input),
        MaterializationState::Metadata
    );
}

struct FakeProbe {
    calls: AtomicUsize,
    result: ProbeResult,
}

impl ProbePort for FakeProbe {
    fn probe<'a>(&'a self, _request: &'a ProbeRequest) -> BoxFuture<'a, ProbeResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(self.result.clone()) })
    }
}

fn probe_request() -> ProbeRequest {
    ProbeRequest {
        candidate: candidate(),
        gap: InformationGap::new("suitable", GapReason::MissingFact, true),
        facet: "suitable".into(),
        access_context: "principal-1".into(),
        access: AccessDecision::Allowed,
        provider_allows_probe: true,
        provider_permission: ProviderContentPermission::FullContent,
        retention_mode: RetentionMode::NoRetention,
        budget: ProbeBudget {
            max_content_bytes: 512,
            ..ProbeBudget::from(budget())
        },
        capability: ProbeCapability {
            source_ref: source_id(),
            probe_type: "facet-query".into(),
            supported_resource_types: vec![ResourceKind::Knowledge],
            supported_facets: vec!["suitable".into()],
            query_mode: ProbeQueryMode::FacetExact,
            return_types: vec![ProbeReturnType::Facts],
            completeness_semantics: ProbeCompletenessSemantics::Partial,
            location: ProbeExecutionLocation::Provider,
            coverage: Coverage::QueryResult,
            estimated_cost: known_estimate(),
        },
    }
}

fn evidence() -> ProbeEvidence {
    ProbeEvidence {
        source_ref: source_id(),
        candidate_id: "candidate-1".into(),
        resource_ref: Some(resource_id()),
        facet: "suitable".into(),
        probe_type: "facet-query".into(),
        query_mode: ProbeQueryMode::FacetExact,
        completeness_semantics: ProbeCompletenessSemantics::Partial,
        provenance: "provider-query".into(),
        coverage: Coverage::QueryResult,
    }
}

#[tokio::test]
async fn probe_no_hit_has_no_false_fact_or_absence() {
    let port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::not_found_by_probe(evidence()),
    };
    let result = MaterializationService::probe(
        &port,
        &FakeCandidateAccess(AccessDecision::Allowed),
        &source_policy(),
        &probe_request(),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome(), ProbeOutcome::NotFoundByProbe);
    assert!(result.facts().is_none());
    assert_eq!(result.evidence().unwrap().coverage, Coverage::QueryResult);
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn probe_checks_access_capability_and_budget_before_port_call() {
    let port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::unsupported("unsupported"),
    };
    let mut request = probe_request();
    request.access = AccessDecision::Unknown;
    assert!(
        MaterializationService::probe(
            &port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &source_policy(),
            &request
        )
        .await
        .is_err()
    );
    request.access = AccessDecision::Allowed;
    request.capability.supported_facets.clear();
    assert!(
        MaterializationService::probe(
            &port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &source_policy(),
            &request
        )
        .await
        .is_err()
    );
    request.capability.supported_facets.push("suitable".into());
    request.capability.estimated_cost.monetary_cost_minor_units = None;
    assert!(
        MaterializationService::probe(
            &port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &source_policy(),
            &request
        )
        .await
        .is_err()
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn probe_rejects_resource_kind_query_return_and_gap_mismatch_before_io() {
    let port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::unsupported("unused"),
    };
    let access = FakeCandidateAccess(AccessDecision::Allowed);
    let policy = source_policy();
    let mut request = probe_request();
    request.capability.supported_resource_types.clear();
    assert!(
        MaterializationService::probe(&port, &access, &policy, &request)
            .await
            .is_err()
    );
    request = probe_request();
    request.capability.query_mode = ProbeQueryMode::FreeText;
    assert!(
        MaterializationService::probe(&port, &access, &policy, &request)
            .await
            .is_err()
    );
    request = probe_request();
    request.capability.return_types = vec![ProbeReturnType::CandidateReferences];
    assert!(
        MaterializationService::probe(&port, &access, &policy, &request)
            .await
            .is_err()
    );
    request = probe_request();
    request.capability.completeness_semantics = ProbeCompletenessSemantics::Unknown;
    assert!(
        MaterializationService::probe(&port, &access, &policy, &request)
            .await
            .is_err()
    );
    request = probe_request();
    request.gap.required_fact = "other".into();
    assert!(
        MaterializationService::probe(&port, &access, &policy, &request)
            .await
            .is_err()
    );
    request = probe_request();
    request.capability.estimated_cost.remote_calls = Some(0);
    assert!(
        MaterializationService::probe(&port, &access, &policy, &request)
            .await
            .is_err()
    );
    request = probe_request();
    let mut mismatched_policy = source_policy();
    mismatched_policy.0.as_mut().unwrap().resource_kind = ResourceKind::Workflow;
    assert!(
        MaterializationService::probe(&port, &access, &mismatched_policy, &request)
            .await
            .is_err()
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn found_probe_retains_typed_facts_and_provenance() {
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    let port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::found(facts, evidence()),
    };
    let result = MaterializationService::probe(
        &port,
        &FakeCandidateAccess(AccessDecision::Allowed),
        &source_policy(),
        &probe_request(),
    )
    .await
    .unwrap();
    assert_eq!(result.outcome(), ProbeOutcome::Found);
    assert!(result.facts().unwrap().contains("suitable"));
    assert_eq!(result.evidence().unwrap().provenance, "provider-query");
}

#[tokio::test]
async fn found_probe_rejects_facts_outside_the_targeted_facet() {
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    facts.insert(
        "unrequested",
        Fact::new(TypedValue::Bool(false), FactOrigin::Observed),
    );
    let port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::found(facts, evidence()),
    };

    let error = MaterializationService::probe(
        &port,
        &FakeCandidateAccess(AccessDecision::Allowed),
        &source_policy(),
        &probe_request(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, SearchError::OperationFailed(_)));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn found_probe_rejects_actual_response_over_the_content_budget() {
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::String("x".repeat(4096)), FactOrigin::Observed),
    );
    let port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::found(facts, evidence()),
    };
    let mut request = probe_request();
    request.budget.max_content_bytes = 16;

    let error = MaterializationService::probe(
        &port,
        &FakeCandidateAccess(AccessDecision::Allowed),
        &source_policy(),
        &request,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, SearchError::OperationFailed(_)));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn found_probe_rejects_evidence_for_a_different_target() {
    for mismatch in ["candidate", "resource", "missing resource"] {
        let mut wrong_evidence = evidence();
        match mismatch {
            "candidate" => wrong_evidence.candidate_id = "candidate-2".into(),
            "resource" => {
                wrong_evidence.resource_ref = Some(ResourceId::from_uuid(Uuid::from_u128(99)))
            }
            "missing resource" => wrong_evidence.resource_ref = None,
            _ => unreachable!(),
        }
        let mut facts = FactSet::default();
        facts.insert(
            "suitable",
            Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
        );
        let port = FakeProbe {
            calls: AtomicUsize::new(0),
            result: ProbeResult::found(facts, wrong_evidence),
        };
        let error = MaterializationService::probe(
            &port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &source_policy(),
            &probe_request(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, SearchError::OperationFailed(_)),
            "{mismatch}"
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn no_hit_probe_rejects_evidence_for_a_different_target() {
    for mismatch in ["candidate", "resource", "missing resource"] {
        let mut wrong_evidence = evidence();
        match mismatch {
            "candidate" => wrong_evidence.candidate_id = "candidate-2".into(),
            "resource" => {
                wrong_evidence.resource_ref = Some(ResourceId::from_uuid(Uuid::from_u128(99)))
            }
            "missing resource" => wrong_evidence.resource_ref = None,
            _ => unreachable!(),
        }
        let port = FakeProbe {
            calls: AtomicUsize::new(0),
            result: ProbeResult::not_found_by_probe(wrong_evidence),
        };
        let error = MaterializationService::probe(
            &port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &source_policy(),
            &probe_request(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, SearchError::OperationFailed(_)),
            "{mismatch}"
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn no_hit_probe_rejects_evidence_for_a_different_facet() {
    let mut request = probe_request();
    request.facet = "alternate".into();
    request.gap.required_fact = "alternate".into();
    request.capability.supported_facets.push("alternate".into());
    let port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::not_found_by_probe(evidence()),
    };

    let error = MaterializationService::probe(
        &port,
        &FakeCandidateAccess(AccessDecision::Allowed),
        &source_policy(),
        &request,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, SearchError::OperationFailed(_)));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn found_probe_rejects_evidence_for_a_different_facet() {
    let mut request = probe_request();
    request.facet = "alternate".into();
    request.gap.required_fact = "alternate".into();
    request.capability.supported_facets.push("alternate".into());
    let mut facts = FactSet::default();
    facts.insert(
        "alternate",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    let port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::found(facts, evidence()),
    };

    let error = MaterializationService::probe(
        &port,
        &FakeCandidateAccess(AccessDecision::Allowed),
        &source_policy(),
        &request,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, SearchError::OperationFailed(_)));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn no_hit_probe_rejects_evidence_from_a_different_capability() {
    for mismatch in ["probe type", "query mode", "completeness"] {
        let mut request = probe_request();
        let mut wrong_evidence = evidence();
        match mismatch {
            "probe type" => request.capability.probe_type = "different-query".into(),
            "query mode" => wrong_evidence.query_mode = ProbeQueryMode::FreeText,
            "completeness" => {
                request.capability.completeness_semantics =
                    ProbeCompletenessSemantics::CompleteForQuery
            }
            _ => unreachable!(),
        }
        let port = FakeProbe {
            calls: AtomicUsize::new(0),
            result: ProbeResult::not_found_by_probe(wrong_evidence),
        };

        let error = MaterializationService::probe(
            &port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &source_policy(),
            &request,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, SearchError::OperationFailed(_)),
            "{mismatch}"
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }
}

struct FakeMaterializer {
    calls: AtomicUsize,
}

impl MaterializerPort for FakeMaterializer {
    fn materialize<'a>(
        &'a self,
        request: &'a MaterializationRequest,
    ) -> BoxFuture<'a, MaterializationReceipt> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(MaterializationReceipt::new(
                request.resource_ref,
                request.binding.clone(),
                request.requested_state,
                MaterializationState::FullContent,
                request.retention_mode,
                Some("digest-only".into()),
                Some("provider-locator".into()),
                Some(b"runtime body".to_vec()),
            ))
        })
    }
}

fn materialization_request() -> MaterializationRequest {
    MaterializationRequest {
        resource_ref: resource_id(),
        binding: binding(),
        current_state: MaterializationState::ReferenceOnly,
        requested_state: MaterializationState::FullContent,
        access_context: "principal-1".into(),
        access: AccessDecision::Allowed,
        provider_permission: ProviderContentPermission::FullContent,
        retention_mode: RetentionMode::NoRetention,
        estimate: known_estimate(),
        budget: budget(),
        allow_direct_full: true,
    }
}

#[tokio::test]
async fn no_retention_full_content_is_runtime_only_and_trace_is_redacted() {
    let port = FakeMaterializer {
        calls: AtomicUsize::new(0),
    };
    let receipt = MaterializationService::materialize(
        &FakeCurrentState(MaterializationState::ReferenceOnly),
        &port,
        &FakeResourceAccess(AccessDecision::Allowed),
        &source_policy(),
        &materialization_request(),
    )
    .await
    .unwrap();
    assert_eq!(receipt.achieved_state(), MaterializationState::FullContent);
    assert_eq!(receipt.content(), Some(b"runtime body".as_slice()));
    assert!(receipt.to_session_store_record().is_err());
    let trace = format!("{:?}", receipt.trace());
    assert!(!trace.contains("runtime body"));
    assert!(!format!("{:?}", receipt).contains("runtime body"));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn materializer_rejects_unknown_cost_and_denied_provider_before_io() {
    let port = FakeMaterializer {
        calls: AtomicUsize::new(0),
    };
    let mut request = materialization_request();
    request.estimate.remote_calls = None;
    assert!(matches!(
        MaterializationService::materialize(
            &FakeCurrentState(MaterializationState::ReferenceOnly),
            &port,
            &FakeResourceAccess(AccessDecision::Allowed),
            &source_policy(),
            &request
        )
        .await,
        Err(SearchError::InvalidRequest(_))
    ));
    request.estimate = known_estimate();
    request.provider_permission = ProviderContentPermission::Fragment;
    assert!(
        MaterializationService::materialize(
            &FakeCurrentState(MaterializationState::ReferenceOnly),
            &port,
            &FakeResourceAccess(AccessDecision::Allowed),
            &source_policy(),
            &request
        )
        .await
        .is_err()
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn targeted_probe_can_be_followed_by_full_read_under_the_general_budget() {
    let port = FakeMaterializer {
        calls: AtomicUsize::new(0),
    };
    let mut request = materialization_request();
    request.current_state = MaterializationState::Probed;
    request.estimate.content_bytes = Some(17);
    request.allow_direct_full = false;
    let receipt = MaterializationService::materialize(
        &FakeCurrentState(MaterializationState::Probed),
        &port,
        &FakeResourceAccess(AccessDecision::Allowed),
        &source_policy(),
        &request,
    )
    .await
    .unwrap();
    assert_eq!(receipt.achieved_state(), MaterializationState::FullContent);
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn forged_probed_request_cannot_bypass_direct_full_limit() {
    let port = FakeMaterializer {
        calls: AtomicUsize::new(0),
    };
    let state = CountingCurrentState {
        evaluation: StateEvaluation::State(MaterializationState::ReferenceOnly),
        calls: AtomicUsize::new(0),
    };
    let mut request = materialization_request();
    request.current_state = MaterializationState::Probed;
    request.estimate.content_bytes = Some(17);
    request.allow_direct_full = false;

    let error = MaterializationService::materialize(
        &state,
        &port,
        &FakeResourceAccess(AccessDecision::Allowed),
        &source_policy(),
        &request,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, SearchError::InvalidRequest(_)));
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn missing_or_failed_trusted_state_fails_closed_before_materializer_io() {
    let port = FakeMaterializer {
        calls: AtomicUsize::new(0),
    };
    for evaluation in [StateEvaluation::Missing, StateEvaluation::Failed] {
        let state = CountingCurrentState {
            evaluation,
            calls: AtomicUsize::new(0),
        };
        let error = MaterializationService::materialize(
            &state,
            &port,
            &FakeResourceAccess(AccessDecision::Allowed),
            &source_policy(),
            &materialization_request(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, SearchError::InvalidRequest(_)));
        assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    }
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
}

struct BindingMaterializer {
    receipt_binding: RepresentationBinding,
    receipt_digest: Option<String>,
    calls: AtomicUsize,
}

impl MaterializerPort for BindingMaterializer {
    fn materialize<'a>(
        &'a self,
        request: &'a MaterializationRequest,
    ) -> BoxFuture<'a, MaterializationReceipt> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(MaterializationReceipt::new(
                request.resource_ref,
                self.receipt_binding.clone(),
                request.requested_state,
                MaterializationState::FullContent,
                request.retention_mode,
                self.receipt_digest.clone(),
                Some("provider-locator".into()),
                Some(b"runtime body".to_vec()),
            ))
        })
    }
}

#[tokio::test]
async fn receipt_must_match_exact_requested_binding_and_pinned_digest() {
    let mut request = materialization_request();
    request.binding.resource_version_ref = Some(ResourceVersionId::from_uuid(Uuid::from_u128(6)));
    request.binding.content_digest = Some("digest-only".into());
    for mismatch in ["binding", "source", "representation", "version", "digest"] {
        let mut receipt_binding = request.binding.clone();
        match mismatch {
            "binding" => receipt_binding.binding_id = BindingId::from_uuid(Uuid::from_u128(13)),
            "source" => receipt_binding.source_ref = SourceId::from_uuid(Uuid::from_u128(11)),
            "representation" => {
                receipt_binding.representation_ref =
                    RepresentationId::from_uuid(Uuid::from_u128(12));
            }
            "version" => {
                receipt_binding.resource_version_ref =
                    Some(ResourceVersionId::from_uuid(Uuid::from_u128(14)));
            }
            "digest" => receipt_binding.content_digest = Some("other-digest".into()),
            _ => unreachable!(),
        }
        let port = BindingMaterializer {
            receipt_binding,
            receipt_digest: Some("digest-only".into()),
            calls: AtomicUsize::new(0),
        };
        let error = MaterializationService::materialize(
            &FakeCurrentState(MaterializationState::ReferenceOnly),
            &port,
            &FakeResourceAccess(AccessDecision::Allowed),
            &source_policy(),
            &request,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, SearchError::OperationFailed(_)),
            "{mismatch}"
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }

    for receipt_digest in [None, Some("other-digest".into())] {
        let port = BindingMaterializer {
            receipt_binding: request.binding.clone(),
            receipt_digest,
            calls: AtomicUsize::new(0),
        };
        let error = MaterializationService::materialize(
            &FakeCurrentState(MaterializationState::ReferenceOnly),
            &port,
            &FakeResourceAccess(AccessDecision::Allowed),
            &source_policy(),
            &request,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, SearchError::OperationFailed(_)));
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }
}

struct OversizedMaterializer;

impl MaterializerPort for OversizedMaterializer {
    fn materialize<'a>(
        &'a self,
        request: &'a MaterializationRequest,
    ) -> BoxFuture<'a, MaterializationReceipt> {
        Box::pin(async move {
            Ok(MaterializationReceipt::new(
                request.resource_ref,
                request.binding.clone(),
                request.requested_state,
                MaterializationState::FullContent,
                request.retention_mode,
                None,
                None,
                Some(vec![b'x'; 17]),
            ))
        })
    }
}

#[tokio::test]
async fn direct_full_rejects_adapter_actual_bytes_above_small_resource_limit() {
    let error = MaterializationService::materialize(
        &FakeCurrentState(MaterializationState::ReferenceOnly),
        &OversizedMaterializer,
        &FakeResourceAccess(AccessDecision::Allowed),
        &source_policy(),
        &materialization_request(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, SearchError::OperationFailed(_)));
}

#[tokio::test]
async fn request_allowed_does_not_override_current_denial() {
    let probe_port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::unsupported("unused"),
    };
    let materializer_port = FakeMaterializer {
        calls: AtomicUsize::new(0),
    };
    let policy = source_policy();
    let denied_candidate = FakeCandidateAccess(AccessDecision::Denied);
    let denied_resource = FakeResourceAccess(AccessDecision::Denied);
    assert!(
        MaterializationService::probe(&probe_port, &denied_candidate, &policy, &probe_request(),)
            .await
            .is_err()
    );
    assert!(
        MaterializationService::materialize(
            &FakeCurrentState(MaterializationState::ReferenceOnly),
            &materializer_port,
            &denied_resource,
            &policy,
            &materialization_request(),
        )
        .await
        .is_err()
    );
    assert_eq!(probe_port.calls.load(Ordering::SeqCst), 0);
    assert_eq!(materializer_port.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unauthorized_probe_never_reads_source_policy_or_reveals_its_presence() {
    let mut errors = Vec::new();
    for evaluation in [
        AccessEvaluation::Decision(AccessDecision::Denied),
        AccessEvaluation::Decision(AccessDecision::Unknown),
        AccessEvaluation::SourceUnavailable,
        AccessEvaluation::OperationFailed,
    ] {
        for present in [true, false] {
            let policy = counting_source_policy(present);
            let probe = FakeProbe {
                calls: AtomicUsize::new(0),
                result: ProbeResult::unsupported("unused"),
            };
            let error = MaterializationService::probe(
                &probe,
                &VariableCandidateAccess(evaluation),
                &policy,
                &probe_request(),
            )
            .await
            .unwrap_err();
            assert!(matches!(&error, SearchError::InvalidRequest(_)));
            errors.push(error.to_string());
            assert_eq!(policy.calls.load(Ordering::SeqCst), 0);
            assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        }
    }
    assert!(errors.iter().all(|error| error == &errors[0]));
}

#[tokio::test]
async fn unauthorized_materialization_never_reads_source_policy_or_reveals_its_presence() {
    let mut errors = Vec::new();
    for evaluation in [
        AccessEvaluation::Decision(AccessDecision::Denied),
        AccessEvaluation::Decision(AccessDecision::Unknown),
        AccessEvaluation::SourceUnavailable,
        AccessEvaluation::OperationFailed,
    ] {
        for present in [true, false] {
            let policy = counting_source_policy(present);
            let materializer = FakeMaterializer {
                calls: AtomicUsize::new(0),
            };
            let error = MaterializationService::materialize(
                &FakeCurrentState(MaterializationState::ReferenceOnly),
                &materializer,
                &VariableResourceAccess(evaluation),
                &policy,
                &materialization_request(),
            )
            .await
            .unwrap_err();
            assert!(matches!(&error, SearchError::InvalidRequest(_)));
            errors.push(error.to_string());
            assert_eq!(policy.calls.load(Ordering::SeqCst), 0);
            assert_eq!(materializer.calls.load(Ordering::SeqCst), 0);
        }
    }
    assert!(errors.iter().all(|error| error == &errors[0]));
}

struct RemoteCandidateAccess(AccessDecision);

impl CurrentCandidateAccessEvaluatorPort for RemoteCandidateAccess {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        _access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            assert_eq!(candidate.source_ref, source_id());
            assert_eq!(candidate.candidate_id, "remote-1");
            assert_eq!(candidate.resource_ref, None);
            Ok(self.0)
        })
    }
}

#[tokio::test]
async fn remote_candidate_without_resource_id_uses_candidate_current_access() {
    let mut request = probe_request();
    request.candidate.candidate_id = "remote-1".into();
    request.candidate.identity_class = CandidateIdentityClass::RemoteStableReference;
    request.candidate.resource_ref = None;
    let allowed_port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::unsupported("remote unsupported"),
    };
    assert_eq!(
        MaterializationService::probe(
            &allowed_port,
            &RemoteCandidateAccess(AccessDecision::Allowed),
            &source_policy(),
            &request,
        )
        .await
        .unwrap()
        .outcome(),
        ProbeOutcome::Unsupported
    );
    assert_eq!(allowed_port.calls.load(Ordering::SeqCst), 1);

    let denied_port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::unsupported("unused"),
    };
    for decision in [AccessDecision::Denied, AccessDecision::Unknown] {
        assert!(
            MaterializationService::probe(
                &denied_port,
                &RemoteCandidateAccess(decision),
                &source_policy(),
                &request,
            )
            .await
            .is_err()
        );
    }
    assert_eq!(denied_port.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_access_and_source_grant_fail_closed_before_io() {
    let probe_port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::unsupported("unused"),
    };
    let materializer_port = FakeMaterializer {
        calls: AtomicUsize::new(0),
    };
    assert!(
        MaterializationService::probe(
            &probe_port,
            &FakeCandidateAccess(AccessDecision::Unknown),
            &source_policy(),
            &probe_request(),
        )
        .await
        .is_err()
    );
    assert!(
        MaterializationService::materialize(
            &FakeCurrentState(MaterializationState::ReferenceOnly),
            &materializer_port,
            &FakeResourceAccess(AccessDecision::Unknown),
            &source_policy(),
            &materialization_request(),
        )
        .await
        .is_err()
    );
    assert!(
        MaterializationService::probe(
            &probe_port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &FakeSourcePolicy(None),
            &probe_request(),
        )
        .await
        .is_err()
    );
    assert!(
        MaterializationService::materialize(
            &FakeCurrentState(MaterializationState::ReferenceOnly),
            &materializer_port,
            &FakeResourceAccess(AccessDecision::Allowed),
            &FakeSourcePolicy(None),
            &materialization_request(),
        )
        .await
        .is_err()
    );
    assert_eq!(probe_port.calls.load(Ordering::SeqCst), 0);
    assert_eq!(materializer_port.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn source_permission_and_retention_mismatch_block_io_and_durable_forgery() {
    let materializer_port = FakeMaterializer {
        calls: AtomicUsize::new(0),
    };
    let probe_port = FakeProbe {
        calls: AtomicUsize::new(0),
        result: ProbeResult::unsupported("unused"),
    };
    let mut request = materialization_request();
    let mut policy = source_policy();
    policy.0.as_mut().unwrap().provider_permission = ProviderContentPermission::Fragment;
    assert!(
        MaterializationService::materialize(
            &FakeCurrentState(MaterializationState::ReferenceOnly),
            &materializer_port,
            &FakeResourceAccess(AccessDecision::Allowed),
            &policy,
            &request,
        )
        .await
        .is_err()
    );

    request.retention_mode = RetentionMode::PersistentResource;
    assert!(
        MaterializationService::materialize(
            &FakeCurrentState(MaterializationState::ReferenceOnly),
            &materializer_port,
            &FakeResourceAccess(AccessDecision::Allowed),
            &source_policy(),
            &request,
        )
        .await
        .is_err()
    );
    assert_eq!(materializer_port.calls.load(Ordering::SeqCst), 0);

    let mut probe_request = probe_request();
    probe_request.retention_mode = RetentionMode::PersistentResource;
    assert!(
        MaterializationService::probe(
            &probe_port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &source_policy(),
            &probe_request,
        )
        .await
        .is_err()
    );
    probe_request.retention_mode = RetentionMode::NoRetention;
    probe_request.provider_permission = ProviderContentPermission::ReferenceOnly;
    assert!(
        MaterializationService::probe(
            &probe_port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &source_policy(),
            &probe_request,
        )
        .await
        .is_err()
    );
    probe_request.provider_permission = ProviderContentPermission::FullContent;
    let mut policy = source_policy();
    policy.0.as_mut().unwrap().probe_allowed = false;
    assert!(
        MaterializationService::probe(
            &probe_port,
            &FakeCandidateAccess(AccessDecision::Allowed),
            &policy,
            &probe_request,
        )
        .await
        .is_err()
    );
    assert_eq!(probe_port.calls.load(Ordering::SeqCst), 0);

    let forged = MaterializationReceipt::new(
        resource_id(),
        binding(),
        MaterializationState::FullContent,
        MaterializationState::FullContent,
        RetentionMode::PersistentResource,
        Some("digest".into()),
        Some("locator".into()),
        Some(b"body".to_vec()),
    );
    assert!(forged.to_session_store_record().is_err());
}

#[tokio::test]
async fn durable_record_requires_matching_persistent_source_grant() {
    let port = FakeMaterializer {
        calls: AtomicUsize::new(0),
    };
    let mut request = materialization_request();
    request.retention_mode = RetentionMode::PersistentResource;
    request.binding.resource_version_ref = Some(ResourceVersionId::from_uuid(Uuid::from_u128(6)));
    request.binding.content_digest = Some("digest-only".into());
    let mut policy = source_policy();
    policy.0.as_mut().unwrap().retention_mode = RetentionMode::PersistentResource;
    let receipt = MaterializationService::materialize(
        &FakeCurrentState(MaterializationState::ReferenceOnly),
        &port,
        &FakeResourceAccess(AccessDecision::Allowed),
        &policy,
        &request,
    )
    .await
    .unwrap();
    let record = receipt.to_session_store_record().unwrap();
    assert_eq!(record.resource_ref(), resource_id());
    assert_eq!(record.binding(), &request.binding);
    assert_eq!(record.achieved_state(), MaterializationState::FullContent);
    assert_eq!(record.content_digest(), Some("digest-only"));
    assert_eq!(record.locator(), Some("provider-locator"));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}
