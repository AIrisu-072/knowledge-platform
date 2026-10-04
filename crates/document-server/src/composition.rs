//! The composition root selects adapters and joins existing route families only.
use crate::{
    config::{Command, RuntimeConfig},
    health::{RuntimeHealth, executable, health_router, storage_usable},
    identity::{StaticPoCIdentityAdapter, UtcClock},
    web::web_router,
};
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use document_api_http::{
    api::{DocumentApiRouters, compose_document_api},
    identity::IdentityAdapter,
};
use document_application::{
    EnsureSemanticInspection, IdGenerator, SemanticInspectionExecutor, document_diff::DiffExecutor,
};
use document_diff_runner::RunnerDiffExecutor;
use document_repository_postgres::{PostgresDocumentRepository, check_schema_compatibility};
use document_semantic_inspection_runner::RunnerInspectionExecutor;
use document_storage_fs::FileSystemStorage;
use sha2::{Digest, Sha256};
use sqlx::{
    ConnectOptions, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{future::Future, io::Cursor, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::net::TcpListener;
use tower::ServiceExt;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum StartupError {
    #[error("runtime command configuration is invalid")]
    Configuration,
    #[error("document database is unavailable")]
    Database,
    #[error(
        "document schema is incompatible; run the explicit migration operation on the disposable PoC database"
    )]
    Schema,
    #[error("document storage is unavailable")]
    Storage,
    #[error("DSI worker or required native sandbox is unavailable")]
    DsiWorker,
    #[error("Diff worker or required native sandbox is unavailable")]
    DiffWorker,
    #[error("built GUI artifacts are unavailable or invalid")]
    Web,
    #[error("runtime identity is unavailable")]
    Identity,
    #[error("HTTP listener is unavailable")]
    Listener,
}

pub struct Runtime {
    pool: PgPool,
    router: Router,
    health: Arc<RuntimeHealth>,
}
impl Runtime {
    /// An outer composition can join separately owned routes while retaining the
    /// same health, pool lifetime and graceful-drain implementation.
    pub fn with_router(mut self, router: Router) -> Self {
        self.router = router;
        self
    }
    pub fn router(&self) -> Router {
        self.router.clone()
    }
    pub fn health(&self) -> Arc<RuntimeHealth> {
        self.health.clone()
    }
    pub async fn serve(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), StartupError> {
        let health = self.health.clone();
        let result = axum::serve(listener, self.router)
            .with_graceful_shutdown(async move {
                shutdown.await;
                health.begin_shutdown();
            })
            .await;
        self.pool.close().await;
        result.map_err(|_| StartupError::Listener)
    }
}

pub async fn connect_database(config: &RuntimeConfig) -> Result<PgPool, StartupError> {
    let options = config
        .database_url()
        .parse::<PgConnectOptions>()
        .map_err(|_| StartupError::Database)?
        .disable_statement_logging();
    // Operational connection bound below the existing 30-second ordinary API budget.
    PgPoolOptions::new()
        .max_connections(12)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await
        .map_err(|_| StartupError::Database)
}

struct UuidV7Ids;
impl IdGenerator for UuidV7Ids {
    fn next_uuid_v7(&self) -> uuid::Uuid {
        uuid::Uuid::now_v7()
    }
}

pub async fn compose_runtime(config: &RuntimeConfig) -> Result<Runtime, StartupError> {
    if config.command() != Command::Serve {
        return Err(StartupError::Configuration);
    }
    let identity: Arc<dyn IdentityAdapter> = Arc::new(StaticPoCIdentityAdapter::new(
        config.profile().ok_or(StartupError::Identity)?,
    ));
    compose_runtime_with_identity(config, identity).await
}

/// Reuse the exact Document runtime with an identity supplied by a trusted
/// composition root. This does not add a request-selected identity profile.
pub async fn compose_runtime_with_identity(
    config: &RuntimeConfig,
    identity: Arc<dyn IdentityAdapter>,
) -> Result<Runtime, StartupError> {
    if config.command() != Command::Serve {
        return Err(StartupError::Configuration);
    }
    let serve = config.serve().ok_or(StartupError::Configuration)?;
    let pool = connect_database(config).await?;
    check_schema_compatibility(&pool)
        .await
        .map_err(|_| StartupError::Schema)?;
    let health = Arc::new(RuntimeHealth::new(pool.clone(), serve.clone()));
    let serve = health.targets();
    if !storage_usable(serve.storage_root()) {
        return Err(StartupError::Storage);
    }
    if !executable(serve.dsi_worker()) {
        return Err(StartupError::DsiWorker);
    }
    if !executable(serve.diff_worker()) {
        return Err(StartupError::DiffWorker);
    }
    let storage_path = serve
        .storage_root()
        .canonicalize()
        .map_err(|_| StartupError::Storage)?;
    let dsi_path = serve
        .dsi_worker()
        .canonicalize()
        .map_err(|_| StartupError::DsiWorker)?;
    let diff_path = serve
        .diff_worker()
        .canonicalize()
        .map_err(|_| StartupError::DiffWorker)?;
    let mut dsi_config = document_semantic_inspection_runner::RunnerConfig::new(dsi_path);
    let mut diff_config = document_diff_runner::RunnerConfig::new(diff_path);
    if let Some(directory) = serve.pdfium_runtime_dir() {
        let directory = directory
            .canonicalize()
            .map_err(|_| StartupError::DsiWorker)?;
        if !directory.is_dir() {
            return Err(StartupError::DsiWorker);
        }
        // The Diff child clears its environment; pass this explicitly to BOTH runners.
        dsi_config = dsi_config.with_pdfium_runtime_dir(&directory);
        diff_config = diff_config.with_pdfium_runtime_dir(directory);
    }
    let inspection =
        Arc::new(RunnerInspectionExecutor::new(dsi_config).map_err(|_| StartupError::DsiWorker)?);
    let diff =
        Arc::new(RunnerDiffExecutor::new(diff_config).map_err(|_| StartupError::DiffWorker)?);
    preflight_workers(&inspection, &diff).await?;
    let repository = Arc::new(PostgresDocumentRepository::new(pool.clone()));
    let storage = Arc::new(FileSystemStorage::new(storage_path));
    let ids = Arc::new(UuidV7Ids);
    let clock = Arc::new(UtcClock);
    let evidence = Arc::new(EnsureSemanticInspection::new(
        repository.clone(),
        storage.clone(),
        inspection.clone(),
        clock.clone(),
    ));
    let map_identity = |_| StartupError::Identity;
    let api = compose_document_api(DocumentApiRouters::new(
        document_api_http::read::read_router(repository.clone(), identity.clone())
            .map_err(map_identity)?,
        document_api_http::management::management_router(repository.clone(), identity.clone())
            .map_err(map_identity)?,
        document_api_http::create::create_router(
            ids.clone(),
            clock.clone(),
            storage.clone(),
            repository.clone(),
            identity.clone(),
        )
        .map_err(map_identity)?,
        document_api_http::versioning::versioning_router(
            ids.clone(),
            clock.clone(),
            storage.clone(),
            inspection.clone(),
            repository.clone(),
            identity.clone(),
        )
        .map_err(map_identity)?,
        document_api_http::publication::publication_router(
            ids,
            clock,
            storage.clone(),
            inspection,
            repository.clone(),
            identity.clone(),
        )
        .map_err(map_identity)?,
        document_api_http::file_download::file_download_router(
            repository.clone(),
            storage.clone(),
            identity.clone(),
        )
        .map_err(map_identity)?,
        document_api_http::diff::diff_router(repository, storage, diff, evidence, identity)
            .map_err(map_identity)?,
    ));
    let web = serve.web_dist().map(web_router).transpose()?;
    let router = runtime_router(api, health.clone(), web);
    Ok(Runtime {
        pool,
        router,
        health,
    })
}

async fn preflight_workers(
    inspection: &RunnerInspectionExecutor,
    diff: &RunnerDiffExecutor,
) -> Result<(), StartupError> {
    // Synthetic bytes never enter repository/storage. Running the real sandbox
    // protocol catches unavailable native enforcement before accepting HTTP traffic.
    let bytes = b"Document runtime synthetic preflight.\n";
    let hash: [u8; 32] = Sha256::digest(bytes).into();
    inspection
        .inspect(
            document_semantic_inspection_core::WorkerRequest {
                protocol_version: document_semantic_inspection_core::WorkerProtocolVersion::V0,
                inspection_profile_version:
                    document_semantic_inspection_core::InspectionProfileVersion::DsiV0,
                declared_media_type: "text/plain".into(),
                expected_raw_content_hash: hash,
                expected_size_bytes: bytes.len() as u64,
                trace_context: None,
            },
            Box::pin(Cursor::new(bytes.to_vec())),
        )
        .await
        .map_err(|_| StartupError::DsiWorker)?;
    diff.compare(
        document_diff_core::WorkerDiffRequest {
            protocol_version: document_diff_core::WorkerProtocolVersion::V0,
            diff_profile_version: document_diff_core::DiffProfileVersion::V0,
            resource_profile_version: document_diff_core::ResourceProfileVersion::V0,
            format: document_diff_core::FormatId::Txt,
            base_raw_sha256: hash,
            base_size_bytes: bytes.len() as u64,
            target_raw_sha256: hash,
            target_size_bytes: bytes.len() as u64,
        },
        Box::pin(Cursor::new(bytes.to_vec())),
        Box::pin(Cursor::new(bytes.to_vec())),
    )
    .await
    .map_err(|_| StartupError::DiffWorker)?;
    Ok(())
}

#[derive(Clone)]
struct RuntimeRoutes {
    api: Router,
    health: Router,
    web: Option<Router>,
}
pub fn runtime_router(api: Router, health: Arc<RuntimeHealth>, web: Option<Router>) -> Router {
    Router::new().fallback(dispatch).with_state(RuntimeRoutes {
        api,
        health: health_router(health),
        web,
    })
}
async fn dispatch(State(routes): State<RuntimeRoutes>, request: Request<Body>) -> Response {
    let path = request.uri().path();
    let selected = if path == "/v1" || path.starts_with("/v1/") {
        Some(routes.api)
    } else if path == "/health" || path.starts_with("/health/") {
        Some(routes.health)
    } else {
        routes.web
    };
    match selected {
        Some(router) => match router.oneshot(request).await {
            Ok(response) => response,
            Err(error) => match error {},
        },
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    use axum::{
        body::{Body, Bytes},
        routing::get,
    };
    use http_body::{Body as HttpBody, Frame};
    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::oneshot,
    };

    struct PendingBody {
        release: Option<oneshot::Receiver<()>>,
    }
    impl HttpBody for PendingBody {
        type Data = Bytes;
        type Error = Infallible;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
            let Some(release) = self.release.as_mut() else {
                return Poll::Ready(None);
            };
            match Pin::new(release).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(_) => {
                    self.release = None;
                    Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"finished")))))
                }
            }
        }
    }

    #[tokio::test]
    async fn graceful_shutdown_waits_for_stream_and_exits_when_client_releases() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://127.0.0.1/synthetic")
            .unwrap();
        let health = Arc::new(RuntimeHealth::for_drain_test(pool.clone()));
        let (release, body_release) = oneshot::channel();
        let body_release = Arc::new(tokio::sync::Mutex::new(Some(body_release)));
        let router = Router::new().route(
            "/stream",
            get(move || {
                let body_release = body_release.clone();
                async move {
                    Body::new(PendingBody {
                        release: body_release.lock().await.take(),
                    })
                }
            }),
        );
        let runtime = Runtime {
            pool,
            router,
            health: health.clone(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let mut server = tokio::spawn(runtime.serve(listener, async {
            let _ = stopped.await;
        }));
        let mut client = TcpStream::connect(address).await.unwrap();
        client
            .write_all(b"GET /stream HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut header = [0u8; 256];
        let count = tokio::time::timeout(Duration::from_secs(2), client.read(&mut header))
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&header[..count]).contains("200 OK"));
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !health.is_draining() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut server)
                .await
                .is_err(),
            "unreleased response body must remain draining"
        );
        assert!(
            TcpStream::connect(address).await.is_err(),
            "new connections must be refused"
        );
        release.send(()).unwrap();
        let mut tail = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut tail))
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&tail).contains("finished"));
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

#[cfg(test)]
mod composition_extension_tests {
    use super::*;
    use axum::{body::to_bytes, http::Request, routing::get};

    #[tokio::test]
    async fn outer_composition_preserves_runtime_health_and_replaces_only_router() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://127.0.0.1/synthetic")
            .unwrap();
        let health = Arc::new(RuntimeHealth::for_drain_test(pool.clone()));
        let runtime = Runtime {
            pool,
            router: Router::new(),
            health: health.clone(),
        };
        let runtime =
            runtime.with_router(Router::new().route("/joined", get(|| async { "joined" })));
        assert!(Arc::ptr_eq(&health, &runtime.health()));
        let response = runtime
            .router()
            .oneshot(
                Request::builder()
                    .uri("/joined")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(to_bytes(response.into_body(), 100).await.unwrap(), "joined");
    }
}
