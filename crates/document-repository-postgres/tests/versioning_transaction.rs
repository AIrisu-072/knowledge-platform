#[path = "support/versioning.rs"]
mod support;

use document_application::{
    ApplicationError, CreateVersionCommand, RebaseWorkingVersionCommand,
    UpdateWorkingVersionCommand, VersioningRepository,
};
use document_domain::DocumentVersionId;
use support::{actor, fixture, initial_fixture, install_new_current, operation_id};
use uuid::Uuid;

#[tokio::test]
async fn create_update_replay_and_outbox_are_atomic() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let prepared = f.prepare("Changed", 2).await;
    let create =
        CreateVersionCommand::new(operation_id(1), f.document_id, target_id, 1, actor()).unwrap();
    let first = service
        .create_version(create.clone(), prepared.clone())
        .await
        .unwrap();
    assert_eq!(
        (
            first.version_no(),
            first.base_version_id(),
            first.resulting_revision()
        ),
        (2, Some(f.base_id), 2)
    );
    assert_eq!(
        service
            .create_version(create.clone(), prepared.clone())
            .await
            .unwrap(),
        first
    );
    assert_eq!(
        f.repository
            .get_version_operation(operation_id(1))
            .await
            .unwrap()
            .unwrap()
            .result(),
        &first
    );
    assert_eq!(
        service
            .create_version(create, f.prepare("Changed again", 3).await)
            .await,
        Err(ApplicationError::Conflict)
    );

    let update =
        UpdateWorkingVersionCommand::new(operation_id(2), f.document_id, target_id, 2, actor())
            .unwrap();
    let updated = service
        .update_working(update.clone(), f.prepare("Revised", 4).await)
        .await
        .unwrap();
    assert_eq!(
        (
            updated.version_no(),
            updated.target_version_id(),
            updated.resulting_revision()
        ),
        (2, target_id, 3)
    );
    assert_eq!(
        service
            .update_working(update, f.prepare("Other", 5).await)
            .await,
        Err(ApplicationError::Conflict)
    );
    let version_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_versions WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(version_count, 2);
    let public: (Option<Uuid>, String, String) = sqlx::query_as("SELECT d.current_version_id, v.lifecycle_state, v.title FROM documents d JOIN document_versions v ON v.document_version_id = d.current_version_id WHERE d.document_id = $1")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        public,
        (Some(f.base_id.as_uuid()), "PUBLISHED".into(), "Base".into())
    );
    let item_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM content_items WHERE document_version_id = $1")
            .bind(target_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(item_count, 1);
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
async fn no_change_missing_base_and_duplicate_working_fail_closed() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let same = f.prepare("Base", 1).await;
    let command =
        CreateVersionCommand::new(operation_id(3), f.document_id, target_id, 1, actor()).unwrap();
    assert_eq!(
        service.create_version(command, same).await,
        Err(ApplicationError::BusinessRule)
    );
    let changed = f.prepare("Changed", 2).await;
    let create =
        CreateVersionCommand::new(operation_id(4), f.document_id, target_id, 1, actor()).unwrap();
    service.create_version(create, changed).await.unwrap();
    let duplicate = CreateVersionCommand::new(
        operation_id(5),
        f.document_id,
        DocumentVersionId::from_uuid(Uuid::now_v7()),
        2,
        actor(),
    )
    .unwrap();
    assert_eq!(
        service
            .create_version(duplicate, f.prepare("Again", 3).await)
            .await,
        Err(ApplicationError::BusinessRule)
    );
    sqlx::query("UPDATE documents SET current_version_id = NULL, revision = revision + 1 WHERE document_id = $1")
        .bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let absent = CreateVersionCommand::new(
        operation_id(6),
        f.document_id,
        DocumentVersionId::from_uuid(Uuid::now_v7()),
        3,
        actor(),
    )
    .unwrap();
    assert_eq!(
        service
            .create_version(absent, f.prepare("Absent", 4).await)
            .await,
        Err(ApplicationError::BusinessRule)
    );
}

#[tokio::test]
async fn two_creators_allocate_only_one_working_number() {
    let f = fixture().await;
    let service = f.service();
    let one = CreateVersionCommand::new(
        operation_id(7),
        f.document_id,
        DocumentVersionId::from_uuid(Uuid::now_v7()),
        1,
        actor(),
    )
    .unwrap();
    let two = CreateVersionCommand::new(
        operation_id(8),
        f.document_id,
        DocumentVersionId::from_uuid(Uuid::now_v7()),
        1,
        actor(),
    )
    .unwrap();
    let prepared_one = f.prepare("One", 2).await;
    let prepared_two = f.prepare("Two", 3).await;
    let (a, b) = tokio::join!(
        service.create_version(one, prepared_one),
        service.create_version(two, prepared_two)
    );
    assert_eq!(a.is_ok() as u8 + b.is_ok() as u8, 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM document_versions WHERE document_id = $1 AND lifecycle_state = 'WORKING'")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn stale_working_requires_explicit_rebase_and_keeps_version_number() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(9), f.document_id, target_id, 1, actor()).unwrap();
    service
        .create_version(create, f.prepare("Working", 2).await)
        .await
        .unwrap();
    let new_current = install_new_current(&f, "New current", 3).await;
    let stale_update =
        UpdateWorkingVersionCommand::new(operation_id(10), f.document_id, target_id, 3, actor())
            .unwrap();
    assert_eq!(
        service
            .update_working(stale_update, f.prepare("Edited", 4).await)
            .await,
        Err(ApplicationError::Conflict)
    );
    let rebase =
        RebaseWorkingVersionCommand::new(operation_id(11), f.document_id, target_id, 3, actor())
            .unwrap();
    let rebased = service.rebase_working(rebase.clone()).await.unwrap();
    assert_eq!(
        (
            rebased.version_no(),
            rebased.base_version_id(),
            rebased.resulting_revision()
        ),
        (2, Some(new_current), 4)
    );
    assert_eq!(service.rebase_working(rebase).await.unwrap(), rebased);
    let persisted: (Uuid, i64) = sqlx::query_as("SELECT base_document_version_id, version_no FROM document_versions WHERE document_version_id = $1")
        .bind(target_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(persisted, (new_current.as_uuid(), 2));
    let event_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE aggregate_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(event_count, 2);
}

#[tokio::test]
async fn failed_update_keeps_previous_complete_manifest() {
    let f = fixture().await;
    let service = f.service();
    let target_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let create =
        CreateVersionCommand::new(operation_id(12), f.document_id, target_id, 1, actor()).unwrap();
    service
        .create_version(create, f.prepare("Working", 2).await)
        .await
        .unwrap();
    let before: (Uuid, String) = sqlx::query_as(
        "SELECT cr.file_id, v.title FROM document_versions v \
         JOIN content_items ci ON ci.document_version_id = v.document_version_id \
         JOIN content_representations cr ON cr.content_representation_id = ci.authoritative_representation_id \
         WHERE v.document_version_id = $1",
    )
    .bind(target_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    let prepared = f.prepare("Broken update", 4).await;
    let file_id = prepared.items()[0].file().file_id().as_uuid();
    sqlx::query(
        "UPDATE document_semantic_inspections SET fingerprint_digest = $1 WHERE file_id = $2",
    )
    .bind(vec![9_u8; 32])
    .bind(file_id)
    .execute(&f.pool)
    .await
    .unwrap();
    let update =
        UpdateWorkingVersionCommand::new(operation_id(13), f.document_id, target_id, 2, actor())
            .unwrap();
    assert_eq!(
        service.update_working(update, prepared).await,
        Err(ApplicationError::IntegrityViolation)
    );
    let after: (Uuid, String) = sqlx::query_as(
        "SELECT cr.file_id, v.title FROM document_versions v \
         JOIN content_items ci ON ci.document_version_id = v.document_version_id \
         JOIN content_representations cr ON cr.content_representation_id = ci.authoritative_representation_id \
         WHERE v.document_version_id = $1",
    )
    .bind(target_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(after, before);
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(revision, 2);
}

#[tokio::test]
async fn initial_working_update_preserves_identity_and_null_base_with_replay() {
    let f = initial_fixture().await;
    let prepared = f.prepare("Base", 1).await;
    let command =
        UpdateWorkingVersionCommand::new(operation_id(60), f.document_id, f.base_id, 0, actor())
            .unwrap();
    let result = f
        .service()
        .update_working(command.clone(), prepared.clone())
        .await
        .unwrap();
    assert_eq!(
        (
            result.target_version_id(),
            result.version_no(),
            result.resulting_revision()
        ),
        (f.base_id, 1, 1)
    );
    let row: (Option<Uuid>, Option<Uuid>, i64) = sqlx::query_as("SELECT d.current_version_id, v.base_document_version_id, v.version_no FROM documents d JOIN document_versions v ON v.document_id = d.document_id WHERE d.document_id = $1")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(row, (None, None, 1));
    assert_eq!(
        f.service().update_working(command, prepared).await.unwrap(),
        result
    );
    assert_eq!(
        f.repository
            .get_version_operation(operation_id(60))
            .await
            .unwrap()
            .unwrap()
            .result(),
        &result
    );
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM document_version_operations WHERE document_id = $1), (SELECT count(*) FROM outbox_events WHERE aggregate_id = $1), (SELECT count(*) FROM audit_outbox_events WHERE resource_id = $1)")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(counts, (1, 1, 1));
    assert_eq!(result.base_version_id(), None);
    // A fresh operation with the same initial semantics remains a real update.
    let next =
        UpdateWorkingVersionCommand::new(operation_id(64), f.document_id, f.base_id, 1, actor())
            .unwrap();
    let second = f
        .service()
        .update_working(next, f.prepare("Base", 1).await)
        .await
        .unwrap();
    assert_eq!(
        (
            second.version_no(),
            second.base_version_id(),
            second.resulting_revision()
        ),
        (1, None, 2)
    );
}

#[tokio::test]
async fn prior_publication_history_with_null_current_is_not_initial_working() {
    let f = fixture().await;
    sqlx::query("UPDATE documents SET current_version_id = NULL WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    // Even a legacy/malformed WORKING row cannot bypass the durable publication history.
    sqlx::query("UPDATE document_versions SET lifecycle_state = 'WORKING', published_at = NULL WHERE document_version_id = $1")
        .bind(f.base_id.as_uuid()).execute(&f.pool).await.unwrap();
    let command =
        UpdateWorkingVersionCommand::new(operation_id(61), f.document_id, f.base_id, 1, actor())
            .unwrap();
    assert_eq!(
        f.service()
            .update_working(command, f.prepare("Changed", 2).await)
            .await,
        Err(ApplicationError::BusinessRule)
    );
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(revision, 1);
}

#[tokio::test]
async fn initial_working_update_rejects_pending_schedule_without_mutation() {
    let f = initial_fixture().await;
    let schedule = document_application::SchedulePublishCommand::new(
        document_application::PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
        f.document_id,
        f.base_id,
        0,
        actor(),
        time::OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap(),
    )
    .unwrap();
    let accepted = f.service().schedule_publish(schedule).await.unwrap();
    let command = UpdateWorkingVersionCommand::new(
        operation_id(62),
        f.document_id,
        f.base_id,
        accepted.accepted_revision,
        actor(),
    )
    .unwrap();
    assert_eq!(
        f.service()
            .update_working(command, f.prepare("Changed", 2).await)
            .await,
        Err(ApplicationError::BusinessRule)
    );
    let state: (i64, Option<Uuid>, String) = sqlx::query_as("SELECT d.revision, d.current_version_id, v.title FROM documents d JOIN document_versions v ON v.document_id = d.document_id WHERE d.document_id = $1")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(state, (accepted.accepted_revision, None, "Base".into()));
}

#[tokio::test]
async fn initial_update_failed_inspection_binding_keeps_old_manifest_and_revision() {
    let f = initial_fixture().await;
    let prepared = f.prepare("Changed", 2).await;
    sqlx::query("DELETE FROM document_semantic_inspections WHERE file_id = $1")
        .bind(prepared.items()[0].file().file_id().as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let command =
        UpdateWorkingVersionCommand::new(operation_id(63), f.document_id, f.base_id, 0, actor())
            .unwrap();
    assert_eq!(
        f.service().update_working(command, prepared).await,
        Err(ApplicationError::IntegrityViolation)
    );
    let state: (i64, Option<Uuid>, String, String) = sqlx::query_as("SELECT d.revision, d.current_version_id, v.title, r.original_filename FROM documents d JOIN document_versions v ON v.document_id = d.document_id JOIN content_items i ON i.document_version_id = v.document_version_id JOIN content_representations r ON r.content_representation_id = i.authoritative_representation_id WHERE d.document_id = $1")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(state, (0, None, "Base".into(), "base.txt".into()));
    assert!(
        f.repository
            .get_version_operation(operation_id(63))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn new_operation_with_same_working_manifest_is_still_an_update_against_published_base() {
    let f = fixture().await;
    let target = DocumentVersionId::from_uuid(Uuid::now_v7());
    let prepared = f.prepare("Working", 2).await;
    let create =
        CreateVersionCommand::new(operation_id(65), f.document_id, target, 1, actor()).unwrap();
    f.service()
        .create_version(create, prepared.clone())
        .await
        .unwrap();
    let update =
        UpdateWorkingVersionCommand::new(operation_id(66), f.document_id, target, 2, actor())
            .unwrap();
    let result = f
        .service()
        .update_working(update.clone(), prepared.clone())
        .await
        .unwrap();
    assert_eq!(
        (
            result.version_no(),
            result.base_version_id(),
            result.resulting_revision()
        ),
        (2, Some(f.base_id), 3)
    );
    assert_eq!(
        f.service().update_working(update, prepared).await.unwrap(),
        result
    );
    let public: (Option<Uuid>, i64) =
        sqlx::query_as("SELECT current_version_id,revision FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(public, (Some(f.base_id.as_uuid()), 3));
    let no_change =
        UpdateWorkingVersionCommand::new(operation_id(67), f.document_id, target, 3, actor())
            .unwrap();
    assert_eq!(
        f.service()
            .update_working(no_change, f.prepare("Base", 1).await)
            .await,
        Err(ApplicationError::BusinessRule)
    );
}
