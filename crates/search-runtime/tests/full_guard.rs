use std::time::Duration;

use search_runtime::full_guard::FullGuardTtl;

#[test]
fn pure_guard_ttl_accepts_exact_microseconds_through_the_bound() {
    for ttl in [
        Duration::from_micros(1),
        Duration::from_secs(30),
        Duration::from_secs(120),
    ] {
        assert!(
            FullGuardTtl::new(ttl).is_some(),
            "valid bounded TTL: {ttl:?}"
        );
    }
}

#[test]
fn pure_guard_ttl_rejects_zero_rounding_and_unbounded_values() {
    for ttl in [
        Duration::ZERO,
        Duration::from_nanos(1),
        Duration::from_nanos(1001),
        Duration::from_secs(121),
        Duration::MAX,
    ] {
        assert!(FullGuardTtl::new(ttl).is_none(), "invalid TTL: {ttl:?}");
    }
}

#[path = "support/registration.rs"]
mod registration;
mod support;

use search_application::indexing_service::DocumentSourceEvent;
use search_application::ports::{SearchDeliveryFence, SourceFence};
use search_application::remote_registration::RemoteSourceRegistration;
use search_application::search_core::id::ProjectionGenerationId;
use search_application::search_core::observation::Coverage;
use search_application::search_core::projection::ProjectionGenerationManifest;
use search_application::search_core::source::RetentionMode;
use search_application::source_registration::{
    RegistrationActivation, RegistrationNamespace, SourceRegistration,
    SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_runtime::generation_registration::{
    FullBuildRequest, GenerationError, PgGenerationRegistrar,
};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

fn ttl() -> FullGuardTtl {
    FullGuardTtl::new(Duration::from_secs(30)).unwrap()
}

fn request(number: u128) -> FullBuildRequest {
    FullBuildRequest {
        expected_snapshot: "synthetic-snapshot-v1".into(),
        manifest: ProjectionGenerationManifest {
            source_id: registration::source(7101),
            generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(number)),
            projection_schema_version: "projection-v1".into(),
            lens_version: 1,
            semantic_registry_version: "registry-v1".into(),
            analyzer_version: None,
            embedding_model_version: None,
            graph_schema_version: None,
            source_snapshot: "synthetic-snapshot-v1".into(),
            resource_count: 0,
            relation_count: None,
            coverage: Coverage::CompleteEnumeration,
            digest: format!("sha256:{}", "a".repeat(64)),
            built_at: OffsetDateTime::UNIX_EPOCH,
        },
    }
}

async fn fixture() -> (
    support::postgres::DatabaseGuard,
    PgPool,
    PgGenerationRegistrar,
    PgSourceRegistrationLedger,
) {
    let (guard, pool, _) = support::postgres::postgres("full_guard_test").await;
    document_repository_postgres::migrate(&pool).await.unwrap();
    search_runtime::migrate(&pool).await.unwrap();
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    let ledger = PgSourceRegistrationLedger::new(pool.clone(), host.clone());
    let empty_remote = registration::publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&empty_remote).await.unwrap();
    let registration = registration::document(registration::source(7101), "tenant-a").await;
    let desired = registration::publish(
        &host,
        RegistrationNamespace::Document,
        1,
        vec![registration.clone()],
    )
    .await;
    let activations = ledger.reconcile(&desired).await.unwrap();
    let registrar = ledger
        .generation_registrar(registration, activations[&registration::source(7101)])
        .unwrap();
    (guard, pool, registrar, ledger)
}

async fn event(pool: &PgPool) -> (DocumentSourceEvent, SearchDeliveryFence) {
    let event = DocumentSourceEvent {
        event_id: Uuid::from_u128(7201),
        event_type: "DocumentVersionPublished".into(),
        aggregate_id: Uuid::from_u128(7202),
        occurred_at: OffsetDateTime::UNIX_EPOCH,
    };
    let fence = SearchDeliveryFence {
        event_id: event.event_id,
        outbox_token: Uuid::from_u128(7203),
        source: SourceFence {
            source_id: registration::source(7101),
            owner_token: Uuid::from_u128(7204),
            epoch: 1,
        },
    };
    sqlx::query("INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at,lease_token,lease_owner,lease_expires_at) VALUES ($1,$2,'Document',$3,'{}',$4,clock_timestamp(),$5,$6,clock_timestamp()+interval '1 hour')")
        .bind(event.event_id).bind(&event.event_type).bind(event.aggregate_id).bind(event.occurred_at)
        .bind(fence.outbox_token).bind(Uuid::from_u128(7205)).execute(pool).await.unwrap();
    sqlx::query("UPDATE search_source_coordination SET owner_token=$2,fence_epoch=$3,lease_expires_at=clock_timestamp()+interval '1 hour' WHERE source_id=$1")
        .bind(fence.source.source_id.as_uuid()).bind(fence.source.owner_token).bind(fence.source.epoch)
        .execute(pool).await.unwrap();
    (event, fence)
}

async fn counts(pool: &PgPool) -> (i64, i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM search_generation_identity), (SELECT count(*) FROM search_generation), (SELECT count(*) FROM search_generation_full_guard), (SELECT count(*) FROM search_full_guard_issuance), build_fence_seq FROM search_source_coordination WHERE source_id=$1")
        .bind(registration::source(7101).as_uuid()).fetch_one(pool).await.unwrap()
}

fn assert_db_code(error: sqlx::Error, expected: &str) {
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some(expected)
    );
}

#[tokio::test]
async fn manual_and_event_register_one_bound_building_target_per_transaction() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    let manual = registrar
        .register_manual(&request(7301), ttl())
        .await
        .unwrap();
    let (event, fence) = event(&pool).await;
    let event_handle = registrar
        .register_event(&event, fence, &request(7302), ttl())
        .await
        .unwrap();
    assert_eq!(manual.key(), request(7301).manifest.key());
    assert_eq!(event_handle.key(), request(7302).manifest.key());
    assert_eq!(counts(&pool).await, (2, 2, 2, 2, 2));
    let rows = sqlx::query("SELECT g.*, f.expires_at > clock_timestamp() AS live, f.guard_token=g.full_guard_token AND f.build_fence=g.full_build_fence AS exact FROM search_generation g JOIN search_generation_full_guard f ON g.source_id=f.source_id AND g.generation_id=f.target_generation_id ORDER BY full_build_fence")
        .fetch_all(&pool).await.unwrap();
    for row in &rows {
        assert_eq!(row.get::<String, _>("state"), "BUILDING");
        assert!(row.get::<bool, _>("live"));
        assert!(row.get::<bool, _>("exact"));
        assert_eq!(
            row.get::<String, _>("source_snapshot"),
            "synthetic-snapshot-v1"
        );
    }
    assert_eq!(rows[0].get::<String, _>("stage_origin"), "MANUAL");
    assert_eq!(rows[0].get::<Option<Uuid>, _>("stage_event_id"), None);
    assert_eq!(rows[1].get::<String, _>("stage_origin"), "EVENT");
    assert_eq!(
        rows[1].get::<Option<Uuid>, _>("stage_event_id"),
        Some(event.event_id)
    );
    assert_eq!(
        rows[1].get::<Option<i64>, _>("stage_source_epoch"),
        Some(fence.source.epoch)
    );
    let pointer: Option<Uuid> =
        sqlx::query_scalar("SELECT current_generation_id FROM search_source_coordination")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pointer, None);
    let delivered: bool = sqlx::query_scalar("SELECT delivered_at IS NOT NULL FROM outbox_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!delivered);
    registrar.renew_manual(&manual, ttl()).await.unwrap();
    registrar.renew_event(&event_handle, ttl()).await.unwrap();
    assert_eq!(counts(&pool).await, (2, 2, 2, 2, 2));
}

#[tokio::test]
async fn wrong_event_route_identity_and_lease_refuse_without_residue() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    let (original, fence) = event(&pool).await;
    for case in 0..9 {
        let mut event = original.clone();
        let mut bad = fence;
        match case {
            0 => event.event_id = Uuid::from_u128(7999),
            1 => event.event_type = "DocumentCreated".into(),
            2 => event.aggregate_id = Uuid::from_u128(7999),
            3 => event.occurred_at += time::Duration::seconds(1),
            4 => bad.outbox_token = Uuid::from_u128(7999),
            5 => bad.source.owner_token = Uuid::from_u128(7999),
            6 => bad.source.epoch += 1,
            7 => bad.source.source_id = registration::source(7999),
            8 => {
                sqlx::query("UPDATE outbox_events SET aggregate_type='Folder'")
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            registrar
                .register_event(&event, bad, &request(7400 + case), ttl())
                .await
                .is_err(),
            "case {case}"
        );
        assert_eq!(counts(&pool).await, (0, 0, 0, 0, 0));
    }
}

#[tokio::test]
async fn expired_outbox_or_source_lease_refuses_registration() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    let (event, fence) = event(&pool).await;
    sqlx::query("UPDATE outbox_events SET lease_expires_at=clock_timestamp()")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        registrar
            .register_event(&event, fence, &request(7501), ttl())
            .await,
        Err(GenerationError::Lost)
    ));
    sqlx::query("UPDATE outbox_events SET lease_expires_at=clock_timestamp()+interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_coordination SET lease_expires_at=clock_timestamp()")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        registrar
            .register_event(&event, fence, &request(7502), ttl())
            .await,
        Err(GenerationError::Lost)
    ));
    assert_eq!(counts(&pool).await, (0, 0, 0, 0, 0));
}

#[tokio::test]
async fn registration_and_activation_changes_refuse_old_registrar_and_guard() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    let handle = registrar
        .register_manual(&request(7601), ttl())
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE search_source_coordination SET activation_epoch=2,registration_revision=2 WHERE source_id=$1")
        .bind(registration::source(7101).as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE search_source_ownership SET activation_epoch=2,registration_revision=2 WHERE source_id=$1")
        .bind(registration::source(7101).as_uuid()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        registrar.register_manual(&request(7602), ttl()).await,
        Err(GenerationError::Lost)
    ));
    assert!(matches!(
        registrar.renew_manual(&handle, ttl()).await,
        Err(GenerationError::Lost)
    ));
    assert_eq!(counts(&pool).await, (1, 1, 1, 1, 1));
}

#[tokio::test]
async fn guard_insert_failure_rolls_back_identity_target_and_fence() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    sqlx::raw_sql("CREATE FUNCTION synthetic_reject_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic guard insert failure' USING ERRCODE='23514'; END $$; CREATE TRIGGER synthetic_reject_guard BEFORE INSERT ON search_generation_full_guard FOR EACH ROW EXECUTE FUNCTION synthetic_reject_guard();")
        .execute(&pool).await.unwrap();
    assert!(
        registrar
            .register_manual(&request(7701), ttl())
            .await
            .is_err()
    );
    assert_eq!(counts(&pool).await, (0, 0, 0, 0, 0));
    sqlx::query("DROP TRIGGER synthetic_reject_guard ON search_generation_full_guard")
        .execute(&pool)
        .await
        .unwrap();
    registrar
        .register_manual(&request(7701), ttl())
        .await
        .unwrap();
    assert_eq!(counts(&pool).await, (1, 1, 1, 1, 1));
}

#[tokio::test]
async fn generation_key_collision_rolls_back_fence_and_preserves_first_target() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    registrar
        .register_manual(&request(7801), ttl())
        .await
        .unwrap();
    assert!(matches!(
        registrar.register_manual(&request(7801), ttl()).await,
        Err(GenerationError::Conflict)
    ));
    assert_eq!(counts(&pool).await, (1, 1, 1, 1, 1));
}

#[tokio::test]
async fn build_fence_overflow_has_no_partial_registration() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    sqlx::query("UPDATE search_source_coordination SET build_fence_seq=9223372036854775807")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        registrar.register_manual(&request(7901), ttl()).await,
        Err(GenerationError::FenceOverflow)
    ));
    assert_eq!(counts(&pool).await, (0, 0, 0, 0, i64::MAX));
}

#[tokio::test]
async fn expired_full_guard_rejects_late_child_write_and_ready() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    let handle = registrar
        .register_manual(&request(8001), ttl())
        .await
        .unwrap();
    let peer = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let mut boundary = peer.begin().await.unwrap();
    sqlx::query("SELECT source_id FROM search_source_coordination FOR UPDATE")
        .fetch_one(&mut *boundary)
        .await
        .unwrap();
    sqlx::query("UPDATE search_generation_full_guard SET expires_at=clock_timestamp()")
        .execute(&mut *boundary)
        .await
        .unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let barrier_task = barrier.clone();
    let worker = tokio::spawn(async move {
        barrier_task.wait().await;
        registrar.renew_manual(&handle, ttl()).await
    });
    barrier.wait().await;
    boundary.commit().await.unwrap();
    assert!(matches!(worker.await.unwrap(), Err(GenerationError::Lost)));
    assert_db_code(sqlx::query("INSERT INTO search_generation_payload(source_id,generation_id,kind,dto_version,payload,logical_digest,logical_count) VALUES($1,$2,'projection','v1','{\"dto_version\":\"v1\"}',$3,0)")
        .bind(registration::source(7101).as_uuid()).bind(Uuid::from_u128(8001))
        .bind(format!("sha256:{}","a".repeat(64))).execute(&peer).await.unwrap_err(),"23514");
    // これは失効拒否の負例だけ。正のREADYを作らない。
    assert_db_code(
        sqlx::query("UPDATE search_generation SET state='READY',ready_at=clock_timestamp()")
            .execute(&peer)
            .await
            .unwrap_err(),
        "23514",
    );
    assert_eq!(counts(&pool).await, (1, 1, 1, 1, 1));
    peer.close().await;
}

#[tokio::test]
async fn full_target_guard_cannot_be_reissued() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    let handle = registrar
        .register_manual(&request(8101), ttl())
        .await
        .unwrap();
    sqlx::query("UPDATE search_generation_full_guard SET expires_at=clock_timestamp()")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        registrar.renew_manual(&handle, ttl()).await,
        Err(GenerationError::Lost)
    ));
    assert!(matches!(
        registrar.register_manual(&request(8101), ttl()).await,
        Err(GenerationError::Conflict)
    ));
    assert_eq!(counts(&pool).await, (1, 1, 1, 1, 1));
    let expired: bool = sqlx::query_scalar(
        "SELECT expires_at <= clock_timestamp() FROM search_generation_full_guard",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(expired);
}

#[tokio::test]
async fn concurrent_manual_targets_get_distinct_monotone_fences() {
    let (_guard, pool, registrar, _ledger) = fixture().await;
    let registrar = Arc::new(registrar);
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let mut tasks = tokio::task::JoinSet::new();
    for number in [8201, 8202] {
        let registrar = registrar.clone();
        let barrier = barrier.clone();
        tasks.spawn(async move {
            barrier.wait().await;
            registrar.register_manual(&request(number), ttl()).await
        });
    }
    barrier.wait().await;
    while let Some(result) = tasks.join_next().await {
        result.unwrap().unwrap();
    }
    assert_eq!(counts(&pool).await, (2, 2, 2, 2, 2));
    let fences: Vec<i64> = sqlx::query_scalar(
        "SELECT full_build_fence FROM search_generation ORDER BY full_build_fence",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(fences, vec![1, 2]);
}

#[tokio::test]
async fn pure_nonpersistent_or_metadata_only_registration_is_not_a_full_bundle_permit() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://synthetic.invalid/unused")
        .unwrap();
    let ledger = PgSourceRegistrationLedger::new(
        pool.clone(),
        Arc::new(SyntheticHostRegistrationAuthority::new()),
    );
    for retention in [
        RetentionMode::NoRetention,
        RetentionMode::SessionOnly,
        RetentionMode::CacheWithExpiry,
        RetentionMode::PersistentDiscoveryMetadata,
    ] {
        let mut config = registration::remote_config(registration::source(7101), "tenant-a", 1);
        config.retention_mode = retention;
        let registration = SourceRegistration::Remote(
            RemoteSourceRegistration::from_server_config(config).unwrap(),
        );
        assert!(matches!(
            ledger.generation_registrar(
                registration,
                RegistrationActivation::from_persisted(1).unwrap()
            ),
            Err(GenerationError::InvalidInput)
        ));
    }
    pool.close().await;
}

#[tokio::test]
async fn pure_invalid_manifest_and_snapshot_refuse_before_database_io() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://synthetic.invalid/unused")
        .unwrap();
    let ledger = PgSourceRegistrationLedger::new(
        pool.clone(),
        Arc::new(SyntheticHostRegistrationAuthority::new()),
    );
    let registrar = ledger
        .generation_registrar(
            registration::document(registration::source(7101), "tenant-a").await,
            RegistrationActivation::from_persisted(1).unwrap(),
        )
        .unwrap();
    pool.close().await;
    for case in 0..7 {
        let mut target = request(8301);
        match case {
            0 => target.expected_snapshot = "different".into(),
            1 => target.manifest.source_id = registration::source(7999),
            2 => target.manifest.generation_id = ProjectionGenerationId::from_uuid(Uuid::nil()),
            3 => target.manifest.source_snapshot = String::new(),
            4 => target.manifest.resource_count = u64::MAX,
            5 => target.manifest.digest = "sha256:unknown".into(),
            6 => target.manifest.projection_schema_version = String::new(),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                registrar.register_manual(&target, ttl()).await,
                Err(GenerationError::InvalidInput)
            ),
            "case {case}"
        );
    }
}

#[tokio::test]
async fn failed_registration_recheck_closes_existing_generation_registrar() {
    let (_guard, pool, registrar, ledger) = fixture().await;
    let manual = registrar
        .register_manual(&request(8401), ttl())
        .await
        .unwrap();
    let (event, fence) = event(&pool).await;
    let event_handle = registrar
        .register_event(&event, fence, &request(8402), ttl())
        .await
        .unwrap();
    let other_host = SyntheticHostRegistrationAuthority::new();
    let stale =
        registration::publish(&other_host, RegistrationNamespace::Document, 2, vec![]).await;
    assert!(ledger.reconcile(&stale).await.is_err());
    // 正本DBが旧ACTIVEのままでも、不明になった同じprocess gateを迂回できない。
    let active: bool =
        sqlx::query_scalar("SELECT registration_active FROM search_source_coordination")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(active);
    assert!(matches!(
        registrar.register_manual(&request(8403), ttl()).await,
        Err(GenerationError::StoreUnknown)
    ));
    assert!(matches!(
        registrar
            .register_event(&event, fence, &request(8404), ttl())
            .await,
        Err(GenerationError::StoreUnknown)
    ));
    assert!(matches!(
        registrar.renew_manual(&manual, ttl()).await,
        Err(GenerationError::StoreUnknown)
    ));
    assert!(matches!(
        registrar.renew_event(&event_handle, ttl()).await,
        Err(GenerationError::StoreUnknown)
    ));
    assert_eq!(counts(&pool).await, (2, 2, 2, 2, 2));
}

#[tokio::test]
async fn configured_registration_coordinator_role_can_build_but_cannot_ack_or_mutate_domain() {
    let (_guard, admin, _registrar, _ledger) = fixture().await;
    let (event, fence) = event(&admin).await;
    sqlx::raw_sql(include_str!("../sql/roles.sql"))
        .execute(&admin)
        .await
        .unwrap();
    let login = format!("p7_full_guard_{}", Uuid::new_v4().simple());
    let create = format!("CREATE ROLE {login} LOGIN PASSWORD 'p7-synthetic-fixture'");
    sqlx::query(sqlx::AssertSqlSafe(create.as_str()))
        .execute(&admin)
        .await
        .unwrap();
    let grant = format!("GRANT search_registration,search_coordinator TO {login}");
    sqlx::query(sqlx::AssertSqlSafe(grant.as_str()))
        .execute(&admin)
        .await
        .unwrap();
    let role_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(
            (*admin.connect_options())
                .clone()
                .username(&login)
                .password("p7-synthetic-fixture"),
        )
        .await
        .unwrap();
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    let ledger = PgSourceRegistrationLedger::new(role_pool.clone(), host.clone());
    let registration = registration::document(registration::source(7101), "tenant-a").await;
    let remote = registration::publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&remote).await.unwrap();
    let desired = registration::publish(
        &host,
        RegistrationNamespace::Document,
        1,
        vec![registration.clone()],
    )
    .await;
    let activations = ledger.reconcile(&desired).await.unwrap();
    let registrar = ledger
        .generation_registrar(registration, activations[&registration::source(7101)])
        .unwrap();
    let manual = registrar
        .register_manual(&request(8501), ttl())
        .await
        .unwrap();
    let event_handle = registrar
        .register_event(&event, fence, &request(8502), ttl())
        .await
        .unwrap();
    registrar.renew_manual(&manual, ttl()).await.unwrap();
    registrar.renew_event(&event_handle, ttl()).await.unwrap();
    assert_eq!(counts(&admin).await, (2, 2, 2, 2, 2));
    for statement in [
        "UPDATE outbox_events SET delivered_at=NULL",
        "UPDATE outbox_events SET lease_expires_at=clock_timestamp()",
        "UPDATE outbox_events SET payload='{}'",
        "UPDATE audit_outbox_events SET delivered_at=NULL",
        "UPDATE documents SET revision=revision",
    ] {
        assert_db_code(
            sqlx::query(statement)
                .execute(&role_pool)
                .await
                .unwrap_err(),
            "42501",
        );
    }
    let current_token: Uuid = sqlx::query_scalar("SELECT lease_token FROM outbox_events")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(current_token, fence.outbox_token);
    role_pool.close().await;
    let drop_role = format!("DROP OWNED BY {login}; DROP ROLE {login}");
    sqlx::raw_sql(sqlx::AssertSqlSafe(drop_role.as_str()))
        .execute(&admin)
        .await
        .unwrap();
}
