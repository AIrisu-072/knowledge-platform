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

fn grant(actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        actions,
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
        .bind(file).bind(vec![7_u8;32]).bind(format!("objects/{file}/secret-source"))
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(item).bind(version.as_uuid()).bind(representation).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename,detected_format,inspection_profile_version) VALUES ($1,$2,$3,'AUTHORITATIVE','secret-source.txt','txt','dsi-v0')")
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

fn request(
    document: document_domain::DocumentId,
    base: DocumentVersionId,
    target: DocumentVersionId,
) -> DiffRequest {
    DiffRequest {
        document_id: document,
        base_version_id: base,
        target_version_id: target,
        profile: DiffProfileVersion::V0,
    }
}

#[tokio::test]
async fn cache_hit_reauthorizes_and_writes_minimal_audit_before_disclosure() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let a = seed_version(&f.pool, f.document_id.as_uuid(), 1, "PUBLISHED").await;
    let b = seed_version(&f.pool, f.document_id.as_uuid(), 2, "PUBLISHED").await;
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(b.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let pair = f
        .repository
        .capture_pair(&context(), request(f.document_id, a, b))
        .await
        .unwrap();
    let result = result(&pair);
    let id = f
        .repository
        .authorize_and_audit_result(&context(), &pair, &result, true, Some("diff-test"))
        .await
        .unwrap();
    let row: (String, serde_json::Value) =
        sqlx::query_as("SELECT event_type,data FROM audit_outbox_events WHERE event_id = $1")
            .bind(id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(row.0, "document.diff.result_access_granted");
    assert_eq!(row.1["cache_hit"], true);
    let audit = row.1.to_string();
    assert!(!audit.contains("secret-source"));
    assert!(!audit.contains("storage_locator"));
    let policy: Uuid =
        sqlx::query_scalar("SELECT policy_id FROM access_policy_bindings WHERE folder_id = $1")
            .bind(f.root_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    sqlx::query(
        "DELETE FROM access_policy_grants WHERE policy_id = $1 AND action = 'read_history'",
    )
    .bind(policy)
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        f.repository
            .authorize_and_audit_result(&context(), &pair, &result, true, None)
            .await
            .unwrap_err(),
        RepositoryError::Forbidden
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type = 'document.diff.result_access_granted'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn working_mutation_and_t10_invalidate_old_pair() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant([Action::Read, Action::ReadHistory, Action::Write])],
        )
        .await
        .unwrap();
    let a = seed_version(&f.pool, f.document_id.as_uuid(), 1, "PUBLISHED").await;
    let working = seed_version(&f.pool, f.document_id.as_uuid(), 2, "WORKING").await;
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(a.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let pair = f
        .repository
        .capture_pair(&context(), request(f.document_id, a, working))
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET revision = revision + 1 WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        f.repository
            .authorize_and_audit_result(&context(), &pair, &result(&pair), false, None)
            .await
            .unwrap_err(),
        RepositoryError::StaleComparisonInput
    );
    let pair = f
        .repository
        .capture_pair(&context(), request(f.document_id, a, working))
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,2,$4,'test-idp','policy-admin','end',$4,3,now())")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(vec![0_u8;32]).bind(a.as_uuid())
        .execute(&f.pool).await.unwrap();
    sqlx::query(
        "UPDATE documents SET current_version_id = NULL, revision = 3 WHERE document_id = $1",
    )
    .bind(f.document_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(
        f.repository
            .authorize_and_audit_result(&context(), &pair, &result(&pair), false, None)
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type = 'document.diff.result_access_granted'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn expired_actor_and_audit_insert_failure_do_not_disclose() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let a = seed_version(&f.pool, f.document_id.as_uuid(), 1, "PUBLISHED").await;
    let b = seed_version(&f.pool, f.document_id.as_uuid(), 2, "PUBLISHED").await;
    let pair = f
        .repository
        .capture_pair(&context(), request(f.document_id, a, b))
        .await
        .unwrap();
    let short = document_application::VerifiedActorContext::from_trusted_adapter(
        support::actor(),
        context().subjects().to_vec(),
        time::OffsetDateTime::now_utc() + time::Duration::milliseconds(30),
        document_application::InvocationKind::HumanInteractive,
        None,
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        f.repository
            .authorize_and_audit_result(&short, &pair, &result(&pair), false, None)
            .await
            .unwrap_err(),
        RepositoryError::Forbidden
    );
    sqlx::query("CREATE FUNCTION reject_diff_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type = 'document.diff.result_access_granted' THEN RAISE EXCEPTION 'blocked'; END IF; RETURN NEW; END; $$")
        .execute(&f.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_diff_audit BEFORE INSERT ON audit_outbox_events FOR EACH ROW EXECUTE FUNCTION reject_diff_audit()")
        .execute(&f.pool).await.unwrap();
    assert!(
        f.repository
            .authorize_and_audit_result(&context(), &pair, &result(&pair), false, None)
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type = 'document.diff.result_access_granted'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn publication_end_invalidates_pair_even_when_both_versions_remain_readable_as_history() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let a = seed_version(&f.pool, f.document_id.as_uuid(), 1, "PUBLISHED").await;
    let b = seed_version(&f.pool, f.document_id.as_uuid(), 2, "PUBLISHED").await;
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(b.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let pair = f
        .repository
        .capture_pair(&context(), request(f.document_id, a, b))
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,1,$4,'test-idp','policy-admin','end',$4,2,now())")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(vec![0_u8;32]).bind(b.as_uuid())
        .execute(&f.pool).await.unwrap();
    sqlx::query(
        "UPDATE documents SET current_version_id = NULL, revision = 2 WHERE document_id = $1",
    )
    .bind(f.document_id.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(
        f.repository
            .capture_pair(&context(), request(f.document_id, a, b))
            .await
            .is_ok()
    );
    assert_eq!(
        f.repository
            .authorize_and_audit_result(&context(), &pair, &result(&pair), false, None)
            .await
            .unwrap_err(),
        RepositoryError::StaleComparisonInput
    );
}

#[tokio::test]
async fn cached_result_and_projection_repeat_current_authorization_and_audit() {
    use document_application::document_diff::{DiffCache, DiffCacheKey};
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let a = seed_version(&f.pool, f.document_id.as_uuid(), 1, "PUBLISHED").await;
    let b = seed_version(&f.pool, f.document_id.as_uuid(), 2, "PUBLISHED").await;
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(b.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let pair = f
        .repository
        .capture_pair(&context(), request(f.document_id, a, b))
        .await
        .unwrap();
    let result = result(&pair);
    let key = DiffCacheKey::from_result(&result);
    f.repository
        .put(key, result.clone(), result.canonical_digest())
        .await
        .unwrap();
    for _ in 0..2 {
        let cached = f.repository.get(&key).await.unwrap().unwrap();
        f.repository
            .authorize_and_audit_result(&context(), &pair, &cached, true, None)
            .await
            .unwrap();
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type = 'document.diff.result_access_granted'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 2);
    let policy: Uuid =
        sqlx::query_scalar("SELECT policy_id FROM access_policy_bindings WHERE folder_id = $1")
            .bind(f.root_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    sqlx::query(
        "DELETE FROM access_policy_grants WHERE policy_id = $1 AND action = 'read_history'",
    )
    .bind(policy)
    .execute(&f.pool)
    .await
    .unwrap();
    let cached = f.repository.get(&key).await.unwrap().unwrap();
    assert_eq!(
        f.repository
            .authorize_and_audit_result(&context(), &pair, &cached, true, None)
            .await
            .unwrap_err(),
        RepositoryError::Forbidden
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type = 'document.diff.result_access_granted'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 2);
}
