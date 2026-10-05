#[path = "support/versioning.rs"]
mod support;

use std::{io::Cursor, sync::Arc};

use document_application::{
    Clock, CreateDocumentCommand, CreateVersionCommand, DocumentService, DueExecutionOutcome,
    PublicationScheduleRepository, PublishDocumentCommand, PublishOperationId,
    RebaseWorkingVersionCommand, ReconciliationClassification, SchedulePublishCommand,
    UpdateWorkingVersionCommand, VersioningItemInput, VersioningPreflight, WithdrawVersionCommand,
};
use document_domain::{
    DocumentVersionId, FileId, FolderId, LogicalPath, MediaType, Metadata, Title,
};
use document_semantic_inspection_core::InspectionProfileVersion;
use document_storage_fs::FileSystemStorage;
use support::{TestIds, actor, fixture, operation_id};
use tempfile::TempDir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[tokio::test]
async fn initial_to_replacement_schedule_withdrawal_and_rebase_preserve_one_lineage() {
    let f = fixture().await;
    let document_service = DocumentService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        f.repository.clone(),
    );
    let initial = document_service
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(document_repository_postgres::SYSTEM_ROOT_FOLDER_ID),
            title: "Initial version".to_owned(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: actor(),
            original_filename: "initial.txt".to_owned(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(b"Initial authoritative text".to_vec())),
        })
        .await
        .unwrap();
    let document_id = initial.document_id();
    let base_id = initial.document_version_id();
    let service = f.service();
    service
        .publish_document(
            PublishDocumentCommand::new(
                PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                document_id,
                base_id,
                0,
                actor(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let manual_audit: serde_json::Value = sqlx::query_scalar(
        "SELECT data FROM audit_outbox_events WHERE resource_id = $1 \
         AND event_type = 'document.version.published'",
    )
    .bind(document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert!(manual_audit.get("serviceExecutor").is_none());

    let replacement_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let created = service
        .create_version(
            CreateVersionCommand::new(operation_id(81), document_id, replacement_id, 1, actor())
                .unwrap(),
            f.prepare("Replacement", 2).await,
        )
        .await
        .unwrap();
    assert_eq!((created.version_no(), created.resulting_revision()), (2, 2));
    let updated = service
        .update_working(
            UpdateWorkingVersionCommand::new(
                operation_id(82),
                document_id,
                replacement_id,
                2,
                actor(),
            )
            .unwrap(),
            f.prepare("Replacement revised", 3).await,
        )
        .await
        .unwrap();
    assert_eq!(updated.resulting_revision(), 3);

    let publish_id = PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap();
    let scheduled = service
        .schedule_publish(
            SchedulePublishCommand::new(
                publish_id,
                document_id,
                replacement_id,
                3,
                actor(),
                OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(scheduled.accepted_revision, 4);
    let due: OffsetDateTime = sqlx::query_scalar("SELECT now() - INTERVAL '1 second'")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE document_publish_schedules SET scheduled_publish_at = $1 WHERE publish_operation_id = $2")
        .bind(due).bind(publish_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query(
        "UPDATE document_versions SET scheduled_publish_at = $1 WHERE document_version_id = $2",
    )
    .bind(due)
    .bind(replacement_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(matches!(
        service.execute_due(publish_id).await.unwrap(),
        DueExecutionOutcome::Published(_)
    ));
    assert_eq!(
        f.repository
            .get_schedule(publish_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "PUBLISHED"
    );

    let third_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let third = service
        .create_version(
            CreateVersionCommand::new(operation_id(83), document_id, third_id, 5, actor()).unwrap(),
            f.prepare("Third", 4).await,
        )
        .await
        .unwrap();
    assert_eq!(
        (third.version_no(), third.base_version_id()),
        (3, Some(replacement_id))
    );
    let withdrawn = service
        .withdraw_version(
            WithdrawVersionCommand::new(
                operation_id(84),
                document_id,
                replacement_id,
                6,
                actor(),
                "Superseded publication withdrawn",
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(withdrawn.resulting_current_version_id, Some(base_id));
    let rebased = service
        .rebase_working(
            RebaseWorkingVersionCommand::new(operation_id(85), document_id, third_id, 7, actor())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        (rebased.version_no(), rebased.base_version_id()),
        (3, Some(base_id))
    );
    let row: (Option<Uuid>, i64) =
        sqlx::query_as("SELECT current_version_id,revision FROM documents WHERE document_id = $1")
            .bind(document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(row, (Some(base_id.as_uuid()), 8));
    let state: String = sqlx::query_scalar(
        "SELECT lifecycle_state FROM document_versions WHERE document_version_id = $1",
    )
    .bind(replacement_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(state, "WITHDRAWN");
    let canonical_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM content_items WHERE document_version_id IN ($1,$2,$3)",
    )
    .bind(base_id.as_uuid())
    .bind(replacement_id.as_uuid())
    .bind(third_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(canonical_count, 3);
}

struct LateClock;
impl Clock for LateClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap()
    }
}

#[tokio::test]
async fn unreferenced_prepared_file_is_visible_to_reconciliation_after_grace() {
    let f = fixture().await;
    let root = TempDir::new().unwrap();
    let storage = Arc::new(FileSystemStorage::new(root.path()));
    let orphan_id = FileId::from_uuid(Uuid::now_v7());
    VersioningPreflight::new(
        f.repository.clone(),
        storage.clone(),
        f.executor.clone(),
        f.clock.clone(),
    )
    .prepare(
        Title::new("Uncommitted version candidate").unwrap(),
        vec![VersioningItemInput::new(
            LogicalPath::new("primary").unwrap(),
            0,
            orphan_id,
            MediaType::new("text/plain").unwrap(),
            "orphan.txt",
            Box::pin(Cursor::new(b"Uncommitted candidate".to_vec())),
        )],
        InspectionProfileVersion::DsiV0,
    )
    .await
    .unwrap();
    let findings = DocumentService::new(
        Arc::new(TestIds),
        Arc::new(LateClock),
        storage,
        f.repository.clone(),
    )
    .reconcile_storage(Duration::hours(1))
    .await
    .unwrap();
    assert!(findings.iter().any(|finding| {
        finding.file_id() == orphan_id
            && finding.classification() == ReconciliationClassification::Orphan
    }));
}
