#![forbid(unsafe_code)]
//! Separate Work schema, migration ledger and atomic workflow/operation/event transaction.
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, types::Json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use work_application::{WorkFuture, WorkRepository, command_digest};
use work_domain::*;

const MIGRATION: &str = include_str!("../migrations/0001_work.sql");
const MIGRATION_LOCK: i64 = 0x574F524B504F4301;
#[derive(Clone)]
pub struct PostgresWorkRepository {
    pool: PgPool,
}
impl PostgresWorkRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
    async fn load(&self) -> Result<Workflow, WorkError> {
        let Json(workflow): Json<Workflow> =
            sqlx::query_scalar("SELECT body FROM work.workflow_instances WHERE id = $1")
                .bind(WORKFLOW_ID)
                .fetch_optional(&self.pool)
                .await
                .map_err(database_error)?
                .ok_or(WorkError::DependencyUnavailable)?;
        Ok(workflow)
    }
    async fn execute_command(
        &self,
        actor: VerifiedActor,
        command: Command,
    ) -> Result<MutationResult, WorkError> {
        let digest = command_digest(actor, &command)?;
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        // One locked aggregate contains the fixed definition's current task/attempt/assignment/artifacts.
        // No provider calls occur while holding this Work transaction lock.
        let Json(mut workflow): Json<Workflow> =
            sqlx::query_scalar("SELECT body FROM work.workflow_instances WHERE id=$1 FOR UPDATE")
                .bind(WORKFLOW_ID)
                .fetch_optional(&mut *tx)
                .await
                .map_err(database_error)?
                .ok_or(WorkError::DependencyUnavailable)?;
        workflow.authorize_command(actor, &command)?;
        let operation_id = command.context().operation_id;
        let previous = sqlx::query("SELECT principal_id, command_digest, outcome FROM work.operation_ledger WHERE operation_id=$1")
            .bind(operation_id).fetch_optional(&mut *tx).await.map_err(database_error)?;
        if let Some(previous) = previous {
            let principal: String = previous.try_get("principal_id").map_err(database_error)?;
            if principal != actor.principal_id() {
                return Err(WorkError::WorkItemNotFound);
            }
            let previous_digest: Vec<u8> =
                previous.try_get("command_digest").map_err(database_error)?;
            if digest != previous_digest {
                return Err(WorkError::OperationConflict);
            }
            let Json(outcome): Json<MutationResult> =
                previous.try_get("outcome").map_err(database_error)?;
            workflow.authorize_recovery(actor, &outcome)?;
            return Ok(outcome);
        }
        let now = OffsetDateTime::now_utc();
        let timestamp = now
            .format(&Rfc3339)
            .map_err(|_| WorkError::IntegrityViolation)?;
        let result = workflow.apply(actor, &command, &timestamp)?;
        let action = match &result {
            MutationResult::DraftSaved { .. } => "draft_saved",
            MutationResult::Claimed { .. } => "claimed",
            MutationResult::Submitted { .. } => "submitted",
        };
        sqlx::query("UPDATE work.workflow_instances SET revision=$2, body=$3 WHERE id=$1")
            .bind(WORKFLOW_ID)
            .bind(workflow.revision)
            .bind(Json(&workflow))
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        sqlx::query("INSERT INTO work.operation_ledger(operation_id,workflow_id,principal_id,acting_assignment_id,command_digest,outcome) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(operation_id).bind(WORKFLOW_ID).bind(actor.principal_id()).bind(command.context().acting_assignment_id).bind(digest).bind(Json(&result))
            .execute(&mut *tx).await.map_err(database_error)?;
        if action != "draft_saved" {
            sqlx::query("INSERT INTO work.workflow_history(id,workflow_id,operation_id,kind,occurred_at) VALUES($1,$2,$3,$4,$5)")
                .bind(Uuid::now_v7()).bind(WORKFLOW_ID).bind(operation_id).bind(action).bind(now).execute(&mut *tx).await.map_err(database_error)?;
        }
        // Mandatory unsampled staging. Failure rolls back the workflow, history and ledger too.
        // It is not a claim that the separate Audit delivery pipeline is connected or qualified.
        let payload = serde_json::json!({"schemaVersion":1,"resourceType":"work_item","result":"committed","operationId":operation_id});
        sqlx::query("INSERT INTO work.event_staging(id,operation_id,workflow_id,principal_id,acting_assignment_id,task_id,action,occurred_at,payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(Uuid::now_v7()).bind(operation_id).bind(WORKFLOW_ID).bind(actor.principal_id()).bind(command.context().acting_assignment_id).bind(command.task_id()).bind(action).bind(now).bind(Json(payload))
            .execute(&mut *tx).await.map_err(database_error)?;
        tx.commit()
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        Ok(result)
    }
}
fn database_error(_: sqlx::Error) -> WorkError {
    WorkError::DependencyUnavailable
}
/// Explicit administrative command only; never called by router or ordinary startup.
pub async fn migrate(pool: &PgPool) -> Result<(), WorkError> {
    let mut tx = pool.begin().await.map_err(database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(MIGRATION_LOCK)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    sqlx::raw_sql("CREATE SCHEMA IF NOT EXISTS work; CREATE TABLE IF NOT EXISTS work.schema_migrations(version bigint PRIMARY KEY, checksum bytea NOT NULL, applied_at timestamptz NOT NULL DEFAULT clock_timestamp());")
        .execute(&mut *tx).await.map_err(database_error)?;
    let records: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version, checksum FROM work.schema_migrations ORDER BY version")
            .fetch_all(&mut *tx)
            .await
            .map_err(database_error)?;
    let checksum = Sha256::digest(MIGRATION.as_bytes()).to_vec();
    match records.as_slice() {
        [] => {
            sqlx::raw_sql(MIGRATION)
                .execute(&mut *tx)
                .await
                .map_err(database_error)?;
            sqlx::query("INSERT INTO work.schema_migrations(version,checksum) VALUES(1,$1)")
                .bind(&checksum)
                .execute(&mut *tx)
                .await
                .map_err(database_error)?;
        }
        [(1, stored)] if stored == &checksum => (),
        _ => return Err(WorkError::IntegrityViolation),
    }
    tx.commit()
        .await
        .map_err(|_| WorkError::CommitOutcomeUnknown)
}
/// Read-only startup check, with no automatic migration or seed.
pub async fn check_schema_compatibility(pool: &PgPool) -> Result<(), WorkError> {
    let records: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version,checksum FROM work.schema_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .map_err(database_error)?;
    let checksum = Sha256::digest(MIGRATION.as_bytes()).to_vec();
    if records.as_slice() != [(1, checksum)] {
        return Err(WorkError::IntegrityViolation);
    }
    let ready: bool = sqlx::query_scalar("SELECT to_regclass('work.workflow_instances') IS NOT NULL AND to_regclass('work.operation_ledger') IS NOT NULL AND to_regclass('work.workflow_history') IS NOT NULL AND to_regclass('work.event_staging') IS NOT NULL")
        .fetch_one(pool).await.map_err(database_error)?;
    if !ready {
        return Err(WorkError::IntegrityViolation);
    }
    Ok(())
}
/// Explicit synthetic seed; existing workflow state is never reset or overwritten.
/// Document ID is a deliberately shared input reference, not private Work content.
pub async fn seed_synthetic(pool: &PgPool, document_id: Option<Uuid>) -> Result<(), WorkError> {
    check_schema_compatibility(pool).await?;
    let workflow = Workflow::synthetic(document_id);
    sqlx::query("INSERT INTO work.workflow_instances(id,revision,body) VALUES($1,0,$2) ON CONFLICT(id) DO NOTHING")
        .bind(WORKFLOW_ID).bind(Json(workflow)).execute(pool).await.map_err(database_error)?;
    Ok(())
}
impl WorkRepository for PostgresWorkRepository {
    fn list_tasks(&self, actor: VerifiedActor, view: TaskView) -> WorkFuture<'_, Vec<TaskSummary>> {
        Box::pin(async move { Ok(self.load().await?.list_tasks(actor, view)) })
    }
    fn task(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, TaskDetail> {
        Box::pin(async move { self.load().await?.detail(actor, id) })
    }
    fn artifact(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, WorkingArtifact> {
        Box::pin(async move { self.load().await?.artifact(actor, id) })
    }
    fn snapshot(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, HandoffSnapshot> {
        Box::pin(async move { self.load().await?.snapshot(actor, id) })
    }
    fn execute(&self, actor: VerifiedActor, command: Command) -> WorkFuture<'_, MutationResult> {
        Box::pin(async move { self.execute_command(actor, command).await })
    }
    fn recover(&self, actor: VerifiedActor, operation_id: Uuid) -> WorkFuture<'_, MutationResult> {
        Box::pin(async move {
            // A single statement snapshot must observe the workflow and committed result together.
            // Two READ COMMITTED SELECTs can pair a pre-commit workflow with a new outcome,
            // making a just-committed artifact, submission or claim fail authorization spuriously.
            let row: Option<(Json<Workflow>, Json<MutationResult>)> = sqlx::query_as(
                "SELECT workflow.body, ledger.outcome FROM work.operation_ledger AS ledger \
                 JOIN work.workflow_instances AS workflow ON workflow.id = ledger.workflow_id \
                 WHERE ledger.operation_id = $1 AND ledger.principal_id = $2 AND workflow.id = $3",
            )
            .bind(operation_id)
            .bind(actor.principal_id())
            .bind(WORKFLOW_ID)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
            let (Json(workflow), Json(outcome)) = row.ok_or(WorkError::WorkItemNotFound)?;
            workflow.authorize_recovery(actor, &outcome)?;
            Ok(outcome)
        })
    }
}
