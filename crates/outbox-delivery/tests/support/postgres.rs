//! Disposable PostgreSQL 18.6 for outbox integration tests.
//!
//! CI uses the existing testcontainer image. A cloud run may pass
//! `P6_TEST_DATABASE_URL` pointing at a disposable PostgreSQL server with
//! CREATEDB, and each test then gets a distinct database on that server.

use std::str::FromStr;

use sqlx::{
    ConnectOptions, Connection, PgConnection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

pub enum DatabaseGuard {
    Docker {
        _container: Box<testcontainers::ContainerAsync<GenericImage>>,
    },
    External {
        admin_url: String,
        database: String,
    },
}

impl Drop for DatabaseGuard {
    fn drop(&mut self) {
        let DatabaseGuard::External {
            admin_url,
            database,
        } = self
        else {
            return;
        };
        let admin_url = admin_url.clone();
        let database = database.clone();
        let result = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let mut admin = PgConnection::connect(&admin_url).await?;
                let statement = format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)");
                sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                    .execute(&mut admin)
                    .await?;
                admin.close().await
            })
        })
        .join();
        if !matches!(result, Ok(Ok(()))) {
            eprintln!("P6 test database cleanup failed");
        }
    }
}

pub async fn postgres(name: &str) -> (DatabaseGuard, PgPool, String) {
    if let Ok(admin_url) = std::env::var("P6_TEST_DATABASE_URL") {
        let database = format!("p6_test_{}", Uuid::now_v7().simple());
        let mut admin = PgConnection::connect(&admin_url)
            .await
            .expect("P6_TEST_DATABASE_URL requires PostgreSQL 18.6 with CREATEDB");
        let statement = format!("CREATE DATABASE {database}");
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(&mut admin)
            .await
            .expect("P6 test database must be creatable");
        admin.close().await.unwrap();
        let options = PgConnectOptions::from_str(&admin_url)
            .unwrap()
            .database(&database);
        let url = options.to_url_lossy().to_string();
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap();
        return (
            DatabaseGuard::External {
                admin_url,
                database,
            },
            pool,
            url,
        );
    }

    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", name)
        .start()
        .await
        .expect("disposable PostgreSQL should start");
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/{name}");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    (
        DatabaseGuard::Docker {
            _container: Box::new(container),
        },
        pool,
        url,
    )
}

// G07-only pure reducers, implemented after the recorded six-contract RED.
// These have no connection to postgres(), Docker, or its networked destructor.
// Real observations/effects and the owned runtime entrypoint remain unwired.
pub mod g07_owned {
    use std::{
        ffi::{OsStr, OsString},
        path::{Path, PathBuf},
    };

    use serde_json::Value;
    use uuid::Uuid;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct OwnedPgScope {
        manifest: Value,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PathFact {
        pub field: &'static str,
        pub canonical: PathBuf,
        pub has_symlink_component: bool,
        pub uid: u32,
        pub mode: u32,
        pub kind: PathKind,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PathKind {
        Directory,
        RegularFile,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ProcessIdentity {
        pub pid: u32,
        pub start_ticks: u64,
        pub uid: u32,
        pub executable: PathBuf,
        pub executable_sha256: String,
        pub executable_dev: u64,
        pub executable_inode: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct OwnedScopeFacts {
        pub expected_run_root: PathBuf,
        pub current_uid: u32,
        pub current_os_user: String,
        pub paths: Vec<PathFact>,
        pub server: ProcessIdentity,
        pub offline_system_identifier: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum FixtureError {
        ConfigurationRejected,
        IdentityMismatch,
    }

    // The real observer must supply canonical/symlink/uid/executable/process
    // facts to this same reducer before any connection or DDL. A JSON marker
    // alone cannot provide ownership evidence. No environment is read here.
    pub fn validate_owned_scope(
        scope_path: Option<&OsStr>,
        document: &str,
        facts: &OwnedScopeFacts,
        ambient: &[(OsString, OsString)],
    ) -> Result<OwnedPgScope, FixtureError> {
        let reject = || FixtureError::ConfigurationRejected;
        let scope_path = scope_path
            .and_then(OsStr::to_str)
            .map(Path::new)
            .ok_or_else(reject)?;
        let root = &facts.expected_run_root;
        if !root.is_absolute()
            || scope_path != root.join("scope.json")
            || facts.current_uid != 1000
            || facts.current_os_user != "agent"
            || ambient
                .iter()
                .any(|(name, _)| name.to_string_lossy().starts_with("PG"))
        {
            return Err(reject());
        }
        let manifest: Value = serde_json::from_str(document).map_err(|_| reject())?;
        let object = manifest.as_object().ok_or_else(reject)?;
        let expected_paths = [
            ("run_root", root.clone(), PathKind::Directory),
            ("data_dir", root.join("data"), PathKind::Directory),
            ("socket_dir", root.join("socket"), PathKind::Directory),
            ("home_dir", root.join("home"), PathKind::Directory),
            ("tmp_dir", root.join("tmp"), PathKind::Directory),
            (
                "config_file",
                root.join("data/postgresql.conf"),
                PathKind::RegularFile,
            ),
            (
                "hba_file",
                root.join("data/pg_hba.conf"),
                PathKind::RegularFile,
            ),
            (
                "postgres_executable",
                root.join("install/bin/postgres"),
                PathKind::RegularFile,
            ),
        ];
        let other_fields = [
            "schema_version",
            "run_id",
            "postgres_executable_sha256",
            "server_pid",
            "server_start_ticks",
            "server_uid",
            "server_executable_dev",
            "server_executable_inode",
            "os_user",
            "database_user",
            "admin_database",
            "port",
            "server_version_num",
            "offline_system_identifier",
            "listen_addresses",
        ];
        if object.len() != expected_paths.len() + other_fields.len()
            || object.keys().any(|field| {
                !other_fields.contains(&field.as_str())
                    && !expected_paths
                        .iter()
                        .any(|(known, _, _)| field.as_str() == *known)
            })
            || facts.paths.len() != expected_paths.len()
        {
            return Err(reject());
        }
        for (field, expected_path, kind) in &expected_paths {
            let mut matching = facts.paths.iter().filter(|fact| fact.field == *field);
            let fact = matching.next().ok_or_else(reject)?;
            let permissions = fact.mode & 0o7777;
            let mode_allowed = match *field {
                "postgres_executable" => permissions & 0o100 != 0 && permissions & 0o7022 == 0,
                "config_file" | "hba_file" => permissions == 0o600,
                _ => permissions == 0o700,
            };
            if matching.next().is_some()
                || object.get(*field).and_then(Value::as_str) != expected_path.to_str()
                || fact.canonical != *expected_path
                || fact.has_symlink_component
                || fact.uid != facts.current_uid
                || fact.kind != *kind
                || !mode_allowed
            {
                return Err(reject());
            }
        }
        let server = &facts.server;
        let hash = &server.executable_sha256;
        let system_identifier = &facts.offline_system_identifier;
        let numbers = [
            ("schema_version", 1),
            ("port", 5432),
            ("server_version_num", 180006),
            ("server_pid", u64::from(server.pid)),
            ("server_start_ticks", server.start_ticks),
            ("server_uid", u64::from(server.uid)),
            ("server_executable_dev", server.executable_dev),
            ("server_executable_inode", server.executable_inode),
        ];
        let strings = [
            ("os_user", "agent"),
            ("database_user", "agent"),
            ("admin_database", "postgres"),
            ("listen_addresses", ""),
            ("postgres_executable_sha256", hash.as_str()),
            ("offline_system_identifier", system_identifier.as_str()),
        ];
        if server.pid == 0
            || server.start_ticks == 0
            || server.uid != facts.current_uid
            || server.executable_dev == 0
            || server.executable_inode == 0
            || server.executable != root.join("install/bin/postgres")
            || hash.len() != 64
            || !hash.bytes().all(lower_hex)
            || !positive_decimal(system_identifier)
            || numbers.iter().any(|(field, expected)| {
                object.get(*field).and_then(Value::as_u64) != Some(*expected)
            })
            || strings.iter().any(|(field, expected)| {
                object.get(*field).and_then(Value::as_str) != Some(*expected)
            })
            || object
                .get("run_id")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                .is_none_or(|id| id.is_nil())
        {
            return Err(reject());
        }
        Ok(OwnedPgScope { manifest })
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DatabaseIdentity {
        pub cluster_system_identifier: String,
        pub name: String,
        pub oid: u32,
        pub owner: String,
    }

    pub fn checked_drop_statement(
        expected: &DatabaseIdentity,
        observed: &DatabaseIdentity,
    ) -> Result<String, FixtureError> {
        if expected != observed || !valid_database_identity(expected) {
            return Err(FixtureError::IdentityMismatch);
        }
        Ok(format!("DROP DATABASE {}", expected.name))
    }

    fn lower_hex(byte: u8) -> bool {
        byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
    }

    fn positive_decimal(value: &str) -> bool {
        !value.starts_with('0')
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.parse::<u64>().is_ok_and(|number| number > 0)
    }

    fn valid_database_identity(identity: &DatabaseIdentity) -> bool {
        identity.oid > 0
            && identity.owner == "agent"
            && positive_decimal(&identity.cluster_system_identifier)
            && identity
                .name
                .strip_prefix("p6_g07_")
                .and_then(|name| name.split_once('_'))
                .is_some_and(|(run, database)| {
                    run.len() == 16
                        && database.len() == 32
                        && run.bytes().all(lower_hex)
                        && database.bytes().all(lower_hex)
                })
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DdlOperation {
        Create,
        Drop,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DdlResponse {
        Confirmed,
        Unknown,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DatabaseResolution {
        Ready(DatabaseIdentity),
        Dropped,
        Unknown,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DdlDecision {
        pub resolution: DatabaseResolution,
        pub retry: bool,
        pub cleanup_target: Option<DatabaseIdentity>,
    }

    pub fn classify_ddl_response(
        operation: DdlOperation,
        response: DdlResponse,
        confirmed_identity: Option<&DatabaseIdentity>,
    ) -> DdlDecision {
        let identity = confirmed_identity.filter(|identity| valid_database_identity(identity));
        let (resolution, cleanup_target) = match (response, identity, operation) {
            (DdlResponse::Confirmed, Some(identity), DdlOperation::Create) => (
                DatabaseResolution::Ready(identity.clone()),
                Some(identity.clone()),
            ),
            (DdlResponse::Confirmed, Some(_), DdlOperation::Drop) => {
                (DatabaseResolution::Dropped, None)
            }
            _ => (DatabaseResolution::Unknown, None),
        };
        DdlDecision {
            resolution,
            retry: false,
            cleanup_target,
        }
    }
}
