#[path = "support/management.rs"]
mod support;

use document_application::{
    ApplicationError, BootstrapRootPolicy, FolderService, ManagementCommand, ManagementOperationId,
};
use document_domain::{Action, FolderId, PolicyGrant, PolicySubject, PolicySubjectKind};
use document_repository_postgres::{FolderPreflightCategory, preflight_folder_names};
use support::{Fixture, context, fixture};
use uuid::Uuid;

fn operation_id() -> ManagementOperationId {
    ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap()
}

fn create(
    folder_id: FolderId,
    parent_folder_id: FolderId,
    expected_parent_revision: i64,
    name: &str,
) -> ManagementCommand {
    ManagementCommand::CreateFolder {
        operation_id: operation_id(),
        folder_id,
        parent_folder_id,
        expected_parent_revision,
        name: name.into(),
        reason: "create folder".into(),
    }
}

fn rename(folder_id: FolderId, expected_folder_revision: i64, name: &str) -> ManagementCommand {
    ManagementCommand::RenameFolder {
        operation_id: operation_id(),
        folder_id,
        expected_folder_revision,
        name: name.into(),
        reason: "rename folder".into(),
    }
}

async fn allow(f: &Fixture) {
    let grant = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        [Action::Administer, Action::Read],
    )
    .unwrap();
    f.repository
        .initialize_root_policy(&context(), vec![grant])
        .await
        .unwrap();
}

async fn folder(f: &Fixture, folder_id: FolderId) -> (Option<Uuid>, String, String, i64) {
    sqlx::query_as("SELECT parent_folder_id,name,status,revision FROM folders WHERE folder_id = $1")
        .bind(folder_id.as_uuid())
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

#[tokio::test]
async fn create_normalizes_name_and_rename_has_revisioned_noop() {
    let f = fixture().await;
    allow(&f).await;
    let service = FolderService::new(f.repository.clone());
    let folder_id = FolderId::from_uuid(Uuid::now_v7());
    let command = create(folder_id, f.root_id, 0, "  a\u{308}  ");
    let created = service
        .create_folder(&context(), command.clone())
        .await
        .unwrap();
    assert!(created.changed);
    assert_eq!(created.resulting_revision, 0);
    assert_eq!(
        folder(&f, folder_id).await,
        (Some(f.root_id.as_uuid()), "ä".into(), "ACTIVE".into(), 0)
    );
    assert_eq!(
        created,
        service.create_folder(&context(), command).await.unwrap()
    );
    let unchanged = service
        .rename_folder(&context(), rename(folder_id, 0, " ä "))
        .await
        .unwrap();
    assert!(!unchanged.changed);
    assert_eq!(folder(&f, folder_id).await.3, 0);
    let renamed = service
        .rename_folder(&context(), rename(folder_id, 0, "Reports"))
        .await
        .unwrap();
    assert!(renamed.changed);
    assert_eq!(renamed.resulting_revision, 1);
    assert_eq!(folder(&f, folder_id).await.1, "Reports");
    assert_eq!(event_count(&f, "FolderCreated").await, 1);
    assert_eq!(event_count(&f, "FolderRenamed").await, 1);
    let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type IN ('folder.created','folder.renamed')")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(audit_count, 2);
    assert_eq!(
        f.root_id.as_uuid(),
        document_repository_postgres::SYSTEM_ROOT_FOLDER_ID
    );
}

#[tokio::test]
async fn invalid_parent_root_and_stale_revision_do_not_mutate() {
    let f = fixture().await;
    allow(&f).await;
    let service = FolderService::new(f.repository.clone());
    let unknown = FolderId::from_uuid(Uuid::now_v7());
    assert!(matches!(
        service
            .create_folder(
                &context(),
                create(FolderId::from_uuid(Uuid::now_v7()), unknown, 0, "Child")
            )
            .await,
        Err(ApplicationError::FolderNotFound)
    ));
    assert!(matches!(
        service
            .rename_folder(&context(), rename(f.root_id, 0, "Other Root"))
            .await,
        Err(ApplicationError::BusinessRule)
    ));
    assert!(matches!(
        service
            .create_folder(
                &context(),
                create(FolderId::from_uuid(Uuid::now_v7()), f.root_id, 1, "Child")
            )
            .await,
        Err(ApplicationError::Conflict)
    ));
    let root = folder(&f, f.root_id).await;
    assert_eq!(root.0, None);
    assert_eq!(root.1, "System Root");
    assert_eq!(event_count(&f, "FolderCreated").await, 0);
}

#[tokio::test]
async fn normalized_collision_is_atomic_and_case_remains_distinct() {
    let f = fixture().await;
    allow(&f).await;
    let service = FolderService::new(f.repository.clone());
    service
        .create_folder(
            &context(),
            create(FolderId::from_uuid(Uuid::now_v7()), f.root_id, 0, "Ä"),
        )
        .await
        .unwrap();
    assert!(matches!(
        service
            .create_folder(
                &context(),
                create(
                    FolderId::from_uuid(Uuid::now_v7()),
                    f.root_id,
                    0,
                    "A\u{308}"
                )
            )
            .await,
        Err(ApplicationError::Conflict)
    ));
    service
        .create_folder(
            &context(),
            create(FolderId::from_uuid(Uuid::now_v7()), f.root_id, 0, "ä"),
        )
        .await
        .unwrap();
    assert_eq!(event_count(&f, "FolderCreated").await, 2);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM folders WHERE parent_folder_id = $1")
        .bind(f.root_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn same_name_concurrent_creation_has_one_success_and_one_event() {
    let f = fixture().await;
    allow(&f).await;
    let service = FolderService::new(f.repository.clone());
    let ctx = context();
    let (first, second) = tokio::join!(
        service.create_folder(
            &ctx,
            create(FolderId::from_uuid(Uuid::now_v7()), f.root_id, 0, "Same")
        ),
        service.create_folder(
            &ctx,
            create(FolderId::from_uuid(Uuid::now_v7()), f.root_id, 0, "Same")
        ),
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert_eq!(event_count(&f, "FolderCreated").await, 1);
}

#[tokio::test]
async fn mandatory_audit_failure_rolls_back_create() {
    let f = fixture().await;
    allow(&f).await;
    sqlx::query("CREATE FUNCTION reject_folder_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type = 'folder.created' THEN RAISE EXCEPTION 'audit blocked'; END IF; RETURN NEW; END; $$")
        .execute(&f.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_folder_audit BEFORE INSERT ON audit_outbox_events FOR EACH ROW EXECUTE FUNCTION reject_folder_audit()")
        .execute(&f.pool).await.unwrap();
    let id = FolderId::from_uuid(Uuid::now_v7());
    assert!(
        FolderService::new(f.repository.clone())
            .create_folder(&context(), create(id, f.root_id, 0, "Rollback"))
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM folders WHERE folder_id = $1")
        .bind(id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(event_count(&f, "FolderCreated").await, 0);
}

#[tokio::test]
async fn preflight_reports_only_ids_and_classifications_then_migration_stops() {
    let f = fixture().await;
    assert!(preflight_folder_names(&f.pool).await.unwrap().is_clean());
    sqlx::query("ALTER TABLE folders DROP CONSTRAINT ck_folders_canonical_name_v0")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DROP INDEX uq_folders_parent_canonical_name_v0")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DROP INDEX uq_folders_single_root_v0")
        .execute(&f.pool)
        .await
        .unwrap();
    let first = Uuid::now_v7();
    let second = Uuid::now_v7();
    let invalid = Uuid::now_v7();
    let extra_root = Uuid::now_v7();
    let cycle_a = Uuid::now_v7();
    let cycle_b = Uuid::now_v7();
    let orphan = Uuid::now_v7();
    for (id, parent, name) in [
        (first, Some(f.root_id.as_uuid()), "Ä"),
        (second, Some(f.root_id.as_uuid()), "A\u{308}"),
        (invalid, Some(f.root_id.as_uuid()), "bad/name"),
        (extra_root, None, "Extra Root"),
        (cycle_a, Some(f.root_id.as_uuid()), "Cycle A"),
        (cycle_b, Some(f.root_id.as_uuid()), "Cycle B"),
    ] {
        sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$2,$3,'ACTIVE',0,now())")
            .bind(id).bind(parent).bind(name).execute(&f.pool).await.unwrap();
    }
    sqlx::query("UPDATE folders SET parent_folder_id = $1 WHERE folder_id = $2")
        .bind(cycle_b)
        .bind(cycle_a)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE folders SET parent_folder_id = $1 WHERE folder_id = $2")
        .bind(cycle_a)
        .bind(cycle_b)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE folders DISABLE TRIGGER ALL")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) VALUES ($1,$2,'Orphan','ACTIVE',0,now())")
        .bind(orphan)
        .bind(Uuid::now_v7())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE folders ENABLE TRIGGER ALL")
        .execute(&f.pool)
        .await
        .unwrap();
    let report = preflight_folder_names(&f.pool).await.unwrap();
    let has = |id, category| {
        report
            .findings
            .iter()
            .any(|finding| finding.folder_id == id && finding.category == category)
    };
    assert!(has(first, FolderPreflightCategory::NormalizationCollision));
    assert!(has(second, FolderPreflightCategory::NormalizationCollision));
    assert!(has(second, FolderPreflightCategory::NonNormalizedName));
    assert!(has(invalid, FolderPreflightCategory::InvalidName));
    assert!(has(extra_root, FolderPreflightCategory::MultipleRoots));
    assert!(has(extra_root, FolderPreflightCategory::UnexpectedRoot));
    assert!(has(cycle_a, FolderPreflightCategory::Cycle));
    assert!(has(orphan, FolderPreflightCategory::MissingParent));
    let visible = format!("{report:?}");
    assert!(!visible.contains("bad/name"));
    assert!(!visible.contains("Extra Root"));
    let migration_error = document_repository_postgres::migrate(&f.pool)
        .await
        .unwrap_err()
        .to_string();
    assert!(migration_error.contains(&invalid.to_string()));
    assert!(!migration_error.contains("bad/name"));
    let sql = include_str!("../migrations/0007_document_folder_names_v0.sql");
    assert!(
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&f.pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn folder_revision_overflow_does_not_mutate() {
    let f = fixture().await;
    allow(&f).await;
    let id = FolderId::from_uuid(Uuid::now_v7());
    let service = FolderService::new(f.repository.clone());
    service
        .create_folder(&context(), create(id, f.root_id, 0, "Before"))
        .await
        .unwrap();
    sqlx::query("UPDATE folders SET revision = $1 WHERE folder_id = $2")
        .bind(i64::MAX)
        .bind(id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(matches!(
        service
            .rename_folder(&context(), rename(id, i64::MAX, "After"))
            .await,
        Err(ApplicationError::BusinessRule)
    ));
    assert_eq!(folder(&f, id).await.1, "Before");
    assert_eq!(event_count(&f, "FolderRenamed").await, 0);
}
