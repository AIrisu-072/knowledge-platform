#[path = "support/management.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use document_application::{
    AccessPolicyService, BootstrapRootPolicy, DocumentManagementService, FolderService,
    ManagementCommand, ManagementOperationId, MarkVersionRead, ReadStateService,
};
use document_domain::{
    Action, DocumentVersionId, FolderId, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind,
    PolicyTarget,
};
use serde_json::json;
use support::{context, fixture};
use uuid::Uuid;

fn operation_id() -> ManagementOperationId {
    ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap()
}

fn grant(actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        actions,
    )
    .unwrap()
}

async fn count(f: &support::Fixture, table: &str, event_type: &str) -> i64 {
    let query = match table {
        "domain" => "SELECT count(*) FROM outbox_events WHERE event_type = $1",
        "audit" => "SELECT count(*) FROM audit_outbox_events WHERE event_type = $1",
        _ => unreachable!(),
    };
    sqlx::query_scalar(query)
        .bind(event_type)
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn t5_t7_t8_and_t9_use_the_declared_event_and_audit_matrix() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant([
                Action::Read,
                Action::Write,
                Action::ReadHistory,
                Action::Administer,
                Action::Publish,
            ])],
        )
        .await
        .unwrap();
    let metadata_id = operation_id();
    let metadata_command = ManagementCommand::UpdateDocumentMetadata {
        operation_id: metadata_id,
        document_id: f.document_id,
        expected_document_revision: 1,
        set: BTreeMap::from([("category".into(), json!("report"))]),
        unset: BTreeSet::new(),
        reason: "set category".into(),
    };
    let metadata = DocumentManagementService::new(f.repository.clone())
        .update_document_metadata(&context(), metadata_command.clone())
        .await
        .unwrap();
    assert_eq!(metadata.resulting_revision, 2);
    assert_eq!(count(&f, "domain", "DocumentMetadataChanged").await, 1);
    assert_eq!(count(&f, "audit", "document.metadata.changed").await, 1);
    let replay = DocumentManagementService::new(f.repository.clone())
        .update_document_metadata(&context(), metadata_command)
        .await
        .unwrap();
    assert_eq!(replay, metadata);
    assert_eq!(count(&f, "domain", "DocumentMetadataChanged").await, 1);

    let folder_id = FolderId::from_uuid(Uuid::now_v7());
    FolderService::new(f.repository.clone())
        .create_folder(
            &context(),
            ManagementCommand::CreateFolder {
                operation_id: operation_id(),
                folder_id,
                parent_folder_id: f.root_id,
                expected_parent_revision: 0,
                name: "Reports".into(),
                reason: "create".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(count(&f, "domain", "FolderCreated").await, 1);
    assert_eq!(count(&f, "audit", "folder.created").await, 1);
    let typed: (String, Uuid) = sqlx::query_as("SELECT resource_type,resource_id FROM audit_outbox_events WHERE event_type = 'folder.created'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(typed, ("Folder".into(), folder_id.as_uuid()));

    AccessPolicyService::new(f.repository.clone())
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: operation_id(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![grant([
                    Action::Read,
                    Action::Write,
                    Action::ReadHistory,
                    Action::Administer,
                ])]),
                reason: "document override".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(count(&f, "domain", "AccessPolicyChanged").await, 2);
    assert_eq!(count(&f, "audit", "access_policy.changed").await, 2);

    let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Read me',now(),'test-idp','policy-admin','{}',now())")
        .bind(version_id.as_uuid()).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let read_service = ReadStateService::new(f.repository.clone());
    let command = MarkVersionRead {
        document_id: f.document_id,
        document_version_id: version_id,
    };
    read_service
        .mark_version_read(&context(), command)
        .await
        .unwrap();
    read_service
        .mark_version_read(&context(), command)
        .await
        .unwrap();
    assert_eq!(
        count(&f, "audit", "document.version.read_confirmed").await,
        1
    );
    assert_eq!(count(&f, "domain", "DocumentVersionReadConfirmed").await, 0);
    let ledger_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_management_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(ledger_count, 3);
}

#[tokio::test]
async fn t6_and_t7_rename_move_record_one_typed_pair_per_change() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant([Action::Read, Action::Write, Action::Administer])],
        )
        .await
        .unwrap();
    let folders = FolderService::new(f.repository.clone());
    let moved_folder = FolderId::from_uuid(Uuid::now_v7());
    let parent = FolderId::from_uuid(Uuid::now_v7());
    for (folder_id, revision, name) in [(moved_folder, 0, "Before"), (parent, 0, "Destination")] {
        folders
            .create_folder(
                &context(),
                ManagementCommand::CreateFolder {
                    operation_id: operation_id(),
                    folder_id,
                    parent_folder_id: f.root_id,
                    expected_parent_revision: revision,
                    name: name.into(),
                    reason: "create".into(),
                },
            )
            .await
            .unwrap();
    }
    let renamed = folders
        .rename_folder(
            &context(),
            ManagementCommand::RenameFolder {
                operation_id: operation_id(),
                folder_id: moved_folder,
                expected_folder_revision: 0,
                name: "After".into(),
                reason: "rename".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.resulting_revision, 1);
    let moved = folders
        .move_folder(
            &context(),
            ManagementCommand::MoveFolder {
                operation_id: operation_id(),
                folder_id: moved_folder,
                from_parent_id: f.root_id,
                to_parent_id: parent,
                expected_folder_revision: 1,
                reason: "move subtree".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.resulting_revision, 2);
    let document = DocumentManagementService::new(f.repository.clone())
        .move_document(
            &context(),
            ManagementCommand::MoveDocument {
                operation_id: operation_id(),
                document_id: f.document_id,
                from_folder_id: f.root_id,
                to_folder_id: moved_folder,
                expected_document_revision: 1,
                reason: "move document".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(document.resulting_revision, 2);
    for (domain, audit, target) in [
        ("FolderRenamed", "folder.renamed", "Folder"),
        ("FolderMoved", "folder.moved", "Folder"),
        ("DocumentMoved", "document.moved", "Document"),
    ] {
        assert_eq!(count(&f, "domain", domain).await, 1);
        assert_eq!(count(&f, "audit", audit).await, 1);
        let recorded_type: String = sqlx::query_scalar(
            "SELECT resource_type FROM audit_outbox_events WHERE event_type = $1",
        )
        .bind(audit)
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(recorded_type, target);
    }
}
