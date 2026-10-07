//! Explicit opt-in test: requires a disposable *_work_poc_test PostgreSQL database.
//! This test is never replaced with a fake database or an alternate socket.
use sqlx::postgres::PgPoolOptions;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use uuid::Uuid;
use work_application::{EvidenceSourcePort, EvidenceSourcePurpose, WorkFuture, WorkRepository};
const EVIDENCE_DOCUMENT: Uuid = Uuid::from_u128(0x01900000000070008000000000000071);
struct TestEvidenceSource {
    pool: sqlx::PgPool,
    allowed: AtomicBool,
    reject_next_read: AtomicBool,
    calls: AtomicUsize,
}
impl EvidenceSourcePort for TestEvidenceSource {
    fn authorize(
        &self,
        _actor: VerifiedActor,
        source: EvidenceSource,
        purpose: EvidenceSourcePurpose,
    ) -> WorkFuture<'_, ()> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            // A provider invocation must never occur while the caller owns the Work row lock.
            sqlx::query("SELECT id FROM work.workflow_instances WHERE id=$1 FOR UPDATE NOWAIT")
                .bind(WORKFLOW_ID)
                .fetch_one(&self.pool)
                .await
                .map_err(|_| WorkError::IntegrityViolation)?;
            if !self.allowed.load(Ordering::SeqCst)
                || source.source_ref.resource_id != EVIDENCE_DOCUMENT
                || (purpose == EvidenceSourcePurpose::ReadHistory
                    && self.reject_next_read.swap(false, Ordering::SeqCst))
            {
                return Err(WorkError::EvidenceNotFound);
            }
            Ok(())
        })
    }
}

struct TestAgentSource {
    pool: sqlx::PgPool,
    requester_allowed: AtomicBool,
    provider_allowed: AtomicBool,
    deny_call: AtomicUsize,
    calls: AtomicUsize,
}
impl work_application::AgentSourcePort for TestAgentSource {
    fn authorize(
        &self,
        context: AgentDispatchContext,
        reference: RevisionRef,
        remaining: std::time::Duration,
    ) -> WorkFuture<'_, ()> {
        Box::pin(async move {
            context.validate_scope()?;
            assert!(remaining <= std::time::Duration::from_secs(5));
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            sqlx::query("SELECT id FROM work.workflow_instances WHERE id=$1 FOR UPDATE NOWAIT")
                .bind(WORKFLOW_ID)
                .fetch_one(&self.pool)
                .await
                .map_err(|_| WorkError::IntegrityViolation)?;
            if self.deny_call.load(Ordering::SeqCst) == call
                || !self.requester_allowed.load(Ordering::SeqCst)
                || !self.provider_allowed.load(Ordering::SeqCst)
                || !context
                    .execution
                    .evidence_revision_refs
                    .contains(&reference)
            {
                return Err(WorkError::EvidenceNotFound);
            }
            Ok(())
        })
    }
}

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
    assert_eq!(migrations_before.len(), 7);
    migrate(&pool).await.unwrap();
    let migrations_after: Vec<(i64, Vec<u8>, time::OffsetDateTime)> = sqlx::query_as(
        "SELECT version, checksum, applied_at FROM work.schema_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(migrations_after, migrations_before);
    // The two HTTP profiles start before seed-work creates the fixture.
    // Startup/shutdown interruption alone accepts a genuinely empty Work schema.
    let unseeded = PostgresWorkRepository::new(pool.clone());
    let empty_counts = "SELECT (SELECT count(*) FROM work.workflow_instances), (SELECT count(*) FROM work.operation_ledger), (SELECT count(*) FROM work.event_staging)";
    let before: (i64, i64, i64) = sqlx::query_as(empty_counts).fetch_one(&pool).await.unwrap();
    assert_eq!(before, (0, 0, 0));
    for actor in [VerifiedActor::Sales01, VerifiedActor::Office01] {
        assert_eq!(unseeded.interrupt_agent_executions(actor).await.unwrap(), 0);
    }
    let after: (i64, i64, i64) = sqlx::query_as(empty_counts).fetch_one(&pool).await.unwrap();
    assert_eq!(
        after, before,
        "empty interruption must not seed or stage anything"
    );
    assert_eq!(
        unseeded.task(VerifiedActor::Sales01, SALES_TASK_ID).await,
        Err(WorkError::DependencyUnavailable),
        "ordinary reads still require the fixture"
    );
    seed_synthetic(&pool, Some(EVIDENCE_DOCUMENT))
        .await
        .unwrap();
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
        expected_attempt_id: None,
        evidence_revision_refs: vec![],
        finding_revision_refs: vec![],
        decision_revision_refs: vec![],
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
        expected_attempt_id: None,
        evidence_revision_refs: vec![],
        finding_revision_refs: vec![],
        decision_revision_refs: vec![],
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
    // Continue the same disposable transaction fixture: evidence uses the same
    // staging/ledger/OCC boundaries, never a second runner or local database.
    let source = Arc::new(TestEvidenceSource {
        pool: pool.clone(),
        allowed: AtomicBool::new(true),
        reject_next_read: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
    });
    let evidence_repository =
        PostgresWorkRepository::with_evidence_source(pool.clone(), source.clone());
    let current = evidence_repository
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let registration = Command::RegisterEvidence {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        source: EvidenceSource {
            source_ref: SourceRef {
                provider_id: "document".into(),
                resource_id: EVIDENCE_DOCUMENT,
                revision_id: Uuid::from_u128(72),
                version_id: Uuid::from_u128(73),
            },
            authoritative_locator: AuthoritativeLocator {
                kind: "contentItem".into(),
                content_item_id: Uuid::from_u128(74),
                representation_id: Uuid::from_u128(75),
            },
        },
        relevant_location: "人間が選択した該当箇所".into(),
    };
    assert_eq!(
        repository
            .execute(VerifiedActor::Office01, registration.clone())
            .await,
        Err(WorkError::DependencyUnavailable),
        "missing provider must fail closed before committing"
    );
    let before: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
        .bind(WORKFLOW_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION work.reject_staging() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test staging failure'; END $$; CREATE TRIGGER reject_staging BEFORE INSERT ON work.event_staging FOR EACH ROW EXECUTE FUNCTION work.reject_staging();").execute(&pool).await.unwrap();
    assert_eq!(
        evidence_repository
            .execute(VerifiedActor::Office01, registration.clone())
            .await,
        Err(WorkError::DependencyUnavailable)
    );
    let after: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
        .bind(WORKFLOW_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        before, after,
        "evidence row, ledger and staging roll back together"
    );
    sqlx::raw_sql(
        "DROP TRIGGER reject_staging ON work.event_staging; DROP FUNCTION work.reject_staging();",
    )
    .execute(&pool)
    .await
    .unwrap();
    source.reject_next_read.store(true, Ordering::SeqCst);
    assert_eq!(
        evidence_repository
            .execute(VerifiedActor::Office01, registration.clone())
            .await,
        Err(WorkError::CommitOutcomeUnknown),
        "post-commit provider denial cannot disclose stale result"
    );
    let saved = evidence_repository
        .recover(VerifiedActor::Office01, registration.context().operation_id)
        .await
        .unwrap();
    assert_eq!(
        evidence_repository
            .execute(VerifiedActor::Office01, registration.clone())
            .await
            .unwrap(),
        saved
    );
    let ev = match &saved {
        MutationResult::EvidenceRegistered { evidence, .. } => evidence.clone(),
        _ => panic!(),
    };
    assert_eq!(ev.revision, 1);
    assert_eq!(ev.origin, "human");
    assert_eq!(ev.fragment_omission_reason, "not_retained");
    let mut changed = registration.clone();
    if let Command::RegisterEvidence {
        relevant_location, ..
    } = &mut changed
    {
        *relevant_location = "different request".into();
    }
    assert_eq!(
        evidence_repository
            .execute(VerifiedActor::Office01, changed)
            .await,
        Err(WorkError::OperationConflict)
    );
    let calls = source.calls.load(Ordering::SeqCst);
    assert_eq!(
        evidence_repository
            .evidence(VerifiedActor::Sales01, ev.id)
            .await,
        Err(WorkError::EvidenceNotFound)
    );
    assert_eq!(
        source.calls.load(Ordering::SeqCst),
        calls,
        "Work scope denies before provider fanout"
    );
    let current = evidence_repository
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let finding_command = Command::RegisterFinding {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        claim: "根拠を分けて保存する候補".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: ev.id,
            revision: 1,
        }],
        supersedes_finding_id: None,
    };
    let f = match evidence_repository
        .execute(VerifiedActor::Office01, finding_command.clone())
        .await
        .unwrap()
    {
        MutationResult::FindingRegistered { finding, .. } => finding,
        _ => panic!(),
    };
    let current = evidence_repository
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let decision_command = Command::RecordDecision {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        finding_id: f.id,
        finding_revision: 1,
        decision: DecisionKind::Modified,
        adopted_claim: Some("人間が修正して採用した文".into()),
        reason: Some("照合結果".into()),
        evidence_revision_refs: vec![RevisionRef {
            id: ev.id,
            revision: 1,
        }],
        supersedes_decision_id: None,
    };
    let decision = evidence_repository
        .execute(VerifiedActor::Office01, decision_command.clone())
        .await
        .unwrap();
    assert_eq!(
        evidence_repository
            .execute(VerifiedActor::Office01, decision_command.clone())
            .await
            .unwrap(),
        decision
    );
    let f_after = evidence_repository
        .finding(VerifiedActor::Office01, f.id)
        .await
        .unwrap();
    assert_eq!(f_after, f, "human decision cannot rewrite the candidate");
    let persisted: sqlx::types::Json<Workflow> =
        sqlx::query_scalar("SELECT body FROM work.workflow_instances WHERE id=$1")
            .bind(WORKFLOW_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        persisted.validate_selection(
            VerifiedActor::Office01,
            OFFICE_TASK_ID,
            &[],
            &[RevisionRef {
                id: f.id,
                revision: 1
            }],
            &[]
        ),
        Err(WorkError::HandoffNotReady)
    );
    source.allowed.store(false, Ordering::SeqCst);
    assert_eq!(
        evidence_repository
            .list_evidence(VerifiedActor::Office01, OFFICE_TASK_ID)
            .await,
        Err(WorkError::EvidenceNotFound)
    );
    assert_eq!(
        evidence_repository
            .finding(VerifiedActor::Office01, f.id)
            .await,
        Err(WorkError::EvidenceNotFound)
    );
    assert_eq!(
        evidence_repository
            .list_decisions(VerifiedActor::Office01, f.id)
            .await,
        Err(WorkError::EvidenceNotFound)
    );
    assert_eq!(
        evidence_repository
            .recover(
                VerifiedActor::Office01,
                decision_command.context().operation_id
            )
            .await,
        Err(WorkError::CommitOutcomeUnknown)
    );
    source.allowed.store(true, Ordering::SeqCst);
    let reconnected = PostgresWorkRepository::with_evidence_source(pool.clone(), source.clone());
    assert_eq!(
        reconnected
            .recover(
                VerifiedActor::Office01,
                decision_command.context().operation_id
            )
            .await
            .unwrap(),
        decision
    );
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM work.operation_ledger WHERE operation_id=$1),(SELECT count(*) FROM work.event_staging WHERE operation_id=$1),(SELECT count(*) FROM work.workflow_history WHERE operation_id=$1)").bind(decision_command.context().operation_id).fetch_one(&pool).await.unwrap();
    assert_eq!(
        counts,
        (1, 1, 0),
        "one ledger/staging record, no fake workflow transition"
    );
    // Same opt-in real transaction trial: no new database, runner or listener.
    let provider = Arc::new(TestAgentSource {
        pool: pool.clone(),
        requester_allowed: AtomicBool::new(true),
        provider_allowed: AtomicBool::new(true),
        deny_call: AtomicUsize::new(0),
        calls: AtomicUsize::new(0),
    });
    let agents =
        PostgresWorkRepository::with_agent_source(pool.clone(), source.clone(), provider.clone());
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let command = Command::RequestAgentExecution {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        purpose: "合成・本文分析なし".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: ev.id,
            revision: 1,
        }],
    };
    let before: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
        .bind(WORKFLOW_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    for requester_side in [true, false] {
        let flag = if requester_side {
            &provider.requester_allowed
        } else {
            &provider.provider_allowed
        };
        flag.store(false, Ordering::SeqCst);
        assert_eq!(
            agents
                .request_agent_execution(VerifiedActor::Office01, command.clone())
                .await,
            Err(WorkError::EvidenceNotFound)
        );
        flag.store(true, Ordering::SeqCst);
        let after: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
            .bind(WORKFLOW_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before, after);
    }
    let accepted = agents
        .request_agent_execution(VerifiedActor::Office01, command.clone())
        .await
        .unwrap();
    assert!(accepted.dispatch);
    let replay = agents
        .request_agent_execution(VerifiedActor::Office01, command.clone())
        .await
        .unwrap();
    assert!(!replay.dispatch);
    assert_eq!(replay.outcome, accepted.outcome);
    let mut changed = command.clone();
    if let Command::RequestAgentExecution { purpose, .. } = &mut changed {
        *purpose = "別の目的".into();
    }
    assert_eq!(
        agents
            .request_agent_execution(VerifiedActor::Office01, changed)
            .await,
        Err(WorkError::OperationConflict)
    );
    let execution = match &accepted.outcome {
        MutationResult::AgentExecutionRequested { execution, .. } => execution.clone(),
        _ => panic!(),
    };
    let calls = provider.calls.load(Ordering::SeqCst);
    assert_eq!(
        agents
            .agent_execution(VerifiedActor::Sales01, execution.id)
            .await,
        Err(WorkError::WorkItemNotFound)
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), calls);
    let dispatch = agents
        .start_agent_execution(VerifiedActor::Office01, execution.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        agents
            .start_agent_execution(VerifiedActor::Office01, execution.id)
            .await
            .unwrap()
            .is_none()
    );
    let output = AgentFindingOutput {
        summary: "合成実行・本文分析なし".into(),
        claim: "人間が原本を確認してください".into(),
        uncertainty: vec!["実LLM/MCP通信なし".into()],
    };
    let before: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
        .bind(WORKFLOW_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION work.reject_staging() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test staging failure'; END $$; CREATE TRIGGER reject_staging BEFORE INSERT ON work.event_staging FOR EACH ROW EXECUTE FUNCTION work.reject_staging();").execute(&pool).await.unwrap();
    assert_eq!(
        agents
            .finish_agent_execution(dispatch.clone(), output.clone())
            .await,
        Err(WorkError::DependencyUnavailable)
    );
    let after: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
        .bind(WORKFLOW_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        before, after,
        "Finding, terminal result and staging rollback together"
    );
    sqlx::raw_sql(
        "DROP TRIGGER reject_staging ON work.event_staging; DROP FUNCTION work.reject_staging();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let done = agents
        .finish_agent_execution(dispatch, output.clone())
        .await
        .unwrap();
    assert_eq!(done.status, AgentExecutionStatus::Succeeded);
    let result = agents
        .agent_result(VerifiedActor::Office01, execution.id)
        .await
        .unwrap();
    assert_eq!(result.finding_revision_refs.len(), 1);
    assert!(result.simulated);
    assert!(!result.body_analyzed);
    let finding = agents
        .finding(VerifiedActor::Office01, result.finding_revision_refs[0].id)
        .await
        .unwrap();
    assert_eq!(finding.author, SYNTHETIC_EXECUTOR);
    assert_eq!(finding.origin_execution_id, Some(execution.id));
    assert!(
        agents
            .list_decisions(VerifiedActor::Office01, finding.id)
            .await
            .unwrap()
            .is_empty()
    );
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    agents
        .execute(
            VerifiedActor::Office01,
            Command::RecordDecision {
                task_id: OFFICE_TASK_ID,
                context: ctx(VerifiedActor::Office01, current.task.revision),
                expected_attempt_id: current.task.attempt_id,
                finding_id: finding.id,
                finding_revision: 1,
                decision: DecisionKind::Accepted,
                adopted_claim: None,
                reason: None,
                evidence_revision_refs: finding.evidence_revision_refs.clone(),
                supersedes_decision_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        agents
            .agent_result(VerifiedActor::Office01, execution.id)
            .await
            .unwrap(),
        result,
        "same-attempt Human decision does not erase terminal result"
    );

    for requester_side in [true, false] {
        let flag = if requester_side {
            &provider.requester_allowed
        } else {
            &provider.provider_allowed
        };
        flag.store(false, Ordering::SeqCst);
        assert_eq!(
            agents
                .agent_result(VerifiedActor::Office01, execution.id)
                .await,
            Err(WorkError::EvidenceNotFound)
        );
        assert_eq!(
            agents.finding(VerifiedActor::Office01, finding.id).await,
            Err(WorkError::EvidenceNotFound)
        );
        assert_eq!(
            agents
                .recover(VerifiedActor::Office01, command.context().operation_id)
                .await,
            Err(WorkError::CommitOutcomeUnknown)
        );
        flag.store(true, Ordering::SeqCst);
    }
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    agents
        .execute(
            VerifiedActor::Office01,
            Command::CancelAgentExecution {
                task_id: OFFICE_TASK_ID,
                context: ctx(VerifiedActor::Office01, current.task.revision),
                expected_attempt_id: current.task.attempt_id,
                execution_id: execution.id,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        agents
            .agent_execution(VerifiedActor::Office01, execution.id)
            .await
            .unwrap()
            .status,
        AgentExecutionStatus::Succeeded
    );
    let reconnected =
        PostgresWorkRepository::with_agent_source(pool.clone(), source.clone(), provider.clone());
    assert_eq!(
        reconnected
            .agent_result(VerifiedActor::Office01, execution.id)
            .await
            .unwrap(),
        result
    );
    assert_eq!(
        reconnected
            .recover(VerifiedActor::Office01, command.context().operation_id)
            .await
            .unwrap(),
        accepted.outcome
    );
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let second = Command::RequestAgentExecution {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        purpose: "取消対象".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: ev.id,
            revision: 1,
        }],
    };
    let accepted = agents
        .request_agent_execution(VerifiedActor::Office01, second)
        .await
        .unwrap();
    let execution = match accepted.outcome {
        MutationResult::AgentExecutionRequested { execution, .. } => execution,
        _ => panic!(),
    };
    let running = agents
        .start_agent_execution(VerifiedActor::Office01, execution.id)
        .await
        .unwrap()
        .unwrap();
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    provider.provider_allowed.store(false, Ordering::SeqCst);
    assert_eq!(
        agents
            .execute(
                VerifiedActor::Office01,
                Command::CancelAgentExecution {
                    task_id: OFFICE_TASK_ID,
                    context: ctx(VerifiedActor::Office01, current.task.revision),
                    expected_attempt_id: current.task.attempt_id,
                    execution_id: execution.id,
                },
            )
            .await,
        Err(WorkError::CommitOutcomeUnknown)
    );
    provider.provider_allowed.store(true, Ordering::SeqCst);
    assert_eq!(
        agents.finish_agent_execution(running, output).await,
        Err(WorkError::WorkContextStale)
    );
    assert_eq!(
        agents
            .agent_result(VerifiedActor::Office01, execution.id)
            .await,
        Err(WorkError::AgentResultNotReady)
    );
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let third = Command::RequestAgentExecution {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        purpose: "再起動対象".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: ev.id,
            revision: 1,
        }],
    };
    let accepted = agents
        .request_agent_execution(VerifiedActor::Office01, third)
        .await
        .unwrap();
    let execution = match accepted.outcome {
        MutationResult::AgentExecutionRequested { execution, .. } => execution,
        _ => panic!(),
    };
    assert_eq!(
        agents
            .interrupt_agent_executions(VerifiedActor::Sales01)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        agents
            .interrupt_agent_executions(VerifiedActor::Office01)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        agents
            .agent_execution(VerifiedActor::Office01, execution.id)
            .await
            .unwrap()
            .status,
        AgentExecutionStatus::OutcomeUnknown
    );
    assert!(
        agents
            .start_agent_execution(VerifiedActor::Office01, execution.id)
            .await
            .unwrap()
            .is_none()
    );
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let undisclosed = Command::RequestAgentExecution {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        purpose: "受付応答前の認可喪失".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: ev.id,
            revision: 1,
        }],
    };
    provider
        .deny_call
        .store(provider.calls.load(Ordering::SeqCst) + 2, Ordering::SeqCst);
    assert_eq!(
        agents
            .request_agent_execution(VerifiedActor::Office01, undisclosed.clone())
            .await,
        Err(WorkError::CommitOutcomeUnknown)
    );
    let current = agents
        .agent_execution(VerifiedActor::Office01, undisclosed.context().operation_id)
        .await
        .unwrap();
    assert_eq!(
        current.status,
        AgentExecutionStatus::Failed,
        "known committed but undispatched request cannot remain an active queue"
    );
    let replay = agents
        .request_agent_execution(VerifiedActor::Office01, undisclosed)
        .await
        .unwrap();
    assert!(!replay.dispatch);
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let hold_pending = agents
        .request_agent_execution(
            VerifiedActor::Office01,
            Command::RequestAgentExecution {
                task_id: OFFICE_TASK_ID,
                context: ctx(VerifiedActor::Office01, current.task.revision),
                expected_attempt_id: current.task.attempt_id,
                purpose: "保留と再開をまたぐ古い結果を拒否".into(),
                evidence_revision_refs: vec![RevisionRef {
                    id: ev.id,
                    revision: 1,
                }],
            },
        )
        .await
        .unwrap();
    assert!(hold_pending.dispatch);
    let hold_pending_id = match hold_pending.outcome {
        MutationResult::AgentExecutionRequested { execution, .. } => execution.id,
        _ => panic!(),
    };
    let hold_context = agents
        .start_agent_execution(VerifiedActor::Office01, hold_pending_id)
        .await
        .unwrap()
        .unwrap();
    // Hold/resume reuse one atomic operation ledger/history/staging boundary.
    // Every failed staging insert leaves state and all private/immutable values untouched.
    for (action, expected_state, action_id) in [
        (
            "hold",
            "held",
            Uuid::from_u128(0x01900000000070008000000000000014),
        ),
        (
            "resume",
            "active",
            Uuid::from_u128(0x01900000000070008000000000000015),
        ),
    ] {
        let current = agents
            .task(VerifiedActor::Office01, OFFICE_TASK_ID)
            .await
            .unwrap();
        let command: Command = serde_json::from_value(serde_json::json!({
            "kind":action,"task_id":OFFICE_TASK_ID,
            "context":ctx(VerifiedActor::Office01,current.task.revision),
            "expected_attempt_id":current.task.attempt_id,"definition_action_id":action_id
        }))
        .unwrap();
        let before: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
            .bind(WORKFLOW_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::raw_sql("CREATE FUNCTION work.reject_staging() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test staging failure'; END $$; CREATE TRIGGER reject_staging BEFORE INSERT ON work.event_staging FOR EACH ROW EXECUTE FUNCTION work.reject_staging();").execute(&pool).await.unwrap();
        assert!(
            agents
                .execute(VerifiedActor::Office01, command.clone())
                .await
                .is_err()
        );
        let failed: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
            .bind(WORKFLOW_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            failed, before,
            "{action} state/ledger/history/staging rollback"
        );
        sqlx::raw_sql("DROP TRIGGER reject_staging ON work.event_staging; DROP FUNCTION work.reject_staging();").execute(&pool).await.unwrap();
        let outcome = agents
            .execute(VerifiedActor::Office01, command.clone())
            .await
            .unwrap();
        let value = serde_json::to_value(&outcome).unwrap();
        let kind = if action == "hold" { "held" } else { "resumed" };
        assert_eq!(value["kind"], kind);
        assert_eq!(value["task"]["state"], expected_state);
        assert_eq!(
            agents
                .execute(VerifiedActor::Office01, command.clone())
                .await
                .unwrap(),
            outcome
        );
        let restored = PostgresWorkRepository::with_agent_source(
            pool.clone(),
            source.clone(),
            provider.clone(),
        );
        assert_eq!(
            restored
                .recover(VerifiedActor::Office01, command.context().operation_id)
                .await
                .unwrap(),
            outcome
        );
        // A previous committed evidence command remains recoverable/replayable while held.
        // This does not adopt its old task projection as current mutable state.
        assert_eq!(
            restored
                .recover(VerifiedActor::Office01, registration.context().operation_id)
                .await
                .unwrap(),
            saved
        );
        assert_eq!(
            restored
                .execute(VerifiedActor::Office01, registration.clone())
                .await
                .unwrap(),
            saved
        );

        assert_eq!(
            restored
                .recover(VerifiedActor::Sales01, command.context().operation_id)
                .await,
            Err(WorkError::WorkItemNotFound)
        );
        let after: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
            .bind(WORKFLOW_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
        for key in [
            "source",
            "snapshots",
            "completedAttempts",
            "returnInstructions",
            "artifacts",
            "evidence",
            "findings",
            "decisions",
        ] {
            assert_eq!(before.0[key], after.0[key], "{action} preserves {key}");
        }
        assert_eq!(
            restored
                .agent_execution(VerifiedActor::Office01, hold_pending_id)
                .await
                .unwrap()
                .status,
            AgentExecutionStatus::Failed
        );
        assert!(
            restored
                .start_agent_execution(VerifiedActor::Office01, hold_pending_id)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            restored
                .finish_agent_execution(
                    hold_context.clone(),
                    AgentFindingOutput {
                        summary: "合成実行".into(),
                        claim: "古い出力".into(),
                        uncertainty: vec!["本文分析なし".into()],
                    }
                )
                .await,
            Err(WorkError::WorkContextStale)
        );
        if action == "resume" {
            assert_eq!(before.0["agentExecutions"], after.0["agentExecutions"]);
        }
        for key in [
            "attemptId",
            "attemptNumber",
            "workAssignmentId",
            "actingAssignmentId",
            "assignee",
            "handoffSnapshotId",
            "completedAt",
        ] {
            assert_eq!(
                before.0["next"][key], after.0["next"][key],
                "{action} preserves {key}"
            );
        }
        let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM work.operation_ledger WHERE operation_id=$1), (SELECT count(*) FROM work.workflow_history WHERE operation_id=$1 AND kind=$2), (SELECT count(*) FROM work.event_staging WHERE operation_id=$1 AND action=$2)").bind(command.context().operation_id).bind(kind).fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (1, 1, 1));
        let mut changed = serde_json::to_value(&command).unwrap();
        changed["definition_action_id"] = serde_json::json!(COMPLETE_ACTION_ID);
        assert_eq!(
            restored
                .execute(
                    VerifiedActor::Office01,
                    serde_json::from_value(changed).unwrap()
                )
                .await,
            Err(WorkError::OperationConflict)
        );
        let mut stale = serde_json::to_value(&command).unwrap();
        stale["context"]["operationId"] = serde_json::json!(Uuid::now_v7());
        assert_eq!(
            restored
                .execute(
                    VerifiedActor::Office01,
                    serde_json::from_value(stale).unwrap()
                )
                .await,
            Err(WorkError::RevisionConflict)
        );
    }
    // A final Human completion uses the same transaction and operation ledger.
    // Pending Agent output is fenced by the same atomic close, without editing
    // submitted membership or archived attempts.
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let pending_command = Command::RequestAgentExecution {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        purpose: "完了後の遅い結果を拒否".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: ev.id,
            revision: 1,
        }],
    };
    let pending = agents
        .request_agent_execution(VerifiedActor::Office01, pending_command)
        .await
        .unwrap();
    let pending_id = match pending.outcome {
        MutationResult::AgentExecutionRequested { execution, .. } => execution.id,
        _ => panic!(),
    };
    let late_context = agents
        .start_agent_execution(VerifiedActor::Office01, pending_id)
        .await
        .unwrap()
        .unwrap();
    let current = agents
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    assert!(current.task.can_complete);
    let complete = Command::Complete {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        definition_action_id: current.task.completion_action_id.unwrap(),
    };
    let before: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
        .bind(WORKFLOW_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION work.reject_staging() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test staging failure'; END $$; CREATE TRIGGER reject_staging BEFORE INSERT ON work.event_staging FOR EACH ROW EXECUTE FUNCTION work.reject_staging();").execute(&pool).await.unwrap();
    assert!(
        agents
            .execute(VerifiedActor::Office01, complete.clone())
            .await
            .is_err()
    );
    let after: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
        .bind(WORKFLOW_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        after, before,
        "completion, running Agent invalidation, ledger and staging roll back together"
    );
    sqlx::raw_sql(
        "DROP TRIGGER reject_staging ON work.event_staging; DROP FUNCTION work.reject_staging();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let completed = agents
        .execute(VerifiedActor::Office01, complete.clone())
        .await
        .unwrap();
    let serialized = serde_json::to_value(&completed).unwrap();
    assert_eq!(serialized["kind"], "completed");
    assert_eq!(serialized["task"]["state"], "completed");
    assert_eq!(serialized["task"]["canComplete"], false);
    assert_eq!(
        agents
            .execute(VerifiedActor::Office01, complete.clone())
            .await
            .unwrap(),
        completed
    );
    let restored =
        PostgresWorkRepository::with_agent_source(pool.clone(), source.clone(), provider.clone());
    assert_eq!(
        restored
            .recover(VerifiedActor::Office01, complete.context().operation_id)
            .await
            .unwrap(),
        completed
    );
    assert_eq!(
        restored
            .task(VerifiedActor::Office01, OFFICE_TASK_ID)
            .await
            .unwrap()
            .task
            .state,
        TaskState::Completed
    );
    assert_eq!(
        restored
            .recover(VerifiedActor::Sales01, complete.context().operation_id)
            .await,
        Err(WorkError::WorkItemNotFound)
    );
    let after: (serde_json::Value, i64, i64, i64, i64) = sqlx::query_as(rollback_state_sql)
        .bind(WORKFLOW_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    for key in [
        "source",
        "snapshots",
        "completedAttempts",
        "returnInstructions",
        "artifacts",
        "evidence",
        "findings",
        "decisions",
    ] {
        assert_eq!(before.0[key], after.0[key], "completion preserves {key}");
    }
    let counts: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM work.operation_ledger WHERE operation_id=$1), (SELECT count(*) FROM work.workflow_history WHERE operation_id=$1 AND kind='completed'), (SELECT count(*) FROM work.event_staging WHERE operation_id=$1 AND action='completed')").bind(complete.context().operation_id).fetch_one(&pool).await.unwrap();
    assert_eq!(counts, (1, 1, 1));
    assert_eq!(
        restored
            .agent_execution(VerifiedActor::Office01, pending_id)
            .await
            .unwrap()
            .status,
        AgentExecutionStatus::Failed
    );
    assert_eq!(
        restored
            .finish_agent_execution(
                late_context,
                AgentFindingOutput {
                    summary: "合成実行".into(),
                    claim: "遅い候補".into(),
                    uncertainty: vec!["本文分析なし".into()]
                }
            )
            .await,
        Err(WorkError::WorkContextStale)
    );
    let mut changed = complete.clone();
    if let Command::Complete {
        definition_action_id,
        ..
    } = &mut changed
    {
        *definition_action_id = RETURN_TRANSITION_ID;
    }
    assert_eq!(
        restored.execute(VerifiedActor::Office01, changed).await,
        Err(WorkError::OperationConflict)
    );
    let current = restored
        .task(VerifiedActor::Office01, OFFICE_TASK_ID)
        .await
        .unwrap();
    let new_operation = Command::Complete {
        task_id: OFFICE_TASK_ID,
        context: ctx(VerifiedActor::Office01, current.task.revision),
        expected_attempt_id: current.task.attempt_id,
        definition_action_id: COMPLETE_ACTION_ID,
    };
    assert_eq!(
        restored
            .execute(VerifiedActor::Office01, new_operation)
            .await,
        Err(WorkError::HandoffNotReady)
    );
    source.allowed.store(false, Ordering::SeqCst);
    assert_eq!(
        restored.evidence(VerifiedActor::Office01, ev.id).await,
        Err(WorkError::EvidenceNotFound)
    );
    source.allowed.store(true, Ordering::SeqCst);
}

async fn disposable_pool() -> sqlx::PgPool {
    let url =
        std::env::var("WORK_POC_TEST_DATABASE_URL").expect("disposable database URL required");
    let pool = PgPoolOptions::new()
        .max_connections(6)
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
    pool
}
fn acting(acting_assignment_id: Uuid, revision: i64) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id,
    }
}
async fn office_revision(repository: &PostgresWorkRepository, actor: VerifiedActor) -> TaskSummary {
    repository
        .list_tasks(actor, TaskView::Queue)
        .await
        .unwrap()
        .into_iter()
        .find(|task| task.id == OFFICE_TASK_ID)
        .unwrap()
}

/// Multiple principals against one real PostgreSQL: concurrent claims, bounded
/// delegation, reassignment, and policy-writer fencing of in-flight Work commands.
/// Runs after the fresh-schema journey above and leaves the schema absent again.
#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL database"]
async fn organization_policy_fences_concurrent_claims_delegation_and_revocation() {
    let pool = disposable_pool().await;
    sqlx::raw_sql("DROP SCHEMA IF EXISTS work CASCADE")
        .execute(&pool)
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    seed_synthetic(&pool, None).await.unwrap();
    seed_synthetic(&pool, None).await.unwrap();
    let repository = PostgresWorkRepository::new(pool.clone());
    // Sales submits; the office attempt becomes ready for every processing responsibility.
    let MutationResult::DraftSaved { artifact, .. } = repository
        .execute(
            VerifiedActor::Sales01,
            Command::SaveDraft {
                task_id: SALES_TASK_ID,
                artifact_id: None,
                context: acting(SALES_ASSIGNMENT_ID, 0),
                value: TextValue {
                    text: "複数担当の検証文案".into(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    repository
        .execute(
            VerifiedActor::Sales01,
            Command::Submit {
                task_id: SALES_TASK_ID,
                context: acting(SALES_ASSIGNMENT_ID, 1),
                expected_attempt_id: Some(SALES_ATTEMPT_ID),
                artifacts: vec![ArtifactSelection {
                    artifact_id: artifact.id,
                    revision: artifact.revision,
                }],
                evidence_revision_refs: vec![],
                finding_revision_refs: vec![],
                decision_revision_refs: vec![],
            },
        )
        .await
        .unwrap();

    // Office delegates its processing responsibility for a bounded period.
    let view = repository
        .organization(VerifiedActor::Office01)
        .await
        .unwrap();
    assert!(!view.can_manage);
    let until = (time::OffsetDateTime::now_utc() + time::Duration::hours(2))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let delegation_command = PolicyCommand::CreateDelegation {
        context: acting(OFFICE_ASSIGNMENT_ID, view.policy_revision),
        source_assignment_id: OFFICE_ASSIGNMENT_ID,
        recipient: VerifiedActor::Delegate01,
        actions: vec![
            PolicyAction::QueueRead,
            PolicyAction::WorkRead,
            PolicyAction::WorkClaim,
            PolicyAction::WorkComplete,
        ],
        valid_from: None,
        valid_until: until,
        reason: "代理対応".into(),
    };
    let created = repository
        .execute_policy(VerifiedActor::Office01, delegation_command.clone())
        .await
        .unwrap();
    let MutationResult::DelegationCreated { delegation, .. } = created.clone() else {
        panic!()
    };
    // Exact replay returns the committed receipt; a changed payload conflicts.
    assert_eq!(
        repository
            .execute_policy(VerifiedActor::Office01, delegation_command.clone())
            .await
            .unwrap(),
        created
    );
    let mut changed = delegation_command.clone();
    if let PolicyCommand::CreateDelegation { reason, .. } = &mut changed {
        *reason = "変更".into();
    }
    assert_eq!(
        repository
            .execute_policy(VerifiedActor::Office01, changed)
            .await,
        Err(WorkError::OperationConflict)
    );
    let policy_operation = delegation_command.context().operation_id;
    assert_eq!(
        repository
            .recover(VerifiedActor::Office01, policy_operation)
            .await
            .unwrap(),
        created
    );
    assert_eq!(
        repository
            .recover(VerifiedActor::Sales01, policy_operation)
            .await,
        Err(WorkError::WorkItemNotFound)
    );
    let staged: (Option<Uuid>, Option<Uuid>, Option<Uuid>, String, serde_json::Value) =
        sqlx::query_as("SELECT workflow_id, policy_id, task_id, action, payload FROM work.event_staging WHERE operation_id=$1")
            .bind(policy_operation)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(staged.0, None);
    assert_eq!(staged.1, Some(ORGANIZATION_POLICY_ID));
    assert_eq!(staged.2, None);
    assert_eq!(staged.3, "delegation_created");
    assert_eq!(staged.4["recipientPrincipalId"], "delegate-01");
    assert!(!staged.4.to_string().contains("代理対応"));

    // Two eligible principals claim the same revision concurrently: one winner.
    let ready = office_revision(&repository, VerifiedActor::Office01).await;
    assert!(ready.can_claim);
    let delegate_view = office_revision(&repository, VerifiedActor::Delegate01).await;
    assert_eq!(delegate_view.claim_assignment_id, Some(delegation.id));
    let office_claim = Command::Claim {
        task_id: OFFICE_TASK_ID,
        context: acting(OFFICE_ASSIGNMENT_ID, ready.revision),
    };
    let delegate_claim = Command::Claim {
        task_id: OFFICE_TASK_ID,
        context: acting(delegation.id, ready.revision),
    };
    let second = PostgresWorkRepository::new(pool.clone());
    let (first, other) = tokio::join!(
        repository.execute(VerifiedActor::Office01, office_claim),
        second.execute(VerifiedActor::Delegate01, delegate_claim),
    );
    let winners = [&first, &other]
        .iter()
        .filter(|value| value.is_ok())
        .count();
    assert_eq!(winners, 1, "{first:?} {other:?}");
    for outcome in [&first, &other] {
        if let Err(error) = outcome {
            assert!(
                matches!(
                    error,
                    WorkError::RevisionConflict | WorkError::WorkAssignmentConflict
                ),
                "{error:?}"
            );
        }
    }
    let (winner, loser) = if first.is_ok() {
        (VerifiedActor::Office01, VerifiedActor::Delegate01)
    } else {
        (VerifiedActor::Delegate01, VerifiedActor::Office01)
    };
    assert!(repository.task(winner, OFFICE_TASK_ID).await.is_ok());
    assert_eq!(
        repository.task(loser, OFFICE_TASK_ID).await,
        Err(WorkError::WorkItemNotFound)
    );
    let claims: i64 =
        sqlx::query_scalar("SELECT count(*) FROM work.workflow_history WHERE kind='claimed'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(claims, 1);

    // The manager reassigns to the multi-role principal; the old assignee loses access.
    let managed = office_revision(&repository, VerifiedActor::Approver01).await;
    assert!(managed.can_assign);
    assert_eq!(
        managed.assignment.as_ref().unwrap().principal_id,
        winner.principal_id()
    );
    let assign = Command::Assign {
        task_id: OFFICE_TASK_ID,
        context: acting(APPROVER_MANAGEMENT_ASSIGNMENT_ID, managed.revision),
        expected_attempt_id: managed.attempt_id,
        assignee: VerifiedActor::MultiRole01,
        assignee_responsibility_id: MULTI_ROLE_PROCESSING_ASSIGNMENT_ID,
        reason: "担当の平準化".into(),
    };
    let assigned = repository
        .execute(VerifiedActor::Approver01, assign.clone())
        .await
        .unwrap();
    assert!(matches!(assigned, MutationResult::Assigned { .. }));
    assert_eq!(
        repository
            .recover(VerifiedActor::Approver01, assign.context().operation_id)
            .await
            .unwrap(),
        assigned
    );
    assert_eq!(
        repository.task(winner, OFFICE_TASK_ID).await,
        Err(WorkError::WorkItemNotFound)
    );
    let detail = repository
        .task(VerifiedActor::MultiRole01, OFFICE_TASK_ID)
        .await
        .unwrap();
    assert_eq!(
        detail.task.assignment.unwrap().acting_assignment_id,
        MULTI_ROLE_PROCESSING_ASSIGNMENT_ID
    );
    let payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM work.event_staging WHERE operation_id=$1 AND action='assigned'",
    )
    .bind(assign.context().operation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(payload["assigneePrincipalId"], "multi-role-01");
    assert_eq!(payload["actingResponsibilityKind"], "role_assignment");
    assert!(!payload.to_string().contains("担当の平準化"));

    // A policy writer holding the update lock fences an in-flight Work command,
    // which then re-evaluates against the committed revocation.
    let policy = repository
        .organization(VerifiedActor::Approver01)
        .await
        .unwrap();
    let mut writer = pool.begin().await.unwrap();
    let sqlx::types::Json(mut locked): sqlx::types::Json<OrganizationPolicy> =
        sqlx::query_scalar("SELECT body FROM work.organization_policies WHERE id=$1 FOR UPDATE")
            .bind(ORGANIZATION_POLICY_ID)
            .fetch_one(&mut *writer)
            .await
            .unwrap();
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    locked
        .apply(
            VerifiedActor::Approver01,
            &PolicyCommand::RevokeRoleAssignment {
                context: acting(APPROVER_MANAGEMENT_ASSIGNMENT_ID, policy.policy_revision),
                assignment_id: MULTI_ROLE_PROCESSING_ASSIGNMENT_ID,
                reason: "兼務解除".into(),
            },
            &now,
        )
        .unwrap();
    let hold = Command::Hold {
        task_id: OFFICE_TASK_ID,
        context: acting(MULTI_ROLE_PROCESSING_ASSIGNMENT_ID, detail.task.revision),
        expected_attempt_id: detail.task.attempt_id,
        definition_action_id: HOLD_ACTION_ID,
    };
    let fenced = {
        let repository = PostgresWorkRepository::new(pool.clone());
        tokio::spawn(async move { repository.execute(VerifiedActor::MultiRole01, hold).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(
        !fenced.is_finished(),
        "Work command must wait for the policy writer"
    );
    sqlx::query("UPDATE work.organization_policies SET revision=$2, body=$3 WHERE id=$1")
        .bind(ORGANIZATION_POLICY_ID)
        .bind(locked.revision)
        .bind(sqlx::types::Json(&locked))
        .execute(&mut *writer)
        .await
        .unwrap();
    writer.commit().await.unwrap();
    // The acting responsibility no longer resolves for this actor under the lock.
    assert_eq!(fenced.await.unwrap(), Err(WorkError::Forbidden));
    let held: i64 =
        sqlx::query_scalar("SELECT count(*) FROM work.workflow_history WHERE kind='held'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(held, 0);
    assert_eq!(
        repository
            .task(VerifiedActor::MultiRole01, OFFICE_TASK_ID)
            .await,
        Err(WorkError::WorkItemNotFound)
    );
    let ended = office_revision(&repository, VerifiedActor::Approver01).await;
    assert!(!ended.assignment.unwrap().responsibility_effective);
    sqlx::raw_sql("DROP SCHEMA work CASCADE")
        .execute(&pool)
        .await
        .unwrap();
}
