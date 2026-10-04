use std::sync::Arc;
use std::time::Duration;

use search_application::ports::BoxFuture;
use search_application::remote::{
    OpaqueCursor, PlannedRemoteAction, RemoteActionOutcome, RemoteOperation, RemotePage,
    RemoteResponseInput, RemoteResponseStatus, TrustedRemoteContext, UntrustedRemoteHit,
};
use search_application::remote_observation::{
    CheckedRemoteObservationAdapter, RemoteSnapshotVerifierPort, SnapshotAttestation,
    SnapshotExtent, verify_absence,
};
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig,
};
use search_application::retrieval::{LiveInput, OpaqueNativeId, RemoteQueryInput};
use search_application::scoped::{
    AccessContextAuthorityPort, AccessRevision, CurrentSourceVisibilityPort, PrincipalRef,
    RegistrationRevision, ScopedSourceRegistryPort, SyntheticAuthorityAdapter,
    SyntheticVisibilityAdapter, TenantId, TrustedDiscoveryBinding, VisibilityRevision,
    VisibleSourceRegistration,
};
use search_application::source_registration::{
    CompleteDesiredRegistrations, RegistrationNamespace, RegistrationSetRevision,
    SourceRegistration, SourceRegistrationCatalog, SyntheticHostRegistrationAuthority,
    SyntheticRegistrationLedger, TrustedVisibleRegistry,
};
use search_core::id::{DiscoveryEvaluationId, SourceId};
use search_core::observation::{Coverage, Presence};
use search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use time::OffsetDateTime;
use uuid::Uuid;

fn native(value: &str) -> OpaqueNativeId {
    OpaqueNativeId::new(value).unwrap()
}

struct Fixture {
    registration: RemoteSourceRegistration,
    authority: SyntheticAuthorityAdapter,
    catalog: SourceRegistrationCatalog,
    binding: TrustedDiscoveryBinding,
    visible: VisibleSourceRegistration,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_semantics(EnumerationSemantics::Complete).await
    }

    async fn with_semantics(enumeration_semantics: EnumerationSemantics) -> Self {
        let registration =
            RemoteSourceRegistration::from_server_config(ServerRemoteRegistrationConfig {
                tenant: TenantId::new("test-tenant").unwrap(),
                source_id: SourceId::from_uuid(Uuid::from_u128(1)),
                provider_kind: "synthetic".into(),
                endpoint: RegisteredEndpoint::new("https", "catalog.example.test", 443, "/v1")
                    .unwrap(),
                supported_modes: vec![
                    DiscoveryMode::RemoteEnumeration,
                    DiscoveryMode::RemoteQuery,
                    DiscoveryMode::DirectAddress,
                    DiscoveryMode::LiveOnly,
                ],
                enumeration_semantics,
                authority_predicates: vec!["provider.claimed.absence".into()],
                allowed_resource_kinds: vec![],
                current_access_contract: CurrentAccessContract::PublicReadWithFieldPolicy,
                retention_mode: RetentionMode::SessionOnly,
                freshness_policy: None,
                canonical_upstream_lineage: "synthetic".into(),
                limits: RemoteRegistrationLimits::synthetic_canary(),
                registration_revision: RegistrationRevision::new(1).unwrap(),
                visibility_revision: VisibilityRevision::new(1).unwrap(),
            })
            .unwrap();
        let host = Arc::new(SyntheticHostRegistrationAuthority::new());
        for (namespace, values) in [
            (RegistrationNamespace::Document, vec![]),
            (
                RegistrationNamespace::Remote,
                vec![SourceRegistration::Remote(registration.clone())],
            ),
        ] {
            host.publish(namespace, RegistrationSetRevision::new(1).unwrap(), values)
                .unwrap();
        }
        let document =
            CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Document)
                .await
                .unwrap();
        let remote = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote)
            .await
            .unwrap();
        let catalog = SourceRegistrationCatalog::try_new(
            Arc::new(SyntheticRegistrationLedger::with_host(host)),
            &document,
            &remote,
        )
        .await
        .unwrap();
        let authority = SyntheticAuthorityAdapter::new();
        let handle = authority
            .issue_verified_identity(
                registration.tenant().clone(),
                PrincipalRef::new("reader").unwrap(),
                None,
                AccessRevision::new(1).unwrap(),
                Duration::from_secs(60),
            )
            .unwrap();
        let actor = authority.resolve(&handle).await.unwrap().unwrap();
        let binding = authority
            .bind_discovery(
                &actor,
                DiscoveryEvaluationId::from_uuid(Uuid::from_u128(50)),
            )
            .await
            .unwrap()
            .unwrap();
        let visibility = SyntheticVisibilityAdapter::new(&catalog);
        visibility
            .grant(
                registration.tenant().clone(),
                actor.principal().clone(),
                registration.source_id(),
                registration.registration_revision(),
                registration.visibility_revision(),
            )
            .unwrap();
        let registry = TrustedVisibleRegistry::new(&authority, &visibility, &catalog);
        let visible = registry.visible_sources(&actor).await.unwrap()[0].clone();
        Self {
            registration,
            authority,
            catalog,
            binding,
            visible,
        }
    }

    fn visibility(&self) -> SyntheticVisibilityAdapter<'_> {
        let visibility = SyntheticVisibilityAdapter::new(&self.catalog);
        visibility
            .grant(
                self.registration.tenant().clone(),
                self.binding.actor().principal().clone(),
                self.registration.source_id(),
                self.registration.registration_revision(),
                self.registration.visibility_revision(),
            )
            .unwrap();
        visibility
    }

    async fn context(&self, visibility: &dyn CurrentSourceVisibilityPort) -> TrustedRemoteContext {
        TrustedRemoteContext::bind(
            self.binding.clone(),
            &self.visible,
            &self.authority,
            visibility,
        )
        .await
        .unwrap()
    }
}

struct Verifier {
    snapshot: &'static str,
    extent: SnapshotExtent,
    accepted: bool,
}
impl RemoteSnapshotVerifierPort for Verifier {
    fn verify<'a>(
        &'a self,
        _context: &'a TrustedRemoteContext,
        _action: &'a PlannedRemoteAction,
        _input: &'a RemoteResponseInput,
    ) -> BoxFuture<'a, Option<SnapshotAttestation>> {
        Box::pin(async move {
            if !self.accepted {
                return Ok(None);
            }
            Ok(Some(SnapshotAttestation::new(
                self.snapshot,
                self.extent,
                OffsetDateTime::now_utc(),
                vec![native("known-missing")],
            )?))
        })
    }
}
fn verifier() -> Verifier {
    Verifier {
        snapshot: "snapshot-one",
        extent: SnapshotExtent::CompleteSource,
        accepted: true,
    }
}
fn input(page: RemotePage, hits: &[&str], status: RemoteResponseStatus) -> RemoteResponseInput {
    RemoteResponseInput::new(
        status,
        page,
        hits.iter()
            .map(|value| UntrustedRemoteHit::new(Some(native(value)), None, None).unwrap())
            .collect(),
        Some(0),
    )
    .unwrap()
}
fn page(cursor: Option<&str>, next: Option<&str>, terminal: bool) -> RemotePage {
    RemotePage::Enumeration {
        requested: cursor.map(|v| OpaqueCursor::new(v).unwrap()),
        next: next.map(|v| OpaqueCursor::new(v).unwrap()),
        terminal,
    }
}
fn completed(outcome: RemoteActionOutcome) -> search_application::remote::RemoteActionResponse {
    match outcome {
        RemoteActionOutcome::Completed(response) => *response,
        other => panic!("expected completion, got {other:?}"),
    }
}

#[tokio::test]
async fn query_and_live_miss_are_unknown() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    for operation in [
        RemoteOperation::Query {
            input: RemoteQueryInput::new("query", vec![], 10, &[]).unwrap(),
        },
        RemoteOperation::Live {
            input: LiveInput::lookup(native("known-missing")),
        },
    ] {
        let action = PlannedRemoteAction::new(&context, "search", operation).unwrap();
        let outcome = adapter
            .observe(
                &context,
                &action,
                input(RemotePage::Unpaged, &[], RemoteResponseStatus::Success),
            )
            .await
            .unwrap();
        assert_eq!(
            outcome.presence(&native("known-missing")),
            Presence::Unknown
        );
        let response = completed(outcome);
        assert!(
            verify_absence(
                &f.registration,
                &context,
                &[response],
                &native("known-missing")
            )
            .is_none()
        );
    }
}

#[tokio::test]
async fn only_terminal_consistent_enumeration_proves_absence() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    let first = completed(
        adapter
            .observe(
                &context,
                &action,
                input(
                    page(None, Some("page-two"), false),
                    &["present"],
                    RemoteResponseStatus::Success,
                ),
            )
            .await
            .unwrap(),
    );
    let last = completed(
        adapter
            .observe(
                &context,
                &action,
                input(
                    page(Some("page-two"), None, true),
                    &[],
                    RemoteResponseStatus::Success,
                ),
            )
            .await
            .unwrap(),
    );
    assert_eq!(first.coverage(), Coverage::PartialEnumeration);
    assert!(
        verify_absence(
            &f.registration,
            &context,
            std::slice::from_ref(&first),
            &native("known-missing")
        )
        .is_none()
    );
    let receipt = verify_absence(
        &f.registration,
        &context,
        &[first.clone(), last.clone()],
        &native("known-missing"),
    )
    .unwrap();
    assert_eq!(receipt.presence(), Presence::Absent);
    assert_eq!(receipt.coverage(), Coverage::CompleteEnumeration);
    assert_eq!(receipt.native_id(), &native("known-missing"));
    assert!(
        verify_absence(
            &f.registration,
            &context,
            &[first.clone(), last.clone()],
            &native("unknown-to-source")
        )
        .is_none()
    );
    assert!(
        verify_absence(
            &f.registration,
            &context,
            &[first.clone(), last.clone()],
            &native("present")
        )
        .is_none()
    );
    assert!(
        verify_absence(
            &f.registration,
            &context,
            &[last.clone(), first.clone()],
            &native("known-missing")
        )
        .is_none()
    );
    assert!(
        verify_absence(
            &f.registration,
            &context,
            &[first.clone(), first, last],
            &native("known-missing")
        )
        .is_none()
    );
}

#[tokio::test]
async fn plain_404_403_and_partial_page_do_not_prove_absence() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    for status in [
        RemoteResponseStatus::NotFound,
        RemoteResponseStatus::Forbidden,
        RemoteResponseStatus::Partial,
    ] {
        let action = PlannedRemoteAction::new(
            &context,
            "lookup",
            RemoteOperation::Lookup {
                native_id: native("known-missing"),
            },
        )
        .unwrap();
        let outcome = adapter
            .observe(&context, &action, input(RemotePage::Unpaged, &[], status))
            .await
            .unwrap();
        assert_eq!(
            outcome.presence(&native("known-missing")),
            Presence::Unknown
        );
    }
    let action = PlannedRemoteAction::new(
        &context,
        "enumeration",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    let outcome = adapter
        .observe(
            &context,
            &action,
            input(page(None, None, true), &[], RemoteResponseStatus::Partial),
        )
        .await
        .unwrap();
    assert_eq!(
        outcome.presence(&native("known-missing")),
        Presence::Unknown
    );
    if let RemoteActionOutcome::Completed(response) = outcome {
        assert!(
            verify_absence(
                &f.registration,
                &context,
                &[*response],
                &native("known-missing")
            )
            .is_none()
        );
    }
}

#[tokio::test]
async fn outage_does_not_mutate_resource_state() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let source_before = f.registration.clone();
    let action = PlannedRemoteAction::new(
        &context,
        "query",
        RemoteOperation::Query {
            input: RemoteQueryInput::new("query", vec![], 10, &[]).unwrap(),
        },
    )
    .unwrap();
    for status in [
        RemoteResponseStatus::Timeout,
        RemoteResponseStatus::Unavailable,
        RemoteResponseStatus::Malformed,
    ] {
        let outcome = adapter
            .observe(&context, &action, input(RemotePage::Unpaged, &[], status))
            .await
            .unwrap();
        assert_eq!(
            outcome.presence(&native("known-missing")),
            Presence::Unknown
        );
        assert!(matches!(outcome, RemoteActionOutcome::Unknown { .. }));
    }
    assert_eq!(f.registration, source_before);
}

#[tokio::test]
async fn provider_snapshot_or_total_alone_cannot_create_absence() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = Verifier {
        accepted: false,
        ..verifier()
    };
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    let outcome = adapter
        .observe(
            &context,
            &action,
            input(page(None, None, true), &[], RemoteResponseStatus::Success),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, RemoteActionOutcome::Unknown { .. }));
    assert_eq!(
        outcome.presence(&native("known-missing")),
        Presence::Unknown
    );
}

#[tokio::test]
async fn different_snapshot_or_evaluation_cannot_join_enumeration_pages() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let v1 = verifier();
    let v2 = Verifier {
        snapshot: "snapshot-two",
        ..verifier()
    };
    let a1 = CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &v1);
    let a2 = CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &v2);
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    let first = completed(
        a1.observe(
            &context,
            &action,
            input(
                page(None, Some("next"), false),
                &[],
                RemoteResponseStatus::Success,
            ),
        )
        .await
        .unwrap(),
    );
    let last = completed(
        a2.observe(
            &context,
            &action,
            input(
                page(Some("next"), None, true),
                &[],
                RemoteResponseStatus::Success,
            ),
        )
        .await
        .unwrap(),
    );
    assert!(
        verify_absence(
            &f.registration,
            &context,
            &[first, last],
            &native("known-missing")
        )
        .is_none()
    );
    let other_binding = f
        .authority
        .bind_discovery(
            f.binding.actor(),
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(51)),
        )
        .await
        .unwrap()
        .unwrap();
    let other = TrustedRemoteContext::bind(other_binding, &f.visible, &f.authority, &visibility)
        .await
        .unwrap();
    assert!(
        a1.observe(
            &other,
            &action,
            input(page(None, None, true), &[], RemoteResponseStatus::Success)
        )
        .await
        .is_err()
    );
}

struct RevokingVerifier<'a> {
    authority: &'a SyntheticAuthorityAdapter,
}
impl RemoteSnapshotVerifierPort for RevokingVerifier<'_> {
    fn verify<'a>(
        &'a self,
        context: &'a TrustedRemoteContext,
        _action: &'a PlannedRemoteAction,
        _input: &'a RemoteResponseInput,
    ) -> BoxFuture<'a, Option<SnapshotAttestation>> {
        Box::pin(async move {
            self.authority
                .revoke(context.binding().actor().access_handle())?;
            Ok(Some(SnapshotAttestation::new(
                "snapshot-one",
                SnapshotExtent::CompleteSource,
                OffsetDateTime::now_utc(),
                vec![native("known-missing")],
            )?))
        })
    }
}

#[tokio::test]
async fn revocation_during_verification_cannot_issue_response() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = RevokingVerifier {
        authority: &f.authority,
    };
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    assert!(
        adapter
            .observe(
                &context,
                &action,
                input(page(None, None, true), &[], RemoteResponseStatus::Success)
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn idless_or_duplicate_hits_cannot_prove_a_known_id_absent() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    for hits in [
        vec![UntrustedRemoteHit::new(None, None, None).unwrap()],
        vec![UntrustedRemoteHit::new(Some(native("duplicate")), None, None).unwrap(); 2],
    ] {
        let response = completed(
            adapter
                .observe(
                    &context,
                    &action,
                    RemoteResponseInput::new(
                        RemoteResponseStatus::Success,
                        page(None, None, true),
                        hits,
                        None,
                    )
                    .unwrap(),
                )
                .await
                .unwrap(),
        );
        assert!(
            verify_absence(
                &f.registration,
                &context,
                &[response],
                &native("known-missing")
            )
            .is_none()
        );
    }
}

#[tokio::test]
async fn unwired_or_single_response_verifier_cannot_prove_enumeration() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    let adapter =
        CheckedRemoteObservationAdapter::unwired(&f.registration, &f.authority, &visibility);
    assert!(matches!(
        adapter
            .observe(
                &context,
                &action,
                input(page(None, None, true), &[], RemoteResponseStatus::Success)
            )
            .await
            .unwrap(),
        RemoteActionOutcome::Unknown { .. }
    ));
    let verifier = Verifier {
        extent: SnapshotExtent::SingleResponse,
        ..verifier()
    };
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let response = completed(
        adapter
            .observe(
                &context,
                &action,
                input(page(None, None, true), &[], RemoteResponseStatus::Success),
            )
            .await
            .unwrap(),
    );
    assert!(
        !response
            .source_snapshot_proof()
            .same_source_snapshot(response.source_snapshot_proof())
    );
    assert!(
        verify_absence(
            &f.registration,
            &context,
            &[response],
            &native("known-missing")
        )
        .is_none()
    );
}

#[tokio::test]
async fn source_revocation_prevents_consuming_old_absence() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    let response = completed(
        adapter
            .observe(
                &context,
                &action,
                input(page(None, None, true), &[], RemoteResponseStatus::Success),
            )
            .await
            .unwrap(),
    );
    assert!(
        adapter
            .verify_absence(
                &context,
                std::slice::from_ref(&response),
                &native("known-missing")
            )
            .await
            .unwrap()
            .is_some()
    );
    visibility
        .revoke(f.binding.actor().principal(), f.registration.source_id())
        .unwrap();
    assert!(
        adapter
            .verify_absence(&context, &[response], &native("known-missing"))
            .await
            .is_err()
    );
    assert!(
        adapter
            .observe(
                &context,
                &action,
                input(page(None, None, true), &[], RemoteResponseStatus::Success)
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn batch_rejects_duplicate_actions_foreign_evaluation_and_excess_fanout() {
    use search_application::remote::validate_remote_batch;
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    assert!(validate_remote_batch(&context, std::slice::from_ref(&action)).is_ok());
    assert!(validate_remote_batch(&context, &[]).is_err());
    assert!(validate_remote_batch(&context, &[action.clone(), action.clone()]).is_err());
    let actions = (0..5)
        .map(|i| {
            PlannedRemoteAction::new(
                &context,
                format!("catalog-{i}"),
                RemoteOperation::Enumerate { cursor: None },
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(validate_remote_batch(&context, &actions).is_err());
    let other_binding = f
        .authority
        .bind_discovery(
            f.binding.actor(),
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(52)),
        )
        .await
        .unwrap()
        .unwrap();
    let other = TrustedRemoteContext::bind(other_binding, &f.visible, &f.authority, &visibility)
        .await
        .unwrap();
    assert!(validate_remote_batch(&other, &[action]).is_err());
}

#[tokio::test]
async fn pinned_target_requires_observed_exact_id_version_or_digest() {
    use search_application::remote::PinnedRemoteTarget;
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "lookup",
        RemoteOperation::Lookup {
            native_id: native("present"),
        },
    )
    .unwrap();
    let response = completed(
        adapter
            .observe(
                &context,
                &action,
                RemoteResponseInput::new(
                    RemoteResponseStatus::Success,
                    RemotePage::Unpaged,
                    vec![
                        UntrustedRemoteHit::new(
                            Some(native("present")),
                            Some("v1".into()),
                            Some("digest-one".into()),
                        )
                        .unwrap(),
                    ],
                    None,
                )
                .unwrap(),
            )
            .await
            .unwrap(),
    );
    let target =
        PinnedRemoteTarget::from_response(&context, &response, &native("present")).unwrap();
    assert_eq!(target.identity().source_scope(), context.source_scope());
    assert_eq!(target.identity().native_id(), &native("present"));
    assert_eq!(target.version(), Some("v1"));
    assert_eq!(target.digest(), Some("digest-one"));
    assert!(PinnedRemoteTarget::from_response(&context, &response, &native("unobserved")).is_err());
    let missing_pin = completed(
        adapter
            .observe(
                &context,
                &action,
                input(
                    RemotePage::Unpaged,
                    &["present"],
                    RemoteResponseStatus::Success,
                ),
            )
            .await
            .unwrap(),
    );
    assert!(PinnedRemoteTarget::from_response(&context, &missing_pin, &native("present")).is_err());
    assert!(matches!(
        adapter
            .observe(
                &context,
                &action,
                input(
                    RemotePage::Unpaged,
                    &["foreign-id"],
                    RemoteResponseStatus::Success
                )
            )
            .await
            .unwrap(),
        RemoteActionOutcome::Unknown { .. }
    ));
}

#[tokio::test]
async fn verifier_cannot_upgrade_registration_enumeration_semantics() {
    let f = Fixture::with_semantics(EnumerationSemantics::Partial).await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    let outcome = adapter
        .observe(
            &context,
            &action,
            input(page(None, None, true), &[], RemoteResponseStatus::Success),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, RemoteActionOutcome::Unknown { .. }));
    assert_eq!(
        outcome.presence(&native("known-missing")),
        Presence::Unknown
    );
}

struct SourceRevokingVerifier<'a> {
    visibility: &'a SyntheticVisibilityAdapter<'a>,
}
impl RemoteSnapshotVerifierPort for SourceRevokingVerifier<'_> {
    fn verify<'a>(
        &'a self,
        context: &'a TrustedRemoteContext,
        _action: &'a PlannedRemoteAction,
        _input: &'a RemoteResponseInput,
    ) -> BoxFuture<'a, Option<SnapshotAttestation>> {
        Box::pin(async move {
            self.visibility.revoke(
                context.binding().actor().principal(),
                context.source_scope().source_id(),
            )?;
            Ok(Some(SnapshotAttestation::new(
                "snapshot-one",
                SnapshotExtent::CompleteSource,
                OffsetDateTime::now_utc(),
                vec![native("known-missing")],
            )?))
        })
    }
}

#[tokio::test]
async fn source_revocation_during_verification_cannot_issue_response() {
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = SourceRevokingVerifier {
        visibility: &visibility,
    };
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    assert!(
        adapter
            .observe(
                &context,
                &action,
                input(page(None, None, true), &[], RemoteResponseStatus::Success)
            )
            .await
            .is_err()
    );
}

fn substituted_registration(registration: &RemoteSourceRegistration) -> RemoteSourceRegistration {
    RemoteSourceRegistration::from_server_config(ServerRemoteRegistrationConfig {
        tenant: registration.tenant().clone(),
        source_id: registration.source_id(),
        provider_kind: registration.provider_kind().into(),
        endpoint: RegisteredEndpoint::new("https", "substituted.example.test", 443, "/v1").unwrap(),
        supported_modes: registration.supported_modes().to_vec(),
        enumeration_semantics: EnumerationSemantics::Complete,
        authority_predicates: registration.authority_predicates().to_vec(),
        allowed_resource_kinds: registration.allowed_resource_kinds().to_vec(),
        current_access_contract: registration.current_access_contract(),
        retention_mode: registration.retention_mode(),
        freshness_policy: registration.freshness_policy().map(str::to_owned),
        canonical_upstream_lineage: registration.canonical_upstream_lineage().into(),
        limits: registration.limits(),
        registration_revision: registration.registration_revision(),
        visibility_revision: registration.visibility_revision(),
    })
    .unwrap()
}

#[tokio::test]
async fn substituted_registration_never_matches_catalog_scope() {
    let f = Fixture::with_semantics(EnumerationSemantics::Partial).await;
    let visibility = f.visibility();
    let forged = substituted_registration(&f.registration);
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&forged, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "catalog",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    assert!(
        adapter
            .observe(
                &context,
                &action,
                input(page(None, None, true), &[], RemoteResponseStatus::Success)
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn conflicting_duplicate_hit_cannot_be_pinned() {
    use search_application::remote::PinnedRemoteTarget;
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let verifier = verifier();
    let adapter =
        CheckedRemoteObservationAdapter::new(&f.registration, &f.authority, &visibility, &verifier);
    let action = PlannedRemoteAction::new(
        &context,
        "query",
        RemoteOperation::Query {
            input: RemoteQueryInput::new("query", vec![], 10, &[]).unwrap(),
        },
    )
    .unwrap();
    let hits = ["digest-one", "digest-two"]
        .into_iter()
        .map(|digest| {
            UntrustedRemoteHit::new(
                Some(native("present")),
                Some("v1".into()),
                Some(digest.into()),
            )
            .unwrap()
        })
        .collect();
    let response = completed(
        adapter
            .observe(
                &context,
                &action,
                RemoteResponseInput::new(
                    RemoteResponseStatus::Success,
                    RemotePage::Unpaged,
                    hits,
                    None,
                )
                .unwrap(),
            )
            .await
            .unwrap(),
    );
    assert!(PinnedRemoteTarget::from_response(&context, &response, &native("present")).is_err());
}

#[tokio::test]
async fn remote_identity_debug_does_not_disclose_actor_or_native_id() {
    use search_application::remote::{RemoteAccessTarget, RemoteIdentity};
    let f = Fixture::new().await;
    let visibility = f.visibility();
    let context = f.context(&visibility).await;
    let identity = RemoteIdentity::new(&context, native("private-native-id")).unwrap();
    let debug = format!("{:?}", RemoteAccessTarget::Resource(identity));
    for private in ["test-tenant", "reader", "private-native-id"] {
        assert!(!debug.contains(private), "Debug disclosed {private}");
    }
}
