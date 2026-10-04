use std::str::FromStr;

use sqlx::{
    Connection, PgConnection, PgPool,
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
            eprintln!("external Search test database cleanup failed");
        }
    }
}

pub async fn postgres(name: &str) -> (DatabaseGuard, PgPool, PgConnectOptions) {
    if let Ok(admin_url) = std::env::var("SEARCH_TEST_DATABASE_URL") {
        let database = format!("search_test_{}", Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&admin_url)
            .await
            .expect("SEARCH_TEST_DATABASE_URL must connect to PostgreSQL 18.6 with CREATEDB");
        let statement =
            format!("CREATE DATABASE {database} WITH ENCODING 'UTF8' TEMPLATE template0");
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(&mut admin)
            .await
            .expect("external test database must be creatable");
        admin.close().await.unwrap();
        let options = PgConnectOptions::from_str(&admin_url)
            .unwrap()
            .database(&database);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options.clone())
            .await
            .unwrap();
        return (
            DatabaseGuard::External {
                admin_url,
                database,
            },
            pool,
            options,
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
        PgConnectOptions::from_str(&url).unwrap(),
    )
}
