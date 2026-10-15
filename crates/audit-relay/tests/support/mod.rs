//! Disposable PostgreSQL 18.6 with TWO databases in one container:
//! - the Document database (Document migrations, relay migration, relay
//!   roles), and
//! - the Audit Store database (Store migration, roles, privileges).
//!
//! Runtime paths connect as dedicated LOGIN roles, never as the superuser.
//! The superuser pools exist only for setup, producer simulation (the PoC
//! producer is the Document owner) and tamper/outage simulation. All data
//! is synthetic.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use audit_core::port::BoxFuture;
use audit_core::{
    AuditEnvelope, AuditStore, ControlReceipt, ControlReceiptRow, IngestReceipt, IngestRow, Origin,
    OutageCode, ProbeExpectation, ReceiptIdentity, ReceiptRow, RelayControl, StoreError,
    StoreState, StoreStatus,
};
use audit_relay::breaker::BreakerConfig;
use audit_relay::handler::{HandlerConfig, Projector};
use audit_relay::relay::{Relay, RelayParts};
use audit_relay::source::RelayPolicy;
use audit_relay::store::{LostRange, RelayStore, StoreStatusRow};
use audit_store_postgres::PostgresAuditStore;
use audit_store_postgres::admin::AuditAdmin;
use outbox_delivery::DeliveryConfig;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use testcontainers::core::{ExecCommand, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use uuid::Uuid;

pub const PASSWORD: &str = "synthetic-test-password";
pub const ISSUER: &str = "synthetic-idp";
pub const DOC_DB: &str = "document_test";
pub const STORE_DB: &str = "audit_store_test";
pub const SOURCE: &str = "urn:knowledge-platform:document-platform";
pub const SOURCE_FORMAT: &str = "document-audit-outbox-v0";

/// Store roles of the relay service login (design §10.1).
pub const RELAY_SERVICE_ROLES: [&str; 3] = [
    "audit_store_ingest",
    "audit_store_relay_control",
    "audit_store_reconciler",
];
/// Store roles of the relay operator login: never ingest.
pub const RELAY_OPERATOR_ROLES: [&str; 2] = ["audit_store_relay_control", "audit_store_reconciler"];

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
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    }
    panic!("PostgreSQL did not accept connections: {last:?}");
}

/// A synthetic LOGIN role and its pool.
#[derive(Clone)]
pub struct Login {
    pub role: String,
    pub url: String,
    pub pool: PgPool,
}

pub struct Cluster {
    pub container: ContainerAsync<GenericImage>,
    pub host: String,
    pub port: u16,
    /// Superuser on the Document database.
    pub doc_admin: PgPool,
}

impl Cluster {
    pub async fn start() -> Self {
        let container = GenericImage::new("postgres", "18.6-bookworm")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", "postgres")
            .with_env_var("POSTGRES_DB", DOC_DB)
            .with_cmd(["postgres", "-c", "fsync=off", "-c", "full_page_writes=off"])
            .start()
            .await
            .expect("disposable PostgreSQL 18.6 should start");
        let port = container
            .get_host_port_ipv4(5432.tcp())
            .await
            .expect("mapped port");
        let host = "127.0.0.1".to_owned();
        let url = format!("postgres://postgres:postgres@{host}:{port}/{DOC_DB}");
        let doc_admin = connect_with_retry(&url, 8).await;
        loop {
            if sqlx::query("SELECT 1").execute(&doc_admin).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Self {
            container,
            host,
            port,
            doc_admin,
        }
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

    /// Runs a command inside the container.
    pub async fn docker_exec(&self, command: &[&str]) -> (i64, String) {
        let mut result = self
            .container
            .exec(ExecCommand::new(command.iter().map(|s| s.to_string())))
            .await
            .expect("exec");
        let stdout = result.stdout_to_vec().await.expect("stdout");
        let stderr = result.stderr_to_vec().await.expect("stderr");
        let mut code = None;
        for _ in 0..100 {
            code = result.exit_code().await.expect("exit code");
            if code.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut output = String::from_utf8_lossy(&stdout).into_owned();
        output.push_str(&String::from_utf8_lossy(&stderr));
        (code.unwrap_or(-1), output)
    }
}

pub async fn exec(pool: &PgPool, sql: &str) {
    sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("script failed: {error}\n{sql}"));
}

/// The Document database only: Document and relay migrations, relay roles,
/// worker and operator logins.
pub struct DocEnv {
    pub cluster: Cluster,
    pub worker: Login,
    pub operator: Login,
}

impl DocEnv {
    pub async fn start() -> Self {
        let cluster = Cluster::start().await;
        document_repository_postgres::migrate(&cluster.doc_admin)
            .await
            .expect("document migrations");
        audit_relay::migrate(&cluster.doc_admin)
            .await
            .expect("relay migration");
        exec(&cluster.doc_admin, audit_relay::ROLES_SQL).await;
        let worker = doc_login(&cluster, "relay_worker", &["audit_relay_worker"]).await;
        let operator = doc_login(&cluster, "relay_operator", &["audit_relay_operator"]).await;
        Self {
            cluster,
            worker,
            operator,
        }
    }

    pub fn admin(&self) -> &PgPool {
        &self.cluster.doc_admin
    }
}

/// The full two-database environment.
pub struct Env {
    pub cluster: Cluster,
    pub doc_admin: PgPool,
    pub store_admin: PgPool,
    /// Document logins.
    pub worker: Login,
    pub operator: Login,
    /// Store logins: the relay service (ingest + relay_control +
    /// reconciler) and the relay operator (relay_control + reconciler).
    pub relay_store: Login,
    pub operator_store: Login,
    pub verifier: Login,
    pub maintainer: Login,
    pub dba: Login,
}

impl Env {
    /// Document migrations, relay migration and roles, Store, logins.
    pub async fn start() -> Self {
        let cluster = Cluster::start().await;
        document_repository_postgres::migrate(&cluster.doc_admin)
            .await
            .expect("document migrations");
        Self::finish(cluster).await
    }

    /// Completes an environment whose Document database is migrated (and
    /// possibly holds staging rows): relay migration, roles, Store, logins.
    pub async fn finish(cluster: Cluster) -> Self {
        audit_relay::migrate(&cluster.doc_admin)
            .await
            .expect("relay migration");
        exec(&cluster.doc_admin, audit_relay::ROLES_SQL).await;
        let doc_admin = cluster.doc_admin.clone();
        exec(&doc_admin, &format!("CREATE DATABASE {STORE_DB}")).await;
        let store_admin = connect_with_retry(&cluster.superuser_url(STORE_DB), 8).await;
        audit_store_postgres::migrate(&store_admin)
            .await
            .expect("store migration");
        exec(&store_admin, audit_store_postgres::ROLES_SQL).await;
        exec(&store_admin, audit_store_postgres::PRIVILEGES_SQL).await;

        let worker = doc_login(&cluster, "relay_worker", &["audit_relay_worker"]).await;
        let operator = doc_login(&cluster, "relay_operator", &["audit_relay_operator"]).await;
        let dba = store_login(&cluster, &store_admin, "store_dba", &["audit_store_owner"]).await;
        let admin = store_login(
            &cluster,
            &store_admin,
            "store_admin",
            &["audit_store_admin"],
        )
        .await;
        let relay_store =
            store_login(&cluster, &store_admin, "relay_svc", &RELAY_SERVICE_ROLES).await;
        let operator_store = store_login(
            &cluster,
            &store_admin,
            "operator_store",
            &RELAY_OPERATOR_ROLES,
        )
        .await;
        let verifier = store_login(
            &cluster,
            &store_admin,
            "verifier",
            &["audit_store_verifier"],
        )
        .await;
        let maintainer = store_login(
            &cluster,
            &store_admin,
            "maintainer",
            &["audit_store_maintainer"],
        )
        .await;
        let owner = AuditAdmin::connect_owner(dba.pool.clone())
            .await
            .expect("owner session");
        owner
            .bootstrap_administrator(&admin.role, ISSUER, "admin-1")
            .await
            .expect("bootstrap");
        owner
            .bind_principal(&relay_store.role, "service", "audit-relay")
            .await
            .expect("bind relay");
        for (login, principal) in [
            (&operator_store, "operator-1"),
            (&verifier, "verifier-1"),
            (&maintainer, "maintainer-1"),
        ] {
            owner
                .bind_principal(&login.role, ISSUER, principal)
                .await
                .expect("bind");
        }
        let administrator = AuditAdmin::connect(admin.pool.clone())
            .await
            .expect("admin");
        for (principal, capability) in [("verifier-1", "verify"), ("maintainer-1", "maintain")] {
            administrator
                .change_access(
                    ISSUER,
                    principal,
                    capability,
                    audit_store_postgres::admin::AccessChange::Grant,
                )
                .await
                .expect("grant");
        }
        Self {
            cluster,
            doc_admin,
            store_admin,
            worker,
            operator,
            relay_store,
            operator_store,
            verifier,
            maintainer,
            dba,
        }
    }

    pub async fn store_client(&self) -> PostgresAuditStore {
        PostgresAuditStore::new(self.relay_store.pool.clone(), Duration::from_millis(1_500))
            .await
            .expect("relay store session")
    }

    /// The relay operator's Store client (reconcile, replay, health).
    pub async fn operator_client(&self) -> PostgresAuditStore {
        PostgresAuditStore::new(
            self.operator_store.pool.clone(),
            Duration::from_millis(1_500),
        )
        .await
        .expect("operator store session")
    }

    /// A relay over the worker login and the relay Store login.
    pub async fn relay(&self) -> Relay {
        self.relay_with(RelayOverrides::default()).await
    }

    pub async fn relay_with(&self, overrides: RelayOverrides) -> Relay {
        let store: Arc<dyn AuditStore> = match overrides.store {
            Some(store) => store,
            None => Arc::new(self.store_client().await),
        };
        Relay::assemble(RelayParts {
            source: overrides.source.unwrap_or_else(|| self.worker.pool.clone()),
            store,
            delivery: overrides.delivery.unwrap_or_else(test_delivery),
            handler: test_handler(),
            breaker: test_breaker(),
            policy: overrides.policy.unwrap_or_default(),
            projector: overrides.projector,
        })
        .expect("relay assembles")
    }

    pub async fn insert(&self, row: &Staged) {
        insert_staged(&self.doc_admin, row).await;
    }

    pub async fn delivery(&self, event_id: Uuid) -> Value {
        delivery(&self.doc_admin, event_id).await
    }

    pub async fn status(&self) -> Value {
        sqlx::query_scalar("SELECT audit_relay.status()")
            .fetch_one(&self.worker.pool)
            .await
            .expect("status")
    }

    /// Superuser UPDATE of delivery state, bypassing guards and FKs
    /// (simulates an owner with `session_replication_role = replica`).
    pub async fn force(&self, sql: &str) {
        force(&self.doc_admin, sql).await;
    }

    /// Store events for one event id: `(seq, origin, event_type)`.
    pub async fn store_rows(&self, event_id: Uuid) -> Vec<(i64, String, String)> {
        sqlx::query("SELECT seq, origin, event_type FROM audit_store.events WHERE event_id = $1")
            .bind(event_id)
            .fetch_all(&self.store_admin)
            .await
            .expect("store rows")
            .iter()
            .map(|r| (r.get("seq"), r.get("origin"), r.get("event_type")))
            .collect()
    }

    pub async fn store_body(&self, event_id: Uuid) -> Option<String> {
        sqlx::query_scalar(
            "SELECT b.envelope::text FROM audit_store.events e \
             JOIN audit_store.event_bodies b ON b.seq = e.seq WHERE e.event_id = $1",
        )
        .bind(event_id)
        .fetch_optional(&self.store_admin)
        .await
        .expect("store body")
    }

    /// Control events of one type: `(seq, details)`.
    pub async fn controls(&self, event_type: &str) -> Vec<(i64, Value)> {
        control_events(&self.store_admin, event_type).await
    }
}

pub async fn control_events(pool: &PgPool, event_type: &str) -> Vec<(i64, Value)> {
    sqlx::query(
        "SELECT e.seq, b.envelope -> 'data' -> 'details' AS details \
         FROM audit_store.events AS e JOIN audit_store.event_bodies AS b ON b.seq = e.seq \
         WHERE e.event_type = $1 ORDER BY e.seq",
    )
    .bind(event_type)
    .fetch_all(pool)
    .await
    .expect("control events")
    .iter()
    .map(|row| (row.get("seq"), row.get("details")))
    .collect()
}

pub async fn force(pool: &PgPool, sql: &str) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .expect("replica");
    sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("forced statement failed: {error}\n{sql}"));
    tx.commit().await.expect("commit");
}

pub async fn doc_login(cluster: &Cluster, base: &str, roles: &[&str]) -> Login {
    exec(
        &cluster.doc_admin,
        &format!(
            "CREATE ROLE {base} LOGIN PASSWORD '{PASSWORD}' NOSUPERUSER NOCREATEDB NOCREATEROLE"
        ),
    )
    .await;
    for role in roles {
        exec(&cluster.doc_admin, &format!("GRANT {role} TO {base}")).await;
    }
    exec(&cluster.doc_admin, audit_relay::ROLES_SQL).await;
    let url = cluster.url(base, DOC_DB);
    let pool = connect_with_retry(&url, 8).await;
    Login {
        role: base.to_owned(),
        url,
        pool,
    }
}

pub async fn store_login(
    cluster: &Cluster,
    store_admin: &PgPool,
    base: &str,
    roles: &[&str],
) -> Login {
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
    exec(store_admin, audit_store_postgres::PRIVILEGES_SQL).await;
    let url = cluster.url(base, STORE_DB);
    let pool = connect_with_retry(&url, 8).await;
    Login {
        role: base.to_owned(),
        url,
        pool,
    }
}

#[derive(Default)]
pub struct RelayOverrides {
    pub store: Option<Arc<dyn AuditStore>>,
    pub source: Option<PgPool>,
    pub delivery: Option<DeliveryConfig>,
    pub policy: Option<RelayPolicy>,
    pub projector: Option<Projector>,
}

pub fn test_delivery() -> DeliveryConfig {
    DeliveryConfig {
        batch_size: 8,
        max_in_flight: 4,
        lease_duration: Duration::from_secs(6),
        renew_interval: Duration::from_secs(1),
        max_processing: Duration::from_secs(60),
        drain_timeout: Duration::from_secs(5),
        poll_interval: Duration::from_millis(50),
        reap_batch: 32,
    }
}

pub fn test_handler() -> HandlerConfig {
    HandlerConfig {
        ingest_timeout: Duration::from_millis(1_500),
        control_timeout: Duration::from_millis(1_000),
        control_attempts: 2,
    }
}

pub fn test_breaker() -> BreakerConfig {
    BreakerConfig {
        initial_cooldown: Duration::from_millis(20),
        max_cooldown: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(1_500),
        closed_claims: 8,
    }
}

/// Runs relay cycles until `done` holds (or panics after `limit`).
pub async fn drive<F, Fut>(relay: &Relay, limit: Duration, mut done: F)
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
            "relay did not converge in {limit:?}"
        );
        match relay.runner.run_cycle().await {
            Ok(_) | Err(outbox_delivery::DeliveryError::StoreUnknown) => {}
            Err(error) => panic!("relay cycle failed: {error:?}"),
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

/// Runs `n` relay cycles, tolerating outages.
pub async fn cycles(relay: &Relay, n: usize) {
    for _ in 0..n {
        match relay.runner.run_cycle().await {
            Ok(_) | Err(outbox_delivery::DeliveryError::StoreUnknown) => {}
            Err(error) => panic!("relay cycle failed: {error:?}"),
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

pub async fn delivered_count(env: &Env) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_relay.deliveries WHERE delivered_at IS NOT NULL")
        .fetch_one(&env.doc_admin)
        .await
        .expect("count")
}

pub async fn delivery(pool: &PgPool, event_id: Uuid) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(d) FROM audit_relay.deliveries d WHERE event_id = $1")
        .bind(event_id)
        .fetch_one(pool)
        .await
        .expect("delivery row")
}

/// A synthetic staging row in the exact shape Document producers write.
#[derive(Clone, Debug)]
pub struct Staged {
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

pub const ACTOR_IDP: &str = "poc";
pub const ACTOR_PID: &str = "poc-human";
pub const OCCURRED: &str = "2026-10-07T01:02:03.456789Z";

pub fn row_actor() -> Value {
    json!({"identityProvider": ACTOR_IDP, "principalId": ACTOR_PID})
}

impl Staged {
    fn base(
        event_type: &str,
        subject: String,
        resource: (&str, Uuid, Option<Uuid>),
        data: Value,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            event_type: event_type.to_owned(),
            source: SOURCE.to_owned(),
            subject,
            actor_idp: ACTOR_IDP.to_owned(),
            actor_pid: ACTOR_PID.to_owned(),
            resource_type: resource.0.to_owned(),
            resource_id: resource.1,
            resource_version_id: resource.2,
            result: "success".to_owned(),
            trace_id: None,
            data,
            occurred_at: OCCURRED.to_owned(),
        }
    }

    /// `document.created` (document_management / repository create).
    pub fn created() -> Self {
        let doc = Uuid::now_v7();
        Self::base(
            "document.created",
            format!("document/{doc}"),
            ("Document", doc, None),
            json!({"documentId": doc.to_string()}),
        )
    }

    /// `document.version.withdrawn` with a free-text reason and the
    /// duplicated actor (withdrawal.rs shape).
    pub fn withdrawn(reason: &str) -> Self {
        let doc = Uuid::now_v7();
        let ver = Uuid::now_v7();
        Self::base(
            "document.version.withdrawn",
            format!("document/{doc}/version/{ver}"),
            ("Document", doc, Some(ver)),
            json!({
                "documentId": doc.to_string(), "withdrawnDocumentVersionId": ver.to_string(),
                "formerCurrentVersionId": ver.to_string(), "resultingCurrentVersionId": null,
                "actor": row_actor(), "restorationWithheldByValidation": true,
                "restorationWithheldReason": "base_inspection_unavailable",
                "invalidatedScheduleCount": 1, "reason": reason
            }),
        )
    }

    /// `document.metadata.changed` with a reason (document_management.rs).
    pub fn metadata_changed(reason: &str) -> Self {
        let doc = Uuid::now_v7();
        let op = Uuid::now_v7();
        Self::base(
            "document.metadata.changed",
            format!("document/{doc}"),
            ("Document", doc, None),
            json!({
                "operation_id": op.to_string(), "document_id": doc.to_string(),
                "changed_keys": ["category", "extensions"], "document_revision": 10,
                "reason": reason
            }),
        )
    }

    /// `folder.renamed` with a reason (folder_management.rs).
    pub fn folder_renamed(reason: &str) -> Self {
        let folder = Uuid::now_v7();
        let op = Uuid::now_v7();
        Self::base(
            "folder.renamed",
            format!("folder/{folder}"),
            ("Folder", folder, None),
            json!({"folder_id": folder.to_string(), "folder_revision": 1,
                   "operation_id": op.to_string(), "reason": reason}),
        )
    }

    /// `access_policy.changed` (no reason recorded by the source).
    pub fn access_policy_changed() -> Self {
        let doc = Uuid::now_v7();
        let op = Uuid::now_v7();
        let policy = Uuid::now_v7();
        Self::base(
            "access_policy.changed",
            format!("document/{doc}"),
            ("Document", doc, None),
            json!({
                "operation_id": op.to_string(), "target_type": "Document",
                "target_id": doc.to_string(), "policy_id": policy.to_string(),
                "policy_revision": 2, "access_revision": 5
            }),
        )
    }

    /// `document.version.detail_viewed` (VIEW, with `first_record`) or
    /// `document.version.marked_unread` (RESET) as `current_read_state.rs`
    /// stages a real transition: `document/{id}` subject, the version as
    /// resource version, no trace id, `resulting = expected + 1`.
    pub fn read_state(view: bool, expected: i64, first_record: bool) -> Self {
        let doc = Uuid::now_v7();
        let ver = Uuid::now_v7();
        let (event_type, trigger) = if view {
            ("document.version.detail_viewed", "detail_display")
        } else {
            ("document.version.marked_unread", "user_reset")
        };
        let mut data = json!({
            "document_version_id": ver,
            "operation_id": Uuid::now_v7(),
            "expected_read_state_revision": expected,
            "resulting_read_state_revision": expected + 1,
            "trigger": trigger
        });
        if view {
            data["first_record"] = json!(first_record);
        }
        Self::base(
            event_type,
            format!("document/{doc}"),
            ("Document", doc, Some(ver)),
            data,
        )
    }

    /// `authorization.denied` as `targeted_events.rs`
    /// `record_authorization_denied` stages it: nil `AccessPolicy` resource,
    /// fixed subject, result `denied`, `{action_code, reason_code}` data.
    pub fn denied(action_code: &str) -> Self {
        let mut row = Self::base(
            "authorization.denied",
            "authorization/denied".to_owned(),
            ("AccessPolicy", Uuid::nil(), None),
            json!({"action_code": action_code, "reason_code": "forbidden"}),
        );
        row.result = "denied".to_owned();
        row
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    pub fn with_type(mut self, event_type: &str) -> Self {
        self.event_type = event_type.to_owned();
        self
    }
}

pub async fn insert_staged(pool: &PgPool, row: &Staged) {
    sqlx::query(
        "INSERT INTO public.audit_outbox_events (event_id, event_type, source, subject, \
             actor_identity_provider, actor_principal_id, resource_type, resource_id, \
             resource_version_id, result, trace_id, data, occurred_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::text::jsonb, $13::timestamptz)",
    )
    .bind(row.event_id)
    .bind(&row.event_type)
    .bind(&row.source)
    .bind(&row.subject)
    .bind(&row.actor_idp)
    .bind(&row.actor_pid)
    .bind(&row.resource_type)
    .bind(row.resource_id)
    .bind(row.resource_version_id)
    .bind(&row.result)
    .bind(&row.trace_id)
    .bind(row.data.to_string())
    .bind(&row.occurred_at)
    .execute(pool)
    .await
    .expect("staging insert");
}

/// The SQLSTATE of a failed query.
pub fn sqlstate(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(db) => db.code().map(|c| c.into_owned()).unwrap_or_default(),
        other => panic!("expected a database error, got {other:?}"),
    }
}

/// Validates every Store event: chain recomputed with audit-core, control
/// events conform to the catalog on their origin path, relay events parse
/// as relay envelopes. Returns the number of relay events.
pub async fn assert_store_conforms(pool: &PgPool) -> usize {
    let rows = sqlx::query(
        "SELECT e.seq, e.event_id, e.origin, e.envelope_digest, e.prev_chain, e.chain, \
                e.expired_at IS NOT NULL AS expired, b.envelope::text AS body \
         FROM audit_store.events AS e LEFT JOIN audit_store.event_bodies AS b ON b.seq = e.seq \
         ORDER BY e.seq",
    )
    .fetch_all(pool)
    .await
    .expect("read events");
    let mut prev = audit_core::GENESIS;
    let mut relayed = 0;
    for row in &rows {
        let seq: i64 = row.get("seq");
        let event_id: Uuid = row.get("event_id");
        let origin: String = row.get("origin");
        let digest: Vec<u8> = row.get("envelope_digest");
        let prev_chain: Vec<u8> = row.get("prev_chain");
        let chain: Vec<u8> = row.get("chain");
        let body: Option<String> = row.get("body");
        assert_eq!(prev_chain, prev.to_vec(), "seq {seq}: prev_chain");
        let digest: [u8; 32] = digest.try_into().expect("32 bytes");
        let expected = audit_core::chain_next(&prev, seq, event_id, &digest);
        assert_eq!(chain, expected.to_vec(), "seq {seq}: chain");
        let path = Origin::parse(&origin).expect("origin");
        let expired: bool = row.get("expired");
        if expired && path == Origin::Relay {
            // A retention or purge tombstone: only the identity row remains.
            assert!(body.is_none(), "seq {seq}: an expired row keeps no body");
            relayed += 1;
            prev = expected;
            continue;
        }
        let body = body.unwrap_or_else(|| panic!("event {seq} keeps its body"));
        assert_eq!(
            audit_core::envelope_digest(&body),
            digest,
            "seq {seq}: digest"
        );
        let envelope = AuditEnvelope::from_json(&body, path)
            .unwrap_or_else(|r| panic!("event {seq} violates the catalog: {r}"));
        assert_eq!(envelope.id(), event_id);
        if path == Origin::Relay {
            relayed += 1;
        }
        prev = expected;
    }
    relayed
}

// ---------------------------------------------------------------------------
// Test doubles around the real Store
// ---------------------------------------------------------------------------

type IngestHook =
    Box<dyn Fn(Uuid, &Result<IngestReceipt, StoreError>) -> Option<StoreError> + Send + Sync>;

/// Wraps the real Store: optional probe bypass, an ingest hook that may
/// replace the real result (e.g. commit, then report an outage), failing
/// control recording, and a log of regression reports (forwarded to the
/// real Store).
pub struct WrappedStore {
    pub inner: Arc<dyn RelayStore>,
    pub bypass_probe: bool,
    pub fail_before: Mutex<Vec<(Uuid, StoreError)>>,
    pub hook: Option<IngestHook>,
    pub calls: Mutex<Vec<Uuid>>,
    pub fail_control: AtomicBool,
    pub reports: Mutex<Vec<ReceiptIdentity>>,
}

impl WrappedStore {
    pub fn new(inner: Arc<dyn RelayStore>) -> Self {
        Self {
            inner,
            bypass_probe: false,
            fail_before: Mutex::new(Vec::new()),
            hook: None,
            calls: Mutex::new(Vec::new()),
            fail_control: AtomicBool::new(false),
            reports: Mutex::new(Vec::new()),
        }
    }

    pub fn calls_for(&self, id: Uuid) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| **c == id)
            .count()
    }
}

impl AuditStore for WrappedStore {
    fn ingest<'a>(
        &'a self,
        envelope: &'a AuditEnvelope,
    ) -> BoxFuture<'a, Result<IngestReceipt, StoreError>> {
        Box::pin(async move {
            let id = envelope.id();
            self.calls.lock().unwrap().push(id);
            let injected = self
                .fail_before
                .lock()
                .unwrap()
                .iter()
                .find(|(target, _)| *target == id)
                .map(|(_, error)| error.clone());
            if let Some(error) = injected {
                return Err(error);
            }
            let result = self.inner.ingest(envelope).await;
            if let Some(hook) = &self.hook
                && let Some(replacement) = hook(id, &result)
            {
                return Err(replacement);
            }
            result
        })
    }

    fn probe<'a>(
        &'a self,
        expected: &'a ProbeExpectation,
    ) -> BoxFuture<'a, Result<StoreStatus, StoreError>> {
        Box::pin(async move {
            if self.bypass_probe {
                return Ok(StoreStatus {
                    head_seq: i64::MAX,
                    recovery_epoch: 1,
                    state: StoreState::Operational,
                    missing_types: Vec::new(),
                    regression_detected: false,
                    last_verified_seq: None,
                });
            }
            self.inner.probe(expected).await
        })
    }

    fn lookup_receipts<'a>(
        &'a self,
        event_ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>> {
        self.inner.lookup_receipts(event_ids)
    }

    fn list_source_receipts<'a>(
        &'a self,
        source: &'a str,
        after_seq: i64,
        limit: u32,
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>> {
        self.inner.list_source_receipts(source, after_seq, limit)
    }

    fn lookup_control_receipts<'a>(
        &'a self,
        seqs: &'a [i64],
    ) -> BoxFuture<'a, Result<Vec<ControlReceiptRow>, StoreError>> {
        self.inner.lookup_control_receipts(seqs)
    }

    fn record_relay_control<'a>(
        &'a self,
        control: &'a RelayControl,
    ) -> BoxFuture<'a, Result<ControlReceipt, StoreError>> {
        if self.fail_control.load(Ordering::SeqCst) {
            return Box::pin(async { Err(StoreError::outage(OutageCode::Connection)) });
        }
        self.inner.record_relay_control(control)
    }

    fn report_regression<'a>(
        &'a self,
        identity: &'a ReceiptIdentity,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        self.reports.lock().unwrap().push(*identity);
        self.inner.report_regression(identity)
    }
}

impl RelayStore for WrappedStore {
    fn store_status(&self) -> BoxFuture<'_, Result<StoreStatusRow, StoreError>> {
        self.inner.store_status()
    }

    fn lookup_lost_ranges(&self) -> BoxFuture<'_, Result<Vec<LostRange>, StoreError>> {
        self.inner.lookup_lost_ranges()
    }
}

/// The relay of a newer deploy: ingests through `audit_store.ingest` with
/// `data.provenance.adapter_version` rewritten to a version this audit-core
/// does not know (an `AuditEnvelope` can only carry the catalog's version).
/// Everything else goes to the real Store.
pub struct ReprojectingStore {
    pub inner: Arc<PostgresAuditStore>,
    pub adapter_version: i32,
}

impl AuditStore for ReprojectingStore {
    fn ingest<'a>(
        &'a self,
        envelope: &'a AuditEnvelope,
    ) -> BoxFuture<'a, Result<IngestReceipt, StoreError>> {
        Box::pin(async move {
            let mut value = envelope.as_value().clone();
            value["data"]["provenance"]["adapter_version"] = json!(self.adapter_version);
            let row = sqlx::query(
                "SELECT status, seq, envelope_digest, adapter_version, code \
                 FROM audit_store.ingest($1::text::jsonb)",
            )
            .bind(value.to_string())
            .fetch_one(self.inner.pool())
            .await
            .map_err(|error| audit_store_postgres::classify_sqlx_error(&error))?;
            IngestRow {
                status: row.get("status"),
                seq: row.get("seq"),
                envelope_digest: row.get("envelope_digest"),
                adapter_version: row.get("adapter_version"),
                code: row.get("code"),
            }
            .into_result()
        })
    }

    fn probe<'a>(
        &'a self,
        expected: &'a ProbeExpectation,
    ) -> BoxFuture<'a, Result<StoreStatus, StoreError>> {
        self.inner.probe(expected)
    }

    fn lookup_receipts<'a>(
        &'a self,
        event_ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>> {
        self.inner.lookup_receipts(event_ids)
    }

    fn list_source_receipts<'a>(
        &'a self,
        source: &'a str,
        after_seq: i64,
        limit: u32,
    ) -> BoxFuture<'a, Result<Vec<ReceiptRow>, StoreError>> {
        self.inner.list_source_receipts(source, after_seq, limit)
    }

    fn lookup_control_receipts<'a>(
        &'a self,
        seqs: &'a [i64],
    ) -> BoxFuture<'a, Result<Vec<ControlReceiptRow>, StoreError>> {
        self.inner.lookup_control_receipts(seqs)
    }

    fn record_relay_control<'a>(
        &'a self,
        control: &'a RelayControl,
    ) -> BoxFuture<'a, Result<ControlReceipt, StoreError>> {
        self.inner.record_relay_control(control)
    }

    fn report_regression<'a>(
        &'a self,
        identity: &'a ReceiptIdentity,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        self.inner.report_regression(identity)
    }
}

/// Registers an adapter version of one Document type in the Store (what a
/// Store migration does).
pub async fn register_type(store_admin: &PgPool, event_type: &str, adapter_version: i32) {
    exec(
        store_admin,
        &format!(
            "SET audit_store.write_context = 'migration'; \
             INSERT INTO audit_store.registered_types VALUES \
                 ('{SOURCE}', '{event_type}', {adapter_version}, '{SOURCE_FORMAT}'); \
             RESET audit_store.write_context;"
        ),
    )
    .await;
}
