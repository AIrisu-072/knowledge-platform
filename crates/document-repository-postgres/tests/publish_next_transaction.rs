#[path = "support/versioning.rs"]
mod support;

use std::{io::Cursor, sync::Arc};

use document_application::{
    ApplicationError, CreateDocumentCommand, CreateVersionCommand, DocumentService,
    PublishDocumentCommand, PublishOperationId, VersioningItemInput, VersioningRepository,
};
use document_domain::{
    DocumentVersionId, FileId, FolderId, LifecycleState, LogicalPath, MediaType, Metadata, Title,
};
use document_semantic_inspection_core::InspectionProfileVersion;
use support::{TestIds, actor, fixture, install_new_current, operation_id};
use uuid::Uuid;

fn publish_id(value: u8) -> PublishOperationId {
    PublishOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8001-{value:012x}")).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn replacement_publish_switches_current_and_replays_once() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(21), f.document_id, target_id, 1, actor()).unwrap();
    service
        .create_version(create, f.prepare("Replacement", 2).await)
        .await
        .unwrap();
    let command =
        PublishDocumentCommand::new(publish_id(1), f.document_id, target_id, 2, actor()).unwrap();
    let published = service.publish_document(command.clone()).await.unwrap();
    assert_eq!(published.resulting_document_revision(), 3);
    let issued_revision: (i64, i64, Uuid, String, Option<Uuid>) = sqlx::query_as(
        "SELECT major_no,minor_no,document_version_id,source_kind,operation_id \
         FROM document_revisions WHERE document_id = $1 ORDER BY major_no DESC,minor_no DESC LIMIT 1",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        issued_revision,
        (
            2,
            0,
            target_id.as_uuid(),
            "contentPublication".to_owned(),
            Some(publish_id(1).as_uuid())
        )
    );
    assert_eq!(
        service.publish_document(command.clone()).await.unwrap(),
        published
    );
    let current: Uuid =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, target_id.as_uuid());
    let revision_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_revisions WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        revision_count, 2,
        "publish replay cannot create another revision"
    );
    let old: String = sqlx::query_scalar(
        "SELECT lifecycle_state FROM document_versions WHERE document_version_id = $1",
    )
    .bind(f.base_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(old, "PUBLISHED");
    let target = f
        .repository
        .get_version_snapshot(f.document_id, target_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        target.version().lifecycle_state(),
        LifecycleState::Published
    );
    let bad_replay =
        PublishDocumentCommand::new(publish_id(1), f.document_id, f.base_id, 2, actor()).unwrap();
    assert_eq!(
        service.publish_document(bad_replay).await,
        Err(ApplicationError::OperationConflict)
    );
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
    assert_eq!((event_count, audit_count), (2, 2));
}

#[tokio::test]
async fn stale_base_and_pending_schedule_cannot_publish() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(22), f.document_id, target_id, 1, actor()).unwrap();
    service
        .create_version(create, f.prepare("Replacement", 2).await)
        .await
        .unwrap();
    let prepared = f
        .repository
        .get_version_snapshot(f.document_id, target_id)
        .await
        .unwrap()
        .unwrap();
    let target_digest = f
        .preflight()
        .inspect_existing(&prepared)
        .await
        .unwrap()
        .identity_digest();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,base_document_version_id,current_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,$4,$4,1,2,to_timestamp(2000000000),'test-idp','editor',$5,'PENDING',to_timestamp(0))")
        .bind(publish_id(2).as_uuid()).bind(f.document_id.as_uuid()).bind(target_id.as_uuid())
        .bind(f.base_id.as_uuid()).bind(target_digest.to_vec()).execute(&f.pool).await.unwrap();
    let manual =
        PublishDocumentCommand::new(publish_id(3), f.document_id, target_id, 2, actor()).unwrap();
    assert_eq!(
        service.publish_document(manual).await,
        Err(ApplicationError::BusinessRule)
    );
    sqlx::query("UPDATE document_publish_schedules SET status = 'CANCELLED', cancelled_at = to_timestamp(1) WHERE publish_operation_id = $1")
        .bind(publish_id(2).as_uuid()).execute(&f.pool).await.unwrap();
    let new_current = install_new_current(&f, "New current", 3).await;
    assert_ne!(new_current, f.base_id);
    let stale =
        PublishDocumentCommand::new(publish_id(4), f.document_id, target_id, 3, actor()).unwrap();
    assert_eq!(
        service.publish_document(stale).await,
        Err(ApplicationError::Conflict)
    );
}

#[tokio::test]
async fn publication_quality_rejects_embedded_comments() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(23), f.document_id, target_id, 1, actor()).unwrap();
    let prepared = f
        .preflight()
        .prepare(
            Title::new("Replacement").unwrap(),
            vec![
                VersioningItemInput::new(
                    LogicalPath::new("primary").unwrap(),
                    0,
                    FileId::from_uuid(Uuid::now_v7()),
                    MediaType::new("text/plain").unwrap(),
                    "main.txt",
                    Box::pin(Cursor::new(vec![2_u8; 3])),
                ),
                VersioningItemInput::new(
                    LogicalPath::new("appendix").unwrap(),
                    1,
                    FileId::from_uuid(Uuid::now_v7()),
                    MediaType::new("text/plain").unwrap(),
                    "appendix.txt",
                    Box::pin(Cursor::new(vec![3_u8; 3])),
                ),
            ],
            InspectionProfileVersion::DsiV0,
        )
        .await
        .unwrap();
    let file_id = prepared.items()[1].file().file_id().as_uuid();
    service.create_version(create, prepared).await.unwrap();
    sqlx::query("UPDATE document_semantic_inspections SET editorial_provenance = $1 WHERE file_id = $2")
        .bind(serde_json::json!({"tracked_changes":[],"comments":[{"author_label":null,"timestamp":null,"resolved_state":"resolved","source_locator":"body/1","content":"comment"}],"document_author_labels":[],"last_modified_by":null,"modification_metadata":{}}))
        .bind(file_id).execute(&f.pool).await.unwrap();
    let command =
        PublishDocumentCommand::new(publish_id(5), f.document_id, target_id, 2, actor()).unwrap();
    assert!(matches!(
        service.publish_document(command).await,
        Err(ApplicationError::PublishQualityRejected(_))
    ));
    let current: Uuid =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, f.base_id.as_uuid());
}

#[tokio::test]
async fn manual_initial_publish_contract_still_works() {
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
            title: "Initial".to_owned(),
            document_metadata: Metadata::from_map(serde_json::Map::new()),
            version_metadata: Metadata::from_map(serde_json::Map::new()),
            principal: actor(),
            original_filename: "initial.txt".to_owned(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(vec![7_u8; 3])),
        })
        .await
        .unwrap();
    let command = PublishDocumentCommand::new(
        publish_id(6),
        created.document_id(),
        created.document_version_id(),
        0,
        actor(),
    )
    .unwrap();
    let result = document_service.publish_document(command).await.unwrap();
    assert_eq!(result.resulting_document_revision(), 1);

    let scheduled_style = document_service
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(document_repository_postgres::SYSTEM_ROOT_FOLDER_ID),
            title: "Inspected initial".to_owned(),
            document_metadata: Metadata::from_map(serde_json::Map::new()),
            version_metadata: Metadata::from_map(serde_json::Map::new()),
            principal: actor(),
            original_filename: "inspected.txt".to_owned(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(vec![8_u8; 3])),
        })
        .await
        .unwrap();
    let inspected_command = PublishDocumentCommand::new(
        publish_id(7),
        scheduled_style.document_id(),
        scheduled_style.document_version_id(),
        0,
        actor(),
    )
    .unwrap();
    let inspected = f
        .service()
        .publish_document(inspected_command)
        .await
        .unwrap();
    assert_eq!(inspected.resulting_document_revision(), 1);
}
