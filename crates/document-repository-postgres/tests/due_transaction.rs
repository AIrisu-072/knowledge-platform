#[path = "support/versioning.rs"]
mod support;

use std::{io::Cursor, sync::Arc};

use document_application::{
    CancelScheduleCommand, CreateDocumentCommand, CreateVersionCommand, DocumentService,
    DueExecutionOutcome, PublicationScheduleRepository, PublishOperationId, SchedulePublishCommand,
};
use document_domain::{DocumentVersionId, FolderId, MediaType, Metadata};
use support::{TestIds, actor, fixture, operation_id};
use time::OffsetDateTime;
use uuid::Uuid;

fn publish_id(value: u8) -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8004-{value:012x}")).unwrap(),
    )
    .unwrap()
}

async fn replacement(f: &support::Fixture) -> DocumentVersionId {
    let target = DocumentVersionId::from_uuid(Uuid::now_v7());
    f.service()
        .create_version(
            CreateVersionCommand::new(operation_id(70), f.document_id, target, 1, actor()).unwrap(),
            f.prepare("Scheduled replacement", 9).await,
        )
        .await
        .unwrap();
    target
}

async fn reserve(f: &support::Fixture, id: PublishOperationId, target: DocumentVersionId) {
    let scheduled_at = OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap();
    f.service()
        .schedule_publish(
            SchedulePublishCommand::new(id, f.document_id, target, 2, actor(), scheduled_at)
                .unwrap(),
        )
        .await
        .unwrap();
}

async fn force_due(f: &support::Fixture, id: PublishOperationId, target: DocumentVersionId) {
    let database_due: OffsetDateTime = sqlx::query_scalar("SELECT now() - INTERVAL '1 second'")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE document_publish_schedules SET scheduled_publish_at = $1, next_retry_at = NULL \
         WHERE publish_operation_id = $2",
    )
    .bind(database_due)
    .bind(id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE document_versions SET scheduled_publish_at = $1 WHERE document_version_id = $2",
    )
    .bind(database_due)
    .bind(target.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn due_later_publication_cannot_run_early_and_replays_one_ledger_pair() {
    let f = fixture().await;
    let target = replacement(&f).await;
    let id = publish_id(1);
    reserve(&f, id, target).await;
    assert_eq!(
        f.service().execute_due(id).await.unwrap(),
        DueExecutionOutcome::NotDue
    );
    force_due(&f, id, target).await;
    let service = f.service();
    let (first, second) = tokio::join!(service.execute_due(id), service.execute_due(id));
    let first = first.unwrap();
    let second = second.unwrap();
    assert!(matches!(first, DueExecutionOutcome::Published(_)));
    assert_eq!(
        second, first,
        "a concurrent runner must replay the published result"
    );
    let stored = f.repository.get_schedule(id).await.unwrap().unwrap();
    assert_eq!(stored.status, "PUBLISHED");
    let row: (i64, Option<Uuid>) =
        sqlx::query_as("SELECT revision,current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(row, (4, Some(target.as_uuid())));
    let ledger: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_publish_operations WHERE publish_operation_id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE aggregate_id = $1 AND event_type = 'DocumentVersionPublished'",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_outbox_events WHERE resource_id = $1 AND event_type = 'document.version.published'",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!((ledger, events, audits), (1, 1, 1));
}

#[tokio::test]
async fn initial_schedule_uses_same_publish_ledger_and_quality_gate() {
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
            title: "Scheduled initial".to_owned(),
            document_metadata: Metadata::from_map(serde_json::Map::new()),
            version_metadata: Metadata::from_map(serde_json::Map::new()),
            principal: actor(),
            original_filename: "initial.txt".to_owned(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(vec![7_u8; 3])),
        })
        .await
        .unwrap();
    let id = publish_id(2);
    f.service()
        .schedule_publish(
            SchedulePublishCommand::new(
                id,
                created.document_id(),
                created.document_version_id(),
                0,
                actor(),
                OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    force_due(&f, id, created.document_version_id()).await;
    assert!(matches!(
        f.service().execute_due(id).await.unwrap(),
        DueExecutionOutcome::Published(_)
    ));
    let row: (Option<Uuid>, i64) =
        sqlx::query_as("SELECT current_version_id,revision FROM documents WHERE document_id = $1")
            .bind(created.document_id().as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(row, (Some(created.document_version_id().as_uuid()), 2));
}

#[tokio::test]
async fn stale_revision_terminalizes_once_and_clears_projection() {
    let f = fixture().await;
    let target = replacement(&f).await;
    let id = publish_id(3);
    reserve(&f, id, target).await;
    force_due(&f, id, target).await;
    sqlx::query("UPDATE documents SET revision = revision + 1 WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        f.service().execute_due(id).await.unwrap(),
        DueExecutionOutcome::Terminal("stale_publication_intent".to_owned())
    );
    assert_eq!(
        f.service().execute_due(id).await.unwrap(),
        DueExecutionOutcome::Inactive
    );
    let row: (String, Option<OffsetDateTime>) = sqlx::query_as(
        "SELECT s.status,v.scheduled_publish_at FROM document_publish_schedules s \
         JOIN document_versions v ON v.document_version_id = s.target_document_version_id \
         WHERE s.publish_operation_id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(row, ("TERMINAL".to_owned(), None));
}

#[tokio::test]
async fn transient_inspection_failure_retries_the_same_id_then_publishes() {
    let f = fixture().await;
    let target = replacement(&f).await;
    let id = publish_id(4);
    reserve(&f, id, target).await;
    force_due(&f, id, target).await;
    sqlx::query(
        "DELETE FROM document_semantic_inspections WHERE file_id IN \
         (SELECT cr.file_id FROM content_items ci JOIN content_representations cr \
          ON cr.content_representation_id = ci.authoritative_representation_id \
          WHERE ci.document_version_id = $1)",
    )
    .bind(target.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    f.executor.set_unavailable();
    assert!(matches!(
        f.service().execute_due(id).await.unwrap(),
        DueExecutionOutcome::RetryScheduled(_)
    ));
    assert_eq!(
        f.repository.get_schedule(id).await.unwrap().unwrap().status,
        "PENDING"
    );
    f.executor.set_available();
    assert_eq!(
        f.service().execute_due(id).await.unwrap(),
        DueExecutionOutcome::NotDue
    );
    sqlx::query("UPDATE document_publish_schedules SET next_retry_at = now() - INTERVAL '1 second' WHERE publish_operation_id = $1")
        .bind(id.as_uuid()).execute(&f.pool).await.unwrap();
    assert!(matches!(
        f.service().execute_due(id).await.unwrap(),
        DueExecutionOutcome::Published(_)
    ));
}

#[tokio::test]
async fn manifest_and_quality_changes_terminalize_without_publication() {
    let f = fixture().await;
    let target = replacement(&f).await;
    let id = publish_id(5);
    reserve(&f, id, target).await;
    force_due(&f, id, target).await;
    let file_id: Uuid = sqlx::query_scalar(
        "SELECT cr.file_id FROM content_items ci JOIN content_representations cr \
         ON cr.content_representation_id = ci.authoritative_representation_id \
         WHERE ci.document_version_id = $1",
    )
    .bind(target.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE document_semantic_inspections SET editorial_provenance = $1 WHERE file_id = $2")
        .bind(serde_json::json!({"tracked_changes":[],"comments":[{"author_label":null,"timestamp":null,"resolved_state":"resolved","source_locator":"body/1","content":"comment"}],"document_author_labels":[],"last_modified_by":null,"modification_metadata":{}}))
        .bind(file_id).execute(&f.pool).await.unwrap();
    assert_eq!(
        f.service().execute_due(id).await.unwrap(),
        DueExecutionOutcome::Terminal("publish_quality_rejected".to_owned())
    );
    let published: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_publish_operations WHERE publish_operation_id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(published, 0);
    let audit_result: String = sqlx::query_scalar(
        "SELECT result FROM audit_outbox_events WHERE resource_id = $1 \
         AND event_type = 'document.version.publication.terminal'",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(audit_result, "failure");
}

#[tokio::test]
async fn cancellation_and_manifest_change_cannot_publish_stale_intent() {
    let f = fixture().await;
    let target = replacement(&f).await;
    let cancelled_id = publish_id(6);
    reserve(&f, cancelled_id, target).await;
    f.service()
        .cancel_schedule(
            CancelScheduleCommand::new(
                operation_id(71),
                cancelled_id,
                f.document_id,
                target,
                3,
                actor(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        f.service().execute_due(cancelled_id).await.unwrap(),
        DueExecutionOutcome::Inactive
    );
    let stale_id = publish_id(7);
    f.service()
        .schedule_publish(
            SchedulePublishCommand::new(
                stale_id,
                f.document_id,
                target,
                4,
                actor(),
                OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    force_due(&f, stale_id, target).await;
    sqlx::query("UPDATE document_versions SET title = 'Changed after acceptance' WHERE document_version_id = $1")
        .bind(target.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        f.service().execute_due(stale_id).await.unwrap(),
        DueExecutionOutcome::Terminal("stale_publication_intent".to_owned())
    );
    let published: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_publish_operations WHERE publish_operation_id IN ($1,$2)",
    )
    .bind(cancelled_id.as_uuid())
    .bind(stale_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(published, 0);
}

#[tokio::test]
async fn changed_current_terminalizes_without_ancestor_search_or_publish() {
    let f = fixture().await;
    let target = replacement(&f).await;
    let id = publish_id(8);
    reserve(&f, id, target).await;
    force_due(&f, id, target).await;
    sqlx::query("UPDATE documents SET current_version_id = NULL WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(matches!(
        f.service().execute_due(id).await.unwrap(),
        DueExecutionOutcome::Terminal(_)
    ));
    let published: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_publish_operations WHERE publish_operation_id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(published, 0);
}
