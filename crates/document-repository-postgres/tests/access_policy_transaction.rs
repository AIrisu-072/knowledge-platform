#[path = "support/management.rs"]
mod support;

use document_application::{
    AccessPolicyService, ApplicationError, BootstrapRootPolicy, ManagementOperationId,
    ManagementRepository,
};
use document_domain::{
    Action, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind, PolicyTarget, ResourceRef,
    evaluate_policy,
};
use support::{context, fixture};
use uuid::Uuid;

fn operation_id() -> ManagementOperationId {
    ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap()
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

#[tokio::test]
async fn root_uninitialized_denies_and_bootstrap_is_one_time() {
    let f = fixture().await;
    let service = AccessPolicyService::new(f.repository.clone());
    let command = document_application::ManagementCommand::SetAccessPolicy {
        operation_id: operation_id(),
        target: PolicyTarget::Document(f.document_id),
        expected_policy_revision: 0,
        mode: PolicyMode::Explicit(grants([Action::Read])),
        reason: "test policy".into(),
    };
    assert!(matches!(
        service.set_access_policy(&context(), command.clone()).await,
        Err(ApplicationError::Forbidden)
    ));
    f.repository
        .initialize_root_policy(&context(), grants([Action::Administer, Action::Read]))
        .await
        .unwrap();
    assert!(
        f.repository
            .initialize_root_policy(&context(), grants([Action::Administer]))
            .await
            .is_err()
    );
    assert!(service.set_access_policy(&context(), command).await.is_ok());
}

#[tokio::test]
async fn nearest_policy_matches_pure_evaluator_and_t8_does_not_change_document_revision() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), grants([Action::Administer, Action::Read]))
        .await
        .unwrap();
    let child_id = Uuid::now_v7();
    sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$2,'Child','ACTIVE',0,now())")
        .bind(child_id).bind(f.root_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET folder_id = $1 WHERE document_id = $2")
        .bind(child_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let service = AccessPolicyService::new(f.repository.clone());
    let before: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let result = service
        .set_access_policy(
            &context(),
            document_application::ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Folder(document_domain::FolderId::from_uuid(child_id)),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants([Action::Write])),
                reason: "child override".into(),
            },
        )
        .await
        .unwrap();
    assert!(result.changed);
    assert_eq!(result.access_revision, Some(2));
    let after: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(after, before);
    let effective = PolicyMode::Explicit(grants([Action::Write]));
    let verified = context();
    assert!(!evaluate_policy(
        verified.subjects(),
        &effective,
        &[Action::Read]
    ));
    assert!(evaluate_policy(
        verified.subjects(),
        &effective,
        &[Action::Write]
    ));
    assert!(
        !f.repository
            .authorize_resource(
                &context(),
                ResourceRef::Document(f.document_id),
                &[Action::Read]
            )
            .await
            .unwrap()
    );
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
}

#[tokio::test]
async fn same_id_replay_conflict_noop_and_audit_failure_rollback() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), grants([Action::Administer, Action::Read]))
        .await
        .unwrap();
    let service = AccessPolicyService::new(f.repository.clone());
    let command = document_application::ManagementCommand::SetAccessPolicy {
        operation_id: operation_id(),
        target: PolicyTarget::Document(f.document_id),
        expected_policy_revision: 0,
        mode: PolicyMode::Explicit(grants([Action::Administer, Action::Read])),
        reason: "document policy".into(),
    };
    let first = service
        .set_access_policy(&context(), command.clone())
        .await
        .unwrap();
    let replay = service
        .set_access_policy(&context(), command.clone())
        .await
        .unwrap();
    assert_eq!(first, replay);
    let operation_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_management_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(operation_count, 1);
    let mut changed = command.clone();
    if let document_application::ManagementCommand::SetAccessPolicy { reason, .. } = &mut changed {
        *reason = "different".into();
    }
    assert!(matches!(
        service.set_access_policy(&context(), changed).await,
        Err(ApplicationError::Conflict)
    ));
    let noop = service
        .set_access_policy(
            &context(),
            document_application::ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 1,
                mode: PolicyMode::Explicit(grants([Action::Administer, Action::Read])),
                reason: "same policy".into(),
            },
        )
        .await
        .unwrap();
    assert!(!noop.changed);
    let domain_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE event_type = 'AccessPolicyChanged'",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(domain_count, 2);
    let audit_before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    sqlx::query("CREATE FUNCTION reject_policy_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type = 'access_policy.changed' THEN RAISE EXCEPTION 'audit blocked'; END IF; RETURN NEW; END; $$")
        .execute(&f.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_policy_audit BEFORE INSERT ON audit_outbox_events FOR EACH ROW EXECUTE FUNCTION reject_policy_audit()")
        .execute(&f.pool).await.unwrap();
    let before: (i64, i64) = sqlx::query_as("SELECT revision, access_revision FROM access_policy_bindings CROSS JOIN document_access_state WHERE document_id = $1")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    let result = service
        .set_access_policy(
            &context(),
            document_application::ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 1,
                mode: PolicyMode::Explicit(grants([Action::Administer])),
                reason: "drop read".into(),
            },
        )
        .await;
    assert!(result.is_err());
    let after: (i64, i64) = sqlx::query_as("SELECT revision, access_revision FROM access_policy_bindings CROSS JOIN document_access_state WHERE document_id = $1")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(after, before);
    let audit_after: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(audit_after, audit_before);
    let operation_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_management_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(operation_after, 2);
}

#[tokio::test]
async fn replay_and_lookup_require_current_authorization() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), grants([Action::Administer, Action::Read]))
        .await
        .unwrap();
    let service = AccessPolicyService::new(f.repository.clone());
    let command = document_application::ManagementCommand::SetAccessPolicy {
        operation_id: operation_id(),
        target: PolicyTarget::Document(f.document_id),
        expected_policy_revision: 0,
        mode: PolicyMode::Explicit(grants([Action::Read])),
        reason: "restrict management".into(),
    };
    service
        .set_access_policy(&context(), command.clone())
        .await
        .unwrap();
    assert!(matches!(
        service.set_access_policy(&context(), command.clone()).await,
        Err(ApplicationError::Forbidden)
    ));
    assert!(matches!(
        f.repository
            .lookup(&context(), command.operation_id())
            .await,
        Err(document_application::RepositoryError::Forbidden)
    ));
    let forged_target = document_application::ManagementCommand::SetAccessPolicy {
        operation_id: command.operation_id(),
        target: PolicyTarget::Folder(f.root_id),
        expected_policy_revision: 1,
        mode: PolicyMode::Explicit(grants([Action::Administer])),
        reason: "probe another target".into(),
    };
    assert!(matches!(
        service.set_access_policy(&context(), forged_target).await,
        Err(ApplicationError::Forbidden)
    ));
}

#[tokio::test]
async fn pending_schedule_does_not_block_policy_revocation() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), grants([Action::Administer, Action::Read]))
        .await
        .unwrap();
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Pending','test-idp','policy-admin','{}',now())")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let publish_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,1,2,now() + interval '1 day','test-idp','policy-admin',$4,'PENDING',now())")
        .bind(publish_id).bind(f.document_id.as_uuid()).bind(version_id).bind(vec![1_u8;32])
        .execute(&f.pool).await.unwrap();
    let service = AccessPolicyService::new(f.repository.clone());
    let result = service
        .set_access_policy(
            &context(),
            document_application::ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants([Action::Read])),
                reason: "revoke administer while pending".into(),
            },
        )
        .await
        .unwrap();
    assert!(result.changed);
    let schedule: (String, i64) = sqlx::query_as("SELECT status, expected_document_revision FROM document_publish_schedules WHERE publish_operation_id = $1")
        .bind(publish_id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(schedule, ("PENDING".into(), 1));
    let document_revision: i64 =
        sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(document_revision, 1);
}

#[tokio::test]
async fn legacy_document_audit_default_and_typed_policy_audit_round_trip() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), grants([Action::Administer]))
        .await
        .unwrap();
    let typed: (String, Uuid) = sqlx::query_as(
        "SELECT resource_type, resource_id FROM audit_outbox_events WHERE event_type = 'access_policy.changed' LIMIT 1",
    )
    .fetch_one(&f.pool).await.unwrap();
    assert_eq!(typed.0, "Folder");
    assert_eq!(typed.1, f.root_id.as_uuid());
    let legacy_id = Uuid::now_v7();
    sqlx::query("INSERT INTO audit_outbox_events (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id,resource_id,result,data,occurred_at) VALUES ($1,'document.created','test','document/test','test-idp','policy-admin',$2,'success','{}',now())")
        .bind(legacy_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let legacy: String =
        sqlx::query_scalar("SELECT resource_type FROM audit_outbox_events WHERE event_id = $1")
            .bind(legacy_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(legacy, "Document");
}
