//! P7-11 with P3-C02/C03: GC never removes a current, pinned or guarded key;
//! expired cleanup runs as the GC role in the exact FK order; a failure after
//! the guard DELETE rolls the whole transaction back.

#[path = "support/bundle.rs"]
mod bundle;
#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use bundle::*;
use search_application::scoped::{
    AccessContextAuthorityPort, AccessRevision, CurrentSourceVisibilityPort, PrincipalRef,
    SyntheticAuthorityAdapter, SyntheticVisibilityAdapter, TenantId,
};
use search_application::search_core::id::{DiscoveryEvaluationId, SessionId};
use search_runtime::gc::{GcError, GcOutcome, PgGenerationGc, Protection};
use search_runtime::pin::{PgEvaluationPins, PinTtl};

async fn expire_guard(admin: &PgPool, key: ProjectionGenerationKey) {
    sqlx::query(
        "UPDATE search_generation_full_guard SET expires_at = clock_timestamp() \
         - interval '1 second' WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .execute(admin)
    .await
    .unwrap();
}

async fn count(admin: &PgPool, table: &str, key: ProjectionGenerationKey) -> i64 {
    let column = if table.ends_with("full_guard") || table.ends_with("issuance") {
        "target_generation_id"
    } else {
        "generation_id"
    };
    let statement = format!("SELECT count(*) FROM {table} WHERE source_id=$1 AND {column}=$2");
    sqlx::query_scalar(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_one(admin)
        .await
        .unwrap()
}

#[tokio::test]
async fn current_pinned_or_guarded_key_is_not_deleted() {
    let fixture = fixture().await;
    let gc = PgGenerationGc::new(fixture.admin.clone(), &fixture.root);
    let first = fixture.publish_current(8_101).await;
    assert_eq!(
        gc.retire_unpinned(first).await,
        Ok(GcOutcome::Protected(Protection::Current))
    );

    // Pin the first key, then move the pointer on.
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
        .bind_discovery(&actor, DiscoveryEvaluationId::from_uuid(Uuid::from_u128(1)))
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
    let second = fixture.publish_current(8_102).await;
    assert_eq!(
        gc.retire_unpinned(first).await,
        Ok(GcOutcome::Protected(Protection::Pinned))
    );

    // A live full guard protects both a BUILDING target and a READY target
    // that lost its publication CAS.
    let building = fixture.build(8_103, "document-platform").await;
    assert_eq!(
        gc.discard_unpublished(building.key).await,
        Ok(GcOutcome::Protected(Protection::Guarded))
    );
    let lost = fixture.build(8_104, "document-platform").await;
    fixture
        .coordinator()
        .ready_manual(&lost.handle)
        .await
        .unwrap();
    assert_eq!(
        gc.retire_unpinned(lost.key).await,
        Ok(GcOutcome::Protected(Protection::Guarded))
    );
    assert_eq!(
        gc.discard_unpublished(first).await,
        Ok(GcOutcome::Protected(Protection::State))
    );

    // Once the pin expires, coordinator cleanup removes it and the old key
    // retires; the current key stays.
    sqlx::query(
        "UPDATE search_evaluation_lease SET expires_at = clock_timestamp() - interval '1 second' \
         WHERE lease_id=$1",
    )
    .bind(pin.lease.lease_id())
    .execute(&fixture.admin)
    .await
    .unwrap();
    let report = gc.cleanup_expired(10).await.unwrap();
    assert_eq!((report.generations, report.leases), (0, 1));
    assert_eq!(gc.retire_unpinned(first).await, Ok(GcOutcome::Deleted));
    assert_eq!(gc.retire_unpinned(first).await, Ok(GcOutcome::Missing));
    assert_eq!(
        gc.retire_unpinned(second).await,
        Ok(GcOutcome::Protected(Protection::Current))
    );
    // The guard holder may abort its own lost target.
    assert_eq!(gc.abort_manual(&lost.handle).await, Ok(GcOutcome::Deleted));
    assert_eq!(
        count(&fixture.admin, "search_generation", lost.key).await,
        0
    );
    assert_eq!(count(&fixture.admin, "search_generation", second).await, 1);
}

#[tokio::test]
async fn expired_unpublished_cleanup_commits_without_23503_as_gc_role() {
    let fixture = fixture().await;
    let target = fixture.build(8_201, "document-platform").await;
    let final_dir =
        LexicalArtifactStore::new(&fixture.root, fixture.admin.clone()).final_dir(target.key);
    assert!(final_dir.is_dir());
    expire_guard(&fixture.admin, target.key).await;

    let gc_login = fixture.login("search_gc").await;
    let gc = PgGenerationGc::new(gc_login, &fixture.root);
    let report = gc.cleanup_expired(10).await.unwrap();
    assert_eq!(report.generations, 1);
    for table in [
        "search_generation",
        "search_generation_full_guard",
        "search_generation_payload",
        "search_lexical_artifact",
        "search_graph.generation",
        "search_graph.relation",
        "search_graph.participant",
        "search_graph.resource",
    ] {
        assert_eq!(count(&fixture.admin, table, target.key).await, 0, "{table}");
    }
    // Permanent identity and issuance stay, so the key never gets a new guard.
    assert_eq!(
        count(&fixture.admin, "search_generation_identity", target.key).await,
        1
    );
    assert_eq!(
        count(&fixture.admin, "search_full_guard_issuance", target.key).await,
        1
    );
    assert!(!final_dir.exists());
    let (resources, _) = graph_records("document-platform");
    assert!(
        fixture
            .registrar
            .register_manual_with_graph(
                &FullBuildRequest {
                    manifest: manifest(8_201),
                    expected_snapshot: SNAPSHOT.into(),
                },
                &canonical_mapping_digest(source_id(), SNAPSHOT, &resources).unwrap(),
                FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn guard_delete_then_child_failure_rolls_back_full_target() {
    let fixture = fixture().await;
    let target = fixture.build(8_301, "document-platform").await;
    expire_guard(&fixture.admin, target.key).await;
    // Test-only fault: the first Graph child DELETE fails, after the guard DELETE.
    sqlx::raw_sql(
        "CREATE FUNCTION test_gc_fault() RETURNS trigger LANGUAGE plpgsql AS \
         $fault$ BEGIN RAISE EXCEPTION 'injected child delete failure'; END $fault$; \
         CREATE TRIGGER test_gc_fault BEFORE DELETE ON search_graph.participant \
         FOR EACH ROW EXECUTE FUNCTION test_gc_fault();",
    )
    .execute(&fixture.admin)
    .await
    .unwrap();
    let gc = PgGenerationGc::new(fixture.admin.clone(), &fixture.root);
    assert_eq!(
        gc.discard_unpublished(target.key).await,
        Err(GcError::Store)
    );

    // Another connection still sees both the guard and the whole target.
    let state: String = sqlx::query_scalar(
        "SELECT state FROM search_generation WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(target.key.source_id.as_uuid())
    .bind(target.key.generation_id.as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(state, "BUILDING");
    assert_eq!(
        count(&fixture.admin, "search_generation_full_guard", target.key).await,
        1
    );
    assert_eq!(
        count(&fixture.admin, "search_graph.participant", target.key).await,
        2
    );
    assert!(
        LexicalArtifactStore::new(&fixture.root, fixture.admin.clone())
            .final_dir(target.key)
            .is_dir()
    );

    sqlx::raw_sql("DROP TRIGGER test_gc_fault ON search_graph.participant")
        .execute(&fixture.admin)
        .await
        .unwrap();
    assert_eq!(
        gc.discard_unpublished(target.key).await,
        Ok(GcOutcome::Deleted)
    );
    assert_eq!(
        count(&fixture.admin, "search_generation", target.key).await,
        0
    );
}
