#![cfg(target_os = "linux")]

use std::{io::Cursor, path::PathBuf, sync::Arc};

use document_application::{
    BootstrapRootPolicy, Clock, CreateDocumentCommand, DocumentService, DocumentVersionService,
    IdGenerator, IdentityContextResolver, IdentityResolutionError, InvocationKind,
    PublicationScheduleRepository, PublishOperationId, SchedulePublishCommand,
    VerifiedActorContext,
};
use document_domain::{
    Action, FolderId, MediaType, Metadata, PolicyGrant, PolicySubject, PolicySubjectKind,
    PrincipalRef,
};
use document_publication_scheduler::DueScheduler;
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use document_semantic_inspection_runner::{RunnerConfig, RunnerInspectionExecutor};
use document_storage_fs::FileSystemStorage;
use sqlx::postgres::PgPoolOptions;
use tempfile::TempDir;
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
struct UuidV7Ids;
impl IdGenerator for UuidV7Ids {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}

struct CanaryResolver;
impl IdentityContextResolver for CanaryResolver {
    async fn resolve(
        &self,
        principal: &PrincipalRef,
    ) -> Result<VerifiedActorContext, IdentityResolutionError> {
        let subject = PolicySubject::new(
            PolicySubjectKind::Principal,
            principal.identity_provider(),
            principal.principal_id(),
        )
        .map_err(|_| IdentityResolutionError::InvalidIdentity)?;
        VerifiedActorContext::from_trusted_adapter(
            principal.clone(),
            vec![subject],
            OffsetDateTime::now_utc() + time::Duration::hours(1),
            InvocationKind::HumanInteractive,
            None,
        )
        .map_err(|_| IdentityResolutionError::InvalidIdentity)
    }
}

#[tokio::test]
#[ignore = "explicit Linux container canary with Docker and DSI_WORKER_BIN"]
async fn scheduled_initial_publication_runs_with_postgres_file_storage_and_sandbox() {
    let worker =
        PathBuf::from(std::env::var_os("DSI_WORKER_BIN").expect("DSI_WORKER_BIN is required"));
    let postgres = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "scheduler_canary")
        .start()
        .await
        .unwrap();
    let host =
        std::env::var("TESTCONTAINERS_HOST_OVERRIDE").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = postgres.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/scheduler_canary");
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    let root = TempDir::new().unwrap();
    let storage = Arc::new(FileSystemStorage::new(root.path()));
    let repository = Arc::new(PostgresDocumentRepository::new(pool.clone()));
    let executor = Arc::new(RunnerInspectionExecutor::new(RunnerConfig::new(&worker)).unwrap());
    let ids = Arc::new(UuidV7Ids);
    let clock = Arc::new(SystemClock);
    let actor = PrincipalRef::new("test", "scheduler-canary").unwrap();
    let actor_context = CanaryResolver.resolve(&actor).await.unwrap();
    let grants = vec![
        PolicyGrant::new(
            PolicySubject::new(PolicySubjectKind::Principal, "test", "scheduler-canary").unwrap(),
            [
                Action::Read,
                Action::Write,
                Action::Publish,
                Action::Administer,
            ],
        )
        .unwrap(),
    ];
    PostgresDocumentRepository::new_with_bootstrap_actor(pool.clone(), actor.clone())
        .initialize_root_policy(&actor_context, grants)
        .await
        .unwrap();
    let document_service = DocumentService::new(
        ids.clone(),
        clock.clone(),
        storage.clone(),
        repository.clone(),
    );
    let created = document_service
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
            title: "Scheduler canary".to_owned(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: actor.clone(),
            original_filename: "canary.txt".to_owned(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(
                b"Scheduled initial publication canary.\n".to_vec(),
            )),
        })
        .await
        .unwrap();
    let service = DocumentVersionService::new(ids, clock, storage, executor, repository.clone());
    let publish_id = PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap();
    service
        .schedule_publish(
            SchedulePublishCommand::new(
                publish_id,
                created.document_id(),
                created.document_version_id(),
                0,
                actor,
                OffsetDateTime::now_utc() + time::Duration::hours(1),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let due: OffsetDateTime = sqlx::query_scalar("SELECT now() - INTERVAL '1 second'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE document_publish_schedules SET scheduled_publish_at = $1 WHERE publish_operation_id = $2")
        .bind(due).bind(publish_id.as_uuid()).execute(&pool).await.unwrap();
    sqlx::query(
        "UPDATE document_versions SET scheduled_publish_at = $1 WHERE document_version_id = $2",
    )
    .bind(due)
    .bind(created.document_version_id().as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let scheduler = DueScheduler::connect_with_resolver(
        &url,
        root.path(),
        &worker,
        None,
        Arc::new(CanaryResolver),
        PrincipalRef::new("service", "publication-scheduler").unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(scheduler.poll_once().await.unwrap(), 1);
    assert_eq!(scheduler.poll_once().await.unwrap(), 0);
    let status = repository.get_schedule(publish_id).await.unwrap().unwrap();
    assert_eq!(status.status, "PUBLISHED");
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(created.document_id().as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(current, Some(created.document_version_id().as_uuid()));
    let ledger: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_publish_operations WHERE publish_operation_id = $1",
    )
    .bind(publish_id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ledger, 1);
}
