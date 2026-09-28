#[path = "support/management.rs"]
mod support;

use std::time::Instant;

use document_application::{
    BootstrapRootPolicy, DocumentManagementService, DocumentQueryService, ManagementCommand,
    ManagementOperationId, MarkVersionRead, PublishedQuery, ReadStateService,
};
use document_domain::{
    Action, DocumentVersionId, FolderId, PolicyGrant, PolicySubject, PolicySubjectKind,
};
use support::{context, fixture};
use uuid::Uuid;

#[tokio::test]
#[ignore = "run once for the MB-11 synthetic capacity baseline"]
async fn synthetic_thousand_principals_ten_thousand_documents_and_deep_folders() {
    let f = fixture().await;
    let grant = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        [
            Action::Read,
            Action::ReadHistory,
            Action::Write,
            Action::Administer,
        ],
    )
    .unwrap();
    f.repository
        .initialize_root_policy(&context(), vec![grant])
        .await
        .unwrap();
    let root_policy: Uuid =
        sqlx::query_scalar("SELECT policy_id FROM access_policy_bindings WHERE folder_id = $1")
            .bind(f.root_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO access_policy_grants (policy_id,subject_kind,identity_provider,subject_id,action) SELECT $1,'principal','test-idp','synthetic-' || n,'read' FROM generate_series(1,999) n")
        .bind(root_policy).execute(&f.pool).await.unwrap();

    for level in 0..10 {
        let start = level * 100 + 1;
        let end = ((level + 1) * 100).min(999);
        sqlx::query("INSERT INTO folders (folder_id,parent_folder_id,name,status,revision,created_at) SELECT md5('load-folder-' || n)::uuid, CASE WHEN n <= 100 THEN $1 ELSE md5('load-folder-' || (n - 100))::uuid END, 'Synthetic ' || n,'ACTIVE',0,now() FROM generate_series($2::int,$3::int) n")
            .bind(f.root_id.as_uuid()).bind(start).bind(end)
            .execute(&f.pool).await.unwrap();
    }
    let leaf = FolderId::from_uuid(
        sqlx::query_scalar("SELECT md5('load-folder-999')::uuid")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
    );
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) SELECT gen_random_uuid(),$1,NULL,1,'{}',now() FROM generate_series(1,9999)")
        .bind(leaf.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) SELECT gen_random_uuid(),d.document_id,1,'PUBLISHED','Synthetic document',now(),'test-idp','policy-admin','{}',now() FROM documents d WHERE d.folder_id = $1")
        .bind(leaf.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents d SET current_version_id = v.document_version_id FROM document_versions v WHERE v.document_id = d.document_id AND d.folder_id = $1")
        .bind(leaf.as_uuid()).execute(&f.pool).await.unwrap();
    let counts: (i64, i64, i64, i32) = sqlx::query_as("SELECT (SELECT count(DISTINCT (identity_provider,subject_id)) FROM access_policy_grants WHERE policy_id=$1), (SELECT count(*) FROM documents), (SELECT count(*) FROM folders), (WITH RECURSIVE tree AS (SELECT folder_id,0 AS depth FROM folders WHERE parent_folder_id IS NULL UNION ALL SELECT child.folder_id,tree.depth+1 FROM folders child JOIN tree ON child.parent_folder_id=tree.folder_id) SELECT max(depth) FROM tree)")
        .bind(root_policy).fetch_one(&f.pool).await.unwrap();
    assert_eq!(counts, (1_000, 10_000, 1_000, 10));

    let sample_id: Uuid =
        sqlx::query_scalar("SELECT document_id FROM documents WHERE folder_id = $1 LIMIT 1")
            .bind(leaf.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let plan: serde_json::Value = sqlx::query_scalar("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) SELECT dmb_allows_document($1::uuid, $2::jsonb, ARRAY['read']::text[])")
        .bind(sample_id)
        .bind(serde_json::json!([{"kind":"principal","identity_provider":"test-idp","subject_id":"policy-admin"}]))
        .fetch_one(&f.pool).await.unwrap();
    let subjects = serde_json::json!([{"kind":"principal","identity_provider":"test-idp","subject_id":"policy-admin"}]);
    let list_plan: serde_json::Value = sqlx::query_scalar("EXPLAIN (FORMAT JSON) SELECT d.document_id FROM documents d JOIN document_versions v ON v.document_version_id=d.current_version_id WHERE dmb_allows_document(d.document_id,$1::jsonb,ARRAY['read']::text[]) ORDER BY d.created_at DESC,d.document_id DESC LIMIT 50")
        .bind(subjects).fetch_one(&f.pool).await.unwrap();
    let move_plan: serde_json::Value = sqlx::query_scalar(
        "EXPLAIN (FORMAT JSON) UPDATE documents SET folder_id=$1 WHERE document_id=$2",
    )
    .bind(leaf.as_uuid())
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let read_plan: serde_json::Value = sqlx::query_scalar("EXPLAIN (FORMAT JSON) SELECT current_version_id FROM documents WHERE document_id=$1 FOR UPDATE")
        .bind(f.document_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    let list_start = Instant::now();
    let listed = DocumentQueryService::new(f.repository.clone())
        .list_published_documents(&context(), PublishedQuery::default())
        .await
        .unwrap();
    let list_elapsed = list_start.elapsed();
    assert_eq!(listed.items.len(), 50);

    let own_version = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Own sample',now(),'test-idp','policy-admin','{}',now())")
        .bind(own_version.as_uuid()).bind(f.document_id.as_uuid())
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(own_version.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let move_start = Instant::now();
    DocumentManagementService::new(f.repository.clone())
        .move_document(
            &context(),
            ManagementCommand::MoveDocument {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                document_id: f.document_id,
                from_folder_id: f.root_id,
                to_folder_id: leaf,
                expected_document_revision: 1,
                reason: "synthetic load move".into(),
            },
        )
        .await
        .unwrap();
    let move_elapsed = move_start.elapsed();
    let read_start = Instant::now();
    ReadStateService::new(f.repository.clone())
        .mark_version_read(
            &context(),
            MarkVersionRead {
                document_id: f.document_id,
                document_version_id: own_version,
            },
        )
        .await
        .unwrap();
    let read_elapsed = read_start.elapsed();
    println!(
        "MB11 synthetic baseline: principals=1000 documents=10000 folders=1000 max_depth=10 list_ms={} move_ms={} read_ms={} inheritance_plan_node={} inheritance_plan_execution_ms={} list_plan_node={} move_plan_node={} read_plan_node={}",
        list_elapsed.as_millis(),
        move_elapsed.as_millis(),
        read_elapsed.as_millis(),
        plan[0]["Plan"]["Node Type"],
        plan[0]["Execution Time"],
        list_plan[0]["Plan"]["Node Type"],
        move_plan[0]["Plan"]["Node Type"],
        read_plan[0]["Plan"]["Node Type"]
    );
}
