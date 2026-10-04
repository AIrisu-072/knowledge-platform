#[path = "support/database.rs"]
mod database;
#[path = "support/runtime.rs"]
mod runtime;
use database::TestDatabase;
use document_repository_postgres::migrate;
use document_server::composition::{StartupError, compose_runtime};
use runtime::Files;

#[tokio::test]
async fn serve_fails_on_fresh_database_without_creating_schema() {
    let database = TestDatabase::new().await;
    let files = Files::new(&database.pool);
    assert!(matches!(
        compose_runtime(&files.config()).await,
        Err(StartupError::Schema)
    ));
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables WHERE table_schema='public'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(tables, 0);
    database.close().await;
}

#[tokio::test]
async fn missing_storage_and_missing_or_nonexecutable_workers_fail_startup() {
    let database = TestDatabase::new().await;
    migrate(&database.pool).await.unwrap();
    for (key, expected) in [
        ("KP_STORAGE_ROOT", StartupError::Storage),
        ("KP_DSI_WORKER", StartupError::DsiWorker),
        ("KP_DIFF_WORKER", StartupError::DiffWorker),
    ] {
        let mut files = Files::new(&database.pool);
        files.environment.0.insert(
            key.into(),
            files.root.path().join("missing").display().to_string(),
        );
        let result = compose_runtime(&files.config()).await;
        assert!(matches!(result,Err(error) if error==expected));
    }
    for (worker, expected) in [
        ("dsi", StartupError::DsiWorker),
        ("diff", StartupError::DiffWorker),
    ] {
        let files = Files::new(&database.pool);
        Files::executable(&files.root.path().join(worker), false);
        assert!(matches!(compose_runtime(&files.config()).await,Err(error) if error==expected));
    }
    database.close().await;
}

#[tokio::test]
async fn executable_is_not_enough_if_production_worker_preflight_fails() {
    let database = TestDatabase::new().await;
    migrate(&database.pool).await.unwrap();
    let files = Files::new(&database.pool);
    assert!(matches!(
        compose_runtime(&files.config()).await,
        Err(StartupError::DsiWorker)
    ));
    database.close().await;
}

#[tokio::test]
async fn unavailable_database_error_is_redacted() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://synthetic-secret:synthetic-secret@127.0.0.1:1/synthetic-secret")
        .unwrap();
    let files = Files::new(&pool);
    let error = match compose_runtime(&files.config()).await {
        Err(error) => error,
        Ok(_) => panic!("unavailable DB must prevent startup"),
    };
    assert_eq!(error, StartupError::Database);
    assert!(!format!("{error:?} {error}").contains("synthetic-secret"));
    assert!(!format!("{error:?} {error}").contains("127.0.0.1"));
}
