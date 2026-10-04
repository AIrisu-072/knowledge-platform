use std::{future::Future, io::Cursor, path::Path, pin::Pin, sync::Arc, time::Duration};

use document_application::{
    Clock, DocumentVersionService, IdGenerator, IdentityContextResolver, IdentityResolutionError,
    PublicationScheduleRepository, SemanticInspectionExecutor, VerifiedActorContext,
};
use document_domain::PrincipalRef;
use document_repository_postgres::PostgresDocumentRepository;
use document_semantic_inspection_core::{
    InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest,
};
use document_semantic_inspection_runner::{RunnerConfig, RunnerInspectionExecutor};
use document_storage_fs::FileSystemStorage;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use time::OffsetDateTime;
use uuid::Uuid;

struct UuidV7Ids;
impl IdGenerator for UuidV7Ids {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("publication scheduler requires KP_RUNTIME_MODE=poc")]
    UnsupportedRuntimeMode,
    #[error("publication scheduler requires Linux")]
    LinuxRequired,
    #[error("mandatory inspection sandbox is unavailable")]
    SandboxUnavailable,
    #[error("publication database is unavailable")]
    DatabaseUnavailable,
    #[error("authoritative file storage root is unavailable")]
    StorageUnavailable,
    #[error("publication attempt could not be completed")]
    PublicationUnavailable,
    #[error("publication scheduler requires an identity resolver")]
    IdentityResolverRequired,
}

type Resolution<'a> =
    Pin<Box<dyn Future<Output = Result<VerifiedActorContext, IdentityResolutionError>> + 'a>>;

trait DynIdentityResolver: Send + Sync {
    fn resolve<'a>(&'a self, principal: &'a PrincipalRef) -> Resolution<'a>;
}

impl<R: IdentityContextResolver> DynIdentityResolver for R {
    fn resolve<'a>(&'a self, principal: &'a PrincipalRef) -> Resolution<'a> {
        Box::pin(IdentityContextResolver::resolve(self, principal))
    }
}

struct ResolverAdapter {
    inner: Arc<dyn DynIdentityResolver>,
}

impl IdentityContextResolver for ResolverAdapter {
    async fn resolve(
        &self,
        principal: &PrincipalRef,
    ) -> Result<VerifiedActorContext, IdentityResolutionError> {
        self.inner.resolve(principal).await
    }
}

type VersionService = DocumentVersionService<
    UuidV7Ids,
    SystemClock,
    FileSystemStorage,
    RunnerInspectionExecutor,
    PostgresDocumentRepository,
>;

pub struct DueScheduler {
    repository: Arc<PostgresDocumentRepository>,
    service: VersionService,
    resolver: ResolverAdapter,
    service_executor: PrincipalRef,
}

impl DueScheduler {
    pub async fn connect(
        _database_url: &str,
        _storage_root: &Path,
        _worker_executable: &Path,
        _pdfium_runtime_dir: Option<&Path>,
    ) -> Result<Self, SchedulerError> {
        Err(SchedulerError::IdentityResolverRequired)
    }

    pub async fn connect_with_resolver<R: IdentityContextResolver + 'static>(
        database_url: &str,
        storage_root: &Path,
        worker_executable: &Path,
        pdfium_runtime_dir: Option<&Path>,
        resolver: Arc<R>,
        service_executor: PrincipalRef,
    ) -> Result<Self, SchedulerError> {
        if !cfg!(target_os = "linux") {
            return Err(SchedulerError::LinuxRequired);
        }
        if !std::fs::metadata(storage_root)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            return Err(SchedulerError::StorageUnavailable);
        }
        let mut config = RunnerConfig::new(worker_executable);
        if let Some(dir) = pdfium_runtime_dir {
            config = config.with_pdfium_runtime_dir(dir);
        }
        let executor = Arc::new(
            RunnerInspectionExecutor::new(config)
                .map_err(|_| SchedulerError::SandboxUnavailable)?,
        );
        probe_mandatory_sandbox(&executor).await?;
        let pool: PgPool = PgPoolOptions::new()
            .max_connections(8)
            .connect(database_url)
            .await
            .map_err(|_| SchedulerError::DatabaseUnavailable)?;
        let repository = Arc::new(PostgresDocumentRepository::new(pool));
        let now = repository
            .database_now()
            .await
            .map_err(|_| SchedulerError::DatabaseUnavailable)?;
        repository
            .list_due(now, 1)
            .await
            .map_err(|_| SchedulerError::DatabaseUnavailable)?;
        let service = DocumentVersionService::new(
            Arc::new(UuidV7Ids),
            Arc::new(SystemClock),
            Arc::new(FileSystemStorage::new(storage_root)),
            executor,
            repository.clone(),
        );
        Ok(Self {
            repository,
            service,
            resolver: ResolverAdapter { inner: resolver },
            service_executor,
        })
    }

    pub async fn poll_once(&self) -> Result<usize, SchedulerError> {
        let now = self
            .repository
            .database_now()
            .await
            .map_err(|_| SchedulerError::DatabaseUnavailable)?;
        let due = self
            .repository
            .list_due(now, 100)
            .await
            .map_err(|_| SchedulerError::DatabaseUnavailable)?;
        let count = due.len();
        for id in due {
            self.service
                .execute_due_authorized(id, &self.resolver, &self.service_executor)
                .await
                .map_err(|_| SchedulerError::PublicationUnavailable)?;
        }
        Ok(count)
    }

    pub async fn run_until_shutdown(&self, poll_interval: Duration) {
        let poll_interval = poll_interval.clamp(Duration::from_secs(1), Duration::from_secs(60));
        let mut failure_delay = Duration::from_secs(1);
        let mut termination =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Linux termination signal must be available");
        loop {
            match self.poll_once().await {
                Ok(_) => failure_delay = Duration::from_secs(1),
                Err(_) => {
                    eprintln!("publication scheduler poll failed; retrying");
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => break,
                        _ = termination.recv() => break,
                        _ = tokio::time::sleep(failure_delay) => {},
                    }
                    failure_delay = (failure_delay * 2).min(Duration::from_secs(60));
                    continue;
                }
            }
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                _ = termination.recv() => break,
                _ = tokio::time::sleep(poll_interval) => {},
            }
        }
    }
}

pub async fn probe_mandatory_sandbox(
    executor: &RunnerInspectionExecutor,
) -> Result<(), SchedulerError> {
    let bytes = b"DSI mandatory sandbox startup probe\n".to_vec();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: "text/plain".to_owned(),
        expected_raw_content_hash: digest,
        expected_size_bytes: bytes.len() as u64,
        trace_context: None,
    };
    executor
        .inspect(request, Box::pin(Cursor::new(bytes)))
        .await
        .map(|_| ())
        .map_err(|_| SchedulerError::SandboxUnavailable)
}
