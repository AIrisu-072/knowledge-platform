//! P4-13: scoped entrypoint, revocation before disclosure and the
//! transient disclosure lease.

#[path = "support/remote_discovery.rs"]
mod discovery;
#[path = "support/remote.rs"]
mod support;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use discovery::*;
use search_application::SearchError;
use search_application::ports::BoxFuture;
use search_application::remote::PinnedRemoteTarget;
use search_application::remote_disclosure::{
    CurrentDisclosureAccessPort, DisclosedFields, DisclosureOwner, RemoteWiring,
    ScopedDisclosureGate, ScopedDiscoveryService, TransientDisclosure,
};
use search_application::remote_evidence::{
    RegisteredLineage, RemoteProvenanceLookupPort, VerifiedSourceProvenance,
};
use search_application::remote_lease::LeaseState;
use search_application::routing::RoutingConstraints;
use search_application::scoped::{
    AccessContextAuthorityPort, AccessRevision, AuthorizedSourceScope, PrincipalRef,
    ScopedSourceRegistryPort, SyntheticAuthorityAdapter, SyntheticVisibilityAdapter,
    TrustedDiscoveryBinding, TrustedSearchScope, VisibleCatalogSnapshot,
};
use search_application::source_registration::TrustedVisibleRegistry;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::discovery::{DiscoveryResult, GapReason};
use search_core::evidence::{ClaimState, EvidenceRole, EvidenceSufficiency};
use search_core::id::{ClaimId, DiscoveryEvaluationId, ResourceId, SourceId};
use search_core::source::RetentionMode;
use support::*;
use uuid::Uuid;

/// Counts registry reads: routing never starts for an unbound actor.
struct Counting<'a> {
    inner: &'a dyn ScopedSourceRegistryPort,
    calls: Mutex<usize>,
}
impl ScopedSourceRegistryPort for Counting<'_> {
    fn visible_sources<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
    ) -> BoxFuture<'a, VisibleCatalogSnapshot> {
        *self.calls.lock().unwrap() += 1;
        self.inner.visible_sources(actor)
    }
}

/// The fixed Source's provenance read, during which the actor loses the
/// Source: the evaluation is already past routing and the provider batch.
struct RevokingLookup<'a> {
    visibility: &'a SyntheticVisibilityAdapter<'a>,
    principal: PrincipalRef,
    source: SourceId,
}
impl RemoteProvenanceLookupPort for RevokingLookup<'_> {
    fn lookup<'a>(
        &'a self,
        scope: &'a AuthorizedSourceScope,
        target: &'a PinnedRemoteTarget,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedSourceProvenance>> {
        Box::pin(async move {
            self.visibility.revoke(&self.principal, self.source).ok();
            Lookup.lookup(scope, target, evidence_ref).await
        })
    }
}

/// A final gate that never answers: the caller cancels the disclosure.
struct Pending;
impl CurrentDisclosureAccessPort for Pending {
    fn authorize<'a>(
        &'a self,
        _: &'a DisclosureOwner,
        _: &'a DisclosedFields,
    ) -> BoxFuture<'a, ()> {
        Box::pin(std::future::pending())
    }
}

struct Outcome {
    disclosure: Result<TransientDisclosure<DiscoveryResult>, SearchError>,
    registry_calls: usize,
    batches: Vec<usize>,
}

/// One scoped Discovery over the remote fixture, requiring `required`.
async fn run(
    remote: &Remote,
    visibility: &SyntheticVisibilityAdapter<'_>,
    provenance: &dyn RemoteProvenanceLookupPort,
    binding: &TrustedDiscoveryBinding,
    required: SourceId,
    clock: std::sync::Arc<ManualClock>,
) -> Outcome {
    let source = remote.registration.source_id();
    let trusted = TrustedVisibleRegistry::new(&remote.authority, visibility, &remote.catalog);
    let registry = Counting {
        inner: &trusted,
        calls: Mutex::new(0),
    };
    let verifier = Verifier::shared("snapshot-1");
    let port = Port {
        remote,
        visibility,
        verifier: &verifier,
        answers: answers(Some("doc-2"), Some("doc-1")),
        reverse: false,
        batches: Mutex::new(vec![]),
    };
    let sources = InMemorySourceRegistry::default();
    let nothing = Nothing;
    let access = AllowAll;
    let service = service(&sources, &nothing, &access, config(source, 2));
    let selectors = selectors();
    let scoped = ScopedDiscoveryService::new(
        &service,
        &remote.authority,
        visibility,
        &registry,
        &selectors,
        vec![(
            source,
            RemoteWiring {
                port: &port,
                lineage: RegisteredLineage::new(&remote.registration, vec![]).unwrap(),
                provenance,
                evaluation_ttl: Duration::from_secs(60),
                idle_timeout: None,
            },
        )],
        clock,
        Duration::from_secs(30),
    )
    .unwrap();
    let disclosure = scoped
        .discover(
            binding,
            request(remote),
            RoutingConstraints {
                required_source_ids: vec![required],
                preferred_source_ids: vec![],
                max_initial_optional_sources: 0,
            },
        )
        .await;
    let registry_calls = *registry.calls.lock().unwrap();
    let batches = port.batches.lock().unwrap().clone();
    Outcome {
        disclosure,
        registry_calls,
        batches,
    }
}

/// Everything a disclosure reveals, copied out inside the callback.
#[derive(Debug, PartialEq, Eq)]
struct Shape {
    sufficiency: EvidenceSufficiency,
    qualified: Vec<ResourceId>,
    claims: Vec<(ClaimId, ClaimState, Vec<EvidenceRole>)>,
    gaps: Vec<(String, GapReason, bool)>,
    trace: Vec<String>,
}

async fn disclose(
    disclosure: &mut TransientDisclosure<DiscoveryResult>,
    gate: &dyn CurrentDisclosureAccessPort,
) -> Result<Shape, SearchError> {
    let mut shape = None;
    disclosure
        .with_disclosure(gate, |view| {
            shape = Some(Shape {
                sufficiency: view.sufficiency(),
                qualified: view.qualified_resources().collect(),
                claims: view
                    .claims()
                    .map(|(id, state)| (id, state, view.evidence_roles(id).collect()))
                    .collect(),
                gaps: view
                    .gaps()
                    .map(|(fact, reason, blocking)| (fact.to_owned(), reason, blocking))
                    .collect(),
                trace: view.trace().map(str::to_owned).collect(),
            });
            Ok(())
        })
        .await?;
    Ok(shape.unwrap())
}

fn mentions(shape: &Shape, source: SourceId) -> bool {
    let needle = source.as_uuid().to_string();
    shape.gaps.iter().any(|(fact, _, _)| fact.contains(&needle))
        || shape.trace.iter().any(|entry| entry.contains(&needle))
}

#[tokio::test]
async fn foreign_or_expired_handle_stops_before_route_and_network() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    let visibility = remote.visibility();
    let tenant = remote.registration.tenant().clone();
    let evaluation = || DiscoveryEvaluationId::from_uuid(Uuid::now_v7());

    // Issued by another authority.
    let other = SyntheticAuthorityAdapter::new();
    let handle = other
        .issue_verified_identity(
            tenant.clone(),
            PrincipalRef::new("reader").unwrap(),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = other.resolve(&handle).await.unwrap().unwrap();
    let foreign = other
        .bind_discovery(&actor, evaluation())
        .await
        .unwrap()
        .unwrap();
    // Expired, and revoked, at this authority.
    let issue = |ttl| {
        remote
            .authority
            .issue_verified_identity(
                tenant.clone(),
                PrincipalRef::new("reader").unwrap(),
                None,
                AccessRevision::new(1).unwrap(),
                ttl,
            )
            .unwrap()
    };
    let short = issue(Duration::from_millis(50));
    let actor = remote.authority.resolve(&short).await.unwrap().unwrap();
    let expired = remote
        .authority
        .bind_discovery(&actor, evaluation())
        .await
        .unwrap()
        .unwrap();
    let revoked_handle = issue(Duration::from_secs(60));
    let actor = remote
        .authority
        .resolve(&revoked_handle)
        .await
        .unwrap()
        .unwrap();
    let revoked = remote
        .authority
        .bind_discovery(&actor, evaluation())
        .await
        .unwrap()
        .unwrap();
    remote.authority.revoke(&revoked_handle).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    for binding in [&foreign, &expired, &revoked] {
        let outcome = run(
            &remote,
            &visibility,
            &Lookup,
            binding,
            source,
            ManualClock::new(),
        )
        .await;
        assert!(outcome.disclosure.is_err());
        assert_eq!(outcome.registry_calls, 0);
        assert!(outcome.batches.is_empty());
    }
    // The fixture's own current binding passes the same entrypoint.
    let outcome = run(
        &remote,
        &visibility,
        &Lookup,
        &remote.binding,
        source,
        ManualClock::new(),
    )
    .await;
    assert!(outcome.disclosure.is_ok());
    assert_eq!(outcome.registry_calls, 1);
}

#[tokio::test]
async fn revoke_during_evidence_read_removes_source() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    let visibility = remote.visibility();
    let revoking = RevokingLookup {
        visibility: &visibility,
        principal: remote.binding.actor().principal().clone(),
        source,
    };
    let mut revoked = run(
        &remote,
        &visibility,
        &revoking,
        &remote.binding,
        source,
        ManualClock::new(),
    )
    .await;
    // The provider ran once; the recomputation never reached it again.
    assert_eq!(revoked.batches, vec![2]);
    let revoked_gate = ScopedDisclosureGate::new(&remote.authority, &visibility);
    let revoked = disclose(revoked.disclosure.as_mut().unwrap(), &revoked_gate)
        .await
        .unwrap();

    // Never visible to this actor, and a Required Source that does not exist.
    let hidden = SyntheticVisibilityAdapter::new(&remote.catalog);
    let hidden_gate = ScopedDisclosureGate::new(&remote.authority, &hidden);
    let mut invisible = run(
        &remote,
        &hidden,
        &Lookup,
        &remote.binding,
        source,
        ManualClock::new(),
    )
    .await;
    let invisible = disclose(invisible.disclosure.as_mut().unwrap(), &hidden_gate)
        .await
        .unwrap();
    let mut missing = run(
        &remote,
        &hidden,
        &Lookup,
        &remote.binding,
        SourceId::from_uuid(Uuid::from_u128(9_999)),
        ManualClock::new(),
    )
    .await;
    let missing = disclose(missing.disclosure.as_mut().unwrap(), &hidden_gate)
        .await
        .unwrap();

    assert_eq!(revoked, invisible);
    assert_eq!(revoked, missing);
    assert!(revoked.qualified.is_empty());
    assert!(!mentions(&revoked, source));
    assert!(
        revoked
            .gaps
            .iter()
            .any(|(fact, _, blocking)| fact == "required_source_unavailable" && *blocking)
    );
    assert_ne!(revoked.sufficiency, EvidenceSufficiency::Sufficient);
}

#[tokio::test]
async fn source_visibility_revoked_removes_claim_rank_trace_locator() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    let principal = remote.binding.actor().principal().clone();
    let visibility = remote.visibility();
    let gate = ScopedDisclosureGate::new(&remote.authority, &visibility);
    let evaluate = || {
        run(
            &remote,
            &visibility,
            &Lookup,
            &remote.binding,
            source,
            ManualClock::new(),
        )
    };

    // While visible, the Source supplies ranked, Supported, traced output.
    let mut visible = evaluate().await;
    let shown = disclose(visible.disclosure.as_mut().unwrap(), &gate)
        .await
        .unwrap();
    assert_eq!(shown.sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(shown.qualified.len(), 2);
    assert!(
        shown
            .claims
            .iter()
            .all(|(_, state, roles)| *state == ClaimState::Supported
                && roles.contains(&EvidenceRole::Primary))
    );
    assert!(mentions(&shown, source));

    // Revoked after the result was built: the final gate refuses it whole.
    let mut pending = evaluate().await;
    visibility.revoke(&principal, source).unwrap();
    let invoked = AtomicBool::new(false);
    let refused = pending
        .disclosure
        .as_mut()
        .unwrap()
        .with_disclosure(&gate, |_| {
            invoked.store(true, Ordering::SeqCst);
            Ok(())
        })
        .await;
    assert!(refused.is_err());
    assert!(!invoked.load(Ordering::SeqCst));
    assert_eq!(
        pending.disclosure.as_ref().unwrap().state(),
        LeaseState::Closed
    );

    // Recomputed while revoked: no claim, rank, trace or locator remains.
    let mut recomputed = evaluate().await;
    assert!(recomputed.batches.is_empty());
    let shape = disclose(recomputed.disclosure.as_mut().unwrap(), &gate)
        .await
        .unwrap();
    assert!(shape.qualified.is_empty());
    assert!(
        shape
            .claims
            .iter()
            .all(|(_, state, roles)| *state != ClaimState::Supported && roles.is_empty())
    );
    assert!(!mentions(&shape, source));
    assert!(
        !shape
            .trace
            .iter()
            .any(|entry| entry.starts_with("candidate:"))
    );
}

#[tokio::test]
async fn no_retention_callback_success_error_cancel_closes_both_leases() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let source = remote.registration.source_id();
    let visibility = remote.visibility();
    let gate = ScopedDisclosureGate::new(&remote.authority, &visibility);
    let clock = ManualClock::new();
    let evaluate = || {
        run(
            &remote,
            &visibility,
            &Lookup,
            &remote.binding,
            source,
            clock.clone(),
        )
    };

    // Success: the evaluation lease closed at return, the disclosure lease
    // closes after one callback.
    let mut success = evaluate().await.disclosure.unwrap();
    assert!(success.evaluation_closed());
    assert_eq!(success.state(), LeaseState::Open);
    assert!(disclose(&mut success, &gate).await.is_ok());
    assert_eq!(success.state(), LeaseState::Closed);
    assert!(disclose(&mut success, &gate).await.is_err());

    // Callback error.
    let mut failed = evaluate().await.disclosure.unwrap();
    let error = failed
        .with_disclosure(&gate, |_| {
            Err(SearchError::OperationFailed("caller failed".into()))
        })
        .await;
    assert!(error.is_err());
    assert_eq!(failed.state(), LeaseState::Closed);
    assert!(disclose(&mut failed, &gate).await.is_err());

    // Cancelled while the final gate is pending.
    let mut cancelled = evaluate().await.disclosure.unwrap();
    let invoked = AtomicBool::new(false);
    let timed_out = tokio::time::timeout(
        Duration::from_millis(20),
        cancelled.with_disclosure(&Pending, |_| {
            invoked.store(true, Ordering::SeqCst);
            Ok(())
        }),
    )
    .await;
    assert!(timed_out.is_err());
    assert!(!invoked.load(Ordering::SeqCst));
    assert_eq!(cancelled.state(), LeaseState::Closed);
    assert!(disclose(&mut cancelled, &gate).await.is_err());

    // Not read in time: the short disclosure lease expires.
    let mut late = evaluate().await.disclosure.unwrap();
    clock.advance(Duration::from_secs(31));
    assert_eq!(late.state(), LeaseState::Expired);
    assert!(disclose(&mut late, &gate).await.is_err());
    assert!(format!("{late:?}").contains("<transient>"));
}
