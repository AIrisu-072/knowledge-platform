#[path = "support/versioning.rs"]
mod support;

use std::sync::Arc;

use document_application::{
    ApplicationError, CreateVersionCommand, DocumentPublicationEndService, DueExecutionOutcome,
    EndDocumentPublicationCommand, PublicationEndOperationId, PublishDocumentCommand,
    PublishOperationId, RebaseWorkingVersionCommand, SchedulePublishCommand,
    UpdateWorkingVersionCommand, WithdrawVersionCommand,
};
use document_domain::DocumentVersionId;
use support::{Fixture, TestIds, actor, fixture, operation_id};
use time::OffsetDateTime;
use uuid::Uuid;

fn end_id(value: u8) -> PublicationEndOperationId {
    PublicationEndOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8004-{value:012x}")).unwrap(),
    )
    .unwrap()
}

fn publish_id(value: u8) -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8003-{value:012x}")).unwrap(),
    )
    .unwrap()
}

fn end_command(
    f: &Fixture,
    value: u8,
    revision: i64,
    current: DocumentVersionId,
) -> EndDocumentPublicationCommand {
    EndDocumentPublicationCommand::new(
        end_id(value),
        f.document_id,
        revision,
        current,
        actor(),
        "end public access".to_owned(),
    )
    .unwrap()
}

fn end_service(
    f: &Fixture,
) -> DocumentPublicationEndService<
    TestIds,
    support::TestClock,
    document_repository_postgres::PostgresDocumentRepository,
> {
    DocumentPublicationEndService::new(Arc::new(TestIds), f.clock.clone(), f.repository.clone())
}

async fn working(f: &Fixture) -> DocumentVersionId {
    let target = DocumentVersionId::from_uuid(Uuid::now_v7());
    f.service()
        .create_version(
            CreateVersionCommand::new(operation_id(1), f.document_id, target, 1, actor()).unwrap(),
            f.prepare("Replacement", 2).await,
        )
        .await
        .unwrap();
    target
}

#[tokio::test]
async fn ended_document_rejects_manual_initial_publish_even_with_a_working_target() {
    let f = fixture().await;
    end_service(&f)
        .end_document_publication(end_command(&f, 1, 1, f.base_id))
        .await
        .unwrap();
    // Model an old/imported initial target. The end ledger must still be decisive.
    sqlx::query(
        "UPDATE document_versions SET lifecycle_state = 'WORKING', published_at = NULL \
         WHERE document_version_id = $1",
    )
    .bind(f.base_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    let result = f
        .service()
        .publish_document(
            PublishDocumentCommand::new(publish_id(1), f.document_id, f.base_id, 2, actor())
                .unwrap(),
        )
        .await;
    assert_eq!(result, Err(ApplicationError::BusinessRule));
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, None);
}

#[tokio::test]
async fn end_blocks_new_version_schedule_and_publish_but_allows_historical_withdrawal() {
    let f = fixture().await;
    let target = working(&f).await;
    let schedule = SchedulePublishCommand::new(
        publish_id(2),
        f.document_id,
        target,
        2,
        actor(),
        OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap(),
    )
    .unwrap();
    f.service().schedule_publish(schedule).await.unwrap();
    end_service(&f)
        .end_document_publication(end_command(&f, 2, 3, f.base_id))
        .await
        .unwrap();
    let service = f.service();
    let new_target = DocumentVersionId::from_uuid(Uuid::now_v7());
    assert!(matches!(
        service
            .create_version(
                CreateVersionCommand::new(operation_id(2), f.document_id, new_target, 4, actor())
                    .unwrap(),
                f.prepare("New", 3).await,
            )
            .await,
        Err(ApplicationError::BusinessRule | ApplicationError::Conflict)
    ));
    assert!(matches!(
        service
            .update_working(
                UpdateWorkingVersionCommand::new(
                    operation_id(3),
                    f.document_id,
                    target,
                    4,
                    actor(),
                )
                .unwrap(),
                f.prepare("Edited", 4).await,
            )
            .await,
        Err(ApplicationError::BusinessRule | ApplicationError::Conflict)
    ));
    assert!(matches!(
        service
            .rebase_working(
                RebaseWorkingVersionCommand::new(
                    operation_id(4),
                    f.document_id,
                    target,
                    4,
                    actor(),
                )
                .unwrap(),
            )
            .await,
        Err(ApplicationError::BusinessRule | ApplicationError::Conflict)
    ));
    assert!(matches!(
        service
            .publish_document(
                PublishDocumentCommand::new(publish_id(3), f.document_id, target, 4, actor())
                    .unwrap(),
            )
            .await,
        Err(ApplicationError::BusinessRule | ApplicationError::Conflict)
    ));
    assert!(matches!(
        service
            .schedule_publish(
                SchedulePublishCommand::new(
                    publish_id(4),
                    f.document_id,
                    target,
                    4,
                    actor(),
                    OffsetDateTime::from_unix_timestamp(2_000_000_001).unwrap(),
                )
                .unwrap(),
            )
            .await,
        Err(ApplicationError::BusinessRule | ApplicationError::Conflict)
    ));
    assert_eq!(
        service.execute_due(publish_id(2)).await.unwrap(),
        DueExecutionOutcome::Inactive
    );
    let withdrawn = service
        .withdraw_version(
            WithdrawVersionCommand::new(
                operation_id(5),
                f.document_id,
                f.base_id,
                4,
                actor(),
                "retire historical version",
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(withdrawn.resulting_current_version_id, None);
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, None);
}

#[tokio::test]
async fn due_publish_and_end_race_never_resurrects_an_ended_document() {
    let f = fixture().await;
    let target = working(&f).await;
    f.service()
        .schedule_publish(
            SchedulePublishCommand::new(
                publish_id(5),
                f.document_id,
                target,
                2,
                actor(),
                OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE document_publish_schedules SET scheduled_publish_at = to_timestamp(1) \
         WHERE publish_operation_id = $1",
    )
    .bind(publish_id(5).as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE document_versions SET scheduled_publish_at = to_timestamp(1) \
         WHERE document_version_id = $1",
    )
    .bind(target.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();

    let end = end_service(&f);
    let service = f.service();
    let (ended, due) = tokio::join!(
        end.end_document_publication(end_command(&f, 3, 3, f.base_id)),
        service.execute_due(publish_id(5)),
    );
    let due = due.unwrap();
    match ended {
        // A worker can read PENDING before T10, then observe the terminal row in is_due.
        Ok(_) => assert!(
            matches!(
                due,
                DueExecutionOutcome::Inactive
                    | DueExecutionOutcome::NotDue
                    | DueExecutionOutcome::Terminal(_)
            ),
            "due outcome after successful end: {due:?}"
        ),
        Err(ApplicationError::Conflict) => {
            assert!(matches!(due, DueExecutionOutcome::Published(_)));
            end_service(&f)
                .end_document_publication(end_command(&f, 4, 4, target))
                .await
                .unwrap();
        }
        other => panic!("unexpected end result: {other:?}"),
    }
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, None);
    assert_eq!(
        f.service().execute_due(publish_id(5)).await.unwrap(),
        if matches!(&due, DueExecutionOutcome::Published(_)) {
            due
        } else {
            DueExecutionOutcome::Inactive
        }
    );
}
