#[path = "support/versioning.rs"]
mod support;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use document_application::{
    AccessPolicyService, BootstrapRootPolicy, ContentReader, CreateVersionCommand,
    DocumentVersionService, DueExecutionOutcome, IdentityContextResolver, IdentityResolutionError,
    InspectionExecutionError, InvocationKind, ManagementCommand, ManagementOperationId,
    PublicationScheduleRepository, PublishOperationId, SchedulePublishCommand,
    SemanticInspectionExecutor, VerifiedActorContext,
};
use document_domain::{
    Action, DocumentVersionId, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind,
    PolicyTarget, PrincipalRef,
};
use document_repository_postgres::PostgresDocumentRepository;
use document_semantic_inspection_core::{WorkerRequest, WorkerResponse};
use support::{TestExecutor, TestIds, actor, fixture, operation_id};
use time::{Duration, OffsetDateTime};
use tokio::sync::Notify;
use uuid::Uuid;

struct PauseOnceExecutor {
    delegate: Arc<TestExecutor>,
    entered: Arc<Notify>,
    resume: Arc<Notify>,
    pause_next: AtomicBool,
}

impl SemanticInspectionExecutor for PauseOnceExecutor {
    async fn inspect(
        &self,
        request: WorkerRequest,
        content: ContentReader,
    ) -> Result<WorkerResponse, InspectionExecutionError> {
        if self.pause_next.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        self.delegate.inspect(request, content).await
    }
}

struct Resolver {
    unavailable: bool,
    invalid: bool,
}

impl IdentityContextResolver for Resolver {
    async fn resolve(
        &self,
        principal: &PrincipalRef,
    ) -> Result<VerifiedActorContext, IdentityResolutionError> {
        if self.unavailable {
            return Err(IdentityResolutionError::Unavailable);
        }
        if self.invalid {
            return Err(IdentityResolutionError::InvalidIdentity);
        }
        let subject = PolicySubject::new(
            PolicySubjectKind::Principal,
            principal.identity_provider(),
            principal.principal_id(),
        )
        .unwrap();
        VerifiedActorContext::from_trusted_adapter(
            principal.clone(),
            vec![subject],
            OffsetDateTime::now_utc() + Duration::hours(1),
            InvocationKind::HumanInteractive,
            None,
        )
        .map_err(|_| IdentityResolutionError::InvalidIdentity)
    }
}

fn context() -> VerifiedActorContext {
    let subject = PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        actor(),
        vec![subject],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn grants(actions: impl IntoIterator<Item = Action>) -> Vec<PolicyGrant> {
    vec![
        PolicyGrant::new(
            PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap(),
            actions,
        )
        .unwrap(),
    ]
}

fn publish_id() -> PublishOperationId {
    PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap()
}

async fn reserved_fixture() -> (
    support::Fixture,
    DocumentVersionId,
    PublishOperationId,
    Arc<PostgresDocumentRepository>,
) {
    let f = fixture().await;
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        f.pool.clone(),
        actor(),
    ));
    repository
        .initialize_root_policy(
            &context(),
            grants([
                Action::Read,
                Action::Write,
                Action::Publish,
                Action::Administer,
            ]),
        )
        .await
        .unwrap();
    let target = DocumentVersionId::from_uuid(Uuid::now_v7());
    f.service()
        .create_version(
            CreateVersionCommand::new(operation_id(81), f.document_id, target, 1, actor()).unwrap(),
            f.prepare("Scheduled replacement", 9).await,
        )
        .await
        .unwrap();
    let id = publish_id();
    f.service()
        .schedule_publish(
            SchedulePublishCommand::new(
                id,
                f.document_id,
                target,
                2,
                actor(),
                OffsetDateTime::from_unix_timestamp(2_000_000_000).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let due: OffsetDateTime = sqlx::query_scalar("SELECT now() - INTERVAL '1 second'")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE document_publish_schedules SET scheduled_publish_at = $1, next_retry_at = NULL WHERE publish_operation_id = $2")
        .bind(due).bind(id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query(
        "UPDATE document_versions SET scheduled_publish_at = $1 WHERE document_version_id = $2",
    )
    .bind(due)
    .bind(target.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    (f, target, id, repository)
}

fn executor() -> PrincipalRef {
    PrincipalRef::new("service", "publication-scheduler").unwrap()
}

#[tokio::test]
async fn revoked_requester_is_terminalized_without_publication() {
    let (f, _target, id, repository) = reserved_fixture().await;
    AccessPolicyService::new(repository)
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants([Action::Read])),
                reason: "revoke publish".into(),
            },
        )
        .await
        .unwrap();
    let outcome = f
        .service()
        .execute_due_authorized(
            id,
            &Resolver {
                unavailable: false,
                invalid: false,
            },
            &executor(),
        )
        .await
        .unwrap();
    assert_eq!(
        outcome,
        DueExecutionOutcome::Terminal("authorization_revoked".into())
    );
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, Some(f.base_id.as_uuid()));
    assert_eq!(
        f.repository.get_schedule(id).await.unwrap().unwrap().status,
        "TERMINAL"
    );
}

#[tokio::test]
async fn identity_outage_retries_same_publish_id_without_terminalizing() {
    let (f, target, id, _repository) = reserved_fixture().await;
    let outcome = f
        .service()
        .execute_due_authorized(
            id,
            &Resolver {
                unavailable: true,
                invalid: false,
            },
            &executor(),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, DueExecutionOutcome::RetryScheduled(_)));
    let schedule = f.repository.get_schedule(id).await.unwrap().unwrap();
    assert_eq!(schedule.status, "PENDING");
    assert_eq!(schedule.command.publish_operation_id(), id);
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, Some(f.base_id.as_uuid()));
    sqlx::query("UPDATE document_publish_schedules SET next_retry_at = NULL WHERE publish_operation_id = $1")
        .bind(id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(matches!(
        f.service()
            .execute_due_authorized(
                id,
                &Resolver {
                    unavailable: false,
                    invalid: false,
                },
                &executor(),
            )
            .await
            .unwrap(),
        DueExecutionOutcome::Published(_)
    ));
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, Some(target.as_uuid()));
}

#[tokio::test]
async fn invalid_identity_terminalizes_without_publication() {
    let (f, _target, id, _repository) = reserved_fixture().await;
    assert_eq!(
        f.service()
            .execute_due_authorized(
                id,
                &Resolver {
                    unavailable: false,
                    invalid: true,
                },
                &executor(),
            )
            .await
            .unwrap(),
        DueExecutionOutcome::Terminal("identity_invalid".into())
    );
    assert_eq!(
        f.repository.get_schedule(id).await.unwrap().unwrap().status,
        "TERMINAL"
    );
    let published_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_publish_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(published_count, 0);
}

#[tokio::test]
async fn concurrent_authorized_workers_publish_and_audit_once() {
    let (f, target, id, _repository) = reserved_fixture().await;
    let service = f.service();
    let resolver = Resolver {
        unavailable: false,
        invalid: false,
    };
    let executor = executor();
    let (first, second) = tokio::join!(
        service.execute_due_authorized(id, &resolver, &executor),
        service.execute_due_authorized(id, &resolver, &executor)
    );
    assert!(matches!(first.unwrap(), DueExecutionOutcome::Published(_)));
    assert!(matches!(second.unwrap(), DueExecutionOutcome::Published(_)));
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, Some(target.as_uuid()));
    let ledger: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_publish_operations WHERE publish_operation_id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_outbox_events WHERE resource_id = $1 AND event_type = 'document.version.published'",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!((ledger, audit), (1, 1));
    let audit_identity: (String, String, serde_json::Value) = sqlx::query_as(
        "SELECT actor_identity_provider, actor_principal_id, data->'serviceExecutor' \
         FROM audit_outbox_events WHERE resource_id = $1 AND event_type = 'document.version.published'",
    )
    .bind(f.document_id.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(audit_identity.0, "test-idp");
    assert_eq!(audit_identity.1, "editor");
    assert_eq!(
        audit_identity.2,
        serde_json::json!({
            "identityProvider": "service",
            "principalId": "publication-scheduler",
        })
    );
}

#[tokio::test]
async fn permission_revoked_during_due_inspection_terminalizes_before_commit() {
    let (f, target, id, repository) = reserved_fixture().await;
    sqlx::query(
        "DELETE FROM document_semantic_inspections WHERE file_id IN \
         (SELECT cr.file_id FROM content_items ci JOIN content_representations cr \
          ON cr.content_representation_id = ci.authoritative_representation_id \
          WHERE ci.document_version_id = $1)",
    )
    .bind(target.as_uuid())
    .execute(&f.pool)
    .await
    .unwrap();
    let entered = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    let paused = Arc::new(PauseOnceExecutor {
        delegate: f.executor.clone(),
        entered: entered.clone(),
        resume: resume.clone(),
        pause_next: AtomicBool::new(true),
    });
    let service = DocumentVersionService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        paused,
        f.repository.clone(),
    );
    let task = tokio::spawn(async move {
        service
            .execute_due_authorized(
                id,
                &Resolver {
                    unavailable: false,
                    invalid: false,
                },
                &executor(),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), entered.notified())
        .await
        .unwrap();
    AccessPolicyService::new(repository)
        .set_access_policy(
            &context(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(f.document_id),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(grants([Action::Read])),
                reason: "revoke during due inspection".into(),
            },
        )
        .await
        .unwrap();
    resume.notify_one();
    assert_eq!(
        task.await.unwrap().unwrap(),
        DueExecutionOutcome::Terminal("authorization_revoked".into())
    );
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id = $1")
            .bind(f.document_id.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(current, Some(f.base_id.as_uuid()));
    let published_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_publish_operations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(published_count, 0);
    assert_eq!(
        f.repository.get_schedule(id).await.unwrap().unwrap().status,
        "TERMINAL"
    );
}
