//! P4-14: remote representation binding and target revalidation.

#[path = "support/remote.rs"]
mod support;

use std::sync::Mutex;

use search_application::SearchError;
use search_application::materialization::MaterializationBudget;
use search_application::ports::{AccessDecision, BoxFuture, CurrentSourcePolicy};
use search_application::remote::{
    EvaluationLeaseId, PinnedRemoteTarget, PlannedRemoteAction, RemoteAccessTarget,
    RemoteActionOutcome, RemoteIdentity, RemoteReadOutcome, RemoteSourcePort, RemoteUnknownReason,
    TrustedRemoteContext, UntrustedRemoteHit,
};
use search_application::remote_binding::RemoteBindingService;
use search_application::remote_generation::{RemoteEvaluationGeneration, RemoteGenerationBuilder};
use search_application::scoped::SyntheticVisibilityAdapter;
use search_core::binding::{BindingMode, RevalidationMarker};
use search_core::discovery::FederatedCandidate;
use search_core::materialization::{MaterializationState, ProviderContentPermission};
use search_core::resource::ResourceKind;
use search_core::source::RetentionMode;
use support::*;

/// The fixed Source's registered adapter: current access and policy, and a
/// read that answers with a scripted observation of the pinned identity.
struct Port {
    access: AccessDecision,
    permission: ProviderContentPermission,
    observed: Option<PinnedRemoteTarget>,
    reads: Mutex<Vec<String>>,
}

impl Port {
    fn new(observed: Option<PinnedRemoteTarget>) -> Self {
        Self {
            access: AccessDecision::Allowed,
            permission: ProviderContentPermission::FullContent,
            observed,
            reads: Mutex::new(vec![]),
        }
    }
}

impl RemoteSourcePort for Port {
    fn execute_batch<'a>(
        &'a self,
        _: &'a TrustedRemoteContext,
        _: &'a [PlannedRemoteAction],
    ) -> BoxFuture<'a, Vec<RemoteActionOutcome>> {
        Box::pin(async { Err(SearchError::SourceUnavailable("not used".into())) })
    }
    fn current_access<'a>(
        &'a self,
        _: &'a TrustedRemoteContext,
        _: &'a RemoteAccessTarget,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move { Ok(self.access) })
    }
    fn current_policy<'a>(
        &'a self,
        context: &'a TrustedRemoteContext,
        _: &'a RemoteIdentity,
    ) -> BoxFuture<'a, CurrentSourcePolicy> {
        Box::pin(async move {
            Ok(CurrentSourcePolicy {
                resource_kind: ResourceKind::Knowledge,
                provider_permission: self.permission,
                retention_mode: context.registration().retention_mode(),
                probe_allowed: false,
            })
        })
    }
    fn probe_or_materialize<'a>(
        &'a self,
        _: &'a TrustedRemoteContext,
        target: &'a PinnedRemoteTarget,
        stage: MaterializationState,
    ) -> BoxFuture<'a, RemoteReadOutcome> {
        Box::pin(async move {
            self.reads
                .lock()
                .unwrap()
                .push(target.identity().native_id().as_str().to_owned());
            Ok(match &self.observed {
                Some(observed) => RemoteReadOutcome::Observed {
                    target: Box::new(observed.clone()),
                    state: stage,
                },
                None => RemoteReadOutcome::Unknown(RemoteUnknownReason::Unavailable),
            })
        })
    }
}

fn budget() -> MaterializationBudget {
    MaterializationBudget {
        max_content_bytes: 4_096,
        max_latency_ms: 1_000,
        max_remote_calls: 1,
        max_monetary_cost_minor_units: 0,
        currency: "USD".into(),
        direct_full_max_bytes: 4_096,
    }
}

struct Sealed {
    context: TrustedRemoteContext,
    generation: RemoteEvaluationGeneration,
    candidate: FederatedCandidate,
    target: Option<PinnedRemoteTarget>,
}

/// Seals one lookup of `doc-1` and pins the target from the same response.
async fn sealed(
    remote: &Remote,
    visibility: &SyntheticVisibilityAdapter<'_>,
    hit: UntrustedRemoteHit,
) -> Sealed {
    let context = remote.context(visibility).await;
    let response = observe(
        remote,
        visibility,
        &Verifier::shared("snapshot-1"),
        &context,
        "lookup",
        lookup("doc-1"),
        vec![hit],
    )
    .await;
    let target = PinnedRemoteTarget::from_response(&context, &response, &native("doc-1")).ok();
    let mut builder = RemoteGenerationBuilder::new(
        context.clone(),
        remote.evaluation(),
        EvaluationLeaseId::new(),
    )
    .unwrap();
    builder.stage(response).unwrap();
    let generation = builder.seal().unwrap();
    let candidate = generation.candidates("lookup").unwrap()[0].clone();
    Sealed {
        context,
        generation,
        candidate,
        target,
    }
}

/// A later observation of `doc-1` with its own version and digest.
async fn observed(
    remote: &Remote,
    visibility: &SyntheticVisibilityAdapter<'_>,
    context: &TrustedRemoteContext,
    version: &str,
    digest: &str,
) -> PinnedRemoteTarget {
    let response = observe(
        remote,
        visibility,
        &Verifier::shared("snapshot-2"),
        context,
        "lookup",
        lookup("doc-1"),
        vec![hit(Some("doc-1"), Some(version), Some(digest), &[])],
    )
    .await;
    PinnedRemoteTarget::from_response(context, &response, &native("doc-1")).unwrap()
}

#[tokio::test]
async fn changed_live_digest_requires_new_qualification() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let sealed = sealed(
        &remote,
        &visibility,
        hit(Some("doc-1"), Some("v1"), Some("d1"), &[]),
    )
    .await;
    let target = sealed.target.clone().unwrap();
    let unchanged = Port::new(Some(
        observed(&remote, &visibility, &sealed.context, "v1", "d1").await,
    ));
    let service = RemoteBindingService::new(&unchanged, &remote.authority, &visibility);
    let binding = service
        .bind_live(
            &sealed.context,
            &sealed.generation,
            &sealed.candidate,
            &target,
        )
        .unwrap();
    assert_eq!(binding.binding_mode, BindingMode::LiveReference);
    assert_eq!(binding.revalidation_marker, RevalidationMarker::Required);
    assert_eq!(binding.content_digest.as_deref(), Some("d1"));
    assert!(matches!(
        service
            .revalidate_target(
                &sealed.context,
                &target,
                MaterializationState::Metadata,
                &budget()
            )
            .await
            .unwrap(),
        RemoteReadOutcome::Observed { .. }
    ));
    // The live Resource changed: no silent rebind, a new qualification.
    for (version, digest) in [("v1", "d2"), ("v2", "d1")] {
        let changed = Port::new(Some(
            observed(&remote, &visibility, &sealed.context, version, digest).await,
        ));
        let service = RemoteBindingService::new(&changed, &remote.authority, &visibility);
        assert_eq!(
            service
                .revalidate_target(
                    &sealed.context,
                    &target,
                    MaterializationState::Metadata,
                    &budget()
                )
                .await
                .unwrap(),
            RemoteReadOutcome::Unknown(RemoteUnknownReason::SnapshotIncompatible)
        );
    }
}

#[tokio::test]
async fn missing_version_and_digest_refuses_live_bind() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let bare = sealed(&remote, &visibility, hit(Some("doc-1"), None, None, &[])).await;
    // Neither version nor digest: no target can be pinned at all.
    assert!(bare.target.is_none());
    // A target pinned elsewhere never binds to the bare sealed Resource.
    let elsewhere = observed(&remote, &visibility, &bare.context, "v1", "d1").await;
    let port = Port::new(None);
    let service = RemoteBindingService::new(&port, &remote.authority, &visibility);
    assert!(
        service
            .bind_live(&bare.context, &bare.generation, &bare.candidate, &elsewhere)
            .is_err()
    );
    // Nor does a target of another candidate or generation.
    let pinned = sealed(
        &remote,
        &visibility,
        hit(Some("doc-1"), Some("v1"), Some("d1"), &[]),
    )
    .await;
    assert!(
        service
            .bind_live(
                &pinned.context,
                &bare.generation,
                &pinned.candidate,
                pinned.target.as_ref().unwrap()
            )
            .is_err()
    );
}

#[tokio::test]
async fn remote_version_pin_requires_version() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let port = Port::new(None);
    let service = RemoteBindingService::new(&port, &remote.authority, &visibility);
    let digest_only = sealed(
        &remote,
        &visibility,
        hit(Some("doc-1"), None, Some("d1"), &[]),
    )
    .await;
    let target = digest_only.target.clone().unwrap();
    assert!(
        service
            .bind(
                &digest_only.context,
                &digest_only.generation,
                &digest_only.candidate,
                &target,
                BindingMode::RemoteVersionPinned,
            )
            .is_err()
    );
    let versioned = sealed(
        &remote,
        &visibility,
        hit(Some("doc-1"), Some("v1"), None, &[]),
    )
    .await;
    let binding = service
        .bind(
            &versioned.context,
            &versioned.generation,
            &versioned.candidate,
            versioned.target.as_ref().unwrap(),
            BindingMode::RemoteVersionPinned,
        )
        .unwrap();
    assert!(binding.resource_version_ref.is_some());
    assert_eq!(binding.revalidation_marker, RevalidationMarker::NotRequired);
}

#[tokio::test]
async fn snapshot_pin_requires_version_or_digest() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let port = Port::new(None);
    let service = RemoteBindingService::new(&port, &remote.authority, &visibility);
    for (version, digest) in [
        (Some("v1"), None),
        (None, Some("d1")),
        (Some("v1"), Some("d1")),
    ] {
        let one = sealed(
            &remote,
            &visibility,
            hit(Some("doc-1"), version, digest, &[]),
        )
        .await;
        let binding = service
            .bind(
                &one.context,
                &one.generation,
                &one.candidate,
                one.target.as_ref().unwrap(),
                BindingMode::SnapshotPinned,
            )
            .unwrap();
        assert!(binding.validate().is_ok());
        assert_eq!(binding.content_digest.as_deref(), digest);
        assert_eq!(binding.resource_version_ref.is_some(), version.is_some());
        // A session snapshot is the session working set's, not a remote pin.
        assert!(
            service
                .bind(
                    &one.context,
                    &one.generation,
                    &one.candidate,
                    one.target.as_ref().unwrap(),
                    BindingMode::SessionSnapshot,
                )
                .is_err()
        );
    }
}

#[tokio::test]
async fn revoked_policy_or_budget_denies_materialization() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let sealed = sealed(
        &remote,
        &visibility,
        hit(Some("doc-1"), Some("v1"), Some("d1"), &[]),
    )
    .await;
    let target = sealed.target.clone().unwrap();
    let current = observed(&remote, &visibility, &sealed.context, "v1", "d1").await;

    // Policy below the requested stage, and item access denied.
    let mut metadata_only = Port::new(Some(current.clone()));
    metadata_only.permission = ProviderContentPermission::Metadata;
    let mut item_denied = Port::new(Some(current.clone()));
    item_denied.access = AccessDecision::Denied;
    for port in [&metadata_only, &item_denied] {
        let service = RemoteBindingService::new(port, &remote.authority, &visibility);
        assert!(
            service
                .revalidate_target(
                    &sealed.context,
                    &target,
                    MaterializationState::FullContent,
                    &budget()
                )
                .await
                .is_err()
        );
        assert!(port.reads.lock().unwrap().is_empty());
    }
    // No budget for a remote call or for content bytes.
    let port = Port::new(Some(current.clone()));
    let service = RemoteBindingService::new(&port, &remote.authority, &visibility);
    let mut no_calls = budget();
    no_calls.max_remote_calls = 0;
    let mut no_bytes = budget();
    no_bytes.max_content_bytes = 0;
    for budget in [no_calls, no_bytes] {
        assert!(
            service
                .revalidate_target(
                    &sealed.context,
                    &target,
                    MaterializationState::FullContent,
                    &budget
                )
                .await
                .is_err()
        );
    }
    assert!(port.reads.lock().unwrap().is_empty());
    // Source visibility revoked.
    visibility
        .revoke(
            remote.binding.actor().principal(),
            remote.registration.source_id(),
        )
        .unwrap();
    assert!(
        service
            .revalidate_target(
                &sealed.context,
                &target,
                MaterializationState::Metadata,
                &budget()
            )
            .await
            .is_err()
    );
    assert!(port.reads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn provider_locator_is_never_fetch_target() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let locator = "https://elsewhere.example.test/raw/doc-1";
    let sealed = sealed(
        &remote,
        &visibility,
        hit(
            Some("doc-1"),
            Some("v1"),
            Some("d1"),
            &[("locator", locator, None), ("self_link", locator, None)],
        ),
    )
    .await;
    let target = sealed.target.clone().unwrap();
    let port = Port::new(Some(
        observed(&remote, &visibility, &sealed.context, "v1", "d1").await,
    ));
    let service = RemoteBindingService::new(&port, &remote.authority, &visibility);
    let binding = service
        .bind_live(
            &sealed.context,
            &sealed.generation,
            &sealed.candidate,
            &target,
        )
        .unwrap();
    assert_eq!(binding.provider_ref.as_deref(), Some("synthetic"));
    assert!(!format!("{binding:?}").contains("elsewhere"));
    service
        .revalidate_target(
            &sealed.context,
            &target,
            MaterializationState::Metadata,
            &budget(),
        )
        .await
        .unwrap();
    // The registered adapter is asked only for the pinned stable identity.
    assert_eq!(*port.reads.lock().unwrap(), vec!["doc-1".to_owned()]);
}
