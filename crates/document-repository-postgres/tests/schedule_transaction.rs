#[path = "support/versioning.rs"]
mod support;

use std::{io::Cursor, sync::Arc};

use document_application::{
    ApplicationError, CancelScheduleCommand, CreateDocumentCommand, CreateVersionCommand,
    DocumentService, PublicationScheduleRepository, PublishDocumentCommand, PublishOperationId,
    SchedulePublishCommand, UpdateWorkingVersionCommand,
};
use document_domain::{DocumentVersionId, FolderId, MediaType, Metadata};
use support::{TestIds, actor, fixture, operation_id};
use time::OffsetDateTime;
use uuid::Uuid;

fn publish_id(value: u8) -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8003-{value:012x}")).unwrap(),
    )
    .unwrap()
}
fn due(value: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(value).unwrap()
}
fn schedule(
    value: u8,
    f: &support::Fixture,
    target: DocumentVersionId,
    revision: i64,
    at: i64,
) -> SchedulePublishCommand {
    SchedulePublishCommand::new(
        publish_id(value),
        f.document_id,
        target,
        revision,
        actor(),
        due(at),
    )
    .unwrap()
}
async fn working_replacement(f: &support::Fixture) -> DocumentVersionId {
    let target = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(51), f.document_id, target, 1, actor()).unwrap();
    f.service()
        .create_version(create, f.prepare("Scheduled", 2).await)
        .await
        .unwrap();
    target
}

#[tokio::test]
async fn later_version_reservation_replay_and_cancellation_are_atomic() {
    let f = fixture().await;
    let target = working_replacement(&f).await;
    let service = f.service();
    let command = schedule(1, &f, target, 2, 2_000_000_000);
    let accepted = service.schedule_publish(command.clone()).await.unwrap();
    assert_eq!(accepted.accepted_revision, 3);
    assert_eq!(
        service.schedule_publish(command.clone()).await.unwrap(),
        accepted
    );
    let stored = f
        .repository
        .get_schedule(command.publish_operation_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "PENDING");
    assert_eq!(stored.result, accepted);
    let projection: Option<OffsetDateTime> = sqlx::query_scalar(
        "SELECT scheduled_publish_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(target.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(projection, Some(due(2_000_000_000)));
    let approved: Option<OffsetDateTime> = sqlx::query_scalar(
        "SELECT approved_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(target.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert!(approved.is_some());
    assert_eq!(
        service
            .schedule_publish(schedule(1, &f, target, 2, 2_000_000_001))
            .await,
        Err(ApplicationError::Conflict)
    );
    assert_eq!(
        service
            .schedule_publish(schedule(2, &f, target, 3, 2_000_000_001))
            .await,
        Err(ApplicationError::BusinessRule)
    );
    let update =
        UpdateWorkingVersionCommand::new(operation_id(52), f.document_id, target, 3, actor())
            .unwrap();
    assert_eq!(
        service
            .update_working(update, f.prepare("Edit", 3).await)
            .await,
        Err(ApplicationError::BusinessRule)
    );
    let manual =
        PublishDocumentCommand::new(publish_id(3), f.document_id, target, 3, actor()).unwrap();
    assert_eq!(
        service.publish_document(manual).await,
        Err(ApplicationError::BusinessRule)
    );
    let cancel = CancelScheduleCommand::new(
        operation_id(53),
        publish_id(1),
        f.document_id,
        target,
        3,
        actor(),
    )
    .unwrap();
    let cancelled = service.cancel_schedule(cancel.clone()).await.unwrap();
    assert_eq!(cancelled.resulting_revision, 4);
    assert_eq!(
        service.cancel_schedule(cancel.clone()).await.unwrap(),
        cancelled
    );
    let mismatch = CancelScheduleCommand::new(
        operation_id(53),
        publish_id(2),
        f.document_id,
        target,
        3,
        actor(),
    )
    .unwrap();
    assert_eq!(
        service.cancel_schedule(mismatch).await,
        Err(ApplicationError::Conflict)
    );
    let stored = f
        .repository
        .get_schedule(publish_id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "CANCELLED");
    let projection: Option<OffsetDateTime> = sqlx::query_scalar(
        "SELECT scheduled_publish_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(target.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(projection, None);
    let rescheduled = service
        .schedule_publish(schedule(4, &f, target, 4, 2_000_000_010))
        .await
        .unwrap();
    assert_eq!(rescheduled.accepted_revision, 5);
    let event_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE aggregate_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let audit_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE resource_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!((event_count, audit_count), (4, 4));
}

#[tokio::test]
async fn initial_version_can_be_reserved_without_changing_lifecycle() {
    let f = fixture().await;
    let document_service = DocumentService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        f.repository.clone(),
    );
    let created = document_service
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(document_repository_postgres::SYSTEM_ROOT_FOLDER_ID),
            title: "Initial scheduled".to_owned(),
            document_metadata: Metadata::from_map(serde_json::Map::new()),
            version_metadata: Metadata::from_map(serde_json::Map::new()),
            principal: actor(),
            original_filename: "initial.txt".to_owned(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(vec![7_u8; 3])),
        })
        .await
        .unwrap();
    let command = SchedulePublishCommand::new(
        publish_id(5),
        created.document_id(),
        created.document_version_id(),
        0,
        actor(),
        due(2_000_000_000),
    )
    .unwrap();
    let accepted = f.service().schedule_publish(command).await.unwrap();
    assert_eq!(accepted.accepted_revision, 1);
    let row: (Option<Uuid>, Option<Uuid>, String) = sqlx::query_as(
        "SELECT s.base_document_version_id,s.current_version_id,v.lifecycle_state \
         FROM document_publish_schedules s JOIN document_versions v ON v.document_version_id = s.target_document_version_id \
         WHERE s.publish_operation_id = $1",
    ).bind(publish_id(5).as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(row, (None, None, "WORKING".to_owned()));
}

#[tokio::test]
async fn past_due_and_quality_failure_leave_no_reservation() {
    let f = fixture().await;
    let target = working_replacement(&f).await;
    let service = f.service();
    assert!(matches!(
        service
            .schedule_publish(schedule(6, &f, target, 2, 1))
            .await,
        Err(ApplicationError::Validation(_))
    ));
    let file_id: Uuid = sqlx::query_scalar(
        "SELECT cr.file_id FROM content_items ci JOIN content_representations cr \
         ON cr.content_representation_id = ci.authoritative_representation_id WHERE ci.document_version_id = $1",
    ).bind(target.as_uuid()).fetch_one(&f.pool).await.unwrap();
    sqlx::query("UPDATE document_semantic_inspections SET editorial_provenance = $1 WHERE file_id = $2")
        .bind(serde_json::json!({"tracked_changes":[],"comments":[{"author_label":null,"timestamp":null,"resolved_state":"resolved","source_locator":"body/1","content":"comment"}],"document_author_labels":[],"last_modified_by":null,"modification_metadata":{}}))
        .bind(file_id).execute(&f.pool).await.unwrap();
    assert!(matches!(
        service
            .schedule_publish(schedule(7, &f, target, 2, 2_000_000_000))
            .await,
        Err(ApplicationError::PublishQualityRejected(_))
    ));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_publish_schedules WHERE document_id = $1",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn two_reservers_commit_only_one_pending_intent() {
    let f = fixture().await;
    let target = working_replacement(&f).await;
    let service = f.service();
    let first = schedule(8, &f, target, 2, 2_000_000_000);
    let second = schedule(9, &f, target, 2, 2_000_000_001);
    let (a, b) = tokio::join!(
        service.schedule_publish(first),
        service.schedule_publish(second)
    );
    assert_eq!(a.is_ok() as u8 + b.is_ok() as u8, 1);
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM document_publish_schedules WHERE document_id = $1 AND status = 'PENDING'")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(pending, 1);
}
