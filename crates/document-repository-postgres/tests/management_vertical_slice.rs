#[path = "support/management.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use document_application::{
    AccessPolicyService, ApplicationError, BootstrapRootPolicy, DocumentHistoryService,
    DocumentListFilter, DocumentManagementService, DocumentQueryService, FolderService,
    HistoryPageQuery, ManagementCommand, ManagementOperationId, MarkVersionRead, PublishedQuery,
    ReadStateService,
};
use document_domain::{
    Action, DocumentVersionId, FolderId, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind,
    PolicyTarget,
};
use serde_json::json;
use support::{context, fixture};
use uuid::Uuid;

type MetadataRevisionRow = (
    i64,
    i64,
    String,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    Option<serde_json::Value>,
);

fn operation_id() -> ManagementOperationId {
    ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap()
}

#[tokio::test]
async fn management_changes_flow_into_published_list_read_state_and_history_then_revoke() {
    let f = fixture().await;
    let grant = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        [
            Action::Read,
            Action::ReadHistory,
            Action::Write,
            Action::Publish,
            Action::Administer,
        ],
    )
    .unwrap();
    f.repository
        .initialize_root_policy(&context(), vec![grant])
        .await
        .unwrap();
    let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Operations guide',now(),'test-idp','policy-admin','{}',now())")
        .bind(version_id.as_uuid()).bind(f.document_id.as_uuid())
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO document_revisions \
         (document_id,document_version_id,major_no,minor_no,metadata_snapshot, \
          metadata_snapshot_status,source_kind,operation_id,created_at, \
          actor_identity_provider,actor_principal_id,reason) \
         VALUES ($1,$2,1,0, \
                 jsonb_build_object('document_type',NULL,'owning_department',NULL, \
                                    'category',NULL,'extensions',NULL), \
                 'complete','legacyBackfill',NULL,now(),NULL,NULL,NULL)",
    )
    .bind(f.document_id.as_uuid())
    .bind(version_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();

    let management = DocumentManagementService::new(f.repository.clone());
    let metadata_operation_id = operation_id();
    let metadata_command = ManagementCommand::UpdateDocumentMetadata {
        operation_id: metadata_operation_id,
        document_id: f.document_id,
        expected_document_revision: 1,
        set: BTreeMap::from([("category".into(), json!("operations"))]),
        unset: BTreeSet::new(),
        reason: "classify".into(),
    };
    let updated = management
        .update_document_metadata(&context(), metadata_command.clone())
        .await
        .unwrap();
    assert_eq!(updated.resulting_revision, 2);
    let metadata_revision: MetadataRevisionRow = sqlx::query_as(
        "SELECT major_no,minor_no,source_kind,operation_id,actor_identity_provider, \
                    actor_principal_id,metadata_snapshot \
             FROM document_revisions WHERE document_id = $1 \
             ORDER BY major_no DESC,minor_no DESC LIMIT 1",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        metadata_revision,
        (
            1,
            1,
            "metadataRevision".to_owned(),
            Some(metadata_operation_id.as_uuid()),
            Some("test-idp".to_owned()),
            Some("policy-admin".to_owned()),
            Some(serde_json::json!({
                "document_type": null,
                "owning_department": null,
                "category": "operations",
                "extensions": null
            }))
        )
    );
    assert_eq!(
        management
            .update_document_metadata(&context(), metadata_command.clone())
            .await
            .unwrap(),
        updated,
        "exact management replay returns the prior mutation"
    );
    let no_op = management
        .update_document_metadata(
            &context(),
            ManagementCommand::UpdateDocumentMetadata {
                operation_id: operation_id(),
                document_id: f.document_id,
                expected_document_revision: 2,
                set: BTreeMap::from([("category".into(), json!("operations"))]),
                unset: BTreeSet::new(),
                reason: "repeat classification".into(),
            },
        )
        .await
        .unwrap();
    assert!(!no_op.changed);
    assert_eq!(no_op.resulting_revision, 2);

    let folder_id = FolderId::from_uuid(Uuid::now_v7());
    FolderService::new(f.repository.clone())
        .create_folder(
            &context(),
            ManagementCommand::CreateFolder {
                operation_id: operation_id(),
                folder_id,
                parent_folder_id: f.root_id,
                expected_parent_revision: 0,
                name: "Operations".into(),
                reason: "organize".into(),
            },
        )
        .await
        .unwrap();
    let moved = management
        .move_document(
            &context(),
            ManagementCommand::MoveDocument {
                operation_id: operation_id(),
                document_id: f.document_id,
                from_folder_id: f.root_id,
                to_folder_id: folder_id,
                expected_document_revision: 2,
                reason: "file with team".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.resulting_revision, 3);
    let persisted: (Uuid, Option<Uuid>, i64) = sqlx::query_as(
        "SELECT folder_id,current_version_id,revision FROM documents WHERE document_id = $1",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (folder_id.as_uuid(), Some(version_id.as_uuid()), 3)
    );
    let revision_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_revisions WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        revision_count, 2,
        "folder, policy and read-state changes do not revise content"
    );

    let query = DocumentQueryService::new(f.repository.clone());
    let list = query
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    folder_id: Some(folder_id),
                    category: Some("operations".into()),
                    ..Default::default()
                },
                unread_only: true,
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].document_version_id, version_id);
    ReadStateService::new(f.repository.clone())
        .mark_version_read(
            &context(),
            MarkVersionRead {
                document_id: f.document_id,
                document_version_id: version_id,
            },
        )
        .await
        .unwrap();
    assert!(
        query
            .list_published_documents(
                &context(),
                PublishedQuery {
                    unread_only: true,
                    ..PublishedQuery::default()
                },
            )
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let history = DocumentHistoryService::new(f.repository.clone())
        .list_document_history(
            &context(),
            HistoryPageQuery {
                document_id: f.document_id,
                page_size: None,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert!(
        history
            .items
            .iter()
            .any(|item| item.action_code == "document.metadata.changed")
    );
    assert!(
        history
            .items
            .iter()
            .any(|item| item.action_code == "document.moved")
    );

    let policy = AccessPolicyService::new(f.repository.clone());
    assert!(matches!(
        policy
            .set_access_policy(
                &context(),
                ManagementCommand::SetAccessPolicy {
                    operation_id: operation_id(),
                    target: PolicyTarget::Document(f.document_id),
                    expected_policy_revision: 0,
                    mode: PolicyMode::Explicit(vec![]),
                    reason: "invalid empty policy".into(),
                },
            )
            .await,
        Err(ApplicationError::Validation(_))
    ));
    let other_only = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "other-reader").unwrap(),
        [Action::Read],
    )
    .unwrap();
    policy
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![other_only]),
                reason: "revoke access".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        query
            .list_published_documents(&context(), PublishedQuery::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(matches!(
        DocumentHistoryService::new(f.repository.clone())
            .list_document_history(
                &context(),
                HistoryPageQuery {
                    document_id: f.document_id,
                    page_size: None,
                    cursor: None,
                },
            )
            .await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert!(matches!(
        management
            .update_document_metadata(&context(), metadata_command)
            .await,
        Err(ApplicationError::Forbidden)
    ));
}
