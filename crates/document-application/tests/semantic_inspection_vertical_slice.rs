//! Task 12 RED: real authoritative file, PostgreSQL, sandbox runner, and worker.

#[allow(unused_imports)]
use document_semantic_inspection_runner::RunnerInspectionExecutor;

#[cfg(target_os = "linux")]
mod linux_tests {
    use std::{
        ffi::OsString,
        io::Cursor,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use document_application::{
        ApplicationError, Clock, ContentReader, CreateDocumentCommand, DocumentService,
        EnsureSemanticInspection, IdGenerator, InspectionExecutionError,
        SemanticInspectionExecutor,
    };
    use document_domain::{FolderId, MediaType, Metadata, PrincipalRef};
    use document_repository_postgres::{
        PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate,
    };
    use document_semantic_inspection_core::{
        FormatId, InspectionProfileVersion, WorkerRequest, WorkerResponse,
    };
    use document_semantic_inspection_runner::RunnerConfig;
    use document_storage_fs::FileSystemStorage;
    use sqlx::{PgPool, postgres::PgPoolOptions};
    use tempfile::TempDir;
    use testcontainers::{
        GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::RunnerInspectionExecutor;

    struct SystemClock;
    impl Clock for SystemClock {
        fn now(&self) -> OffsetDateTime {
            OffsetDateTime::now_utc()
        }
    }

    struct UuidV7Generator;
    impl IdGenerator for UuidV7Generator {
        fn next_uuid_v7(&self) -> Uuid {
            Uuid::now_v7()
        }
    }

    struct CountingExecutor {
        inner: RunnerInspectionExecutor,
        launches: Arc<AtomicUsize>,
    }

    impl SemanticInspectionExecutor for CountingExecutor {
        async fn inspect(
            &self,
            request: WorkerRequest,
            content: ContentReader,
        ) -> Result<WorkerResponse, InspectionExecutionError> {
            self.launches.fetch_add(1, Ordering::SeqCst);
            self.inner.inspect(request, content).await
        }
    }

    struct Fixture {
        _postgres: testcontainers::ContainerAsync<GenericImage>,
        _storage_root: TempDir,
        pool: PgPool,
        storage: Arc<FileSystemStorage>,
        repository: Arc<PostgresDocumentRepository>,
        executor: Arc<CountingExecutor>,
        launches: Arc<AtomicUsize>,
    }

    async fn fixture() -> Fixture {
        let postgres = GenericImage::new("postgres", "18.6-bookworm")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", "postgres")
            .with_env_var("POSTGRES_DB", "knowledge_platform_test")
            .start()
            .await
            .unwrap();
        let port = postgres.get_host_port_ipv4(5432.tcp()).await.unwrap();
        let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/knowledge_platform_test");
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        let storage_root = TempDir::new().unwrap();
        let storage = Arc::new(FileSystemStorage::new(storage_root.path()));
        let repository = Arc::new(PostgresDocumentRepository::new(pool.clone()));
        let worker = OsString::from(
            std::env::var_os("DSI_WORKER_BIN")
                .expect("DSI_WORKER_BIN must point to the built production worker"),
        );
        let mut config = RunnerConfig::new(worker);
        if let Some(dir) = std::env::var_os("PDFIUM_DYNAMIC_LIB_PATH") {
            config = config.with_pdfium_runtime_dir(dir);
        }
        let launches = Arc::new(AtomicUsize::new(0));
        let executor = Arc::new(CountingExecutor {
            inner: RunnerInspectionExecutor::new(config).unwrap(),
            launches: launches.clone(),
        });
        Fixture {
            _postgres: postgres,
            _storage_root: storage_root,
            pool,
            storage,
            repository,
            executor,
            launches,
        }
    }

    impl Fixture {
        async fn create(&self, media_type: &str, bytes: &[u8]) -> document_domain::FileId {
            let service = DocumentService::new(
                Arc::new(UuidV7Generator),
                Arc::new(SystemClock),
                self.storage.clone(),
                self.repository.clone(),
            );
            service
                .create_document(CreateDocumentCommand {
                    folder_id: FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
                    title: "Synthetic semantic inspection".into(),
                    document_metadata: Metadata::default(),
                    version_metadata: Metadata::default(),
                    principal: PrincipalRef::new("test", "synthetic-principal").unwrap(),
                    original_filename: "synthetic.bin".into(),
                    media_type: MediaType::new(media_type).unwrap(),
                    content: Box::pin(Cursor::new(bytes.to_vec())),
                })
                .await
                .unwrap()
                .file_id()
        }

        fn ensure(
            &self,
        ) -> EnsureSemanticInspection<
            PostgresDocumentRepository,
            FileSystemStorage,
            CountingExecutor,
            SystemClock,
        > {
            EnsureSemanticInspection::new(
                self.repository.clone(),
                self.storage.clone(),
                self.executor.clone(),
                Arc::new(SystemClock),
            )
        }

        async fn row_count(&self, file_id: document_domain::FileId) -> i64 {
            sqlx::query_scalar(
                "SELECT count(*) FROM document_semantic_inspections WHERE file_id = $1",
            )
            .bind(file_id.as_uuid())
            .fetch_one(&self.pool)
            .await
            .unwrap()
        }
    }

    #[tokio::test]
    async fn authoritative_create_inspect_then_cache_hit_launches_one_worker() {
        let fixture = fixture().await;
        let file_id = fixture
            .create("text/plain", b"Synthetic policy text.\n")
            .await;
        let service = fixture.ensure();
        let first = service
            .ensure(file_id, InspectionProfileVersion::DsiV0)
            .await
            .unwrap();
        assert_eq!(first.response().detected_format, FormatId::Txt);
        assert_eq!(fixture.launches.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.row_count(file_id).await, 1);
        let second = service
            .ensure(file_id, InspectionProfileVersion::DsiV0)
            .await
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(fixture.launches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn changed_authoritative_raw_binding_fails_without_reinspection() {
        let fixture = fixture().await;
        let file_id = fixture
            .create("text/plain", b"Synthetic policy text.\n")
            .await;
        let service = fixture.ensure();
        service
            .ensure(file_id, InspectionProfileVersion::DsiV0)
            .await
            .unwrap();
        sqlx::query("UPDATE file_objects SET content_hash = $1 WHERE file_id = $2")
            .bind(vec![0x55_u8; 32])
            .bind(file_id.as_uuid())
            .execute(&fixture.pool)
            .await
            .unwrap();
        let error = service
            .ensure(file_id, InspectionProfileVersion::DsiV0)
            .await
            .unwrap_err();
        assert_eq!(error, ApplicationError::IntegrityViolation);
        assert_eq!(fixture.launches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn incompatible_synthetic_document_cannot_create_success_row() {
        let fixture = fixture().await;
        let file_id = fixture
            .create("text/plain", b"<html><body>different format</body></html>")
            .await;
        let error = fixture
            .ensure()
            .ensure(file_id, InspectionProfileVersion::DsiV0)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ApplicationError::InspectionFailed(InspectionExecutionError::FormatMismatch)
        ));
        assert_eq!(fixture.row_count(file_id).await, 0);
    }
}
