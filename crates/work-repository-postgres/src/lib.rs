#![forbid(unsafe_code)]
//! Separate Work schema, migration ledger and atomic workflow/operation/event transaction.
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, types::Json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use work_application::{
    EvidenceSourcePort, EvidenceSourcePurpose, WorkFuture, WorkRepository, command_digest,
};
use work_domain::*;

type StoredOperation = (Json<Workflow>, String, Vec<u8>, Json<MutationResult>);

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_work.sql")),
    (2, include_str!("../migrations/0002_return.sql")),
    (3, include_str!("../migrations/0003_evidence.sql")),
];
const MIGRATION_LOCK: i64 = 0x574F524B504F4301;
#[derive(Clone)]
pub struct PostgresWorkRepository {
    pool: PgPool,
    evidence_source: Option<Arc<dyn EvidenceSourcePort>>,
}
impl PostgresWorkRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            evidence_source: None,
        }
    }
    pub fn with_evidence_source(
        pool: PgPool,
        evidence_source: Arc<dyn EvidenceSourcePort>,
    ) -> Self {
        Self {
            pool,
            evidence_source: Some(evidence_source),
        }
    }
    async fn load(&self) -> Result<Workflow, WorkError> {
        let Json(workflow): Json<Workflow> =
            sqlx::query_scalar("SELECT body FROM work.workflow_instances WHERE id = $1")
                .bind(WORKFLOW_ID)
                .fetch_optional(&self.pool)
                .await
                .map_err(database_error)?
                .ok_or(WorkError::DependencyUnavailable)?;
        workflow.validate_integrity()?;
        Ok(workflow)
    }
    async fn authorize_sources(
        &self,
        actor: VerifiedActor,
        sources: &[EvidenceSource],
        purpose: EvidenceSourcePurpose,
    ) -> Result<(), WorkError> {
        if sources.is_empty() {
            return Ok(());
        }
        if sources.len() > MAX_REFERENCES {
            return Err(WorkError::ValidationFailed);
        }
        let provider = self
            .evidence_source
            .as_ref()
            .ok_or(WorkError::DependencyUnavailable)?;
        let started = Instant::now();
        // Sequential fanout is <=8. Each port call has a five-second cancellation
        // boundary; elapsed checks prevent N * timeout waits and stale receipts.
        for source in sources {
            if started.elapsed() > PREFLIGHT_LIFETIME {
                return Err(WorkError::DependencyUnavailable);
            }
            provider.authorize(actor, source.clone(), purpose).await?;
            if started.elapsed() > PREFLIGHT_LIFETIME {
                return Err(WorkError::DependencyUnavailable);
            }
        }
        Ok(())
    }
    async fn verify_read_sources(
        &self,
        actor: VerifiedActor,
        workflow_revision: i64,
        evidence: Vec<EvidenceRecord>,
    ) -> Result<(), WorkError> {
        let sources = source_set(&evidence);
        if sources.is_empty() {
            return Ok(());
        }
        let started = Instant::now();
        self.authorize_sources(actor, &sources, EvidenceSourcePurpose::ReadHistory)
            .await?;
        // Providers are checked outside Work locks; immediately recheck current
        // local authority by revision before disclosure, never use the snapshot as ACL.
        if self.load().await?.revision != workflow_revision
            || started.elapsed() > PREFLIGHT_LIFETIME
        {
            return Err(WorkError::DependencyUnavailable);
        }
        Ok(())
    }
    async fn operation(
        &self,
        operation_id: Uuid,
    ) -> Result<Option<(Workflow, String, Vec<u8>, MutationResult)>, WorkError> {
        let row: Option<StoredOperation> = sqlx::query_as(
            "SELECT workflow.body, ledger.principal_id, ledger.command_digest, ledger.outcome FROM work.operation_ledger AS ledger \
             JOIN work.workflow_instances AS workflow ON workflow.id = ledger.workflow_id \
             WHERE ledger.operation_id = $1 AND workflow.id = $2")
            .bind(operation_id).bind(WORKFLOW_ID).fetch_optional(&self.pool).await.map_err(database_error)?;
        row.map(|(Json(workflow), principal, digest, Json(outcome))| {
            workflow.validate_integrity()?;
            Ok((workflow, principal, digest, outcome))
        })
        .transpose()
    }
    async fn disclose_result(
        &self,
        actor: VerifiedActor,
        result: MutationResult,
    ) -> Result<MutationResult, WorkError> {
        let workflow = self.load().await?;
        let evidence = workflow.result_evidence(actor, &result)?;
        self.verify_read_sources(actor, workflow.revision, evidence)
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        Ok(result)
    }
    async fn execute_command(
        &self,
        actor: VerifiedActor,
        command: Command,
    ) -> Result<MutationResult, WorkError> {
        command.context().authorize(actor)?;
        let digest = command_digest(actor, &command)?;
        let operation_id = command.context().operation_id;
        // Existing committed operations use their exact digest and current output
        // authority, independently of a now-closed command attempt.
        if let Some((workflow, principal, previous_digest, outcome)) =
            self.operation(operation_id).await?
        {
            if principal != actor.principal_id() {
                return Err(WorkError::WorkItemNotFound);
            }
            if previous_digest != digest {
                return Err(WorkError::OperationConflict);
            }
            workflow.authorize_recovery(actor, &outcome)?;
            return self.disclose_result(actor, outcome).await;
        }
        let observed = self.load().await?;
        let mut preview = observed.clone();
        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| WorkError::IntegrityViolation)?;
        // Full Work scope, bounds, OCC and selection closure precede provider fanout.
        let preview_result = preview.apply(actor, &command, &timestamp)?;
        validate_record_collections(&preview)?;
        let sources = source_set(&preview.result_evidence(actor, &preview_result)?);
        let receipt = EvidencePreflight {
            started: Instant::now(),
            actor,
            workflow_revision: observed.revision,
            digest: digest.clone(),
            sources,
        };
        let purpose = if matches!(&command, Command::RegisterEvidence { .. }) {
            EvidenceSourcePurpose::RegisterPublished
        } else {
            EvidenceSourcePurpose::ReadHistory
        };
        self.authorize_sources(actor, &receipt.sources, purpose)
            .await?;
        let checked_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| WorkError::IntegrityViolation)?;
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let Json(mut workflow): Json<Workflow> =
            sqlx::query_scalar("SELECT body FROM work.workflow_instances WHERE id=$1 FOR UPDATE")
                .bind(WORKFLOW_ID)
                .fetch_optional(&mut *tx)
                .await
                .map_err(database_error)?
                .ok_or(WorkError::DependencyUnavailable)?;
        workflow.validate_integrity()?;
        let previous=sqlx::query("SELECT principal_id,command_digest,outcome FROM work.operation_ledger WHERE operation_id=$1")
            .bind(operation_id).fetch_optional(&mut *tx).await.map_err(database_error)?;
        if let Some(previous) = previous {
            let principal: String = previous.try_get("principal_id").map_err(database_error)?;
            if principal != actor.principal_id() {
                return Err(WorkError::WorkItemNotFound);
            }
            let previous_digest: Vec<u8> =
                previous.try_get("command_digest").map_err(database_error)?;
            if previous_digest != digest {
                return Err(WorkError::OperationConflict);
            }
            let Json(outcome): Json<MutationResult> =
                previous.try_get("outcome").map_err(database_error)?;
            workflow.authorize_recovery(actor, &outcome)?;
            // Explicit rollback releases the lock before any provider disclosure.
            tx.rollback().await.map_err(database_error)?;
            return self.disclose_result(actor, outcome).await;
        }
        receipt.validate(actor, workflow.revision, &digest, &receipt.sources)?;
        let now = OffsetDateTime::now_utc();
        let timestamp = now
            .format(&Rfc3339)
            .map_err(|_| WorkError::IntegrityViolation)?;
        let mut result = workflow.apply(actor, &command, &timestamp)?;
        // Preserve the actual server preflight observation time, not a client receipt.
        if let MutationResult::EvidenceRegistered { evidence, .. } = &mut result {
            evidence.provider_checked_at = checked_at.clone();
            evidence.retrieved_at = checked_at;
            if let Some(stored) = workflow.evidence.iter_mut().find(|e| e.id == evidence.id) {
                stored.provider_checked_at = evidence.provider_checked_at.clone();
                stored.retrieved_at = evidence.retrieved_at.clone();
            }
        }
        validate_record_collections(&workflow)?;
        let actual_sources = source_set(&workflow.result_evidence(actor, &result)?);
        receipt.validate(actor, observed.revision, &digest, &actual_sources)?;
        let action = match &result {
            MutationResult::EvidenceRegistered { .. } => "evidence_registered",
            MutationResult::FindingRegistered { .. } => "finding_registered",
            MutationResult::DecisionRecorded { .. } => "decision_recorded",
            MutationResult::DraftSaved { .. } => "draft_saved",
            MutationResult::Claimed { .. } => "claimed",
            MutationResult::Submitted { .. } => "submitted",
            MutationResult::Returned { .. } => "returned",
        };
        sqlx::query("UPDATE work.workflow_instances SET revision=$2,body=$3 WHERE id=$1")
            .bind(WORKFLOW_ID)
            .bind(workflow.revision)
            .bind(Json(&workflow))
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        sqlx::query("INSERT INTO work.operation_ledger(operation_id,workflow_id,principal_id,acting_assignment_id,command_digest,outcome) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(operation_id).bind(WORKFLOW_ID).bind(actor.principal_id()).bind(command.context().acting_assignment_id).bind(&digest).bind(Json(&result)).execute(&mut *tx).await.map_err(database_error)?;
        // Candidate/decision records are not workflow transitions.
        if matches!(action, "submitted" | "claimed" | "returned") {
            sqlx::query("INSERT INTO work.workflow_history(id,workflow_id,operation_id,kind,occurred_at) VALUES($1,$2,$3,$4,$5)")
                .bind(Uuid::now_v7()).bind(WORKFLOW_ID).bind(operation_id).bind(action).bind(now).execute(&mut *tx).await.map_err(database_error)?;
        }
        let payload = serde_json::json!({"schemaVersion":1,"resourceType":"work_item","result":"committed","operationId":operation_id});
        sqlx::query("INSERT INTO work.event_staging(id,operation_id,workflow_id,principal_id,acting_assignment_id,task_id,action,occurred_at,payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(Uuid::now_v7()).bind(operation_id).bind(WORKFLOW_ID).bind(actor.principal_id()).bind(command.context().acting_assignment_id).bind(command.task_id()).bind(action).bind(now).bind(Json(payload)).execute(&mut *tx).await.map_err(database_error)?;
        // Final local/freshness check under the same lock, with no remote call.
        receipt.validate(actor, observed.revision, &digest, &actual_sources)?;
        tx.commit()
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        self.disclose_result(actor, result).await
    }
}
/// A complete visible collection must stay retrievable after every mutation,
/// including claim/resubmit introducing received membership. This fixed two-person
/// PoC is deliberately bounded, rather than pretending to provide pagination.
fn validate_record_collections(workflow: &Workflow) -> Result<(), WorkError> {
    for (actor, task) in [
        (VerifiedActor::Sales01, SALES_TASK_ID),
        (VerifiedActor::Office01, OFFICE_TASK_ID),
    ] {
        if workflow.detail(actor, task).is_ok() {
            let evidence = workflow.list_evidence(actor, task)?;
            validate_collection_bytes(evidence.len(), serde_json::to_vec(&evidence))?;
            let findings = workflow.list_findings(actor, task)?;
            validate_collection_bytes(findings.len(), serde_json::to_vec(&findings))?;
        }
        // Historical explicitly selected findings also have decision-list routes.
        for finding in &workflow.findings {
            if workflow.finding(actor, finding.id).is_ok() {
                let decisions = workflow.list_decisions(actor, finding.id)?;
                validate_collection_bytes(decisions.len(), serde_json::to_vec(&decisions))?;
            }
        }
    }
    Ok(())
}
fn validate_collection_bytes(
    count: usize,
    serialized: Result<Vec<u8>, serde_json::Error>,
) -> Result<(), WorkError> {
    let bytes = serialized.map_err(|_| WorkError::IntegrityViolation)?;
    // Reserve the page envelope and response bookkeeping; actual JSON escaping
    // (not UTF-8 input bytes) is what consumes the HTTP response profile.
    if count > MAX_VISIBLE_RECORDS || bytes.len() > 1024 * 1024 - 1024 {
        return Err(WorkError::ValidationFailed);
    }
    Ok(())
}
const PREFLIGHT_LIFETIME: Duration = Duration::from_secs(5);
fn source_set(evidence: &[EvidenceRecord]) -> Vec<EvidenceSource> {
    evidence
        .iter()
        .map(|e| e.source.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

struct EvidencePreflight {
    started: Instant,
    actor: VerifiedActor,
    workflow_revision: i64,
    digest: Vec<u8>,
    sources: Vec<EvidenceSource>,
}
impl EvidencePreflight {
    fn validate(
        &self,
        actor: VerifiedActor,
        revision: i64,
        digest: &[u8],
        sources: &[EvidenceSource],
    ) -> Result<(), WorkError> {
        if self.actor != actor
            || self.workflow_revision != revision
            || self.digest != digest
            || self.sources != sources
        {
            return Err(WorkError::RevisionConflict);
        }
        if !sources.is_empty() && self.started.elapsed() > PREFLIGHT_LIFETIME {
            return Err(WorkError::DependencyUnavailable);
        }
        Ok(())
    }
}
fn database_error(_: sqlx::Error) -> WorkError {
    WorkError::DependencyUnavailable
}
fn validate_migration_records(records: &[(i64, Vec<u8>)], complete: bool) -> Result<(), WorkError> {
    if records.len() > MIGRATIONS.len() || (complete && records.len() != MIGRATIONS.len()) {
        return Err(WorkError::IntegrityViolation);
    }
    for ((version, checksum), (expected, migration)) in records.iter().zip(MIGRATIONS) {
        if version != expected || checksum != &Sha256::digest(migration.as_bytes()).to_vec() {
            return Err(WorkError::IntegrityViolation);
        }
    }
    Ok(())
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
    validate_migration_records(&records, false)?;
    for (version, migration) in MIGRATIONS.iter().skip(records.len()) {
        sqlx::raw_sql(*migration)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        sqlx::query("INSERT INTO work.schema_migrations(version,checksum) VALUES($1,$2)")
            .bind(version)
            .bind(Sha256::digest(migration.as_bytes()).to_vec())
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
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
    validate_migration_records(&records, true)?;
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
    fn list_evidence(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
    ) -> WorkFuture<'_, Vec<EvidenceRecord>> {
        Box::pin(async move {
            let w = self.load().await?;
            let evidence = w.list_evidence(actor, task_id)?;
            self.verify_read_sources(actor, w.revision, evidence.clone())
                .await?;
            Ok(evidence)
        })
    }
    fn evidence(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, EvidenceRecord> {
        Box::pin(async move {
            let w = self.load().await?;
            let evidence = w.evidence_record(actor, id)?;
            self.verify_read_sources(actor, w.revision, vec![evidence.clone()])
                .await?;
            Ok(evidence)
        })
    }
    fn list_findings(&self, actor: VerifiedActor, task_id: Uuid) -> WorkFuture<'_, Vec<Finding>> {
        Box::pin(async move {
            let w = self.load().await?;
            let findings = w.list_findings(actor, task_id)?;
            let mut evidence = vec![];
            for f in &findings {
                evidence.extend(w.resolve_evidence(actor, &f.evidence_revision_refs)?);
            }
            self.verify_read_sources(actor, w.revision, evidence)
                .await?;
            Ok(findings)
        })
    }
    fn finding(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, Finding> {
        Box::pin(async move {
            let w = self.load().await?;
            let finding = w.finding(actor, id)?;
            self.verify_read_sources(
                actor,
                w.revision,
                w.resolve_evidence(actor, &finding.evidence_revision_refs)?,
            )
            .await?;
            Ok(finding)
        })
    }
    fn list_decisions(
        &self,
        actor: VerifiedActor,
        finding_id: Uuid,
    ) -> WorkFuture<'_, Vec<HumanDecision>> {
        Box::pin(async move {
            let w = self.load().await?;
            let finding = w.finding(actor, finding_id)?;
            let decisions = w.list_decisions(actor, finding_id)?;
            let mut evidence = w.resolve_evidence(actor, &finding.evidence_revision_refs)?;
            for d in &decisions {
                evidence.extend(w.resolve_evidence(actor, &d.evidence_revision_refs)?);
            }
            self.verify_read_sources(actor, w.revision, evidence)
                .await?;
            Ok(decisions)
        })
    }
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
        Box::pin(async move {
            let workflow = self.load().await?;
            let snapshot = workflow.snapshot(actor, id)?;
            self.verify_read_sources(
                actor,
                workflow.revision,
                workflow.snapshot_evidence(actor, id)?,
            )
            .await?;
            Ok(snapshot)
        })
    }
    fn return_instruction(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, ReturnInstruction> {
        Box::pin(async move { self.load().await?.return_instruction(actor, id) })
    }
    fn execute(&self, actor: VerifiedActor, command: Command) -> WorkFuture<'_, MutationResult> {
        Box::pin(async move { self.execute_command(actor, command).await })
    }
    fn recover(&self, actor: VerifiedActor, operation_id: Uuid) -> WorkFuture<'_, MutationResult> {
        Box::pin(async move {
            let (workflow, principal, _, outcome) = self
                .operation(operation_id)
                .await?
                .ok_or(WorkError::WorkItemNotFound)?;
            if principal != actor.principal_id() {
                return Err(WorkError::WorkItemNotFound);
            }
            workflow.authorize_recovery(actor, &outcome)?;
            self.disclose_result(actor, outcome).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maximum_decision_strings_cannot_make_a_committed_collection_unreadable() {
        let actor = VerifiedActor::Sales01;
        let doc = Uuid::now_v7();
        let mut workflow = Workflow::synthetic(Some(doc));
        let context = |revision| CommandContext {
            operation_id: Uuid::now_v7(),
            expected_revision: revision,
            acting_assignment_id: SALES_ASSIGNMENT_ID,
        };
        let e = match workflow
            .apply(
                actor,
                &Command::RegisterEvidence {
                    task_id: SALES_TASK_ID,
                    context: context(0),
                    expected_attempt_id: SALES_ATTEMPT_ID,
                    source: EvidenceSource {
                        source_ref: SourceRef {
                            provider_id: "document".into(),
                            resource_id: doc,
                            revision_id: Uuid::now_v7(),
                            version_id: Uuid::now_v7(),
                        },
                        authoritative_locator: AuthoritativeLocator {
                            kind: "contentItem".into(),
                            content_item_id: Uuid::now_v7(),
                            representation_id: Uuid::now_v7(),
                        },
                    },
                    relevant_location: "selected".into(),
                },
                "2026-10-04T00:00:00Z",
            )
            .unwrap()
        {
            MutationResult::EvidenceRegistered { evidence, .. } => evidence,
            _ => panic!(),
        };
        let f = match workflow
            .apply(
                actor,
                &Command::RegisterFinding {
                    task_id: SALES_TASK_ID,
                    context: context(1),
                    expected_attempt_id: SALES_ATTEMPT_ID,
                    claim: "candidate".into(),
                    evidence_revision_refs: vec![RevisionRef {
                        id: e.id,
                        revision: 1,
                    }],
                    supersedes_finding_id: None,
                },
                "2026-10-04T00:00:00Z",
            )
            .unwrap()
        {
            MutationResult::FindingRegistered { finding, .. } => finding,
            _ => panic!(),
        };
        let text = format!("x{}", "\u{0001}".repeat(MAX_TEXT_BYTES - 1));
        for i in 0..MAX_VISIBLE_RECORDS {
            let command = Command::RecordDecision {
                task_id: SALES_TASK_ID,
                context: context(workflow.source.revision),
                expected_attempt_id: SALES_ATTEMPT_ID,
                finding_id: f.id,
                finding_revision: 1,
                decision: DecisionKind::Modified,
                adopted_claim: Some(text.clone()),
                reason: Some(text.clone()),
                evidence_revision_refs: vec![],
                supersedes_decision_id: None,
            };
            workflow
                .apply(actor, &command, "2026-10-04T00:00:00Z")
                .unwrap();
            if i == 0 {
                assert_eq!(validate_record_collections(&workflow), Ok(()));
            }
        }
        assert!(
            serde_json::to_vec(&workflow.list_decisions(actor, f.id).unwrap())
                .unwrap()
                .len()
                > 1024 * 1024
        );
        assert_eq!(
            validate_record_collections(&workflow),
            Err(WorkError::ValidationFailed)
        );
    }
    #[test]
    fn preflight_rejects_expiry_actor_revision_payload_and_source_drift() {
        let source = EvidenceSource {
            source_ref: SourceRef {
                provider_id: "document".into(),
                resource_id: Uuid::now_v7(),
                revision_id: Uuid::now_v7(),
                version_id: Uuid::now_v7(),
            },
            authoritative_locator: AuthoritativeLocator {
                kind: "contentItem".into(),
                content_item_id: Uuid::now_v7(),
                representation_id: Uuid::now_v7(),
            },
        };
        let mut r = EvidencePreflight {
            started: Instant::now(),
            actor: VerifiedActor::Sales01,
            workflow_revision: 5,
            digest: vec![1; 32],
            sources: vec![source.clone()],
        };
        assert_eq!(
            r.validate(
                VerifiedActor::Sales01,
                5,
                &[1; 32],
                std::slice::from_ref(&source)
            ),
            Ok(())
        );
        assert_eq!(
            r.validate(
                VerifiedActor::Office01,
                5,
                &[1; 32],
                std::slice::from_ref(&source)
            ),
            Err(WorkError::RevisionConflict)
        );
        assert_eq!(
            r.validate(
                VerifiedActor::Sales01,
                6,
                &[1; 32],
                std::slice::from_ref(&source)
            ),
            Err(WorkError::RevisionConflict)
        );
        assert_eq!(
            r.validate(
                VerifiedActor::Sales01,
                5,
                &[2; 32],
                std::slice::from_ref(&source)
            ),
            Err(WorkError::RevisionConflict)
        );
        assert_eq!(
            r.validate(VerifiedActor::Sales01, 5, &[1; 32], &[]),
            Err(WorkError::RevisionConflict)
        );
        r.started = Instant::now() - Duration::from_secs(6);
        assert_eq!(
            r.validate(VerifiedActor::Sales01, 5, &[1; 32], &[source]),
            Err(WorkError::DependencyUnavailable)
        );
    }
    #[test]
    fn migration_upgrade_accepts_only_unchanged_prefix_and_startup_requires_both() {
        let records: Vec<_> = MIGRATIONS
            .iter()
            .map(|(version, sql)| (*version, Sha256::digest(sql.as_bytes()).to_vec()))
            .collect();
        assert_eq!(validate_migration_records(&[], false), Ok(()));
        assert_eq!(validate_migration_records(&records[..1], false), Ok(()));
        assert_eq!(
            validate_migration_records(&records[..1], true),
            Err(WorkError::IntegrityViolation)
        );
        assert_eq!(validate_migration_records(&records, true), Ok(()));
        assert_eq!(
            validate_migration_records(&records[1..], false),
            Err(WorkError::IntegrityViolation)
        );
        let mut changed = records.clone();
        changed[0].1[0] ^= 1;
        assert_eq!(
            validate_migration_records(&changed, false),
            Err(WorkError::IntegrityViolation)
        );
        let mut future = records;
        future.push((4, vec![0; 32]));
        assert_eq!(
            validate_migration_records(&future, false),
            Err(WorkError::IntegrityViolation)
        );
    }
}
