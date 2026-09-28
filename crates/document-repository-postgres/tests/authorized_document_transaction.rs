#[path = "support/versioning.rs"]
mod support;

use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use document_application::{
    AccessPolicyService, ApplicationError, BootstrapRootPolicy, ContentReader,
    CreateDocumentCommand, CreateVersionCommand, DocumentPublicationEndService, DocumentRepository,
    DocumentService, DocumentVersionService, EndDocumentPublicationCommand,
    InspectionExecutionError, InvocationKind, ManagementCommand, ManagementOperationId,
    PublicationEndOperationId, PublishDocumentCommand, PublishOperationId,
    SemanticInspectionExecutor, VerifiedActorContext,
};
use document_domain::{
    Action, MediaType, Metadata, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind,
    PolicyTarget, PrincipalRef,
};
use document_repository_postgres::PostgresDocumentRepository;
use document_semantic_inspection_core::{WorkerRequest, WorkerResponse};
use support::{TestExecutor, TestIds, actor, fixture, operation_id};
use time::{Duration, OffsetDateTime};
use tokio::sync::Notify;
use uuid::Uuid;

struct PauseOnceExecutor {
    delegate: Arc<TestExecutor>,
    entered: Arc<Notify>,
    resume: Arc<Notify>,
    pause_next: AtomicBool,
}

impl SemanticInspectionExecutor for PauseOnceExecutor {
    async fn inspect(
        &self,
        request: WorkerRequest,
        content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
        if self.pause_next.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        self.delegate.inspect(request, content).await
    }
}

fn context() -> VerifiedActorContext {
    let principal = actor();
    let subject = PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![subject],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn grants(actions: impl IntoIterator<Item = Action>) -> Vec<PolicyGrant> {
    vec![
        PolicyGrant::new(
            PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap(),
            actions,
        )
        .unwrap(),
    ]
}

#[tokio::test]
async fn published_read_requires_current_policy_and_hides_unreadable_document() {
    let f = fixture().await;
    let repository = PostgresDocumentRepository::new_with_bootstrap_actor(f.pool.clone(), actor());
    let scoped = repository.with_verified_actor(context());
    assert!(
        scoped
            .get_current_published_document(f.document_id)
            .await
            .unwrap()
            .is_none()
    );
    repository
        .initialize_root_policy(&context(), grants([Action::Read]))
        .await
        .unwrap();
    assert!(
        scoped
            .get_current_published_document(f.document_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn initial_working_authoring_read_requires_read_and_write() {
    let f = fixture().await;
    sqlx::query(
        "UPDATE documents SET current_version_id = NULL, revision = 0 WHERE document_id = $1",
    )
    .bind(f.document_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE document_versions SET lifecycle_state = 'WORKING', published_at = NULL WHERE document_version_id = $1")
        .bind(f.base_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let repository = PostgresDocumentRepository::new_with_bootstrap_actor(f.pool.clone(), actor());
    repository
        .initialize_root_policy(
            &context(),
            grants([Action::Read, Action::Write, Action::Administer]),
        )
        .await
        .unwrap();
    let scoped = repository.with_verified_actor(context());
    assert_eq!(
        scoped
            .get_authoring_document(f.document_id)
            .await
            .unwrap()
            .unwrap()
            .version()
            .document_version_id(),
        f.base_id
    );
    assert!(
        scoped
            .get_current_published_document(f.document_id)
            .await
            .unwrap()
            .is_none()
    );
    AccessPolicyService::new(Arc::new(repository))
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants([Action::Read])),
                reason: "remove authoring right".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        scoped
            .get_authoring_document(f.document_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn disconnected_folder_policy_cannot_grant_document_read() {
    let f = fixture().await;
    let repository = PostgresDocumentRepository::new_with_bootstrap_actor(f.pool.clone(), actor());
    repository
        .initialize_root_policy(&context(), grants([Action::Read]))
        .await
        .unwrap();
    let orphan_folder_id = Uuid::now_v7();
    let policy_id = Uuid::now_v7();
    // A self-referencing Folder is disconnected from System Root without
    // violating the single-root constraint installed by M-B.
    sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$1,'Orphan','ACTIVE',0,now())")
        .bind(orphan_folder_id)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET folder_id = $1 WHERE document_id = $2")
        .bind(orphan_folder_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO access_policy_bindings (policy_id,folder_id,mode,revision,created_at,updated_at) VALUES ($1,$2,'EXPLICIT',1,now(),now())")
        .bind(policy_id)
        .bind(orphan_folder_id)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO access_policy_grants (policy_id,subject_kind,identity_provider,subject_id,action) VALUES ($1,'principal','test-idp','editor','read')")
        .bind(policy_id)
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(
        repository
            .with_verified_actor(context())
            .get_current_published_document(f.document_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn read_only_actor_cannot_publish_initial_working_version() {
    let f = fixture().await;
    sqlx::query(
        "UPDATE documents SET current_version_id = NULL, revision = 0 WHERE document_id = $1",
    )
    .bind(f.document_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE document_versions SET lifecycle_state = 'WORKING', published_at = NULL WHERE document_version_id = $1")
        .bind(f.base_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let repository = PostgresDocumentRepository::new_with_bootstrap_actor(f.pool.clone(), actor());
    repository
        .initialize_root_policy(&context(), grants([Action::Read]))
        .await
        .unwrap();
    let scoped = Arc::new(repository.with_verified_actor(context()));
    let service = DocumentVersionService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        f.executor.clone(),
        scoped,
    );
    let command = PublishDocumentCommand::new(
        PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
        f.document_id,
        f.base_id,
        0,
        actor(),
    )
    .unwrap();
    assert!(matches!(
        service.publish_document(command).await,
        Err(ApplicationError::Forbidden | ApplicationError::DocumentVersionNotFound)
    ));
    let state: (Option<Uuid>, i64) =
        sqlx::query_as("SELECT current_version_id, revision FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(state, (None, 0));
    let operation_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_publish_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(operation_count, 0);
}

#[tokio::test]
async fn create_document_requires_folder_read_and_write_inside_commit() {
    let f = fixture().await;
    let repository = PostgresDocumentRepository::new_with_bootstrap_actor(f.pool.clone(), actor());
    repository
        .initialize_root_policy(&context(), grants([Action::Read]))
        .await
        .unwrap();
    let scoped = Arc::new(repository.with_verified_actor(context()));
    let service = DocumentService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        scoped,
    );
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM documents")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let command = CreateDocumentCommand {
        folder_id: document_domain::FolderId::from_uuid(
            document_repository_postgres::SYSTEM_ROOT_FOLDER_ID,
        ),
        title: "Denied create".into(),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: PrincipalRef::new("test-idp", "editor").unwrap(),
        original_filename: "denied.txt".into(),
        media_type: MediaType::new("text/plain").unwrap(),
        content: Box::pin(Cursor::new(vec![1_u8, 2, 3])),
    };
    assert!(matches!(
        service.create_document(command).await,
        Err(ApplicationError::Forbidden)
    ));
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM documents")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn old_t10_result_cannot_be_replayed_after_publish_permission_is_revoked() {
    let f = fixture().await;
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        f.pool.clone(),
        actor(),
    ));
    repository
        .initialize_root_policy(
            &context(),
            grants([Action::Read, Action::Publish, Action::Administer]),
        )
        .await
        .unwrap();
    let scoped = Arc::new(repository.with_verified_actor(context()));
    let service =
        DocumentPublicationEndService::new(Arc::new(TestIds), f.clock.clone(), scoped.clone());
    let command = EndDocumentPublicationCommand::new(
        PublicationEndOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
        f.document_id,
        1,
        f.base_id,
        actor(),
        "end publication".into(),
    )
    .unwrap();
    service
        .end_document_publication(command.clone())
        .await
        .unwrap();
    assert!(
        scoped
            .get_current_published_document(f.document_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        scoped
            .get_authoring_document(f.document_id)
            .await
            .unwrap()
            .is_none()
    );
    AccessPolicyService::new(repository)
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants([Action::Read])),
                reason: "revoke publish".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        service.end_document_publication(command).await,
        Err(ApplicationError::Forbidden)
    ));
}

#[tokio::test]
async fn old_version_operation_cannot_be_replayed_after_write_permission_is_revoked() {
    let f = fixture().await;
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        f.pool.clone(),
        actor(),
    ));
    repository
        .initialize_root_policy(
            &context(),
            grants([Action::Read, Action::Write, Action::Administer]),
        )
        .await
        .unwrap();
    let scoped = Arc::new(repository.with_verified_actor(context()));
    let service = DocumentVersionService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        f.executor.clone(),
        scoped,
    );
    let target_id = document_domain::DocumentVersionId::from_uuid(Uuid::now_v7());
    let command =
        CreateVersionCommand::new(operation_id(41), f.document_id, target_id, 1, actor()).unwrap();
    let prepared = f.prepare("Changed", 2).await;
    service
        .create_version(command.clone(), prepared.clone())
        .await
        .unwrap();
    AccessPolicyService::new(repository)
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants([Action::Read])),
                reason: "revoke write".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        service.create_version(command, prepared).await,
        Err(ApplicationError::Forbidden)
    ));
}

#[tokio::test]
async fn permission_revoked_during_inspection_blocks_version_commit() {
    let f = fixture().await;
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        f.pool.clone(),
        actor(),
    ));
    repository
        .initialize_root_policy(
            &context(),
            grants([Action::Read, Action::Write, Action::Administer]),
        )
        .await
        .unwrap();
    let entered = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    let executor = Arc::new(PauseOnceExecutor {
        delegate: f.executor.clone(),
        entered: entered.clone(),
        resume: resume.clone(),
        pause_next: AtomicBool::new(true),
    });
    let service = DocumentVersionService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        executor,
        Arc::new(repository.with_verified_actor(context())),
    );
    let target_id = document_domain::DocumentVersionId::from_uuid(Uuid::now_v7());
    let command =
        CreateVersionCommand::new(operation_id(42), f.document_id, target_id, 1, actor()).unwrap();
    let prepared = f.prepare("Changed", 2).await;
    let task = tokio::spawn(async move { service.create_version(command, prepared).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), entered.notified())
        .await
        .unwrap();
    AccessPolicyService::new(repository)
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants([Action::Read])),
                reason: "revoke during inspection".into(),
            },
        )
        .await
        .unwrap();
    resume.notify_one();
    assert_eq!(task.await.unwrap(), Err(ApplicationError::Forbidden));
    let document: (Option<Uuid>, i64) =
        sqlx::query_as("SELECT current_version_id, revision FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(document, (Some(f.base_id.as_uuid()), 1));
    let operation_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_version_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(operation_count, 0);
}
