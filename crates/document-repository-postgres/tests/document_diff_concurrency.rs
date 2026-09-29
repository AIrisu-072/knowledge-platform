#[path = "support/management.rs"]
mod support;

use document_application::{
    BootstrapRootPolicy, RepositoryError,
    document_diff::{DiffRequest, DiffResult, DocumentDiffRepository},
};
use document_diff_core::{
    ContentVerdict, DiffCoverage, DiffProfileVersion, ResourceProfileVersion,
};
use document_domain::{Action, DocumentVersionId, PolicyGrant, PolicySubject, PolicySubjectKind};
use support::{context, fixture};
use uuid::Uuid;

fn grant() -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        [Action::Read, Action::ReadHistory, Action::Write],
    )
    .unwrap()
}

async fn seed_version(
    pool: &sqlx::PgPool,
    document: Uuid,
    number: i64,
    state: &str,
) -> DocumentVersionId {
    let version = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,$3,$4,$5,CASE WHEN $4 = 'PUBLISHED' THEN now() ELSE NULL END,'test-idp','policy-admin','{}',now())")
        .bind(version.as_uuid()).bind(document).bind(number).bind(state)
        .bind(format!("Version {number}")).execute(pool).await.unwrap();
    let file = Uuid::now_v7();
    let item = Uuid::now_v7();
    let representation = Uuid::now_v7();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',13,$3,now())")
        .bind(file).bind(vec![7_u8;32]).bind(format!("objects/{file}/source"))
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(item).bind(version.as_uuid()).bind(representation).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename,detected_format,inspection_profile_version) VALUES ($1,$2,$3,'AUTHORITATIVE','source.txt','txt','dsi-v0')")
        .bind(representation).bind(item).bind(file).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    version
}

fn result(pair: &document_application::document_diff::DiffPairSnapshot) -> DiffResult {
    DiffResult {
        document_id: pair.document_id,
        base_version_id: pair.base.version_id,
        target_version_id: pair.target.version_id,
        base_snapshot_digest: pair.base.snapshot_digest(),
        target_snapshot_digest: pair.target.snapshot_digest(),
        profile: DiffProfileVersion::V0,
        resource_profile: ResourceProfileVersion::V0,
        verdict: ContentVerdict::Same,
        coverage: DiffCoverage::Full,
        changes: vec![],
        unverified_regions: vec![],
        ancillary_changes: vec![],
    }
}

#[tokio::test]
async fn concurrent_working_revision_update_wins_before_final_audit_and_disclosure() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant()])
        .await
        .unwrap();
    let published = seed_version(&f.pool, f.document_id.as_uuid(), 1, "PUBLISHED").await;
    let working = seed_version(&f.pool, f.document_id.as_uuid(), 2, "WORKING").await;
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(published.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let pair = f
        .repository
        .capture_pair(
            &context(),
            DiffRequest {
                document_id: f.document_id,
                base_version_id: published,
                target_version_id: working,
                profile: DiffProfileVersion::V0,
            },
        )
        .await
        .unwrap();

    let mut update = f.pool.begin().await.unwrap();
    sqlx::query("SELECT revision FROM documents WHERE document_id = $1 FOR UPDATE")
        .bind(f.document_id.as_uuid())
        .fetch_one(&mut *update)
        .await
        .unwrap();
    let repository = f.repository.clone();
    let expected = result(&pair);
    let finalizer = tokio::spawn(async move {
        repository
            .authorize_and_audit_result(&context(), &pair, &expected, false, None)
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !finalizer.is_finished(),
        "final disclosure must wait for the document row lock"
    );
    sqlx::query("UPDATE documents SET revision = revision + 1 WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&mut *update)
        .await
        .unwrap();
    update.commit().await.unwrap();
    assert_eq!(
        finalizer.await.unwrap(),
        Err(RepositoryError::StaleComparisonInput)
    );
    let disclosed: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type = 'document.diff.result_access_granted'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(disclosed, 0);
}
