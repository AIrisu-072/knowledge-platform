#[path = "support/database.rs"]
mod database;

use database::TestDatabase;
use document_application::{
    AccessPolicyService, BootstrapRootPolicy, Clock, CreateDocumentCommand, DocumentService,
    IdGenerator, IdentityContextResolver, ManagementCommand, ManagementOperationId,
    PublicationScheduleRepository, authorize_scheduled_publish,
};
use document_domain::{
    Action, FolderId, MediaType, Metadata, PolicyGrant, PolicyMode, PolicySubject,
    PolicySubjectKind, PolicyTarget, PrincipalRef,
};
use document_publication_scheduler::{StaticRequesterResolver, scheduler_executor};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use document_storage_fs::FileSystemStorage;
use std::{io::Cursor, sync::Arc};
use time::OffsetDateTime;
use uuid::Uuid;

struct Ids;
impl IdGenerator for Ids {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}
struct UtcClock;
impl Clock for UtcClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
fn grant(group: &str, actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Group, "poc", group).unwrap(),
        actions,
    )
    .unwrap()
}

#[tokio::test]
async fn fixed_requester_uses_current_database_acl_and_executor_never_grants_publish() {
    let db = TestDatabase::new().await;
    migrate(&db.pool).await.unwrap();
    let resolver = StaticRequesterResolver::for_runtime_mode("poc").unwrap();
    let human = PrincipalRef::new("poc", "poc-human").unwrap();
    let human_context = resolver.resolve(&human).await.unwrap();
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        db.pool.clone(),
        human.clone(),
    ));
    repository
        .initialize_root_policy(
            &human_context,
            vec![
                grant(
                    "poc-users",
                    [
                        Action::Read,
                        Action::Write,
                        Action::Publish,
                        Action::Administer,
                    ],
                ),
                grant("poc-agents", [Action::Read, Action::ReadHistory]),
            ],
        )
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let created = DocumentService::new(
        Arc::new(Ids),
        Arc::new(UtcClock),
        Arc::new(FileSystemStorage::new(root.path())),
        repository.clone(),
    )
    .create_document(CreateDocumentCommand {
        folder_id: FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
        title: "Synthetic scheduler authorization".into(),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: human.clone(),
        original_filename: "synthetic.txt".into(),
        media_type: MediaType::new("text/plain").unwrap(),
        content: Box::pin(Cursor::new(b"Synthetic authorization fixture\n".to_vec())),
    })
    .await
    .unwrap();
    let due = authorize_scheduled_publish(&resolver, &human, &scheduler_executor())
        .await
        .unwrap();
    assert!(
        repository
            .authorize_due_document(&due, created.document_id())
            .await
            .unwrap()
    );
    let agent = authorize_scheduled_publish(
        &resolver,
        &PrincipalRef::new("poc", "poc-agent").unwrap(),
        &scheduler_executor(),
    )
    .await
    .unwrap();
    assert!(
        !repository
            .authorize_due_document(&agent, created.document_id())
            .await
            .unwrap()
    );
    AccessPolicyService::new(repository.clone())
        .set_access_policy(
            &human_context,
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(created.document_id()),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![grant("poc-users", [Action::Read])]),
                reason: "Synthetic revocation before due execution".into(),
            },
        )
        .await
        .unwrap();
    // Even the already-resolved requester is subject to the current DB policy.
    assert!(
        !repository
            .authorize_due_document(&due, created.document_id())
            .await
            .unwrap()
    );
    let refreshed = authorize_scheduled_publish(&resolver, &human, &scheduler_executor())
        .await
        .unwrap();
    assert!(
        !repository
            .authorize_due_document(&refreshed, created.document_id())
            .await
            .unwrap()
    );
    assert_eq!(refreshed.subjects(), human_context.subjects());
    db.close().await;
}
