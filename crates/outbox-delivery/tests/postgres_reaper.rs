use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use document_repository_postgres::migrate;
use outbox_delivery::{
    DeliveryError, DeliveryPolicy, FenceResult, OutboxStore, postgres::PostgresOutboxStore,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

struct Fixture {
    _container: testcontainers::ContainerAsync<GenericImage>,
    url: String,
    pool: PgPool,
}

async fn fixture() -> Fixture {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "outbox_reaper_test")
        .start()
        .await
        .expect("disposable PostgreSQL should start");
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/outbox_reaper_test");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    Fixture {
        _container: container,
        url,
        pool,
    }
}

async fn insert_event(pool: &PgPool, event_id: Uuid, attempts: i32, limit: Option<i32>) {
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at, \
          attempt_count,attempt_limit) \
         VALUES ($1,'DocumentRegistered','Document',$2,$3, \
                 '2000-01-01T00:00:00Z','2000-01-01T00:00:00Z',$4,$5)",
    )
    .bind(event_id)
    .bind(Uuid::from_u128(50))
    .bind(json!({"preserve": [1, null], "event": event_id.to_string()}))
    .bind(attempts)
    .bind(limit)
    .execute(pool)
    .await
    .unwrap();
}

async fn event_row(pool: &PgPool, id: Uuid) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(o) FROM outbox_events AS o WHERE event_id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn store(pool: &PgPool) -> PostgresOutboxStore {
    PostgresOutboxStore::new(pool.clone(), DeliveryPolicy::default())
}

// A separate test executable is a separate process and opens its own pool.
// This catches a process-local-only duplicate guard.
#[tokio::test]
#[ignore = "child process fixture"]
async fn child_reaper_fixture() {
    let Ok(url) = std::env::var("G04_REAPER_URL") else {
        return;
    };
    let address = std::env::var("G04_REAPER_BARRIER").unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let mut gate = TcpStream::connect(address).unwrap();
    gate.write_all(&[1]).unwrap();
    let mut release = [0];
    gate.read_exact(&mut release).unwrap();
    assert_eq!(release, [1]);
    let count = store(&pool).reap_exhausted(1).await.unwrap();
    println!("G04_REAP_COUNT={count}");
}

#[tokio::test]
async fn crashed_final_claim_reaped_once_by_competing_workers() {
    // A missing reaper, duplicate terminal update, or attempt reset fails here.
    let f = fixture().await;
    let id = Uuid::from_u128(1);
    insert_event(&f.pool, id, 7, Some(8)).await;
    let claim = store(&f.pool)
        .claim(Uuid::from_u128(2), 1, Duration::from_secs(120))
        .await
        .unwrap();
    assert_eq!(claim.len(), 1);
    assert_eq!(claim[0].attempt, 8);
    let original = event_row(&f.pool, id).await;
    sqlx::query(
        "UPDATE outbox_events SET lease_expires_at=clock_timestamp()-interval '1 second' \
         WHERE event_id=$1",
    )
    .bind(id)
    .execute(&f.pool)
    .await
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let barrier = listener.local_addr().unwrap().to_string();
    let executable = std::env::current_exe().unwrap();
    let mut children = Vec::new();
    for _ in 0..2 {
        children.push(
            Command::new(&executable)
                .args([
                    "--exact",
                    "child_reaper_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("G04_REAPER_URL", &f.url)
                .env("G04_REAPER_BARRIER", &barrier)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let mut gates = Vec::new();
    for _ in 0..2 {
        let started = Instant::now();
        let (mut gate, _) = loop {
            match listener.accept() {
                Ok(accepted) => break accepted,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && started.elapsed() < Duration::from_secs(15) =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    for child in &mut children {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    panic!("child reaper did not reach barrier: {error}");
                }
            }
        };
        let mut ready = [0];
        gate.read_exact(&mut ready).unwrap();
        assert_eq!(ready, [1]);
        gates.push(gate);
    }
    for mut gate in gates {
        gate.write_all(&[1]).unwrap();
    }
    let mut counts = Vec::new();
    for child in children {
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            output.status.success(),
            "child failed: {stdout} {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let count = stdout
            .split("G04_REAP_COUNT=")
            .nth(1)
            .and_then(|s| s.lines().next())
            .unwrap()
            .parse::<u64>()
            .unwrap();
        counts.push(count);
    }
    counts.sort_unstable();
    assert_eq!(counts, [0, 1]);

    let after = event_row(&f.pool, id).await;
    for key in [
        "event_id",
        "event_type",
        "aggregate_type",
        "aggregate_id",
        "payload",
        "occurred_at",
        "available_at",
        "attempt_count",
        "attempt_limit",
        "last_attempt_at",
    ] {
        assert_eq!(after[key], original[key], "reaper changed {key}");
    }
    let row = sqlx::query(
        "SELECT payload,attempt_count,attempt_limit,last_attempt_at,available_at, \
         delivered_at,dead_lettered_at,last_error_code,lease_token,lease_owner,lease_expires_at \
         FROM outbox_events WHERE event_id=$1",
    )
    .bind(id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(row.get::<Value, _>("payload"), original["payload"]);
    assert_eq!(row.get::<i32, _>("attempt_count"), 8);
    assert_eq!(row.get::<Option<i32>, _>("attempt_limit"), Some(8));
    assert_eq!(
        row.get::<OffsetDateTime, _>("available_at")
            .unix_timestamp(),
        946_684_800
    );
    assert!(
        row.get::<Option<OffsetDateTime>, _>("delivered_at")
            .is_none()
    );
    assert!(
        row.get::<Option<OffsetDateTime>, _>("dead_lettered_at")
            .is_some()
    );
    assert_eq!(
        row.get::<Option<String>, _>("last_error_code").as_deref(),
        Some("delivery_unknown_at_limit")
    );
    assert!(row.get::<Option<Uuid>, _>("lease_token").is_none());
    assert!(row.get::<Option<Uuid>, _>("lease_owner").is_none());
    assert!(
        row.get::<Option<OffsetDateTime>, _>("lease_expires_at")
            .is_none()
    );
    assert_eq!(store(&f.pool).reap_exhausted(1).await.unwrap(), 0);
    assert!(
        store(&f.pool)
            .claim(Uuid::from_u128(3), 1, Duration::from_secs(1))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn policy_mismatch_refuses_reap() {
    // Dropping any one of the six policy comparisons makes a changed policy reap.
    let f = fixture().await;
    let id = Uuid::from_u128(10);
    insert_event(&f.pool, id, 8, Some(8)).await;
    let before = event_row(&f.pool, id).await;
    for change in [
        "UPDATE outbox_delivery_policy SET revision=2",
        "UPDATE outbox_delivery_policy SET max_attempts=9",
        "UPDATE outbox_delivery_policy SET lease_min_ms=2000",
        "UPDATE outbox_delivery_policy SET lease_max_ms=119000",
        "UPDATE outbox_delivery_policy SET backoff_min_ms=2000",
        "UPDATE outbox_delivery_policy SET backoff_max_ms=299000",
    ] {
        sqlx::query(change).execute(&f.pool).await.unwrap();
        assert_eq!(
            store(&f.pool).reap_exhausted(1).await,
            Err(DeliveryError::PolicyMismatch)
        );
        assert_eq!(event_row(&f.pool, id).await, before);
        sqlx::query(
            "UPDATE outbox_delivery_policy SET revision=1,max_attempts=8, \
             lease_min_ms=1000,lease_max_ms=120000,backoff_min_ms=1000,backoff_max_ms=300000",
        )
        .execute(&f.pool)
        .await
        .unwrap();
    }

    let legacy = Uuid::from_u128(11);
    insert_event(&f.pool, legacy, 8, None).await;
    assert_eq!(
        store(&f.pool).reap_exhausted(1).await,
        Err(DeliveryError::LegacyExhausted {
            count: 1,
            first_ids: vec![legacy],
        })
    );
    assert_eq!(event_row(&f.pool, id).await, before);
    let legacy_row = event_row(&f.pool, legacy).await;
    assert!(legacy_row["dead_lettered_at"].is_null());
    assert!(legacy_row["attempt_limit"].is_null());
}

#[tokio::test]
async fn unexpired_final_attempt_is_not_reaped() {
    // Reaping any active lease, retry, delivered or dead row is observable.
    let f = fixture().await;
    let ids: Vec<_> = (20..=24).map(Uuid::from_u128).collect();
    insert_event(&f.pool, ids[0], 7, Some(8)).await;
    let final_claim = store(&f.pool)
        .claim(Uuid::from_u128(1), 1, Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(final_claim[0].attempt, 8);
    assert_eq!(
        store(&f.pool)
            .renew(ids[0], final_claim[0].lease_token, Duration::from_secs(120))
            .await,
        Ok(FenceResult::Updated)
    );
    insert_event(&f.pool, ids[1], 7, Some(8)).await;
    insert_event(&f.pool, ids[2], 7, Some(8)).await;
    sqlx::query(
        "UPDATE outbox_events SET lease_token=$2,lease_owner=$3, \
         lease_expires_at=clock_timestamp()-interval '1 second' WHERE event_id=$1",
    )
    .bind(ids[2])
    .bind(Uuid::from_u128(3))
    .bind(Uuid::from_u128(4))
    .execute(&f.pool)
    .await
    .unwrap();
    insert_event(&f.pool, ids[3], 8, Some(8)).await;
    sqlx::query("UPDATE outbox_events SET delivered_at=clock_timestamp() WHERE event_id=$1")
        .bind(ids[3])
        .execute(&f.pool)
        .await
        .unwrap();
    insert_event(&f.pool, ids[4], 8, Some(8)).await;
    sqlx::query("UPDATE outbox_events SET dead_lettered_at=clock_timestamp() WHERE event_id=$1")
        .bind(ids[4])
        .execute(&f.pool)
        .await
        .unwrap();
    let mut before = Vec::new();
    for &id in &ids {
        before.push(event_row(&f.pool, id).await);
    }
    assert_eq!(store(&f.pool).reap_exhausted(32).await.unwrap(), 0);
    for (id, snapshot) in ids.into_iter().zip(before) {
        assert_eq!(event_row(&f.pool, id).await, snapshot);
    }
}

#[tokio::test]
async fn reaper_bounds_batch_and_handles_unclaimed_exhausted() {
    // An unleased pinned final row must be recoverable; a 33rd row waits.
    let f = fixture().await;
    let ids: Vec<_> = (100..=133).map(Uuid::from_u128).collect();
    for &id in &ids {
        insert_event(&f.pool, id, 8, Some(8)).await;
    }
    let store = store(&f.pool);
    assert_eq!(
        store.reap_exhausted(0).await,
        Err(DeliveryError::InvalidConfig)
    );
    assert_eq!(
        store.reap_exhausted(33).await,
        Err(DeliveryError::InvalidConfig)
    );
    // The first candidate is locked elsewhere. The bounded reaper must skip
    // it and make progress on the next 32 without waiting for that lock.
    let mut held = f.pool.begin().await.unwrap();
    sqlx::query("SELECT event_id FROM outbox_events WHERE event_id=$1 FOR UPDATE")
        .bind(ids[0])
        .fetch_one(&mut *held)
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), store.reap_exhausted(32))
            .await
            .expect("SKIP LOCKED should not wait for the first event")
            .unwrap(),
        32
    );
    for (index, id) in ids.iter().copied().enumerate() {
        let row = event_row(&f.pool, id).await;
        assert_eq!(row["dead_lettered_at"].is_null(), index == 0 || index == 33);
        assert_eq!(row["attempt_count"], 8);
        assert_eq!(row["attempt_limit"], 8);
        let expected_id = id.to_string();
        assert_eq!(row["payload"]["event"].as_str(), Some(expected_id.as_str()));
    }
    held.rollback().await.unwrap();
    assert_eq!(store.reap_exhausted(32).await.unwrap(), 2);
    assert_eq!(store.reap_exhausted(32).await.unwrap(), 0);
    f.pool.close().await;
    assert_eq!(
        store.reap_exhausted(1).await,
        Err(DeliveryError::StoreUnknown)
    );
}
