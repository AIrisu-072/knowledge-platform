#[path = "support/management.rs"]
mod support;

use document_application::{
    ApplicationError, BootstrapRootPolicy, CurrentReadProjection, CurrentReadStateService,
    DocumentQueryRepository, MAX_READ_STATE_REVISION, MarkVersionRead, PublishedQuery,
    ReadStateMutation, ReadStateMutationKind, ReadStateMutationResult, ReadStateOperationId,
    ReadStateService,
};
use document_domain::{
    Action, DocumentId, DocumentVersionId, PolicyGrant, PolicySubject, PolicySubjectKind,
};
use support::{Fixture, context, fixture};
use uuid::Uuid;

async fn published(f: &Fixture, document_id: DocumentId, version_no: i64) -> DocumentVersionId {
    let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO document_versions(document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES($1,$2,$3,'PUBLISHED','Synthetic',now(),'test-idp','policy-admin','{}',now())")
        .bind(version_id.as_uuid()).bind(document_id.as_uuid()).bind(version_no)
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id=$1 WHERE document_id=$2")
        .bind(version_id.as_uuid())
        .bind(document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    version_id
}

async fn setup() -> (Fixture, DocumentVersionId) {
    let f = fixture().await;
    let grant = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        [Action::Read, Action::ReadHistory],
    )
    .unwrap();
    f.repository
        .initialize_root_policy(&context(), vec![grant])
        .await
        .unwrap();
    let version_id = published(&f, f.document_id, 1).await;
    (f, version_id)
}

fn command(
    f: &Fixture,
    version_id: DocumentVersionId,
    kind: ReadStateMutationKind,
    revision: i64,
) -> ReadStateMutation {
    ReadStateMutation {
        operation_id: ReadStateOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
        document_id: f.document_id,
        document_version_id: version_id,
        expected_read_state_revision: revision,
        kind,
    }
}

async fn send(
    f: &Fixture,
    command: ReadStateMutation,
) -> Result<ReadStateMutationResult, ApplicationError> {
    CurrentReadStateService::new(f.repository.clone())
        .mutate_read_state(&context(), command)
        .await
}

async fn state(f: &Fixture, version_id: DocumentVersionId) -> CurrentReadProjection {
    CurrentReadStateService::new(f.repository.clone())
        .get_current_read_state(&context(), f.document_id, version_id)
        .await
        .unwrap()
        .state
}

async fn counts(f: &Fixture) -> (i64, i64, i64) {
    let states = sqlx::query_scalar("SELECT count(*) FROM document_read_states")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let receipts = sqlx::query_scalar("SELECT count(*) FROM document_read_state_operations")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let audit = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type IN ('document.version.detail_viewed','document.version.marked_unread','document.version.read_confirmed')").fetch_one(&f.pool).await.unwrap();
    (states, receipts, audit)
}

#[tokio::test]
async fn legacy_put_after_reset_preserves_unread() {
    let (f, version_id) = setup().await;
    let legacy = ReadStateService::new(f.repository.clone());
    let command = MarkVersionRead {
        document_id: f.document_id,
        document_version_id: version_id,
    };
    let first = legacy.mark_version_read(&context(), command).await.unwrap();
    let reset = send(
        &f,
        self::command(&f, version_id, ReadStateMutationKind::Reset, 1),
    )
    .await
    .unwrap();
    let replay = legacy.mark_version_read(&context(), command).await.unwrap();
    assert!(!replay.inserted);
    assert_eq!(first.first_read_at, replay.first_read_at);
    assert_eq!(state(&f, version_id).await, reset.resulting_read_state);
    assert_eq!(counts(&f).await, (1, 1, 2));
}

#[tokio::test]
async fn legacy_put_racing_initial_view_has_one_first_record() {
    let (f, version_id) = setup().await;
    let legacy = ReadStateService::new(f.repository.clone());
    let ctx = context();
    let (old, new) = tokio::join!(
        legacy.mark_version_read(
            &ctx,
            MarkVersionRead {
                document_id: f.document_id,
                document_version_id: version_id
            }
        ),
        send(&f, command(&f, version_id, ReadStateMutationKind::View, 0)),
    );
    let old = old.unwrap();
    match new {
        Ok(new) => {
            assert!(!old.inserted);
            assert_eq!(
                Some(old.first_read_at),
                new.resulting_read_state.first_read_at
            );
            assert_eq!(counts(&f).await, (1, 1, 1));
        }
        Err(ApplicationError::ReadStateRevisionConflict) => {
            assert!(old.inserted);
            assert_eq!(counts(&f).await, (1, 0, 1));
        }
        other => panic!("unexpected race result: {other:?}"),
    }
    assert_eq!(state(&f, version_id).await.read_state_revision, 1);
}

#[tokio::test]
async fn two_initial_views_one_transition() {
    let (f, version_id) = setup().await;
    let (first, second) = tokio::join!(
        send(&f, command(&f, version_id, ReadStateMutationKind::View, 0)),
        send(&f, command(&f, version_id, ReadStateMutationKind::View, 0)),
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert!(matches!(
        first.err().or(second.err()),
        Some(ApplicationError::ReadStateRevisionConflict)
    ));
    assert_eq!(counts(&f).await, (1, 1, 1));
}

#[tokio::test]
async fn same_operation_replays_once() {
    let (f, version_id) = setup().await;
    let command = command(&f, version_id, ReadStateMutationKind::View, 0);
    let (first, second) = tokio::join!(send(&f, command), send(&f, command));
    let first = first.unwrap();
    assert_eq!(first, second.unwrap());
    assert_eq!(send(&f, command).await.unwrap(), first);
    assert_eq!(
        send(
            &f,
            ReadStateMutation {
                kind: ReadStateMutationKind::Reset,
                ..command
            }
        )
        .await,
        Err(ApplicationError::OperationConflict)
    );
    assert_eq!(
        send(
            &f,
            ReadStateMutation {
                expected_read_state_revision: 1,
                ..command
            }
        )
        .await,
        Err(ApplicationError::OperationConflict)
    );
    assert_eq!(counts(&f).await, (1, 1, 1));
}

#[tokio::test]
async fn cross_document_operation_collision_rolls_back_state_and_audit() {
    let (f, version_id) = setup().await;
    let other_document = DocumentId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO documents(document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES($1,$2,NULL,1,'{}',now())")
        .bind(other_document.as_uuid()).bind(f.root_id.as_uuid()).execute(&f.pool).await.unwrap();
    let other_version = published(&f, other_document, 1).await;
    let first = command(&f, version_id, ReadStateMutationKind::View, 0);
    let second = ReadStateMutation {
        document_id: other_document,
        document_version_id: other_version,
        ..first
    };
    let (first, second) = tokio::join!(send(&f, first), send(&f, second));
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert!(matches!(
        first.err().or(second.err()),
        Some(ApplicationError::OperationConflict)
    ));
    assert_eq!(counts(&f).await, (1, 1, 1));
}

#[tokio::test]
async fn old_view_replay_does_not_clear_reset() {
    let (f, version_id) = setup().await;
    let command = command(&f, version_id, ReadStateMutationKind::View, 0);
    let first = send(&f, command).await.unwrap();
    let reset = send(
        &f,
        self::command(&f, version_id, ReadStateMutationKind::Reset, 1),
    )
    .await
    .unwrap();
    assert_eq!(send(&f, command).await.unwrap(), first);
    assert_eq!(state(&f, version_id).await, reset.resulting_read_state);
    let audit: Vec<(String, serde_json::Value)> = sqlx::query_as("SELECT event_type,data FROM audit_outbox_events WHERE event_type IN ('document.version.detail_viewed','document.version.marked_unread')").fetch_all(&f.pool).await.unwrap();
    let viewed = &audit.iter().find(|(event, _)| event == "document.version.detail_viewed").unwrap().1;
    assert_eq!(viewed, &serde_json::json!({"document_version_id": version_id.as_uuid(), "operation_id": command.operation_id.as_uuid(), "expected_read_state_revision": 0, "resulting_read_state_revision": 1, "first_record": true, "trigger": "detail_display"}));
    let reset_audit = &audit.iter().find(|(event, _)| event == "document.version.marked_unread").unwrap().1;
    assert_eq!(reset_audit, &serde_json::json!({"document_version_id": version_id.as_uuid(), "operation_id": reset.operation_id.as_uuid(), "expected_read_state_revision": 1, "resulting_read_state_revision": 2, "trigger": "user_reset"}));
    assert_eq!(counts(&f).await, (1, 2, 2));

    let returning_command = self::command(&f, version_id, ReadStateMutationKind::View, 2);
    let returning_view = send(&f, returning_command).await.unwrap();
    assert!(returning_view.changed);
    assert_eq!(returning_view.resulting_read_state.read_state_revision, 3);
    assert!(returning_view.resulting_read_state.is_read());
    assert!(!returning_view.resulting_read_state.needs_recheck);
    assert_eq!(returning_view.resulting_read_state.first_read_at, first.resulting_read_state.first_read_at);
    let returning_audit: serde_json::Value = sqlx::query_scalar(
        "SELECT data FROM audit_outbox_events WHERE event_type='document.version.detail_viewed' AND data->>'operation_id'=$1",
    )
    .bind(returning_command.operation_id.as_uuid().to_string())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(returning_audit, serde_json::json!({
        "document_version_id": version_id.as_uuid(),
        "operation_id": returning_command.operation_id.as_uuid(),
        "expected_read_state_revision": 2,
        "resulting_read_state_revision": 3,
        "first_record": false,
        "trigger": "detail_display"
    }));
    let unread_query = PublishedQuery { unread_only: true, ..PublishedQuery::default() };
    assert!(f.repository.list_published_documents(&context(), unread_query.clone()).await.unwrap().items.is_empty());
    let later_reset = send(&f, self::command(&f, version_id, ReadStateMutationKind::Reset, 3)).await.unwrap();
    assert_eq!(later_reset.resulting_read_state.read_state_revision, 4);
    assert_eq!(send(&f, returning_command).await.unwrap(), returning_view);
    assert_eq!(state(&f, version_id).await, later_reset.resulting_read_state);
    assert_eq!(f.repository.list_published_documents(&context(), unread_query).await.unwrap().items.len(), 1);
    assert_eq!(counts(&f).await, (1, 4, 4));
}

#[tokio::test]
async fn stale_other_tab_view_conflicts() {
    let (f, version_id) = setup().await;
    send(&f, command(&f, version_id, ReadStateMutationKind::View, 0))
        .await
        .unwrap();
    let reset = send(&f, command(&f, version_id, ReadStateMutationKind::Reset, 1))
        .await
        .unwrap();
    assert_eq!(
        send(&f, command(&f, version_id, ReadStateMutationKind::View, 1)).await,
        Err(ApplicationError::ReadStateRevisionConflict)
    );
    assert_eq!(state(&f, version_id).await, reset.resulting_read_state);
    assert_eq!(counts(&f).await, (1, 2, 2));
}

#[tokio::test]
async fn reset_of_unread_rejected() {
    let (f, version_id) = setup().await;
    assert_eq!(
        send(&f, command(&f, version_id, ReadStateMutationKind::Reset, 0)).await,
        Err(ApplicationError::BusinessRule)
    );
    assert_eq!(counts(&f).await, (0, 0, 0));
    send(&f, command(&f, version_id, ReadStateMutationKind::View, 0))
        .await
        .unwrap();
    send(&f, command(&f, version_id, ReadStateMutationKind::Reset, 1))
        .await
        .unwrap();
    assert_eq!(
        send(&f, command(&f, version_id, ReadStateMutationKind::Reset, 2)).await,
        Err(ApplicationError::BusinessRule)
    );
    assert_eq!(counts(&f).await, (1, 2, 2));
}

#[tokio::test]
async fn audit_failure_rolls_back_all() {
    let (f, version_id) = setup().await;
    sqlx::query("ALTER TABLE audit_outbox_events ADD CONSTRAINT synthetic_read_audit_failure CHECK(event_type <> 'document.version.detail_viewed')").execute(&f.pool).await.unwrap();
    assert!(
        send(&f, command(&f, version_id, ReadStateMutationKind::View, 0))
            .await
            .is_err()
    );
    assert_eq!(counts(&f).await, (0, 0, 0));
    sqlx::query("ALTER TABLE audit_outbox_events DROP CONSTRAINT synthetic_read_audit_failure")
        .execute(&f.pool)
        .await
        .unwrap();
    let first = send(&f, command(&f, version_id, ReadStateMutationKind::View, 0))
        .await
        .unwrap();
    sqlx::query("ALTER TABLE audit_outbox_events ADD CONSTRAINT synthetic_reset_audit_failure CHECK(event_type <> 'document.version.marked_unread')").execute(&f.pool).await.unwrap();
    assert!(
        send(&f, command(&f, version_id, ReadStateMutationKind::Reset, 1))
            .await
            .is_err()
    );
    assert_eq!(state(&f, version_id).await, first.resulting_read_state);
    assert_eq!(counts(&f).await, (1, 1, 1));
}

#[tokio::test]
async fn receipt_failure_rolls_back_all() {
    let (f, version_id) = setup().await;
    sqlx::query("ALTER TABLE document_read_state_operations ADD CONSTRAINT synthetic_read_receipt_failure CHECK(operation_kind <> 'VIEW')").execute(&f.pool).await.unwrap();
    assert!(
        send(&f, command(&f, version_id, ReadStateMutationKind::View, 0))
            .await
            .is_err()
    );
    assert_eq!(counts(&f).await, (0, 0, 0));
    sqlx::query(
        "ALTER TABLE document_read_state_operations DROP CONSTRAINT synthetic_read_receipt_failure",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    let first = send(&f, command(&f, version_id, ReadStateMutationKind::View, 0))
        .await
        .unwrap();
    sqlx::query("ALTER TABLE document_read_state_operations ADD CONSTRAINT synthetic_reset_receipt_failure CHECK(operation_kind <> 'RESET')").execute(&f.pool).await.unwrap();
    assert!(
        send(&f, command(&f, version_id, ReadStateMutationKind::Reset, 1))
            .await
            .is_err()
    );
    assert_eq!(state(&f, version_id).await, first.resulting_read_state);
    assert_eq!(counts(&f).await, (1, 1, 1));
}

#[tokio::test]
async fn old_receipt_requires_current_read_history() {
    let (f, version_id) = setup().await;
    let command = command(&f, version_id, ReadStateMutationKind::View, 0);
    let first = send(&f, command).await.unwrap();
    published(&f, f.document_id, 2).await;
    sqlx::query("DELETE FROM access_policy_grants WHERE action='read_history'")
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(send(&f, command).await, Err(ApplicationError::Forbidden));
    assert_eq!(
        send(
            &f,
            self::command(&f, version_id, ReadStateMutationKind::View, 1)
        )
        .await,
        Err(ApplicationError::Forbidden)
    );
    let denied = CurrentReadStateService::new(f.repository.clone())
        .get_current_read_state(&context(), f.document_id, version_id)
        .await;
    assert_eq!(denied, Err(ApplicationError::Forbidden));
    sqlx::query("INSERT INTO access_policy_grants(policy_id,subject_kind,identity_provider,subject_id,action) SELECT policy_id,'principal','test-idp','policy-admin','read_history' FROM access_policy_bindings WHERE folder_id=$1")
        .bind(f.root_id.as_uuid()).execute(&f.pool).await.unwrap();
    assert_eq!(send(&f, command).await.unwrap(), first);
    assert_eq!(
        send(
            &f,
            self::command(&f, version_id, ReadStateMutationKind::View, 1)
        )
        .await,
        Err(ApplicationError::StaleVersion)
    );
    sqlx::query("DELETE FROM access_policy_grants")
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        send(&f, command).await,
        Err(ApplicationError::DocumentNotFound)
    );
}

#[tokio::test]
async fn new_version_is_unread() {
    let (f, version_id) = setup().await;
    let before: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id=$1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    send(&f, command(&f, version_id, ReadStateMutationKind::View, 0))
        .await
        .unwrap();
    let next = published(&f, f.document_id, 2).await;
    assert_eq!(state(&f, next).await, CurrentReadProjection::default());
    let after: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id=$1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn unread_filter_matches_reset_projection() {
    let (f, version_id) = setup().await;
    let first = send(&f, command(&f, version_id, ReadStateMutationKind::View, 0))
        .await
        .unwrap();
    let query = PublishedQuery {
        unread_only: true,
        ..PublishedQuery::default()
    };
    assert!(
        f.repository
            .list_published_documents(&context(), query.clone())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    send(&f, command(&f, version_id, ReadStateMutationKind::Reset, 1))
        .await
        .unwrap();
    let page = f
        .repository
        .list_published_documents(&context(), query)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].gui.read_state.first_read_at,
        first.resulting_read_state.first_read_at
    );
    assert!(!page.items[0].gui.read_state.is_read());
}

#[tokio::test]
async fn maximum_revision_rejects_transition_but_allows_noop_and_replay() {
    let (f, version_id) = setup().await;
    let command = command(&f, version_id, ReadStateMutationKind::View, 0);
    let first = send(&f, command).await.unwrap();
    sqlx::query(
        "UPDATE document_read_states SET read_state_revision=$1 WHERE document_version_id=$2",
    )
    .bind(MAX_READ_STATE_REVISION)
    .bind(version_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    let before = state(&f, version_id).await;
    assert_eq!(
        send(
            &f,
            self::command(
                &f,
                version_id,
                ReadStateMutationKind::Reset,
                MAX_READ_STATE_REVISION
            )
        )
        .await,
        Err(ApplicationError::BusinessRule)
    );
    assert_eq!(state(&f, version_id).await, before);
    assert_eq!(counts(&f).await, (1, 1, 1));
    let noop = self::command(
        &f,
        version_id,
        ReadStateMutationKind::View,
        MAX_READ_STATE_REVISION,
    );
    let result = send(&f, noop).await.unwrap();
    assert!(!result.changed);
    assert_eq!(result.resulting_read_state, before);
    assert_eq!(send(&f, noop).await.unwrap(), result);
    assert_eq!(send(&f, command).await.unwrap(), first);
    assert_eq!(counts(&f).await, (1, 2, 1));
    sqlx::query("UPDATE document_read_states SET needs_recheck=true WHERE document_version_id=$1")
        .bind(version_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let unread = state(&f, version_id).await;
    assert_eq!(
        send(
            &f,
            self::command(
                &f,
                version_id,
                ReadStateMutationKind::View,
                MAX_READ_STATE_REVISION
            )
        )
        .await,
        Err(ApplicationError::BusinessRule)
    );
    assert_eq!(state(&f, version_id).await, unread);
    assert_eq!(counts(&f).await, (1, 2, 1));
    assert_eq!(send(&f, command).await.unwrap(), first);
    assert_eq!(state(&f, version_id).await, unread);
}

#[tokio::test]
async fn ended_receipt_replay_rechecks_history_before_stale_rejection() {
    let (f, version_id) = setup().await;
    let command = command(&f, version_id, ReadStateMutationKind::View, 0);
    let first = send(&f, command).await.unwrap();
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id=$1")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO document_publication_end_operations(operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES($1,$2,$3,$4,$5,'test-idp','policy-admin','synthetic end',$5,$6,now())")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(vec![0_u8; 32]).bind(revision).bind(version_id.as_uuid()).bind(revision + 1).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id=NULL,revision=revision+1 WHERE document_id=$1")
        .bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    assert_eq!(send(&f, command).await.unwrap(), first);
    assert_eq!(send(&f, self::command(&f, version_id, ReadStateMutationKind::Reset, 1)).await, Err(ApplicationError::StaleVersion));
    let service = CurrentReadStateService::new(f.repository.clone());
    assert_eq!(service.get_current_read_state(&context(), f.document_id, version_id).await, Err(ApplicationError::StaleVersion));
    sqlx::query("DELETE FROM access_policy_grants WHERE action='read_history'").execute(&f.pool).await.unwrap();
    assert_eq!(send(&f, command).await, Err(ApplicationError::Forbidden));
    assert_eq!(service.get_current_read_state(&context(), f.document_id, version_id).await, Err(ApplicationError::Forbidden));
    assert_eq!(counts(&f).await, (1, 1, 1));
}
