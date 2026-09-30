#[path = "support/management.rs"]
mod support;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use document_application::{
    AccessPolicyService, ApplicationError, BootstrapRootPolicy, DocumentAccessCheckRepository,
    DocumentAccessCheckService, InvocationKind, ManagementCommand, ManagementOperationId,
    RepositoryError, VerifiedActorContext,
};
use document_domain::{
    Action, DocumentId, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind, PolicyTarget,
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

fn policy_command(
    document_id: DocumentId,
    expected_policy_revision: i64,
    actions: impl IntoIterator<Item = Action>,
) -> ManagementCommand {
    ManagementCommand::SetAccessPolicy {
        operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
        target: PolicyTarget::Document(document_id),
        expected_policy_revision,
        mode: PolicyMode::Explicit(vec![grant(actions)]),
        reason: "test access change".into(),
    }
}

#[tokio::test]
async fn read_uses_current_access_policy_after_revocation() {
    let fixture = fixture().await;
    fixture
        .repository
        .initialize_root_policy(&context(), vec![grant([Action::Read, Action::Administer])])
        .await
        .unwrap();
    let service = DocumentAccessCheckService::new(fixture.repository.clone());
    let actor = context();

    // This is an access-only check: the fixture has no current published version.
    service
        .check(&actor, fixture.document_id, &[Action::Read])
        .await
        .unwrap();
    AccessPolicyService::new(fixture.repository.clone())
        .set_access_policy(
            &actor,
            policy_command(fixture.document_id, 0, [Action::Administer]),
        )
        .await
        .unwrap();

    assert!(matches!(
        service
            .check(&actor, fixture.document_id, &[Action::Read])
            .await,
        Err(ApplicationError::Forbidden)
    ));
    assert!(matches!(
        service
            .check(
                &actor,
                DocumentId::from_uuid(Uuid::now_v7()),
                &[Action::Read]
            )
            .await,
        Err(ApplicationError::Forbidden)
    ));
}

#[tokio::test]
async fn read_and_read_history_are_independent_permissions() {
    let fixture = fixture().await;
    fixture
        .repository
        .initialize_root_policy(
            &context(),
            vec![grant([
                Action::Read,
                Action::ReadHistory,
                Action::Administer,
            ])],
        )
        .await
        .unwrap();
    let service = DocumentAccessCheckService::new(fixture.repository.clone());
    let policy = AccessPolicyService::new(fixture.repository.clone());
    let actor = context();

    policy
        .set_access_policy(
            &actor,
            policy_command(
                fixture.document_id,
                0,
                [Action::ReadHistory, Action::Administer],
            ),
        )
        .await
        .unwrap();
    service
        .check(&actor, fixture.document_id, &[Action::ReadHistory])
        .await
        .unwrap();
    assert!(matches!(
        service
            .check(&actor, fixture.document_id, &[Action::Read])
            .await,
        Err(ApplicationError::Forbidden)
    ));
    assert!(matches!(
        service
            .check(
                &actor,
                fixture.document_id,
                &[Action::Read, Action::ReadHistory]
            )
            .await,
        Err(ApplicationError::Forbidden)
    ));

    policy
        .set_access_policy(
            &actor,
            policy_command(fixture.document_id, 1, [Action::Read, Action::Administer]),
        )
        .await
        .unwrap();
    service
        .check(&actor, fixture.document_id, &[Action::Read])
        .await
        .unwrap();
    assert!(matches!(
        service
            .check(&actor, fixture.document_id, &[Action::ReadHistory])
            .await,
        Err(ApplicationError::Forbidden)
    ));
    assert!(matches!(
        service
            .check(
                &actor,
                fixture.document_id,
                &[Action::Read, Action::ReadHistory]
            )
            .await,
        Err(ApplicationError::Forbidden)
    ));
}

struct CountingRepository(AtomicUsize);

impl DocumentAccessCheckRepository for CountingRepository {
    async fn check_document_access(
        &self,
        _ctx: &VerifiedActorContext,
        _document_id: DocumentId,
        _required: &[Action],
    ) -> Result<(), RepositoryError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn expired_actor_is_rejected_before_repository_call() {
    let principal = support::actor();
    let subject = PolicySubject::new(
        PolicySubjectKind::Principal,
        principal.identity_provider(),
        principal.principal_id(),
    )
    .unwrap();
    let actor = VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![subject],
        OffsetDateTime::now_utc() + Duration::milliseconds(250),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(260)).await;
    assert!(actor.ensure_current().is_err());

    let repository = Arc::new(CountingRepository(AtomicUsize::new(0)));
    let service = DocumentAccessCheckService::new(repository.clone());
    assert!(matches!(
        service
            .check(
                &actor,
                DocumentId::from_uuid(Uuid::now_v7()),
                &[Action::Read]
            )
            .await,
        Err(ApplicationError::Validation(_))
    ));
    assert_eq!(repository.0.load(Ordering::SeqCst), 0);
}
