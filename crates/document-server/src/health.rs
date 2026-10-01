//! Operational probes return only safe component categories internally and status externally.
use crate::{config::ServeConfig, web::validate_dist};
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use document_repository_postgres::check_schema_compatibility;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct RuntimeHealth {
    pool: PgPool,
    config: ServeConfig,
    draining: AtomicBool,
}
impl RuntimeHealth {
    pub fn new(pool: PgPool, config: ServeConfig) -> Self {
        Self {
            pool,
            config: config.capture_targets(),
            draining: AtomicBool::new(false),
        }
    }
    pub(crate) fn targets(&self) -> &ServeConfig {
        &self.config
    }
    pub fn begin_shutdown(&self) {
        self.draining.store(true, Ordering::SeqCst);
    }
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn for_drain_test(pool: PgPool) -> Self {
        struct Environment;
        impl crate::config::ConfigSource for Environment {
            fn get(&self, name: &str) -> Option<String> {
                Some(
                    match name {
                        "KP_RUNTIME_MODE" => "poc",
                        "KP_IDENTITY_PROFILE" => "poc-agent",
                        "KP_DATABASE_URL" => "postgres://127.0.0.1/synthetic",
                        "KP_STORAGE_ROOT" | "KP_DSI_WORKER" | "KP_DIFF_WORKER" => "/synthetic",
                        _ => return None,
                    }
                    .into(),
                )
            }
        }
        let config =
            crate::config::RuntimeConfig::from_env(&Environment, crate::config::Command::Serve)
                .unwrap();
        Self::new(pool, config.serve().unwrap().clone())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessFailure {
    Draining,
    Database,
    Schema,
    Storage,
    DsiWorker,
    DiffWorker,
    Web,
    Timeout,
}

pub(crate) fn executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        metadata.is_file()
            && rustix::fs::accessat(
                rustix::fs::CWD,
                path,
                rustix::fs::Access::EXEC_OK,
                rustix::fs::AtFlags::EACCESS,
            )
            .is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}
pub(crate) fn storage_usable(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    // NamedTempFile atomically owns a unique name and unlinks only that file on drop.
    tempfile::NamedTempFile::new_in(path)
        .and_then(|mut probe| {
            use std::io::{Read, Seek, Write};
            const PROBE: &[u8] = b"document-runtime-readiness";
            probe.write_all(PROBE)?;
            probe.as_file().sync_all()?;
            probe.rewind()?;
            let mut observed = Vec::new();
            probe.read_to_end(&mut observed)?;
            if observed != PROBE {
                return Err(std::io::Error::other("probe mismatch"));
            }
            probe.close()
        })
        .is_ok()
}

fn target_unchanged(path: &Path) -> bool {
    path.canonicalize().is_ok_and(|canonical| canonical == path)
}

pub async fn readiness(health: &RuntimeHealth) -> Result<(), ReadinessFailure> {
    if health.is_draining() {
        return Err(ReadinessFailure::Draining);
    }
    let result = tokio::time::timeout(
        document_api_http::limits::ORDINARY_OPERATION_TIMEOUT,
        async {
            sqlx::query("SELECT 1")
                .execute(&health.pool)
                .await
                .map_err(|_| ReadinessFailure::Database)?;
            check_schema_compatibility(&health.pool)
                .await
                .map_err(|_| ReadinessFailure::Schema)?;
            let config = health.config.clone();
            tokio::task::spawn_blocking(move || {
                if !target_unchanged(config.storage_root())
                    || !storage_usable(config.storage_root())
                {
                    return Err(ReadinessFailure::Storage);
                }
                if !target_unchanged(config.dsi_worker()) || !executable(config.dsi_worker()) {
                    return Err(ReadinessFailure::DsiWorker);
                }
                if !target_unchanged(config.diff_worker()) || !executable(config.diff_worker()) {
                    return Err(ReadinessFailure::DiffWorker);
                }
                if let Some(dist) = config.web_dist() {
                    if !target_unchanged(dist) {
                        return Err(ReadinessFailure::Web);
                    }
                    validate_dist(dist).map_err(|_| ReadinessFailure::Web)?;
                }
                Ok(())
            })
            .await
            .map_err(|_| ReadinessFailure::Storage)?
        },
    )
    .await
    .map_err(|_| ReadinessFailure::Timeout)?;
    if health.is_draining() {
        return Err(ReadinessFailure::Draining);
    }
    result
}

pub fn health_router(health: Arc<RuntimeHealth>) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/health/ready", get(ready))
        .with_state(health)
}
async fn ready(State(health): State<Arc<RuntimeHealth>>) -> (StatusCode, Json<Value>) {
    match readiness(&health).await {
        Ok(()) => (StatusCode::OK, Json(json!({"status":"ok"}))),
        Err(category) => {
            crate::observability::readiness_unavailable(category);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"status":"unavailable"})),
            )
        }
    }
}
