#[path = "support/database.rs"]
mod database;
#[path = "support/runtime.rs"]
mod runtime;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
    routing::get,
};
use database::TestDatabase;
use document_repository_postgres::migrate;
use document_server::{
    composition::runtime_router,
    health::{RuntimeHealth, health_router, readiness},
    web::web_router,
};
use runtime::Files;
use std::sync::Arc;
use tower::ServiceExt;

async fn request(router: &Router, method: Method, path: &str) -> (StatusCode, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("accept", "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    (
        response.status(),
        to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
}

#[tokio::test]
async fn liveness_is_independent_and_readiness_errors_are_status_only() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://synthetic-secret:synthetic-secret@127.0.0.1:1/synthetic-secret")
        .unwrap();
    pool.close().await;
    let files = Files::new(&pool);
    let config = files.config();
    let health = Arc::new(RuntimeHealth::new(pool, config.serve().unwrap().clone()));
    let router = health_router(health);
    assert_eq!(
        request(&router, Method::GET, "/health/live").await,
        (StatusCode::OK, br#"{"status":"ok"}"#.to_vec())
    );
    assert_eq!(
        request(&router, Method::GET, "/health/ready").await,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            br#"{"status":"unavailable"}"#.to_vec()
        )
    );
}

#[tokio::test]
async fn readiness_rechecks_storage_workers_assets_schema_and_draining() {
    let database = TestDatabase::new().await;
    migrate(&database.pool).await.unwrap();
    let files = Files::new(&database.pool);
    let config = files.config();
    let health = RuntimeHealth::new(database.pool.clone(), config.serve().unwrap().clone());
    readiness(&health).await.unwrap();
    assert_eq!(
        std::fs::read_dir(files.root.path().join("storage"))
            .unwrap()
            .count(),
        0,
        "probe must remove only its owned temporary file"
    );
    for item in [
        "storage",
        "dsi",
        "diff",
        "web/assets/main.js",
        "web/index.html",
    ] {
        let path = files.root.path().join(item);
        let hidden = path.with_extension("temporarily-unavailable");
        std::fs::rename(&path, &hidden).unwrap();
        assert!(
            readiness(&health).await.is_err(),
            "readiness ignored {item}"
        );
        std::fs::rename(hidden, path).unwrap();
        readiness(&health).await.unwrap();
    }
    sqlx::query("UPDATE _sqlx_migrations SET success=false WHERE version=1")
        .execute(&database.pool)
        .await
        .unwrap();
    assert!(readiness(&health).await.is_err());
    sqlx::query("UPDATE _sqlx_migrations SET success=true WHERE version=1")
        .execute(&database.pool)
        .await
        .unwrap();
    readiness(&health).await.unwrap();
    health.begin_shutdown();
    assert!(readiness(&health).await.is_err());
    database.close().await;
}

#[tokio::test]
async fn static_routing_preserves_api_and_rejects_unsafe_or_missing_assets() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://127.0.0.1/synthetic")
        .unwrap();
    let files = Files::new(&pool);
    let config = files.config();
    std::fs::write(files.root.path().join("web/.env"), "synthetic-secret").unwrap();
    std::fs::write(
        files.root.path().join("web/assets/main.js.map"),
        "synthetic-source",
    )
    .unwrap();
    let web = web_router(&files.root.path().join("web")).unwrap();
    let health = Arc::new(RuntimeHealth::new(
        pool.clone(),
        config.serve().unwrap().clone(),
    ));
    let api = Router::new().route("/v1/known", get(|| async { "api-response" }));
    let router = runtime_router(api, health, Some(web));
    for path in ["/", "/documents/abc", "/folders/abc"] {
        let (status, body) = request(&router, Method::GET, path).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            String::from_utf8(body)
                .unwrap()
                .contains("synthetic-runtime-spa")
        );
    }
    assert_eq!(
        request(&router, Method::GET, "/v1/known").await,
        (StatusCode::OK, b"api-response".to_vec())
    );
    for path in [
        "/v1",
        "/v1/unknown",
        "/%76%31/unknown",
        "/v1%2funknown",
        "/%68ealth/unknown",
        "/health%2funknown",
        "/health/unknown",
        "/assets/missing.js",
        "/assets/missing",
        "/.env",
        "/assets/main.js.map",
        "/%2e%2e/secret",
        "/%252e%252e/secret",
        "/assets/%2f..%2f.env",
        "/assets/..%5c.env",
    ] {
        let (status, body) = request(&router, Method::GET, path).await;
        assert_ne!(status, StatusCode::OK, "{path}");
        let body = String::from_utf8_lossy(&body);
        assert!(!body.contains("synthetic-runtime-spa"));
        assert!(!body.contains("synthetic-secret"));
    }
    assert_eq!(
        request(&router, Method::POST, "/documents/abc").await.0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        request(&router, Method::HEAD, "/").await,
        (StatusCode::OK, vec![])
    );
    let agent = runtime_router(
        Router::new(),
        Arc::new(RuntimeHealth::new(pool, config.serve().unwrap().clone())),
        None,
    );
    assert_eq!(
        request(&agent, Method::GET, "/").await.0,
        StatusCode::NOT_FOUND
    );
}

#[cfg(unix)]
#[tokio::test]
async fn escaping_symlinks_and_invalid_dist_never_serve_secret() {
    use std::os::unix::fs::symlink;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://127.0.0.1/synthetic")
        .unwrap();
    let files = Files::new(&pool);
    let web = files.root.path().join("web");
    std::fs::write(files.root.path().join("secret.js"), "synthetic-secret").unwrap();
    symlink(
        files.root.path().join("secret.js"),
        web.join("assets/escape.js"),
    )
    .unwrap();
    let router = web_router(&web).unwrap();
    let (_, body) = request(&router, Method::GET, "/assets/escape.js").await;
    assert!(!String::from_utf8_lossy(&body).contains("synthetic-secret"));
    std::fs::remove_file(web.join("assets/main.js")).unwrap();
    assert!(web_router(&web).is_err());
    symlink(
        files.root.path().join("secret.js"),
        web.join("assets/main.js"),
    )
    .unwrap();
    assert!(web_router(&web).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn readiness_tracks_original_targets_when_config_aliases_are_retargeted() {
    use std::os::unix::fs::symlink;
    let database = TestDatabase::new().await;
    migrate(&database.pool).await.unwrap();
    for (key, leaf) in [
        ("KP_STORAGE_ROOT", "storage"),
        ("KP_DSI_WORKER", "dsi"),
        ("KP_DIFF_WORKER", "diff"),
        ("KP_WEB_DIST", "web"),
    ] {
        let mut files = Files::new(&database.pool);
        let alternate = Files::new(&database.pool);
        let alias = files.root.path().join("alias");
        symlink(files.root.path().join(leaf), &alias).unwrap();
        files
            .environment
            .0
            .insert(key.into(), alias.display().to_string());
        let config = files.config();
        let health = RuntimeHealth::new(database.pool.clone(), config.serve().unwrap().clone());
        readiness(&health).await.unwrap();
        std::fs::remove_file(&alias).unwrap();
        symlink(alternate.root.path().join(leaf), &alias).unwrap();
        let original = files.root.path().join(leaf);
        let moved = files.root.path().join("moved-original");
        std::fs::rename(&original, &moved).unwrap();
        assert!(
            readiness(&health).await.is_err(),
            "probe followed new alias rather than actual {key} adapter target"
        );
        std::fs::rename(moved, original).unwrap();
        readiness(&health).await.unwrap();
    }
    database.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn readiness_checks_current_callers_execution_rights() {
    use std::os::unix::fs::PermissionsExt;
    let database = TestDatabase::new().await;
    migrate(&database.pool).await.unwrap();
    let files = Files::new(&database.pool);
    let config = files.config();
    let health = RuntimeHealth::new(database.pool.clone(), config.serve().unwrap().clone());
    for worker in ["dsi", "diff"] {
        let path = files.root.path().join(worker);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o001)).unwrap();
        assert!(
            readiness(&health).await.is_err(),
            "another class execute bit is not caller execution authority"
        );
        Files::executable(&path, true);
        readiness(&health).await.unwrap();
    }
    database.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn rejected_extensionless_symlinks_cannot_become_spa_navigation() {
    use std::os::unix::fs::symlink;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://127.0.0.1/synthetic")
        .unwrap();
    let files = Files::new(&pool);
    let web = files.root.path().join("web");
    std::fs::write(files.root.path().join("secret"), "outside-secret").unwrap();
    std::fs::write(web.join(".secret"), "inside-secret").unwrap();
    std::fs::write(web.join("main.js.map"), "map-secret").unwrap();
    symlink(files.root.path().join("secret"), web.join("escape")).unwrap();
    symlink(web.join(".secret"), web.join("hidden-alias")).unwrap();
    symlink(web.join("main.js.map"), web.join("map-alias")).unwrap();
    symlink(files.root.path(), web.join("outside-directory")).unwrap();
    let router = web_router(&web).unwrap();
    for path in [
        "/escape",
        "/hidden-alias",
        "/map-alias",
        "/outside-directory/no-such-route",
    ] {
        let (status, body) = request(&router, Method::GET, path).await;
        assert_ne!(status, StatusCode::OK, "unsafe target became SPA: {path}");
        assert!(!String::from_utf8_lossy(&body).contains("synthetic-runtime-spa"));
    }
}
