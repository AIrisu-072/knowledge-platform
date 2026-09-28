#[path = "support/management.rs"]
mod support;

use document_application::{
    ApplicationError, AuthoringQuery, BootstrapRootPolicy, DocumentListFilter,
    DocumentQueryService, DocumentSort, FolderPageQuery, HistoryQuery, InvocationKind,
    MarkVersionRead, PublishedQuery, ReadStateService, VerifiedActorContext,
};
use document_domain::{
    Action, DocumentVersionId, FolderId, PolicyGrant, PolicySubject, PolicySubjectKind,
    PrincipalRef,
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

async fn add_version(
    f: &support::Fixture,
    version_no: i64,
    title: &str,
    state: &str,
) -> DocumentVersionId {
    let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,$3,$4,$5,CASE WHEN $4 = 'PUBLISHED' THEN now() ELSE NULL END,'test-idp','policy-admin','{}',now())")
        .bind(version_id.as_uuid())
        .bind(f.document_id.as_uuid())
        .bind(version_no)
        .bind(state)
        .bind(title)
        .execute(&f.pool)
        .await
        .unwrap();
    if state == "PUBLISHED" {
        sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
            .bind(version_id.as_uuid())
            .bind(f.document_id.as_uuid())
            .execute(&f.pool)
            .await
            .unwrap();
    }
    version_id
}

async fn insert_published_document(
    f: &support::Fixture,
    folder_id: FolderId,
    title: &str,
    metadata: serde_json::Value,
    created_at: OffsetDateTime,
    published_at: OffsetDateTime,
) -> Uuid {
    let document_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,1,$3,$4)")
        .bind(document_id).bind(folder_id.as_uuid()).bind(metadata).bind(created_at)
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED',$3,$4,'test-idp','policy-admin','{}',$5)")
        .bind(version_id).bind(document_id).bind(title).bind(published_at).bind(created_at)
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id)
        .bind(document_id)
        .execute(&f.pool)
        .await
        .unwrap();
    document_id
}

#[tokio::test]
async fn each_scope_uses_its_visible_title_and_unread_is_per_version() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant([Action::Read, Action::ReadHistory, Action::Write])],
        )
        .await
        .unwrap();
    let current = add_version(&f, 1, "Public % title", "PUBLISHED").await;
    add_version(&f, 2, "Secret working title", "WORKING").await;
    let service = DocumentQueryService::new(f.repository.clone());
    let published = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    title_contains: Some("%".into()),
                    ..Default::default()
                },
                sort: DocumentSort::CreatedAtDesc,
                page_size: None,
                cursor: None,
                unread_only: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(published.items.len(), 1);
    assert_eq!(published.items[0].document_version_id, current);
    assert_eq!(published.items[0].title, "Public % title");
    assert!(published.items[0].first_read_at.is_none());
    let hidden = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    title_contains: Some("Secret".into()),
                    ..Default::default()
                },
                sort: DocumentSort::CreatedAtDesc,
                page_size: None,
                cursor: None,
                unread_only: false,
            },
        )
        .await
        .unwrap();
    assert!(hidden.items.is_empty());
    let authoring = service
        .list_authoring_documents(&context(), AuthoringQuery::default())
        .await
        .unwrap();
    assert_eq!(authoring.items[0].title, "Secret working title");
    let history = service
        .list_history_documents(&context(), HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(history.items[0].title, "Secret working title");

    ReadStateService::new(f.repository.clone())
        .mark_version_read(
            &context(),
            MarkVersionRead {
                document_id: f.document_id,
                document_version_id: current,
            },
        )
        .await
        .unwrap();
    let unread = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                unread_only: true,
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert!(unread.items.is_empty());
}

#[tokio::test]
async fn cursor_binds_access_epoch_and_page_size_is_enforced() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read])])
        .await
        .unwrap();
    add_version(&f, 1, "First", "PUBLISHED").await;
    let service = DocumentQueryService::new(f.repository.clone());
    assert!(matches!(
        service
            .list_published_documents(
                &context(),
                PublishedQuery {
                    page_size: Some(201),
                    ..PublishedQuery::default()
                }
            )
            .await,
        Err(ApplicationError::Validation(_))
    ));
    let first = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                page_size: Some(1),
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(first.items.len(), 1);
    assert!(first.next_cursor.is_none());
}

#[tokio::test]
async fn authorization_precedes_limit_and_cursor_rechecks_query_and_access_epoch() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read])])
        .await
        .unwrap();
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) \
         SELECT gen_random_uuid(),$1,NULL,1,'{}',to_timestamp(100) FROM generate_series(1,53) \
         RETURNING document_id",
    )
    .bind(f.root_id.as_uuid())
    .fetch_all(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) SELECT gen_random_uuid(),d.document_id,1,'PUBLISHED','Bulk',now(),'test-idp','policy-admin','{}',now() FROM documents d WHERE d.created_at = to_timestamp(100)")
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents d SET current_version_id = v.document_version_id FROM document_versions v WHERE v.document_id = d.document_id AND d.created_at = to_timestamp(100)")
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO access_policy_bindings (policy_id,document_id,mode,revision,created_at,updated_at) VALUES ($1,$2,'EXPLICIT',1,now(),now())")
        .bind(Uuid::now_v7())
        .bind(ids[0])
        .execute(&f.pool).await.unwrap();
    let service = DocumentQueryService::new(f.repository.clone());
    let first = service
        .list_published_documents(&context(), PublishedQuery::default())
        .await
        .unwrap();
    assert_eq!(first.items.len(), 50);
    assert!(
        first
            .items
            .iter()
            .all(|item| item.document_id.as_uuid() != ids[0])
    );
    let cursor = first
        .next_cursor
        .clone()
        .expect("52 authorized rows have a second page");
    let second = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                cursor: Some(cursor.clone()),
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(second.items.len(), 2);
    assert!(second.next_cursor.is_none());
    let mut all = first
        .items
        .iter()
        .chain(second.items.iter())
        .map(|item| item.document_id.as_uuid())
        .collect::<Vec<_>>();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 52);
    assert!(matches!(
        service
            .list_published_documents(
                &context(),
                PublishedQuery {
                    cursor: Some(cursor.clone()),
                    filter: DocumentListFilter {
                        title_contains: Some("Bulk".into()),
                        ..Default::default()
                    },
                    ..PublishedQuery::default()
                }
            )
            .await,
        Err(ApplicationError::CursorStale)
    ));
    let other = VerifiedActorContext::from_trusted_adapter(
        PrincipalRef::new("other-idp", "policy-admin").unwrap(),
        vec![
            PolicySubject::new(PolicySubjectKind::Principal, "other-idp", "policy-admin").unwrap(),
        ],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap();
    assert!(matches!(
        service
            .list_published_documents(
                &other,
                PublishedQuery {
                    cursor: Some(cursor.clone()),
                    ..PublishedQuery::default()
                }
            )
            .await,
        Err(ApplicationError::CursorStale)
    ));
    sqlx::query(
        "UPDATE document_access_state SET access_revision = access_revision + 1 WHERE id = 1",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(matches!(
        service
            .list_published_documents(
                &context(),
                PublishedQuery {
                    cursor: Some(cursor),
                    ..PublishedQuery::default()
                }
            )
            .await,
        Err(ApplicationError::CursorStale)
    ));
}

#[tokio::test]
async fn child_folder_list_and_document_summary_hide_unreadable_folder_name() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read])])
        .await
        .unwrap();
    let hidden_folder = FolderId::from_uuid(Uuid::now_v7());
    let visible_folder = FolderId::from_uuid(Uuid::now_v7());
    for (id, name) in [(hidden_folder, "Hidden"), (visible_folder, "Visible")] {
        sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$2,$3,'ACTIVE',0,now())")
            .bind(id.as_uuid()).bind(f.root_id.as_uuid()).bind(name)
            .execute(&f.pool).await.unwrap();
    }
    sqlx::query("INSERT INTO access_policy_bindings (policy_id,folder_id,mode,revision,created_at,updated_at) VALUES ($1,$2,'EXPLICIT',1,now(),now())")
        .bind(Uuid::now_v7()).bind(hidden_folder.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET folder_id = $1 WHERE document_id = $2")
        .bind(hidden_folder.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO access_policy_bindings (policy_id,document_id,mode,revision,created_at,updated_at) VALUES ($1,$2,'EXPLICIT',1,now(),now())")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let policy_id: Uuid =
        sqlx::query_scalar("SELECT policy_id FROM access_policy_bindings WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO access_policy_grants (policy_id,subject_kind,identity_provider,subject_id,action) VALUES ($1,'principal','test-idp','policy-admin','read')")
        .bind(policy_id).execute(&f.pool).await.unwrap();
    add_version(&f, 1, "Visible document", "PUBLISHED").await;
    let service = DocumentQueryService::new(f.repository.clone());
    let folders = service
        .list_child_folders(
            &context(),
            FolderPageQuery {
                parent_folder_id: f.root_id,
                page_size: None,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(folders.items.len(), 1);
    assert_eq!(folders.items[0].name, "Visible");
    let documents = service
        .list_published_documents(&context(), PublishedQuery::default())
        .await
        .unwrap();
    assert_eq!(documents.items.len(), 1);
    assert_eq!(documents.items[0].folder_id, None);
    assert_eq!(documents.items[0].folder_name, None);
    assert!(matches!(
        service
            .list_child_folders(
                &context(),
                FolderPageQuery {
                    parent_folder_id: hidden_folder,
                    page_size: None,
                    cursor: None,
                }
            )
            .await,
        Err(ApplicationError::FolderNotFound)
    ));
}

#[tokio::test]
async fn literal_nfc_filters_folder_scope_and_both_extra_sorts_are_stable() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read])])
        .await
        .unwrap();
    let child = FolderId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$2,'Child','ACTIVE',0,now())")
        .bind(child.as_uuid()).bind(f.root_id.as_uuid()).execute(&f.pool).await.unwrap();
    let earlier = OffsetDateTime::from_unix_timestamp(100).unwrap();
    let later = OffsetDateTime::from_unix_timestamp(200).unwrap();
    let percent_id = insert_published_document(
        &f,
        f.root_id,
        "Café %",
        serde_json::json!({"document_type":"report"}),
        earlier,
        later,
    )
    .await;
    let underscore_id = insert_published_document(
        &f,
        child,
        "Cafe\u{301} _",
        serde_json::json!({"document_type":"memo"}),
        later,
        earlier,
    )
    .await;
    let service = DocumentQueryService::new(f.repository.clone());
    let percent = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    title_contains: Some(" % ".into()),
                    ..Default::default()
                },
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(percent.items.len(), 1);
    assert_eq!(percent.items[0].document_id.as_uuid(), percent_id);
    let underscore = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    title_contains: Some("_".into()),
                    ..Default::default()
                },
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(underscore.items.len(), 1);
    assert_eq!(underscore.items[0].document_id.as_uuid(), underscore_id);
    let normalized = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    title_contains: Some(" Café ".into()),
                    ..Default::default()
                },
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(normalized.items.len(), 2);
    let direct = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    folder_id: Some(f.root_id),
                    ..Default::default()
                },
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(direct.items.len(), 1);
    let descendants = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    folder_id: Some(f.root_id),
                    include_descendants: true,
                    ..Default::default()
                },
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(descendants.items.len(), 2);
    let metadata = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    document_type: Some("report".into()),
                    ..Default::default()
                },
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(metadata.items[0].document_id.as_uuid(), percent_id);
    assert_eq!(metadata.items.len(), 1);
    let date = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                filter: DocumentListFilter {
                    created_from: Some(later),
                    created_before: Some(OffsetDateTime::from_unix_timestamp(201).unwrap()),
                    ..Default::default()
                },
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(date.items.len(), 1);
    assert_eq!(date.items[0].document_id.as_uuid(), underscore_id);
    let title_first = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                sort: DocumentSort::TitleAsc,
                page_size: Some(1),
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(title_first.items[0].document_id.as_uuid(), percent_id);
    let title_second = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                sort: DocumentSort::TitleAsc,
                page_size: Some(1),
                cursor: title_first.next_cursor,
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(title_second.items[0].document_id.as_uuid(), underscore_id);
    let published_first = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                sort: DocumentSort::PublishedAtDesc,
                page_size: Some(1),
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(published_first.items[0].document_id.as_uuid(), percent_id);
    let published_second = service
        .list_published_documents(
            &context(),
            PublishedQuery {
                sort: DocumentSort::PublishedAtDesc,
                page_size: Some(1),
                cursor: published_first.next_cursor,
                ..PublishedQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        published_second.items[0].document_id.as_uuid(),
        underscore_id
    );
}

#[tokio::test]
async fn initial_working_and_publication_end_stay_in_their_typed_scopes() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant([Action::Read, Action::Write, Action::ReadHistory])],
        )
        .await
        .unwrap();
    let working = add_version(&f, 1, "Initial working", "WORKING").await;
    let service = DocumentQueryService::new(f.repository.clone());
    assert!(
        service
            .list_published_documents(&context(), PublishedQuery::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let authoring = service
        .list_authoring_documents(&context(), AuthoringQuery::default())
        .await
        .unwrap();
    assert_eq!(authoring.items[0].document_version_id, working);
    assert_eq!(authoring.items[0].lifecycle_state, "WORKING");
    sqlx::query("UPDATE document_versions SET lifecycle_state = 'PUBLISHED', published_at = now() WHERE document_version_id = $1")
        .bind(working.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1, revision = revision + 1 WHERE document_id = $2")
        .bind(working.as_uuid()).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,$4,$5,'test-idp','policy-admin','end',$5,$6,now())")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(vec![0_u8;32])
        .bind(revision).bind(working.as_uuid()).bind(revision + 1)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = NULL, revision = revision + 1 WHERE document_id = $1")
        .bind(f.document_id.as_uuid()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let audits_before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert!(
        service
            .list_published_documents(&context(), PublishedQuery::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        service
            .list_authoring_documents(&context(), AuthoringQuery::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let history = service
        .list_history_documents(&context(), HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(history.items.len(), 1);
    assert!(history.items[0].ended);
    let audits_after: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(audits_after, audits_before);
}
