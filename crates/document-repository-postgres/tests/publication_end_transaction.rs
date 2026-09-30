#[path = "support/versioning.rs"]
mod support;

use std::sync::Arc;

use document_application::{
    ApplicationError, DocumentPublicationEndService, EndDocumentPublicationCommand,
    PublicationEndOperationId,
};
use document_domain::DocumentVersionId;
use sqlx::Row;
use support::{Fixture, TestIds, actor, fixture};
use time::OffsetDateTime;
use uuid::Uuid;

fn operation_id(value: u8) -> PublicationEndOperationId {
    PublicationEndOperationId::try_from_uuid(
        Uuid::parse_str(&format!("01890f7a-6f6e-7b0a-8004-{value:012x}")).unwrap(),
    )
    .unwrap()
}

fn command(
    f: &Fixture,
    value: u8,
    revision: i64,
    current: DocumentVersionId,
    reason: &str,
) -> EndDocumentPublicationCommand {
    EndDocumentPublicationCommand::new(
        operation_id(value),
        f.document_id,
        revision,
        current,
        actor(),
        reason.to_owned(),
    )
    .unwrap()
}

fn service(
    f: &Fixture,
) -> DocumentPublicationEndService<
    TestIds,
    support::TestClock,
    document_repository_postgres::PostgresDocumentRepository,
> {
    DocumentPublicationEndService::new(Arc::new(TestIds), f.clock.clone(), f.repository.clone())
}

async fn seed_successor_schedule(f: &Fixture) -> Uuid {
    let target_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO document_versions \
         (document_version_id,document_id,version_no,base_document_version_id,lifecycle_state,title, \
          scheduled_publish_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) \
         VALUES ($1,$2,2,$3,'WORKING','Scheduled',to_timestamp(2000000000),'test-idp','editor','{}',to_timestamp(1))",
    )
    .bind(target_id)
    .bind(f.document_id.as_uuid())
    .bind(f.base_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO document_publish_schedules \
         (publish_operation_id,document_id,target_document_version_id,base_document_version_id,current_version_id, \
          expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider, \
          actor_principal_id,manifest_digest,status,created_at) \
         VALUES ($1,$2,$3,$4,$4,1,2,to_timestamp(2000000000),'test-idp','editor',$5,'PENDING',to_timestamp(1))",
    )
    .bind(Uuid::now_v7())
    .bind(f.document_id.as_uuid())
    .bind(target_id)
    .bind(f.base_id.as_uuid())
    .bind(vec![9_u8; 32])
    .execute(&f.pool)
    .await
    .unwrap();
    target_id
}

async fn count(f: &Fixture, table: &str) -> i64 {
    let statement = match table {
        "file_objects" => "SELECT count(*) FROM file_objects",
        "document_publication_end_operations" => {
            "SELECT count(*) FROM document_publication_end_operations"
        }
        "outbox_events" => "SELECT count(*) FROM outbox_events",
        "audit_outbox_events" => "SELECT count(*) FROM audit_outbox_events",
        _ => panic!("unsupported test table"),
    };
    sqlx::query_scalar(statement)
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn publication_end_is_atomic_replayable_and_preserves_published_history() {
    let f = fixture().await;
    let target_id = seed_successor_schedule(&f).await;
    let original_published_at: Option<OffsetDateTime> = sqlx::query_scalar(
        "SELECT published_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(f.base_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    f.storage.remove("objects/base");
    f.executor.set_unavailable();
    let service = service(&f);
    let request = command(&f, 1, 1, f.base_id, "retired document");

    let result = service
        .end_document_publication(request.clone())
        .await
        .unwrap();
    assert_eq!(result.operation_id(), operation_id(1));
    assert_eq!(result.former_current_version_id(), f.base_id);
    assert_eq!(result.resulting_current_version_id(), None);
    assert_eq!(result.resulting_document_revision(), 2);
    let document: (Option<Uuid>, i64) =
        sqlx::query_as("SELECT current_version_id,revision FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(document, (None, 2));
    let version: (String, Option<OffsetDateTime>) = sqlx::query_as(
        "SELECT lifecycle_state,published_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(f.base_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(version, ("PUBLISHED".to_owned(), original_published_at));
    assert_eq!(count(&f, "file_objects").await, 1);
    let schedule: (String, Option<String>) = sqlx::query_as(
        "SELECT status,terminal_reason FROM document_publish_schedules WHERE target_document_version_id = $1",
    )
    .bind(target_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        schedule,
        (
            "TERMINAL".to_owned(),
            Some("document_publication_ended".to_owned())
        )
    );
    let projection: Option<OffsetDateTime> = sqlx::query_scalar(
        "SELECT scheduled_publish_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(target_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(projection, None);

    let operation = sqlx::query(
        "SELECT document_id,former_current_version_id,command_digest,reason,resulting_document_revision,ended_at \
         FROM document_publication_end_operations WHERE operation_id = $1",
    )
    .bind(operation_id(1).as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        operation.get::<Uuid, _>("document_id"),
        f.document_id.as_uuid()
    );
    assert_eq!(
        operation.get::<Uuid, _>("former_current_version_id"),
        f.base_id.as_uuid()
    );
    assert_eq!(
        operation.get::<Vec<u8>, _>("command_digest"),
        request.command_digest()
    );
    assert_eq!(operation.get::<String, _>("reason"), "retired document");
    assert_eq!(operation.get::<i64, _>("resulting_document_revision"), 2);
    assert_eq!(
        operation.get::<OffsetDateTime, _>("ended_at"),
        result.ended_at()
    );
    let domain_event = sqlx::query(
        "SELECT payload FROM outbox_events WHERE event_type = 'DocumentPublicationEnded'",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        domain_event.get::<serde_json::Value, _>("payload")["invalidatedScheduleCount"],
        1
    );
    let audit_event = sqlx::query(
        "SELECT data FROM audit_outbox_events WHERE event_type = 'document.publication.ended'",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        audit_event.get::<serde_json::Value, _>("data")["reason"],
        "retired document"
    );

    assert_eq!(
        service
            .end_document_publication(request.clone())
            .await
            .unwrap(),
        result
    );
    assert_eq!(
        service
            .end_document_publication(command(&f, 1, 1, f.base_id, "different"))
            .await,
        Err(ApplicationError::OperationConflict)
    );
    assert_eq!(
        service
            .end_document_publication(command(&f, 2, 2, f.base_id, "another operation"))
            .await,
        Err(ApplicationError::BusinessRule)
    );
    assert_eq!(count(&f, "document_publication_end_operations").await, 1);
    assert_eq!(count(&f, "outbox_events").await, 1);
    assert_eq!(count(&f, "audit_outbox_events").await, 1);
}

#[tokio::test]
async fn audit_outbox_failure_rolls_back_publication_end_and_schedule_changes() {
    let f = fixture().await;
    let target_id = seed_successor_schedule(&f).await;
    sqlx::query(
        "CREATE FUNCTION reject_publication_end_audit() RETURNS trigger AS $$ \
         BEGIN IF NEW.event_type = 'document.publication.ended' THEN \
         RAISE EXCEPTION 'test audit rejection'; END IF; RETURN NEW; END $$ LANGUAGE plpgsql",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER reject_publication_end_audit BEFORE INSERT ON audit_outbox_events \
         FOR EACH ROW EXECUTE FUNCTION reject_publication_end_audit()",
    )
    .execute(&f.pool)
    .await
    .unwrap();

    assert!(matches!(
        service(&f)
            .end_document_publication(command(&f, 1, 1, f.base_id, "retire"))
            .await,
        Err(ApplicationError::Internal(_))
    ));
    let document: (Option<Uuid>, i64) =
        sqlx::query_as("SELECT current_version_id,revision FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(document, (Some(f.base_id.as_uuid()), 1));
    let schedule: String = sqlx::query_scalar(
        "SELECT status FROM document_publish_schedules WHERE target_document_version_id = $1",
    )
    .bind(target_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(schedule, "PENDING");
    let projection: Option<OffsetDateTime> = sqlx::query_scalar(
        "SELECT scheduled_publish_at FROM document_versions WHERE document_version_id = $1",
    )
    .bind(target_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert!(projection.is_some());
    assert_eq!(count(&f, "document_publication_end_operations").await, 0);
    assert_eq!(count(&f, "outbox_events").await, 0);
    assert_eq!(count(&f, "audit_outbox_events").await, 0);
}

#[tokio::test]
async fn concurrent_different_operation_ids_end_a_document_once() {
    let f = fixture().await;
    let service = service(&f);
    let first = command(&f, 1, 1, f.base_id, "retire");
    let second = command(&f, 2, 1, f.base_id, "retire");

    let (left, right) = tokio::join!(
        service.end_document_publication(first),
        service.end_document_publication(second)
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let loser = if left.is_err() { left } else { right };
    assert!(matches!(
        loser,
        Err(ApplicationError::Conflict | ApplicationError::BusinessRule)
    ));
    assert_eq!(count(&f, "document_publication_end_operations").await, 1);
    assert_eq!(count(&f, "outbox_events").await, 1);
    assert_eq!(count(&f, "audit_outbox_events").await, 1);
}

#[tokio::test]
async fn publication_end_rejects_unpublished_and_stale_documents() {
    let f = fixture().await;
    let service = service(&f);
    assert_eq!(
        service
            .end_document_publication(command(&f, 1, 0, f.base_id, "retire"))
            .await,
        Err(ApplicationError::Conflict)
    );
    assert_eq!(
        service
            .end_document_publication(command(
                &f,
                2,
                1,
                DocumentVersionId::from_uuid(Uuid::now_v7()),
                "retire"
            ))
            .await,
        Err(ApplicationError::Conflict)
    );
    sqlx::query("UPDATE documents SET current_version_id = NULL WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        service
            .end_document_publication(command(&f, 3, 1, f.base_id, "retire"))
            .await,
        Err(ApplicationError::BusinessRule)
    );
    sqlx::query("UPDATE document_versions SET lifecycle_state = 'WORKING', published_at = NULL WHERE document_version_id = $1")
        .bind(f.base_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(f.base_id.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        service
            .end_document_publication(command(&f, 4, 1, f.base_id, "retire"))
            .await,
        Err(ApplicationError::BusinessRule)
    );
    assert_eq!(count(&f, "document_publication_end_operations").await, 0);
}
