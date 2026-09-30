#[path = "support/management.rs"]
mod support;

use document_application::{
    AccessPolicyReadService, ApplicationError, BootstrapRootPolicy, CreateOutcomeProbe,
    CreateOutcomeRecoveryService, DocumentManagementService, DocumentQueryService,
    ManagementCommand, ManagementErrorCode, ManagementOperationId, ManagementRepository,
    PolicyBindingMode, RepositoryError,
};
use document_domain::{
    Action, DocumentId, DocumentVersionId, FileId, FolderId, PolicyGrant, PolicyMode,
    PolicySubject, PolicySubjectKind, PolicyTarget, PrincipalRef,
};
use support::{context, fixture};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

fn grant(actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        actions,
    )
    .unwrap()
}

fn operation_id() -> ManagementOperationId {
    ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap()
}

#[tokio::test]
async fn policy_read_separates_local_revision_from_effective_grants_and_rechecks_administer() {
    let fixture = fixture().await;
    fixture
        .repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::Administer])])
        .await
        .unwrap();
    let service = AccessPolicyReadService::new(fixture.repository.clone());
    let target = PolicyTarget::Document(fixture.document_id);
    let inherited = service.read(&context(), target).await.unwrap();
    assert_eq!(inherited.binding_mode, PolicyBindingMode::Inherit);
    assert_eq!(inherited.policy_id, None);
    assert_eq!(inherited.policy_revision, 0);
    assert_eq!(
        inherited.effective_source,
        PolicyTarget::Folder(fixture.root_id)
    );
    assert_eq!(
        inherited.effective_grants,
        vec![grant([Action::Read, Action::Administer])]
    );

    document_application::AccessPolicyService::new(fixture.repository.clone())
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target,
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![grant([Action::Read, Action::Administer])]),
                reason: "explicit override".into(),
            },
        )
        .await
        .unwrap();
    let explicit = service.read(&context(), target).await.unwrap();
    assert_eq!(explicit.binding_mode, PolicyBindingMode::Explicit);
    assert_eq!(explicit.policy_revision, 1);
    assert!(explicit.policy_id.is_some());
    assert_eq!(explicit.effective_source, target);

    let expiring = document_application::VerifiedActorContext::from_trusted_adapter(
        support::actor(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap()],
        OffsetDateTime::now_utc() + Duration::milliseconds(100),
        document_application::InvocationKind::HumanInteractive,
        None,
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(matches!(
        service.read(&expiring, target).await,
        Err(ApplicationError::Validation(_))
    ));

    document_application::AccessPolicyService::new(fixture.repository.clone())
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target,
                expected_policy_revision: 1,
                mode: PolicyMode::Explicit(vec![grant([Action::Read])]),
                reason: "remove administer".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        service.read(&context(), target).await,
        Err(ApplicationError::Forbidden)
    ));
}

#[tokio::test]
async fn root_discovery_returns_current_visible_revision_without_exposing_hidden_root() {
    let fixture = fixture().await;
    let service = DocumentQueryService::new(fixture.repository.clone());
    assert!(matches!(
        service.get_root_folder(&context()).await,
        Err(ApplicationError::FolderNotFound)
    ));
    fixture
        .repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::Administer])])
        .await
        .unwrap();
    let root = service.get_root_folder(&context()).await.unwrap();
    assert_eq!(root.folder_id, fixture.root_id);
    assert_eq!(root.revision, 0);
    assert!(!root.name.is_empty());

    let outsider = PrincipalRef::new("test-idp", "outsider").unwrap();
    let outsider_subject =
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "outsider").unwrap();
    let hidden = document_application::VerifiedActorContext::from_trusted_adapter(
        outsider,
        vec![outsider_subject],
        OffsetDateTime::now_utc() + Duration::hours(1),
        document_application::InvocationKind::HumanInteractive,
        None,
    )
    .unwrap();
    assert!(matches!(
        service.get_root_folder(&hidden).await,
        Err(ApplicationError::FolderNotFound)
    ));
}

#[tokio::test]
async fn create_outcome_requires_all_generated_ids_and_current_authoring_permission() {
    let fixture = fixture().await;
    fixture
        .repository
        .initialize_root_policy(
            &context(),
            vec![grant([Action::Read, Action::Write, Action::Administer])],
        )
        .await
        .unwrap();
    let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let file_id = FileId::from_uuid(Uuid::now_v7());
    let content_item_id = Uuid::now_v7();
    let representation_id = Uuid::now_v7();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',1,'fixture',now())")
        .bind(file_id.as_uuid()).bind(vec![0_u8; 32]).execute(&fixture.pool).await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Initial','test-idp','policy-admin','{}',now())")
        .bind(version_id.as_uuid()).bind(fixture.document_id.as_uuid()).execute(&fixture.pool).await.unwrap();
    let mut tx = fixture.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(content_item_id).bind(version_id.as_uuid()).bind(representation_id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,'AUTHORITATIVE','fixture.txt')")
        .bind(representation_id).bind(content_item_id).bind(file_id.as_uuid()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();

    let service = CreateOutcomeRecoveryService::new(fixture.repository.clone());
    let probe = CreateOutcomeProbe {
        document_id: fixture.document_id,
        document_version_id: version_id,
        file_id,
    };
    let result = service.recover(&context(), probe).await.unwrap().unwrap();
    assert_eq!(result.document_id(), fixture.document_id);
    assert_eq!(result.document_version_id(), version_id);
    assert_eq!(result.file_id(), file_id);
    assert!(
        service
            .recover(
                &context(),
                CreateOutcomeProbe {
                    file_id: FileId::from_uuid(Uuid::now_v7()),
                    ..probe
                }
            )
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        service
            .recover(
                &context(),
                CreateOutcomeProbe {
                    document_id: DocumentId::from_uuid(Uuid::now_v7()),
                    ..probe
                }
            )
            .await
            .unwrap()
            .is_none()
    );

    document_application::AccessPolicyService::new(fixture.repository.clone())
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Document(fixture.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![grant([Action::Read])]),
                reason: "remove authoring".into(),
            },
        )
        .await
        .unwrap();
    assert!(service.recover(&context(), probe).await.unwrap().is_none());
}

#[tokio::test]
async fn root_and_cycle_management_failures_retain_typed_reason() {
    let fixture = fixture().await;
    fixture
        .repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::Administer])])
        .await
        .unwrap();
    let root_rename = ManagementCommand::RenameFolder {
        operation_id: operation_id(),
        folder_id: fixture.root_id,
        expected_folder_revision: 0,
        name: "Renamed".into(),
        reason: "fixture".into(),
    };
    let error = fixture
        .repository
        .execute(&context(), root_rename)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        RepositoryError::Management(ManagementErrorCode::RootProtected)
    );
    assert_eq!(
        ApplicationError::from(error),
        ApplicationError::Management(ManagementErrorCode::RootProtected)
    );

    let child = FolderId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$2,'Child','ACTIVE',0,now())")
        .bind(child.as_uuid()).bind(fixture.root_id.as_uuid()).execute(&fixture.pool).await.unwrap();
    let cycle = ManagementCommand::MoveFolder {
        operation_id: operation_id(),
        folder_id: child,
        from_parent_id: fixture.root_id,
        to_parent_id: child,
        expected_folder_revision: 0,
        reason: "fixture".into(),
    };
    let error = fixture
        .repository
        .execute(&context(), cycle)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        RepositoryError::Management(ManagementErrorCode::FolderCycle)
    );
}

#[tokio::test]
async fn inherited_document_policy_tracks_parent_policy_changes() {
    let fixture = fixture().await;
    fixture
        .repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::Administer])])
        .await
        .unwrap();
    let child = FolderId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$2,'Child','ACTIVE',0,now())")
        .bind(child.as_uuid()).bind(fixture.root_id.as_uuid()).execute(&fixture.pool).await.unwrap();
    sqlx::query("UPDATE documents SET folder_id = $1 WHERE document_id = $2")
        .bind(child.as_uuid())
        .bind(fixture.document_id.as_uuid())
        .execute(&fixture.pool)
        .await
        .unwrap();
    let reader = AccessPolicyReadService::new(fixture.repository.clone());
    let target = PolicyTarget::Document(fixture.document_id);
    let before = reader.read(&context(), target).await.unwrap();
    assert_eq!(
        before.effective_source,
        PolicyTarget::Folder(fixture.root_id)
    );

    document_application::AccessPolicyService::new(fixture.repository.clone())
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Folder(child),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![grant([Action::Administer])]),
                reason: "parent override".into(),
            },
        )
        .await
        .unwrap();
    let after = reader.read(&context(), target).await.unwrap();
    assert_eq!(after.binding_mode, PolicyBindingMode::Inherit);
    assert_eq!(after.policy_revision, 0);
    assert_eq!(after.effective_source, PolicyTarget::Folder(child));
    assert_eq!(after.effective_grants, vec![grant([Action::Administer])]);
}

#[tokio::test]
async fn pending_schedule_reports_reserved_document_as_typed_management_error() {
    let fixture = fixture().await;
    fixture
        .repository
        .initialize_root_policy(
            &context(),
            vec![grant([Action::Read, Action::Write, Action::Administer])],
        )
        .await
        .unwrap();
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','Pending','test-idp','policy-admin','{}',now())")
        .bind(version_id).bind(fixture.document_id.as_uuid()).execute(&fixture.pool).await.unwrap();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,1,2,now() + interval '1 day','test-idp','policy-admin',$4,'PENDING',now())")
        .bind(Uuid::now_v7()).bind(fixture.document_id.as_uuid()).bind(version_id).bind(vec![1_u8; 32]).execute(&fixture.pool).await.unwrap();
    let service = DocumentManagementService::new(fixture.repository.clone());
    let error = service
        .update_document_metadata(
            &context(),
            ManagementCommand::UpdateDocumentMetadata {
                operation_id: operation_id(),
                document_id: fixture.document_id,
                expected_document_revision: 1,
                set: [("category".into(), serde_json::json!("fixture"))].into(),
                unset: Default::default(),
                reason: "fixture".into(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ApplicationError::Management(ManagementErrorCode::ReservedDocument)
    );
}
