#![cfg(target_os = "linux")]

#[path = "support/database.rs"]
mod database;

use database::TestDatabase;
use document_application::{
    AccessPolicyService, BootstrapRootPolicy, CancelScheduleCommand, Clock,
    CreateDocumentCommand, DocumentService, DocumentVersionService, IdGenerator,
    IdentityContextResolver, ManagementCommand,
    ManagementOperationId, PublicationScheduleRepository, PublishOperationId, SchedulePublishCommand,
    VersionOperationId,
};
use document_domain::{
    Action, DocumentId, DocumentVersionId, FolderId, MediaType, Metadata, PolicyGrant, PolicyMode,
    PolicySubject, PolicySubjectKind, PolicyTarget, PrincipalRef,
};
use document_publication_scheduler::{StaticRequesterResolver, probe_mandatory_sandbox};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use document_semantic_inspection_runner::{RunnerConfig, RunnerInspectionExecutor};
use document_storage_fs::FileSystemStorage;
use organization_server::organization_root_grants;
use sqlx::{ConnectOptions, PgPool, postgres::PgConnectOptions};
use std::str::FromStr;
use std::{io::Cursor, path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use time::OffsetDateTime;
use tokio::process::{Child, Command};
use uuid::Uuid;

// Fixed labels only: no document identifiers, paths, URLs or worker output.
// Bypass libtest's print capture and flush so SIGABRT cannot hide progress.
#[derive(Debug)]
enum CanaryStage {
    Started,
    DatabaseReady,
    SchemaMigrated,
    SandboxStarted,
    SandboxVerified,
    RootPolicyInitialized,
    ReservationStarted,
    DocumentCreated,
    ReservationStored,
    FixturesReserved,
    RevocationsPrepared,
    FirstProcessStarted,
    WarmupPublished,
    FirstProcessStopped,
    StoppedStateVerified,
    ContendersStarted,
    ContendersBlocked,
    ContendersCompleted,
    AttributionVerified,
    ContendersStopped,
    Restarted,
    RestartPublished,
    Complete,
}

impl CanaryStage {
    fn report(self) {
        use std::io::Write;
        let mut stderr = std::io::stderr().lock();
        writeln!(stderr, "scheduler-canary:{self:?}").unwrap();
        stderr.flush().unwrap();
    }
}

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
#[derive(Clone, Copy)]
enum RuntimeFixture {
    Poc,
    Organization,
}

impl RuntimeFixture {
    fn mode(self) -> &'static str {
        match self {
            Self::Poc => "poc",
            Self::Organization => "organization-synthetic",
        }
    }

    fn requester(self) -> &'static str {
        match self {
            Self::Poc => "poc-human",
            Self::Organization => "sales-01",
        }
    }

    fn read_only_requester(self) -> &'static str {
        match self {
            Self::Poc => "poc-agent",
            Self::Organization => "office-01",
        }
    }

    fn principal(self) -> PrincipalRef {
        PrincipalRef::new(self.mode(), self.requester()).unwrap()
    }

    fn grants(self, actions: impl IntoIterator<Item = Action>) -> Vec<PolicyGrant> {
        let (kind, subject) = match self {
            Self::Poc => (PolicySubjectKind::Group, "poc-users"),
            Self::Organization => (PolicySubjectKind::Principal, self.requester()),
        };
        vec![
            PolicyGrant::new(
                PolicySubject::new(kind, self.mode(), subject).unwrap(),
                actions,
            )
            .unwrap(),
        ]
    }

    fn root_grants(self) -> Vec<PolicyGrant> {
        if matches!(self, Self::Organization) {
            return organization_root_grants();
        }
        let mut grants = self.grants([
            Action::Read,
            Action::ReadHistory,
            Action::Write,
            Action::Publish,
            Action::Administer,
        ]);
        grants.push(
            PolicyGrant::new(
                PolicySubject::new(PolicySubjectKind::Group, "poc", "poc-agents").unwrap(),
                [Action::Read, Action::ReadHistory],
            )
            .unwrap(),
        );
        grants
    }
}

#[derive(Clone, Copy)]
struct Reservation {
    document: DocumentId,
    version: DocumentVersionId,
    operation: PublishOperationId,
    revision: i64,
}

type Versions = DocumentVersionService<
    UuidV7Ids,
    SystemClock,
    FileSystemStorage,
    RunnerInspectionExecutor,
    PostgresDocumentRepository,
>;
type Documents =
    DocumentService<UuidV7Ids, SystemClock, FileSystemStorage, PostgresDocumentRepository>;

async fn reserve(
    documents: &Documents,
    versions: &Versions,
    title: &str,
    runtime: RuntimeFixture,
) -> Reservation {
    // Each scenario call site must carry only the small wrapper future.
    // Boxing only the scenario leaves repeated large reservation temporaries in
    // its debug poll stack, which overflows the unchanged default test stack.
    Box::pin(run_reservation(documents, versions, title, runtime)).await
}

async fn run_reservation(
    documents: &Documents,
    versions: &Versions,
    title: &str,
    runtime: RuntimeFixture,
) -> Reservation {
    CanaryStage::ReservationStarted.report();
    let human = runtime.principal();
    let created = documents
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
            title: title.into(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: human.clone(),
            original_filename: "synthetic.txt".into(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(b"Synthetic scheduled publication.\n".to_vec())),
        })
        .await
        .unwrap();
    CanaryStage::DocumentCreated.report();
    let operation = PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap();
    let scheduled = versions
        .schedule_publish(
            SchedulePublishCommand::new(
                operation,
                created.document_id(),
                created.document_version_id(),
                0,
                human,
                OffsetDateTime::now_utc() + time::Duration::hours(1),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    CanaryStage::ReservationStored.report();
    Reservation {
        document: created.document_id(),
        version: created.document_version_id(),
        operation,
        revision: scheduled.accepted_revision,
    }
}

// Test-only clock advancement in the owned disposable DB. Business publication
// still runs only through the production scheduler/Application/Repository path.
async fn make_due(pool: &PgPool, reservation: Reservation) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE document_publish_schedules SET scheduled_publish_at = now() - INTERVAL '1 second' WHERE publish_operation_id = $1")
        .bind(reservation.operation.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE document_versions SET scheduled_publish_at = now() - INTERVAL '1 second' WHERE document_version_id = $1")
        .bind(reservation.version.as_uuid()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

fn scheduler_database_url(options: &PgConnectOptions, application: &str) -> String {
    // SQLx 0.9's lossy serializer omits application_name; preserve the test's
    // process tag explicitly in the URL passed across the executable boundary.
    let mut url = options.to_url_lossy();
    url.query_pairs_mut()
        .append_pair("application_name", application);
    url.to_string()
}

#[test]
fn subprocess_connection_preserves_distinct_application_names() {
    let options = PgConnectOptions::new()
        .host("127.0.0.1")
        .username("synthetic")
        .database("synthetic");
    for application in ["r5-first", "r5-second"] {
        let parsed =
            PgConnectOptions::from_str(&scheduler_database_url(&options, application)).unwrap();
        assert_eq!(parsed.get_application_name(), Some(application));
    }
}

fn start(
    pool: &PgPool,
    storage: &std::path::Path,
    worker: &std::path::Path,
    application: &str,
    runtime: RuntimeFixture,
) -> Child {
    Command::new(env!("CARGO_BIN_EXE_document-publication-scheduler"))
        .env_clear()
        .env("KP_RUNTIME_MODE", runtime.mode())
        .env(
            "DOCUMENT_DATABASE_URL",
            scheduler_database_url(pool.connect_options().as_ref(), application),
        )
        .env("DOCUMENT_STORAGE_ROOT", storage)
        .env("DSI_WORKER_EXECUTABLE", worker)
        .env("DOCUMENT_PUBLICATION_POLL_SECONDS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

async fn stop(child: &mut Child) {
    let id = child.id().expect("owned scheduler must be running");
    assert!(
        Command::new("kill")
            .arg("-TERM")
            .arg(id.to_string())
            .status()
            .await
            .unwrap()
            .success()
    );
    let status = tokio::time::timeout(Duration::from_secs(15), child.wait())
        .await
        .expect("scheduler did not drain in the test observation window")
        .unwrap();
    assert!(
        status.success(),
        "scheduler did not exit successfully on SIGTERM"
    );
}

async fn await_status(
    pool: &PgPool,
    reservation: Reservation,
    expected: &str,
    children: &mut [&mut Child],
) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            for child in &mut *children {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "scheduler exited before the expected schedule status"
                );
            }
            let status: String = sqlx::query_scalar(
                "SELECT status FROM document_publish_schedules WHERE publish_operation_id = $1",
            )
            .bind(reservation.operation.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
            if status == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("schedule did not reach expected status within the observation window");
}

async fn publication_counts(pool: &PgPool, reservation: Reservation) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM document_publish_operations WHERE publish_operation_id = $1), (SELECT count(*) FROM audit_outbox_events WHERE resource_id = $2 AND event_type = 'document.version.published'), (SELECT count(*) FROM document_revisions WHERE document_id = $2)")
        .bind(reservation.operation.as_uuid()).bind(reservation.document.as_uuid()).fetch_one(pool).await.unwrap()
}

#[tokio::test]
#[ignore = "explicit real PostgreSQL, production DSI sandbox and separate scheduler process acceptance"]
async fn future_stop_restart_revocation_and_concurrent_processes_preserve_single_publication() {
    canary(RuntimeFixture::Poc).await;
}

#[tokio::test]
#[ignore = "explicit Organization PostgreSQL, production DSI and separate scheduler process acceptance"]
async fn organization_publication_cancellation_revocation_and_restart_use_current_authority() {
    canary(RuntimeFixture::Organization).await;
}

async fn canary(runtime: RuntimeFixture) {
    CanaryStage::Started.report();
    // Keep the large test-only scenario state on the heap. Do not expand the
    // test/production stack limit or change any worker isolation setting.
    Box::pin(run_process_canary(runtime)).await;
}

#[test]
fn process_canary_future_has_a_small_test_stack_frame() {
    fn frame_size<F>(_constructor: impl FnOnce() -> F) -> usize {
        std::mem::size_of::<F>()
    }
    let size = frame_size(|| canary(RuntimeFixture::Poc));
    assert!(
        size < 64 * 1024,
        "canary future frame is {size} bytes; keep below one thirty-second of the default 2MiB test stack"
    );
}

#[test]
fn process_canary_scenario_has_a_small_test_stack_frame() {
    fn frame_size<F>(_constructor: impl FnOnce() -> F) -> usize {
        std::mem::size_of::<F>()
    }
    let size = frame_size(|| run_process_canary(RuntimeFixture::Poc));
    assert!(
        size < 64 * 1024,
        "scenario future frame is {size} bytes; nested fixture futures must not inflate the scenario poll stack"
    );
}

#[test]
fn reservation_wrapper_has_a_small_test_stack_frame() {
    fn frame_size<F>(
        _constructor: impl FnOnce(&'static Documents, &'static Versions, &'static str) -> F,
    ) -> usize {
        std::mem::size_of::<F>()
    }
    let size = frame_size(|documents, versions, title| {
        reserve(documents, versions, title, RuntimeFixture::Poc)
    });
    assert!(
        size < 1024,
        "reservation wrapper future is {size} bytes; heap-box the fixture body before embedding it in each scenario call site"
    );
}

#[test]
fn stage_report_survives_libtest_capture_and_process_abort() {
    const PROBE: &str = "DOCUMENT_SCHEDULER_STAGE_ABORT_PROBE";
    if std::env::var_os(PROBE).is_some() {
        CanaryStage::ReservationStarted.report();
        std::process::abort();
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("stage_report_survives_libtest_capture_and_process_abort")
        .env(PROBE, "1")
        .output()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(output.status.signal(), Some(6));
    assert_eq!(output.stderr, b"scheduler-canary:ReservationStarted\n");
}

async fn run_process_canary(runtime: RuntimeFixture) {
    let worker =
        PathBuf::from(std::env::var_os("DSI_WORKER_BIN").expect("DSI_WORKER_BIN is required"));
    let database = TestDatabase::new().await;
    CanaryStage::DatabaseReady.report();
    let pool = &database.pool;
    migrate(pool).await.unwrap();
    CanaryStage::SchemaMigrated.report();
    let storage_root = tempfile::tempdir().unwrap();
    let storage = Arc::new(FileSystemStorage::new(storage_root.path()));
    let repository = Arc::new(PostgresDocumentRepository::new(pool.clone()));
    let executor = Arc::new(RunnerInspectionExecutor::new(RunnerConfig::new(&worker)).unwrap());
    CanaryStage::SandboxStarted.report();
    probe_mandatory_sandbox(&executor).await.expect(
        "mandatory production sandbox must pass; no fake/skip/fallback qualifies acceptance",
    );
    CanaryStage::SandboxVerified.report();
    let resolver = StaticRequesterResolver::for_runtime_mode(runtime.mode()).unwrap();
    let human = runtime.principal();
    let human_context = resolver.resolve(&human).await.unwrap();
    PostgresDocumentRepository::new_with_bootstrap_actor(pool.clone(), human.clone())
        .initialize_root_policy(&human_context, runtime.root_grants())
        .await
        .unwrap();
    CanaryStage::RootPolicyInitialized.report();
    let ids = Arc::new(UuidV7Ids);
    let clock = Arc::new(SystemClock);
    let authorized_repository = Arc::new(repository.with_verified_actor(human_context.clone()));
    let documents = DocumentService::new(
        ids.clone(),
        clock.clone(),
        storage.clone(),
        authorized_repository.clone(),
    );
    let versions =
        DocumentVersionService::new(ids, clock, storage, executor, authorized_repository);
    let warmup = reserve(&documents, &versions, "Synthetic ready proof", runtime).await;
    let future = reserve(&documents, &versions, "Synthetic future publication", runtime).await;
    let revoked = reserve(&documents, &versions, "Synthetic revoked publication", runtime).await;
    let unknown = reserve(&documents, &versions, "Synthetic unresolvable requester", runtime).await;
    let agent = reserve(&documents, &versions, "Synthetic agent requester denial", runtime).await;
    let cancelled = reserve(&documents, &versions, "Synthetic cancelled publication", runtime).await;
    make_due(pool, cancelled).await;
    let cancellation = CancelScheduleCommand::new(
        VersionOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
        cancelled.operation,
        cancelled.document,
        cancelled.version,
        cancelled.revision,
        human.clone(),
    )
    .unwrap();
    let cancellation_result = Box::pin(versions.cancel_schedule(cancellation.clone()))
        .await
        .unwrap();
    assert_eq!(
        Box::pin(versions.cancel_schedule(cancellation)).await.unwrap(),
        cancellation_result
    );
    assert_eq!(
        repository
            .get_schedule(cancelled.operation)
            .await
            .unwrap()
            .unwrap()
            .status,
        "CANCELLED"
    );
    CanaryStage::FixturesReserved.report();
    AccessPolicyService::new(repository.clone())
        .set_access_policy(
            &human_context,
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(revoked.document),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(runtime.grants([Action::Read])),
                reason: "Synthetic revoke before due".into(),
            },
        )
        .await
        .unwrap();
    // Fault injection only: an original requester no longer resolvable by this runtime.
    sqlx::query("UPDATE document_publish_schedules SET actor_principal_id = 'missing-requester' WHERE publish_operation_id = $1")
        .bind(unknown.operation.as_uuid()).execute(pool).await.unwrap();
    sqlx::query("UPDATE document_publish_schedules SET actor_principal_id = $2 WHERE publish_operation_id = $1")
        .bind(agent.operation.as_uuid()).bind(runtime.read_only_requester()).execute(pool).await.unwrap();
    CanaryStage::RevocationsPrepared.report();
    make_due(pool, warmup).await;
    let mut first = start(pool, storage_root.path(), &worker, "r5-first", runtime);
    CanaryStage::FirstProcessStarted.report();
    // A committed warmup proves the real main passed preflight and polled. A
    // fixed sleep or merely alive PID would not prove process startup.
    await_status(pool, warmup, "PUBLISHED", &mut [&mut first]).await;
    CanaryStage::WarmupPublished.report();
    assert_eq!(
        repository
            .get_schedule(future.operation)
            .await
            .unwrap()
            .unwrap()
            .status,
        "PENDING"
    );
    assert_eq!(publication_counts(pool, future).await, (0, 0, 0));
    stop(&mut first).await;
    CanaryStage::FirstProcessStopped.report();
    for reservation in [future, revoked, unknown, agent] {
        make_due(pool, reservation).await;
    }
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(publication_counts(pool, future).await, (0, 0, 0));
    assert_eq!(
        repository
            .get_schedule(future.operation)
            .await
            .unwrap()
            .unwrap()
            .status,
        "PENDING"
    );
    CanaryStage::StoppedStateVerified.report();
    // Hold exactly the due document's mutation lock. Both owned child sessions
    // must reach blocked DB work before release, proving real contenders rather
    // than treating two alive/startup PIDs as concurrency evidence.
    let mut barrier = pool.begin().await.unwrap();
    sqlx::query("SELECT document_id FROM documents WHERE document_id = $1 FOR UPDATE")
        .bind(future.document.as_uuid())
        .fetch_one(&mut *barrier)
        .await
        .unwrap();
    let mut first = start(pool, storage_root.path(), &worker, "r5-first", runtime);
    let mut second = start(pool, storage_root.path(), &worker, "r5-second", runtime);
    CanaryStage::ContendersStarted.report();
    tokio::time::timeout(Duration::from_secs(30), async {
    loop {
        assert!(first.try_wait().unwrap().is_none() && second.try_wait().unwrap().is_none(), "a contender exited during startup");
        let waiting: i64 = sqlx::query_scalar("SELECT count(DISTINCT application_name) FROM pg_stat_activity WHERE datname = current_database() AND application_name IN ('r5-first', 'r5-second') AND wait_event_type = 'Lock'")
            .fetch_one(pool).await.unwrap();
        if waiting == 2 { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    }).await.expect("both scheduler processes must contend before the row-lock barrier is released");
    CanaryStage::ContendersBlocked.report();
    barrier.commit().await.unwrap();
    await_status(pool, future, "PUBLISHED", &mut [&mut first, &mut second]).await;
    await_status(pool, revoked, "TERMINAL", &mut [&mut first, &mut second]).await;
    await_status(pool, unknown, "TERMINAL", &mut [&mut first, &mut second]).await;
    await_status(pool, agent, "TERMINAL", &mut [&mut first, &mut second]).await;
    CanaryStage::ContendersCompleted.report();
    assert_eq!(publication_counts(pool, cancelled).await, (0, 0, 0));
    assert_eq!(
        repository
            .get_schedule(cancelled.operation)
            .await
            .unwrap()
            .unwrap()
            .status,
        "CANCELLED"
    );
    assert_eq!(publication_counts(pool, agent).await, (0, 0, 0));
    assert_eq!(publication_counts(pool, future).await, (1, 1, 1));
    assert_eq!(publication_counts(pool, revoked).await, (0, 0, 0));
    assert_eq!(publication_counts(pool, unknown).await, (0, 0, 0));
    let identity: (String, String, String, String) = sqlx::query_as("SELECT actor_identity_provider, actor_principal_id, data->'serviceExecutor'->>'identityProvider', data->'serviceExecutor'->>'principalId' FROM audit_outbox_events WHERE resource_id = $1 AND event_type = 'document.version.published'")
        .bind(future.document.as_uuid()).fetch_one(pool).await.unwrap();
    assert_eq!(
        identity,
        (
            runtime.mode().into(),
            runtime.requester().into(),
            "service".into(),
            "scheduler".into()
        )
    );
    let reasons: Vec<String> = sqlx::query_scalar("SELECT terminal_reason FROM document_publish_schedules WHERE publish_operation_id IN ($1, $2) ORDER BY terminal_reason")
        .bind(revoked.operation.as_uuid()).bind(unknown.operation.as_uuid()).fetch_all(pool).await.unwrap();
    assert_eq!(reasons, ["authorization_revoked", "identity_invalid"]);
    for (reservation, requester, reason) in [
        (revoked, runtime.requester(), "authorization_revoked"),
        (unknown, "missing-requester", "identity_invalid"),
        (agent, runtime.read_only_requester(), "authorization_revoked"),
    ] {
        let terminal: (String, String, String, String, String) = sqlx::query_as("SELECT actor_identity_provider, actor_principal_id, data->'serviceExecutor'->>'identityProvider', data->'serviceExecutor'->>'principalId', data->>'terminalReason' FROM audit_outbox_events WHERE resource_id = $1 AND event_type = 'document.version.publication.terminal'")
            .bind(reservation.document.as_uuid()).fetch_one(pool).await.unwrap();
        assert_eq!(
            terminal,
            (
                runtime.mode().into(),
                requester.into(),
                "service".into(),
                "scheduler".into(),
                reason.into()
            )
        );
        let current: Option<Uuid> =
            sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
                .bind(reservation.document.as_uuid())
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(current, None);
    }

    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(future.document.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(current, Some(future.version.as_uuid()));
    CanaryStage::AttributionVerified.report();
    stop(&mut first).await;
    stop(&mut second).await;
    CanaryStage::ContendersStopped.report();
    let mut restarted = start(pool, storage_root.path(), &worker, "r5-restarted", runtime);
    CanaryStage::Restarted.report();
    // Fresh work proves the restarted process actually polls before checking replay.
    let after_restart = reserve(&documents, &versions, "Synthetic second restart proof", runtime).await;
    make_due(pool, after_restart).await;
    await_status(pool, after_restart, "PUBLISHED", &mut [&mut restarted]).await;
    CanaryStage::RestartPublished.report();
    assert_eq!(publication_counts(pool, cancelled).await, (0, 0, 0));
    let cancelled_current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(cancelled.document.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(cancelled_current, None);
    let cancelled_projection: (String, Option<OffsetDateTime>) = sqlx::query_as(
        "SELECT lifecycle_state, scheduled_publish_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(cancelled.version.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(cancelled_projection, ("WORKING".into(), None));
    let cancel_audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_outbox_events WHERE resource_id = $1 AND event_type = 'document.version.publication.cancelled'",
    )
    .bind(cancelled.document.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(cancel_audit_count, 1);
    assert_eq!(
        repository
            .get_schedule(cancelled.operation)
            .await
            .unwrap()
            .unwrap()
            .status,
        "CANCELLED"
    );
    let cancellation_identity: (String, String, String, String, bool) = sqlx::query_as(
        "SELECT actor_identity_provider, actor_principal_id, data->>'documentVersionId', data->>'publishOperationId', data ? 'serviceExecutor' FROM audit_outbox_events WHERE resource_id = $1 AND event_type = 'document.version.publication.cancelled'",
    )
    .bind(cancelled.document.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        cancellation_identity,
        (
            runtime.mode().into(),
            runtime.requester().into(),
            cancelled.version.as_uuid().to_string(),
            cancelled.operation.as_uuid().to_string(),
            false,
        )
    );
    assert_eq!(publication_counts(pool, future).await, (1, 1, 1));
    stop(&mut restarted).await;
    database.close().await;
    CanaryStage::Complete.report();
}
