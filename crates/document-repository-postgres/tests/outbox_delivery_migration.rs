use std::path::Path;

use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

const OLD_MIGRATIONS: [&str; 8] = [
    include_str!("../migrations/0001_document_authoritative_core.sql"),
    include_str!("../migrations/0002_document_publish_v0.sql"),
    include_str!("../migrations/0003_document_semantic_inspection_v0.sql"),
    include_str!("../migrations/0004_document_versioning_v0.sql"),
    include_str!("../migrations/0005_document_publication_end_v0.sql"),
    include_str!("../migrations/0006_document_management_access_v0.sql"),
    include_str!("../migrations/0007_document_folder_names_v0.sql"),
    include_str!("../migrations/0008_document_read_state_v0.sql"),
];

async fn postgres() -> (testcontainers::ContainerAsync<GenericImage>, PgPool) {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "outbox_migration_test")
        .start()
        .await
        .expect("disposable PostgreSQL should start");
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/outbox_migration_test"
        ))
        .await
        .unwrap();
    (container, pool)
}

async fn apply_old_migrations(pool: &PgPool) {
    for migration in OLD_MIGRATIONS {
        sqlx::raw_sql(sqlx::AssertSqlSafe(migration))
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn apply_new_migration(pool: &PgPool) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations/0011_outbox_delivery_v0.sql");
    let migration = std::fs::read_to_string(path).expect("0011 migration file must exist");
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration))
        .execute(pool)
        .await
        .unwrap();
}

async fn insert_legacy_event(
    pool: &PgPool,
    event_id: Uuid,
    payload: Value,
    attempt_count: i32,
    delivered: bool,
) {
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at,attempt_count,delivered_at) \
         VALUES ($1,'DocumentRegistered','Document',$2,$3,'2000-01-02T03:04:05Z', \
                 '2000-01-03T04:05:06Z',$4,CASE WHEN $5 THEN '2000-01-04T05:06:07Z'::timestamptz ELSE NULL END)",
    )
    .bind(event_id)
    .bind(Uuid::from_u128(500))
    .bind(payload)
    .bind(attempt_count)
    .bind(delivered)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_audit_event(pool: &PgPool, event_id: Uuid, delivered: bool) {
    sqlx::query(
        "INSERT INTO audit_outbox_events \
         (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id,resource_id, \
          result,trace_id,data,occurred_at,attempt_count,delivered_at) \
         VALUES ($1,'document.registered','fixture','document','test-idp','fixture-actor',$2, \
                 'SUCCEEDED',NULL,'{\"keep\":null}'::jsonb,'2000-02-03T04:05:06Z',3, \
                 CASE WHEN $3 THEN '2000-02-04T05:06:07Z'::timestamptz ELSE NULL END)",
    )
    .bind(event_id)
    .bind(Uuid::from_u128(500))
    .bind(delivered)
    .execute(pool)
    .await
    .unwrap();
}

async fn snapshot(pool: &PgPool, table: &str, added_columns: bool) -> Value {
    let expression = if added_columns {
        "to_jsonb(o) - ARRAY['lease_token','lease_owner','lease_expires_at','last_attempt_at', \
         'dead_lettered_at','last_error_code','attempt_limit','traceparent','tracestate']::text[]"
    } else {
        "to_jsonb(o)"
    };
    let sql = format!(
        "SELECT COALESCE(jsonb_agg({expression} ORDER BY event_id),'[]'::jsonb) FROM {table} AS o"
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn migration_preserves_legacy_domain_and_audit_rows() {
    let (container, pool) = postgres().await;
    apply_old_migrations(&pool).await;
    insert_legacy_event(
        &pool,
        Uuid::from_u128(1),
        json!({"keep": [null, {"x": 1}]}),
        0,
        false,
    )
    .await;
    insert_legacy_event(
        &pool,
        Uuid::from_u128(2),
        json!({"keep": "retry"}),
        8,
        false,
    )
    .await;
    insert_legacy_event(
        &pool,
        Uuid::from_u128(3),
        json!({"keep": "delivered"}),
        2,
        true,
    )
    .await;
    insert_audit_event(&pool, Uuid::from_u128(11), false).await;
    insert_audit_event(&pool, Uuid::from_u128(12), true).await;

    let domain_before = snapshot(&pool, "outbox_events", false).await;
    let audit_before = snapshot(&pool, "audit_outbox_events", false).await;
    let counts_before: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM outbox_events WHERE delivered_at IS NULL), \
                (SELECT count(*) FROM outbox_events WHERE delivered_at IS NOT NULL), \
                (SELECT count(*) FROM audit_outbox_events WHERE delivered_at IS NULL), \
                (SELECT count(*) FROM audit_outbox_events WHERE delivered_at IS NOT NULL)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts_before, (2, 1, 1, 1));

    apply_new_migration(&pool).await;

    assert_eq!(snapshot(&pool, "outbox_events", true).await, domain_before);
    assert_eq!(
        snapshot(&pool, "audit_outbox_events", false).await,
        audit_before
    );
    let counts_after: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM outbox_events WHERE delivered_at IS NULL), \
                (SELECT count(*) FROM outbox_events WHERE delivered_at IS NOT NULL), \
                (SELECT count(*) FROM audit_outbox_events WHERE delivered_at IS NULL), \
                (SELECT count(*) FROM audit_outbox_events WHERE delivered_at IS NOT NULL)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts_after, counts_before);
    let old_rows_without_new_values: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE num_nonnulls(lease_token,lease_owner, \
         lease_expires_at,last_attempt_at,dead_lettered_at,last_error_code,attempt_limit, \
         traceparent,tracestate) = 0",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(old_rows_without_new_values, 3);
    let old_exhausted_is_preserved: bool = sqlx::query_scalar(
        "SELECT attempt_count=8 AND attempt_limit IS NULL AND dead_lettered_at IS NULL \
         FROM outbox_events WHERE event_id=$1",
    )
    .bind(Uuid::from_u128(2))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(old_exhausted_is_preserved);
    let legacy_states: Vec<(Uuid, String, bool)> = sqlx::query_as(
        "SELECT event_id,processing_state,recovery_pending FROM outbox_delivery_state ORDER BY event_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        legacy_states,
        vec![
            (Uuid::from_u128(1), "PENDING".into(), false),
            (Uuid::from_u128(2), "PENDING".into(), false),
            (Uuid::from_u128(3), "DELIVERED".into(), false),
        ]
    );
    let policy: (i16, i64, i32, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT policy_id,revision,max_attempts,lease_min_ms,lease_max_ms, \
         backoff_min_ms,backoff_max_ms FROM outbox_delivery_policy",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(policy, (1, 1, 8, 1000, 120000, 1000, 300000));
    let guessed_version: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
         WHERE table_schema='public' AND table_name='outbox_events' AND column_name='aggregate_version')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!guessed_version);
    insert_legacy_event(&pool, Uuid::from_u128(4), json!({"later": true}), 0, false).await;
    let new_producer_row_has_no_delivery_metadata: bool = sqlx::query_scalar(
        "SELECT num_nonnulls(lease_token,lease_owner,lease_expires_at,last_attempt_at, \
         dead_lettered_at,last_error_code,attempt_limit,traceparent,tracestate) = 0 \
         FROM outbox_events WHERE event_id = $1",
    )
    .bind(Uuid::from_u128(4))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(new_producer_row_has_no_delivery_metadata);

    // The repository's public migration entrypoint must also discover 0011.
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
        ))
        .await
        .unwrap();
    sqlx::query("CREATE DATABASE embedded_migration_trial")
        .execute(&admin)
        .await
        .unwrap();
    let trial = PgPoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/embedded_migration_trial"
        ))
        .await
        .unwrap();
    document_repository_postgres::migrate(&trial).await.unwrap();
    let migrated_version: Option<i64> =
        sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&trial)
            .await
            .unwrap();
    assert_eq!(migrated_version, Some(11));
    let policy_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_delivery_policy")
        .fetch_one(&trial)
        .await
        .unwrap();
    assert_eq!(policy_rows, 1);
}

async fn assert_check(pool: &PgPool, sql: &'static str, constraint: &str) {
    let error = sqlx::query(sql)
        .bind(Uuid::from_u128(21))
        .execute(pool)
        .await
        .expect_err("invalid state must be rejected by PostgreSQL");
    let sqlx::Error::Database(error) = error else {
        panic!("expected a PostgreSQL CHECK error");
    };
    assert_eq!(error.code().as_deref(), Some("23514"));
    assert_eq!(error.constraint(), Some(constraint));
}

#[tokio::test]
async fn migration_rejects_partial_lease_and_double_terminal() {
    let (_container, pool) = postgres().await;
    apply_old_migrations(&pool).await;
    insert_legacy_event(&pool, Uuid::from_u128(21), json!({"kept": true}), 0, false).await;
    apply_new_migration(&pool).await;

    assert_check(
        &pool,
        "UPDATE outbox_events SET attempt_limit=0 WHERE event_id=$1",
        "ck_outbox_attempt_limit",
    )
    .await;
    assert_check(
        &pool,
        "UPDATE outbox_events SET lease_token=gen_random_uuid() WHERE event_id=$1",
        "ck_outbox_lease_complete",
    )
    .await;
    assert_check(&pool, "UPDATE outbox_events SET lease_owner=gen_random_uuid(),lease_expires_at=now() WHERE event_id=$1", "ck_outbox_lease_complete").await;
    assert_check(
        &pool,
        "UPDATE outbox_events SET delivered_at=now(),dead_lettered_at=now() WHERE event_id=$1",
        "ck_outbox_one_terminal",
    )
    .await;
    assert_check(&pool, "UPDATE outbox_events SET delivered_at=now(),lease_token=gen_random_uuid(),lease_owner=gen_random_uuid(),lease_expires_at=now()+interval '1 minute' WHERE event_id=$1", "ck_outbox_terminal_unleased").await;
    assert_check(&pool, "UPDATE outbox_events SET dead_lettered_at=now(),lease_token=gen_random_uuid(),lease_owner=gen_random_uuid(),lease_expires_at=now()+interval '1 minute' WHERE event_id=$1", "ck_outbox_terminal_unleased").await;
    assert_check(
        &pool,
        "UPDATE outbox_events SET last_error_code='raw secret' WHERE event_id=$1",
        "ck_outbox_error_code",
    )
    .await;
    sqlx::query("UPDATE outbox_events SET lease_token=gen_random_uuid(),lease_owner=gen_random_uuid(),lease_expires_at=now()+interval '1 minute',last_error_code='source_unavailable',attempt_limit=8 WHERE event_id=$1")
        .bind(Uuid::from_u128(21)).execute(&pool).await.unwrap();
    for sql in [
        "UPDATE outbox_delivery_policy SET revision=0",
        "UPDATE outbox_delivery_policy SET max_attempts=0",
        "UPDATE outbox_delivery_policy SET lease_min_ms=0",
        "UPDATE outbox_delivery_policy SET lease_min_ms=120001",
        "UPDATE outbox_delivery_policy SET backoff_min_ms=300001",
    ] {
        let error = sqlx::query(sql)
            .execute(&pool)
            .await
            .expect_err("invalid policy must fail");
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514")
        );
    }
}

#[tokio::test]
async fn policy_requires_finite_bounds_and_ordered_ranges() {
    let (_container, pool) = postgres().await;
    apply_old_migrations(&pool).await;
    apply_new_migration(&pool).await;

    // Both inclusive endpoints are valid when each range remains ordered.
    for sql in [
        "UPDATE outbox_delivery_policy SET max_attempts=1,lease_min_ms=1000,lease_max_ms=1000,backoff_min_ms=1000,backoff_max_ms=1000",
        "UPDATE outbox_delivery_policy SET max_attempts=32,lease_min_ms=120000,lease_max_ms=120000,backoff_min_ms=300000,backoff_max_ms=300000",
        "UPDATE outbox_delivery_policy SET max_attempts=8,lease_min_ms=1000,lease_max_ms=120000,backoff_min_ms=1000,backoff_max_ms=300000",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }

    for sql in [
        "UPDATE outbox_delivery_policy SET max_attempts=33",
        "UPDATE outbox_delivery_policy SET max_attempts=0",
        "UPDATE outbox_delivery_policy SET lease_min_ms=999",
        "UPDATE outbox_delivery_policy SET lease_min_ms=999,lease_max_ms=999",
        "UPDATE outbox_delivery_policy SET lease_min_ms=120001,lease_max_ms=120001",
        "UPDATE outbox_delivery_policy SET lease_max_ms=120001",
        "UPDATE outbox_delivery_policy SET lease_min_ms=2000,lease_max_ms=1000",
        "UPDATE outbox_delivery_policy SET backoff_min_ms=999",
        "UPDATE outbox_delivery_policy SET backoff_min_ms=999,backoff_max_ms=999",
        "UPDATE outbox_delivery_policy SET backoff_min_ms=300001,backoff_max_ms=300001",
        "UPDATE outbox_delivery_policy SET backoff_max_ms=300001",
        "UPDATE outbox_delivery_policy SET backoff_min_ms=2000,backoff_max_ms=1000",
        "UPDATE outbox_delivery_policy SET revision=0",
    ] {
        let error = sqlx::query(sql)
            .execute(&pool)
            .await
            .expect_err("out-of-range policy must fail");
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514"),
            "policy statement: {sql}"
        );
    }
}

#[tokio::test]
async fn processing_state_distinguishes_reclaim_and_reap_pending() {
    let (_container, pool) = postgres().await;
    apply_old_migrations(&pool).await;
    for suffix in 31..=37 {
        insert_legacy_event(&pool, Uuid::from_u128(suffix), json!({}), 0, false).await;
    }
    apply_new_migration(&pool).await;
    sqlx::query("UPDATE outbox_events SET delivered_at=now() WHERE event_id=$1")
        .bind(Uuid::from_u128(31))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE outbox_events SET dead_lettered_at=now() WHERE event_id=$1")
        .bind(Uuid::from_u128(32))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE outbox_events SET lease_token=gen_random_uuid(),lease_owner=gen_random_uuid(),lease_expires_at=now()+interval '5 minutes',attempt_count=1,attempt_limit=8 WHERE event_id=$1")
        .bind(Uuid::from_u128(33)).execute(&pool).await.unwrap();
    sqlx::query("UPDATE outbox_events SET lease_token=gen_random_uuid(),lease_owner=gen_random_uuid(),lease_expires_at=now()-interval '5 minutes',attempt_count=1,attempt_limit=8 WHERE event_id=$1")
        .bind(Uuid::from_u128(34)).execute(&pool).await.unwrap();
    sqlx::query("UPDATE outbox_events SET lease_token=gen_random_uuid(),lease_owner=gen_random_uuid(),lease_expires_at=now()-interval '5 minutes',attempt_count=8,attempt_limit=8 WHERE event_id=$1")
        .bind(Uuid::from_u128(35)).execute(&pool).await.unwrap();
    sqlx::query("UPDATE outbox_events SET attempt_count=8 WHERE event_id=$1")
        .bind(Uuid::from_u128(36))
        .execute(&pool)
        .await
        .unwrap();
    let states: Vec<(Uuid, String, bool)> = sqlx::query_as(
        "SELECT event_id,processing_state,recovery_pending FROM outbox_delivery_state ORDER BY event_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        states,
        vec![
            (Uuid::from_u128(31), "DELIVERED".into(), false),
            (Uuid::from_u128(32), "DEAD_LETTER".into(), false),
            (Uuid::from_u128(33), "IN_FLIGHT".into(), false),
            (Uuid::from_u128(34), "PENDING".into(), false),
            (Uuid::from_u128(35), "PENDING".into(), true),
            (Uuid::from_u128(36), "PENDING".into(), false),
            (Uuid::from_u128(37), "PENDING".into(), false),
        ]
    );
    let legacy_exhausted_untouched: bool = sqlx::query_scalar(
        "SELECT attempt_limit IS NULL AND dead_lettered_at IS NULL FROM outbox_events WHERE event_id=$1",
    )
    .bind(Uuid::from_u128(36))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(legacy_exhausted_untouched);

    // A bounded synthetic mix records real planner choices without a performance claim.
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at,attempt_count,delivered_at,attempt_limit,last_attempt_at) \
         SELECT gen_random_uuid(),'DocumentRegistered','Document',gen_random_uuid(),'{}'::jsonb, \
                now()-interval '1 hour',now()-interval '1 minute', \
                CASE WHEN n <= 10 THEN 8 ELSE 0 END, \
                CASE WHEN n > 80 THEN now() ELSE NULL END,8, \
                CASE WHEN n <= 10 THEN now()-interval '2 hours' ELSE NULL END \
         FROM generate_series(1,240) AS n",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("ANALYZE outbox_events")
        .execute(&pool)
        .await
        .unwrap();
    for (name, sql) in [
        (
            "candidate",
            "EXPLAIN (ANALYZE, BUFFERS) SELECT event_id FROM outbox_events WHERE delivered_at IS NULL AND dead_lettered_at IS NULL AND available_at <= clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at <= clock_timestamp()) AND attempt_count < COALESCE(attempt_limit,8) ORDER BY available_at,occurred_at,event_id LIMIT 16",
        ),
        (
            "reaper",
            "EXPLAIN (ANALYZE, BUFFERS) SELECT event_id FROM outbox_events WHERE delivered_at IS NULL AND dead_lettered_at IS NULL AND attempt_limit IS NOT NULL AND attempt_count >= attempt_limit AND (lease_token IS NULL OR lease_expires_at <= clock_timestamp()) ORDER BY last_attempt_at NULLS FIRST,event_id LIMIT 16",
        ),
    ] {
        let plan: Vec<String> = sqlx::query_scalar(sql).fetch_all(&pool).await.unwrap();
        assert!(!plan.is_empty());
        println!("{name} EXPLAIN: {}", plan.join(" | "));
    }
}
