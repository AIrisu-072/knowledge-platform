#[path = "support/management.rs"]
mod support;

use document_application::{
    ApplicationError, BootstrapRootPolicy, DocumentHistoryService, HistoryPageQuery,
    ProvenanceQuality, VersionPageQuery,
};
use document_domain::{Action, PolicyGrant, PolicySubject, PolicySubjectKind};
use support::{context, fixture};
use uuid::Uuid;

fn grant(actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        actions,
    )
    .unwrap()
}

#[tokio::test]
async fn history_is_authorized_and_survives_outbox_cleanup() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read])])
        .await
        .unwrap();
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Historical',to_timestamp(20),'test-idp','policy-admin','{}',to_timestamp(10))")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let service = DocumentHistoryService::new(f.repository.clone());
    assert!(matches!(
        service
            .list_document_history(
                &context(),
                HistoryPageQuery {
                    document_id: f.document_id,
                    page_size: None,
                    cursor: None,
                }
            )
            .await,
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
    sqlx::query("INSERT INTO document_management_operations (operation_id,operation_kind,resource_type,resource_id,expected_revision,actor_identity_provider,actor_principal_id,command_digest,changed,result,resulting_revision,occurred_at) VALUES ($1,'update_document_metadata','Document',$2,1,'test-idp','policy-admin',$3,true,'{}',2,to_timestamp(30))")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(vec![0_u8;32])
        .execute(&f.pool).await.unwrap();
    let query = HistoryPageQuery {
        document_id: f.document_id,
        page_size: None,
        cursor: None,
    };
    let before = service
        .list_document_history(&context(), query.clone())
        .await
        .unwrap();
    assert_eq!(before.items.len(), 3);
    assert!(
        before
            .items
            .iter()
            .any(|entry| entry.action_code == "document.version.created")
    );
    assert!(before.items.iter().any(|entry| entry.action_code == "document.version.published" && entry.actor.is_none()));
    assert!(
        before
            .items
            .iter()
            .any(|entry| entry.action_code == "document.metadata.changed")
    );
    sqlx::query("DELETE FROM outbox_events")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM audit_outbox_events")
        .execute(&f.pool)
        .await
        .unwrap();
    let after = service
        .list_document_history(&context(), query)
        .await
        .unwrap();
    assert_eq!(after.items, before.items);
    let versions = service
        .list_document_versions(
            &context(),
            VersionPageQuery {
                document_id: f.document_id,
                purpose: document_application::VersionPurpose::History,
                page_size: None,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(versions.items.len(), 1);
    assert_eq!(versions.items[0].document_version_id.as_uuid(), version_id);
    let working_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,2,'WORKING','Private draft','test-idp','policy-admin','{}',now())")
        .bind(working_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let hidden_working = service
        .list_document_versions(
            &context(),
            VersionPageQuery {
                document_id: f.document_id,
                purpose: document_application::VersionPurpose::History,
                page_size: None,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(hidden_working.items.len(), 1);
    sqlx::query("INSERT INTO access_policy_grants (policy_id,subject_kind,identity_provider,subject_id,action) VALUES ($1,'principal','test-idp','policy-admin','write')")
        .bind(policy_id).execute(&f.pool).await.unwrap();
    let visible_working = service
        .list_document_versions(
            &context(),
            VersionPageQuery {
                document_id: f.document_id,
                purpose: document_application::VersionPurpose::History,
                page_size: None,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(visible_working.items.len(), 2);
    assert_eq!(
        visible_working.items[0].document_version_id.as_uuid(),
        working_id
    );
}

#[tokio::test]
async fn publish_ledger_replaces_fallback_and_unknown_terminal_facts_stay_unknown() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Historical',to_timestamp(20),'test-idp','policy-admin','{}',to_timestamp(10))")
        .bind(version_id).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id)
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_publish_operations (publish_operation_id,document_id,target_document_version_id,expected_document_revision,actor_identity_provider,actor_principal_id,published_at,resulting_document_revision,created_at) VALUES ($1,$2,$3,1,'test-idp','policy-admin',to_timestamp(20),2,to_timestamp(19))")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(version_id)
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,terminal_reason,created_at) VALUES ($1,$2,$3,1,2,to_timestamp(30),'test-idp','policy-admin',$4,'TERMINAL','legacy_reason',to_timestamp(15))")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(version_id).bind(vec![1_u8;32])
        .execute(&f.pool).await.unwrap();
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
    let published = history
        .items
        .iter()
        .filter(|entry| entry.action_code == "document.version.published")
        .collect::<Vec<_>>();
    assert_eq!(published.len(), 1);
    assert_eq!(
        published[0].provenance_quality,
        ProvenanceQuality::OperationLedger
    );
    assert!(published[0].actor.is_some());
    let terminal = history
        .items
        .iter()
        .find(|entry| entry.action_code == "document.version.publication.terminal")
        .unwrap();
    assert_eq!(
        terminal.provenance_quality,
        ProvenanceQuality::LegacyUnknown
    );
    assert!(terminal.occurred_at.is_none());
    assert!(terminal.actor.is_none());
    assert_eq!(
        history.items.last().unwrap().source_key,
        terminal.source_key
    );
}
