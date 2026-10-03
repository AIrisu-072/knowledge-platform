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
