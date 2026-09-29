#[path = "support/semantic_inspection.rs"]
mod semantic_support;
#[path = "support/management.rs"]
mod support;

use document_application::{
    BootstrapRootPolicy, RepositoryError, SemanticInspectionRepository,
    document_diff::{DiffRequest, DocumentDiffRepository},
};
use document_diff_core::DiffProfileVersion;
use document_domain::{
    Action, DocumentId, DocumentVersionId, PolicyGrant, PolicySubject, PolicySubjectKind,
};
use document_repository_postgres::PostgresDocumentRepository;
use document_semantic_inspection_core::FormatId;
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
    document: DocumentId,
    number: i64,
    state: &str,
    classified: bool,
    with_inspection: bool,
) -> DocumentVersionId {
    let version = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,withdrawn_at,created_by_identity_provider,created_by_principal_id,metadata,created_at,requires_content_classification) VALUES ($1,$2,$3,$4,$5,CASE WHEN $4 = 'WORKING' THEN NULL ELSE now() END,CASE WHEN $4 = 'WITHDRAWN' THEN now() ELSE NULL END,'test-idp','policy-admin','{}',now(),$6)")
        .bind(version.as_uuid()).bind(document.as_uuid()).bind(number).bind(state)
        .bind(format!("Version {number}")).bind(!classified)
        .execute(pool).await.unwrap();
    let file = Uuid::now_v7();
    let item = Uuid::now_v7();
    let representation = Uuid::now_v7();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',13,$3,now())")
        .bind(file).bind(semantic_support::RAW_HASH.to_vec()).bind(format!("objects/{file}/source"))
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(item).bind(version.as_uuid()).bind(representation).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename,detected_format,inspection_profile_version,semantic_fingerprint) VALUES ($1,$2,$3,'AUTHORITATIVE','source.txt','txt','dsi-v0',$4)")
        .bind(representation).bind(item).bind(file).bind(vec![number as u8;32])
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    if with_inspection {
        PostgresDocumentRepository::new(pool.clone())
            .insert_or_converge_semantic_inspection(semantic_support::record(file, number as u8))
            .await
            .unwrap();
    }
    version
}

fn request(
    document_id: DocumentId,
    base: DocumentVersionId,
    target: DocumentVersionId,
) -> DiffRequest {
    DiffRequest {
        document_id,
        base_version_id: base,
        target_version_id: target,
        profile: DiffProfileVersion::V0,
    }
}

#[tokio::test]
async fn captures_current_history_withdrawn_and_working_under_current_permissions() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(
            &context(),
            vec![grant([Action::Read, Action::ReadHistory, Action::Write])],
        )
        .await
        .unwrap();
    let history = seed_version(&f.pool, f.document_id, 1, "PUBLISHED", true, true).await;
    let current = seed_version(&f.pool, f.document_id, 2, "PUBLISHED", true, true).await;
    let withdrawn = seed_version(&f.pool, f.document_id, 3, "WITHDRAWN", true, true).await;
    let working = seed_version(&f.pool, f.document_id, 4, "WORKING", true, true).await;
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(current.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();

    let pair = f
        .repository
        .capture_pair(&context(), request(f.document_id, current, working))
        .await
        .unwrap();
    assert_eq!(pair.base.version_id, current);
    assert_eq!(pair.target.version_id, working);
    assert_eq!(pair.base.items[0].format, Some(FormatId::Txt));
    assert!(pair.base.items[0].inspection_binding_digest.is_some());
    let pair = f
        .repository
        .capture_pair(&context(), request(f.document_id, history, withdrawn))
        .await
        .unwrap();
    assert_eq!(pair.base.version_id, history);
    assert_eq!(pair.target.version_id, withdrawn);
    assert_ne!(pair.base.snapshot_digest(), pair.target.snapshot_digest());

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
            .capture_pair(&context(), request(f.document_id, current, history))
            .await
            .unwrap_err(),
        RepositoryError::Forbidden,
    );
    assert!(
        f.repository
            .capture_pair(&context(), request(f.document_id, current, working))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn rejects_same_version_other_document_and_unclassified_legacy_manifest() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let a = seed_version(&f.pool, f.document_id, 1, "PUBLISHED", true, true).await;
    let legacy = seed_version(&f.pool, f.document_id, 2, "PUBLISHED", false, false).await;
    let other_document = DocumentId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,1,'{}',now())")
        .bind(other_document.as_uuid()).bind(f.root_id.as_uuid()).execute(&f.pool).await.unwrap();
    let foreign = seed_version(&f.pool, other_document, 1, "PUBLISHED", true, true).await;
    assert_eq!(
        f.repository
            .capture_pair(&context(), request(f.document_id, a, a))
            .await
            .unwrap_err(),
        RepositoryError::BusinessRule
    );
    assert_eq!(
        f.repository
            .capture_pair(&context(), request(f.document_id, a, foreign))
            .await
            .unwrap_err(),
        RepositoryError::DocumentVersionNotFound
    );
    assert_eq!(
        f.repository
            .capture_pair(&context(), request(f.document_id, a, legacy))
            .await
            .unwrap_err(),
        RepositoryError::BusinessRule
    );
}

#[tokio::test]
async fn missing_dsi_is_explicit_and_t10_requires_history_permission() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let a = seed_version(&f.pool, f.document_id, 1, "PUBLISHED", true, true).await;
    let missing = seed_version(&f.pool, f.document_id, 2, "PUBLISHED", true, false).await;
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(missing.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let pair = f
        .repository
        .capture_pair(&context(), request(f.document_id, a, missing))
        .await
        .unwrap();
    assert_eq!(pair.target.items[0].semantic_fingerprint, None);
    assert_eq!(pair.target.items[0].inspection_binding_digest, None);
    assert_ne!(pair.base.snapshot_digest(), pair.target.snapshot_digest());

    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,1,$4,'test-idp','policy-admin','end',$4,2,now())")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(vec![0_u8;32]).bind(missing.as_uuid())
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
            .capture_pair(&context(), request(f.document_id, a, missing))
            .await
            .is_ok()
    );
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
            .capture_pair(&context(), request(f.document_id, a, missing))
            .await
            .unwrap_err(),
        RepositoryError::Forbidden
    );
}

struct RegenerateEvidence {
    repository: PostgresDocumentRepository,
    pool: sqlx::PgPool,
    changed_version: Option<DocumentVersionId>,
}

impl document_application::document_diff::DiffInspectionEvidence for RegenerateEvidence {
    async fn ensure(
        &self,
        file_id: document_domain::FileId,
        _profile: document_semantic_inspection_core::InspectionProfileVersion,
    ) -> Result<
        document_application::SemanticInspectionRecord,
        document_application::ApplicationError,
    > {
        let record = self
            .repository
            .insert_or_converge_semantic_inspection(semantic_support::record(file_id.as_uuid(), 2))
            .await?;
        if let Some(version) = self.changed_version {
            sqlx::query("UPDATE content_items SET logical_path = 'changed/path' WHERE document_version_id = $1")
                .bind(version.as_uuid()).execute(&self.pool).await.unwrap();
        }
        Ok(record)
    }
}

#[tokio::test]
async fn regenerated_dsi_is_recaptured_and_concurrent_manifest_change_is_rejected() {
    use document_application::{ApplicationError, document_diff::capture_pair_with_evidence};
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let a = seed_version(&f.pool, f.document_id, 1, "PUBLISHED", true, true).await;
    let missing = seed_version(&f.pool, f.document_id, 2, "PUBLISHED", true, false).await;
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(missing.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let evidence = RegenerateEvidence {
        repository: PostgresDocumentRepository::new(f.pool.clone()),
        pool: f.pool.clone(),
        changed_version: Some(missing),
    };
    assert!(matches!(
        capture_pair_with_evidence(
            &*f.repository,
            &evidence,
            &context(),
            request(f.document_id, a, missing)
        )
        .await,
        Err(ApplicationError::StaleComparisonInput)
    ));
    sqlx::query("UPDATE content_items SET logical_path = 'primary' WHERE document_version_id = $1")
        .bind(missing.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let pair = capture_pair_with_evidence(
        &*f.repository,
        &evidence,
        &context(),
        request(f.document_id, a, missing),
    )
    .await
    .unwrap();
    assert!(pair.target.items[0].inspection_binding_digest.is_some());
    assert!(pair.target.items[0].semantic_fingerprint.is_some());
}

struct UnsupportedEvidence;

impl document_application::document_diff::DiffInspectionEvidence for UnsupportedEvidence {
    async fn ensure(
        &self,
        _file_id: document_domain::FileId,
        _profile: document_semantic_inspection_core::InspectionProfileVersion,
    ) -> Result<
        document_application::SemanticInspectionRecord,
        document_application::ApplicationError,
    > {
        Err(document_application::ApplicationError::InspectionFailed(
            document_application::InspectionExecutionError::UnsupportedDocumentFormat,
        ))
    }
}

#[tokio::test]
async fn unavailable_semantic_evidence_remains_unverified() {
    use document_application::document_diff::capture_pair_with_evidence;
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let a = seed_version(&f.pool, f.document_id, 1, "PUBLISHED", true, true).await;
    let missing = seed_version(&f.pool, f.document_id, 2, "PUBLISHED", true, false).await;
    let pair = capture_pair_with_evidence(
        &*f.repository,
        &UnsupportedEvidence,
        &context(),
        request(f.document_id, a, missing),
    )
    .await
    .unwrap();
    assert_eq!(pair.target.items[0].semantic_fingerprint, None);
    assert_eq!(pair.target.items[0].inspection_binding_digest, None);
}
