//! P6-G07: a real child process can die after a committed claim.

#[path = "support/postgres.rs"]
mod postgres_fixture;

use std::{
    collections::HashSet,
    io::{Read, Write},
    net::TcpListener,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use document_repository_postgres::migrate;
use outbox_delivery::{DeliveryPolicy, OutboxStore, postgres::PostgresOutboxStore};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn kill_and_wait(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn wait_with_output(mut self) -> Output {
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

fn child(url: &str, mode: &str, owner: Uuid, limit: u32, barrier: Option<&str>) -> ChildGuard {
    let executable = std::env::current_exe().unwrap();
    let mut command = Command::new(executable);
    command
        .args([
            "--exact",
            "child_worker_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("P6_CHILD_URL", url)
        .env("P6_CHILD_MODE", mode)
        .env("P6_CHILD_OWNER", owner.to_string())
        .env("P6_CHILD_LIMIT", limit.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(barrier) = barrier {
        command.env("P6_CHILD_BARRIER", barrier);
    }
    ChildGuard(Some(command.spawn().unwrap()))
}

async fn insert_event(pool: &PgPool, id: Uuid, attempts: i32, limit: Option<i32>) {
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at, \
          attempt_count,attempt_limit) \
         VALUES ($1,'DocumentRegistered','Document',$2,$3, \
                 '2000-01-01T00:00:00Z','2000-01-01T00:00:00Z',$4,$5)",
    )
    .bind(id)
    .bind(Uuid::from_u128(50))
    .bind(json!({"preserve": [1, null], "event": id.to_string()}))
    .bind(attempts)
    .bind(limit)
    .execute(pool)
    .await
    .unwrap();
}

async fn wait_for_claimed(pool: &PgPool, owners: &[Uuid], count: i64) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let active: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM outbox_events \
                 WHERE lease_owner = ANY($1) AND lease_token IS NOT NULL \
                   AND delivered_at IS NULL AND dead_lettered_at IS NULL",
            )
            .bind(owners)
            .fetch_one(pool)
            .await
            .unwrap();
            if active == count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("child did not commit expected claims");
}

async fn row(pool: &PgPool, id: Uuid) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(o) FROM outbox_events AS o WHERE event_id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn store(pool: &PgPool) -> PostgresOutboxStore {
    PostgresOutboxStore::new(pool.clone(), DeliveryPolicy::default())
}

// This ignored entry point is intentionally a separate executable process.
// G07's first RED must show that the child path is missing, then wire the
// production runner/real store in this test fixture without changing prod.
#[tokio::test]
#[ignore = "child process fixture"]
async fn child_worker_fixture() {
    let Ok(_) = std::env::var("P6_CHILD_MODE") else {
        return;
    };
    panic!("G07 child process path not wired");
}

fn release_barrier(listener: &TcpListener, count: usize) {
    listener.set_nonblocking(true).unwrap();
    let mut gates = Vec::new();
    for _ in 0..count {
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
                Err(error) => panic!("child did not reach barrier: {error}"),
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
}

#[tokio::test]
async fn last_claim_kill9_restart_reaps_unknown_once() {
    let (_guard, pool, url) = postgres_fixture::postgres("p6_crash_final").await;
    migrate(&pool).await.unwrap();
    let id = Uuid::from_u128(1);
    insert_event(&pool, id, 7, Some(8)).await;
    let original = row(&pool, id).await;

    let owner = Uuid::from_u128(500);
    let mut worker = child(&url, "claim", owner, 1, None);
    wait_for_claimed(&pool, &[owner], 1).await;
    let claimed = row(&pool, id).await;
    assert_eq!(claimed["attempt_count"], 8);
    assert_eq!(claimed["attempt_limit"], 8);
    worker.kill_and_wait(); // an actual OS-process death after claim COMMIT
    sqlx::query(
        "UPDATE outbox_events SET lease_expires_at=clock_timestamp()-interval '1 second' \
         WHERE event_id=$1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let reapers = (0..2)
        .map(|n| child(&url, "reap", Uuid::from_u128(600 + n), 1, Some(&address)))
        .collect::<Vec<_>>();
    release_barrier(&listener, 2);
    let mut counts = Vec::new();
    for reaper in reapers {
        let output = reaper.wait_with_output();
        assert!(
            output.status.success(),
            "reaper failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let count = stdout
            .split("P6_REAP_COUNT=")
            .nth(1)
            .and_then(|s| s.lines().next())
            .unwrap()
            .parse::<u64>()
            .unwrap();
        counts.push(count);
    }
    counts.sort_unstable();
    assert_eq!(counts, [0, 1]);
    let after = row(&pool, id).await;
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
        assert_eq!(after[key], claimed[key], "reaper changed {key}");
    }
    assert_eq!(after["payload"], original["payload"]);
    assert_eq!(after["last_error_code"], "delivery_unknown_at_limit");
    assert!(!after["dead_lettered_at"].is_null());
    assert!(after["delivered_at"].is_null());
    assert!(after["lease_token"].is_null());
    assert_eq!(store(&pool).reap_exhausted(1).await.unwrap(), 0);
    assert!(
        store(&pool)
            .claim(Uuid::from_u128(700), 1, Duration::from_secs(1))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn four_processes_disjoint_claims_and_recover_expired() {
    let (_guard, pool, url) = postgres_fixture::postgres("p6_crash_disjoint").await;
    migrate(&pool).await.unwrap();
    let ids = (100_u128..108).map(Uuid::from_u128).collect::<Vec<_>>();
    for id in &ids {
        insert_event(&pool, *id, 0, None).await;
    }
    let owners = (0_u128..4)
        .map(|n| Uuid::from_u128(1_000 + n))
        .collect::<Vec<_>>();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let mut workers = owners
        .iter()
        .map(|owner| child(&url, "claim", *owner, 2, Some(&address)))
        .collect::<Vec<_>>();
    release_barrier(&listener, 4);
    wait_for_claimed(&pool, &owners, 8).await;
    let rows = sqlx::query(
        "SELECT event_id,attempt_count,lease_owner,lease_token \
         FROM outbox_events WHERE event_id = ANY($1) ORDER BY event_id",
    )
    .bind(&ids)
    .fetch_all(&pool)
    .await
    .unwrap();
    let tokens = rows
        .iter()
        .map(|r| r.get::<Uuid, _>("lease_token"))
        .collect::<HashSet<_>>();
    assert_eq!(tokens.len(), 8, "live tokens must be disjoint");
    for owner in &owners {
        assert_eq!(
            rows.iter()
                .filter(|r| r.get::<Uuid, _>("lease_owner") == *owner)
                .count(),
            2
        );
    }
    workers[0].kill_and_wait();
    sqlx::query(
        "UPDATE outbox_events SET lease_expires_at=clock_timestamp()-interval '1 second' \
         WHERE lease_owner=$1",
    )
    .bind(owners[0])
    .execute(&pool)
    .await
    .unwrap();
    let recovered_owner = Uuid::from_u128(2_000);
    let mut replacement = child(&url, "claim", recovered_owner, 2, None);
    wait_for_claimed(&pool, &[recovered_owner], 2).await;
    let recovered = sqlx::query(
        "SELECT event_id,attempt_count,lease_token FROM outbox_events WHERE lease_owner=$1",
    )
    .bind(recovered_owner)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(recovered.len(), 2);
    for row in recovered {
        let id: Uuid = row.get("event_id");
        assert_eq!(row.get::<i32, _>("attempt_count"), 2);
        let previous = rows
            .iter()
            .find(|previous| previous.get::<Uuid, _>("event_id") == id)
            .unwrap();
        assert_eq!(previous.get::<Uuid, _>("lease_owner"), owners[0]);
        assert_ne!(
            row.get::<Uuid, _>("lease_token"),
            previous.get::<Uuid, _>("lease_token")
        );
    }
    let untouched: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events \
         WHERE lease_owner = ANY($1) AND attempt_count=1",
    )
    .bind(&owners[1..])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(untouched, 6);
    replacement.kill_and_wait();
    for worker in &mut workers {
        worker.kill_and_wait();
    }
}
