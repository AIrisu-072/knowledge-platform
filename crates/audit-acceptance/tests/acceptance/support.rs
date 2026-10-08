//! The acceptance environment: one disposable PostgreSQL 18.6 container
//! with two databases.
//!
//! - The Document database is owned by the non-superuser LOGIN role
//!   `document_app`, which runs the Document migrations and every Document
//!   producer (the runtime role separation the design hands to Document;
//!   the registration trigger and the digest are definer functions). The
//!   superuser only runs `audit-relay migrate` and `roles.sql`, test-only
//!   sabotage, and read-only bookkeeping.
//! - The Audit Store database (`audit-admin migrate`, `roles.sql`,
//!   `privileges.sql`, bootstrap, bindings, grants) is a separate database
//!   of the same cluster.
//!
//! Relay and Store runtime paths connect as the LOGIN roles of
//! `roles.sql`, never as the superuser. All data is synthetic.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use audit_core::{AuditEnvelope, AuditStore, Origin};
use audit_relay::config::RunConfig;
use audit_relay::health::{HealthOptions, health};
use audit_relay::monitor::{Monitor, RuntimeReporter};
use audit_relay::relay::{Relay, RelayParts, RunError, connect_for_health};
use audit_relay::store::RelayStore;
use audit_store_postgres::PostgresAuditStore;
use audit_store_postgres::admin::{AccessChange, AuditAdmin};
use outbox_delivery::DeliveryError;
use outbox_delivery::runner::RunSummary;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use testcontainers::core::{ExecCommand, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

pub const PASSWORD: &str = "synthetic-acceptance-password";
/// Issuer of the synthetic Store operators (never a Document identity).
pub const STORE_ISSUER: &str = "synthetic-audit-idp";
pub const DOC_DB: &str = "document_acceptance";
pub const STORE_DB: &str = "audit_store_acceptance";
pub const DOC_OWNER: &str = "document_app";

/// Store roles of the relay service login (design §10.1).
pub const RELAY_SERVICE_ROLES: [&str; 3] = [
    "audit_store_ingest",
    "audit_store_relay_control",
    "audit_store_reconciler",
];
/// Store roles of a relay operator login: never ingest.
pub const RELAY_OPERATOR_ROLES: [&str; 2] = ["audit_store_relay_control", "audit_store_reconciler"];

/// The bound for every wait on the relay or the scheduler.
pub const CONVERGE: Duration = Duration::from_secs(45);

/// The relay's lease in these tests.
pub const LEASE: Duration = Duration::from_secs(15);
/// The relay's ingest (and probe and control) timeout, and the timeout of
/// every Store client the tests open. Generous for a loaded CI host, and
/// below a third of [`LEASE`] as `AUDIT_RELAY_INGEST_TIMEOUT_MS` requires
/// (`RunConfig::from_lookup` refuses anything else). An answer later than
/// this is a commit-unknown outage: the attempt is returned and the retry
/// is acknowledged as `duplicate` ([`assert_stored_or_late_duplicate`]).
pub const STORE_TIMEOUT: Duration = Duration::from_secs(4);

/// Runs one acceptance scenario on its own runtime, on a thread with a large
/// stack: the scenarios compose deep production futures (Document services,
/// relay, Store administration) that overflow the default test-thread stack
/// in unoptimized builds. A panic in the scenario fails the test.
pub fn run_scenario<F, Fut>(scenario: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let thread = std::thread::Builder::new()
        .name("acceptance-scenario".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(16 * 1024 * 1024)
                .enable_all()
                .build()
                .expect("runtime")
                .block_on(scenario());
        })
        .expect("scenario thread");
    if let Err(panic) = thread.join() {
        std::panic::resume_unwind(panic);
    }
}

pub async fn connect_with_retry(url: &str, max: u32) -> PgPool {
    let mut last = None;
    for _ in 0..60 {
        match PgPoolOptions::new()
            .max_connections(max)
            .acquire_timeout(Duration::from_secs(10))
            .connect(url)
            .await
        {
            Ok(pool) => return pool,
            Err(error) => {
                last = Some(error);
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
    panic!("PostgreSQL did not accept connections: {last:?}");
}

pub async fn exec(pool: &PgPool, sql: &str) {
    sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("script failed: {error}\n{sql}"));
}

/// A synthetic LOGIN role and its pool.
#[derive(Clone)]
pub struct Login {
    pub role: String,
    pub url: String,
    pub pool: PgPool,
}

pub struct Env {
    pub container: ContainerAsync<GenericImage>,
    pub host: String,
    pub port: u16,
    /// Superuser on the Document database: migrations of the relay, test
    /// sabotage and read-only bookkeeping only.
    pub doc_admin: PgPool,
    /// The Document runtime (`document_app`, owner of the Document schema).
    pub document: PgPool,
    /// Superuser on the Store database: migration, roles and reads of the
    /// stored rows for the assertions only.
    pub store_admin: PgPool,
    /// Document logins of the relay.
    pub worker: Login,
    pub operator: Login,
    /// Store logins.
    pub relay_store: Login,
    pub operator_store: Login,
    pub verifier: Login,
    pub maintainer: Login,
    pub admin: Login,
    pub dba: Login,
}

impl Env {
    pub async fn start() -> Self {
        // Parallel scenarios pull the same image on a cold runner; a pull
        // that breaks mid-stream is retried a bounded number of times. Any
        // other start error fails at once.
        let mut attempt = 1;
        let container = loop {
            match GenericImage::new("postgres", "18.6-bookworm")
                .with_exposed_port(5432.tcp())
                .with_wait_for(WaitFor::message_on_stderr(
                    "database system is ready to accept connections",
                ))
                .with_env_var("POSTGRES_USER", "postgres")
                .with_env_var("POSTGRES_PASSWORD", "postgres")
                .with_env_var("POSTGRES_DB", "postgres")
                .with_cmd([
                    "postgres",
                    "-c",
                    "fsync=off",
                    "-c",
                    "full_page_writes=off",
                    "-c",
                    "max_connections=300",
                ])
                .start()
                .await
            {
                Ok(container) => break container,
                Err(error) if attempt < 3 && format!("{error:?}").contains("PullImage") => {
                    eprintln!("postgres image pull failed (attempt {attempt}); retrying");
                    tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
                    attempt += 1;
                }
                Err(error) => panic!("disposable PostgreSQL 18.6 should start: {error:?}"),
            }
        };
        let port = container
            .get_host_port_ipv4(5432.tcp())
            .await
            .expect("mapped port");
        let host = "127.0.0.1".to_owned();
        let cluster = connect_with_retry(
            &format!("postgres://postgres:postgres@{host}:{port}/postgres"),
            2,
        )
        .await;
        for statement in [
            format!(
                "CREATE ROLE {DOC_OWNER} LOGIN PASSWORD '{PASSWORD}' \
                 NOSUPERUSER NOCREATEDB NOCREATEROLE"
            ),
            format!("CREATE DATABASE {DOC_DB} OWNER {DOC_OWNER}"),
            format!("CREATE DATABASE {STORE_DB}"),
        ] {
            exec(&cluster, &statement).await;
        }
        cluster.close().await;
        let url = |role: &str, password: &str, database: &str| {
            format!("postgres://{role}:{password}@{host}:{port}/{database}")
        };

        // Document: the Document migrations as the Document owner, then the
        // relay migration and roles as the superuser (crates/audit-relay
        // README, "migrateの順序").
        let document = connect_with_retry(&url(DOC_OWNER, PASSWORD, DOC_DB), 12).await;
        document_repository_postgres::migrate(&document)
            .await
            .expect("document migrations as the Document owner");
        let doc_admin = connect_with_retry(&url("postgres", "postgres", DOC_DB), 6).await;
        audit_relay::migrate(&doc_admin)
            .await
            .expect("relay migration");
        exec(&doc_admin, audit_relay::ROLES_SQL).await;

        // Store: migrate, roles, privileges.
        let store_admin = connect_with_retry(&url("postgres", "postgres", STORE_DB), 6).await;
        audit_store_postgres::migrate(&store_admin)
            .await
            .expect("store migration");
        exec(&store_admin, audit_store_postgres::ROLES_SQL).await;
        exec(&store_admin, audit_store_postgres::PRIVILEGES_SQL).await;

        let cluster = Cluster {
            host: host.clone(),
            port,
        };
        let worker = cluster
            .doc_login(&doc_admin, "relay_worker", &["audit_relay_worker"])
            .await;
        let operator = cluster
            .doc_login(&doc_admin, "relay_operator", &["audit_relay_operator"])
            .await;
        let dba = cluster
            .store_login(&store_admin, "store_dba", &["audit_store_owner"])
            .await;
        let admin = cluster
            .store_login(&store_admin, "store_admin", &["audit_store_admin"])
            .await;
        let relay_store = cluster
            .store_login(&store_admin, "relay_svc", &RELAY_SERVICE_ROLES)
            .await;
        let operator_store = cluster
            .store_login(&store_admin, "operator_store", &RELAY_OPERATOR_ROLES)
            .await;
        let verifier = cluster
            .store_login(&store_admin, "store_verifier", &["audit_store_verifier"])
            .await;
        let maintainer = cluster
            .store_login(
                &store_admin,
                "store_maintainer",
                &["audit_store_maintainer"],
            )
            .await;
        let env = Self {
            container,
            host,
            port,
            doc_admin,
            document,
            store_admin,
            worker,
            operator,
            relay_store,
            operator_store,
            verifier,
            maintainer,
            admin,
            dba,
        };

        // Bindings and grants (crates/audit-store-postgres README, steps 5–7).
        let owner = AuditAdmin::connect_owner(env.dba.pool.clone())
            .await
            .expect("owner session");
        owner
            .bootstrap_administrator(&env.admin.role, STORE_ISSUER, "admin-1")
            .await
            .expect("bootstrap administrator");
        owner
            .bind_principal(&env.relay_store.role, "service", "audit-relay")
            .await
            .expect("bind the relay service");
        for (login, principal) in [
            (&env.operator_store, "operator-1"),
            (&env.verifier, "verifier-1"),
            (&env.maintainer, "maintainer-1"),
        ] {
            owner
                .bind_principal(&login.role, STORE_ISSUER, principal)
                .await
                .expect("bind");
        }
        let administrator = AuditAdmin::connect(env.admin.pool.clone())
            .await
            .expect("administrator");
        for (principal, capability) in [("verifier-1", "verify"), ("maintainer-1", "maintain")] {
            administrator
                .change_access(STORE_ISSUER, principal, capability, AccessChange::Grant)
                .await
                .expect("grant");
        }
        env
    }

    pub fn url(&self, role: &str, database: &str) -> String {
        format!(
            "postgres://{role}:{PASSWORD}@{}:{}/{database}",
            self.host, self.port
        )
    }

    pub fn superuser_url(&self, database: &str) -> String {
        format!(
            "postgres://postgres:postgres@{}:{}/{database}",
            self.host, self.port
        )
    }

    /// A pool of `login` on another database of the cluster (a restored
    /// Store).
    pub async fn pool_on(&self, login: &Login, database: &str) -> PgPool {
        connect_with_retry(&self.url(&login.role, database), 4).await
    }

    /// The relay service's Store client (ingest + relay_control +
    /// reconciler).
    pub async fn store_client(&self) -> PostgresAuditStore {
        PostgresAuditStore::new(self.relay_store.pool.clone(), STORE_TIMEOUT)
            .await
            .expect("relay store session")
    }

    /// The relay operator's Store client (reconcile, replay, health).
    pub async fn operator_client(&self) -> PostgresAuditStore {
        PostgresAuditStore::new(self.operator_store.pool.clone(), STORE_TIMEOUT)
            .await
            .expect("operator store session")
    }

    pub async fn verifier_admin(&self) -> AuditAdmin {
        AuditAdmin::connect(self.verifier.pool.clone())
            .await
            .expect("verifier session")
    }

    /// `audit-relay health` as the CLI runs it: the worker login on the
    /// Document database and the relay service login on the Store
    /// (`connect_for_health`, which reports an unreachable Store instead of
    /// failing).
    pub async fn health(&self, reconcile: bool) -> Value {
        self.health_against(&self.relay_store.url, reconcile).await
    }

    pub async fn health_against(&self, store_url: &str, reconcile: bool) -> Value {
        let (source, store) = connect_for_health(&self.worker.url, store_url, STORE_TIMEOUT)
            .await
            .expect("health connects");
        let report = health(
            &source,
            store,
            HealthOptions {
                forecast: false,
                reconcile,
            },
        )
        .await
        .expect("health report");
        source.close().await;
        report
    }

    /// `audit_relay.status()` as the worker reads it.
    pub async fn relay_status(&self) -> Value {
        sqlx::query_scalar("SELECT audit_relay.status()")
            .fetch_one(&self.worker.pool)
            .await
            .expect("relay status")
    }

    /// Takes the Store database down: new connections are refused and the
    /// open sessions are terminated (a Store server outage as the relay
    /// sees it).
    pub async fn store_down(&self) {
        store_down(&self.doc_admin).await;
    }

    pub async fn store_up(&self) {
        exec(
            &self.doc_admin,
            &format!("ALTER DATABASE {STORE_DB} ALLOW_CONNECTIONS true"),
        )
        .await;
    }

    /// Runs a command inside the container: `(exit code, stdout, stderr)`.
    pub async fn docker_exec(&self, command: &[&str]) -> (i64, String, String) {
        let mut result = self
            .container
            .exec(ExecCommand::new(command.iter().map(|s| (*s).to_owned())))
            .await
            .expect("exec");
        let stdout = result.stdout_to_vec().await.expect("stdout");
        let stderr = result.stderr_to_vec().await.expect("stderr");
        let mut code = None;
        for _ in 0..200 {
            code = result.exit_code().await.expect("exit code");
            if code.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        (
            code.unwrap_or(-1),
            String::from_utf8_lossy(&stdout).into_owned(),
            String::from_utf8_lossy(&stderr).into_owned(),
        )
    }

    /// Every byte the Store database holds, as `pg_dump --data-only` text
    /// (all schemas, all tables): what a scan for leaked content reads.
    pub async fn store_dump_text(&self, database: &str) -> String {
        let dump = self.dump_text(database).await;
        assert!(dump.contains("audit_store.events"), "dump has the events");
        dump
    }

    /// `pg_dump --data-only` text of one database (all schemas, all tables).
    pub async fn dump_text(&self, database: &str) -> String {
        let (code, stdout, stderr) = self
            .docker_exec(&[
                "pg_dump",
                "-U",
                "postgres",
                "--data-only",
                "--no-owner",
                "-d",
                database,
            ])
            .await;
        assert_eq!(code, 0, "pg_dump: {stderr}");
        stdout
    }

    /// Every Document staging row (`public.audit_outbox_events`) as JSON
    /// text: what the relay reads.
    pub async fn staging_text(&self) -> String {
        sqlx::query_scalar(
            "SELECT coalesce(string_agg(to_jsonb(e)::text, E'\\n'), '') \
             FROM public.audit_outbox_events AS e",
        )
        .fetch_one(&self.doc_admin)
        .await
        .expect("staging text")
    }

    /// The highest Store seq the relay references in one Store recovery
    /// epoch (`--relay-max-seq`, read from the Document database).
    pub async fn relay_max_seq(&self, epoch: i64) -> i64 {
        self.relay_status().await["max_referenced_store_seq"][epoch.to_string()]
            .as_i64()
            .expect("referenced seq")
    }
}

/// Host and port of the container, for the LOGIN role URLs.
struct Cluster {
    host: String,
    port: u16,
}

impl Cluster {
    fn url(&self, role: &str, database: &str) -> String {
        format!(
            "postgres://{role}:{PASSWORD}@{}:{}/{database}",
            self.host, self.port
        )
    }

    async fn doc_login(&self, doc_admin: &PgPool, base: &str, roles: &[&str]) -> Login {
        exec(
            doc_admin,
            &format!(
                "CREATE ROLE {base} LOGIN PASSWORD '{PASSWORD}' NOSUPERUSER NOCREATEDB NOCREATEROLE"
            ),
        )
        .await;
        for role in roles {
            exec(doc_admin, &format!("GRANT {role} TO {base}")).await;
        }
        // roles.sql sets the login timeouts (re-applied after a new login).
        exec(doc_admin, audit_relay::ROLES_SQL).await;
        let url = self.url(base, DOC_DB);
        let pool = connect_with_retry(&url, 4).await;
        Login {
            role: base.to_owned(),
            url,
            pool,
        }
    }

    async fn store_login(&self, store_admin: &PgPool, base: &str, roles: &[&str]) -> Login {
        exec(
            store_admin,
            &format!(
                "CREATE ROLE {base} LOGIN PASSWORD '{PASSWORD}' NOSUPERUSER NOCREATEDB NOCREATEROLE"
            ),
        )
        .await;
        for role in roles {
            exec(store_admin, &format!("GRANT {role} TO {base}")).await;
        }
        // privileges.sql is re-applied after every new login role.
        exec(store_admin, audit_store_postgres::PRIVILEGES_SQL).await;
        let url = self.url(base, STORE_DB);
        let pool = connect_with_retry(&url, 4).await;
        Login {
            role: base.to_owned(),
            url,
            pool,
        }
    }
}

pub async fn store_down(cluster_admin: &PgPool) {
    exec(
        cluster_admin,
        &format!(
            "ALTER DATABASE {STORE_DB} ALLOW_CONNECTIONS false; \
             SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{STORE_DB}' AND pid <> pg_backend_pid();"
        ),
    )
    .await;
}

pub fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "audit-acceptance-{name}-{}",
        Uuid::now_v7().simple()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

// ---------------------------------------------------------------------------
// The relay, in process
// ---------------------------------------------------------------------------

/// The `audit-relay run` configuration from its environment variables, with
/// test timings (short poll and breaker cooldowns, [`LEASE`],
/// [`STORE_TIMEOUT`]).
pub fn run_config(source_url: &str, store_url: &str) -> RunConfig {
    let vars: BTreeMap<&str, String> = BTreeMap::from([
        (audit_relay::config::SOURCE_URL, source_url.to_owned()),
        (audit_relay::config::STORE_URL, store_url.to_owned()),
        ("AUDIT_RELAY_BATCH_SIZE", "8".to_owned()),
        ("AUDIT_RELAY_MAX_IN_FLIGHT", "4".to_owned()),
        ("AUDIT_RELAY_LEASE_MS", LEASE.as_millis().to_string()),
        ("AUDIT_RELAY_RENEW_MS", "1000".to_owned()),
        ("AUDIT_RELAY_MAX_PROCESSING_MS", "60000".to_owned()),
        ("AUDIT_RELAY_DRAIN_MS", "5000".to_owned()),
        ("AUDIT_RELAY_POLL_MS", "50".to_owned()),
        (
            "AUDIT_RELAY_INGEST_TIMEOUT_MS",
            STORE_TIMEOUT.as_millis().to_string(),
        ),
        ("AUDIT_RELAY_BREAKER_INITIAL_MS", "50".to_owned()),
        ("AUDIT_RELAY_BREAKER_MAX_MS", "400".to_owned()),
        ("AUDIT_RELAY_PROGRESS_MS", "1000".to_owned()),
    ]);
    RunConfig::from_lookup(&|name| vars.get(name).cloned()).expect("valid relay configuration")
}

/// A relay running in this process until [`RunningRelay::stop`].
pub struct RunningRelay {
    shutdown: watch::Sender<bool>,
    handle: JoinHandle<Result<RunSummary, RunError>>,
}

impl RunningRelay {
    /// `audit-relay run` itself (`audit_relay::relay::run`): connects with
    /// the worker and relay service logins, runs the startup checks
    /// (privileged session, same database, posture), reports its circuit
    /// for `health` and delivers until stopped.
    pub fn start(env: &Env) -> Self {
        Self::start_with(run_config(&env.worker.url, &env.relay_store.url))
    }

    pub fn start_with(config: RunConfig) -> Self {
        let (shutdown, stopped) = watch::channel(false);
        let handle = tokio::spawn(async move { audit_relay::relay::run(&config, stopped).await });
        Self { shutdown, handle }
    }

    /// The same assembly as `audit_relay::relay::run` (runner, breaker
    /// admission, handler, monitor with the runtime report) over a
    /// substituted Store client, for faults that must hit between the
    /// relay's probe and its ingest.
    pub async fn start_over(env: &Env, store: Arc<dyn AuditStore>) -> Self {
        let config = run_config(&env.worker.url, &env.relay_store.url);
        let source = connect_with_retry(&env.worker.url, 8).await;
        let relay = Relay::assemble(RelayParts {
            source: source.clone(),
            store,
            delivery: config.delivery,
            handler: config.handler,
            breaker: config.breaker,
            policy: config.policy,
            projector: None,
        })
        .expect("relay assembles");
        let monitor = Monitor::new(
            relay.breaker.clone(),
            relay.progress.clone(),
            config.monitor,
        )
        .with_reporter(RuntimeReporter::new(source));
        let (shutdown, stopped) = watch::channel(false);
        let handle = tokio::spawn(async move {
            let (stop, monitor_stopped) = watch::channel(false);
            let monitor = tokio::spawn(monitor.run(monitor_stopped));
            let result = relay.runner.run_until_shutdown(stopped).await;
            let _ = stop.send(true);
            let _ = monitor.await;
            result.map_err(RunError::from)
        });
        Self { shutdown, handle }
    }

    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }

    /// Stops the relay (SIGTERM in the CLI). A stop that arrives while the
    /// runner is processing ends with `StoreUnknown`, which the operations
    /// guide documents as expected (the lease expires and the row is
    /// claimed again).
    pub async fn stop(self) -> Option<RunSummary> {
        let _ = self.shutdown.send(true);
        match tokio::time::timeout(Duration::from_secs(30), self.handle)
            .await
            .expect("relay stops within the drain bound")
            .expect("relay task")
        {
            Ok(summary) => Some(summary),
            Err(RunError::Delivery(DeliveryError::StoreUnknown)) => None,
            Err(error) => panic!("relay stopped with an error: {error:?}"),
        }
    }
}

/// Waits until `done` holds, polling every 100 ms, for at most `limit`.
pub async fn wait_until<F, Fut>(what: &str, limit: Duration, mut done: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        if done().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what}: not reached within {limit:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Polls `audit-relay health` until `accept` holds and returns that report.
pub async fn health_when(env: &Env, accept: impl Fn(&Value) -> bool) -> Value {
    health_when_against(env, &env.relay_store.url, accept).await
}

/// [`health_when`] against another Store database (a restored Store).
pub async fn health_when_against(
    env: &Env,
    store_url: &str,
    accept: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = tokio::time::Instant::now() + CONVERGE;
    loop {
        let report = env.health_against(store_url, false).await;
        if accept(&report) {
            return report;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "health never reached the expected state: {report}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Every staging row has been delivered (and nothing is pending).
pub async fn drained(env: &Env) -> bool {
    let status = env.relay_status().await;
    status["pending"] == json!(0)
        && status["leased"] == json!(0)
        && status["delivered"] == status["staged"]
}

// ---------------------------------------------------------------------------
// Source and Store rows
// ---------------------------------------------------------------------------

/// One `public.audit_outbox_events` row as a Document producer wrote it.
#[derive(Debug, Clone)]
pub struct StagedRow {
    pub event_id: Uuid,
    pub event_type: String,
    pub source: String,
    pub subject: String,
    pub actor_idp: String,
    pub actor_pid: String,
    pub resource_type: String,
    pub resource_id: Uuid,
    pub resource_version_id: Option<Uuid>,
    pub result: String,
    pub trace_id: Option<String>,
    pub data: Value,
    pub occurred_at: String,
}

pub async fn staged_rows(env: &Env) -> Vec<StagedRow> {
    sqlx::query(
        "SELECT event_id, event_type, source, subject, actor_identity_provider, \
                actor_principal_id, resource_type, resource_id, resource_version_id, result, \
                trace_id, data, to_char(occurred_at AT TIME ZONE 'UTC', \
                'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS occurred \
         FROM public.audit_outbox_events ORDER BY occurred_at, event_id",
    )
    .fetch_all(&env.doc_admin)
    .await
    .expect("staging rows")
    .iter()
    .map(|row| StagedRow {
        event_id: row.get("event_id"),
        event_type: row.get("event_type"),
        source: row.get("source"),
        subject: row.get("subject"),
        actor_idp: row.get("actor_identity_provider"),
        actor_pid: row.get("actor_principal_id"),
        resource_type: row.get("resource_type"),
        resource_id: row.get("resource_id"),
        resource_version_id: row.get("resource_version_id"),
        result: row.get("result"),
        trace_id: row.get("trace_id"),
        data: row.get("data"),
        occurred_at: row.get("occurred"),
    })
    .collect()
}

/// The relay's delivery ledger row of one event.
#[derive(Debug, Clone)]
pub struct DeliveryRow {
    pub event_id: Uuid,
    pub delivered: bool,
    pub store_seq: Option<i64>,
    pub store_envelope_digest: Option<Vec<u8>>,
    pub store_outcome: Option<String>,
    pub store_recovery_epoch: Option<i64>,
    pub attempt_count: i32,
    pub quarantine_code: Option<String>,
    pub last_outage_code: Option<String>,
    pub registration_kind: String,
    pub commitment: Vec<u8>,
}

pub async fn deliveries(env: &Env) -> BTreeMap<Uuid, DeliveryRow> {
    sqlx::query(
        "SELECT event_id, delivered_at IS NOT NULL AS delivered, store_seq, \
                store_envelope_digest, store_outcome, store_recovery_epoch, attempt_count, \
                quarantine_code, last_outage_code, registration_kind, \
                audit_relay.commitment(commitment_salt, source_digest) AS commitment \
         FROM audit_relay.deliveries",
    )
    .fetch_all(&env.doc_admin)
    .await
    .expect("deliveries")
    .iter()
    .map(|row| {
        let id: Uuid = row.get("event_id");
        (
            id,
            DeliveryRow {
                event_id: id,
                delivered: row.get("delivered"),
                store_seq: row.get("store_seq"),
                store_envelope_digest: row.get("store_envelope_digest"),
                store_outcome: row.get("store_outcome"),
                store_recovery_epoch: row.get("store_recovery_epoch"),
                attempt_count: row.get("attempt_count"),
                quarantine_code: row.get("quarantine_code"),
                last_outage_code: row.get("last_outage_code"),
                registration_kind: row.get("registration_kind"),
                commitment: row.get("commitment"),
            },
        )
    })
    .collect()
}

/// The Store outcome of one acknowledged first delivery: `stored`, or
/// `duplicate` only after an earlier attempt of the same row whose answer
/// came too late. Such an ingest timed out ([`STORE_TIMEOUT`],
/// `store_timeout`; the attempt is returned as an outage) after the Store
/// had committed, and the retry found the stored event (design §6.3,
/// commit outcome unknown) — correct behaviour on a loaded host.
/// Exactly-once is proven by the Store's rows
/// ([`assert_delivered_exactly_once`]), not by this outcome. Returns
/// whether the row was such a late duplicate.
pub fn assert_stored_or_late_duplicate(delivery: &DeliveryRow) -> bool {
    match delivery.store_outcome.as_deref() {
        Some("stored") => false,
        Some("duplicate") => {
            assert_eq!(
                delivery.last_outage_code.as_deref(),
                Some("store_timeout"),
                "a duplicate needs an earlier timed-out attempt: {delivery:?}"
            );
            true
        }
        other => panic!("unexpected Store outcome {other:?}: {delivery:?}"),
    }
}

/// One relay-origin event as the Store holds it.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub seq: i64,
    pub event_id: Uuid,
    pub event_type: String,
    pub recovery_epoch: i64,
    pub envelope_digest: Vec<u8>,
    pub envelope: Value,
}

pub async fn stored_relay_events(store_admin: &PgPool) -> Vec<StoredEvent> {
    sqlx::query(
        "SELECT e.seq, e.event_id, e.event_type, e.recovery_epoch, e.envelope_digest, \
                b.envelope::text AS body \
         FROM audit_store.events AS e JOIN audit_store.event_bodies AS b ON b.seq = e.seq \
         WHERE e.origin = 'relay' ORDER BY e.seq",
    )
    .fetch_all(store_admin)
    .await
    .expect("stored relay events")
    .iter()
    .map(|row| {
        let body: String = row.get("body");
        StoredEvent {
            seq: row.get("seq"),
            event_id: row.get("event_id"),
            event_type: row.get("event_type"),
            recovery_epoch: row.get("recovery_epoch"),
            envelope_digest: row.get("envelope_digest"),
            envelope: serde_json::from_str(&body).expect("stored envelope JSON"),
        }
    })
    .collect()
}

/// Control events of one type: `(seq, details)`.
pub async fn control_events(store_admin: &PgPool, event_type: &str) -> Vec<(i64, Value)> {
    sqlx::query(
        "SELECT e.seq, b.envelope -> 'data' -> 'details' AS details \
         FROM audit_store.events AS e JOIN audit_store.event_bodies AS b ON b.seq = e.seq \
         WHERE e.event_type = $1 ORDER BY e.seq",
    )
    .bind(event_type)
    .fetch_all(store_admin)
    .await
    .expect("control events")
    .iter()
    .map(|row| (row.get("seq"), row.get("details")))
    .collect()
}

/// Recomputes the whole Store chain with audit-core and validates every
/// body against the catalog on its origin path. Returns the number of
/// relay-origin events.
pub async fn assert_store_chain(store_admin: &PgPool) -> usize {
    let rows = sqlx::query(
        "SELECT e.seq, e.event_id, e.origin, e.envelope_digest, e.prev_chain, e.chain, \
                e.expired_at IS NOT NULL AS expired, b.envelope::text AS body \
         FROM audit_store.events AS e LEFT JOIN audit_store.event_bodies AS b ON b.seq = e.seq \
         ORDER BY e.seq",
    )
    .fetch_all(store_admin)
    .await
    .expect("read events");
    let mut prev = audit_core::GENESIS;
    let mut relayed = 0;
    for (index, row) in rows.iter().enumerate() {
        let seq: i64 = row.get("seq");
        assert_eq!(seq, index as i64 + 1, "the Store chain has no gaps");
        let event_id: Uuid = row.get("event_id");
        let origin: String = row.get("origin");
        let digest: Vec<u8> = row.get("envelope_digest");
        let prev_chain: Vec<u8> = row.get("prev_chain");
        let chain: Vec<u8> = row.get("chain");
        assert_eq!(prev_chain, prev.to_vec(), "seq {seq}: prev_chain");
        let digest: [u8; 32] = digest.try_into().expect("32 bytes");
        let expected = audit_core::chain_next(&prev, seq, event_id, &digest);
        assert_eq!(chain, expected.to_vec(), "seq {seq}: chain");
        let path = Origin::parse(&origin).expect("origin");
        let body: Option<String> = row.get("body");
        let expired: bool = row.get("expired");
        assert!(!expired, "seq {seq}: nothing expires in these tests");
        let body = body.unwrap_or_else(|| panic!("event {seq} keeps its body"));
        assert_eq!(
            audit_core::envelope_digest(&body),
            digest,
            "seq {seq}: digest"
        );
        let envelope = AuditEnvelope::from_json(&body, path)
            .unwrap_or_else(|rejection| panic!("event {seq} violates the catalog: {rejection}"));
        assert_eq!(envelope.id(), event_id);
        if path == Origin::Relay {
            relayed += 1;
        }
        prev = expected;
    }
    relayed
}

/// The delivery of every staging row is acknowledged, and the Store holds
/// exactly one relay event per staging row: same event id, the
/// acknowledged seq and envelope digest, the source commitment of the
/// relay ledger, and the staging row's actor, subject, type, source,
/// resource, result, time and correlation. Returns the stored events by
/// event id.
pub async fn assert_delivered_exactly_once(
    env: &Env,
    store_admin: &PgPool,
    store: &dyn RelayStore,
) -> BTreeMap<Uuid, StoredEvent> {
    let staged = staged_rows(env).await;
    let ledger = deliveries(env).await;
    let stored = stored_relay_events(store_admin).await;
    assert_eq!(
        ledger.len(),
        staged.len(),
        "every staging row is registered once"
    );
    let mut by_id: BTreeMap<Uuid, StoredEvent> = BTreeMap::new();
    for event in stored {
        let id = event.event_id;
        assert!(
            by_id.insert(id, event).is_none(),
            "event {id} is stored more than once"
        );
    }
    assert_eq!(
        by_id.len(),
        staged.len(),
        "the Store holds exactly the staged events (no extra, none missing)"
    );
    let ids: Vec<Uuid> = staged.iter().map(|row| row.event_id).collect();
    let receipts = store.lookup_receipts(&ids).await.expect("receipts");
    assert_eq!(receipts.len(), staged.len(), "one receipt per staged event");
    for row in &staged {
        let delivery = &ledger[&row.event_id];
        let event = by_id
            .get(&row.event_id)
            .unwrap_or_else(|| panic!("{} {} is not stored", row.event_type, row.event_id));
        assert!(delivery.delivered, "{}: acknowledged", row.event_type);
        assert_eq!(delivery.quarantine_code, None, "{}", row.event_type);
        assert_eq!(delivery.store_seq, Some(event.seq), "{}", row.event_type);
        assert_eq!(
            delivery.store_envelope_digest.as_deref(),
            Some(event.envelope_digest.as_slice()),
            "{}: acknowledged digest",
            row.event_type
        );
        assert_eq!(
            delivery.store_recovery_epoch,
            Some(event.recovery_epoch),
            "{}: acknowledged epoch",
            row.event_type
        );
        let receipt = receipts
            .iter()
            .find(|receipt| receipt.event_id == row.event_id)
            .expect("receipt");
        assert_eq!(receipt.seq, event.seq);
        assert_eq!(receipt.envelope_digest.to_vec(), event.envelope_digest);
        assert_eq!(
            receipt.source_commitment.map(|c| c.to_vec()),
            Some(delivery.commitment.clone()),
            "{}: the Store keeps the relay's source commitment",
            row.event_type
        );
        assert_eq!(
            event.envelope["data"]["provenance"]["source_commitment"],
            json!(audit_core::chain::to_hex(
                &receipt.source_commitment.expect("commitment")
            )),
            "{}: commitment in the envelope",
            row.event_type
        );
        assert_mapping(row, &event.envelope);
    }
    by_id
}

/// The Store envelope carries the staging row's identity as the relay
/// projects it (audit-core `project`).
pub fn assert_mapping(row: &StagedRow, envelope: &Value) {
    let what = format!("{} {}", row.event_type, row.event_id);
    assert_eq!(envelope["id"], json!(row.event_id), "{what}: id");
    assert_eq!(envelope["type"], json!(row.event_type), "{what}: type");
    assert_eq!(envelope["source"], json!(row.source), "{what}: source");
    assert_eq!(envelope["subject"], json!(row.subject), "{what}: subject");
    assert_eq!(envelope["time"], json!(row.occurred_at), "{what}: time");
    let data = &envelope["data"];
    assert_eq!(data["action"], json!(row.event_type), "{what}: action");
    assert_eq!(
        data["actor"],
        json!({"issuer": row.actor_idp, "principal_id": row.actor_pid}),
        "{what}: actor"
    );
    let mut resource = json!({"type": row.resource_type, "id": row.resource_id});
    if let Some(version) = row.resource_version_id {
        resource["version_id"] = json!(version);
    }
    assert_eq!(data["resource"], resource, "{what}: resource");
    assert_eq!(data["result"], json!(row.result), "{what}: result");
    assert_eq!(
        data["correlation"]["source_correlation_id"],
        row.trace_id
            .as_ref()
            .map_or(Value::Null, |trace| json!(trace)),
        "{what}: correlation"
    );
    // The staging row's free-text reason never travels; only its summary.
    match row.data.get("reason").and_then(Value::as_str) {
        Some(reason) => assert_eq!(
            data["reason"],
            json!({"provided": true, "utf8_bytes": reason.len(),
                   "text_retained": "source_systems"}),
            "{what}: reason summary"
        ),
        None => assert!(data.get("reason").is_none(), "{what}: no reason"),
    }
    assert!(
        data["details"].get("reason").is_none(),
        "{what}: no reason in the details"
    );
    // The scheduler's attribution: the requester stays the actor and the
    // executing service is recorded beside it.
    match row.data.get("serviceExecutor") {
        Some(executor) => assert_eq!(
            data["service_executor"],
            json!({"issuer": executor["identityProvider"],
                   "principal_id": executor["principalId"]}),
            "{what}: service executor"
        ),
        None => assert!(data.get("service_executor").is_none(), "{what}"),
    }
}

/// None of `needles` occurs in `haystack` (named for the failure message).
pub fn assert_absent(haystack: &str, what: &str, needles: &[String]) {
    for needle in needles {
        assert!(!needle.is_empty());
        assert!(
            !haystack.contains(needle.as_str()),
            "{what} must not contain {needle:?}"
        );
    }
}

/// `audit_relay.posture_check()` reports nothing: with the Document runtime
/// as the non-superuser owner of the Document schema, `run` may start.
pub async fn assert_relay_posture_clean(env: &Env) {
    let violations: Vec<(String, String)> =
        sqlx::query_as("SELECT violation, object FROM audit_relay.posture_check()")
            .fetch_all(&env.doc_admin)
            .await
            .expect("posture");
    assert!(violations.is_empty(), "relay posture: {violations:?}");
}
