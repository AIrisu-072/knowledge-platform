//! P6-I04: the Search worker composition refuses a mismatched delivery
//! policy, its Search role cannot acknowledge outbox rows or touch Audit,
//! and a shutdown signal stops claiming and drains within the budget.

#[path = "../../search-source-document/tests/support/body.rs"]
mod body_support;
#[path = "../../search-source-document/tests/support/document_discovery.rs"]
mod discovery_support;
#[path = "support/durable.rs"]
mod durable;
#[path = "support/registration.rs"]
mod registration;
mod support;

use std::time::Duration;

use durable::*;
use outbox_delivery::{DeliveryConfig, DeliveryError, DeliveryPolicy};
use search_application::indexing_service::IndexingOutcome;
use search_runtime::full_guard::FullGuardTtl;
use search_runtime::worker::{
    SearchWorkerConfig, SearchWorkerRunner, WorkerError, compose, compose_rebuild,
};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

fn config(durable: &Durable, policy: DeliveryPolicy) -> SearchWorkerConfig {
    let index = discovery_support::index_config(durable.source_id);
    SearchWorkerConfig {
        registration: durable.registration.clone(),
        activation: durable.activation,
        source: durable.source(),
        lens: index.lens,
        projection_schema_version: index.projection_schema_version,
        analyzer_version: index.analyzer_version,
        semantic_registry: index.semantic_registry,
        lexical_root: durable.lexical_root.clone(),
        delivery: DeliveryConfig {
            poll_interval: Duration::from_millis(50),
            drain_timeout: Duration::from_secs(30),
            ..DeliveryConfig::default()
        },
        policy,
        source_lease: Duration::from_secs(120),
        guard_ttl: FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
    }
}

fn worker(durable: &Durable, policy: DeliveryPolicy) -> Result<SearchWorkerRunner, WorkerError> {
    compose(
        durable.pool.clone(),
        durable.pool.clone(),
        &durable.ledger,
        durable.extractor(),
        config(durable, policy),
    )
}

/// A Source lease that cannot outlast three delivery heartbeats would expire
/// in every build longer than one heartbeat; the worker refuses to start.
#[tokio::test]
async fn worker_refuses_a_source_lease_shorter_than_three_renewals() {
    let durable = Durable::start().await;
    let mut short = config(&durable, DeliveryPolicy::default());
    short.source_lease = short.delivery.renew_interval * 3 - Duration::from_millis(1);
    let refused = compose(
        durable.pool.clone(),
        durable.pool.clone(),
        &durable.ledger,
        durable.extractor(),
        short,
    );
    assert!(matches!(refused, Err(WorkerError::InvalidConfig(_))));
}

#[tokio::test]
async fn worker_refuses_mismatched_policy_or_foreign_source() {
    let durable = Durable::start().await;
    let mismatched = worker(
        &durable,
        DeliveryPolicy {
            revision: 99,
            ..DeliveryPolicy::default()
        },
    )
    .unwrap();
    assert!(matches!(
        mismatched.run_cycle().await,
        Err(DeliveryError::PolicyMismatch)
    ));
    let mut foreign = config(&durable, DeliveryPolicy::default());
    foreign.source.source_id = registration::source(9_999);
    assert!(matches!(
        compose(
            durable.pool.clone(),
            durable.pool.clone(),
            &durable.ledger,
            durable.extractor(),
            foreign,
        ),
        Err(WorkerError::InvalidConfig(_))
    ));
}

async fn login(admin: &PgPool, group: &str) -> PgPool {
    for roles in [
        concat!(env!("CARGO_MANIFEST_DIR"), "/sql/roles.sql"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../search-graph/sql/roles.sql"),
    ] {
        let text = std::fs::read_to_string(roles).unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(text.as_str()))
            .execute(admin)
            .await
            .unwrap();
    }
    let options = admin.connect_options();
    let login = format!("p6_worker_{}", Uuid::new_v4().simple());
    for statement in [
        format!("CREATE ROLE {login} LOGIN PASSWORD 'p6-disposable-fixture'"),
        format!("GRANT {group} TO {login}"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(admin)
            .await
            .unwrap();
    }
    PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            (*options)
                .clone()
                .username(&login)
                .password("p6-disposable-fixture"),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn search_role_cannot_ack_or_touch_audit() {
    let durable = Durable::start().await;
    let search = login(&durable.pool, "search_coordinator").await;
    // Row locks for completion are allowed; delivery state and Audit are not.
    sqlx::query("SELECT event_id FROM outbox_events FOR UPDATE")
        .fetch_all(&search)
        .await
        .unwrap();
    for statement in [
        "UPDATE outbox_events SET delivered_at = now()",
        "UPDATE outbox_events SET dead_lettered_at = now()",
        "DELETE FROM outbox_events",
        "UPDATE audit_outbox_events SET delivered_at = now()",
        "DELETE FROM audit_outbox_events",
    ] {
        let error = sqlx::query(statement).execute(&search).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501"),
            "{statement}"
        );
    }
}

#[tokio::test]
async fn shutdown_stops_claiming_and_drains() {
    let durable = Durable::start().await;
    let runner = worker(&durable, DeliveryPolicy::default()).unwrap();
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let running = tokio::spawn(async move { runner.run_until_shutdown(stopped).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    stop.send(true).unwrap();
    let finished = tokio::time::timeout(Duration::from_secs(40), running)
        .await
        .expect("stops within the drain budget")
        .unwrap();
    // An operation abandoned at shutdown is reported unknown, never as success.
    assert!(
        matches!(finished, Ok(_) | Err(DeliveryError::StoreUnknown)),
        "{finished:?}"
    );
    // Nothing claims after shutdown.
    let event = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,payload, \
         occurred_at,available_at) VALUES ($1,'FolderMoved','Folder',$2,'{}',now(),now())",
    )
    .bind(event)
    .bind(Uuid::now_v7())
    .execute(&durable.pool)
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let claimed: Option<Uuid> =
        sqlx::query_scalar("SELECT lease_token FROM outbox_events WHERE event_id=$1")
            .bind(event)
            .fetch_one(&durable.pool)
            .await
            .unwrap();
    assert_eq!(claimed, None);
}

/// A2: the manual retry is a full rebuild outside the outbox. It has no
/// attempt limit: it can run again and again, and each run converges on the
/// current Document snapshot.
#[tokio::test]
async fn manual_rebuild_has_no_attempt_limit() {
    let durable = Durable::start().await;
    publish(&durable.pool, &durable.storage, "manual rebuild body").await;
    let rebuild = || {
        compose_rebuild(
            durable.pool.clone(),
            &durable.ledger,
            durable.extractor(),
            config(&durable, DeliveryPolicy::default()),
        )
        .unwrap()
    };
    let first = rebuild().rebuild().await.unwrap();
    let IndexingOutcome::Published(_) = first else {
        panic!("first rebuild publishes: {first:?}");
    };
    // More attempts than the automatic delivery limit, each one admitted.
    for _ in 0..=DeliveryPolicy::default().max_attempts {
        match rebuild().rebuild().await.unwrap() {
            IndexingOutcome::Published(_) | IndexingOutcome::Unchanged(_) => {}
            other => panic!("manual retry refused: {other:?}"),
        }
    }
}
