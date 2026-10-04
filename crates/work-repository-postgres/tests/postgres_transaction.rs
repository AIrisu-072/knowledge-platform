//! Explicit opt-in test: requires a disposable *_work_poc_test PostgreSQL database.
//! This test is never replaced with a fake database or an alternate socket.
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;
use work_application::WorkRepository;
use work_domain::*;
use work_repository_postgres::{PostgresWorkRepository, migrate, seed_synthetic};
fn ctx(actor: VerifiedActor, revision: i64) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: actor.assignment_id(),
    }
}
#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL database"]
async fn committed_handoff_replays_after_reconnect_and_staging_failure_rolls_back() {
    let url =
        std::env::var("WORK_POC_TEST_DATABASE_URL").expect("disposable database URL required");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        name.ends_with("_work_poc_test"),
        "only a disposable work test database is permitted"
    );
    migrate(&pool).await.unwrap();
    seed_synthetic(&pool, None).await.unwrap();
    let repository = PostgresWorkRepository::new(pool.clone());
    let command = Command::SaveDraft {
        task_id: SALES_TASK_ID,
        artifact_id: None,
        context: ctx(VerifiedActor::Sales01, 0),
        value: TextValue {
            text: "非公開の提出本文".into(),
        },
    };
    let saved_operation_id = command.context().operation_id;
    let saved = repository
        .execute(VerifiedActor::Sales01, command.clone())
        .await
        .unwrap();
    assert_eq!(
        saved,
        repository
            .execute(VerifiedActor::Sales01, command)
            .await
            .unwrap()
    );
    assert_eq!(
        repository
            .recover(VerifiedActor::Sales01, saved_operation_id)
            .await
            .unwrap(),
        saved
    );
    assert_eq!(
        repository
            .recover(VerifiedActor::Sales01, Uuid::now_v7())
            .await,
        Err(WorkError::WorkItemNotFound)
    );
    let artifact = match saved {
        MutationResult::DraftSaved { artifact, .. } => artifact,
        _ => panic!(),
    };
    assert_eq!(
        repository
            .artifact(VerifiedActor::Office01, artifact.id)
            .await,
        Err(WorkError::WorkArtifactNotFound)
    );
    let command = Command::Submit {
        task_id: SALES_TASK_ID,
        context: ctx(VerifiedActor::Sales01, 1),
        artifacts: vec![ArtifactSelection {
            artifact_id: artifact.id,
            revision: artifact.revision,
        }],
    };
    sqlx::raw_sql("CREATE FUNCTION work.reject_staging() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test staging failure'; END $$; CREATE TRIGGER reject_staging BEFORE INSERT ON work.event_staging FOR EACH ROW EXECUTE FUNCTION work.reject_staging();").execute(&pool).await.unwrap();
    assert!(
        repository
            .execute(VerifiedActor::Sales01, command.clone())
            .await
            .is_err()
    );
    assert_eq!(
        repository
            .task(VerifiedActor::Sales01, SALES_TASK_ID)
            .await
            .unwrap()
            .task
            .state,
        TaskState::Active
    );
    assert!(
        repository
            .list_tasks(VerifiedActor::Office01, TaskView::Queue)
            .await
            .unwrap()
            .is_empty()
    );
    let operation_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM work.operation_ledger WHERE operation_id=$1")
            .bind(command.context().operation_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(operation_count, 0);
    sqlx::raw_sql(
        "DROP TRIGGER reject_staging ON work.event_staging; DROP FUNCTION work.reject_staging();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let result = repository
        .execute(VerifiedActor::Sales01, command.clone())
        .await
        .unwrap();
    let snapshot = match &result {
        MutationResult::Submitted { snapshot, .. } => snapshot.clone(),
        _ => panic!(),
    };
    let reconnect = PostgresWorkRepository::new(
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap(),
    );
    assert_eq!(
        reconnect
            .execute(VerifiedActor::Sales01, command.clone())
            .await
            .unwrap(),
        result
    );
    assert_eq!(
        reconnect
            .recover(VerifiedActor::Sales01, command.context().operation_id)
            .await
            .unwrap(),
        result
    );
    assert_eq!(
        reconnect
            .recover(VerifiedActor::Office01, command.context().operation_id)
            .await,
        Err(WorkError::WorkItemNotFound)
    );
    let mut changed = command.clone();
    if let Command::Submit { artifacts, .. } = &mut changed {
        artifacts.clear();
    }
    assert_eq!(
        repository.execute(VerifiedActor::Sales01, changed).await,
        Err(WorkError::OperationConflict)
    );
    let claim = Command::Claim {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, 0),
    };
    let competing = Command::Claim {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, 0),
    };
    let claim_operation_id = claim.context().operation_id;
    let competing_operation_id = competing.context().operation_id;
    let (first, second) = tokio::join!(
        repository.execute(VerifiedActor::Office01, claim),
        reconnect.execute(VerifiedActor::Office01, competing)
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    for (operation_id, outcome) in [
        (claim_operation_id, first),
        (competing_operation_id, second),
    ] {
        match outcome {
            Ok(result) => assert_eq!(
                repository
                    .recover(VerifiedActor::Office01, operation_id)
                    .await
                    .unwrap(),
                result
            ),
            Err(_) => assert_eq!(
                repository
                    .recover(VerifiedActor::Office01, operation_id)
                    .await,
                Err(WorkError::WorkItemNotFound)
            ),
        }
    }
    assert_eq!(
        repository
            .snapshot(VerifiedActor::Office01, snapshot.id)
            .await
            .unwrap(),
        snapshot
    );
    assert_eq!(
        repository
            .artifact(VerifiedActor::Office01, artifact.id)
            .await,
        Err(WorkError::WorkArtifactNotFound)
    );
}
