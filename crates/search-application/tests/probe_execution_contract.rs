use std::sync::atomic::{AtomicUsize, Ordering};

use search_application::SearchError;
use search_application::materialization::{
    ProbeBudget, ProbeCapability, ProbeEvidence, ProbeRequest, ProbeResult, ResourceCostEstimate,
};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentCandidateAccessEvaluatorPort, CurrentProbeCapability,
    CurrentSourcePolicy, CurrentSourcePolicyPort, ProbeCapabilityCatalogPort, ProbeExecutionInput,
    ProbeExecutionService, ProbePort,
};
use search_core::discovery::{
    CandidateIdentityClass, FederatedCandidate, GapReason, InformationGap,
};
use search_core::fact::{Fact, FactOrigin, FactSet};
use search_core::id::{ResourceId, SourceId};
use search_core::materialization::{
    ProbeCompletenessSemantics, ProbeExecutionLocation, ProbeOutcome, ProbeQueryMode,
    ProbeReturnType, ProviderContentPermission,
};
use search_core::observation::Coverage;
use search_core::predicate::TypedValue;
use search_core::resource::ResourceKind;
use search_core::source::RetentionMode;
use uuid::Uuid;

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(101))
}

fn resource_id() -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(102))
}

fn candidate() -> FederatedCandidate {
    let mut candidate = FederatedCandidate::new(
        "candidate-101",
        CandidateIdentityClass::DurableResource,
        source_id(),
        "structured",
    );
    candidate.resource_ref = Some(resource_id());
    candidate
}

fn budget() -> ProbeBudget {
    ProbeBudget {
        max_content_bytes: 512,
        max_latency_ms: 100,
        max_remote_calls: 1,
        max_monetary_cost_minor_units: 10,
        currency: "USD".into(),
    }
}

fn input() -> ProbeExecutionInput {
    ProbeExecutionInput {
        candidate: candidate(),
        gap: InformationGap::new("suitable", GapReason::MissingFact, true),
        facet: "suitable".into(),
        access_context: "principal-101".into(),
        budget: budget(),
    }
}

fn capability() -> CurrentProbeCapability {
    CurrentProbeCapability {
        candidate_id: "candidate-101".into(),
        resource_ref: Some(resource_id()),
        facet: "suitable".into(),
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
            estimated_cost: ResourceCostEstimate {
                content_bytes: Some(100),
                latency_ms: Some(20),
                remote_calls: Some(1),
                monetary_cost_minor_units: Some(0),
                currency: Some("USD".into()),
            },
        },
    }
}

fn policy() -> CurrentSourcePolicy {
    CurrentSourcePolicy {
        resource_kind: ResourceKind::Knowledge,
        provider_permission: ProviderContentPermission::FullContent,
        retention_mode: RetentionMode::NoRetention,
        probe_allowed: true,
    }
}

fn evidence() -> ProbeEvidence {
    ProbeEvidence {
        source_ref: source_id(),
        candidate_id: "candidate-101".into(),
        resource_ref: Some(resource_id()),
        facet: "suitable".into(),
        probe_type: "facet-query".into(),
        query_mode: ProbeQueryMode::FacetExact,
        completeness_semantics: ProbeCompletenessSemantics::Partial,
        provenance: "provider-response".into(),
        coverage: Coverage::QueryResult,
    }
}

enum Access {
    Decision(AccessDecision),
    Error,
}

struct CountingAccess {
    result: Access,
    calls: AtomicUsize,
}

impl CurrentCandidateAccessEvaluatorPort for CountingAccess {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        assert_eq!(candidate.candidate_id, "candidate-101");
        assert_eq!(access_context, "principal-101");
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match self.result {
                Access::Decision(decision) => Ok(decision),
                Access::Error => Err(SearchError::SourceUnavailable("private candidate".into())),
            }
        })
    }
}

struct CountingCatalog {
    current: Option<CurrentProbeCapability>,
    calls: AtomicUsize,
}

impl ProbeCapabilityCatalogPort for CountingCatalog {
    fn for_candidate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        facet: &'a str,
        access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentProbeCapability>> {
        assert_eq!(candidate.source_ref, source_id());
        assert_eq!(facet, "suitable");
        assert_eq!(access_context, "principal-101");
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(self.current.clone()) })
    }
}

struct CountingPolicy {
    current: Option<CurrentSourcePolicy>,
    calls: AtomicUsize,
}

impl CurrentSourcePolicyPort for CountingPolicy {
    fn for_candidate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        assert_eq!(candidate.candidate_id, "candidate-101");
        assert_eq!(access_context, "principal-101");
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(self.current) })
    }

    fn for_resource<'a>(
        &'a self,
        _source_ref: SourceId,
        _resource_ref: ResourceId,
        _binding: &'a search_core::binding::RepresentationBinding,
        _access_context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        panic!("resource policy must not be used for candidate probe")
    }
}

struct CountingProbe {
    result: ProbeResult,
    calls: AtomicUsize,
}

impl ProbePort for CountingProbe {
    fn probe<'a>(&'a self, request: &'a ProbeRequest) -> BoxFuture<'a, ProbeResult> {
        assert_eq!(request.candidate.candidate_id, "candidate-101");
        assert_eq!(request.facet, "suitable");
        assert_eq!(request.capability.source_ref, source_id());
        assert_eq!(
            request.provider_permission,
            ProviderContentPermission::FullContent
        );
        assert_eq!(request.retention_mode, RetentionMode::NoRetention);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(self.result.clone()) })
    }
}

struct Harness {
    access: CountingAccess,
    catalog: CountingCatalog,
    policy: CountingPolicy,
    probe: CountingProbe,
}

impl Harness {
    fn new(result: ProbeResult) -> Self {
        Self {
            access: CountingAccess {
                result: Access::Decision(AccessDecision::Allowed),
                calls: AtomicUsize::new(0),
            },
            catalog: CountingCatalog {
                current: Some(capability()),
                calls: AtomicUsize::new(0),
            },
            policy: CountingPolicy {
                current: Some(policy()),
                calls: AtomicUsize::new(0),
            },
            probe: CountingProbe {
                result,
                calls: AtomicUsize::new(0),
            },
        }
    }

    async fn execute(&self) -> Result<ProbeResult, SearchError> {
        ProbeExecutionService::execute(
            &self.probe,
            &self.access,
            &self.catalog,
            &self.policy,
            &input(),
        )
        .await
    }
}

#[tokio::test]
async fn denied_unknown_or_failed_access_cannot_query_capability_or_source_policy() {
    for result in [
        Access::Decision(AccessDecision::Denied),
        Access::Decision(AccessDecision::Unknown),
        Access::Error,
    ] {
        let mut harness = Harness::new(ProbeResult::unsupported("unused"));
        harness.access.result = result;
        let error = harness.execute().await.unwrap_err().to_string();
        assert_eq!(
            error,
            "invalid search request: current probe access is not authorized"
        );
        assert!(!error.contains("candidate-101"));
        assert_eq!(harness.access.calls.load(Ordering::SeqCst), 1);
        assert_eq!(harness.catalog.calls.load(Ordering::SeqCst), 0);
        assert_eq!(harness.policy.calls.load(Ordering::SeqCst), 0);
        assert_eq!(harness.probe.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn stale_or_mismatched_current_capability_never_reaches_probe_port() {
    for mutation in 0..5 {
        let mut harness = Harness::new(ProbeResult::unsupported("unused"));
        let current = harness.catalog.current.as_mut().unwrap();
        match mutation {
            0 => current.candidate_id = "other-candidate".into(),
            1 => current.resource_ref = None,
            2 => current.facet = "other-facet".into(),
            3 => current.capability.source_ref = SourceId::from_uuid(Uuid::from_u128(999)),
            _ => current.capability.estimated_cost.remote_calls = None,
        }
        assert!(harness.execute().await.is_err());
        assert_eq!(harness.probe.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn found_returns_only_targeted_fact_from_current_capability() {
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    let harness = Harness::new(ProbeResult::found(facts, evidence()));
    let result = harness.execute().await.unwrap();
    assert_eq!(result.outcome(), ProbeOutcome::Found);
    assert_eq!(result.facts().unwrap().len(), 1);
    assert!(result.facts().unwrap().contains("suitable"));
    assert_eq!(harness.probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.access.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn probe_no_hit_stays_unresolved_without_false_fact() {
    let harness = Harness::new(ProbeResult::not_found_by_probe(evidence()));
    let result = harness.execute().await.unwrap();
    assert_eq!(result.outcome(), ProbeOutcome::NotFoundByProbe);
    assert!(result.facts().is_none());
    assert_eq!(harness.probe.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn no_current_capability_or_unsupported_probe_stays_unresolved() {
    let mut no_capability = Harness::new(ProbeResult::unsupported("unused"));
    no_capability.catalog.current = None;
    let result = no_capability.execute().await.unwrap();
    assert_eq!(result.outcome(), ProbeOutcome::Unsupported);
    assert!(result.facts().is_none());
    assert_eq!(no_capability.policy.calls.load(Ordering::SeqCst), 0);
    assert_eq!(no_capability.probe.calls.load(Ordering::SeqCst), 0);

    let unsupported = Harness::new(ProbeResult::unsupported("provider-unsupported"));
    let result = unsupported.execute().await.unwrap();
    assert_eq!(result.outcome(), ProbeOutcome::Unsupported);
    assert!(result.facts().is_none());
    assert_eq!(unsupported.probe.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ephemeral_candidates_sharing_a_source_and_id_cannot_transfer_a_probe_fact() {
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    let mut shared_evidence = evidence();
    shared_evidence.resource_ref = None;
    let mut harness = Harness::new(ProbeResult::found(facts, shared_evidence));
    harness.catalog.current.as_mut().unwrap().resource_ref = None;

    // A and B have the same Source and candidate_id, but different locators.
    // The catalog and ProbePort could otherwise return A's capability and fact for B.
    for locator in ["provider://candidate-a", "provider://candidate-b"] {
        let mut request = input();
        request.candidate.identity_class = CandidateIdentityClass::EphemeralCandidate;
        request.candidate.resource_ref = None;
        request.candidate.locator = Some(locator.into());

        let result = ProbeExecutionService::execute(
            &harness.probe,
            &harness.access,
            &harness.catalog,
            &harness.policy,
            &request,
        )
        .await
        .unwrap();
        assert_eq!(result.outcome(), ProbeOutcome::Unsupported);
        assert!(result.facts().is_none());
    }
    assert_eq!(harness.access.calls.load(Ordering::SeqCst), 2);
    assert_eq!(harness.catalog.calls.load(Ordering::SeqCst), 0);
    assert_eq!(harness.policy.calls.load(Ordering::SeqCst), 0);
    assert_eq!(harness.probe.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn remote_stable_candidate_without_resource_id_can_use_current_capability() {
    let mut facts = FactSet::default();
    facts.insert(
        "suitable",
        Fact::new(TypedValue::Bool(true), FactOrigin::Observed),
    );
    let mut remote_evidence = evidence();
    remote_evidence.resource_ref = None;
    let mut harness = Harness::new(ProbeResult::found(facts, remote_evidence));
    harness.catalog.current.as_mut().unwrap().resource_ref = None;
    let mut request = input();
    request.candidate.identity_class = CandidateIdentityClass::RemoteStableReference;
    request.candidate.resource_ref = None;
    request.candidate.locator = Some("provider://stable-candidate".into());

    let result = ProbeExecutionService::execute(
        &harness.probe,
        &harness.access,
        &harness.catalog,
        &harness.policy,
        &request,
    )
    .await
    .unwrap();
    assert_eq!(result.outcome(), ProbeOutcome::Found);
    assert!(result.facts().unwrap().contains("suitable"));
    assert_eq!(harness.catalog.calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.probe.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn only_blocking_exact_facet_may_trigger_a_probe() {
    let harness = Harness::new(ProbeResult::unsupported("unused"));
    let mut request = input();
    request.gap.blocking = false;
    assert!(
        ProbeExecutionService::execute(
            &harness.probe,
            &harness.access,
            &harness.catalog,
            &harness.policy,
            &request,
        )
        .await
        .is_err()
    );
    request = input();
    request.gap.required_fact = "other".into();
    assert!(
        ProbeExecutionService::execute(
            &harness.probe,
            &harness.access,
            &harness.catalog,
            &harness.policy,
            &request,
        )
        .await
        .is_err()
    );
    assert_eq!(harness.probe.calls.load(Ordering::SeqCst), 0);
}
