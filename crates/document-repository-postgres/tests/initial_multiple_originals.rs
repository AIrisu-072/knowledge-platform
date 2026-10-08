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
        OffsetDateTime::now_utc()
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
