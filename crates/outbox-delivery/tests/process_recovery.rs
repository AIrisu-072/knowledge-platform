//! P6-G07: コミット済み処理権取得後の異常終了と、独立プロセスによる回復。

#[path = "support/postgres.rs"]
mod postgres;

use document_repository_postgres::migrate;
use outbox_delivery::{DeliveryPolicy, OutboxStore, postgres::PostgresOutboxStore};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::HashSet,
    future::Future,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const PHASE_TIMEOUT: Duration = Duration::from_secs(15);
const CHILD_LIFETIME: Duration = Duration::from_secs(90);
const OUTPUT_LIMIT: u64 = 8 * 1024;
const READY: &str = "P6_G07_READY";

#[derive(Debug, PartialEq, Eq)]
enum WorkerMode {
    Claim,
    Reap,
}

#[derive(Debug, PartialEq, Eq)]
struct WorkerRequest {
    mode: WorkerMode,
    owner: Uuid,
    limit: u32,
}

impl WorkerRequest {
    fn parse(mode: &str, owner: &str, limit: &str) -> Result<Self, &'static str> {
        let mode = match mode {
            "claim" => WorkerMode::Claim,
            "reap" => WorkerMode::Reap,
            _ => return Err("unknown child mode"),
        };
        let owner = Uuid::parse_str(owner).map_err(|_| "invalid child owner")?;
        let limit = limit.parse::<u32>().map_err(|_| "invalid child limit")?;
        if owner.is_nil() || !(1..=2).contains(&limit) || (mode == WorkerMode::Reap && limit != 1) {
            return Err("child request outside test bounds");
        }
        Ok(Self { mode, owner, limit })
    }
}

fn reaper_count(line: &str) -> Result<Option<u64>, &'static str> {
    match line.strip_prefix("P6_G07_REAP=") {
        None => Ok(None),
        Some("0") => Ok(Some(0)),
        Some("1") => Ok(Some(1)),
        Some(_) => Err("invalid reaper count"),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ChildMessage {
    Ready,
    Reaped(u64),
    Closed,
}

struct Worker {
    child: Child,
    messages: Receiver<Result<ChildMessage, &'static str>>,
}

impl Worker {
    fn spawn(url: &str, mode: &str, owner: Uuid, limit: u32) -> Self {
        WorkerRequest::parse(mode, &owner.to_string(), &limit.to_string()).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "child_worker_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("P6_G07_DATABASE_URL", url)
            .env("P6_G07_MODE", mode)
            .env("P6_G07_OWNER", owner.to_string())
            .env("P6_G07_LIMIT", limit.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn current test executable");
        eprintln!(
            "G07 child started: pid={}, mode={mode}, owner={owner}",
            child.id()
        );
        let stdout = child.stdout.take().unwrap();
        let (sender, messages) = mpsc::channel();
        let worker = Self { child, messages };
        thread::spawn(move || {
            // libtest の通常出力を許し、子の出力全体には上限を置く。
            let mut reader = BufReader::new(stdout.take(OUTPUT_LIMIT + 1));
            let mut total = 0;
            loop {
                let mut line = String::new();
                let message = match reader.read_line(&mut line) {
                    Ok(0) => Ok(ChildMessage::Closed),
                    Ok(count) => {
                        total += count as u64;
                        if total > OUTPUT_LIMIT {
                            Err("child output exceeded limit")
                        } else if line.trim_end() == READY {
                            Ok(ChildMessage::Ready)
                        } else {
                            match reaper_count(line.trim_end()) {
                                Ok(Some(count)) => Ok(ChildMessage::Reaped(count)),
                                Ok(None) => continue,
                                Err(error) => Err(error),
                            }
                        }
                    }
                    Err(_) => Err("child output read failed"),
                };
                let terminal = message.is_err() || message == Ok(ChildMessage::Closed);
                if sender.send(message).is_err() || terminal {
                    break;
                }
            }
        });
        worker
    }

    async fn message(&mut self) -> ChildMessage {
        within(PHASE_TIMEOUT, async {
            loop {
                match self.messages.try_recv() {
                    Ok(message) => return message.expect("valid bounded child output"),
                    Err(mpsc::TryRecvError::Empty) => {
                        tokio::time::sleep(Duration::from_millis(10)).await
                    }
                    Err(mpsc::TryRecvError::Disconnected) => panic!("child output disconnected"),
                }
            }
        })
        .await
    }

    fn release(&mut self) {
        self.child
            .stdin
            .take()
            .unwrap()
            .write_all(&[1])
            .expect("release child barrier");
    }

    fn assert_alive(&mut self) {
        assert!(
            self.child.try_wait().unwrap().is_none(),
            "child exited before observation"
        );
    }

    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait()? {
                eprintln!("G07 child reaped: pid={}, status={status}", self.child.id());
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "child reap timeout",
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn kill_and_reap(&mut self) {
        self.assert_alive();
        // Child::kill は Unix で所有する子だけに SIGKILL を送る。
        self.child.kill().expect("kill owned child");
        let status = self.wait().expect("reap killed child");
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(status.signal(), Some(9));
        }
        #[cfg(not(unix))]
        assert!(!status.success());
    }

    async fn reaped_count(&mut self) -> u64 {
        let ChildMessage::Reaped(count) = self.message().await else {
            panic!("reaper must report one result");
        };
        assert_eq!(self.message().await, ChildMessage::Closed);
        assert!(self.wait().expect("reap finished child").success());
        count
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // 失敗・取消時にも、このガードが生成した子を停止して回収する。
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            if self.wait().is_err() {
                eprintln!("G07 owned child cleanup failed: pid={}", self.child.id());
            }
        }
    }
}

async fn within<T>(duration: Duration, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(duration, future)
        .await
        .expect("G07 phase deadline")
}

async fn db<T>(future: impl Future<Output = T>) -> T {
    within(Duration::from_secs(5), future).await
}

fn store(pool: &PgPool) -> PostgresOutboxStore {
    PostgresOutboxStore::new(pool.clone(), DeliveryPolicy::default())
}

async fn release_together(workers: &mut [Worker]) {
    for worker in workers.iter_mut() {
        assert_eq!(worker.message().await, ChildMessage::Ready);
        worker.assert_alive();
    }
    for worker in workers {
        worker.release();
    }
}

#[tokio::test]
#[ignore = "独立した子プロセスからだけ実行する"]
async fn child_worker_fixture() {
    // stdin の同期待ちを含む子全体を、独立タイマーで制限する。
    thread::spawn(|| {
        thread::sleep(CHILD_LIFETIME);
        eprintln!("G07 child lifetime exceeded");
        std::process::exit(124);
    });
    let request = WorkerRequest::parse(
        &std::env::var("P6_G07_MODE").unwrap(),
        &std::env::var("P6_G07_OWNER").unwrap(),
        &std::env::var("P6_G07_LIMIT").unwrap(),
    )
    .expect("valid child request");
    let pool = db(PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&std::env::var("P6_G07_DATABASE_URL").unwrap()))
    .await
    .unwrap();
    let store = store(&pool);
    db(store.verify_policy()).await.unwrap();
    println!("\n{READY}");
    std::io::stdout().flush().unwrap();
    let mut release = [0];
    std::io::stdin()
        .read_exact(&mut release)
        .expect("parent barrier");
    assert_eq!(release, [1]);
    match request.mode {
        WorkerMode::Claim => {
            let claims = db(store.claim(request.owner, request.limit, Duration::from_secs(120)))
                .await
                .unwrap();
            assert_eq!(claims.len(), request.limit as usize);
            // 親が別プールからコミットを確認して SIGKILL するまで保持する。
            std::future::pending::<()>().await;
        }
        WorkerMode::Reap => {
            let count = db(store.reap_exhausted(request.limit)).await.unwrap();
            println!("\nP6_G07_REAP={count}");
            std::io::stdout().flush().unwrap();
        }
    }
    db(pool.close()).await;
}

async fn fixture(name: &str) -> (postgres::DatabaseGuard, PgPool, String) {
    let fixture = within(Duration::from_secs(60), postgres::postgres(name)).await;
    db(migrate(&fixture.1)).await.unwrap();
    db(store(&fixture.1).verify_policy()).await.unwrap();
    fixture
}

async fn insert_event(pool: &PgPool, id: Uuid, attempts: i32, limit: Option<i32>) {
    let result = db(sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at,attempt_count,attempt_limit) \
         VALUES ($1,'DocumentRegistered','Document',$2,$3,'2000-01-01T00:00:00Z','2000-01-01T00:00:00Z',$4,$5)")
        .bind(id).bind(Uuid::from_u128(50))
        .bind(json!({"preserve": [1, null], "event": id.to_string()}))
        .bind(attempts).bind(limit).execute(pool)).await.unwrap();
    assert_eq!(result.rows_affected(), 1);
}

async fn wait_for_claimed(pool: &PgPool, workers: &mut [Worker], owners: &[Uuid], count: i64) {
    within(PHASE_TIMEOUT, async {
        loop {
            for worker in workers.iter_mut() {
                worker.assert_alive();
            }
            let (active, observed_at): (i64, String) = db(sqlx::query_as(
                "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t) \
                 SELECT (SELECT count(*) FROM outbox_events CROSS JOIN tick \
                 WHERE lease_owner=ANY($1) AND lease_token IS NOT NULL AND lease_expires_at>tick.t \
                   AND delivered_at IS NULL AND dead_lettered_at IS NULL), tick.t::text FROM tick",
            )
            .bind(owners)
            .fetch_one(pool))
            .await
            .unwrap();
            for worker in workers.iter_mut() {
                worker.assert_alive();
            }
            if active == count {
                eprintln!(
                    "G07 committed live claims: count={active}, database_clock={observed_at}"
                );
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
}

async fn row(pool: &PgPool, id: Uuid) -> Value {
    db(
        sqlx::query_scalar("SELECT to_jsonb(o) FROM outbox_events o WHERE event_id=$1")
            .bind(id)
            .fetch_one(pool),
    )
    .await
    .unwrap()
}

async fn snapshot(pool: &PgPool, ids: &[Uuid]) -> Vec<Value> {
    db(sqlx::query_scalar(
        "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t) \
         SELECT to_jsonb(o) || jsonb_build_object('lease_live',o.lease_expires_at>tick.t) \
         FROM outbox_events o CROSS JOIN tick WHERE event_id=ANY($1) ORDER BY event_id",
    )
    .bind(ids)
    .fetch_all(pool))
    .await
    .unwrap()
}

const EVENT_COLUMNS: &[&str] = &[
    "event_id",
    "event_type",
    "aggregate_type",
    "aggregate_id",
    "payload",
    "occurred_at",
    "available_at",
];

fn unchanged(before: &Value, after: &Value, columns: &[&str]) {
    for column in columns {
        assert_eq!(before[*column], after[*column], "preserved column {column}");
    }
}

#[tokio::test]
async fn last_claim_kill9_restart_reaps_unknown_once() {
    within(Duration::from_secs(120), async {
        let (_database, pool, url) = fixture("outbox_process_last").await;
        let id = Uuid::from_u128(1);
        insert_event(&pool, id, 7, Some(8)).await;
        let original = row(&pool, id).await;
        let owner = Uuid::from_u128(500);
        let mut workers = [Worker::spawn(&url, "claim", owner, 1)];
        release_together(&mut workers).await;
        wait_for_claimed(&pool, &mut workers, &[owner], 1).await;
        let claimed = row(&pool, id).await;
        assert_eq!(claimed["attempt_count"], json!(8));
        assert_eq!(claimed["attempt_limit"], json!(8));
        unchanged(&original, &claimed, EVENT_COLUMNS);
        workers[0].kill_and_reap();
        // FORCED_BY_TEST_SQL: 実時間の経過ではなく試験用 SQL で期限切れにする。
        let affected = db(sqlx::query(
            "UPDATE outbox_events SET lease_expires_at=clock_timestamp()-interval '1 second' \
             WHERE event_id=$1 AND lease_owner=$2",
        )
        .bind(id)
        .bind(owner)
        .execute(&pool))
        .await
        .unwrap()
        .rows_affected();
        assert_eq!(affected, 1);
        eprintln!("G07 FORCED_BY_TEST_SQL: affected_rows={affected}");
        let mut reapers = [
            Worker::spawn(&url, "reap", Uuid::from_u128(600), 1),
            Worker::spawn(&url, "reap", Uuid::from_u128(601), 1),
        ];
        release_together(&mut reapers).await;
        let mut counts = [
            reapers[0].reaped_count().await,
            reapers[1].reaped_count().await,
        ];
        counts.sort_unstable();
        assert_eq!(counts, [0, 1]);
        eprintln!("G07 competing reaper counts: {counts:?}");
        let after = row(&pool, id).await;
        unchanged(&claimed, &after, EVENT_COLUMNS);
        unchanged(
            &claimed,
            &after,
            &["attempt_count", "attempt_limit", "last_attempt_at"],
        );
        assert_eq!(after["payload"], original["payload"]);
        assert_eq!(after["last_error_code"], json!("delivery_unknown_at_limit"));
        assert!(!after["dead_lettered_at"].is_null());
        for column in [
            "delivered_at",
            "lease_token",
            "lease_owner",
            "lease_expires_at",
        ] {
            assert!(after[column].is_null(), "terminal {column}");
        }
        let store = store(&pool);
        assert_eq!(db(store.reap_exhausted(1)).await.unwrap(), 0);
        assert!(
            db(store.claim(Uuid::from_u128(700), 1, Duration::from_secs(1)))
                .await
                .unwrap()
                .is_empty()
        );
        db(pool.close()).await;
    })
    .await;
}

#[tokio::test]
async fn four_processes_disjoint_claims_and_recover_expired() {
    within(Duration::from_secs(120), async {
        let (_database, pool, url) = fixture("outbox_process_four").await;
        let ids = (100_u128..108).map(Uuid::from_u128).collect::<Vec<_>>();
        for id in &ids {
            insert_event(&pool, *id, 0, None).await;
        }
        let owners = (1_000_u128..1_004).map(Uuid::from_u128).collect::<Vec<_>>();
        let mut workers = owners
            .iter()
            .map(|owner| Worker::spawn(&url, "claim", *owner, 2))
            .collect::<Vec<_>>();
        release_together(&mut workers).await;
        wait_for_claimed(&pool, &mut workers, &owners, 8).await;
        let rows = snapshot(&pool, &ids).await;
        for worker in &mut workers {
            worker.assert_alive();
        }
        assert_eq!(rows.len(), 8);
        assert!(
            rows.iter()
                .all(|row| row["lease_live"] == json!(true) && row["attempt_count"] == json!(1))
        );
        let tokens = rows
            .iter()
            .map(|row| Uuid::parse_str(row["lease_token"].as_str().unwrap()).unwrap())
            .collect::<HashSet<_>>();
        assert_eq!(tokens.len(), 8);
        for owner in &owners {
            assert_eq!(
                rows.iter()
                    .filter(|row| row["lease_owner"] == json!(owner.to_string()))
                    .count(),
                2
            );
        }
        workers[0].kill_and_reap();
        // FORCED_BY_TEST_SQL: 強制終了した所有者の二行だけを期限切れにする。
        let affected = db(sqlx::query(
            "UPDATE outbox_events SET lease_expires_at=clock_timestamp()-interval '1 second' \
             WHERE lease_owner=$1 AND event_id=ANY($2)",
        )
        .bind(owners[0])
        .bind(&ids)
        .execute(&pool))
        .await
        .unwrap()
        .rows_affected();
        assert_eq!(affected, 2);
        eprintln!("G07 FORCED_BY_TEST_SQL: affected_rows={affected}");
        let recovered_owner = Uuid::from_u128(2_000);
        let mut replacement = [Worker::spawn(&url, "claim", recovered_owner, 2)];
        release_together(&mut replacement).await;
        wait_for_claimed(&pool, &mut replacement, &[recovered_owner], 2).await;
        let after = snapshot(&pool, &ids).await;
        for worker in workers.iter_mut().skip(1).chain(replacement.iter_mut()) {
            worker.assert_alive();
        }
        assert_eq!(after.len(), 8);
        let (mut recovered, mut untouched) = (0, 0);
        for current in &after {
            let previous = rows
                .iter()
                .find(|previous| previous["event_id"] == current["event_id"])
                .unwrap();
            unchanged(previous, current, EVENT_COLUMNS);
            unchanged(previous, current, &["attempt_limit"]);
            assert_eq!(current["lease_live"], json!(true));
            if previous["lease_owner"] == json!(owners[0].to_string()) {
                assert_eq!(current["lease_owner"], json!(recovered_owner.to_string()));
                assert_eq!(current["attempt_count"], json!(2));
                assert!(!current["lease_token"].is_null());
                assert_ne!(current["lease_token"], previous["lease_token"]);
                recovered += 1;
            } else {
                unchanged(
                    previous,
                    current,
                    &[
                        "lease_owner",
                        "lease_token",
                        "attempt_count",
                        "lease_expires_at",
                        "last_attempt_at",
                    ],
                );
                untouched += 1;
            }
        }
        assert_eq!((recovered, untouched), (2, 6));
        for worker in workers.iter_mut().skip(1).chain(replacement.iter_mut()) {
            worker.kill_and_reap();
        }
        db(pool.close()).await;
    })
    .await;
}
#[test]
fn g07_ci_child_request_accepts_only_the_two_bounded_modes() {
    let owner = Uuid::from_u128(500);
    for (text, mode, limit) in [
        ("claim", WorkerMode::Claim, 2),
        ("reap", WorkerMode::Reap, 1),
    ] {
        assert_eq!(
            WorkerRequest::parse(text, &owner.to_string(), &limit.to_string()),
            Ok(WorkerRequest { mode, owner, limit })
        );
    }
    for (mode, owner, limit) in [
        ("unknown", owner.to_string(), "1"),
        ("claim", Uuid::nil().to_string(), "1"),
        ("claim", "invalid".to_owned(), "1"),
        ("claim", owner.to_string(), "0"),
        ("claim", owner.to_string(), "3"),
        ("reap", owner.to_string(), "2"),
    ] {
        assert!(WorkerRequest::parse(mode, &owner, limit).is_err());
    }
}

#[test]
fn g07_ci_reaper_result_requires_a_bounded_count() {
    assert_eq!(reaper_count("running 1 test"), Ok(None));
    assert_eq!(reaper_count("P6_G07_REAP=0"), Ok(Some(0)));
    assert_eq!(reaper_count("P6_G07_REAP=1"), Ok(Some(1)));
    for invalid in [
        "P6_G07_REAP=2",
        "P6_G07_REAP=",
        "P6_G07_REAP=-1",
        "P6_G07_REAP=1 extra",
    ] {
        assert!(reaper_count(invalid).is_err());
    }
}
