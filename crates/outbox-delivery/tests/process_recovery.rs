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

// Pure G07 preparation only. None of these contracts reads ambient state,
// touches a socket/database/file, or spawns/signals/waits for a real process.
// The child entrypoint above intentionally remains unwired for later G07 RED.
use postgres_fixture::g07_owned::{
    DatabaseIdentity, DatabaseResolution, DdlOperation, DdlResponse, FixtureError, OwnedScopeFacts,
    PathFact, PathKind, ProcessIdentity, checked_drop_statement, classify_ddl_response,
    validate_owned_scope,
};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::PathBuf,
};

const G07_STREAM_CAP: usize = 64 * 1024;
const G07_CHILD_LIFETIME: Duration = Duration::from_secs(45);
const G07_BARRIER_PHASE: Duration = Duration::from_secs(15);
const G07_CLEANUP_PHASE: Duration = Duration::from_secs(15);
const G07_COHORT_REAP: Duration = Duration::from_secs(10);

fn synthetic_scope() -> (Value, OwnedScopeFacts) {
    let root = "/workspace/scratch/13897606dfde/p6pg.Test0007";
    let paths = [
        ("run_root", root.to_string()),
        ("data_dir", format!("{root}/data")),
        ("socket_dir", format!("{root}/socket")),
        ("home_dir", format!("{root}/home")),
        ("tmp_dir", format!("{root}/tmp")),
        ("config_file", format!("{root}/data/postgresql.conf")),
        ("hba_file", format!("{root}/data/pg_hba.conf")),
        (
            "postgres_executable",
            format!("{root}/install/bin/postgres"),
        ),
    ];
    let executable_sha256 = "a".repeat(64);
    let server = ProcessIdentity {
        pid: 41,
        start_ticks: 500,
        uid: 1000,
        executable: PathBuf::from(&paths[7].1),
        executable_sha256: executable_sha256.clone(),
        executable_dev: 7,
        executable_inode: 80,
    };
    let mut manifest = json!({
        "schema_version": 1,
        "run_id": "00000000-0000-0000-0000-000000000007",
        "postgres_executable_sha256": executable_sha256,
        "server_pid": server.pid,
        "server_start_ticks": server.start_ticks,
        "server_uid": server.uid,
        "server_executable_dev": server.executable_dev,
        "server_executable_inode": server.executable_inode,
        "os_user": "agent",
        "database_user": "agent",
        "admin_database": "postgres",
        "port": 5432,
        "server_version_num": 180006,
        "offline_system_identifier": "7500000000000000007",
        "listen_addresses": ""
    });
    for (field, path) in &paths {
        manifest[*field] = json!(path);
    }
    let facts = OwnedScopeFacts {
        expected_run_root: PathBuf::from(root),
        current_uid: 1000,
        current_os_user: "agent".to_string(),
        paths: paths
            .iter()
            .map(|(field, path)| PathFact {
                field,
                canonical: PathBuf::from(path),
                has_symlink_component: false,
                uid: 1000,
                mode: match *field {
                    "config_file" | "hba_file" => 0o600,
                    "postgres_executable" => 0o755,
                    _ => 0o700,
                },
                kind: if matches!(*field, "config_file" | "hba_file" | "postgres_executable") {
                    PathKind::RegularFile
                } else {
                    PathKind::Directory
                },
            })
            .collect(),
        server,
        offline_system_identifier: "7500000000000000007".to_string(),
    };
    (manifest, facts)
}

#[test]
fn g07_fixture_configuration_fails_closed() {
    let (valid, facts) = synthetic_scope();
    let path = OsStr::new("/workspace/scratch/13897606dfde/p6pg.Test0007/scope.json");
    let encoded = valid.to_string();
    assert_eq!(
        validate_owned_scope(None, &encoded, &facts, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    assert_eq!(
        validate_owned_scope(
            Some(OsStr::new("/outside/scope.json")),
            &encoded,
            &facts,
            &[]
        ),
        Err(FixtureError::ConfigurationRejected)
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let non_unicode = OsString::from_vec(vec![0xff]);
        assert_eq!(
            validate_owned_scope(Some(&non_unicode), &encoded, &facts, &[]),
            Err(FixtureError::ConfigurationRejected)
        );
    }
    for malformed in ["", "not-json", "[]", "{\"schema_version\":1}"] {
        assert_eq!(
            validate_owned_scope(Some(path), malformed, &facts, &[]),
            Err(FixtureError::ConfigurationRejected),
            "malformed manifest: {malformed}"
        );
    }
    // URLs/host lists/credentials have no permitted encoding in the structured
    // scope. Reject the field itself, including otherwise valid JSON.
    for (field, value) in [
        ("admin_url", json!("postgres://agent@127.0.0.1/postgres")),
        (
            "hosts",
            json!(["/synthetic/g07-run/socket", "remote.example"]),
        ),
        ("host", json!("localhost")),
        ("password", json!("synthetic-forbidden")),
        ("passfile", json!("/synthetic/g07-run/home/.pgpass")),
        ("service", json!("ambient")),
        ("options", json!("-c search_path=public")),
        ("sslcert", json!("/synthetic/g07-run/client.crt")),
        ("deadline_ms", json!(0)),
    ] {
        let mut hostile = valid.clone();
        hostile[field] = value;
        assert_eq!(
            validate_owned_scope(Some(path), &hostile.to_string(), &facts, &[]),
            Err(FixtureError::ConfigurationRejected),
            "unlisted field {field}"
        );
    }
    for (field, value) in [
        ("run_root", json!("/synthetic/other-run")),
        ("socket_dir", json!("/outside/socket")),
        ("listen_addresses", json!("127.0.0.1")),
        ("database_user", json!("postgres")),
        ("server_pid", json!(42)),
        ("server_start_ticks", json!(501)),
        ("server_uid", json!(1001)),
        ("server_executable_dev", json!(8)),
        ("server_executable_inode", json!(81)),
        ("postgres_executable_sha256", json!("b".repeat(64))),
        ("offline_system_identifier", json!("7500000000000000008")),
        ("server_version_num", json!(180005)),
        ("port", json!(5433)),
    ] {
        let mut hostile = valid.clone();
        hostile[field] = value;
        assert_eq!(
            validate_owned_scope(Some(path), &hostile.to_string(), &facts, &[]),
            Err(FixtureError::ConfigurationRejected),
            "mismatched {field}"
        );
    }
    let mut symlink = facts.clone();
    symlink.paths[2].has_symlink_component = true;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &symlink, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut outside = facts.clone();
    outside.paths[2].canonical = PathBuf::from("/outside/socket");
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &outside, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut wrong_owner = facts.clone();
    wrong_owner.paths[2].uid = 1001;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &wrong_owner, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut public_socket = facts.clone();
    public_socket.paths[2].mode = 0o755;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &public_socket, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut public_root = facts.clone();
    public_root.paths[0].mode = 0o755;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &public_root, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut root_file = facts.clone();
    root_file.paths[0].kind = PathKind::RegularFile;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &root_file, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut executable_directory = facts.clone();
    executable_directory.paths[7].kind = PathKind::Directory;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &executable_directory, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut missing_path = facts.clone();
    missing_path.paths.pop();
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &missing_path, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut duplicate_path = facts.clone();
    duplicate_path.paths.push(duplicate_path.paths[0].clone());
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &duplicate_path, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    // SQLx constructors can consume PG* even without pgpass. The eventual
    // connection adapter must reject ambient PG* before constructing options.
    for variable in [
        "PGHOST",
        "PGHOSTADDR",
        "PGPASSWORD",
        "PGPASSFILE",
        "PGSERVICE",
        "PGOPTIONS",
        "PGSSLKEY",
        "PGUNRECOGNIZED",
    ] {
        let ambient = [(OsString::from(variable), OsString::from("hostile"))];
        assert_eq!(
            validate_owned_scope(Some(path), &encoded, &facts, &ambient),
            Err(FixtureError::ConfigurationRejected),
            "ambient {variable}"
        );
    }
    assert!(
        validate_owned_scope(Some(path), &encoded, &facts, &[]).is_ok(),
        "exact owned configuration must be accepted"
    );
    // No capability is present in this reducer: rejected input cannot connect,
    // perform DDL or invoke Docker. Runtime ordering still needs source review.
}

fn synthetic_database_identity() -> DatabaseIdentity {
    DatabaseIdentity {
        cluster_system_identifier: "7500000000000000007".to_string(),
        name: "p6_g07_0000000000000000_00000000000000000000000000000007".to_string(),
        oid: 16_401,
        owner: "agent".to_string(),
    }
}

#[test]
fn g07_cleanup_requires_exact_owned_identity() {
    let expected = synthetic_database_identity();
    let mut mismatches = Vec::new();
    let mut wrong = expected.clone();
    wrong.cluster_system_identifier.push('8');
    mismatches.push(wrong);
    let mut wrong = expected.clone();
    wrong.oid += 1;
    mismatches.push(wrong);
    let mut wrong = expected.clone();
    wrong.owner = "postgres".to_string();
    mismatches.push(wrong);
    let mut wrong = expected.clone();
    wrong.name.push('8');
    mismatches.push(wrong);
    for observed in mismatches {
        assert_eq!(
            checked_drop_statement(&expected, &observed),
            Err(FixtureError::IdentityMismatch),
            "no DROP for {observed:?}"
        );
    }
    for name in ["postgres", "bad; DROP DATABASE postgres", "p6_g07_*"] {
        let mut invalid = expected.clone();
        invalid.name = name.to_string();
        assert_eq!(
            checked_drop_statement(&invalid, &invalid),
            Err(FixtureError::IdentityMismatch)
        );
    }
    let sql = checked_drop_statement(&expected, &expected)
        .expect("exact owned identity must produce one DROP");
    assert_eq!(sql, format!("DROP DATABASE {}", expected.name));
    assert!(!sql.contains("FORCE"));
    assert!(!sql.contains("IF EXISTS"));
    assert!(!sql.contains("pg_terminate_backend"));
}

#[test]
fn g07_unknown_database_outcome_is_not_success() {
    let identity = synthetic_database_identity();
    for operation in [DdlOperation::Create, DdlOperation::Drop] {
        for recorded in [None, Some(&identity)] {
            let decision = classify_ddl_response(operation, DdlResponse::Unknown, recorded);
            assert_eq!(
                decision.resolution,
                DatabaseResolution::Unknown,
                "lost {operation:?} response is Unknown"
            );
            assert!(!decision.retry, "uncertainty does not permit repeated DDL");
            assert_eq!(
                decision.cleanup_target, None,
                "uncertainty never authorizes DROP"
            );
        }
    }
    let created = classify_ddl_response(
        DdlOperation::Create,
        DdlResponse::Confirmed,
        Some(&identity),
    );
    assert_eq!(
        created.resolution,
        DatabaseResolution::Ready(identity.clone())
    );
    assert!(!created.retry);
    assert_eq!(created.cleanup_target, Some(identity.clone()));
    let no_identity = classify_ddl_response(DdlOperation::Create, DdlResponse::Confirmed, None);
    assert_eq!(
        no_identity.resolution,
        DatabaseResolution::Unknown,
        "generated name is not ownership evidence"
    );
    assert_eq!(no_identity.cleanup_target, None);
    let dropped =
        classify_ddl_response(DdlOperation::Drop, DdlResponse::Confirmed, Some(&identity));
    assert_eq!(dropped.resolution, DatabaseResolution::Dropped);
    assert!(!dropped.retry);
    assert_eq!(dropped.cleanup_target, None);
    let unverified_drop = classify_ddl_response(DdlOperation::Drop, DdlResponse::Confirmed, None);
    assert_eq!(unverified_drop.resolution, DatabaseResolution::Unknown);
    assert_eq!(unverified_drop.cleanup_target, None);
}

#[derive(Debug, Clone, Copy)]
struct Deadline(Duration);

impl Deadline {
    fn phase(parent: Self, now: Duration, allowance: Duration) -> Self {
        Self(now.checked_add(allowance).unwrap_or(now).min(parent.0))
    }

    fn remaining(self, now: Duration, local: Duration) -> Option<Duration> {
        let remaining = self.0.checked_sub(now)?.min(local);
        (!remaining.is_zero()).then_some(remaining)
    }
}

#[test]
fn g07_deadlines_do_not_restart_between_phases() {
    let case = Deadline(Duration::from_secs(90));
    let child = Deadline::phase(case, Duration::ZERO, G07_CHILD_LIFETIME);
    let barrier = Deadline::phase(child, Duration::ZERO, G07_BARRIER_PHASE);
    for operation in ["accept", "read", "write"] {
        assert_eq!(
            barrier.remaining(Duration::from_millis(14_750), Duration::from_secs(2)),
            Some(Duration::from_millis(250)),
            "{operation} shares one cohort barrier deadline"
        );
        assert_eq!(
            barrier.remaining(Duration::from_secs(15), Duration::from_secs(2)),
            None,
            "{operation} cannot restart expired barrier"
        );
    }
    let expired_phase = Deadline::phase(child, Duration::from_secs(45), G07_BARRIER_PHASE);
    assert_eq!(
        expired_phase.remaining(Duration::from_secs(45), Duration::from_secs(2)),
        None
    );
    assert_eq!(
        child.remaining(Duration::from_secs(44), Duration::from_secs(3)),
        Some(Duration::from_secs(1)),
        "wait cannot outlive child lifetime"
    );
    let cleanup = Deadline::phase(case, Duration::from_secs(82), G07_CLEANUP_PHASE);
    let cohort = Deadline::phase(cleanup, Duration::from_secs(83), G07_COHORT_REAP);
    for operation in ["kill/wait", "output", "pool-close", "drop"] {
        assert_eq!(
            cohort.remaining(Duration::from_secs(89), Duration::from_secs(3)),
            Some(Duration::from_secs(1)),
            "{operation} shares remaining cleanup/case budget"
        );
        assert_eq!(
            cohort.remaining(Duration::from_secs(90), Duration::from_secs(3)),
            None
        );
    }
    let overflow = Deadline::phase(case, Duration::MAX, Duration::from_secs(1));
    assert_eq!(
        overflow.remaining(Duration::MAX, Duration::from_secs(1)),
        None,
        "clock overflow fails closed"
    );
}

struct ChildConfiguration {
    scope_path: PathBuf,
    database: String,
    database_oid: u32,
    database_owner: String,
    mode: &'static str,
    owner: Uuid,
    limit: u32,
    handshake: Uuid,
    test_launch_id: Uuid,
    barrier: Option<String>,
    home: PathBuf,
    tmp: PathBuf,
}

struct ChildEnvironment {
    clear_inherited: bool,
    entries: BTreeMap<OsString, OsString>,
}

fn child_environment(
    config: &ChildConfiguration,
    _ambient: &[(OsString, OsString)],
) -> ChildEnvironment {
    let mut entries = [
        (
            "P6_G07_SCOPE_PATH",
            config.scope_path.as_os_str().to_owned(),
        ),
        ("P6_G07_DATABASE_NAME", OsString::from(&config.database)),
        (
            "P6_G07_DATABASE_OID",
            OsString::from(config.database_oid.to_string()),
        ),
        (
            "P6_G07_DATABASE_OWNER",
            OsString::from(&config.database_owner),
        ),
        ("P6_G07_MODE", OsString::from(config.mode)),
        ("P6_G07_OWNER", OsString::from(config.owner.to_string())),
        ("P6_G07_LIMIT", OsString::from(config.limit.to_string())),
        (
            "P6_G07_CHILD_HANDSHAKE",
            OsString::from(config.handshake.to_string()),
        ),
        (
            "P6_G07_TEST_LAUNCH_ID",
            OsString::from(config.test_launch_id.to_string()),
        ),
        ("HOME", config.home.as_os_str().to_owned()),
        ("TMPDIR", config.tmp.as_os_str().to_owned()),
        ("LC_ALL", OsString::from("C")),
        ("TZ", OsString::from("UTC")),
        ("RUST_BACKTRACE", OsString::from("0")),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), value))
    .collect::<BTreeMap<_, _>>();
    if let Some(barrier) = &config.barrier {
        entries.insert(
            OsString::from("P6_G07_CHILD_BARRIER"),
            OsString::from(barrier),
        );
    }
    ChildEnvironment {
        clear_inherited: true,
        entries,
    }
}

#[test]
fn g07_child_environment_is_allowlisted() {
    let mut config = ChildConfiguration {
        scope_path: PathBuf::from("/workspace/scratch/13897606dfde/p6pg.Test0007/scope.json"),
        database: synthetic_database_identity().name,
        database_oid: 16_401,
        database_owner: "agent".to_string(),
        mode: "claim",
        owner: Uuid::from_u128(500),
        limit: 2,
        handshake: Uuid::from_u128(900),
        test_launch_id: Uuid::from_u128(901),
        barrier: None,
        home: PathBuf::from("/workspace/scratch/13897606dfde/p6pg.Test0007/home"),
        tmp: PathBuf::from("/workspace/scratch/13897606dfde/p6pg.Test0007/tmp"),
    };
    let ambient = [
        "DATABASE_URL",
        "PGHOST",
        "PGPASSWORD",
        "DOCKER_HOST",
        "AWS_ACCESS_KEY_ID",
        "CARGO_REGISTRY_TOKEN",
        "P6_CHILD_URL",
        "P6_CHILD_BARRIER",
        "P6_G07_CHILD_BARRIER",
        "PATH",
        "LD_PRELOAD",
    ]
    .map(|name| (OsString::from(name), OsString::from("stale-hostile")));
    let environment = child_environment(&config, &ambient);
    assert!(
        environment.clear_inherited,
        "Command must clear inherited environment"
    );
    let expected = [
        (
            "P6_G07_SCOPE_PATH",
            config.scope_path.to_str().unwrap().to_string(),
        ),
        ("P6_G07_DATABASE_NAME", config.database.clone()),
        ("P6_G07_DATABASE_OID", config.database_oid.to_string()),
        ("P6_G07_DATABASE_OWNER", config.database_owner.clone()),
        ("P6_G07_MODE", config.mode.to_string()),
        ("P6_G07_OWNER", config.owner.to_string()),
        ("P6_G07_LIMIT", config.limit.to_string()),
        ("P6_G07_CHILD_HANDSHAKE", config.handshake.to_string()),
        ("P6_G07_TEST_LAUNCH_ID", config.test_launch_id.to_string()),
        ("HOME", config.home.to_str().unwrap().to_string()),
        ("TMPDIR", config.tmp.to_str().unwrap().to_string()),
        ("LC_ALL", "C".to_string()),
        ("TZ", "UTC".to_string()),
        ("RUST_BACKTRACE", "0".to_string()),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), OsString::from(value)))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(
        environment.entries, expected,
        "only explicit owned values may reach child"
    );
    config.barrier = Some("127.0.0.1:34123".to_string());
    let barrier_environment = child_environment(&config, &ambient);
    let mut barrier_expected = expected;
    barrier_expected.insert(
        OsString::from("P6_G07_CHILD_BARRIER"),
        OsString::from("127.0.0.1:34123"),
    );
    assert!(barrier_environment.clear_inherited);
    assert_eq!(
        barrier_environment.entries, barrier_expected,
        "only supplied barrier/token can appear"
    );
}

#[derive(Default)]
struct CapturedStream {
    bytes: Vec<u8>,
    overflow: bool,
}

impl CapturedStream {
    fn retain(&mut self, chunk: &[u8]) {
        let available = G07_STREAM_CAP.saturating_sub(self.bytes.len());
        let retain = chunk.len().min(available);
        self.bytes.extend_from_slice(&chunk[..retain]);
        self.overflow |= retain < chunk.len();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OwnedChildHandle(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KillObservation {
    Sent,
    Failed,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReapObservation {
    Reaped,
    Running,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildCleanup {
    Reaped,
    Failed,
    Unknown,
}

trait OwnedChildControl {
    fn kill(&mut self, handle: OwnedChildHandle) -> KillObservation;
    fn try_reap(&mut self, handle: OwnedChildHandle) -> ReapObservation;
}

fn finish_owned_child(
    control: &mut impl OwnedChildControl,
    handle: OwnedChildHandle,
) -> ChildCleanup {
    let kill = control.kill(handle);
    let reap = control.try_reap(handle);
    match (kill, reap) {
        (KillObservation::Failed, _) => ChildCleanup::Failed,
        (KillObservation::Sent, ReapObservation::Reaped) => ChildCleanup::Reaped,
        _ => ChildCleanup::Unknown,
    }
}

struct ControlProbe {
    kill_result: KillObservation,
    reap_result: ReapObservation,
    calls: Vec<(&'static str, OwnedChildHandle)>,
}

impl OwnedChildControl for ControlProbe {
    fn kill(&mut self, handle: OwnedChildHandle) -> KillObservation {
        self.calls.push(("kill", handle));
        self.kill_result
    }
    fn try_reap(&mut self, handle: OwnedChildHandle) -> ReapObservation {
        self.calls.push(("try_reap", handle));
        self.reap_result
    }
}

#[test]
fn g07_child_output_is_bounded_and_kill_targets_owned_handle() {
    let mut failures = Vec::new();
    let mut stdout = CapturedStream::default();
    let mut stderr = CapturedStream::default();
    stdout.retain(&vec![b'o'; G07_STREAM_CAP]);
    assert!(!stdout.overflow, "exact cap is allowed");
    stdout.retain(b"discard while continuing to drain");
    if stdout.bytes.len() != G07_STREAM_CAP {
        failures.push("stdout retention exceeds cap".to_string());
    }
    if !stdout.overflow {
        failures.push("stdout overflow did not mark failure".to_string());
    }
    stdout.retain(b"more bytes after overflow");
    if stdout.bytes != vec![b'o'; G07_STREAM_CAP] {
        failures.push("post-overflow bytes were retained".to_string());
    }
    stderr.retain(&vec![b'e'; G07_STREAM_CAP + 1]);
    if stderr.bytes != vec![b'e'; G07_STREAM_CAP] {
        failures.push("stderr retention exceeds cap".to_string());
    }
    if !stderr.overflow {
        failures.push("stderr overflow did not mark failure".to_string());
    }
    let owned = OwnedChildHandle(7);
    for (kill, reap, expected) in [
        (
            KillObservation::Sent,
            ReapObservation::Reaped,
            ChildCleanup::Reaped,
        ),
        (
            KillObservation::Failed,
            ReapObservation::Reaped,
            ChildCleanup::Failed,
        ),
        (
            KillObservation::Unknown,
            ReapObservation::Reaped,
            ChildCleanup::Unknown,
        ),
        (
            KillObservation::Sent,
            ReapObservation::Running,
            ChildCleanup::Unknown,
        ),
        (
            KillObservation::Sent,
            ReapObservation::Unknown,
            ChildCleanup::Unknown,
        ),
    ] {
        let mut probe = ControlProbe {
            kill_result: kill,
            reap_result: reap,
            calls: Vec::new(),
        };
        let observed = finish_owned_child(&mut probe, owned);
        if observed != expected {
            failures.push(format!(
                "kill={kill:?}, reap={reap:?}: expected {expected:?}, observed {observed:?}"
            ));
        }
        if probe.calls != [("kill", owned), ("try_reap", owned)] {
            failures.push(format!(
                "owned-handle calls missing/wrong: {:?}",
                probe.calls
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "bounded output / owned-handle contract: {failures:?}"
    );
}
