//! P4-18: four remote modes end to end over real local TCP, through
//! ScopedDiscoveryService → DiscoveryService::discover_scoped → router,
//! planner and executor → sealed generation → federation, hard gates,
//! Claim/evidence and sufficiency. Synthetic runtime data only.

mod support;

use search_application::materialization::MaterializationBudget;
use search_application::remote::{
    PinnedRemoteTarget, PlannedRemoteAction, RemoteActionOutcome, RemoteActionResponse,
    RemoteOperation, RemoteReadOutcome, RemoteSourcePort, RemoteUnknownReason,
};
use search_application::remote_binding::RemoteBindingService;
use search_application::remote_identity::remote_resource_id;
use search_application::remote_observation::CheckedRemoteObservationAdapter;
use search_application::remote_registration::RemoteSourceRegistration;
use search_application::retrieval::{LiveInput, OpaqueNativeId, RemoteQueryInput};
use search_core::evidence::{ClaimState, EvidenceSufficiency};
use search_core::id::ResourceId;
use search_core::materialization::MaterializationState;
use search_core::observation::Presence;
use search_core::source::RetentionMode;
use support::discovery::*;
use support::synthetic_catalog::{Catalog, Collection, Doc, Fault};
use support::*;

const A: &str = "/t1/v1";
const B: &str = "/t1b/v1";
const C: &str = "/t2/v1";

fn native(value: &str) -> OpaqueNativeId {
    OpaqueNativeId::new(value).unwrap()
}

fn resource(registration: &RemoteSourceRegistration, id: &str) -> ResourceId {
    remote_resource_id(
        registration.tenant(),
        registration.source_id(),
        "synthetic",
        &native(id),
    )
    .unwrap()
}

/// One tenant-a Source at `A` serving `docs`.
async fn single(docs: Vec<Doc>) -> (Catalog, World) {
    let catalog = Catalog::start().await;
    let id = source(9_001);
    catalog.serve(
        A,
        Collection::new("tenant-a", &id.as_uuid().to_string(), docs),
    );
    let world = World::new(vec![registration(
        "tenant-a",
        id,
        catalog.port(),
        A,
        RetentionMode::NoRetention,
    )])
    .await;
    (catalog, world)
}

fn lists(catalog: &Catalog, base: &str) -> Vec<String> {
    catalog
        .log()
        .into_iter()
        .filter(|entry| {
            ["catalog", "search", "lookup", "live"]
                .iter()
                .any(|operation| entry.contains(&format!("{base}/{operation}")))
        })
        .collect()
}

fn completed(outcome: RemoteActionOutcome) -> RemoteActionResponse {
    match outcome {
        RemoteActionOutcome::Completed(response) => *response,
        RemoteActionOutcome::Unknown { reason, .. } => panic!("unknown: {reason:?}"),
    }
}

#[tokio::test]
async fn enumeration_qualifies_and_only_terminal_sweep_proves_absence() {
    let docs = (0..3)
        .map(|n| Doc::new(&format!("doc-{n}"), "規程", &["reader"]))
        .collect();
    let (catalog, world) = single(docs).await;
    catalog.update(A, |collection| {
        collection.page_size = 2;
        collection.extent = "complete".into();
        collection.known = ["doc-0", "doc-1", "doc-2", "doc-removed"]
            .map(str::to_owned)
            .to_vec();
    });
    let registration = world.registrations[0].clone();
    let visibility = world.visibility();
    let actor = world.actor("tenant-a", "reader", false, &visibility).await;
    let adapter = world.adapter(&registration, &visibility);
    let id = registration.source_id();

    let shape = evaluate(
        &world,
        &visibility,
        &[&adapter],
        &actor.binding,
        config(&[id], &[], &[(id, Mode::Enumerate)], 1),
    )
    .await;
    assert_eq!(shape.sufficiency, EvidenceSufficiency::Sufficient);
    assert_eq!(
        shape.qualified,
        vec![
            resource(&registration, "doc-0"),
            resource(&registration, "doc-1")
        ]
    );
    assert_eq!(lists(&catalog, A), vec![format!("GET {A}/catalog?cursor=")]);

    // Absence needs the whole terminal sweep of one complete snapshot.
    let binding = world.rebind(&actor).await;
    let context = world.context(&binding, id, &visibility).await;
    let action = PlannedRemoteAction::new(
        &context,
        "sweep",
        RemoteOperation::Enumerate { cursor: None },
    )
    .unwrap();
    let pages: Vec<_> = adapter
        .sweep(&context, &action)
        .await
        .unwrap()
        .into_iter()
        .map(completed)
        .collect();
    assert_eq!(pages.len(), 2);
    let checked =
        CheckedRemoteObservationAdapter::unwired(&registration, &world.authority, &visibility);
    let absent = |pages: &[RemoteActionResponse], id: &str| {
        let pages = pages.to_vec();
        let native = native(id);
        let checked = &checked;
        let context = &context;
        async move {
            checked
                .verify_absence(context, &pages, &native)
                .await
                .unwrap()
        }
    };
    assert!(absent(&pages, "doc-removed").await.is_some());
    assert!(absent(&pages[..1], "doc-removed").await.is_none());
    assert!(absent(&pages, "doc-1").await.is_none());
    assert!(absent(&pages, "never-known").await.is_none());
    // A partial snapshot never proves absence, however many pages it has.
    catalog.update(A, |collection| collection.extent = "partial".into());
    let partial: Vec<_> = adapter
        .sweep(&context, &action)
        .await
        .unwrap()
        .into_iter()
        .map(completed)
        .collect();
    assert!(absent(&partial, "doc-removed").await.is_none());
}

#[tokio::test]
async fn query_qualifies_but_query_miss_stays_unknown() {
    let (catalog, world) = single(vec![
        Doc::new("doc-1", "規程", &["reader"]),
        Doc::new("doc-2", "手順", &["reader"]),
    ])
    .await;
    let registration = world.registrations[0].clone();
    let visibility = world.visibility();
    let actor = world.actor("tenant-a", "reader", false, &visibility).await;
    let adapter = world.adapter(&registration, &visibility);
    let id = registration.source_id();

    let hit = evaluate(
        &world,
        &visibility,
        &[&adapter],
        &actor.binding,
        config(&[id], &[], &[(id, Mode::Query("規程"))], 1),
    )
    .await;
    assert_eq!(hit.qualified, vec![resource(&registration, "doc-1")]);
    assert_eq!(hit.sufficiency, EvidenceSufficiency::Sufficient);

    let binding = world.rebind(&actor).await;
    let miss = evaluate(
        &world,
        &visibility,
        &[&adapter],
        &binding,
        config(&[id], &[], &[(id, Mode::Query("存在しない"))], 1),
    )
    .await;
    assert!(miss.qualified.is_empty());
    assert_ne!(miss.sufficiency, EvidenceSufficiency::Sufficient);
    assert!(
        miss.claims
            .iter()
            .all(|(_, state)| *state != ClaimState::Absent)
    );
    // The miss is a completed query whose presence stays Unknown.
    let binding = world.rebind(&actor).await;
    let context = world.context(&binding, id, &visibility).await;
    let outcome = adapter
        .execute_batch(
            &context,
            &[PlannedRemoteAction::new(
                &context,
                "search",
                RemoteOperation::Query {
                    input: RemoteQueryInput::new("存在しない", vec![], 10, &[]).unwrap(),
                },
            )
            .unwrap()],
        )
        .await
        .unwrap();
    assert_eq!(outcome[0].presence(&native("doc-1")), Presence::Unknown);
    assert_eq!(lists(&catalog, A).len(), 3);
}

#[tokio::test]
async fn direct_lookup_qualifies_but_unmasked_absence_needs_grant() {
    let (_catalog, world) = single(vec![Doc::new("doc-1", "規程", &["reader"])]).await;
    let registration = world.registrations[0].clone();
    let visibility = world.visibility();
    let actor = world.actor("tenant-a", "reader", false, &visibility).await;
    let adapter = world.adapter(&registration, &visibility);
    let id = registration.source_id();

    let found = evaluate(
        &world,
        &visibility,
        &[&adapter],
        &actor.binding,
        config(&[id], &[], &[(id, Mode::Lookup("doc-1"))], 1),
    )
    .await;
    assert_eq!(found.qualified, vec![resource(&registration, "doc-1")]);
    assert_eq!(found.sufficiency, EvidenceSufficiency::Sufficient);

    let binding = world.rebind(&actor).await;
    let missing = evaluate(
        &world,
        &visibility,
        &[&adapter],
        &binding,
        config(&[id], &[], &[(id, Mode::Lookup("doc-404"))], 1),
    )
    .await;
    assert!(missing.qualified.is_empty());
    assert!(
        missing
            .claims
            .iter()
            .all(|(_, state)| *state != ClaimState::Absent)
    );
    // No registered unmasked-absence grant: a lookup miss proves nothing.
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
                        native_id: native("doc-404"),
                    },
                )
                .unwrap()],
            )
            .await
            .unwrap()
            .remove(0),
    );
    assert!(response.hits().is_empty());
    let checked =
        CheckedRemoteObservationAdapter::unwired(&registration, &world.authority, &visibility);
    assert!(
        checked
            .verify_absence(&context, &[response], &native("doc-404"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn live_qualifies_and_changed_content_requires_new_evaluation() {
    let (catalog, world) = single(vec![Doc::new("doc-1", "規程", &["reader"])]).await;
    let registration = world.registrations[0].clone();
    let visibility = world.visibility();
    let actor = world.actor("tenant-a", "reader", false, &visibility).await;
    let adapter = world.adapter(&registration, &visibility);
    let id = registration.source_id();
    let live = config(&[id], &[], &[(id, Mode::Live("doc-1"))], 1);

    let first = evaluate(
        &world,
        &visibility,
        &[&adapter],
        &actor.binding,
        live.clone(),
    )
    .await;
    assert_eq!(first.qualified, vec![resource(&registration, "doc-1")]);
    assert_eq!(first.sufficiency, EvidenceSufficiency::Sufficient);

    // A live pin taken now no longer matches once the content changes.
    let binding = world.rebind(&actor).await;
    let context = world.context(&binding, id, &visibility).await;
    let response = completed(
        adapter
            .execute_batch(
                &context,
                &[PlannedRemoteAction::new(
                    &context,
                    "live",
                    RemoteOperation::Live {
                        input: LiveInput::lookup(native("doc-1")),
                    },
                )
                .unwrap()],
            )
            .await
            .unwrap()
            .remove(0),
    );
    let target = PinnedRemoteTarget::from_response(&context, &response, &native("doc-1")).unwrap();
    catalog.update(A, |collection| {
        collection.docs[0].version = Some("v2".into());
        collection.docs[0].digest = Some("d-doc-1-2".into());
    });
    let budget = MaterializationBudget {
        max_content_bytes: 64 * 1024,
        max_latency_ms: 2_000,
        max_remote_calls: 1,
        max_monetary_cost_minor_units: 0,
        currency: "USD".into(),
        direct_full_max_bytes: 64 * 1024,
    };
    assert_eq!(
        RemoteBindingService::new(&adapter, &world.authority, &visibility)
            .revalidate_target(&context, &target, MaterializationState::Metadata, &budget)
            .await
            .unwrap(),
        RemoteReadOutcome::Unknown(RemoteUnknownReason::SnapshotIncompatible)
    );
    // A new evaluation qualifies the changed Resource again.
    let binding = world.rebind(&actor).await;
    let second = evaluate(&world, &visibility, &[&adapter], &binding, live).await;
    assert_eq!(second.qualified, first.qualified);
    assert_eq!(second.sufficiency, EvidenceSufficiency::Sufficient);
}

#[tokio::test]
async fn two_actions_share_one_snapshot_and_key() {
    let catalog = Catalog::start().await;
    let (a, b, c) = (source(9_101), source(9_102), source(9_201));
    catalog.serve(
        A,
        Collection::new(
            "tenant-a",
            &a.as_uuid().to_string(),
            vec![
                Doc::new("doc-1", "規程", &["reader"]),
                Doc::new("doc-2", "規程", &["reader"]),
            ],
        ),
    );
    catalog.serve(
        B,
        Collection::new(
            "tenant-a",
            &b.as_uuid().to_string(),
            vec![Doc::new("doc-1", "規程", &["reader"])],
        ),
    );
    catalog.serve(
        C,
        Collection::new(
            "tenant-b",
            &c.as_uuid().to_string(),
            vec![Doc::new("doc-1", "規程", &["reader"])],
        ),
    );
    let port = catalog.port();
    let world = World::new(vec![
        registration("tenant-a", a, port, A, RetentionMode::NoRetention),
        registration("tenant-a", b, port, B, RetentionMode::NoRetention),
        registration("tenant-b", c, port, C, RetentionMode::NoRetention),
    ])
    .await;
    let visibility = world.visibility();
    let actor = world.actor("tenant-a", "reader", false, &visibility).await;
    let source_a = world.adapter(&world.registrations[0], &visibility);
    let source_b = world.adapter(&world.registrations[1], &visibility);
    // The preferred Source is down for this whole evaluation.
    for operation in ["search", "lookup"] {
        catalog.fault(B, operation, Fault::Status(503));
    }
    let modes = [
        (a, Mode::Query("規程")),
        (a, Mode::Lookup("doc-1")),
        (b, Mode::Query("規程")),
        (b, Mode::Lookup("doc-1")),
    ];
    let shape = evaluate(
        &world,
        &visibility,
        &[&source_a, &source_b],
        &actor.binding,
        config(&[a], &[b], &modes, 2),
    )
    .await;
    // One batch, one snapshot, one key: the shared Resource is merged once
    // and the S1 order follows the plan, not the provider.
    assert_eq!(
        lists(&catalog, A),
        vec![format!("POST {A}/search"), format!("POST {A}/lookup")]
    );
    assert_eq!(
        shape.qualified,
        vec![
            resource(&world.registrations[0], "doc-1"),
            resource(&world.registrations[0], "doc-2"),
        ]
    );
    assert_eq!(shape.sufficiency, EvidenceSufficiency::Sufficient);
    assert!(shape.gaps.iter().all(|(_, _, blocking)| !blocking));
    assert!(shape.has_gap(&format!("source:{}:", b.as_uuid())));

    // Another tenant's Source with the same native ID is a different
    // Resource, and the first tenant never sees it.
    assert!(
        !shape
            .trace
            .iter()
            .any(|entry| entry.contains(&c.as_uuid().to_string()))
    );
    let other = world.actor("tenant-b", "reader", false, &visibility).await;
    let source_c = world.adapter(&world.registrations[2], &visibility);
    let theirs = evaluate(
        &world,
        &visibility,
        &[&source_c],
        &other.binding,
        config(&[c], &[], &[(c, Mode::Lookup("doc-1"))], 1),
    )
    .await;
    assert_eq!(
        theirs.qualified,
        vec![resource(&world.registrations[2], "doc-1")]
    );
    assert_ne!(theirs.qualified[0], shape.qualified[0]);
}
