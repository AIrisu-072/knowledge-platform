//! P6-G08: bounded telemetry/trace and a real PostgreSQL least-privilege role.

#[path = "support/postgres.rs"]
mod postgres_fixture;

use std::{
    fs,
    sync::{Arc, Mutex},
};

use document_repository_postgres::migrate;
use outbox_delivery::{
    DeliveryConfig, DeliveryDecision, DeliveryEnvelope, DeliveryPolicy, OutboxStore,
    observe::{
        DeliveryMetric, DeliveryMetricKind, DeliveryObserver, DeliveryRoute, DeliverySpan,
        validate_trace_context,
    },
    postgres::PostgresOutboxStore,
    runner::{DeliveryContext, DeliveryHandler, DeliveryRunner, NoopAdmission, NoopPermit},
};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Default)]
struct RecordingObserver(Mutex<Vec<DeliveryMetric>>, Mutex<Vec<DeliverySpan>>);

impl DeliveryObserver for RecordingObserver {
    fn record(&self, metric: DeliveryMetric) {
        self.0.lock().unwrap().push(metric);
    }
    fn start_span(&self, span: &DeliverySpan) {
        self.1.lock().unwrap().push(span.clone());
    }
}

struct AppliedHandler;

impl DeliveryHandler<NoopPermit> for AppliedHandler {
    fn deliver(
        &self,
        _event: DeliveryEnvelope,
        _context: DeliveryContext,
        _permit: NoopPermit,
    ) -> outbox_delivery::HandlerFuture<'_, DeliveryDecision> {
        Box::pin(async { DeliveryDecision::Applied })
    }
}

#[tokio::test]
async fn metrics_do_not_contain_high_cardinality_or_payload() {
    assert_eq!(
        DeliveryMetricKind::ALL,
        [
            DeliveryMetricKind::Pending,
            DeliveryMetricKind::InFlight,
            DeliveryMetricKind::DeadLetter,
            DeliveryMetricKind::OldestAge,
            DeliveryMetricKind::Claimed,
            DeliveryMetricKind::Acknowledged,
            DeliveryMetricKind::Retried,
            DeliveryMetricKind::Reaped,
            DeliveryMetricKind::StaleFence,
            DeliveryMetricKind::HandlerDuration,
            DeliveryMetricKind::CommitToAckLag,
            DeliveryMetricKind::StoreError,
            DeliveryMetricKind::ConsumerError,
        ]
    );
    let (_guard, pool, _url) = postgres_fixture::postgres("p6_observability").await;
    migrate(&pool).await.unwrap();
    let id = Uuid::now_v7();
    let secret = "payload-sentinel-never-a-metric-label";
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at) \
         VALUES ($1,'DocumentRegistered','Document',$2,$3, \
                 '2000-01-01T00:00:00Z','2000-01-01T00:00:00Z')",
    )
    .bind(id)
    .bind(Uuid::from_u128(40))
    .bind(json!({"secret": secret, "actor": "actor-sentinel"}))
    .execute(&pool)
    .await
    .unwrap();

    let parent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    sqlx::query("UPDATE outbox_events SET traceparent=$2,tracestate='vendor=trace-sentinel' WHERE event_id=$1")
        .bind(id).bind(parent).execute(&pool).await.unwrap();
    // 配送中・期限切れ・上限到達・DLQ を同時に置き、DB 時計での区別を確認する。
    for state in ["in_flight", "expired", "exhausted", "dead_letter"] {
        sqlx::query(
            "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at, \
             attempt_count,attempt_limit,lease_token,lease_owner,lease_expires_at,dead_lettered_at) \
             VALUES ($1,'DocumentRegistered','Document',$2,'{}','2001-01-01T00:00:00Z','2001-01-01T00:00:00Z', \
             CASE WHEN $3='exhausted' THEN 8 ELSE 1 END,8, \
             CASE WHEN $3 IN ('in_flight','expired') THEN gen_random_uuid() END, \
             CASE WHEN $3 IN ('in_flight','expired') THEN gen_random_uuid() END, \
             CASE WHEN $3='in_flight' THEN clock_timestamp()+interval '1 hour' \
                  WHEN $3='expired' THEN clock_timestamp()-interval '1 hour' END, \
             CASE WHEN $3='dead_letter' THEN clock_timestamp() END)"
        ).bind(Uuid::now_v7()).bind(Uuid::from_u128(40)).bind(state).execute(&pool).await.unwrap();
    }
    let observer = Arc::new(RecordingObserver::default());
    let runner = DeliveryRunner::new(
        Arc::new(PostgresOutboxStore::new(
            pool.clone(),
            DeliveryPolicy::default(),
        )),
        Arc::new(AppliedHandler),
        Arc::new(NoopAdmission),
        DeliveryConfig {
            batch_size: 1,
            max_in_flight: 1,
            ..DeliveryConfig::default()
        },
    )
    .unwrap()
    .with_observer(observer.clone(), DeliveryRoute::Generic);
    runner.run_cycle().await.unwrap();
    let metrics = observer.0.lock().unwrap().clone();
    for (kind, value, outcome) in [
        (DeliveryMetricKind::Pending, 3.0, "observed"),
        (DeliveryMetricKind::InFlight, 1.0, "observed"),
        (DeliveryMetricKind::DeadLetter, 1.0, "observed"),
        (DeliveryMetricKind::Pending, 1.0, "lease_expired"),
        (DeliveryMetricKind::Pending, 1.0, "exhausted"),
        (DeliveryMetricKind::Reaped, 1.0, "exhausted"),
    ] {
        assert!(
            metrics.iter().any(|m| m.kind() == kind
                && m.value() == value
                && m.labels().contains(&("outcome", outcome))),
            "missing {kind:?}/{outcome}"
        );
    }
    assert!(
        metrics
            .iter()
            .any(|m| m.kind() == DeliveryMetricKind::OldestAge && m.value() > 86400.0)
    );
    assert!(
        !metrics
            .iter()
            .any(|m| m.kind() == DeliveryMetricKind::CommitToAckLag)
    );
    let spans = observer.1.lock().unwrap().clone();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].parent().unwrap().traceparent(), parent);
    assert_eq!(
        spans[0].context().tracestate(),
        Some("vendor=trace-sentinel")
    );
    assert!(!format!("{:?}", spans[0]).contains("trace-sentinel"));
    assert!(
        metrics
            .iter()
            .any(|m| m.kind() == DeliveryMetricKind::Claimed)
    );
    assert!(
        metrics
            .iter()
            .any(|m| m.kind() == DeliveryMetricKind::Acknowledged)
    );
    for metric in metrics.iter() {
        for (key, value) in metric.labels() {
            assert!(["route", "error_code", "outcome"].contains(&key));
            assert!(!value.contains(secret));
            assert!(!value.contains("actor-sentinel"));
            assert!(!value.contains(&id.to_string()));
        }
        assert!(!format!("{metric:?}").contains(secret));
    }
    assert!(
        metrics
            .iter()
            .all(|m| !m.labels().iter().any(|(_, v)| v.contains(' ')))
    );
    let delivered: bool =
        sqlx::query_scalar("SELECT delivered_at IS NOT NULL FROM outbox_events WHERE event_id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(delivered);
}

#[test]
fn invalid_trace_is_ignored_without_log_leak() {
    let valid = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let parsed = validate_trace_context(Some(valid), Some("vendor=opaque"))
        .expect("valid W3C trace context must propagate");
    assert!(!format!("{parsed:?}").contains("4bf92f3577b34da6a3ce929d0e0e4736"));
    let oversized = "x".repeat(513);
    for (parent, state) in [
        (None, None),
        (Some("00-short"), None),
        (
            Some("00-00000000000000000000000000000000-00f067aa0ba902b7-01"),
            None,
        ),
        (
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01"),
            None,
        ),
        (
            Some("00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01"),
            None,
        ),
        (
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra"),
            None,
        ),
    ] {
        assert!(validate_trace_context(parent, state).is_none());
    }
    assert!(validate_trace_context(None, Some("vendor=opaque")).is_none());
    // W3C §3.3: 不正な tracestate だけを破棄し、有効な親を保持する。
    // https://www.w3.org/TR/trace-context/#tracestate-header
    for state in ["vendor=bad\nlog-injection", oversized.as_str()] {
        let parsed = validate_trace_context(Some(valid), Some(state)).unwrap();
        assert_eq!(parsed.traceparent(), valid);
        assert_eq!(parsed.tracestate(), None);
        assert!(!format!("{parsed:?}").contains("log-injection"));
    }
}

async fn execute_as_role(pool: &PgPool, role: &str, statement: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let assume = format!("SET LOCAL ROLE {role}");
    sqlx::query(sqlx::AssertSqlSafe(assume.as_str()))
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query(sqlx::AssertSqlSafe(statement))
        .execute(&mut *tx)
        .await
        .map(|_| ());
    tx.rollback().await?;
    result
}

#[tokio::test]
async fn delivery_role_cannot_mutate_document_or_audit() {
    let (_guard, pool, _url) = postgres_fixture::postgres("p6_delivery_role").await;
    migrate(&pool).await.unwrap();
    let role = format!("p6_worker_{}", Uuid::now_v7().simple());
    let create = format!("CREATE ROLE {role} NOLOGIN");
    sqlx::query(sqlx::AssertSqlSafe(create.as_str()))
        .execute(&pool)
        .await
        .unwrap();
    let template = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/sql/least_privilege.sql"
    ))
    .expect("P6 least-privilege grant template must exist");
    let grants = template.replace("{{DELIVERY_ROLE}}", &role);
    sqlx::raw_sql(sqlx::AssertSqlSafe(grants.as_str()))
        .execute(&pool)
        .await
        .unwrap();

    // 方針行の FOR SHARE に必要な UPDATE 権限はキーだけに限定する。
    // 方針値そのものは更新できない。
    execute_as_role(
        &pool,
        &role,
        "SELECT policy_id FROM outbox_delivery_policy WHERE policy_id=1 FOR SHARE",
    )
    .await
    .expect("delivery role must be able to lock policy for a claim");
    for column in [
        "revision",
        "max_attempts",
        "lease_min_ms",
        "lease_max_ms",
        "backoff_min_ms",
        "backoff_max_ms",
    ] {
        let statement =
            format!("UPDATE outbox_delivery_policy SET {column}={column} WHERE policy_id=1");
        let error = execute_as_role(&pool, &role, &statement).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
    }
    for (statement, code) in [
        (
            "UPDATE outbox_delivery_policy SET policy_id=2 WHERE policy_id=1",
            "23514",
        ),
        (
            "UPDATE outbox_delivery_policy SET policy_id=NULL WHERE policy_id=1",
            "23502",
        ),
        (
            "DELETE FROM outbox_delivery_policy WHERE policy_id=1",
            "42501",
        ),
        (
            "INSERT INTO outbox_delivery_policy (policy_id,revision,max_attempts,lease_min_ms,lease_max_ms,backoff_min_ms,backoff_max_ms) VALUES (2,1,8,1000,120000,1000,300000)",
            "42501",
        ),
    ] {
        let error = execute_as_role(&pool, &role, statement).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some(code),
            "{statement}"
        );
    }

    execute_as_role(
        &pool,
        &role,
        "UPDATE outbox_delivery_policy SET policy_id=1 WHERE policy_id=1",
    )
    .await
    .expect("singleton no-op must be allowed");
    for statement in [
        "TRUNCATE outbox_delivery_policy",
        "TRUNCATE outbox_events",
        "DELETE FROM outbox_events WHERE false",
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at) VALUES (gen_random_uuid(),'test','test',gen_random_uuid(),'{}',now(),now())",
        "SELECT data FROM audit_outbox_events LIMIT 0",
        "SELECT metadata FROM documents LIMIT 0",
    ] {
        let error = execute_as_role(&pool, &role, statement).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501"),
            "{statement}"
        );
    }
    for (table, column) in [
        ("outbox_events", "payload"),
        ("outbox_events", "event_type"),
        ("outbox_events", "event_id"),
        ("outbox_events", "aggregate_id"),
        ("outbox_events", "aggregate_type"),
        ("outbox_events", "occurred_at"),
        ("outbox_events", "traceparent"),
        ("outbox_events", "tracestate"),
        ("documents", "metadata"),
        ("audit_outbox_events", "delivered_at"),
    ] {
        let statement = format!("UPDATE {table} SET {column}={column} WHERE false");
        let error = execute_as_role(&pool, &role, &statement).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
    }
    for column in [
        "attempt_count",
        "attempt_limit",
        "available_at",
        "lease_token",
        "lease_owner",
        "lease_expires_at",
        "last_attempt_at",
        "delivered_at",
        "dead_lettered_at",
        "last_error_code",
    ] {
        let has: bool =
            sqlx::query_scalar("SELECT has_column_privilege($1,'outbox_events',$2,'UPDATE')")
                .bind(&role)
                .bind(column)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(has, "delivery role lacks UPDATE({column})");
        let statement = format!("UPDATE outbox_events SET {column}={column} WHERE false");
        execute_as_role(&pool, &role, &statement).await.unwrap();
    }
    let role_for_pool = role.clone();
    let role_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_connect(move |connection, _| {
            let statement = format!("SET ROLE {role_for_pool}");
            Box::pin(async move {
                sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let store = PostgresOutboxStore::new(role_pool.clone(), DeliveryPolicy::default());
    store.verify_policy().await.unwrap();
    assert_eq!(store.queue_snapshot().await.unwrap().unwrap().pending, 0);
    assert!(
        store
            .claim(Uuid::now_v7(), 1, std::time::Duration::from_secs(1))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.reap_exhausted(1).await.unwrap(), 0);
    role_pool.close().await;

    let policy = sqlx::query(
        "SELECT revision,max_attempts,lease_min_ms,lease_max_ms,backoff_min_ms,backoff_max_ms \
         FROM outbox_delivery_policy WHERE policy_id=1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(policy.get::<i64, _>("revision"), 1);
    assert_eq!(policy.get::<i32, _>("max_attempts"), 8);
    assert_eq!(policy.get::<i64, _>("lease_min_ms"), 1000);
    assert_eq!(policy.get::<i64, _>("lease_max_ms"), 120000);
    assert_eq!(policy.get::<i64, _>("backoff_min_ms"), 1000);
    assert_eq!(policy.get::<i64, _>("backoff_max_ms"), 300000);

    let cleanup = format!("DROP OWNED BY {role}; DROP ROLE {role}");
    sqlx::raw_sql(sqlx::AssertSqlSafe(cleanup.as_str()))
        .execute(&pool)
        .await
        .unwrap();
}
