#[path = "support/versioning.rs"]
mod support;

use document_application::{
    ApplicationError, CreateVersionCommand, PublishDocumentCommand, PublishOperationId,
    VersioningRepository, WithdrawVersionCommand,
};
use document_domain::{DocumentId, DocumentVersionId, LifecycleState};
use support::{actor, fixture, operation_id};
use uuid::Uuid;

fn publish_id() -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str("01890f7a-6f6e-7b0a-8001-000000000001").unwrap(),
    )
    .unwrap()
}

fn withdrawal(
    value: u8,
    document_id: DocumentId,
    target: DocumentVersionId,
    revision: i64,
    reason: &str,
) -> WithdrawVersionCommand {
    WithdrawVersionCommand::new(
        operation_id(value),
        document_id,
        target,
        revision,
        actor(),
        reason,
    )
    .unwrap()
}

async fn published_replacement(f: &support::Fixture) -> DocumentVersionId {
    let service = f.service();
    let target = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(31), f.document_id, target, 1, actor()).unwrap();
    service
        .create_version(create, f.prepare("Replacement", 2).await)
        .await
        .unwrap();
    let publish =
        PublishDocumentCommand::new(publish_id(), f.document_id, target, 2, actor()).unwrap();
    service.publish_document(publish).await.unwrap();
    target
}

#[tokio::test]
async fn current_withdrawal_restores_only_immediate_safe_base_and_replays() {
    let f = fixture().await;
    let target = published_replacement(&f).await;
    let service = f.service();
    let command = withdrawal(32, f.document_id, target, 3, "superseded in error");
    let result = service.withdraw_version(command.clone()).await.unwrap();
    assert_eq!(result.former_current_version_id, Some(target));
    assert_eq!(result.resulting_current_version_id, Some(f.base_id));
    assert_eq!(result.resulting_revision, 4);
    assert_eq!(result.restoration_withheld_reason, None);
    assert_eq!(
        service.withdraw_version(command.clone()).await.unwrap(),
        result
    );
    assert_eq!(
        service
            .withdraw_version(withdrawal(32, f.document_id, target, 3, "different reason"))
            .await,
        Err(ApplicationError::Conflict)
    );
    let current: Uuid =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, f.base_id.as_uuid());
    let target_state = f
        .repository
        .get_version_snapshot(f.document_id, target)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        target_state.version().lifecycle_state(),
        LifecycleState::Withdrawn
    );
    let base_published_at: time::OffsetDateTime = sqlx::query_scalar(
        "SELECT published_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(f.base_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(base_published_at.unix_timestamp(), 0);
    let event: serde_json::Value = sqlx::query_scalar("SELECT payload FROM outbox_events WHERE aggregate_id = $1 AND event_type = 'DocumentVersionWithdrawn'")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        event["formerCurrentVersionId"],
        target.as_uuid().to_string()
    );
    assert_eq!(
        event["resultingCurrentVersionId"],
        f.base_id.as_uuid().to_string()
    );
    assert_eq!(event["reason"], "superseded in error");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE resource_id = $1 AND event_type = 'document.version.withdrawn'")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn first_version_withdrawal_has_null_current() {
    let f = fixture().await;
    let result = f
        .service()
        .withdraw_version(withdrawal(
            33,
            f.document_id,
            f.base_id,
            1,
            "withdraw original",
        ))
        .await
        .unwrap();
    assert_eq!(result.former_current_version_id, Some(f.base_id));
    assert_eq!(result.resulting_current_version_id, None);
    assert_eq!(result.restoration_withheld_reason, None);
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, None);
}

#[tokio::test]
async fn historical_withdrawal_preserves_current_and_unsafe_base_yields_null() {
    let f = fixture().await;
    let target = published_replacement(&f).await;
    let service = f.service();
    let historical = service
        .withdraw_version(withdrawal(
            34,
            f.document_id,
            f.base_id,
            3,
            "retire history",
        ))
        .await
        .unwrap();
    assert_eq!(historical.resulting_current_version_id, Some(target));
    let current = service
        .withdraw_version(withdrawal(35, f.document_id, target, 4, "remove current"))
        .await
        .unwrap();
    assert_eq!(current.resulting_current_version_id, None);
    assert!(current.restoration_withheld_reason.is_some());
}

#[tokio::test]
async fn missing_base_object_withholds_restoration_but_withdrawal_succeeds() {
    let f = fixture().await;
    let target = published_replacement(&f).await;
    f.storage.remove("objects/base");
    let result = f
        .service()
        .withdraw_version(withdrawal(36, f.document_id, target, 3, "base unavailable"))
        .await
        .unwrap();
    assert_eq!(result.resulting_current_version_id, None);
    assert!(result.restoration_withheld_reason.is_some());
    let state: String = sqlx::query_scalar(
        "SELECT lifecycle_state FROM document_versions WHERE document_version_id = $1",
    )
    .bind(target.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(state, "WITHDRAWN");
}

#[tokio::test]
async fn inspection_outage_withholds_restoration_without_blocking_withdrawal() {
    let f = fixture().await;
    let target = published_replacement(&f).await;
    sqlx::query("DELETE FROM document_semantic_inspections WHERE file_id IN (SELECT cr.file_id FROM content_items ci JOIN content_representations cr ON cr.content_representation_id = ci.authoritative_representation_id WHERE ci.document_version_id = $1)")
        .bind(f.base_id.as_uuid()).execute(&f.pool).await.unwrap();
    f.executor.set_unavailable();
    let result = f
        .service()
        .withdraw_version(withdrawal(
            41,
            f.document_id,
            target,
            3,
            "inspection offline",
        ))
        .await
        .unwrap();
    assert_eq!(result.resulting_current_version_id, None);
    assert_eq!(
        result.restoration_withheld_reason.as_deref(),
        Some("base_inspection_unavailable")
    );
}

#[tokio::test]
async fn withdrawal_does_not_search_past_immediate_withdrawn_base() {
    let f = fixture().await;
    let second = published_replacement(&f).await;
    let service = f.service();
    let third = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(42), f.document_id, third, 3, actor()).unwrap();
    service
        .create_version(create, f.prepare("Third", 3).await)
        .await
        .unwrap();
    let publish = PublishDocumentCommand::new(
        PublishOperationId::try_from_uuid(
            Uuid::parse_str("01890f7a-6f6e-7b0a-8001-000000000002").unwrap(),
        )
        .unwrap(),
        f.document_id,
        third,
        4,
        actor(),
    )
    .unwrap();
    service.publish_document(publish).await.unwrap();
    let historical = service
        .withdraw_version(withdrawal(43, f.document_id, second, 5, "retire second"))
        .await
        .unwrap();
    assert_eq!(historical.resulting_current_version_id, Some(third));
    let current = service
        .withdraw_version(withdrawal(44, f.document_id, third, 6, "retire third"))
        .await
        .unwrap();
    assert_eq!(current.resulting_current_version_id, None);
    let first_state: String = sqlx::query_scalar(
        "SELECT lifecycle_state FROM document_versions WHERE document_version_id = $1",
    )
    .bind(f.base_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(first_state, "PUBLISHED");
}

#[tokio::test]
async fn withdrawal_terminalizes_schedule_based_on_former_current() {
    let f = fixture().await;
    let target = published_replacement(&f).await;
    let service = f.service();
    let working = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(37), f.document_id, working, 3, actor()).unwrap();
    service
        .create_version(create, f.prepare("Future", 3).await)
        .await
        .unwrap();
    let snapshot = f
        .repository
        .get_version_snapshot(f.document_id, working)
        .await
        .unwrap()
        .unwrap();
    let digest = f
        .preflight()
        .inspect_existing(&snapshot)
        .await
        .unwrap()
        .identity_digest();
    let schedule_id = Uuid::parse_str("01890f7a-6f6e-7b0a-8002-000000000001").unwrap();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,base_document_version_id,current_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,$4,$4,3,4,to_timestamp(2000000000),'test-idp','editor',$5,'PENDING',to_timestamp(0))")
        .bind(schedule_id).bind(f.document_id.as_uuid()).bind(working.as_uuid()).bind(target.as_uuid())
        .bind(digest.to_vec()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE document_versions SET scheduled_publish_at = to_timestamp(2000000000) WHERE document_version_id = $1")
        .bind(working.as_uuid()).execute(&f.pool).await.unwrap();
    let result = service
        .withdraw_version(withdrawal(38, f.document_id, target, 4, "retract"))
        .await
        .unwrap();
    assert_eq!(result.resulting_current_version_id, Some(f.base_id));
    let status: String = sqlx::query_scalar(
        "SELECT status FROM document_publish_schedules WHERE publish_operation_id = $1",
    )
    .bind(schedule_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(status, "TERMINAL");
    let projection: Option<time::OffsetDateTime> = sqlx::query_scalar(
        "SELECT scheduled_publish_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(working.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(projection, None);
}

#[tokio::test]
async fn two_withdrawers_commit_one_transition() {
    let f = fixture().await;
    let target = published_replacement(&f).await;
    let service = f.service();
    let one = withdrawal(39, f.document_id, target, 3, "one");
    let two = withdrawal(40, f.document_id, target, 3, "two");
    let (a, b) = tokio::join!(service.withdraw_version(one), service.withdraw_version(two));
    assert_eq!(a.is_ok() as u8 + b.is_ok() as u8, 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM document_version_operations WHERE document_id = $1 AND operation_kind = 'WITHDRAW'")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 1);
}
