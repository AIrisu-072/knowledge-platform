mod support;

use sqlx::{
    PgPool,
    migrate::Migrator,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use support::postgres::DatabaseGuard;
use uuid::Uuid;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn assert_db_code(error: sqlx::Error, code: &str) {
    let sqlx::Error::Database(error) = error else {
        panic!("expected PostgreSQL error, got {error}");
    };
    assert_eq!(error.code().as_deref(), Some(code), "{error}");
}

async fn postgres() -> (DatabaseGuard, PgPool, PgConnectOptions) {
    support::postgres::postgres("generation_schema_test").await
}

async fn login_pool(admin: &PgPool, options: &PgConnectOptions, group: &str) -> (String, PgPool) {
    assert!(matches!(
        group,
        "search_registration"
            | "search_builder"
            | "search_coordinator"
            | "search_reader"
            | "search_gc"
    ));
    let login = format!("p7_test_{}", Uuid::new_v4().simple());
    let create = format!("CREATE ROLE {login} LOGIN PASSWORD 'p7-disposable-fixture'");
    sqlx::query(sqlx::AssertSqlSafe(create.as_str()))
        .execute(admin)
        .await
        .unwrap();
    let grant = format!("GRANT {group} TO {login}");
    sqlx::query(sqlx::AssertSqlSafe(grant.as_str()))
        .execute(admin)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            options
                .clone()
                .username(&login)
                .password("p7-disposable-fixture"),
        )
        .await
        .unwrap();
    (login, pool)
}

async fn source(pool: &PgPool, source_id: Uuid, tenant: &str) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO search_source_ownership (source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest,created_at,updated_at) VALUES ($1,$2,'DOCUMENT',1,1,1,'ACTIVE','{\"dto_version\":\"v1\"}'::jsonb,$3,clock_timestamp(),clock_timestamp())")
        .bind(source_id)
        .bind(tenant)
        .bind(digest('a'))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO search_source_coordination (source_id,tenant_owner_key,registration_revision,visibility_revision,activation_epoch,registration_active) VALUES ($1,$2,1,1,1,true)")
        .bind(source_id)
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn identity(pool: &PgPool, source_id: Uuid, generation_id: Uuid, tenant: &str) {
    sqlx::query("INSERT INTO search_generation_identity (source_id,generation_id,tenant_owner_key,activation_epoch,created_at) VALUES ($1,$2,$3,1,clock_timestamp())")
        .bind(source_id)
        .bind(generation_id)
        .bind(tenant)
        .execute(pool)
        .await
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn target(
    pool: &PgPool,
    source_id: Uuid,
    generation_id: Uuid,
    build_kind: &str,
    stage_origin: &str,
    event_id: Option<Uuid>,
    source_epoch: Option<i64>,
    guard_token: Option<Uuid>,
    build_fence: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO search_generation (source_id,generation_id,activation_epoch,state,build_kind,stage_origin,stage_event_id,stage_source_epoch,full_guard_token,full_build_fence,source_snapshot,projection_manifest,projection_manifest_digest,projection_resource_count,bundle_version) VALUES ($1,$2,1,'BUILDING',$3,$4,$5,$6,$7,$8,'snapshot-v1','{\"dto_version\":\"v1\"}'::jsonb,$9,0,'v1')")
        .bind(source_id)
        .bind(generation_id)
        .bind(build_kind)
        .bind(stage_origin)
        .bind(event_id)
        .bind(source_epoch)
        .bind(guard_token)
        .bind(build_fence)
        .bind(digest('b'))
        .execute(pool)
        .await?;
    Ok(())
}

async fn full_target(pool: &PgPool, source_id: Uuid, generation_id: Uuid, token: Uuid, fence: i64) {
    identity(pool, source_id, generation_id, "tenant-one").await;
    sqlx::query("UPDATE search_source_coordination SET build_fence_seq=$2 WHERE source_id=$1")
        .bind(source_id)
        .bind(fence)
        .execute(pool)
        .await
        .unwrap();
    target(
        pool,
        source_id,
        generation_id,
        "FULL",
        "MANUAL",
        None,
        None,
        Some(token),
        Some(fence),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO search_generation_full_guard (source_id,target_generation_id,guard_token,build_fence,expires_at) VALUES ($1,$2,$3,$4,clock_timestamp()+interval '1 hour')")
        .bind(source_id)
        .bind(generation_id)
        .bind(token)
        .bind(fence)
        .execute(pool)
        .await
        .unwrap();
}

async fn payload(
    pool: &PgPool,
    source_id: Uuid,
    generation_id: Uuid,
    kind: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(r#"INSERT INTO search_generation_payload (source_id,generation_id,kind,dto_version,payload,logical_digest,logical_count) VALUES ($1,$2,$3,'v1','{"dto_version":"v1"}'::jsonb,$4,0)"#)
        .bind(source_id)
        .bind(generation_id)
        .bind(kind)
        .bind(digest('c'))
        .execute(pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn identity_key_owner_and_activation_survive_gc() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(1);
    let generation_id = Uuid::from_u128(2);
    source(&pool, source_id, "tenant-one").await;

    assert_db_code(
        sqlx::query("INSERT INTO search_generation_identity (source_id,generation_id,tenant_owner_key,activation_epoch,created_at) VALUES ($1,$2,'tenant-two',1,clock_timestamp())")
            .bind(source_id)
            .bind(generation_id)
            .execute(&pool)
            .await
            .unwrap_err(),
        "23503",
    );
    assert_db_code(
        sqlx::query("INSERT INTO search_generation_identity (source_id,generation_id,tenant_owner_key,activation_epoch,created_at) VALUES ($1,$2,'tenant-one',2,clock_timestamp())")
            .bind(source_id)
            .bind(generation_id)
            .execute(&pool)
            .await
            .unwrap_err(),
        "23514",
    );
    identity(&pool, source_id, generation_id, "tenant-one").await;
    for statement in [
        "UPDATE search_generation_identity SET tenant_owner_key='tenant-two' WHERE source_id=$1 AND generation_id=$2",
        "UPDATE search_generation_identity SET activation_epoch=2 WHERE source_id=$1 AND generation_id=$2",
        "DELETE FROM search_generation_identity WHERE source_id=$1 AND generation_id=$2",
    ] {
        assert_db_code(
            sqlx::query(statement)
                .bind(source_id)
                .bind(generation_id)
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
    assert_db_code(
        sqlx::query("INSERT INTO search_generation_identity (source_id,generation_id,tenant_owner_key,activation_epoch,created_at) VALUES ($1,$2,'tenant-one',1,clock_timestamp())")
            .bind(source_id)
            .bind(generation_id)
            .execute(&pool)
            .await
            .unwrap_err(),
        "23505",
    );
}

#[tokio::test]
async fn new_identity_rejects_tombstoned_or_old_activation() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(5);
    source(&pool, source_id, "tenant-one").await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE search_source_ownership SET state='TOMBSTONED',activation_epoch=2 WHERE source_id=$1")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_coordination SET registration_active=false,activation_epoch=2 WHERE source_id=$1")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    for (index, activation) in [1_i64, 2].into_iter().enumerate() {
        assert_db_code(
            sqlx::query("INSERT INTO search_generation_identity (source_id,generation_id,tenant_owner_key,activation_epoch,created_at) VALUES ($1,$2,'tenant-one',$3,clock_timestamp())")
                .bind(source_id)
                .bind(Uuid::from_u128(6 + index as u128))
                .bind(activation)
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
}

#[tokio::test]
async fn generation_requires_complete_origin_and_full_target_binding() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(10);
    source(&pool, source_id, "tenant-one").await;
    let token = Uuid::from_u128(11);
    let event_id = Uuid::from_u128(12);
    let invalid = [
        ("FULL", "EVENT", None, Some(1), Some(token), Some(1)),
        ("FULL", "EVENT", Some(event_id), None, Some(token), Some(1)),
        (
            "FULL",
            "EVENT",
            Some(event_id),
            Some(0),
            Some(token),
            Some(1),
        ),
        (
            "FULL",
            "MANUAL",
            Some(event_id),
            Some(1),
            Some(token),
            Some(1),
        ),
        ("FULL", "MANUAL", None, None, None, Some(1)),
        ("FULL", "MANUAL", None, None, Some(token), Some(0)),
        ("INCREMENTAL", "MANUAL", None, None, Some(token), Some(1)),
        ("OTHER", "MANUAL", None, None, None, None),
    ];
    for (index, (kind, origin, event, epoch, guard, fence)) in invalid.into_iter().enumerate() {
        let key = Uuid::from_u128(20 + index as u128);
        identity(&pool, source_id, key, "tenant-one").await;
        assert_db_code(
            target(
                &pool, source_id, key, kind, origin, event, epoch, guard, fence,
            )
            .await
            .unwrap_err(),
            "23514",
        );
    }
    let event_key = Uuid::from_u128(40);
    identity(&pool, source_id, event_key, "tenant-one").await;
    target(
        &pool,
        source_id,
        event_key,
        "FULL",
        "EVENT",
        Some(event_id),
        Some(1),
        Some(token),
        Some(1),
    )
    .await
    .unwrap();
    for statement in [
        "UPDATE search_generation SET stage_origin='MANUAL' WHERE source_id=$1 AND generation_id=$2",
        "UPDATE search_generation SET stage_source_epoch=2 WHERE source_id=$1 AND generation_id=$2",
        "UPDATE search_generation SET full_build_fence=2 WHERE source_id=$1 AND generation_id=$2",
        "UPDATE search_generation SET source_snapshot='other' WHERE source_id=$1 AND generation_id=$2",
    ] {
        assert_db_code(
            sqlx::query(statement)
                .bind(source_id)
                .bind(event_key)
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
}

#[tokio::test]
async fn full_guard_exact_target_binding_and_reissue_are_rejected() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(50);
    source(&pool, source_id, "tenant-one").await;
    let first = Uuid::from_u128(51);
    let token = Uuid::from_u128(52);
    full_target(&pool, source_id, first, token, 1).await;
    let second = Uuid::from_u128(53);
    identity(&pool, source_id, second, "tenant-one").await;
    target(
        &pool,
        source_id,
        second,
        "FULL",
        "MANUAL",
        None,
        None,
        Some(Uuid::from_u128(54)),
        Some(2),
    )
    .await
    .unwrap();
    assert_db_code(
        sqlx::query("INSERT INTO search_generation_full_guard (source_id,target_generation_id,guard_token,build_fence,expires_at) VALUES ($1,$2,$3,3,clock_timestamp()+interval '1 hour')")
            .bind(source_id)
            .bind(second)
            .bind(Uuid::from_u128(55))
            .execute(&pool)
            .await
            .unwrap_err(),
        "23503",
    );
    sqlx::query(
        "UPDATE search_generation SET state='FAILED' WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(source_id)
    .bind(first)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM search_generation_full_guard WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(source_id)
    .bind(first)
    .execute(&pool)
    .await
    .unwrap();
    assert_db_code(
        sqlx::query("INSERT INTO search_generation_full_guard (source_id,target_generation_id,guard_token,build_fence,expires_at) VALUES ($1,$2,$3,1,clock_timestamp()+interval '1 hour')")
            .bind(source_id)
            .bind(first)
            .bind(token)
            .execute(&pool)
            .await
            .unwrap_err(),
        "23514",
    );
}

#[tokio::test]
async fn orphan_legacy_current_stops_0003_without_clearing_pointer() {
    let (_container, pool, _) = postgres().await;
    let migrations = Migrator::new(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations"
    )))
    .await
    .unwrap();
    let first_two = migrations
        .migrations
        .iter()
        .filter(|migration| migration.version <= 2)
        .cloned()
        .collect();
    let mut before_generation = Migrator::with_migrations(first_two);
    before_generation.dangerous_set_table_name("search_runtime_sqlx_migrations");
    before_generation.run(&pool).await.unwrap();
    let source_id = Uuid::from_u128(80);
    let orphan_id = Uuid::from_u128(81);
    source(&pool, source_id, "tenant-one").await;
    sqlx::query("UPDATE search_source_coordination SET current_generation_id=$2,current_manifest_digest=$3,current_bundle_digest=$3,pointer_revision=1 WHERE source_id=$1")
        .bind(source_id)
        .bind(orphan_id)
        .bind(digest('a'))
        .execute(&pool)
        .await
        .unwrap();

    assert!(search_runtime::migrate(&pool).await.is_err());
    let pointer: Uuid = sqlx::query_scalar(
        "SELECT current_generation_id FROM search_source_coordination WHERE source_id=$1",
    )
    .bind(source_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pointer, orphan_id);
    let applied: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM search_runtime_sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(applied, vec![1, 2]);
}

#[tokio::test]
async fn current_pointer_uses_composite_key_but_historical_receipt_does_not_pin() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let first_source = Uuid::from_u128(84);
    let second_source = Uuid::from_u128(85);
    let generation_id = Uuid::from_u128(86);
    source(&pool, first_source, "tenant-one").await;
    source(&pool, second_source, "tenant-one").await;
    identity(&pool, first_source, generation_id, "tenant-one").await;
    target(
        &pool,
        first_source,
        generation_id,
        "INCREMENTAL",
        "MANUAL",
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_db_code(
        sqlx::query("UPDATE search_source_coordination SET current_generation_id=$2,current_manifest_digest=$3,current_bundle_digest=$3 WHERE source_id=$1")
            .bind(second_source)
            .bind(generation_id)
            .bind(digest('b'))
            .execute(&pool)
            .await
            .unwrap_err(),
        "23503",
    );
    sqlx::query("INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at,bundle_version) VALUES ($1,$2,$3,$4,$4,1,clock_timestamp(),'v1')")
        .bind(second_source)
        .bind(Uuid::from_u128(87))
        .bind(generation_id)
        .bind(digest('b'))
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn child_writes_require_live_exact_full_guard_and_building_parent() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(90);
    let target_id = Uuid::from_u128(91);
    source(&pool, source_id, "tenant-one").await;
    full_target(&pool, source_id, target_id, Uuid::from_u128(92), 1).await;

    payload(&pool, source_id, target_id, "projection")
        .await
        .unwrap();
    assert_db_code(
        payload(&pool, source_id, target_id, "unexpected")
            .await
            .unwrap_err(),
        "23514",
    );
    let other_source = Uuid::from_u128(93);
    source(&pool, other_source, "tenant-one").await;
    assert_db_code(
        payload(&pool, other_source, target_id, "unit_manifest")
            .await
            .unwrap_err(),
        "23503",
    );
    sqlx::query("UPDATE search_generation_full_guard SET expires_at=clock_timestamp()-interval '1 second' WHERE source_id=$1 AND target_generation_id=$2")
        .bind(source_id)
        .bind(target_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_db_code(
        payload(&pool, source_id, target_id, "unit_manifest")
            .await
            .unwrap_err(),
        "23514",
    );
    assert_db_code(
        sqlx::query("UPDATE search_generation_payload SET logical_count=1 WHERE source_id=$1 AND generation_id=$2 AND kind='projection'")
            .bind(source_id)
            .bind(target_id)
            .execute(&pool)
            .await
            .unwrap_err(),
        "23514",
    );
}

#[tokio::test]
async fn ready_parent_and_lease_scope_are_immutable() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(100);
    let target_id = Uuid::from_u128(101);
    source(&pool, source_id, "tenant-one").await;
    full_target(&pool, source_id, target_id, Uuid::from_u128(102), 1).await;
    payload(&pool, source_id, target_id, "projection")
        .await
        .unwrap();
    payload(&pool, source_id, target_id, "unit_manifest")
        .await
        .unwrap();
    payload(&pool, source_id, target_id, "body_coverage")
        .await
        .unwrap();
    sqlx::query(r#"INSERT INTO search_generation_receipt (source_id,generation_id,source_snapshot,receipt_version,projection_digest,unit_manifest_digest,unit_count,body_coverage_digest,body_item_count,lexical_digest,lexical_count,lexical_schema_version,lexical_analyzer_version,graph_input_digest,graph_input_count,profile_set_digest,composite_digest,graph_backend,graph_schema_version,graph_mapping_digest,graph_content_digest,graph_resource_count,graph_relation_count,receipt_dto) VALUES ($1,$2,'snapshot-v1','v1',$3,$3,0,$3,0,$3,0,'v1','v1',$3,0,$3,$3,'postgresql','v1',$3,$3,0,0,'{"dto_version":"v1"}'::jsonb)"#)
        .bind(source_id)
        .bind(target_id)
        .bind(digest('c'))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO search_lexical_artifact (source_id,generation_id,index_relpath,index_format_version,lexical_schema_version,tree_digest,logical_digest,searchable_doc_count,unit_seal_digest,unit_seal_count,finalized_at) VALUES ($1,$2,$3,'v1','v1',$4,$4,0,$4,0,clock_timestamp())")
        .bind(source_id)
        .bind(target_id)
        .bind(format!("generations/{source_id}/{target_id}"))
        .bind(digest('c'))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE search_generation SET state='READY',ready_at=clock_timestamp() WHERE source_id=$1 AND generation_id=$2")
        .bind(source_id)
        .bind(target_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_db_code(
        sqlx::query(
            "UPDATE search_generation SET state='BUILDING' WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(source_id)
        .bind(target_id)
        .execute(&pool)
        .await
        .unwrap_err(),
        "23514",
    );
    assert_db_code(
        sqlx::query("UPDATE search_generation_payload SET logical_count=1 WHERE source_id=$1 AND generation_id=$2")
            .bind(source_id)
            .bind(target_id)
            .execute(&pool)
            .await
            .unwrap_err(),
        "23514",
    );
    for statement in [
        "UPDATE search_generation_receipt SET unit_count=1 WHERE source_id=$1 AND generation_id=$2",
        "UPDATE search_lexical_artifact SET searchable_doc_count=1 WHERE source_id=$1 AND generation_id=$2",
    ] {
        assert_db_code(
            sqlx::query(statement)
                .bind(source_id)
                .bind(target_id)
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }

    sqlx::query("UPDATE search_source_coordination SET fence_epoch=1,current_generation_id=$2,current_manifest_digest=$3,current_bundle_digest=$4,pointer_revision=1,last_published_epoch=1 WHERE source_id=$1")
        .bind(source_id)
        .bind(target_id)
        .bind(digest('b'))
        .bind(digest('c'))
        .execute(&pool)
        .await
        .unwrap();

    let lease_id = Uuid::from_u128(103);
    sqlx::query("INSERT INTO search_evaluation_lease (source_id,lease_id,evaluation_id,generation_id,activation_epoch,tenant_owner_key,actor_scope_ref,registration_revision,visibility_revision,access_revision,manifest_digest,bundle_digest,expires_at) VALUES ($1,$2,$3,$4,1,'tenant-one','host:actor-scope',1,1,1,$5,$6,clock_timestamp()+interval '1 hour')")
        .bind(source_id)
        .bind(lease_id)
        .bind(Uuid::from_u128(104))
        .bind(target_id)
        .bind(digest('b'))
        .bind(digest('c'))
        .execute(&pool)
        .await
        .unwrap();
    for statement in [
        "UPDATE search_evaluation_lease SET tenant_owner_key='tenant-two' WHERE source_id=$1 AND lease_id=$2",
        "UPDATE search_evaluation_lease SET actor_scope_ref='other' WHERE source_id=$1 AND lease_id=$2",
        "UPDATE search_evaluation_lease SET registration_revision=2 WHERE source_id=$1 AND lease_id=$2",
        "UPDATE search_evaluation_lease SET bundle_digest='sha256:bad' WHERE source_id=$1 AND lease_id=$2",
    ] {
        assert_db_code(
            sqlx::query(statement)
                .bind(source_id)
                .bind(lease_id)
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
}

#[tokio::test]
async fn roles_reject_pointer_parent_child_and_reader_writes() {
    let (_container, admin, options) = postgres().await;
    search_runtime::migrate(&admin).await.unwrap();
    let roles = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/sql/roles.sql"))
        .expect("P7-03 role grants must exist");
    sqlx::raw_sql(sqlx::AssertSqlSafe(roles.as_str()))
        .execute(&admin)
        .await
        .unwrap();
    let source_id = Uuid::from_u128(110);
    let target_id = Uuid::from_u128(111);
    source(&admin, source_id, "tenant-one").await;
    full_target(&admin, source_id, target_id, Uuid::from_u128(112), 1).await;
    let (registration_login, registration) =
        login_pool(&admin, &options, "search_registration").await;
    let (builder_login, builder) = login_pool(&admin, &options, "search_builder").await;
    let (coordinator_login, coordinator) = login_pool(&admin, &options, "search_coordinator").await;
    let (reader_login, reader) = login_pool(&admin, &options, "search_reader").await;
    let (gc_login, gc) = login_pool(&admin, &options, "search_gc").await;

    sqlx::query(r#"INSERT INTO search_generation_payload (source_id,generation_id,kind,dto_version,payload,logical_digest,logical_count) VALUES ($1,$2,'projection','v1','{"dto_version":"v1"}'::jsonb,$3,0)"#)
        .bind(source_id)
        .bind(target_id)
        .bind(digest('c'))
        .execute(&builder)
        .await
        .unwrap();

    for (pool, statement) in [
        (
            &registration,
            "UPDATE search_source_coordination SET current_generation_id=current_generation_id WHERE source_id=$1",
        ),
        (
            &builder,
            "UPDATE search_generation SET source_snapshot='tampered' WHERE source_id=$1",
        ),
        (
            &builder,
            "UPDATE search_generation_full_guard SET expires_at=clock_timestamp() WHERE source_id=$1",
        ),
        (
            &builder,
            "UPDATE search_source_coordination SET pointer_revision=2 WHERE source_id=$1",
        ),
        (
            &builder,
            "DELETE FROM search_generation_payload WHERE source_id=$1",
        ),
        (
            &reader,
            "UPDATE search_evaluation_lease SET expires_at=clock_timestamp() WHERE source_id=$1",
        ),
    ] {
        assert_db_code(
            sqlx::query(statement)
                .bind(source_id)
                .execute(pool)
                .await
                .unwrap_err(),
            "42501",
        );
    }
    sqlx::query("UPDATE search_source_coordination SET pointer_revision=pointer_revision WHERE source_id=$1")
        .bind(source_id)
        .execute(&coordinator)
        .await
        .unwrap();
    assert_db_code(
        sqlx::query(
            "UPDATE search_generation SET state='DELETING' WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(source_id)
        .bind(target_id)
        .execute(&coordinator)
        .await
        .unwrap_err(),
        "23514",
    );
    assert_db_code(
        sqlx::query("DELETE FROM search_generation_payload WHERE source_id=$1")
            .bind(source_id)
            .execute(&gc)
            .await
            .unwrap_err(),
        "23514",
    );
    sqlx::query(
        "UPDATE search_generation SET state='DELETING' WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(source_id)
    .bind(target_id)
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM search_generation_full_guard WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(source_id)
    .bind(target_id)
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query("DELETE FROM search_generation_payload WHERE source_id=$1 AND generation_id=$2")
        .bind(source_id)
        .bind(target_id)
        .execute(&gc)
        .await
        .unwrap();

    registration.close().await;
    builder.close().await;
    coordinator.close().await;
    reader.close().await;
    gc.close().await;
    for login in [
        registration_login,
        builder_login,
        coordinator_login,
        reader_login,
        gc_login,
    ] {
        let drop = format!("DROP ROLE {login}");
        sqlx::query(sqlx::AssertSqlSafe(drop.as_str()))
            .execute(&admin)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn lease_insert_rejects_invalid_scope_and_digest() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(120);
    let target_id = Uuid::from_u128(121);
    source(&pool, source_id, "tenant-one").await;
    full_target(&pool, source_id, target_id, Uuid::from_u128(122), 1).await;
    sqlx::query("UPDATE search_generation SET state='READY',ready_at=clock_timestamp() WHERE source_id=$1 AND generation_id=$2")
        .bind(source_id)
        .bind(target_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_coordination SET fence_epoch=1,current_generation_id=$2,current_manifest_digest=$3,current_bundle_digest=$4,pointer_revision=1,last_published_epoch=1 WHERE source_id=$1")
        .bind(source_id)
        .bind(target_id)
        .bind(digest('b'))
        .bind(digest('c'))
        .execute(&pool)
        .await
        .unwrap();
    for (index, (actor_ref, revision, bundle_digest)) in [
        ("".into(), 1, digest('c')),
        (" actor".into(), 1, digest('c')),
        ("actor\n".into(), 1, digest('c')),
        ("é".repeat(129), 1, digest('c')),
        ("actor".into(), 0, digest('c')),
        ("actor".into(), 1, "sha256:BAD".into()),
    ]
    .into_iter()
    .enumerate()
    {
        assert_db_code(
            sqlx::query("INSERT INTO search_evaluation_lease (source_id,lease_id,evaluation_id,generation_id,activation_epoch,tenant_owner_key,actor_scope_ref,registration_revision,visibility_revision,access_revision,manifest_digest,bundle_digest,expires_at) VALUES ($1,$2,$3,$4,1,'tenant-one',$5,$6,1,1,$7,$8,clock_timestamp()+interval '1 hour')")
                .bind(source_id)
                .bind(Uuid::from_u128(123 + index as u128))
                .bind(Uuid::from_u128(130 + index as u128))
                .bind(target_id)
                .bind(actor_ref)
                .bind(revision)
                .bind(digest('b'))
                .bind(bundle_digest)
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
}

#[tokio::test]
async fn lease_insert_requires_current_ready_generation() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(140);
    let target_id = Uuid::from_u128(141);
    source(&pool, source_id, "tenant-one").await;
    full_target(&pool, source_id, target_id, Uuid::from_u128(142), 1).await;
    let insert = "INSERT INTO search_evaluation_lease (source_id,lease_id,evaluation_id,generation_id,activation_epoch,tenant_owner_key,actor_scope_ref,registration_revision,visibility_revision,access_revision,manifest_digest,bundle_digest,expires_at) VALUES ($1,$2,$3,$4,1,'tenant-one','host:scope',1,1,1,$5,$6,clock_timestamp()+interval '1 hour')";
    for lease in [Uuid::from_u128(143), Uuid::from_u128(144)] {
        assert_db_code(
            sqlx::query(insert)
                .bind(source_id)
                .bind(lease)
                .bind(Uuid::from_u128(145))
                .bind(target_id)
                .bind(digest('b'))
                .bind(digest('c'))
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
        if lease == Uuid::from_u128(143) {
            sqlx::query("UPDATE search_generation SET state='READY',ready_at=clock_timestamp() WHERE source_id=$1 AND generation_id=$2")
                .bind(source_id)
                .bind(target_id)
                .execute(&pool)
                .await
                .unwrap();
        }
    }
    sqlx::query("UPDATE search_source_coordination SET fence_epoch=1,current_generation_id=$2,current_manifest_digest=$3,current_bundle_digest=$4,pointer_revision=1,last_published_epoch=1 WHERE source_id=$1")
        .bind(source_id)
        .bind(target_id)
        .bind(digest('b'))
        .bind(digest('c'))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(insert)
        .bind(source_id)
        .bind(Uuid::from_u128(146))
        .bind(Uuid::from_u128(145))
        .bind(target_id)
        .bind(digest('b'))
        .bind(digest('c'))
        .execute(&pool)
        .await
        .unwrap();
}

fn malformed_dto_versions() -> [&'static str; 5] {
    [
        r#"{"dto_version":null}"#,
        r#"{"dto_version":7}"#,
        r#"{"dto_version":true}"#,
        r#"{"dto_version":["v1"]}"#,
        "[]",
    ]
}

#[tokio::test]
async fn generation_manifest_rejects_json_null_and_wrong_version_types() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(160);
    source(&pool, source_id, "tenant-one").await;
    for (index, malformed) in malformed_dto_versions().into_iter().enumerate() {
        let generation_id = Uuid::from_u128(161 + index as u128);
        identity(&pool, source_id, generation_id, "tenant-one").await;
        assert_db_code(
            sqlx::query("INSERT INTO search_generation (source_id,generation_id,activation_epoch,state,build_kind,stage_origin,source_snapshot,projection_manifest,projection_manifest_digest,projection_resource_count,bundle_version) VALUES ($1,$2,1,'BUILDING','INCREMENTAL','MANUAL','snapshot-v1',$3::jsonb,$4,0,'v1')")
                .bind(source_id)
                .bind(generation_id)
                .bind(malformed)
                .bind(digest('b'))
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
}

#[tokio::test]
async fn generation_payload_rejects_json_null_and_wrong_version_types() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(170);
    let generation_id = Uuid::from_u128(171);
    source(&pool, source_id, "tenant-one").await;
    full_target(&pool, source_id, generation_id, Uuid::from_u128(172), 1).await;
    for malformed in malformed_dto_versions() {
        assert_db_code(
            sqlx::query("INSERT INTO search_generation_payload (source_id,generation_id,kind,dto_version,payload,logical_digest,logical_count) VALUES ($1,$2,'projection','v1',$3::jsonb,$4,0)")
                .bind(source_id)
                .bind(generation_id)
                .bind(malformed)
                .bind(digest('c'))
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
}

#[tokio::test]
async fn generation_receipt_rejects_json_null_and_wrong_version_types() {
    let (_container, pool, _) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(180);
    let generation_id = Uuid::from_u128(181);
    source(&pool, source_id, "tenant-one").await;
    full_target(&pool, source_id, generation_id, Uuid::from_u128(182), 1).await;
    for malformed in malformed_dto_versions() {
        assert_db_code(
            sqlx::query("INSERT INTO search_generation_receipt (source_id,generation_id,source_snapshot,receipt_version,projection_digest,unit_manifest_digest,unit_count,body_coverage_digest,body_item_count,lexical_digest,lexical_count,lexical_schema_version,lexical_analyzer_version,graph_input_digest,graph_input_count,profile_set_digest,composite_digest,graph_backend,graph_schema_version,graph_mapping_digest,graph_content_digest,graph_resource_count,graph_relation_count,receipt_dto) VALUES ($1,$2,'snapshot-v1','v1',$3,$3,0,$3,0,$3,0,'v1','v1',$3,0,$3,$3,'postgresql','v1',$3,$3,0,0,$4::jsonb)")
                .bind(source_id)
                .bind(generation_id)
                .bind(digest('c'))
                .bind(malformed)
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
}

#[tokio::test]
async fn gc_login_cannot_fail_or_ready_a_building_generation() {
    let (_container, admin, options) = postgres().await;
    search_runtime::migrate(&admin).await.unwrap();
    let roles =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/sql/roles.sql")).unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(roles.as_str()))
        .execute(&admin)
        .await
        .unwrap();
    let source_id = Uuid::from_u128(190);
    let generation_id = Uuid::from_u128(191);
    source(&admin, source_id, "tenant-one").await;
    full_target(&admin, source_id, generation_id, Uuid::from_u128(192), 1).await;
    let (login, gc) = login_pool(&admin, &options, "search_gc").await;
    let ready_error = sqlx::query(
        "UPDATE search_generation SET state='READY' WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(source_id)
    .bind(generation_id)
    .execute(&gc)
    .await
    .unwrap_err();
    let sqlx::Error::Database(ready_error) = ready_error else {
        panic!("expected role-trigger rejection, got {ready_error}");
    };
    assert_eq!(ready_error.code().as_deref(), Some("23514"));
    assert_eq!(
        ready_error.message(),
        "only coordinator can settle generation build"
    );
    assert_db_code(
        sqlx::query(
            "UPDATE search_generation SET state='FAILED' WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(source_id)
        .bind(generation_id)
        .execute(&gc)
        .await
        .unwrap_err(),
        "23514",
    );
    sqlx::query(
        "UPDATE search_generation SET state='DELETING' WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(source_id)
    .bind(generation_id)
    .execute(&gc)
    .await
    .unwrap();
    gc.close().await;
    let drop = format!("DROP ROLE {login}");
    sqlx::query(sqlx::AssertSqlSafe(drop.as_str()))
        .execute(&admin)
        .await
        .unwrap();
}
