#[path = "support/versioning.rs"]
mod support;

use std::{io::Cursor, sync::Arc};

use document_application::{
    ApplicationError, CreateDocumentCommand, CreateVersionCommand, DocumentPublicationEndService,
    DocumentRepository, DocumentService, DueExecutionOutcome, EndDocumentPublicationCommand,
    PublicationEndOperationId, PublicationScheduleRepository, PublishDocumentCommand,
    PublishOperationId, SchedulePublishCommand, VersioningRepository,
};
use document_domain::{DocumentVersionId, FolderId, LifecycleState, MediaType, Metadata};
use support::{TestIds, actor, fixture, operation_id};
use time::OffsetDateTime;
use uuid::Uuid;

fn publish_id(value: u8) -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8003-{value:012x}")).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn publication_history_schedule_and_end_are_one_real_database_path() {
    let f = fixture().await;
    let service = DocumentService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        f.repository.clone(),
    );
    let created = service
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(document_repository_postgres::SYSTEM_ROOT_FOLDER_ID),
            title: "Initial".to_owned(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: actor(),
            original_filename: "initial.txt".to_owned(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(vec![7_u8; 3])),
        })
        .await
        .unwrap();
    let initial_publish = PublishDocumentCommand::new(
        publish_id(1),
        created.document_id(),
        created.document_version_id(),
        0,
        actor(),
    )
    .unwrap();
    service
        .publish_document(initial_publish.clone())
        .await
        .unwrap();
    let first_revision: (i64, i64, Uuid) = sqlx::query_as(
        "SELECT major_no,minor_no,document_version_id FROM document_revisions \
         WHERE document_id = $1 ORDER BY major_no DESC,minor_no DESC LIMIT 1",
    )
    .bind(created.document_id().as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        first_revision,
        (1, 0, created.document_version_id().as_uuid())
    );

    let versioning = f.service();
    let successor = DocumentVersionId::from_uuid(Uuid::now_v7());
    versioning
        .create_version(
            CreateVersionCommand::new(
                operation_id(1),
                created.document_id(),
                successor,
                1,
                actor(),
            )
            .unwrap(),
            f.prepare("Second", 8).await,
        )
        .await
        .unwrap();
    versioning
        .publish_document(
            PublishDocumentCommand::new(
                publish_id(2),
                created.document_id(),
                successor,
                2,
                actor(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let second_revision: (i64, i64, Uuid, String) = sqlx::query_as(
        "SELECT major_no,minor_no,document_version_id,source_kind FROM document_revisions \
         WHERE document_id = $1 ORDER BY major_no DESC,minor_no DESC LIMIT 1",
    )
    .bind(created.document_id().as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        second_revision,
        (2, 0, successor.as_uuid(), "contentPublication".to_owned())
    );
    let scheduled = DocumentVersionId::from_uuid(Uuid::now_v7());
    versioning
        .create_version(
            CreateVersionCommand::new(
                operation_id(2),
                created.document_id(),
                scheduled,
                3,
                actor(),
            )
            .unwrap(),
            f.prepare("Third", 9).await,
        )
        .await
        .unwrap();
    versioning
        .schedule_publish(
            SchedulePublishCommand::new(
                publish_id(3),
                created.document_id(),
                scheduled,
                4,
                actor(),
                OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let revisions_after_schedule: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_revisions WHERE document_id = $1")
            .bind(created.document_id().as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        revisions_after_schedule, 2,
        "schedule acceptance is not a revision"
    );

    let end = DocumentPublicationEndService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.repository.clone(),
    );
    let command = EndDocumentPublicationCommand::new(
        PublicationEndOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
        created.document_id(),
        5,
        successor,
        actor(),
        "obsolete policy".to_owned(),
    )
    .unwrap();
    let result = end.end_document_publication(command.clone()).await.unwrap();
    assert_eq!(result.resulting_document_revision(), 6);
    assert_eq!(end.end_document_publication(command).await.unwrap(), result);
    assert!(matches!(
        service.get_document(created.document_id()).await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert!(matches!(
        service
            .get_current_published_document(created.document_id())
            .await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert!(
        !f.repository
            .is_current_published_version(created.document_id(), successor)
            .await
            .unwrap()
    );
    assert!(
        !f.repository
            .list_current_published_versions(None, 1000)
            .await
            .unwrap()
            .iter()
            .any(|entry| entry.document_id() == created.document_id())
    );
    assert_eq!(
        versioning.execute_due(publish_id(3)).await.unwrap(),
        DueExecutionOutcome::Inactive
    );
    let intent = f
        .repository
        .get_schedule(publish_id(3))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(intent.status, "TERMINAL");
    for version_id in [created.document_version_id(), successor] {
        let snapshot = f
            .repository
            .get_version_snapshot(created.document_id(), version_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot.version().lifecycle_state(),
            LifecycleState::Published
        );
    }
    assert_eq!(
        service
            .publish_document(initial_publish)
            .await
            .unwrap()
            .resulting_document_revision(),
        1
    );
    let domain_count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE aggregate_id = $1 AND event_type = 'DocumentPublicationEnded'")
        .bind(created.document_id().as_uuid()).fetch_one(&f.pool).await.unwrap();
    let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE resource_id = $1 AND event_type = 'document.publication.ended'")
        .bind(created.document_id().as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!((domain_count, audit_count), (1, 1));
    let revisions_after_end: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_revisions WHERE document_id = $1")
            .bind(created.document_id().as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        revisions_after_end, 2,
        "publication end alone is not a revision"
    );
}
