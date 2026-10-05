//! P3-G07: pinned PostgreSQL Graph read. Same paths as the memory oracle,
//! hidden relations never change a public result, access `Unknown` fails the
//! whole read, and a pin that lapses before return discards every hit.

#[path = "support/bundle.rs"]
mod bundle;
#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use bundle::*;
use search_application::SearchError;
use search_application::graph_generation::GraphReadLease;
use search_application::ports::HyperGraphRetrieverPort;
use search_application::scoped::{
    AccessContextAuthorityPort, AccessRevision, CurrentSourceVisibilityPort, PrincipalRef,
    SyntheticAuthorityAdapter, SyntheticVisibilityAdapter, TenantId,
};
use search_application::search_core::id::{DiscoveryEvaluationId, SessionId};
use search_graph::PostgresGraphReader;
use search_runtime::pin::{PgEvaluationPins, PinTtl};

#[path = "support/graph.rs"]
mod graph_support;

use graph_support::*;

#[tokio::test]
async fn pg_nary_path_matches_memory_oracle_and_hidden_degree_matches_absent() {
    let fixture = fixture().await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let (binding, scope) = actor(&catalog, &authority).await;
    let base = graph_ready(&fixture, 8_001, false).await;
    let noisy = graph_ready(&fixture, 8_002, true).await;
    let two_hops = plan(&[101], "cites", ("source", "target"), 2);

    let access = Access {
        denied: denied(true),
        unknown: BTreeSet::new(),
    };
    let verifier = Verifier::new(usize::MAX);
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &access, &verifier);
    let lease = |key| GraphReadLease::from_identifiers(key, binding.evaluation(), Uuid::nil());
    let pg = reader
        .retrieve(&lease(base), &two_hops, &binding, &scope)
        .await
        .unwrap();
    let memory = oracle(
        base,
        false,
        Arc::new(Access {
            denied: denied(false),
            unknown: BTreeSet::new(),
        }),
    )
    .retrieve(base, &two_hops)
    .await
    .unwrap();
    assert_eq!(pg, memory);
    let targets: Vec<_> = pg
        .hits
        .iter()
        .map(|hit| hit.candidate.resource_ref)
        .collect();
    assert_eq!(targets, vec![Some(rid(104)), Some(rid(106))]);
    // The ternary relation 301 keeps its witness in the path evidence.
    assert_eq!(pg.hits[0].paths[0].steps[0].participants.len(), 3);

    // Six hidden branches of R1 consume no visible budget and leave no trace.
    let with_hidden = reader
        .retrieve(&lease(noisy), &two_hops, &binding, &scope)
        .await
        .unwrap();
    assert_eq!(public(&with_hidden), public(&pg));

    // A missing seed and a denied seed share the same empty response.
    let missing = reader
        .retrieve(
            &lease(base),
            &plan(&[999], "cites", ("source", "target"), 2),
            &binding,
            &scope,
        )
        .await
        .unwrap();
    let hidden_seed = reader
        .retrieve(
            &lease(base),
            &plan(&[201], "cites", ("source", "target"), 2),
            &binding,
            &scope,
        )
        .await
        .unwrap();
    assert!(missing.hits.is_empty());
    assert_eq!(public(&missing), public(&hidden_seed));
}

#[tokio::test]
async fn access_unknown_unsupported_stop_budget_and_lapsed_pin_return_no_hits() {
    let fixture = fixture().await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let (binding, scope) = actor(&catalog, &authority).await;
    let key = graph_ready(&fixture, 8_011, false).await;
    let lease = GraphReadLease::from_identifiers(key, binding.evaluation(), Uuid::nil());
    let two_hops = plan(&[101], "cites", ("source", "target"), 2);
    let live = Verifier::new(usize::MAX);

    // Unknown access for a reachable participant fails the whole read.
    let unknown = Access {
        denied: denied(false),
        unknown: BTreeSet::from([rid(106)]),
    };
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &unknown, &live);
    assert!(matches!(
        reader.retrieve(&lease, &two_hops, &binding, &scope).await,
        Err(SearchError::SourceUnavailable(_))
    ));

    let access = Access {
        denied: denied(false),
        unknown: BTreeSet::new(),
    };
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &access, &live);
    let mut stop = two_hops.clone();
    stop.stop_conditions = vec!["first-hit".into()];
    let mut narrow = two_hops.clone();
    narrow.expansion_budget.max_branching_per_node = 1;
    for refused in [stop, narrow] {
        assert!(matches!(
            reader.retrieve(&lease, &refused, &binding, &scope).await,
            Err(SearchError::InvalidRequest(_))
        ));
    }

    // The pin lapses between the snapshot and the before-return check.
    let lapsing = Verifier::new(1);
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &access, &lapsing);
    assert!(
        reader
            .retrieve(&lease, &two_hops, &binding, &scope)
            .await
            .is_err()
    );
    assert_eq!(lapsing.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn real_pin_reads_current_graph_until_the_pin_expires() {
    let fixture = fixture().await;
    let key = fixture.publish_current(8_021).await;
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    visibility
        .grant(
            TenantId::new("tenant-a").unwrap(),
            PrincipalRef::new("alice").unwrap(),
            source_id(),
            registration::revision(1),
            registration::visibility(1),
        )
        .unwrap();
    let handle = authority
        .issue_verified_identity(
            TenantId::new("tenant-a").unwrap(),
            PrincipalRef::new("alice").unwrap(),
            Some(SessionId::from_uuid(Uuid::now_v7())),
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(600),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let binding = authority
        .bind_discovery(&actor, DiscoveryEvaluationId::from_uuid(Uuid::from_u128(5)))
        .await
        .unwrap()
        .unwrap();
    let scope = visibility
        .bind_source(&actor, source_id())
        .await
        .unwrap()
        .unwrap();
    let pins = PgEvaluationPins::new(
        fixture.admin.clone(),
        &fixture.root,
        &authority,
        &visibility,
    );
    let pin = pins
        .pin_current(
            &binding,
            &scope,
            PinTtl::new(Duration::from_secs(60)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pin.lease.key(), key);

    let access = Access {
        denied: BTreeSet::new(),
        unknown: BTreeSet::new(),
    };
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &access, &pins);
    let mut placement = plan(
        &[10],
        "document_current_placement",
        ("document", "folder"),
        1,
    );
    placement.expansion_budget.max_hops = 1;
    let result = reader
        .retrieve(&pin.lease, &placement, &binding, &scope)
        .await
        .unwrap();
    assert_eq!(result.generation, key);
    assert_eq!(
        result
            .hits
            .iter()
            .map(|hit| hit.candidate.resource_ref)
            .collect::<Vec<_>>(),
        vec![Some(rid(11))]
    );

    sqlx::query(
        "UPDATE search_evaluation_lease SET expires_at = clock_timestamp() - interval '1 second' \
         WHERE lease_id=$1",
    )
    .bind(pin.lease.lease_id())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert!(matches!(
        reader
            .retrieve(&pin.lease, &placement, &binding, &scope)
            .await,
        Err(SearchError::SourceUnavailable(_))
    ));
}

#[tokio::test]
async fn missing_mapping_commitment_fails_recovery() {
    let fixture = fixture().await;
    let key = graph_ready(&fixture, 8_031, false).await;
    let store = PostgresGraphStore::new(fixture.admin.clone());
    let manifest_digest = manifest(8_031).digest;
    let receipt = store.recover(key, &manifest_digest).await.unwrap();
    assert_eq!(receipt.resource_count, 7);
    let stored = store.ready_resource(key, rid(101)).await.unwrap().unwrap();
    assert!(matches!(
        stored.mapping,
        GraphSourceMapping::Registered { .. }
    ));

    // Admin-only corruption below the triggers: one owner mapping changes
    // after READY, so the committed mapping digest no longer holds.
    let mut tx = fixture.admin.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE search_graph.resource SET native_id='borrowed' \
         WHERE source_id=$1 AND generation_id=$2 AND resource_id=$3",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(rid(101).as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        store.recover(key, &manifest_digest).await,
        Err(search_graph::GraphError::Integrity(_))
    ));
}
