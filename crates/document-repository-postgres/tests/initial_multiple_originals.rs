#[path = "support/management.rs"]
mod support;

use document_application::{
    ApplicationError, BootstrapRootPolicy, Clock, CreateDocumentItem, CreateDocumentItemsCommand,
    CreateOutcomeProbe, CreateOutcomeRecoveryService, DocumentRepository, DocumentService,
    FileStorage, IdGenerator,
};
use document_domain::{
    Action, LogicalPath, MediaType, Metadata, PolicyGrant, PolicySubject, PolicySubjectKind,
};
use document_storage_fs::FileSystemStorage;
use std::{io::Cursor, sync::Arc};
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

struct Ids;
impl IdGenerator for Ids {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}
struct Now;
impl Clock for Now {
    fn now(&self) -> OffsetDateTime {
        // A deterministic whole-second synthetic clock is exactly representable
        // by PostgreSQL's microsecond timestamps, including strict replay equality.
        OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
    }
}

fn command(folder_id: document_domain::FolderId) -> CreateDocumentItemsCommand {
    CreateDocumentItemsCommand {
        folder_id,
        title: "合成複数原本".into(),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: support::actor(),
        items: [("b.txt", 1, "second"), ("a.txt", 0, "first")]
            .into_iter()
            .map(|(path, ordinal, bytes)| CreateDocumentItem {
                logical_path: LogicalPath::new(path).unwrap(),
                ordinal,
                original_filename: path.into(),
                media_type: MediaType::new("text/plain").unwrap(),
                content: Box::pin(Cursor::new(bytes.as_bytes().to_vec())),
            })
            .collect(),
    }
}

#[tokio::test]
async fn initial_multiple_transaction_full_recovery_and_adapter_restart() {
    let fixture = isolated_fixture().await;
    let grant = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        [Action::Read, Action::Write, Action::Administer],
    )
    .unwrap();
    fixture
        .repository
        .initialize_root_policy(&support::context(), vec![grant])
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(FileSystemStorage::new(directory.path()));
    let scoped = Arc::new(fixture.repository.with_verified_actor(support::context()));
    let service = DocumentService::new(Arc::new(Ids), Arc::new(Now), storage.clone(), scoped);
    let result = service
        .create_document_items(command(fixture.root_id))
        .await
        .unwrap();
    let ids = result.file_ids().unwrap().to_vec();
    assert_eq!(ids.len(), 2);
    let counts: (i64, i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM content_items WHERE document_version_id=$1), (SELECT count(*) FROM content_representations r JOIN content_items i USING(content_item_id) WHERE i.document_version_id=$1), (SELECT count(*) FROM outbox_events WHERE aggregate_id=$2), (SELECT count(*) FROM audit_outbox_events WHERE resource_id=$2)")
        .bind(result.document_version_id().as_uuid()).bind(result.document_id().as_uuid()).fetch_one(&fixture.pool).await.unwrap();
    assert_eq!(counts, (2, 2, 2, 2));
    let probe = CreateOutcomeProbe {
        document_id: result.document_id(),
        document_version_id: result.document_version_id(),
        file_id: result.file_id(),
        file_ids: Some(ids.clone()),
    };
    let recovery = CreateOutcomeRecoveryService::new(fixture.repository.clone());
    assert_eq!(
        recovery
            .recover(&support::context(), probe.clone())
            .await
            .unwrap()
            .unwrap()
            .file_ids(),
        Some(ids.as_slice())
    );
    for file_ids in [
        None,
        Some(vec![ids[0]]),
        Some(vec![
            ids[0],
            document_domain::FileId::from_uuid(Uuid::now_v7()),
        ]),
        Some(vec![ids[1], ids[0]]),
    ] {
        let mismatch = CreateOutcomeProbe {
            file_id: file_ids.as_ref().map(|ids| ids[0]).unwrap_or(ids[0]),
            file_ids,
            ..probe.clone()
        };
        assert!(
            recovery
                .recover(&support::context(), mismatch)
                .await
                .unwrap()
                .is_none()
        );
    }
    drop(service);
    drop(storage);
    let restarted_repository =
        document_repository_postgres::PostgresDocumentRepository::new(fixture.pool.clone())
            .with_verified_actor(support::context());
    let restarted_storage = FileSystemStorage::new(directory.path());
    let document = restarted_repository
        .get_authoring_document(result.document_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(document.version().version_no().get(), 1);
    assert_eq!(
        document.version().lifecycle_state(),
        document_domain::LifecycleState::Working
    );
    assert!(document.document().current_version_id().is_none());
    for (item, expected) in document
        .content_items()
        .iter()
        .zip([b"first".as_slice(), b"second".as_slice()])
    {
        let mut bytes = Vec::new();
        restarted_storage
            .open(item.file().storage_key())
            .await
            .unwrap()
            .read_to_end(&mut bytes)
            .await
            .unwrap();
        assert_eq!(bytes, expected);
    }
    // Fail after documents, all originals and the first domain event were inserted.
    // The existing transaction must roll every authoritative and outbox row back.
    sqlx::query("ALTER TABLE outbox_events ADD CONSTRAINT synthetic_reject_version_event CHECK (event_type <> 'DocumentVersionCreated') NOT VALID")
        .execute(&fixture.pool).await.unwrap();
    let before: (i64, i64, i64, i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM documents),(SELECT count(*) FROM document_versions),(SELECT count(*) FROM file_objects),(SELECT count(*) FROM content_items),(SELECT count(*) FROM outbox_events),(SELECT count(*) FROM audit_outbox_events)")
        .fetch_one(&fixture.pool).await.unwrap();
    let rejected = DocumentService::new(
        Arc::new(Ids),
        Arc::new(Now),
        Arc::new(restarted_storage),
        Arc::new(restarted_repository),
    )
    .create_document_items(command(fixture.root_id))
    .await;
    assert!(matches!(rejected, Err(ApplicationError::Internal(_))));
    let after: (i64, i64, i64, i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM documents),(SELECT count(*) FROM document_versions),(SELECT count(*) FROM file_objects),(SELECT count(*) FROM content_items),(SELECT count(*) FROM outbox_events),(SELECT count(*) FROM audit_outbox_events)")
        .fetch_one(&fixture.pool).await.unwrap();
    assert_eq!(before, after);
    document_application::AccessPolicyService::new(fixture.repository.clone())
        .set_access_policy(
            &support::context(),
            document_application::ManagementCommand::SetAccessPolicy {
                operation_id: document_application::ManagementOperationId::try_from_uuid(
                    Uuid::now_v7(),
                )
                .unwrap(),
                target: document_domain::PolicyTarget::Document(result.document_id()),
                expected_policy_revision: 0,
                mode: document_domain::PolicyMode::Explicit(vec![
                    PolicyGrant::new(
                        PolicySubject::new(
                            PolicySubjectKind::Principal,
                            "test-idp",
                            "policy-admin",
                        )
                        .unwrap(),
                        [Action::Read],
                    )
                    .unwrap(),
                ]),
                reason: "synthetic remove authoring access".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        recovery
            .recover(&support::context(), probe)
            .await
            .unwrap()
            .is_none()
    );
}

struct IsolatedFixture {
    _container: testcontainers::ContainerAsync<testcontainers::GenericImage>,
    pool: sqlx::PgPool,
    repository: Arc<document_repository_postgres::PostgresDocumentRepository>,
    root_id: document_domain::FolderId,
}

async fn isolated_fixture() -> IsolatedFixture {
    use testcontainers::{
        GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_mapped_port(0, 5432.tcp())
        .with_container_name(format!("kp-originals-db-{}", Uuid::now_v7()))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "kp_originals_synthetic")
        .with_host_config_modifier(|config| {
            config.publish_all_ports = Some(false);
            for bindings in config
                .port_bindings
                .as_mut()
                .unwrap()
                .values_mut()
                .flatten()
            {
                for binding in bindings {
                    binding.host_ip = Some("127.0.0.1".into());
                }
            }
        })
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    eprintln!(
        "synthetic DB postgres:18.6-bookworm container={} endpoint=127.0.0.1:{port}",
        container.id()
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/kp_originals_synthetic"
        ))
        .await
        .unwrap();
    document_repository_postgres::migrate(&pool).await.unwrap();
    let repository = Arc::new(
        document_repository_postgres::PostgresDocumentRepository::new_with_bootstrap_actor(
            pool.clone(),
            support::actor(),
        ),
    );
    IsolatedFixture {
        _container: container,
        pool,
        repository,
        root_id: document_domain::FolderId::from_uuid(
            document_repository_postgres::SYSTEM_ROOT_FOLDER_ID,
        ),
    }
}

fn publication_command(
    created: &document_application::CreateDocumentResult,
    operation_id: document_application::PublishOperationId,
    expected_revision: i64,
) -> document_application::PublishDocumentCommand {
    document_application::PublishDocumentCommand::new(
        operation_id,
        created.document_id(),
        created.document_version_id(),
        expected_revision,
        support::actor(),
    )
    .unwrap()
}

type TestService = DocumentService<
    Ids,
    Now,
    FileSystemStorage,
    document_repository_postgres::PostgresDocumentRepository,
>;

// CI reported a stack overflow in this unoptimized integration test. Keep each
// service future behind a heap-allocated pointer instead of embedding its state
// in the test future; preserve the real service, repository and assertions.
fn publish_for_test(
    service: &TestService,
    command: document_application::PublishDocumentCommand,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Result<document_application::PublishDocumentResult, ApplicationError>,
            > + '_,
    >,
> {
    Box::pin(service.publish_document(command))
}

fn create_for_test(
    service: &TestService,
    command: CreateDocumentItemsCommand,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Result<document_application::CreateDocumentResult, ApplicationError>,
            > + '_,
    >,
> {
    Box::pin(service.create_document_items(command))
}

#[tokio::test]
async fn nested_initial_originals_publish_with_full_manifest_and_replay() {
    use document_application::{DocumentPublishRepository, PublishOperationId};
    let fixture = isolated_fixture().await;
    fixture
        .repository
        .initialize_root_policy(
            &support::context(),
            vec![
                PolicyGrant::new(
                    PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin")
                        .unwrap(),
                    [
                        Action::Read,
                        Action::Write,
                        Action::Publish,
                        Action::Administer,
                    ],
                )
                .unwrap(),
            ],
        )
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(FileSystemStorage::new(directory.path()));
    let scoped = Arc::new(fixture.repository.with_verified_actor(support::context()));
    let service = DocumentService::new(
        Arc::new(Ids),
        Arc::new(Now),
        storage.clone(),
        scoped.clone(),
    );
    let mut manifest = command(fixture.root_id);
    manifest.items[0].logical_path = LogicalPath::new("chapter/A.txt").unwrap();
    manifest.items[1].logical_path = LogicalPath::new("appendix/B.txt").unwrap();
    let created = create_for_test(&service, manifest).await.unwrap();
    let candidate = scoped
        .get_publish_candidate(created.document_id(), created.document_version_id())
        .await;
    let stale_id = PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap();
    assert!(matches!(
        publish_for_test(&service, publication_command(&created, stale_id, 1)).await,
        Err(ApplicationError::Conflict)
    ));
    let operation_id = PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap();
    let published = publish_for_test(&service, publication_command(&created, operation_id, 0))
        .await
        .expect(
            "initial publish must accept the complete nested manifest without a primary/0 anchor",
        );
    assert_eq!(candidate.unwrap().file().file_id(), created.file_id());
    assert_eq!(
        publish_for_test(&service, publication_command(&created, operation_id, 0))
            .await
            .unwrap(),
        published
    );
    assert!(matches!(
        publish_for_test(&service, publication_command(&created, operation_id, 1)).await,
        Err(ApplicationError::OperationConflict)
    ));
    assert_eq!(published.resulting_document_revision(), 1);
    let current = scoped
        .get_current_published_document(created.document_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        current.document().current_version_id(),
        Some(created.document_version_id())
    );
    assert_eq!(
        current.version().lifecycle_state(),
        document_domain::LifecycleState::Published
    );
    assert_eq!(
        current
            .content_items()
            .iter()
            .map(|item| (item.logical_path().as_str(), item.ordinal()))
            .collect::<Vec<_>>(),
        [("appendix/B.txt", 0), ("chapter/A.txt", 1)]
    );
    assert_eq!(
        current
            .content_items()
            .iter()
            .map(|item| item.file().file_id())
            .collect::<Vec<_>>(),
        created.file_ids().unwrap()
    );
    for (item, expected) in current
        .content_items()
        .iter()
        .zip([b"first".as_slice(), b"second".as_slice()])
    {
        let mut bytes = Vec::new();
        storage
            .open(item.file().storage_key())
            .await
            .unwrap()
            .read_to_end(&mut bytes)
            .await
            .unwrap();
        assert_eq!(bytes, expected);
    }
    let counts: (i64,i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM content_items WHERE document_version_id=$1), (SELECT count(*) FROM document_publish_operations WHERE document_id=$2), (SELECT count(*) FROM document_revisions WHERE document_id=$2), (SELECT count(*) FROM outbox_events WHERE aggregate_id=$2), (SELECT count(*) FROM audit_outbox_events WHERE resource_id=$2)")
        .bind(created.document_version_id().as_uuid()).bind(created.document_id().as_uuid()).fetch_one(&fixture.pool).await.unwrap();
    assert_eq!(counts, (2, 1, 1, 3, 3));
    let unclassified = create_for_test(&service, command(fixture.root_id))
        .await
        .unwrap();
    sqlx::query("UPDATE document_versions SET requires_content_classification=TRUE WHERE document_version_id=$1")
        .bind(unclassified.document_version_id().as_uuid()).execute(&fixture.pool).await.unwrap();
    assert!(matches!(
        publish_for_test(
            &service,
            publication_command(
                &unclassified,
                PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                0
            )
        )
        .await,
        Err(ApplicationError::BusinessRule)
    ));
    let empty = create_for_test(&service, command(fixture.root_id))
        .await
        .unwrap();
    sqlx::query("DELETE FROM content_items WHERE document_version_id=$1")
        .bind(empty.document_version_id().as_uuid())
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert_rejected_publication(&service, &fixture.pool, &empty).await;
    // These corruption cases change constraints only in this disposable fixture.
    // Normal schema constraints prohibit missing authoritative representations/files.
    sqlx::query(
        "ALTER TABLE content_items DROP CONSTRAINT fk_content_items_authoritative_representation",
    )
    .execute(&fixture.pool)
    .await
    .unwrap();
    let missing_rep = create_for_test(&service, command(fixture.root_id))
        .await
        .unwrap();
    sqlx::query("DELETE FROM content_representations WHERE file_id=$1")
        .bind(missing_rep.file_ids().unwrap()[1].as_uuid())
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert_rejected_publication(&service, &fixture.pool, &missing_rep).await;
    sqlx::query(
        "ALTER TABLE content_representations DROP CONSTRAINT content_representations_file_id_fkey",
    )
    .execute(&fixture.pool)
    .await
    .unwrap();
    let missing_file = create_for_test(&service, command(fixture.root_id))
        .await
        .unwrap();
    sqlx::query("DELETE FROM file_objects WHERE file_id=$1")
        .bind(missing_file.file_ids().unwrap()[1].as_uuid())
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert_rejected_publication(&service, &fixture.pool, &missing_file).await;
}

async fn assert_rejected_publication(
    service: &TestService,
    pool: &sqlx::PgPool,
    created: &document_application::CreateDocumentResult,
) {
    use document_application::PublishOperationId;
    let result = publish_for_test(
        service,
        publication_command(
            created,
            PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
            0,
        ),
    )
    .await;
    assert!(
        matches!(result, Err(ApplicationError::IntegrityViolation)),
        "invalid complete-manifest state must fail closed: {result:?}"
    );
    let state: (Option<Uuid>, i64, String, i64, i64, i64, i64) = sqlx::query_as("SELECT d.current_version_id,d.revision,v.lifecycle_state,(SELECT count(*) FROM document_publish_operations WHERE document_id=d.document_id),(SELECT count(*) FROM document_revisions WHERE document_id=d.document_id),(SELECT count(*) FROM outbox_events WHERE aggregate_id=d.document_id),(SELECT count(*) FROM audit_outbox_events WHERE resource_id=d.document_id) FROM documents d JOIN document_versions v ON v.document_id=d.document_id WHERE d.document_id=$1")
        .bind(created.document_id().as_uuid()).fetch_one(pool).await.unwrap();
    assert_eq!(state, (None, 0, "WORKING".to_owned(), 0, 0, 2, 2));
}
