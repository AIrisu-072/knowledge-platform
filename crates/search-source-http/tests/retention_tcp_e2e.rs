//! P4-19: revocation and retention canary over real local TCP. Every body,
//! token and ACL is runtime-generated; nothing is persisted by the test.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use search_application::ports::CurrentSourcePolicy;
use search_application::projection::{ProjectionError, RemoteFieldKind, RemoteFieldProofs};
use search_application::remote::{
    EvaluationLeaseId, PinnedRemoteTarget, PlannedRemoteAction, RemoteActionOutcome,
    RemoteActionResponse, RemoteOperation, RemoteSourcePort,
};
use search_application::remote_binding::RemoteBindingService;
use search_application::remote_cache::RemoteResultCache;
use search_application::remote_generation::RemoteGenerationBuilder;
use search_application::remote_identity::remote_resource_id;
use search_application::remote_lease::{
    LeaseClock, LeaseState, RemoteLease, RemoteOwner, ScopedOwnerGate,
};
use search_application::remote_session::ScopedSessionWorkingSet;
use search_application::retrieval::{OpaqueNativeId, RemoteQueryInput};
use search_application::session::BoundResourceKey;
use search_core::binding::BindingMode;
use search_core::evidence::EvidenceSufficiency;
use search_core::id::ResourceId;
use search_core::materialization::ProviderContentPermission;
use search_core::resource::ResourceKind;
use search_core::source::RetentionMode;
use search_source_http::protocol::decode_response;
use search_source_http::transport::{GuardedHttpTransport, RegisteredPath, TransportLimits};
use support::discovery::*;
use support::synthetic_catalog::{Catalog, Collection, Doc, Fault};
use support::*;

const A: &str = "/t1/v1";

fn native(value: &str) -> OpaqueNativeId {
    OpaqueNativeId::new(value).unwrap()
}

async fn single(docs: Vec<Doc>, retention: RetentionMode) -> (Catalog, World) {
    let catalog = Catalog::start().await;
    let id = source(10_001);
    catalog.serve(
        A,
        Collection::new("tenant-a", &id.as_uuid().to_string(), docs),
    );
    let world = World::new(vec![registration(
        "tenant-a",
        id,
        catalog.port(),
        A,
        retention,
    )])
    .await;
    (catalog, world)
}

fn resource(world: &World, id: &str) -> ResourceId {
    let registration = &world.registrations[0];
    remote_resource_id(
        registration.tenant(),
        registration.source_id(),
        "synthetic",
        &native(id),
    )
    .unwrap()
}

fn completed(outcome: RemoteActionOutcome) -> RemoteActionResponse {
    match outcome {
        RemoteActionOutcome::Completed(response) => *response,
        RemoteActionOutcome::Unknown { reason, .. } => panic!("unknown: {reason:?}"),
    }
}

fn query() -> RemoteOperation {
    RemoteOperation::Query {
        input: RemoteQueryInput::new("規程", vec![], 10, &[]).unwrap(),
    }
}

#[tokio::test]
async fn revoked_acl_during_response_leaks_no_source_fields() {
    let (catalog, world) = single(
        vec![
            Doc::new("doc-1", "規程", &["reader"]),
            Doc::new("doc-2", "規程", &["reader"]),
        ],
        RetentionMode::NoRetention,
    )
    .await;
    // The item ACL changes while doc-1's evidence is being read.
    catalog.update(A, |collection| {
        collection.revoke_on_content = Some(("doc-1".into(), "reader".into()));
    });
    let visibility = world.visibility();
    let actor = world.actor("tenant-a", "reader", false, &visibility).await;
    let adapter = world.adapter(&world.registrations[0], &visibility);
    let id = world.registrations[0].source_id();
    let shape = evaluate(
        &world,
        &visibility,
        &[&adapter],
        &actor.binding,
        config(&[id], &[], &[(id, Mode::Query("規程"))], 1),
    )
    .await;
    assert_eq!(shape.qualified, vec![resource(&world, "doc-2")]);
    assert_eq!(shape.sufficiency, EvidenceSufficiency::Sufficient);
    let revoked = resource(&world, "doc-1").as_uuid().to_string();
    assert!(!shape.trace.iter().any(|entry| entry.contains(&revoked)));
    assert!(
        !shape
            .gaps
            .iter()
            .any(|(fact, _, _)| fact.contains(&revoked))
    );
}

#[tokio::test]
async fn all_five_modes_store_only_allowed_projection() {
    for mode in [
        RetentionMode::NoRetention,
        RetentionMode::SessionOnly,
        RetentionMode::CacheWithExpiry,
        RetentionMode::PersistentDiscoveryMetadata,
        RetentionMode::PersistentResource,
    ] {
        let mut with_body = Doc::new("doc-2", "規程", &["reader"]);
        with_body
            .fields
            .push(("body".into(), "transient body".into(), None));
        let (_catalog, world) = single(
            vec![Doc::new("doc-1", "規程", &["reader"]), with_body],
            mode,
        )
        .await;
        let registration = world.registrations[0].clone();
        let visibility = world.visibility();
        let actor = world.actor("tenant-a", "reader", true, &visibility).await;
        let adapter = world.adapter(&registration, &visibility);
        let id = registration.source_id();

        // Every mode: the evaluation lease is closed when Discovery returns.
        let disclosure = discover_scoped(
            &world,
            &visibility,
            &[&adapter],
            &actor.binding,
            config(&[id], &[], &[(id, Mode::Query("規程"))], 1),
        )
        .await
        .unwrap();
        assert!(disclosure.evaluation_closed(), "{mode:?}");

        // Only the registration's retention, the current policy and the
        // server's field proofs decide what may become durable.
        let binding = world.rebind(&actor).await;
        let context = world.context(&binding, id, &visibility).await;
        let response = completed(
            adapter
                .execute_batch(
                    &context,
                    &[PlannedRemoteAction::new(&context, "search", query()).unwrap()],
                )
                .await
                .unwrap()
                .remove(0),
        );
        let mut builder = RemoteGenerationBuilder::new(
            context.clone(),
            binding.evaluation(),
            EvaluationLeaseId::new(),
        )
        .unwrap();
        builder.stage(response).unwrap();
        let generation = builder.seal().unwrap();
        let policy = CurrentSourcePolicy {
            resource_kind: ResourceKind::Knowledge,
            provider_permission: ProviderContentPermission::Metadata,
            retention_mode: mode,
            probe_allowed: false,
        };
        let proofs =
            RemoteFieldProofs::new(vec![("catalog.title".into(), RemoteFieldKind::Text)]).unwrap();
        let metadata =
            generation.persistable_projection(resource(&world, "doc-1"), &policy, &proofs);
        let body = generation.persistable_projection(resource(&world, "doc-2"), &policy, &proofs);
        match mode {
            RetentionMode::PersistentDiscoveryMetadata | RetentionMode::PersistentResource => {
                assert!(metadata.is_ok(), "{mode:?}");
                assert!(
                    matches!(body, Err(ProjectionError::FieldNotPermitted { .. })),
                    "{mode:?}"
                );
            }
            _ => {
                assert_eq!(
                    metadata,
                    Err(ProjectionError::PersistenceDenied),
                    "{mode:?}"
                );
                assert_eq!(body, Err(ProjectionError::PersistenceDenied), "{mode:?}");
            }
        }
    }
}

#[tokio::test]
async fn no_retention_success_error_cancel_disconnect_deadline_leave_no_derived_bytes() {
    let (catalog, world) = single(
        vec![Doc::new("doc-1", "規程", &["reader"])],
        RetentionMode::NoRetention,
    )
    .await;
    let visibility = world.visibility();
    let actor = world.actor("tenant-a", "reader", false, &visibility).await;
    let adapter = world.adapter(&world.registrations[0], &visibility);
    let id = world.registrations[0].source_id();
    let query = config(&[id], &[], &[(id, Mode::Query("規程"))], 1);

    // Success: both leases close; the handle is consumed by one read.
    let mut success = discover_scoped(
        &world,
        &visibility,
        &[&adapter],
        &actor.binding,
        query.clone(),
    )
    .await
    .unwrap();
    assert!(success.evaluation_closed());
    assert_eq!(
        disclose(&world, &visibility, &mut success)
            .await
            .unwrap()
            .sufficiency,
        EvidenceSufficiency::Sufficient
    );
    assert_eq!(success.state(), LeaseState::Closed);

    // Provider error, disconnect and deadline: generic gaps, closed leases.
    for fault in [
        Fault::Status(500),
        Fault::Disconnect,
        Fault::Delay(Duration::from_secs(3)),
    ] {
        catalog.fault(A, "search", fault);
        let binding = world.rebind(&actor).await;
        let mut failed = discover_scoped(&world, &visibility, &[&adapter], &binding, query.clone())
            .await
            .unwrap();
        assert!(failed.evaluation_closed());
        let shape = disclose(&world, &visibility, &mut failed).await.unwrap();
        assert!(shape.qualified.is_empty());
        assert_ne!(shape.sufficiency, EvidenceSufficiency::Sufficient);
        assert_eq!(failed.state(), LeaseState::Closed);
    }

    // Cancelled mid-response: the whole evaluation is dropped with its
    // leases; the next evaluation reads the provider afresh.
    catalog.fault(A, "search", Fault::Delay(Duration::from_secs(1)));
    let binding = world.rebind(&actor).await;
    let cancelled = tokio::time::timeout(
        Duration::from_millis(200),
        discover_scoped(&world, &visibility, &[&adapter], &binding, query.clone()),
    )
    .await;
    assert!(cancelled.is_err());
    catalog.clear_faults();
    let before = catalog.requests();
    let binding = world.rebind(&actor).await;
    let mut fresh = discover_scoped(&world, &visibility, &[&adapter], &binding, query)
        .await
        .unwrap();
    assert!(catalog.requests() > before);
    assert_eq!(
        disclose(&world, &visibility, &mut fresh)
            .await
            .unwrap()
            .sufficiency,
        EvidenceSufficiency::Sufficient
    );
}

#[tokio::test]
async fn held_handle_and_response_buffer_fail_after_close() {
    let (_catalog, world) = single(
        vec![Doc::new("doc-1", "規程", &["reader"])],
        RetentionMode::SessionOnly,
    )
    .await;
    let registration = world.registrations[0].clone();
    let visibility = world.visibility();
    let actor = world.actor("tenant-a", "reader", true, &visibility).await;
    let adapter = world.adapter(&registration, &visibility);
    let id = registration.source_id();

    // A disclosure handle reads once.
    let mut disclosure = discover_scoped(
        &world,
        &visibility,
        &[&adapter],
        &actor.binding,
        config(&[id], &[], &[(id, Mode::Lookup("doc-1"))], 1),
    )
    .await
    .unwrap();
    assert!(disclose(&world, &visibility, &mut disclosure).await.is_ok());
    assert!(
        disclose(&world, &visibility, &mut disclosure)
            .await
            .is_err()
    );

    // A session-owned binding of a remote Resource fails after close.
    let binding = world.rebind(&actor).await;
    let context = world.context(&binding, id, &visibility).await;
    let response = completed(
        adapter
            .execute_batch(
                &context,
                &[PlannedRemoteAction::new(
                    &context,
                    "lookup",
                    RemoteOperation::Lookup {
                        native_id: native("doc-1"),
                    },
                )
                .unwrap()],
            )
            .await
            .unwrap()
            .remove(0),
    );
    let target = PinnedRemoteTarget::from_response(&context, &response, &native("doc-1")).unwrap();
    let mut builder = RemoteGenerationBuilder::new(
        context.clone(),
        binding.evaluation(),
        EvaluationLeaseId::new(),
    )
    .unwrap();
    builder.stage(response).unwrap();
    let generation = builder.seal().unwrap();
    let candidate = generation.candidates("lookup").unwrap()[0].clone();
    let representation = RemoteBindingService::new(&adapter, &world.authority, &visibility)
        .bind(
            &context,
            &generation,
            &candidate,
            &target,
            BindingMode::SnapshotPinned,
        )
        .unwrap();
    let clock = ManualClock::new();
    let mut session = ScopedSessionWorkingSet::open(
        &context,
        RemoteLease {
            absolute_deadline: clock.now() + Duration::from_secs(60),
            idle_timeout: Some(Duration::from_secs(30)),
            provider_expiry: None,
        },
        clock.clone(),
    )
    .unwrap();
    let owner = RemoteOwner::for_session(&context).unwrap();
    let gate = ScopedOwnerGate::new(&world.authority, &visibility);
    let resource = candidate.resource_ref.unwrap();
    session
        .bind(
            &owner,
            &gate,
            generation.key(),
            resource,
            &candidate.candidate_id,
            representation.clone(),
        )
        .await
        .unwrap();
    let key = BoundResourceKey::new(generation.key().source_id, resource);
    assert_eq!(
        session.read(&owner, &gate, key).await.unwrap(),
        Some(representation)
    );
    session.close();
    assert!(session.read(&owner, &gate, key).await.is_err());
    assert_eq!(session.state(), LeaseState::Closed);
}

#[tokio::test]
async fn cache_expiry_and_session_close_force_fresh_provider_read() {
    let (catalog, world) = single(
        vec![Doc::new("doc-1", "規程", &["reader"])],
        RetentionMode::CacheWithExpiry,
    )
    .await;
    let registration = world.registrations[0].clone();
    let visibility = world.visibility();
    let actor = world.actor("tenant-a", "reader", false, &visibility).await;
    let context = world
        .context(&actor.binding, registration.source_id(), &visibility)
        .await;
    let transport = GuardedHttpTransport::new_loopback_for_test(
        registration.endpoint().clone(),
        Arc::new(Loopback(catalog.port())),
        TransportLimits::from_registration(registration.limits()),
    )
    .unwrap();
    let read = || async {
        let body = r#"{"query":"規程","limit":10}"#.as_bytes();
        let bytes = transport
            .request(
                &RegisteredPath::search(),
                body,
                Instant::now() + Duration::from_secs(2),
            )
            .await
            .unwrap();
        decode_response(
            search_application::remote::RemoteOperationKind::Query,
            bytes.as_bytes(),
            &registration,
        )
        .unwrap()
        .input
    };
    let clock = ManualClock::new();
    let mut cache = RemoteResultCache::new(Duration::from_secs(30), 8, clock.clone());
    let operation = query();
    cache
        .put(
            &context,
            &operation,
            read().await,
            clock.now() + Duration::from_secs(10),
        )
        .unwrap();
    let requests = catalog.requests();
    // Within expiry: the cached raw input, no provider read.
    assert!(
        cache
            .get(&context, &operation, clock.now())
            .unwrap()
            .is_some()
    );
    assert_eq!(catalog.requests(), requests);
    // Past expiry: nothing cached, the next observation is a fresh read.
    clock.advance(Duration::from_secs(11));
    assert!(
        cache
            .get(&context, &operation, clock.now())
            .unwrap()
            .is_none()
    );
    let _fresh = read().await;
    assert_eq!(catalog.requests(), requests + 1);
    // Owner invalidation clears this actor's entries.
    cache
        .put(
            &context,
            &operation,
            read().await,
            clock.now() + Duration::from_secs(10),
        )
        .unwrap();
    cache.invalidate_owner(&RemoteOwner::for_evaluation(&context));
    assert!(cache.is_empty());

    // A closed SESSION_ONLY working set is never reopened; a new evaluation
    // reads the provider again.
    let (session_catalog, session_world) = single(
        vec![Doc::new("doc-1", "規程", &["reader"])],
        RetentionMode::SessionOnly,
    )
    .await;
    let session_visibility = session_world.visibility();
    let session_actor = session_world
        .actor("tenant-a", "reader", true, &session_visibility)
        .await;
    let session_context = session_world
        .context(
            &session_actor.binding,
            session_world.registrations[0].source_id(),
            &session_visibility,
        )
        .await;
    let mut session = ScopedSessionWorkingSet::open(
        &session_context,
        RemoteLease {
            absolute_deadline: clock.now() + Duration::from_secs(60),
            idle_timeout: Some(Duration::from_secs(30)),
            provider_expiry: None,
        },
        clock.clone(),
    )
    .unwrap();
    session.close();
    assert_eq!(session.state(), LeaseState::Closed);
    let adapter = session_world.adapter(&session_world.registrations[0], &session_visibility);
    let before = session_catalog.requests();
    let id = session_world.registrations[0].source_id();
    let binding = session_world.rebind(&session_actor).await;
    let shape = evaluate(
        &session_world,
        &session_visibility,
        &[&adapter],
        &binding,
        config(&[id], &[], &[(id, Mode::Query("規程"))], 1),
    )
    .await;
    assert!(session_catalog.requests() > before);
    assert_eq!(shape.sufficiency, EvidenceSufficiency::Sufficient);
}
