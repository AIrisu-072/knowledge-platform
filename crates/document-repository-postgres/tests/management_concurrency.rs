#[path = "support/management.rs"]
mod support;

use document_application::{
    ApplicationError, BootstrapRootPolicy, MarkVersionRead, ReadStateService,
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

#[tokio::test]
async fn version_switch_committed_ahead_of_t9_prevents_stale_first_read() {
    let f = fixture().await;
    f.repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::ReadHistory])])
        .await
        .unwrap();
    let old_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let new_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    for (id, number) in [(old_id, 1), (new_id, 2)] {
        sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,$3,'PUBLISHED','Concurrent',now(),'test-idp','policy-admin','{}',now())")
            .bind(id.as_uuid()).bind(f.document_id.as_uuid()).bind(number)
            .execute(&f.pool).await.unwrap();
    }
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(old_id.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let mut switch = f.pool.begin().await.unwrap();
    sqlx::query("SELECT document_id FROM documents WHERE document_id = $1 FOR UPDATE")
        .bind(f.document_id.as_uuid())
        .fetch_one(&mut *switch)
        .await
        .unwrap();
    let repository = f.repository.clone();
    let document_id = f.document_id;
    let pending = tokio::spawn(async move {
        ReadStateService::new(repository)
            .mark_version_read(
                &context(),
                MarkVersionRead {
                    document_id,
                    document_version_id: old_id,
                },
            )
            .await
    });
    tokio::task::yield_now().await;
    sqlx::query("UPDATE documents SET current_version_id = $1, revision = revision + 1 WHERE document_id = $2")
        .bind(new_id.as_uuid()).bind(f.document_id.as_uuid())
        .execute(&mut *switch).await.unwrap();
    switch.commit().await.unwrap();
    assert!(matches!(
        pending.await.unwrap(),
        Err(ApplicationError::StaleVersion)
    ));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM document_read_states")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type = 'document.version.read_confirmed'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!((rows, audits), (0, 0));
}
