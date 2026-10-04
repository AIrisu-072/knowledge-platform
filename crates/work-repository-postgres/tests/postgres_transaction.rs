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
    let migrations_before: Vec<(i64, Vec<u8>, time::OffsetDateTime)> = sqlx::query_as(
        "SELECT version, checksum, applied_at FROM work.schema_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(migrations_before.len(), 2);
    migrate(&pool).await.unwrap();
    let migrations_after: Vec<(i64, Vec<u8>, time::OffsetDateTime)> = sqlx::query_as(
        "SELECT version, checksum, applied_at FROM work.schema_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(migrations_after, migrations_before);
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
    // Observe the complete aggregate (including snapshots), its OCC column and every
    // independently staged record family in one statement before and after rejection.
    let rollback_state_sql = "SELECT body, revision, \
        (SELECT count(*) FROM work.operation_ledger), \
        (SELECT count(*) FROM work.workflow_history), \
        (SELECT count(*) FROM work.event_staging) \
        FROM work.workflow_instances WHERE id = $1";
    let before_failure: (serde_json::Value, i64, i64, i64, i64) =
        sqlx::query_as(rollback_state_sql)
            .bind(WORKFLOW_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::raw_sql("CREATE FUNCTION work.reject_staging() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test staging failure'; END $$; CREATE TRIGGER reject_staging BEFORE INSERT ON work.event_staging FOR EACH ROW EXECUTE FUNCTION work.reject_staging();").execute(&pool).await.unwrap();
    assert_eq!(
        repository
            .execute(VerifiedActor::Sales01, command.clone())
            .await,
        Err(WorkError::DependencyUnavailable)
    );
    let after_failure: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
        .bind(WORKFLOW_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after_failure, before_failure);
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
            Err(error) => {
                assert_eq!(error, WorkError::RevisionConflict);
                assert_eq!(
                    repository
                        .recover(VerifiedActor::Office01, operation_id)
                        .await,
                    Err(WorkError::WorkItemNotFound)
                );
            }
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
    let prior_ledger: Vec<(Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT operation_id,outcome FROM work.operation_ledger ORDER BY operation_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let return_command = Command::Return {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, 1),
        expected_attempt_id: OFFICE_ATTEMPT_ID,
        previous_submission_id: snapshot.id,
        target_task_id: SALES_TASK_ID,
        transition_id: RETURN_TRANSITION_ID,
        reason: "提出内容の再確認をお願いします".into(),
    };
    let before_return_failure: (serde_json::Value, i64, i64, i64, i64) =
        sqlx::query_as(rollback_state_sql)
            .bind(WORKFLOW_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::raw_sql("CREATE FUNCTION work.reject_staging() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test staging failure'; END $$; CREATE TRIGGER reject_staging BEFORE INSERT ON work.event_staging FOR EACH ROW EXECUTE FUNCTION work.reject_staging();")
        .execute(&pool).await.unwrap();
    assert_eq!(
        repository
            .execute(VerifiedActor::Office01, return_command.clone())
            .await,
        Err(WorkError::DependencyUnavailable)
    );
    let after_return_failure: (serde_json::Value, i64, i64, i64, i64) =
        sqlx::query_as(rollback_state_sql)
            .bind(WORKFLOW_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(after_return_failure, before_return_failure);
    sqlx::raw_sql(
        "DROP TRIGGER reject_staging ON work.event_staging; DROP FUNCTION work.reject_staging();",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Same-operation concurrent retries commit one return, one instruction and one
    // new target attempt; the losing caller receives the original committed result.
    let (first, second) = tokio::join!(
        repository.execute(VerifiedActor::Office01, return_command.clone()),
        reconnect.execute(VerifiedActor::Office01, return_command.clone())
    );
    let returned = first.unwrap();
    assert_eq!(returned, second.unwrap());
    let instruction = match &returned {
        MutationResult::Returned {
            return_instruction,
            next_task,
            ..
        } => {
            assert_eq!(next_task.attempt_number, 2);
            return_instruction.clone()
        }
        _ => panic!(),
    };
    assert_eq!(
        reconnect
            .recover(
                VerifiedActor::Office01,
                return_command.context().operation_id
            )
            .await
            .unwrap(),
        returned
    );
    assert_eq!(
        repository
            .return_instruction(VerifiedActor::Sales01, instruction.id)
            .await
            .unwrap(),
        instruction
    );
    let mut changed = return_command.clone();
    if let Command::Return { reason, .. } = &mut changed {
        reason.push('!');
    }
    assert_eq!(
        repository.execute(VerifiedActor::Office01, changed).await,
        Err(WorkError::OperationConflict)
    );
    assert_eq!(
        repository
            .recover(VerifiedActor::Sales01, saved_operation_id)
            .await,
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(
        repository
            .artifact(VerifiedActor::Sales01, artifact.id)
            .await,
        Err(WorkError::WorkArtifactNotFound)
    );
    // Replay uses current immutable submission membership, not the now-unassigned
    // next sales attempt's private-read permission.
    assert_eq!(
        reconnect
            .execute(VerifiedActor::Sales01, command.clone())
            .await
            .unwrap(),
        result
    );
    repository
        .execute(
            VerifiedActor::Sales01,
            Command::Claim {
                task_id: SALES_TASK_ID,
                context: ctx(VerifiedActor::Sales01, 3),
            },
        )
        .await
        .unwrap();
    let new_save = Command::SaveDraft {
        task_id: SALES_TASK_ID,
        artifact_id: None,
        context: ctx(VerifiedActor::Sales01, 4),
        value: TextValue {
            text: "差戻後の新しい非公開文案".into(),
        },
    };
    let new_save_operation = new_save.context().operation_id;
    let new_artifact = match repository
        .execute(VerifiedActor::Sales01, new_save)
        .await
        .unwrap()
    {
        MutationResult::DraftSaved { artifact, .. } => artifact,
        _ => panic!(),
    };
    assert_eq!(
        reconnect
            .artifact(VerifiedActor::Office01, new_artifact.id)
            .await,
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(
        reconnect
            .recover(VerifiedActor::Office01, new_save_operation)
            .await,
        Err(WorkError::WorkItemNotFound)
    );
    assert!(
        !serde_json::to_string(
            &reconnect
                .task(VerifiedActor::Office01, OFFICE_TASK_ID)
                .await
                .unwrap()
        )
        .unwrap()
        .contains("差戻後の新しい非公開文案")
    );
    let resubmit = Command::Submit {
        task_id: SALES_TASK_ID,
        context: ctx(VerifiedActor::Sales01, 5),
        artifacts: vec![ArtifactSelection {
            artifact_id: new_artifact.id,
            revision: 0,
        }],
    };
    let new_snapshot = match repository
        .execute(VerifiedActor::Sales01, resubmit)
        .await
        .unwrap()
    {
        MutationResult::Submitted {
            snapshot,
            next_task,
            ..
        } => {
            assert_eq!(next_task.attempt_number, 2);
            assert_eq!(next_task.revision, 3);
            snapshot
        }
        _ => panic!(),
    };
    assert_eq!(
        reconnect
            .snapshot(VerifiedActor::Office01, new_snapshot.id)
            .await,
        Err(WorkError::WorkArtifactNotFound)
    );
    let mut wrong_actor = return_command.clone();
    if let Command::Return { context, .. } = &mut wrong_actor {
        context.acting_assignment_id = SALES_ASSIGNMENT_ID;
    }
    assert_eq!(
        reconnect.execute(VerifiedActor::Sales01, wrong_actor).await,
        Err(WorkError::WorkItemNotFound)
    );
    // Return remains exactly replayable while office attempt2 is ready, before
    // it is claimed; no private attempt data is disclosed by this old outcome.
    assert_eq!(
        reconnect
            .execute(VerifiedActor::Office01, return_command.clone())
            .await
            .unwrap(),
        returned
    );
    repository
        .execute(
            VerifiedActor::Office01,
            Command::Claim {
                task_id: OFFICE_TASK_ID,
                context: ctx(VerifiedActor::Office01, 3),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        reconnect
            .snapshot(VerifiedActor::Office01, new_snapshot.id)
            .await
            .unwrap(),
        new_snapshot
    );
    assert_eq!(
        reconnect
            .snapshot(VerifiedActor::Office01, snapshot.id)
            .await
            .unwrap(),
        snapshot
    );
    assert_eq!(
        reconnect
            .recover(
                VerifiedActor::Office01,
                return_command.context().operation_id
            )
            .await
            .unwrap(),
        returned
    );
    let persisted: sqlx::types::Json<Workflow> =
        sqlx::query_scalar("SELECT body FROM work.workflow_instances WHERE id=$1")
            .bind(WORKFLOW_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(persisted.completed_attempts.len(), 2);
    assert_eq!(persisted.return_instructions, vec![instruction]);
    assert_eq!(persisted.snapshots.len(), 2);
    persisted.validate_integrity().unwrap();
    let prior_ids: Vec<Uuid> = prior_ledger.iter().map(|(id, _)| *id).collect();
    let preserved_ledger: Vec<(Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT operation_id,outcome FROM work.operation_ledger WHERE operation_id=ANY($1) ORDER BY operation_id"
    ).bind(prior_ids).fetch_all(&pool).await.unwrap();
    assert_eq!(preserved_ledger, prior_ledger);
    let returned_count: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM work.operation_ledger WHERE operation_id=$1), \
         (SELECT count(*) FROM work.workflow_history WHERE kind='returned'), \
         (SELECT count(*) FROM work.event_staging WHERE action='returned')",
    )
    .bind(return_command.context().operation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(returned_count, (1, 1, 1));
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM work.event_staging WHERE action='returned'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!payload.to_string().contains("提出内容の再確認"));
}
