#[path = "support/management.rs"]
mod support;

use document_application::{
    ApplicationError, BootstrapRootPolicy, InvocationKind, MarkVersionRead, ReadStateService,
    VerifiedActorContext,
};
use document_domain::{
    Action, DocumentVersionId, PolicyGrant, PolicySubject, PolicySubjectKind, PrincipalRef,
};
use support::{Fixture, context, fixture};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

fn actor_context(provider: &str, kind: InvocationKind) -> VerifiedActorContext {
    let principal = PrincipalRef::new(provider, "policy-admin").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![PolicySubject::new(PolicySubjectKind::Principal, provider, "policy-admin").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        kind,
        None,
    )
    .unwrap()
}

fn grant(provider: &str, actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, provider, "policy-admin").unwrap(),
        actions,
    )
    .unwrap()
}

async fn allow(f: &Fixture, grants: Vec<PolicyGrant>) {
    f.repository
        .initialize_root_policy(&context(), grants)
        .await
        .unwrap();
}

async fn published(f: &Fixture, version_no: i64) -> DocumentVersionId {
    let id = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,$3,'PUBLISHED','Published',now(),'test-idp','policy-admin','{}',now())")
        .bind(id.as_uuid()).bind(f.document_id.as_uuid()).bind(version_no)
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1, revision = revision + 1 WHERE document_id = $2")
        .bind(id.as_uuid()).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    id
}

fn command(f: &Fixture, version_id: DocumentVersionId) -> MarkVersionRead {
    MarkVersionRead {
        document_id: f.document_id,
        document_version_id: version_id,
    }
}

async fn state_count(f: &Fixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM document_read_states")
        .fetch_one(&f.pool)
        .await
        .unwrap()
}

async fn audit_count(f: &Fixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type = 'document.version.read_confirmed'")
        .fetch_one(&f.pool).await.unwrap()
}

#[tokio::test]
async fn first_explicit_confirmation_is_idempotent_even_under_concurrency() {
    let f = fixture().await;
    allow(&f, vec![grant("test-idp", [Action::Read])]).await;
    let version_id = published(&f, 1).await;
    let before_revision: i64 =
        sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let service = ReadStateService::new(f.repository.clone());
    let ctx = context();
    let (first, second) = tokio::join!(
        service.mark_version_read(&ctx, command(&f, version_id)),
        service.mark_version_read(&ctx, command(&f, version_id)),
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_ne!(first.inserted, second.inserted);
    assert_eq!(first.first_read_at, second.first_read_at);
    assert_eq!(first.document_version_id, version_id);
    assert_eq!(first.principal, *ctx.principal());
    let repeated = service
        .mark_version_read(&ctx, command(&f, version_id))
        .await
        .unwrap();
    assert!(!repeated.inserted);
    assert_eq!(repeated.first_read_at, first.first_read_at);
    let projection: (bool, i64) = sqlx::query_as("SELECT needs_recheck,read_state_revision FROM document_read_states WHERE document_version_id=$1")
        .bind(version_id.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(projection, (false, 1));
    assert_eq!(state_count(&f).await, 1);
    assert_eq!(audit_count(&f).await, 1);
    let after_revision: i64 =
        sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(after_revision, before_revision);
    let domain_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE event_type = 'DocumentVersionReadConfirmed'",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(domain_count, 0);
    let management_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_management_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(management_count, 0);
}

#[tokio::test]
async fn agent_and_service_cannot_mark_a_version_read() {
    let f = fixture().await;
    allow(&f, vec![grant("test-idp", [Action::Read])]).await;
    let version_id = published(&f, 1).await;
    let service = ReadStateService::new(f.repository.clone());
    for kind in [InvocationKind::Agent, InvocationKind::Service] {
        let result = service
            .mark_version_read(&actor_context("test-idp", kind), command(&f, version_id))
            .await;
        assert!(matches!(result, Err(ApplicationError::Forbidden)));
    }
    assert_eq!(state_count(&f).await, 0);
    assert_eq!(audit_count(&f).await, 0);
}

#[tokio::test]
async fn issuer_separates_same_principal_id_and_new_version_is_unread() {
    let f = fixture().await;
    allow(
        &f,
        vec![
            grant("test-idp", [Action::Read, Action::ReadHistory]),
            grant("other-idp", [Action::Read, Action::ReadHistory]),
        ],
    )
    .await;
    let first_version = published(&f, 1).await;
    let service = ReadStateService::new(f.repository.clone());
    let first = service
        .mark_version_read(&context(), command(&f, first_version))
        .await
        .unwrap();
    let other = service
        .mark_version_read(
            &actor_context("other-idp", InvocationKind::HumanInteractive),
            command(&f, first_version),
        )
        .await
        .unwrap();
    assert!(first.inserted && other.inserted);
    assert_eq!(state_count(&f).await, 2);
    let next_version = published(&f, 2).await;
    let unread: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_read_states WHERE document_version_id = $1",
    )
    .bind(next_version.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(unread, 0);
    let old = service
        .mark_version_read(&context(), command(&f, first_version))
        .await
        .unwrap();
    assert!(!old.inserted);
    assert_eq!(old.first_read_at, first.first_read_at);
    let stale = service
        .mark_version_read(
            &actor_context("third-idp", InvocationKind::HumanInteractive),
            command(&f, first_version),
        )
        .await;
    assert!(matches!(stale, Err(ApplicationError::DocumentNotFound)));
}

#[tokio::test]
async fn old_record_replay_requires_current_history_right_and_new_stale_mark_is_rejected() {
    let f = fixture().await;
    allow(&f, vec![grant("test-idp", [Action::Read])]).await;
    let first_version = published(&f, 1).await;
    let service = ReadStateService::new(f.repository.clone());
    service
        .mark_version_read(&context(), command(&f, first_version))
        .await
        .unwrap();
    let _next_version = published(&f, 2).await;
    assert!(matches!(
        service
            .mark_version_read(&context(), command(&f, first_version))
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
    assert!(
        !service
            .mark_version_read(&context(), command(&f, first_version))
            .await
            .unwrap()
            .inserted
    );
    sqlx::query("DELETE FROM document_read_states WHERE document_version_id = $1")
        .bind(first_version.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(matches!(
        service
            .mark_version_read(&context(), command(&f, first_version))
            .await,
        Err(ApplicationError::StaleVersion)
    ));
}

#[tokio::test]
async fn mandatory_audit_failure_rolls_back_first_read() {
    let f = fixture().await;
    allow(&f, vec![grant("test-idp", [Action::Read])]).await;
    let version_id = published(&f, 1).await;
    sqlx::query("CREATE FUNCTION reject_read_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type = 'document.version.read_confirmed' THEN RAISE EXCEPTION 'audit blocked'; END IF; RETURN NEW; END; $$")
        .execute(&f.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_read_audit BEFORE INSERT ON audit_outbox_events FOR EACH ROW EXECUTE FUNCTION reject_read_audit()")
        .execute(&f.pool).await.unwrap();
    assert!(
        ReadStateService::new(f.repository.clone())
            .mark_version_read(&context(), command(&f, version_id))
            .await
            .is_err()
    );
    assert_eq!(state_count(&f).await, 0);
    assert_eq!(audit_count(&f).await, 0);
}

#[tokio::test]
async fn version_from_another_document_cannot_be_marked_on_this_document() {
    let f = fixture().await;
    allow(&f, vec![grant("test-idp", [Action::Read])]).await;
    let other_document_id = Uuid::now_v7();
    let other_version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,1,'{}',now())")
        .bind(other_document_id)
        .bind(f.root_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Other',now(),'test-idp','policy-admin','{}',now())")
        .bind(other_version_id.as_uuid())
        .bind(other_document_id)
        .execute(&f.pool)
        .await
        .unwrap();
    let result = ReadStateService::new(f.repository.clone())
        .mark_version_read(&context(), command(&f, other_version_id))
        .await;
    assert!(matches!(
        result,
        Err(ApplicationError::DocumentVersionNotFound)
    ));
    assert_eq!(state_count(&f).await, 0);
}

#[tokio::test]
async fn publication_end_only_allows_existing_read_state_with_history_right() {
    let f = fixture().await;
    allow(
        &f,
        vec![grant("test-idp", [Action::Read, Action::ReadHistory])],
    )
    .await;
    let version_id = published(&f, 1).await;
    let service = ReadStateService::new(f.repository.clone());
    let first = service
        .mark_version_read(&context(), command(&f, version_id))
        .await
        .unwrap();
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,$4,$5,'test-idp','policy-admin','end',$5,$6,now())")
        .bind(Uuid::now_v7())
        .bind(f.document_id.as_uuid())
        .bind(vec![0_u8; 32])
        .bind(revision)
        .bind(version_id.as_uuid())
        .bind(revision + 1)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET current_version_id = NULL, revision = revision + 1 WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let replay = service
        .mark_version_read(&context(), command(&f, version_id))
        .await
        .unwrap();
    assert!(!replay.inserted);
    assert_eq!(replay.first_read_at, first.first_read_at);
    sqlx::query("DELETE FROM document_read_states WHERE document_version_id = $1")
        .bind(version_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(matches!(
        service
            .mark_version_read(&context(), command(&f, version_id))
            .await,
        Err(ApplicationError::StaleVersion)
    ));
}
