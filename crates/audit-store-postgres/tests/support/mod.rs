//! Disposable PostgreSQL 18.6 Audit Store for integration tests.
//!
//! Each test gets its own container (or, with `AUDIT_STORE_TEST_DATABASE_URL`
//! pointing at a disposable superuser server, its own database). The store is
//! migrated by the superuser, then `sql/roles.sql` and `sql/privileges.sql`
//! are applied. Role-path tests connect as synthetic LOGIN roles created
//! here, never as the superuser. All data is synthetic.
#![allow(dead_code)]

use std::str::FromStr;
use std::time::Duration;

use audit_core::catalog::DOCUMENT_SOURCE;
use audit_core::{AuditEnvelope, DocumentStagingProjection, Origin, chain_next, project};
use audit_store_postgres::admin::AuditAdmin;
use audit_store_postgres::{PRIVILEGES_SQL, ROLES_SQL, migrate};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, ConnectOptions, Connection, PgConnection, PgPool, Row};
use testcontainers::core::{ExecCommand, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use uuid::Uuid;

pub const PASSWORD: &str = "synthetic-test-password";
pub const ISSUER: &str = "synthetic-idp";
pub const STORE_DB: &str = "audit_store_test";

pub struct TestDb {
    pub container: Option<ContainerAsync<GenericImage>>,
    external_admin: Option<String>,
    pub host: String,
    pub port: u16,
    pub database: String,
    /// Superuser pool: migration, role setup and tamper simulation only.
    pub admin: PgPool,
    suffix: String,
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let Some(admin_url) = self.external_admin.clone() else {
            return;
        };
        let database = self.database.clone();
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(async move {
                if let Ok(mut admin) = PgConnection::connect(&admin_url).await {
                    let statement = format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)");
                    let _ = sqlx::query(AssertSqlSafe(statement))
                        .execute(&mut admin)
                        .await;
                }
            });
        })
        .join();
    }
}

/// A synthetic LOGIN role and its pool.
pub struct Login {
    pub role: String,
    pub url: String,
    pub pool: PgPool,
}

impl Login {
    /// Operator client (refuses privileged sessions).
    pub async fn admin(&self) -> AuditAdmin {
        AuditAdmin::connect(self.pool.clone())
            .await
            .expect("operator session")
    }

    /// Owner-member client (bootstrap, bind, unbind).
    pub async fn owner(&self) -> AuditAdmin {
        AuditAdmin::connect_owner(self.pool.clone())
            .await
            .expect("owner session")
    }
}

async fn connect_with_retry(url: &str) -> PgPool {
    let mut last = None;
    for _ in 0..60 {
        match PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(10))
            .connect(url)
            .await
        {
            Ok(pool) => return pool,
            Err(error) => {
                last = Some(error);
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
    panic!("PostgreSQL did not accept connections: {last:?}");
}

impl TestDb {
    /// A migrated store with capability roles and privileges applied.
    pub async fn start() -> Self {
        let db = Self::bare().await;
        migrate(&db.admin).await.expect("migration");
        db.exec(ROLES_SQL).await;
        db.exec(PRIVILEGES_SQL).await;
        db
    }

    /// An empty database (no migration).
    pub async fn bare() -> Self {
        let suffix = Uuid::now_v7().simple().to_string()[20..].to_owned();
        if let Ok(admin_url) = std::env::var("AUDIT_STORE_TEST_DATABASE_URL") {
            let database = format!("audit_store_test_{suffix}");
            let mut admin = PgConnection::connect(&admin_url)
                .await
                .expect("AUDIT_STORE_TEST_DATABASE_URL must be a disposable superuser server");
            sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {database}")))
                .execute(&mut admin)
                .await
                .expect("create test database");
            admin.close().await.expect("close");
            let options = PgConnectOptions::from_str(&admin_url).expect("url");
            let host = options.get_host().to_owned();
            let port = options.get_port();
            let url = options.database(&database).to_url_lossy().to_string();
            let pool = connect_with_retry(&url).await;
            return Self {
                container: None,
                external_admin: Some(admin_url),
                host,
                port,
                database,
                admin: pool,
                suffix,
            };
        }
        let container = GenericImage::new("postgres", "18.6-bookworm")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", "postgres")
            .with_env_var("POSTGRES_DB", STORE_DB)
            .with_cmd(["postgres", "-c", "fsync=off", "-c", "full_page_writes=off"])
            .start()
            .await
            .expect("disposable PostgreSQL 18.6 should start");
        let port = container
            .get_host_port_ipv4(5432.tcp())
            .await
            .expect("mapped port");
        let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/{STORE_DB}");
        let pool = connect_with_retry(&url).await;
        // The image restarts the server after init; wait for the final one.
        loop {
            if sqlx::query("SELECT 1").execute(&pool).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Self {
            container: Some(container),
            external_admin: None,
            host: "127.0.0.1".to_owned(),
            port,
            database: STORE_DB.to_owned(),
            admin: pool,
            suffix,
        }
    }

    pub fn url(&self, role: &str, database: &str) -> String {
        format!(
            "postgres://{role}:{PASSWORD}@{}:{}/{database}",
            self.host, self.port
        )
    }

    pub fn superuser_url(&self, database: &str) -> String {
        match &self.external_admin {
            Some(admin) => PgConnectOptions::from_str(admin)
                .expect("url")
                .database(database)
                .to_url_lossy()
                .to_string(),
            None => format!(
                "postgres://postgres:postgres@{}:{}/{database}",
                self.host, self.port
            ),
        }
    }

    /// Runs a multi-statement script as the superuser.
    pub async fn exec(&self, sql: &str) {
        sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
            .execute(&self.admin)
            .await
            .unwrap_or_else(|error| panic!("script failed: {error}"));
    }

    pub fn role_name(&self, base: &str) -> String {
        format!("{base}_{}", self.suffix)
    }

    /// Creates a LOGIN role with capability roles, re-applies privileges
    /// (timeouts), and connects as it.
    pub async fn login(&self, base: &str, roles: &[&str]) -> Login {
        let role = self.role_name(base);
        self.exec(&format!(
            "CREATE ROLE {role} LOGIN PASSWORD '{PASSWORD}' NOSUPERUSER NOCREATEDB NOCREATEROLE"
        ))
        .await;
        for capability in roles {
            self.exec(&format!("GRANT {capability} TO {role}")).await;
        }
        self.exec(PRIVILEGES_SQL).await;
        let url = self.url(&role, &self.database);
        let pool = connect_with_retry(&url).await;
        Login { role, url, pool }
    }

    /// A LOGIN role that is a member of audit_store_owner (DBA path).
    pub async fn owner_login(&self, base: &str) -> Login {
        self.login(base, &["audit_store_owner"]).await
    }

    /// Validates every control event in the store with the audit-core
    /// catalog on its own origin path, and cross-checks every digest and
    /// chain value against audit-core (design §4.5, §7.2).
    pub async fn assert_store_conforms(&self) {
        assert_store_conforms(&self.admin).await;
    }

    /// Runs a command inside the container (docker mode only).
    pub async fn docker_exec(&self, command: &[&str]) -> (i64, String) {
        let container = self.container.as_ref().expect("docker mode");
        let mut result = container
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

/// See [`TestDb::assert_store_conforms`].
pub async fn assert_store_conforms(pool: &PgPool) -> usize {
    let rows = sqlx::query(
        "SELECT e.seq, e.event_id, e.origin, e.envelope_digest, e.prev_chain, e.chain, \
                b.envelope::text AS body \
         FROM audit_store.events AS e LEFT JOIN audit_store.event_bodies AS b ON b.seq = e.seq \
         ORDER BY e.seq",
    )
    .fetch_all(pool)
    .await
    .expect("read events");
    let mut prev = audit_core::GENESIS;
    let mut controls = 0;
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
        let expected = chain_next(&prev, seq, event_id, &digest);
        assert_eq!(
            chain,
            expected.to_vec(),
            "seq {seq}: SQL chain equals audit_core::chain_next"
        );
        if let Some(body) = &body {
            assert_eq!(
                audit_core::envelope_digest(body),
                digest,
                "seq {seq}: digest"
            );
        }
        let path = Origin::parse(&origin).expect("origin");
        if path != Origin::Relay {
            let body = body.unwrap_or_else(|| panic!("control event {seq} keeps its body"));
            let envelope = AuditEnvelope::from_json(&body, path)
                .unwrap_or_else(|r| panic!("control event {seq} violates the catalog: {r}"));
            assert_eq!(envelope.id(), event_id);
            controls += 1;
        }
        prev = expected;
    }
    controls
}

pub fn hex(bytes: &[u8]) -> String {
    audit_core::chain::to_hex(bytes)
}

/// A projected `document.created` envelope (synthetic).
pub fn document_created(
    event_id: Uuid,
    document_id: Uuid,
    occurred_at: &str,
    commitment: u8,
) -> AuditEnvelope {
    project(&row(
        event_id,
        "document.created",
        &format!("document/{document_id}"),
        document_id,
        None,
        occurred_at,
        json!({"documentId": document_id.to_string()}),
        commitment,
    ))
    .expect("synthetic document.created projects")
}

/// A projected `document.version.read_confirmed` envelope (DATA_ACCESS).
pub fn read_confirmed(
    event_id: Uuid,
    document_id: Uuid,
    occurred_at: &str,
    commitment: u8,
) -> AuditEnvelope {
    let version = Uuid::now_v7();
    project(&row(
        event_id,
        "document.version.read_confirmed",
        &format!("document/{document_id}"),
        document_id,
        Some(version),
        occurred_at,
        json!({"document_version_id": version.to_string()}),
        commitment,
    ))
    .expect("synthetic read_confirmed projects")
}

#[allow(clippy::too_many_arguments)]
fn row(
    event_id: Uuid,
    event_type: &str,
    subject: &str,
    document_id: Uuid,
    version: Option<Uuid>,
    occurred_at: &str,
    data: Value,
    commitment: u8,
) -> DocumentStagingProjection {
    DocumentStagingProjection {
        event_id: event_id.to_string(),
        event_type: event_type.to_owned(),
        source: DOCUMENT_SOURCE.to_owned(),
        subject: subject.to_owned(),
        actor_identity_provider: "poc".to_owned(),
        actor_principal_id: "synthetic-human".to_owned(),
        resource_type: "Document".to_owned(),
        resource_id: document_id.to_string(),
        resource_version_id: version.map(|v| v.to_string()),
        result: "success".to_owned(),
        trace_id: None,
        occurred_at: occurred_at.to_owned(),
        oversize: false,
        data: Some(data),
        data_kind: "object".to_owned(),
        reason_kind: None,
        reason_bytes: None,
        source_intact: true,
        source_commitment: hex(&[commitment; 32]),
        registration_kind: "trigger".to_owned(),
    }
}

pub const OCCURRED: &str = "2026-10-01T00:00:00.000000Z";

/// The standard cast: a DBA, a bootstrapped administrator, a second
/// administrator, and bound principals for each capability.
pub struct Cast {
    pub dba: Login,
    pub admin: Login,
    pub admin2: Login,
    pub reader: Login,
    pub verifier: Login,
    pub maintainer: Login,
    pub relay: Login,
}

impl Cast {
    pub async fn new(db: &TestDb) -> Self {
        let dba = db.owner_login("dba").await;
        let admin = db
            .login("admin", &["audit_store_admin", "audit_store_reader"])
            .await;
        let admin2 = db.login("admin2", &["audit_store_admin"]).await;
        let reader = db.login("reader", &["audit_store_reader"]).await;
        let verifier = db.login("verifier", &["audit_store_verifier"]).await;
        let maintainer = db.login("maintainer", &["audit_store_maintainer"]).await;
        let relay = db
            .login(
                "relay",
                &["audit_store_ingest", "audit_store_relay_control"],
            )
            .await;
        let owner = dba.owner().await;
        owner
            .bootstrap_administrator(&admin.role, ISSUER, "admin-1")
            .await
            .expect("bootstrap");
        for (login, principal) in [
            (&admin2, "admin-2"),
            (&reader, "reader-1"),
            (&verifier, "verifier-1"),
            (&maintainer, "maintainer-1"),
        ] {
            owner
                .bind_principal(&login.role, ISSUER, principal)
                .await
                .expect("bind");
        }
        owner
            .bind_principal(&relay.role, "service", "audit-relay")
            .await
            .expect("bind relay");
        let administrator = admin.admin().await;
        for (principal, capability) in [
            ("admin-2", "administer"),
            ("reader-1", "investigate"),
            ("reader-1", "export"),
            ("verifier-1", "verify"),
            ("maintainer-1", "maintain"),
        ] {
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
            dba,
            admin,
            admin2,
            reader,
            verifier,
            maintainer,
            relay,
        }
    }
}

/// Ingests through the SQL function directly (bypassing the Rust
/// validation) and returns `(status, seq, code)`.
pub async fn sql_ingest(pool: &PgPool, envelope: &Value) -> (String, Option<i64>, Option<String>) {
    let row = sqlx::query("SELECT status, seq, code FROM audit_store.ingest($1::text::jsonb)")
        .bind(envelope.to_string())
        .fetch_one(pool)
        .await
        .expect("ingest call");
    (row.get("status"), row.get("seq"), row.get("code"))
}

/// The SQLSTATE of a failed query.
pub fn sqlstate(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(db) => db.code().map(|c| c.into_owned()).unwrap_or_default(),
        other => panic!("expected a database error, got {other:?}"),
    }
}

/// Head (last_seq, last_chain) as seen by the superuser.
pub async fn head(pool: &PgPool) -> (i64, Vec<u8>, i64) {
    let row = sqlx::query(
        "SELECT last_seq, last_chain, recovery_epoch FROM audit_store.publication_head",
    )
    .fetch_one(pool)
    .await
    .expect("head");
    (
        row.get("last_seq"),
        row.get("last_chain"),
        row.get("recovery_epoch"),
    )
}

/// Control events of `event_type`, as `(seq, details)`.
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
