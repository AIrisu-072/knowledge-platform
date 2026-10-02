#[path = "support/database.rs"]
mod database;

use database::TestDatabase;
use document_application::{AccessPolicyReadRepository, BootstrapRootPolicy, PolicyBindingMode};
use document_domain::{
    Action, FolderId, PolicyGrant, PolicySubject, PolicySubjectKind, PolicyTarget, ResourceRef,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use document_server::{
    bootstrap::{BootstrapError, BootstrapOutcome, bootstrap_poc},
    identity::{PoCIdentityProfile, StaticPoCIdentityAdapter},
};
use sqlx::{PgPool, postgres::PgPoolOptions};

fn grant(group: &str, actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Group, "poc", group).unwrap(),
        actions,
    )
    .unwrap()
}

fn expected_grants() -> Vec<PolicyGrant> {
    vec![
        grant("poc-agents", [Action::Read, Action::ReadHistory]),
        grant(
            "poc-users",
            [
                Action::Read,
                Action::ReadHistory,
                Action::Write,
                Action::Publish,
                Action::Administer,
            ],
        ),
    ]
}

fn root() -> PolicyTarget {
    PolicyTarget::Folder(FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID))
}

async fn mutation_counts(pool: &PgPool) -> (i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM access_policy_bindings), (SELECT count(*) FROM outbox_events), (SELECT count(*) FROM audit_outbox_events), (SELECT access_revision FROM document_access_state WHERE id = 1)")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn uninitialized_root_remains_denied_until_explicit_human_bootstrap() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    let human = StaticPoCIdentityAdapter::new(PoCIdentityProfile::Human)
        .current_context()
        .unwrap();
    let repository = PostgresDocumentRepository::new(pool.clone());
    assert!(
        !repository
            .authorize_resource(&human, ResourceRef::from(root()), &[Action::Read])
            .await
            .unwrap()
    );
    assert_eq!(mutation_counts(&pool).await, (0, 0, 0, 0));

    assert_eq!(
        bootstrap_poc(&pool, PoCIdentityProfile::Human)
            .await
            .unwrap(),
        BootstrapOutcome::Initialized
    );
    let policy = repository.read_access_policy(&human, root()).await.unwrap();
    assert_eq!(policy.target, root());
    assert_eq!(policy.binding_mode, PolicyBindingMode::Explicit);
    assert_eq!(policy.effective_source, root());
    assert_eq!(policy.policy_id, Some(policy.effective_policy_id));
    assert_eq!(policy.policy_revision, 1);
    assert_eq!(policy.effective_grants, expected_grants());
    assert_eq!(mutation_counts(&pool).await, (1, 1, 1, 1));

    let agent = StaticPoCIdentityAdapter::new(PoCIdentityProfile::Agent)
        .current_context()
        .unwrap();
    assert!(
        repository
            .authorize_resource(
                &agent,
                ResourceRef::from(root()),
                &[Action::Read, Action::ReadHistory]
            )
            .await
            .unwrap()
    );
    for forbidden in [Action::Write, Action::Publish, Action::Administer] {
        assert!(
            !repository
                .authorize_resource(&agent, ResourceRef::from(root()), &[forbidden])
                .await
                .unwrap()
        );
    }
    database.close().await;
}

#[tokio::test]
async fn agent_bootstrap_is_rejected_before_any_database_access() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://synthetic-secret:synthetic-secret@127.0.0.1/synthetic-secret")
        .unwrap();
    pool.close().await;
    assert_eq!(
        bootstrap_poc(&pool, PoCIdentityProfile::Agent).await,
        Err(BootstrapError::HumanProfileRequired)
    );
}

#[tokio::test]
async fn retry_of_exact_bootstrap_policy_does_not_mutate_policy_or_audit() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    bootstrap_poc(&pool, PoCIdentityProfile::Human)
        .await
        .unwrap();
    let human = StaticPoCIdentityAdapter::new(PoCIdentityProfile::Human)
        .current_context()
        .unwrap();
    let repository = PostgresDocumentRepository::new(pool.clone());
    let before = repository.read_access_policy(&human, root()).await.unwrap();
    let counts = mutation_counts(&pool).await;
    assert_eq!(
        bootstrap_poc(&pool, PoCIdentityProfile::Human)
            .await
            .unwrap(),
        BootstrapOutcome::AlreadyInitialized
    );
    assert_eq!(
        repository.read_access_policy(&human, root()).await.unwrap(),
        before
    );
    assert_eq!(mutation_counts(&pool).await, counts);
    database.close().await;
}

#[tokio::test]
async fn unexpected_readable_policy_is_never_overwritten_or_reported_as_success() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    let human = StaticPoCIdentityAdapter::new(PoCIdentityProfile::Human)
        .current_context()
        .unwrap();
    let repository = PostgresDocumentRepository::new_with_bootstrap_actor(
        pool.clone(),
        human.principal().clone(),
    );
    let mut unexpected = expected_grants();
    unexpected[0] = grant(
        "poc-agents",
        [Action::Read, Action::ReadHistory, Action::Write],
    );
    repository
        .initialize_root_policy(&human, unexpected)
        .await
        .unwrap();
    let before = repository.read_access_policy(&human, root()).await.unwrap();
    let counts = mutation_counts(&pool).await;
    assert_eq!(
        bootstrap_poc(&pool, PoCIdentityProfile::Human).await,
        Err(BootstrapError::UnexpectedPolicy)
    );
    assert_eq!(
        repository.read_access_policy(&human, root()).await.unwrap(),
        before
    );
    assert_eq!(mutation_counts(&pool).await, counts);
    database.close().await;
}

#[tokio::test]
async fn policy_that_human_cannot_read_is_not_treated_as_idempotent_success() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    let human = StaticPoCIdentityAdapter::new(PoCIdentityProfile::Human)
        .current_context()
        .unwrap();
    let repository = PostgresDocumentRepository::new_with_bootstrap_actor(
        pool.clone(),
        human.principal().clone(),
    );
    repository
        .initialize_root_policy(&human, vec![grant("poc-agents", [Action::Read])])
        .await
        .unwrap();
    let counts = mutation_counts(&pool).await;
    assert_eq!(
        bootstrap_poc(&pool, PoCIdentityProfile::Human).await,
        Err(BootstrapError::PolicyReadFailed)
    );
    assert_eq!(mutation_counts(&pool).await, counts);
    database.close().await;
}

#[tokio::test]
async fn bootstrap_database_failures_are_redacted_and_never_idempotent_success() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://synthetic-secret:synthetic-secret@127.0.0.1/synthetic-secret")
        .unwrap();
    pool.close().await;
    let error = bootstrap_poc(&pool, PoCIdentityProfile::Human)
        .await
        .unwrap_err();
    assert_eq!(error, BootstrapError::InitializationFailed);
    for rendered in [error.to_string(), format!("{error:?}")] {
        assert!(!rendered.contains("synthetic-secret"));
        assert!(!rendered.contains("postgres://"));
        assert!(!rendered.contains("127.0.0.1"));
    }
}

#[tokio::test]
async fn concurrent_bootstrap_initializes_once_and_the_other_call_verifies_existing_policy() {
    let database = TestDatabase::new().await;
    let pool = database.pool.clone();
    migrate(&pool).await.unwrap();
    let (first, second) = tokio::join!(
        bootstrap_poc(&pool, PoCIdentityProfile::Human),
        bootstrap_poc(&pool, PoCIdentityProfile::Human),
    );
    let outcomes = [first.unwrap(), second.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == BootstrapOutcome::Initialized)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == BootstrapOutcome::AlreadyInitialized)
            .count(),
        1
    );
    assert_eq!(mutation_counts(&pool).await, (1, 1, 1, 1));
    database.close().await;
}
