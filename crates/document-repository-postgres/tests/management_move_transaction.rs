#[path = "support/management.rs"]
mod support;

use document_application::{
    AccessPolicyService, ApplicationError, BootstrapRootPolicy, DocumentManagementService,
    FolderService, ManagementCommand, ManagementErrorCode, ManagementOperationId,
    ManagementRepository,
};
use document_domain::{
    Action, FolderId, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind, PolicyTarget,
    ResourceRef,
};
use support::{Fixture, context, fixture};
use uuid::Uuid;

fn operation_id() -> ManagementOperationId {
    ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap()
}

fn move_document(f: &Fixture, from: FolderId, to: FolderId, revision: i64) -> ManagementCommand {
    ManagementCommand::MoveDocument {
        operation_id: operation_id(),
        document_id: f.document_id,
        from_folder_id: from,
        to_folder_id: to,
        expected_document_revision: revision,
        reason: "move document".into(),
    }
}

fn move_folder(
    folder_id: FolderId,
    from: FolderId,
    to: FolderId,
    revision: i64,
) -> ManagementCommand {
    ManagementCommand::MoveFolder {
        operation_id: operation_id(),
        folder_id,
        from_parent_id: from,
        to_parent_id: to,
        expected_folder_revision: revision,
        reason: "move folder".into(),
    }
}

async fn allow(f: &Fixture, actions: impl IntoIterator<Item = Action>) {
    let grant = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        actions,
    )
    .unwrap();
    f.repository
        .initialize_root_policy(&context(), vec![grant])
        .await
        .unwrap();
}

async fn add_folder(f: &Fixture, parent: FolderId, name: &str) -> FolderId {
    let id = FolderId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$2,$3,'ACTIVE',0,now())")
        .bind(id.as_uuid()).bind(parent.as_uuid()).bind(name).execute(&f.pool).await.unwrap();
    id
}

async fn document_state(f: &Fixture) -> (Uuid, i64, Option<Uuid>) {
    sqlx::query_as(
        "SELECT folder_id,revision,current_version_id FROM documents WHERE document_id = $1",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap()
}

async fn folder_state(f: &Fixture, id: FolderId) -> (Option<Uuid>, i64) {
    sqlx::query_as("SELECT parent_folder_id,revision FROM folders WHERE folder_id = $1")
        .bind(id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

async fn event_count(f: &Fixture, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE event_type = $1")
        .bind(kind)
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

fn grants(actions: impl IntoIterator<Item = Action>) -> Vec<PolicyGrant> {
    vec![
        PolicyGrant::new(
            PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
            actions,
        )
        .unwrap(),
    ]
}

async fn explicit_folder_policy(
    f: &Fixture,
    folder_id: FolderId,
    actions: impl IntoIterator<Item = Action>,
) {
    AccessPolicyService::new(f.repository.clone())
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Folder(folder_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants(actions)),
                reason: "fixture policy".into(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn document_move_changes_parent_revision_and_access_epoch_without_touching_versions() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write, Action::Administer]).await;
    let to = add_folder(&f, f.root_id, "Destination").await;
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Initial','test-idp','policy-admin','{}',now())")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let service = DocumentManagementService::new(f.repository.clone());
    assert!(matches!(
        service
            .move_document(&context(), move_document(&f, to, f.root_id, 1))
            .await,
        Err(ApplicationError::Management(
            ManagementErrorCode::RevisionConflict
        ))
    ));
    let moved = service
        .move_document(&context(), move_document(&f, f.root_id, to, 1))
        .await
        .unwrap();
    assert!(moved.changed);
    assert_eq!(moved.resulting_revision, 2);
    assert_eq!(moved.access_revision, Some(2));
    assert_eq!(document_state(&f).await, (to.as_uuid(), 2, None));
    let version: (i64, String) = sqlx::query_as(
        "SELECT version_no,lifecycle_state FROM document_versions WHERE document_version_id = $1",
    )
    .bind(version_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(version, (1, "WORKING".into()));
    let noop = service
        .move_document(&context(), move_document(&f, to, to, 2))
        .await
        .unwrap();
    assert!(!noop.changed);
    assert_eq!(noop.access_revision, Some(2));
    assert_eq!(event_count(&f, "DocumentMoved").await, 1);
    assert!(matches!(
        service
            .move_document(&context(), move_document(&f, to, f.root_id, 1))
            .await,
        Err(ApplicationError::Management(
            ManagementErrorCode::RevisionConflict
        ))
    ));
}

#[tokio::test]
async fn pending_schedule_blocks_real_document_move_but_not_noop() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write, Action::Administer]).await;
    let to = add_folder(&f, f.root_id, "Destination").await;
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Pending','test-idp','policy-admin','{}',now())")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,1,2,now() + interval '1 day','test-idp','policy-admin',$4,'PENDING',now())")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(version_id).bind(vec![1_u8;32])
        .execute(&f.pool).await.unwrap();
    let service = DocumentManagementService::new(f.repository.clone());
    assert!(
        !service
            .move_document(&context(), move_document(&f, f.root_id, f.root_id, 1))
            .await
            .unwrap()
            .changed
    );
    assert!(matches!(
        service
            .move_document(&context(), move_document(&f, f.root_id, to, 1))
            .await,
        Err(ApplicationError::Management(
            ManagementErrorCode::ReservedDocument
        ))
    ));
    assert_eq!(document_state(&f).await.0, f.root_id.as_uuid());
    assert_eq!(event_count(&f, "DocumentMoved").await, 0);
}

#[tokio::test]
async fn ended_document_requires_history_permission_and_move_keeps_current_null() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write, Action::Administer]).await;
    let to = add_folder(&f, f.root_id, "Archive").await;
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Former',now(),'test-idp','policy-admin','{}',now())")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,0,$4,'test-idp','policy-admin','end',$4,1,now())")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(vec![1_u8;32]).bind(version_id)
        .execute(&f.pool).await.unwrap();
    let service = DocumentManagementService::new(f.repository.clone());
    let command = move_document(&f, f.root_id, to, 1);
    assert!(matches!(
        service.move_document(&context(), command.clone()).await,
        Err(ApplicationError::Forbidden)
    ));
    let policy_id: Uuid =
        sqlx::query_scalar("SELECT policy_id FROM access_policy_bindings WHERE folder_id = $1")
            .bind(f.root_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO access_policy_grants (policy_id,subject_kind,identity_provider,subject_id,action) VALUES ($1,'principal','test-idp','policy-admin','read_history')")
        .bind(policy_id).execute(&f.pool).await.unwrap();
    assert!(
        service
            .move_document(&context(), command)
            .await
            .unwrap()
            .changed
    );
    assert_eq!(document_state(&f).await.2, None);
}

#[tokio::test]
async fn concurrent_reciprocal_folder_moves_cannot_form_a_cycle() {
    let f = fixture().await;
    allow(&f, [Action::Administer]).await;
    let a = add_folder(&f, f.root_id, "A").await;
    let b = add_folder(&f, f.root_id, "B").await;
    let service = FolderService::new(f.repository.clone());
    let ctx = context();
    let (left, right) = tokio::join!(
        service.move_folder(&ctx, move_folder(a, f.root_id, b, 0)),
        service.move_folder(&ctx, move_folder(b, f.root_id, a, 0)),
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let a_parent = folder_state(&f, a).await.0;
    let b_parent = folder_state(&f, b).await.0;
    assert!(!(a_parent == Some(b.as_uuid()) && b_parent == Some(a.as_uuid())));
    assert_eq!(event_count(&f, "FolderMoved").await, 1);
}

#[tokio::test]
async fn folder_move_rejects_pending_subtree_and_preserves_document_revision() {
    let f = fixture().await;
    allow(&f, [Action::Administer]).await;
    let a = add_folder(&f, f.root_id, "A").await;
    let b = add_folder(&f, f.root_id, "B").await;
    sqlx::query("UPDATE documents SET folder_id = $1 WHERE document_id = $2")
        .bind(a.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Pending','test-idp','policy-admin','{}',now())")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let publish_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,1,2,now() + interval '1 day','test-idp','policy-admin',$4,'PENDING',now())")
        .bind(publish_id).bind(f.document_id.as_uuid()).bind(version_id).bind(vec![1_u8;32])
        .execute(&f.pool).await.unwrap();
    let service = FolderService::new(f.repository.clone());
    assert!(matches!(
        service
            .move_folder(&context(), move_folder(a, f.root_id, b, 0))
            .await,
        Err(ApplicationError::Management(
            ManagementErrorCode::ReservedDocument
        ))
    ));
    sqlx::query("UPDATE document_publish_schedules SET status='CANCELLED',cancelled_at=now() WHERE publish_operation_id=$1")
        .bind(publish_id).execute(&f.pool).await.unwrap();
    assert!(
        service
            .move_folder(&context(), move_folder(a, f.root_id, b, 0))
            .await
            .unwrap()
            .changed
    );
    assert_eq!(document_state(&f).await.1, 1);
    assert_eq!(folder_state(&f, a).await, (Some(b.as_uuid()), 1));
}

#[tokio::test]
async fn explicit_child_policy_stays_in_place_and_is_excluded_from_impact() {
    let f = fixture().await;
    allow(&f, [Action::Administer, Action::Read]).await;
    let a = add_folder(&f, f.root_id, "A").await;
    let b = add_folder(&f, f.root_id, "B").await;
    let child = add_folder(&f, a, "Explicit Child").await;
    sqlx::query("UPDATE documents SET folder_id = $1 WHERE document_id = $2")
        .bind(child.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    explicit_folder_policy(&f, b, [Action::Administer]).await;
    explicit_folder_policy(&f, child, [Action::Read]).await;
    assert!(
        !f.repository
            .authorize_resource(
                &context(),
                ResourceRef::Folder(child),
                &[Action::Administer]
            )
            .await
            .unwrap()
    );
    let moved = FolderService::new(f.repository.clone())
        .move_folder(&context(), move_folder(a, f.root_id, b, 0))
        .await
        .unwrap();
    assert_eq!(moved.movement.unwrap().subtree_affected, Some(1));
    assert!(
        !f.repository
            .authorize_resource(
                &context(),
                ResourceRef::Folder(child),
                &[Action::Administer]
            )
            .await
            .unwrap()
    );
    assert_eq!(document_state(&f).await.1, 1);
}

#[tokio::test]
async fn explicit_document_policy_survives_move_to_a_different_inherited_policy() {
    let f = fixture().await;
    allow(&f, [Action::Administer, Action::Read, Action::Write]).await;
    let to = add_folder(&f, f.root_id, "Restricted").await;
    explicit_folder_policy(&f, to, [Action::Administer]).await;
    AccessPolicyService::new(f.repository.clone())
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants([
                    Action::Administer,
                    Action::Read,
                    Action::Write,
                ])),
                reason: "document override".into(),
            },
        )
        .await
        .unwrap();
    let result = DocumentManagementService::new(f.repository.clone())
        .move_document(&context(), move_document(&f, f.root_id, to, 1))
        .await
        .unwrap();
    assert!(result.changed);
    assert!(
        f.repository
            .authorize_resource(
                &context(),
                ResourceRef::Document(f.document_id),
                &[Action::Write]
            )
            .await
            .unwrap()
    );
    let binding: i64 =
        sqlx::query_scalar("SELECT count(*) FROM access_policy_bindings WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(binding, 1);
}

#[tokio::test]
async fn move_replay_and_lookup_are_withheld_after_inherited_permission_loss() {
    let f = fixture().await;
    allow(&f, [Action::Administer, Action::Read, Action::Write]).await;
    let to = add_folder(&f, f.root_id, "Restricted").await;
    explicit_folder_policy(&f, to, [Action::Administer]).await;
    let service = DocumentManagementService::new(f.repository.clone());
    let command = move_document(&f, f.root_id, to, 1);
    assert!(
        service
            .move_document(&context(), command.clone())
            .await
            .unwrap()
            .changed
    );
    assert!(matches!(
        service.move_document(&context(), command.clone()).await,
        Err(ApplicationError::Forbidden)
    ));
    assert!(matches!(
        f.repository
            .lookup(&context(), command.operation_id())
            .await,
        Err(document_application::RepositoryError::Forbidden)
    ));
}

#[tokio::test]
async fn folder_move_audit_failure_rolls_back_structure_and_access_revision() {
    let f = fixture().await;
    allow(&f, [Action::Administer]).await;
    let a = add_folder(&f, f.root_id, "A").await;
    let b = add_folder(&f, f.root_id, "B").await;
    sqlx::query("CREATE FUNCTION reject_move_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type = 'folder.moved' THEN RAISE EXCEPTION 'audit blocked'; END IF; RETURN NEW; END; $$")
        .execute(&f.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_move_audit BEFORE INSERT ON audit_outbox_events FOR EACH ROW EXECUTE FUNCTION reject_move_audit()")
        .execute(&f.pool).await.unwrap();
    assert!(
        FolderService::new(f.repository.clone())
            .move_folder(&context(), move_folder(a, f.root_id, b, 0))
            .await
            .is_err()
    );
    assert_eq!(folder_state(&f, a).await, (Some(f.root_id.as_uuid()), 0));
    let access_revision: i64 =
        sqlx::query_scalar("SELECT access_revision FROM document_access_state WHERE id = 1")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(access_revision, 1);
    assert_eq!(event_count(&f, "FolderMoved").await, 0);
}
