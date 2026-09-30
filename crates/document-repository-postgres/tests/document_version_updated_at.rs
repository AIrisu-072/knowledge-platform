#[path = "support/versioning.rs"]
mod support;

use document_application::{
    CancelScheduleCommand, CreateVersionCommand, PublishDocumentCommand, PublishOperationId,
    SchedulePublishCommand, UpdateWorkingVersionCommand, WithdrawVersionCommand,
};
use document_domain::DocumentVersionId;
use support::{actor, fixture, operation_id};
use time::OffsetDateTime;
use uuid::Uuid;

fn publish_id(value: u8) -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8004-{value:012x}")).unwrap(),
    )
    .unwrap()
}

fn due(value: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(value).unwrap()
}

async fn updated_at(f: &support::Fixture, version_id: DocumentVersionId) -> OffsetDateTime {
    sqlx::query_scalar("SELECT updated_at FROM document_versions WHERE document_version_id = $1")
        .bind(version_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn version_projection_timestamp_tracks_content_schedule_publication_and_withdrawal() {
    let f = fixture().await;
    let service = f.service();
    let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    service
        .create_version(
            CreateVersionCommand::new(operation_id(71), f.document_id, version_id, 1, actor())
                .unwrap(),
            f.prepare("Working", 2).await,
        )
        .await
        .unwrap();
    let (created_at, initial_updated_at): (OffsetDateTime, OffsetDateTime) = sqlx::query_as(
        "SELECT created_at, updated_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(version_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(initial_updated_at, created_at);

    service
        .update_working(
            UpdateWorkingVersionCommand::new(
                operation_id(72),
                f.document_id,
                version_id,
                2,
                actor(),
            )
            .unwrap(),
            f.prepare("Revised", 3).await,
        )
        .await
        .unwrap();
    let content_updated_at = updated_at(&f, version_id).await;
    assert!(content_updated_at > initial_updated_at);

    let schedule_id = publish_id(73);
    service
        .schedule_publish(
            SchedulePublishCommand::new(
                schedule_id,
                f.document_id,
                version_id,
                3,
                actor(),
                due(2_000_000_000),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let schedule_updated_at = updated_at(&f, version_id).await;
    assert!(schedule_updated_at > content_updated_at);

    service
        .cancel_schedule(
            CancelScheduleCommand::new(
                operation_id(74),
                schedule_id,
                f.document_id,
                version_id,
                4,
                actor(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let cancelled_updated_at = updated_at(&f, version_id).await;
    assert!(cancelled_updated_at > schedule_updated_at);

    service
        .publish_document(
            PublishDocumentCommand::new(publish_id(75), f.document_id, version_id, 5, actor())
                .unwrap(),
        )
        .await
        .unwrap();
    let published_updated_at = updated_at(&f, version_id).await;
    assert!(published_updated_at > cancelled_updated_at);

    service
        .withdraw_version(
            WithdrawVersionCommand::new(
                operation_id(76),
                f.document_id,
                version_id,
                6,
                actor(),
                "remove current version",
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(updated_at(&f, version_id).await > published_updated_at);
}
