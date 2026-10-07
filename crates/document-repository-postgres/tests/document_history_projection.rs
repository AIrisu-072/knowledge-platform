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

#[tokio::test]
async fn current_publication_schedule_identity_matches_only_the_authorized_version_snapshot() {
    use document_application::{VersionPurpose, VersionRequest};
    use document_domain::DocumentVersionId;

    let f = fixture().await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant([Action::Read, Action::Write, Action::ReadHistory])],
        )
        .await
        .unwrap();
    let working = Uuid::now_v7();
    let historical = Uuid::now_v7();
    for (version, number, state) in [(historical, 1_i64, "PUBLISHED"), (working, 2, "WORKING")] {
        sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,scheduled_publish_at,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,$3,$4,'予約対象',to_timestamp(100),CASE WHEN $4 = 'PUBLISHED' THEN to_timestamp(50) ELSE NULL END,'test-idp','policy-admin','{}',now())")
            .bind(version).bind(f.document_id.as_uuid()).bind(number).bind(state)
            .execute(&f.pool).await.unwrap();
    }
    let request = VersionRequest {
        document_id: f.document_id,
        document_version_id: DocumentVersionId::from_uuid(working),
        purpose: VersionPurpose::Authoring,
    };
    let service = DocumentHistoryService::new(f.repository.clone());
    assert_eq!(
        service
            .get_document_version(&context(), request)
            .await
            .unwrap()
            .current_publication_schedule_id,
        None
    );

    let other_document = Uuid::now_v7();
    let other_version = Uuid::now_v7();
    sqlx::query("INSERT INTO documents (document_id,folder_id,revision,metadata,created_at) VALUES ($1,$2,1,'{}',now())")
        .bind(other_document).bind(f.root_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,scheduled_publish_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'WORKING','別文書',to_timestamp(100),'test-idp','policy-admin','{}',now())")
        .bind(other_version).bind(other_document).execute(&f.pool).await.unwrap();
    for (document, version) in [
        (other_document, other_version),
        (f.document_id.as_uuid(), historical),
    ] {
        sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,1,2,to_timestamp(100),'test-idp','policy-admin',$4,'PENDING',now())")
            .bind(Uuid::now_v7()).bind(document).bind(version).bind(vec![1_u8; 32])
            .execute(&f.pool).await.unwrap();
    }
    // 同時刻でも別文書・別Versionの予約を返さない。
    assert_eq!(
        service
            .get_document_version(&context(), request)
            .await
            .unwrap()
            .current_publication_schedule_id,
        None
    );
    sqlx::query(
        "UPDATE document_publish_schedules SET status = 'CANCELLED' WHERE document_id = $1",
    )
    .bind(f.document_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();

    let schedule = Uuid::now_v7();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,1,2,to_timestamp(101),'test-idp','policy-admin',$4,'PENDING',now())")
        .bind(schedule).bind(f.document_id.as_uuid()).bind(working).bind(vec![2_u8; 32])
        .execute(&f.pool).await.unwrap();
    // 同じ対象でもVersion側の時刻projectionと一致しなければ返さない。
    assert_eq!(
        service
            .get_document_version(&context(), request)
            .await
            .unwrap()
            .current_publication_schedule_id,
        None
    );
    sqlx::query("UPDATE document_publish_schedules SET scheduled_publish_at = to_timestamp(100) WHERE publish_operation_id = $1")
        .bind(schedule).execute(&f.pool).await.unwrap();
    assert_eq!(
        service
            .get_document_version(&context(), request)
            .await
            .unwrap()
            .current_publication_schedule_id,
        Some(schedule)
    );

    for status in ["CANCELLED", "PUBLISHED", "TERMINAL"] {
        sqlx::query(
            "UPDATE document_publish_schedules SET status = $1 WHERE publish_operation_id = $2",
        )
        .bind(status)
        .bind(schedule)
        .execute(&f.pool)
        .await
        .unwrap();
        assert_eq!(
            service
                .get_document_version(&context(), request)
                .await
                .unwrap()
                .current_publication_schedule_id,
            None,
            "{status}"
        );
    }
    // 再予約では過去の履歴IDではなく、新しいPENDINGのIDだけを返す。
    let replacement = Uuid::now_v7();
    sqlx::query("INSERT INTO document_publish_schedules (publish_operation_id,document_id,target_document_version_id,expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status,created_at) VALUES ($1,$2,$3,1,2,to_timestamp(100),'test-idp','policy-admin',$4,'PENDING',now())")
        .bind(replacement).bind(f.document_id.as_uuid()).bind(working).bind(vec![3_u8; 32])
        .execute(&f.pool).await.unwrap();
    assert_eq!(
        service
            .get_document_version(&context(), request)
            .await
            .unwrap()
            .current_publication_schedule_id,
        Some(replacement)
    );
    sqlx::query(
        "UPDATE document_versions SET scheduled_publish_at = NULL WHERE document_version_id = $1",
    )
    .bind(working)
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        service
            .get_document_version(&context(), request)
            .await
            .unwrap()
            .current_publication_schedule_id,
        None
    );

    let wrong_document = VersionRequest {
        document_id: document_domain::DocumentId::from_uuid(other_document),
        ..request
    };
    assert!(matches!(
        service
            .get_document_version(&context(), wrong_document)
            .await,
        Err(ApplicationError::DocumentVersionNotFound)
    ));
    sqlx::query("DELETE FROM access_policy_grants WHERE action = 'write'")
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(matches!(
        service.get_document_version(&context(), request).await,
        Err(ApplicationError::DocumentVersionNotFound)
    ));
    assert!(matches!(
        service
            .get_document_version(
                &context(),
                VersionRequest {
                    purpose: VersionPurpose::History,
                    ..request
                }
            )
            .await,
        Err(ApplicationError::DocumentVersionNotFound)
    ));
    sqlx::query("DELETE FROM access_policy_grants WHERE action = 'read'")
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(matches!(
        service.get_document_version(&context(), request).await,
        Err(ApplicationError::DocumentNotFound)
    ));
}

#[tokio::test]
async fn reset_preserves_version_historical_first_record() {
    use document_application::{
        CurrentReadStateService, ReadStateMutation, ReadStateMutationKind, ReadStateOperationId,
        VersionPurpose, VersionRequest,
    };
    use document_domain::DocumentVersionId;
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO document_versions(document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES($1,$2,1,'PUBLISHED','Synthetic',now(),'test-idp','policy-admin','{}',now())")
        .bind(version_id.as_uuid()).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id=$1 WHERE document_id=$2")
        .bind(version_id.as_uuid()).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let service = CurrentReadStateService::new(f.repository.clone());
    let first = service.mutate_read_state(&context(), ReadStateMutation {
        operation_id: ReadStateOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
        document_id: f.document_id,
        document_version_id: version_id,
        expected_read_state_revision: 0,
        kind: ReadStateMutationKind::View,
    }).await.unwrap();
    service.mutate_read_state(&context(), ReadStateMutation {
        operation_id: ReadStateOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
        document_id: f.document_id,
        document_version_id: version_id,
        expected_read_state_revision: 1,
        kind: ReadStateMutationKind::Reset,
    }).await.unwrap();
    let history = DocumentHistoryService::new(f.repository.clone()).get_document_version(&context(), VersionRequest {
        document_id: f.document_id,
        document_version_id: version_id,
        purpose: VersionPurpose::History,
    }).await.unwrap();
    assert_eq!(history.summary.first_read_at, first.resulting_read_state.first_read_at);
    assert!(!service.get_current_read_state(&context(), f.document_id, version_id).await.unwrap().state.is_read());
}
