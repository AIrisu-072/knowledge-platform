//! P6-G08: bounded telemetry/trace and a real PostgreSQL least-privilege role.

#[path = "support/postgres.rs"]
mod postgres_fixture;

use std::{
    fs,
    sync::{Arc, Mutex},
};

use document_repository_postgres::migrate;
use outbox_delivery::{
    DeliveryConfig, DeliveryDecision, DeliveryEnvelope, DeliveryPolicy,
    observe::{
        DeliveryMetric, DeliveryMetricKind, DeliveryObserver, DeliveryRoute, validate_trace_context,
    },
    postgres::PostgresOutboxStore,
    runner::{DeliveryContext, DeliveryHandler, DeliveryRunner, NoopAdmission, NoopPermit},
};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Default)]
struct RecordingObserver(Mutex<Vec<DeliveryMetric>>);

impl DeliveryObserver for RecordingObserver {
    fn record(&self, metric: DeliveryMetric) {
        self.0.lock().unwrap().push(metric);
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
    let metrics = observer.0.lock().unwrap();
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
        (Some(valid), Some("vendor=bad\nlog-injection")),
        (Some(valid), Some(oversized.as_str())),
    ] {
        assert!(validate_trace_context(parent, state).is_none());
    }
    assert!(validate_trace_context(None, Some("vendor=opaque")).is_none());
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
    sqlx::raw_sql(&grants).execute(&pool).await.unwrap();

    // FOR SHARE on the singleton policy row needs a narrowly scoped UPDATE
    // privilege; the role must still be unable to alter the policy values.
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
        assert!(execute_as_role(&pool, &role, &statement).await.is_err());
    }
    assert!(
        execute_as_role(
            &pool,
            &role,
            "UPDATE outbox_delivery_policy SET policy_id=2 WHERE policy_id=1"
        )
        .await
        .is_err()
    );
    assert!(
        execute_as_role(
            &pool,
            &role,
            "UPDATE outbox_delivery_policy SET policy_id=NULL WHERE policy_id=1"
        )
        .await
        .is_err()
    );
    assert!(
        execute_as_role(
            &pool,
            &role,
            "DELETE FROM outbox_delivery_policy WHERE policy_id=1"
        )
        .await
        .is_err()
    );
    assert!(execute_as_role(&pool, &role, "INSERT INTO outbox_delivery_policy (policy_id,revision,max_attempts,lease_min_ms,lease_max_ms,backoff_min_ms,backoff_max_ms) VALUES (2,1,8,1000,120000,1000,300000)").await.is_err());

    for (table, column) in [
        ("outbox_events", "payload"),
        ("outbox_events", "event_type"),
        ("documents", "metadata"),
        ("audit_outbox_events", "delivered_at"),
    ] {
        let statement = format!("UPDATE {table} SET {column}={column} WHERE false");
        assert!(execute_as_role(&pool, &role, &statement).await.is_err());
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
    }
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
    sqlx::raw_sql(&cleanup).execute(&pool).await.unwrap();
}
