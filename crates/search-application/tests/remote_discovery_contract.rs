//! P4-12: remote Sources in the one common Discovery path.

#[path = "support/remote_discovery.rs"]
mod discovery;
#[path = "support/remote.rs"]
mod support;

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use discovery::*;
use search_application::content_scope::DiscoveryScope;
use search_application::discovery_service::{
    DiscoveryPorts, DiscoveryService, RemoteSourceExecution, ScopedDiscoveryExecution,
};
use search_application::ports::SealedRemoteRetrieverPort;
use search_application::remote::UntrustedRemoteHit;
use search_application::remote_evidence::RegisteredLineage;
use search_application::remote_lease::{LeaseClock, RemoteLease, ScopedOwnerGate};
use search_application::remote_observation::RemoteSnapshotVerifierPort;
use search_application::remote_read_view::{CompositeEvaluationReadView, RemoteClaimSelectors};
use search_application::retrieval::{
    ActionState, RetrievalAction, RetrieverCursorState, RetrieverKind,
};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::routing::RouteStage;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::discovery::DiscoveryResult;
use search_core::evidence::{ClaimState, EvidenceSufficiency};
use search_core::id::ResourceId;
use search_core::source::RetentionMode;
use support::*;

/// One evaluation over the remote fixture: returns the result, the provider
/// batch sizes and whether the sealed key still lacks a DirectAddress list.
async fn evaluate(
    remote: &Remote,
    verifier: &dyn RemoteSnapshotVerifierPort,
    answers: BTreeMap<&'static str, Vec<UntrustedRemoteHit>>,
    reverse: bool,
    max_initial: usize,
) -> (DiscoveryResult, Vec<usize>, bool) {
    let source = remote.registration.source_id();
    let visibility = remote.visibility();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    let clock = ManualClock::new();
    let nothing = Nothing;
    let selectors = RemoteClaimSelectors::new(vec![
        (claim(TITLE), "catalog.title".into(), None),
        (claim(DEPARTMENT), "catalog.department".into(), None),
    ]);
    let view = CompositeEvaluationReadView::new(
        &nothing,
        &nothing,
        &nothing,
        &nothing,
        &nothing,
        &selectors,
        &gate,
        clock.clone(),
    );
    let port = Port {
        remote,
        visibility: &visibility,
        verifier,
        answers,
        reverse,
        batches: Mutex::new(vec![]),
    };
    let mut registry = InMemorySourceRegistry::default();
    registry.insert(remote.visible.discoverable_source());
    let access = AllowAll;
    let config = config(source, max_initial);
    let routing = config.routing.clone();
    let service = DiscoveryService::new(
        config,
        DiscoveryPorts {
            sources: &registry,
            generations: &nothing,
            concepts: &nothing,
            retrieval: RetrievalExecutionPorts {
                directory: None,
                structured: None,
                lexical: None,
                hypergraph: None,
                graph_resource_access: None,
                remote: None,
                access: &access,
            },
            selectors: &nothing,
            assertions: &nothing,
            evidence: &nothing,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    let result = service
        .discover_scoped(
            request(remote),
            ScopedDiscoveryExecution {
                content_scope: DiscoveryScope::Normal,
                binding: &remote.binding,
                visible: std::slice::from_ref(&remote.visible),
                routing,
                remote: vec![RemoteSourceExecution {
                    context: remote.context(&visibility).await,
                    port: &port,
                    lineage: RegisteredLineage::new(&remote.registration, vec![]).unwrap(),
                    provenance: &Lookup,
                    lease: RemoteLease {
                        absolute_deadline: clock.now() + Duration::from_secs(60),
                        idle_timeout: None,
                        provider_expiry: None,
                    },
                }],
                view: &view,
            },
        )
        .await
        .unwrap();
    let direct = RetrievalAction {
        source_id: source,
        retriever_id: format!("{}:{:?}", source.as_uuid(), RetrieverKind::DirectAddress),
        retriever: RetrieverKind::DirectAddress,
        stage: RouteStage::Initial,
        state: ActionState::Planned,
        cursor: RetrieverCursorState::initial(),
    };
    let unsealed_direct = match view.remote_key(source) {
        Some(key) => view.retrieve(&direct, key).await.is_err(),
        None => true,
    };
    let batches = port.batches.lock().unwrap().clone();
    (result, batches, unsealed_direct)
}

fn qualified(result: &DiscoveryResult) -> Vec<ResourceId> {
    result
        .qualified_resources
        .iter()
        .map(|resource| resource.resource_ref)
        .collect()
}

#[tokio::test]
async fn two_remote_actions_one_key_pass_common_federation_and_claims() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let verifier = Verifier::shared("snapshot-1");
    let (result, batches, unsealed_direct) = evaluate(
        &remote,
        &verifier,
        answers(Some("doc-2"), Some("doc-1")),
        false,
        2,
    )
    .await;
    // One provider batch of both actions, sealed under one key.
    assert_eq!(batches, vec![2]);
    assert!(!unsealed_direct);
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(result.unresolved_gaps.iter().all(|gap| !gap.blocking));
    assert_eq!(
        qualified(&result),
        vec![id(&remote, "doc-2"), id(&remote, "doc-1")]
    );
    for claim_id in [claim(TITLE), claim(DEPARTMENT)] {
        assert!(
            result
                .evidence_set
                .iter()
                .any(|claim| claim.claim_id == claim_id && claim.state == ClaimState::Supported)
        );
    }
    let source = remote.registration.source_id().as_uuid();
    for kind in ["RemoteQuery", "DirectAddress"] {
        assert!(
            result
                .retrieval_trace
                .contains(&format!("retriever:{source}:{kind}"))
        );
    }
}

#[tokio::test]
async fn required_remote_planned_but_failed_is_not_executed() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    let verifier = Verifier::shared("snapshot-1");
    // Both planned actions fail: nothing is sealed, nothing executed.
    let (result, batches, _) = evaluate(&remote, &verifier, answers(None, None), false, 2).await;
    assert_eq!(batches, vec![2]);
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(has_gap(&result, source, "required_source_not_executed"));
    assert!(result.qualified_resources.is_empty());
    assert!(
        !result
            .retrieval_trace
            .iter()
            .any(|entry| entry.starts_with("retriever:"))
    );
    // One failed action: the completed one executes, the other is a gap.
    let (result, _, _) = evaluate(&remote, &verifier, answers(None, Some("doc-1")), false, 2).await;
    assert!(has_gap(&result, source, "remote_action_unavailable"));
    assert!(!has_gap(&result, source, "required_source_not_executed"));
    assert_eq!(qualified(&result), vec![id(&remote, "doc-1")]);
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(
        !result
            .retrieval_trace
            .iter()
            .any(|entry| entry.ends_with(":RemoteQuery"))
    );
}

#[tokio::test]
async fn remote_expansion_after_seal_requires_new_evaluation() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    let verifier = Verifier::shared("snapshot-1");
    // Only the query is initial; the lookup is a later expansion.
    let (result, batches, unsealed_direct) = evaluate(
        &remote,
        &verifier,
        answers(Some("doc-2"), Some("doc-1")),
        false,
        1,
    )
    .await;
    assert_eq!(batches, vec![1]);
    assert!(unsealed_direct);
    assert!(has_gap(
        &result,
        source,
        "remote_expansion_requires_new_evaluation"
    ));
    assert_eq!(qualified(&result), vec![id(&remote, "doc-2")]);
    assert_ne!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    // An incompatible extra response is a gap and never joins the seal.
    let rotating = Rotating(Mutex::new(0));
    let (result, batches, unsealed_direct) = evaluate(
        &remote,
        &rotating,
        answers(Some("doc-2"), Some("doc-1")),
        false,
        2,
    )
    .await;
    assert_eq!(batches, vec![2]);
    assert!(unsealed_direct);
    assert!(has_gap(&result, source, "remote_snapshot_incompatible"));
    assert!(has_gap(&result, source, "remote_action_unavailable"));
    assert_eq!(qualified(&result), vec![id(&remote, "doc-2")]);
}

#[tokio::test]
async fn local_discovery_and_s1_priority_unchanged() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    // Provider answer order never changes the plan's S1 priority.
    let verifier = Verifier::shared("snapshot-1");
    let (result, _, _) = evaluate(
        &remote,
        &verifier,
        answers(Some("doc-2"), Some("doc-1")),
        true,
        2,
    )
    .await;
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(
        qualified(&result),
        vec![id(&remote, "doc-2"), id(&remote, "doc-1")]
    );

    // The legacy local entrypoint routes no remote mode and calls no provider.
    let nothing = Nothing;
    let access = AllowAll;
    let mut registry = InMemorySourceRegistry::default();
    registry.insert(remote.visible.discoverable_source());
    let service = DiscoveryService::new(
        config(source, 2),
        DiscoveryPorts {
            sources: &registry,
            generations: &nothing,
            concepts: &nothing,
            retrieval: RetrievalExecutionPorts {
                directory: None,
                structured: None,
                lexical: None,
                hypergraph: None,
                graph_resource_access: None,
                remote: None,
                access: &access,
            },
            selectors: &nothing,
            assertions: &nothing,
            evidence: &nothing,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    let local = service.discover(request(&remote)).await.unwrap();
    assert!(local.qualified_resources.is_empty());
    assert_ne!(local.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(
        local
            .unresolved_gaps
            .iter()
            .any(|gap| gap.required_fact.ends_with(":RuntimePortUnavailable"))
    );
    assert!(
        !local
            .retrieval_trace
            .iter()
            .any(|entry| entry.starts_with("retriever:"))
    );
}
