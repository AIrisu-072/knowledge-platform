#[path = "support/management.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use document_application::{
    AccessPolicyService, ApplicationError, BootstrapRootPolicy, DocumentManagementService,
    ManagementCommand, ManagementErrorCode, ManagementOperationId, ManagementRepository,
    ManagementResult,
};
use document_domain::{
    Action, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind, PolicyTarget,
};
use serde_json::{Value, json};
use support::{Fixture, context, fixture};
use uuid::Uuid;

fn operation_id() -> ManagementOperationId {
    ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap()
}

fn command(
    f: &Fixture,
    operation_id: ManagementOperationId,
    expected_revision: i64,
    set: BTreeMap<String, Value>,
    unset: BTreeSet<String>,
) -> ManagementCommand {
    ManagementCommand::UpdateDocumentMetadata {
        operation_id,
        document_id: f.document_id,
        expected_document_revision: expected_revision,
        set,
        unset,
        reason: "update common attributes".into(),
    }
}

fn set(key: &str, value: Value) -> BTreeMap<String, Value> {
    BTreeMap::from([(key.into(), value)])
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

async fn state(f: &Fixture) -> (i64, Value) {
    sqlx::query_as("SELECT revision, metadata FROM documents WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

async fn count(f: &Fixture, table: &str, event_type: &str) -> i64 {
    let sql = match table {
        "domain" => "SELECT count(*) FROM outbox_events WHERE event_type = $1",
        "audit" => "SELECT count(*) FROM audit_outbox_events WHERE event_type = $1",
        _ => panic!("unknown event table"),
    };
    sqlx::query_scalar(sql)
        .bind(event_type)
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn partial_patch_preserves_unknown_keys_and_version_state() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write]).await;
    sqlx::query("UPDATE documents SET metadata = $1 WHERE document_id = $2")
        .bind(json!({"legacy_key": {"must": "stay"}, "category": "old", "extensions": {"x": 1}}))
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Initial','test-idp','policy-admin',$3,now())")
        .bind(version_id)
        .bind(f.document_id.as_uuid())
        .bind(json!({"version_key": "unchanged"}))
        .execute(&f.pool)
        .await
        .unwrap();
    let service = DocumentManagementService::new(f.repository.clone());
    let result = service
        .update_document_metadata(
            &context(),
            command(
                &f,
                operation_id(),
                1,
                set("category", json!("new")),
                BTreeSet::from(["extensions".into()]),
            ),
        )
        .await
        .unwrap();
    assert!(result.changed);
    assert_eq!(result.resulting_revision, 2);
    let (revision, metadata) = state(&f).await;
    assert_eq!(revision, 2);
    assert_eq!(
        metadata,
        json!({"legacy_key": {"must": "stay"}, "category": "new"})
    );
    assert_eq!(result.document_metadata, Some(metadata));
    assert_eq!(
        f.repository
            .lookup(&context(), result.operation_id)
            .await
            .unwrap(),
        Some(ManagementResult::MetadataUpdate(result.clone()))
    );
    let version: (i64, String, Value) = sqlx::query_as("SELECT version_no,lifecycle_state,metadata FROM document_versions WHERE document_version_id = $1")
        .bind(version_id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        version,
        (1, "WORKING".into(), json!({"version_key": "unchanged"}))
    );
    assert_eq!(count(&f, "domain", "DocumentMetadataChanged").await, 1);
    assert_eq!(count(&f, "audit", "document.metadata.changed").await, 1);
    let audit_data: Value = sqlx::query_scalar(
        "SELECT data FROM audit_outbox_events WHERE event_type = 'document.metadata.changed'",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        audit_data["changed_keys"],
        json!(["category", "extensions"])
    );
    assert!(!audit_data.to_string().contains("legacy_key"));
}

#[tokio::test]
async fn extensions_set_replaces_the_whole_object() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write]).await;
    sqlx::query("UPDATE documents SET metadata = $1 WHERE document_id = $2")
        .bind(json!({"extensions": {"old": 1}, "legacy_key": true}))
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let service = DocumentManagementService::new(f.repository.clone());
    service
        .update_document_metadata(
            &context(),
            command(
                &f,
                operation_id(),
                1,
                set("extensions", json!({"new": 2})),
                BTreeSet::new(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        state(&f).await.1,
        json!({"extensions": {"new": 2}, "legacy_key": true})
    );
}

#[tokio::test]
async fn replay_and_lookup_require_current_metadata_permissions() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write, Action::Administer]).await;
    let service = DocumentManagementService::new(f.repository.clone());
    let metadata_command = command(
        &f,
        operation_id(),
        1,
        set("category", json!("private")),
        BTreeSet::new(),
    );
    service
        .update_document_metadata(&context(), metadata_command.clone())
        .await
        .unwrap();
    AccessPolicyService::new(f.repository.clone())
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![
                    PolicyGrant::new(
                        PolicySubject::new(
                            PolicySubjectKind::Principal,
                            "test-idp",
                            "policy-admin",
                        )
                        .unwrap(),
                        [Action::Read],
                    )
                    .unwrap(),
                ]),
                reason: "revoke write".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        service
            .update_document_metadata(&context(), metadata_command.clone())
            .await,
        Err(ApplicationError::Forbidden)
    ));
    assert!(matches!(
        f.repository
            .lookup(&context(), metadata_command.operation_id())
            .await,
        Err(document_application::RepositoryError::Forbidden)
    ));
}

#[tokio::test]
async fn concurrent_same_operation_commits_once() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write]).await;
    let service = DocumentManagementService::new(f.repository.clone());
    let cmd = command(
        &f,
        operation_id(),
        1,
        set("category", json!("concurrent")),
        BTreeSet::new(),
    );
    let ctx = context();
    let (first, second) = tokio::join!(
        service.update_document_metadata(&ctx, cmd.clone()),
        service.update_document_metadata(&ctx, cmd),
    );
    assert_eq!(first.unwrap(), second.unwrap());
    assert_eq!(state(&f).await.0, 2);
    assert_eq!(count(&f, "domain", "DocumentMetadataChanged").await, 1);
    assert_eq!(count(&f, "audit", "document.metadata.changed").await, 1);
}

#[tokio::test]
async fn invalid_patch_is_rejected_and_noop_records_only_the_operation() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write]).await;
    let service = DocumentManagementService::new(f.repository.clone());
    for bad in [
        command(
            &f,
            operation_id(),
            1,
            set("category", json!("x")),
            BTreeSet::from(["category".into()]),
        ),
        command(
            &f,
            operation_id(),
            1,
            set("extensions", json!("not object")),
            BTreeSet::new(),
        ),
        command(
            &f,
            operation_id(),
            1,
            set("title", json!("forbidden")),
            BTreeSet::new(),
        ),
    ] {
        assert!(matches!(
            service.update_document_metadata(&context(), bad).await,
            Err(ApplicationError::Validation(_))
        ));
    }
    let noop_command = command(&f, operation_id(), 1, BTreeMap::new(), BTreeSet::new());
    let first = service
        .update_document_metadata(&context(), noop_command.clone())
        .await
        .unwrap();
    assert!(!first.changed);
    assert_eq!(first.resulting_revision, 1);
    assert_eq!(
        first,
        service
            .update_document_metadata(&context(), noop_command.clone())
            .await
            .unwrap()
    );
    assert_eq!(state(&f).await, (1, json!({})));
    assert_eq!(count(&f, "domain", "DocumentMetadataChanged").await, 0);
    assert_eq!(count(&f, "audit", "document.metadata.changed").await, 0);
    let operations: i64 = sqlx::query_scalar("SELECT count(*) FROM document_management_operations WHERE operation_kind = 'update_document_metadata'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(operations, 1);
    assert!(matches!(
        service
            .update_document_metadata(
                &context(),
                command(&f, operation_id(), 0, BTreeMap::new(), BTreeSet::new())
            )
            .await,
        Err(ApplicationError::Management(
            ManagementErrorCode::RevisionConflict
        ))
    ));
    let mut different = noop_command;
    if let ManagementCommand::UpdateDocumentMetadata { reason, .. } = &mut different {
        *reason = "different".into();
    }
    assert!(matches!(
        service
            .update_document_metadata(&context(), different)
            .await,
        Err(ApplicationError::Management(
            ManagementErrorCode::OperationConflict
        ))
    ));
}

#[tokio::test]
async fn pending_schedule_rejects_real_change_but_allows_noop() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write]).await;
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Pending','test-idp','policy-admin','{}',now())")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let publish_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,1,2,now() + interval '1 day','test-idp','policy-admin',$4,'PENDING',now())")
        .bind(publish_id).bind(f.document_id.as_uuid()).bind(version_id).bind(vec![1_u8;32])
        .execute(&f.pool).await.unwrap();
    let service = DocumentManagementService::new(f.repository.clone());
    let noop = service
        .update_document_metadata(
            &context(),
            command(&f, operation_id(), 1, BTreeMap::new(), BTreeSet::new()),
        )
        .await
        .unwrap();
    assert!(!noop.changed);
    assert!(matches!(
        service
            .update_document_metadata(
                &context(),
                command(
                    &f,
                    operation_id(),
                    1,
                    set("category", json!("new")),
                    BTreeSet::new()
                )
            )
            .await,
        Err(ApplicationError::Management(
            ManagementErrorCode::ReservedDocument
        ))
    ));
    assert_eq!(state(&f).await, (1, json!({})));
    sqlx::query("UPDATE document_publish_schedules SET status = 'CANCELLED', cancelled_at = now() WHERE publish_operation_id = $1")
        .bind(publish_id).execute(&f.pool).await.unwrap();
    assert!(
        service
            .update_document_metadata(
                &context(),
                command(
                    &f,
                    operation_id(),
                    1,
                    set("category", json!("new")),
                    BTreeSet::new()
                )
            )
            .await
            .unwrap()
            .changed
    );
}

#[tokio::test]
async fn ended_document_requires_history_and_administer_but_never_reopens() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write]).await;
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Former',now(),'test-idp','policy-admin','{}',now())")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let end_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,0,$4,'test-idp','policy-admin','end',$4,1,now())")
        .bind(end_id).bind(f.document_id.as_uuid()).bind(vec![1_u8;32]).bind(version_id)
        .execute(&f.pool).await.unwrap();
    let service = DocumentManagementService::new(f.repository.clone());
    let metadata_command = command(
        &f,
        operation_id(),
        1,
        set("category", json!("archive")),
        BTreeSet::new(),
    );
    assert!(matches!(
        service
            .update_document_metadata(&context(), metadata_command.clone())
            .await,
        Err(ApplicationError::Forbidden)
    ));
    let policy_id: Uuid =
        sqlx::query_scalar("SELECT policy_id FROM access_policy_bindings WHERE folder_id = $1")
            .bind(f.root_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    for action in ["read_history", "administer"] {
        sqlx::query("INSERT INTO access_policy_grants (policy_id,subject_kind,identity_provider,subject_id,action) VALUES ($1,'principal','test-idp','policy-admin',$2)")
            .bind(policy_id).bind(action).execute(&f.pool).await.unwrap();
    }
    assert!(
        service
            .update_document_metadata(&context(), metadata_command)
            .await
            .unwrap()
            .changed
    );
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, None);
    let end_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_publication_end_operations WHERE document_id = $1",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(end_count, 1);
}

#[tokio::test]
async fn audit_failure_rolls_back_metadata_revision_and_operation() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write]).await;
    sqlx::query("CREATE FUNCTION reject_metadata_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type = 'document.metadata.changed' THEN RAISE EXCEPTION 'audit blocked'; END IF; RETURN NEW; END; $$")
        .execute(&f.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_metadata_audit BEFORE INSERT ON audit_outbox_events FOR EACH ROW EXECUTE FUNCTION reject_metadata_audit()")
        .execute(&f.pool).await.unwrap();
    let service = DocumentManagementService::new(f.repository.clone());
    assert!(
        service
            .update_document_metadata(
                &context(),
                command(
                    &f,
                    operation_id(),
                    1,
                    set("category", json!("new")),
                    BTreeSet::new()
                )
            )
            .await
            .is_err()
    );
    assert_eq!(state(&f).await, (1, json!({})));
    assert_eq!(count(&f, "domain", "DocumentMetadataChanged").await, 0);
    let operations: i64 = sqlx::query_scalar("SELECT count(*) FROM document_management_operations WHERE operation_kind = 'update_document_metadata'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(operations, 0);
}

#[tokio::test]
async fn metadata_revision_overflow_does_not_mutate() {
    let f = fixture().await;
    allow(&f, [Action::Read, Action::Write]).await;
    sqlx::query("UPDATE documents SET revision = $1 WHERE document_id = $2")
        .bind(i64::MAX)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let result = DocumentManagementService::new(f.repository.clone())
        .update_document_metadata(
            &context(),
            command(
                &f,
                operation_id(),
                i64::MAX,
                set("category", json!("new")),
                BTreeSet::new(),
            ),
        )
        .await;
    assert!(matches!(result, Err(ApplicationError::BusinessRule)));
    assert_eq!(state(&f).await, (i64::MAX, json!({})));
    assert_eq!(count(&f, "domain", "DocumentMetadataChanged").await, 0);
}
