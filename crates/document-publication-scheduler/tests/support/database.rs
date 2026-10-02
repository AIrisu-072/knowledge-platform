use std::str::FromStr;

use sqlx::{PgPool, postgres::PgConnectOptions, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

pub struct TestDatabase {
    pub pool: PgPool,
    admin: PgPool,
    name: String,
    _container: Option<testcontainers::ContainerAsync<GenericImage>>,
}

impl TestDatabase {
    pub async fn new() -> Self {
        // An explicit URL is only for a disposable PostgreSQL instance. Otherwise
        // create a disposable container, as the repository's existing suites do.
        // Never read runtime DATABASE_URL/KP_DATABASE_URL or skip a missing DB.
        let (options, container) = match std::env::var("TEST_DATABASE_URL") {
            Ok(url) => (
                PgConnectOptions::from_str(&url)
                    .unwrap_or_else(|_| panic!("TEST_DATABASE_URL must be a valid PostgreSQL URL")),
                None,
            ),
            Err(std::env::VarError::NotPresent) => {
                let container = GenericImage::new("postgres", "18.6-bookworm")
                    .with_exposed_port(5432.tcp())
                    // pg_ctl redirects the socket-only initialization server's
                    // stderr to stdout. The final directly executed TCP server
                    // emits one readiness message on stderr, not two.
                    .with_wait_for(WaitFor::message_on_stderr(
                        "database system is ready to accept connections",
                    ))
                    .with_env_var("POSTGRES_USER", "postgres")
                    .with_env_var("POSTGRES_PASSWORD", "postgres")
                    .with_env_var("POSTGRES_DB", "scheduler_test_admin")
                    .start()
                    .await
                    .expect("disposable PostgreSQL is required; this test cannot be skipped");
                let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
                let options = PgConnectOptions::new()
                    .host("127.0.0.1")
                    .port(port)
                    .username("postgres")
                    .password("postgres")
                    .database("scheduler_test_admin");
                (options, Some(container))
            }
            Err(std::env::VarError::NotUnicode(_)) => {
                panic!("TEST_DATABASE_URL must be a valid PostgreSQL URL")
            }
        };
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap_or_else(|_| panic!("the disposable PostgreSQL instance must be reachable"));
        let name = format!("document_scheduler_test_{}", Uuid::now_v7().simple());
        sqlx::query(sqlx::AssertSqlSafe(
            format!("CREATE DATABASE {name}").as_str(),
        ))
        .execute(&admin)
        .await
        .expect("create isolated disposable test database");
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(options.database(&name))
            .await
            .unwrap_or_else(|_| panic!("the isolated test database must be reachable"));
        Self {
            pool,
            admin,
            name,
            _container: container,
        }
    }

    pub async fn close(self) {
        self.pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(
            format!("DROP DATABASE {}", self.name).as_str(),
        ))
        .execute(&self.admin)
        .await
        .expect("drop this test's isolated database");
        self.admin.close().await;
    }
}
