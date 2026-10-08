#![forbid(unsafe_code)]
//! Separate Work schema, migration ledger and atomic workflow/operation/event transaction.
mod agent;
mod files;
mod finding_diagnostics;
use finding_diagnostics::{Dependency, Diagnostic, Failure, Phase, SqlClass};

tokio::task_local! { static FINDING_DIAGNOSTIC: std::cell::RefCell<Diagnostic>; }

fn diagnostic_phase(phase: Phase, dependency: Dependency) {
    let _ = FINDING_DIAGNOSTIC.try_with(|state| {
        let mut state = state.borrow_mut();
        state.phase = phase;
        state.dependency = dependency;
    });
}

async fn diagnose_finding<T>(
    list: bool,
    future: impl std::future::Future<Output = Result<T, WorkError>>,
) -> Result<T, WorkError> {
    diagnose_finding_with(list, future, |line| {
        // A broken diagnostic sink must not change the original HTTP result.
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "KP_FINDING_DIAGNOSTIC {line}");
    })
    .await
}
async fn diagnose_finding_with<T>(
    list: bool,
    future: impl std::future::Future<Output = Result<T, WorkError>>,
    report: impl FnOnce(String),
) -> Result<T, WorkError> {
    FINDING_DIAGNOSTIC
        .scope(std::cell::RefCell::new(Diagnostic::default()), async {
            let started = Instant::now();
            let result = future.await;
            if let Err(error) = &result {
                let failure = match error {
                    WorkError::DependencyUnavailable => Failure::DependencyUnavailable,
                    WorkError::CommitOutcomeUnknown => Failure::CommitUnknown,
                    WorkError::WorkArtifactUnavailable => Failure::ArtifactUnavailable,
                    WorkError::Forbidden => Failure::Forbidden,
                    WorkError::EvidenceNotFound
                    | WorkError::FindingNotFound
                    | WorkError::WorkItemNotFound
                    | WorkError::OrganizationRecordNotFound
                    | WorkError::WorkContextNotFound
                    | WorkError::WorkArtifactNotFound => Failure::NotFound,
                    WorkError::ValidationFailed => Failure::Validation,
                    WorkError::IntegrityViolation => Failure::Integrity,
                    WorkError::RevisionConflict
                    | WorkError::OperationConflict
                    | WorkError::WorkAssignmentConflict
                    | WorkError::WorkContextStale
                    | WorkError::AgentResultNotReady
                    | WorkError::HandoffNotReady
                    | WorkError::CursorStale => Failure::Conflict,
                };
                let line = FINDING_DIAGNOSTIC.with(|state| {
                    state
                        .borrow()
                        .json(list, failure, started.elapsed().as_millis())
                });
                report(line);
            }
            result
        })
        .await
}
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, types::Json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use work_application::{
    AgentExecutionAcceptance, AgentSourcePort, EvidenceSourcePort, EvidenceSourcePurpose,
    WorkArtifactStore, WorkFuture, WorkRepository, command_digest, policy_command_digest,
};
use work_domain::*;

type StoredOperation = (Json<Workflow>, String, Vec<u8>, Json<MutationResult>);
/// The instance and revision a provider check observed before taking locks.
pub(crate) type Observed = (Uuid, i64);
pub(crate) fn observed(workflow: &Workflow) -> Observed {
    (workflow.id, workflow.revision)
}
fn observed_of(workflow: &Workflow) -> Observed {
    observed(workflow)
}

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_work.sql")),
    (2, include_str!("../migrations/0002_return.sql")),
    (3, include_str!("../migrations/0003_evidence.sql")),
    (4, include_str!("../migrations/0004_agent.sql")),
    (5, include_str!("../migrations/0005_complete.sql")),
    (6, include_str!("../migrations/0006_hold_resume.sql")),
    (7, include_str!("../migrations/0007_organization.sql")),
    (8, include_str!("../migrations/0008_attention.sql")),
    (9, include_str!("../migrations/0009_work_files.sql")),
];
const MIGRATION_LOCK: i64 = 0x574F524B504F4301;
#[derive(Clone)]
pub struct PostgresWorkRepository {
    pool: PgPool,
    evidence_source: Option<Arc<dyn EvidenceSourcePort>>,
    agent_source: Option<Arc<dyn AgentSourcePort>>,
    artifact_store: Option<Arc<dyn WorkArtifactStore>>,
}
impl PostgresWorkRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            evidence_source: None,
            agent_source: None,
            artifact_store: None,
        }
    }
    pub fn with_evidence_source(
        pool: PgPool,
        evidence_source: Arc<dyn EvidenceSourcePort>,
    ) -> Self {
        Self {
            pool,
            evidence_source: Some(evidence_source),
            agent_source: None,
            artifact_store: None,
        }
    }
    async fn load_policy(&self) -> Result<OrganizationPolicy, WorkError> {
        diagnostic_phase(Phase::Policy, Dependency::Work);
        let Json(policy): Json<OrganizationPolicy> =
            sqlx::query_scalar("SELECT body FROM work.organization_policies WHERE id = $1")
                .bind(ORGANIZATION_POLICY_ID)
                .fetch_optional(&self.pool)
                .await
                .map_err(database_error)?
                .ok_or(WorkError::DependencyUnavailable)?;
        policy.validate_integrity()?;
        Ok(policy)
    }
    /// The actor's acknowledged assignment periods (presentation state only),
    /// optionally for one instance. Never read while a row lock is held.
    async fn acknowledgements(
        &self,
        actor: VerifiedActor,
        workflow_id: Option<Uuid>,
    ) -> Result<std::collections::BTreeSet<Uuid>, WorkError> {
        diagnostic_phase(Phase::Acknowledgements, Dependency::Work);
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT work_assignment_id FROM work.attention_acknowledgements WHERE principal_id=$1 AND ($2::uuid IS NULL OR workflow_id=$2)",
        )
        .bind(actor.principal_id())
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(ids.into_iter().collect())
    }
    /// Every read evaluates the current Organization policy at the current server
    /// instant; a stored workflow never carries its own authorization. The PoC
    /// reads every (bounded) instance for one projection.
    async fn load_all(&self, actor: VerifiedActor) -> Result<Vec<Workflow>, WorkError> {
        diagnostic_phase(Phase::Load, Dependency::Work);
        let rows: Vec<Json<Workflow>> =
            sqlx::query_scalar("SELECT body FROM work.workflow_instances ORDER BY id LIMIT $1")
                .bind(
                    i64::try_from(MAX_WORK_CONTEXTS + 1)
                        .map_err(|_| WorkError::IntegrityViolation)?,
                )
                .fetch_all(&self.pool)
                .await
                .map_err(database_error)?;
        if rows.is_empty() {
            return Err(WorkError::DependencyUnavailable);
        }
        if rows.len() > MAX_WORK_CONTEXTS {
            return Err(WorkError::IntegrityViolation);
        }
        let policy = Arc::new(self.load_policy().await?);
        let acknowledged = self.acknowledgements(actor, None).await?;
        let now = OffsetDateTime::now_utc();
        diagnostic_phase(Phase::Load, Dependency::Work);
        rows.into_iter()
            .map(|Json(mut workflow)| {
                workflow.validate_integrity()?;
                workflow.attach_authority(policy.clone(), now);
                workflow.attach_acknowledgements(actor, acknowledged.clone());
                Ok(workflow)
            })
            .collect()
    }
    /// The instance that stores `target`; when none does, the first instance so
    /// the domain answers with its usual not-found error (no existence oracle).
    async fn load_for(
        &self,
        actor: VerifiedActor,
        target: WorkTarget,
    ) -> Result<Workflow, WorkError> {
        let mut all = self.load_all(actor).await?;
        let index = all.iter().position(|w| w.owns(target)).unwrap_or(0);
        Ok(all.swap_remove(index))
    }
    async fn load_id(&self, actor: VerifiedActor, id: Uuid) -> Result<Workflow, WorkError> {
        diagnostic_phase(Phase::Authority, Dependency::Work);
        let Json(mut workflow): Json<Workflow> =
            sqlx::query_scalar("SELECT body FROM work.workflow_instances WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(database_error)?
                .ok_or(WorkError::DependencyUnavailable)?;
        workflow.validate_integrity()?;
        let policy = self.load_policy().await?;
        workflow.attach_authority(Arc::new(policy), OffsetDateTime::now_utc());
        workflow.attach_acknowledgements(actor, self.acknowledgements(actor, Some(id)).await?);
        diagnostic_phase(Phase::Authority, Dependency::Work);
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
                diagnostic_phase(Phase::Freshness, Dependency::None);
                return Err(WorkError::DependencyUnavailable);
            }
            diagnostic_phase(Phase::Evidence, Dependency::Document);
            provider.authorize(actor, source.clone(), purpose).await?;
            if started.elapsed() > PREFLIGHT_LIFETIME {
                diagnostic_phase(Phase::Freshness, Dependency::None);
                return Err(WorkError::DependencyUnavailable);
            }
        }
        Ok(())
    }
    async fn verify_read_sources(
        &self,
        actor: VerifiedActor,
        (workflow_id, workflow_revision): Observed,
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
        if self.load_id(actor, workflow_id).await?.revision != workflow_revision {
            return Err(WorkError::DependencyUnavailable);
        }
        if started.elapsed() > PREFLIGHT_LIFETIME {
            diagnostic_phase(Phase::Freshness, Dependency::None);
            return Err(WorkError::DependencyUnavailable);
        }
        Ok(())
    }
    async fn operation(
        &self,
        actor: VerifiedActor,
        operation_id: Uuid,
    ) -> Result<Option<(Workflow, String, Vec<u8>, MutationResult)>, WorkError> {
        let row: Option<StoredOperation> = sqlx::query_as(
            "SELECT workflow.body, ledger.principal_id, ledger.command_digest, ledger.outcome FROM work.operation_ledger AS ledger \
             JOIN work.workflow_instances AS workflow ON workflow.id = ledger.workflow_id \
             WHERE ledger.operation_id = $1")
            .bind(operation_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        let Some((Json(workflow), principal, digest, Json(outcome))) = row else {
            return Ok(None);
        };
        workflow.validate_integrity()?;
        // Re-read through the ordinary path so authority and acknowledgments attach.
        let workflow = self.load_id(actor, workflow.id).await?;
        Ok(Some((workflow, principal, digest, outcome)))
    }
    async fn disclose_result(
        &self,
        actor: VerifiedActor,
        workflow_id: Uuid,
        result: MutationResult,
    ) -> Result<MutationResult, WorkError> {
        let started = Instant::now();
        let workflow = self.load_id(actor, workflow_id).await?;
        let evidence = workflow.result_evidence(actor, &result)?;
        self.verify_result_agent_sources(&workflow, &result, observed(&workflow))
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        self.verify_read_sources(actor, observed(&workflow), evidence)
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        validate_disclosure_freshness(started, Instant::now())
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        Ok(result)
    }
    async fn execute_command_receipt(
        &self,
        actor: VerifiedActor,
        command: Command,
    ) -> Result<AgentExecutionAcceptance, WorkError> {
        command.context().authorize(actor)?;
        let digest = command_digest(actor, &command)?;
        let operation_id = command.context().operation_id;
        // Existing committed operations use their exact digest and current output
        // authority, independently of a now-closed command attempt.
        if let Some((workflow, principal, previous_digest, outcome)) =
            self.operation(actor, operation_id).await?
        {
            if principal != actor.principal_id() {
                return Err(WorkError::WorkItemNotFound);
            }
            if previous_digest != digest {
                return Err(WorkError::OperationConflict);
            }
            workflow.authorize_recovery(actor, &outcome)?;
            return Ok(AgentExecutionAcceptance {
                outcome: self.disclose_result(actor, workflow.id, outcome).await?,
                dispatch: false,
            });
        }
        let observed = self
            .load_for(actor, WorkTarget::Task(command.task_id()))
            .await?;
        let mut preview = observed.clone();
        // Store receipts are taken after the full preview, outside any lock.
        preview.defer_generation_receipts();
        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| WorkError::IntegrityViolation)?;
        // Full Work scope, bounds, OCC and selection closure precede provider fanout.
        let preview_result = preview.apply(actor, &command, &timestamp)?;
        validate_record_collections(&preview)?;
        let verified = self.verify_generations(&preview_result).await;
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
        if matches!(&command, Command::RequestAgentExecution { .. }) {
            let id = command.context().operation_id;
            self.authorize_agent_context(
                preview.agent_disclosure_context(id)?,
                observed_of(&observed),
            )
            .await?;
        } else if !matches!(&command, Command::CancelAgentExecution { .. }) {
            self.authorize_sources(actor, &receipt.sources, purpose)
                .await?;
            self.verify_result_agent_sources(&preview, &preview_result, observed_of(&observed))
                .await?;
        }
        let checked_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| WorkError::IntegrityViolation)?;
        // Presentation-only acknowledgments are read before any row lock is taken.
        let acknowledged = self.acknowledgements(actor, Some(observed.id)).await?;
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        // Lock order: policy (share) before workflow (update). Policy writers take
        // the policy update lock, so authority cannot change before this commit.
        let policy = lock_policy(&mut tx, "FOR SHARE").await?;
        let Json(mut workflow): Json<Workflow> =
            sqlx::query_scalar("SELECT body FROM work.workflow_instances WHERE id=$1 FOR UPDATE")
                .bind(observed.id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(database_error)?
                .ok_or(WorkError::DependencyUnavailable)?;
        workflow.validate_integrity()?;
        workflow.attach_authority(Arc::new(policy), OffsetDateTime::now_utc());
        workflow.attach_acknowledgements(actor, acknowledged);
        workflow.attach_verified_generations(verified);
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
            let owner = sqlx::query_scalar::<_, Option<Uuid>>(
                "SELECT workflow_id FROM work.operation_ledger WHERE operation_id=$1",
            )
            .bind(operation_id)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?
            .ok_or(WorkError::WorkItemNotFound)?;
            return Ok(AgentExecutionAcceptance {
                outcome: self.disclose_result(actor, owner, outcome).await?,
                dispatch: false,
            });
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
        for before in &observed.agent_executions {
            if before.status.is_active()
                && let Some(after) = workflow
                    .agent_executions
                    .iter()
                    .find(|e| e.id == before.id && e.status == AgentExecutionStatus::Failed)
            {
                agent::stage(&mut tx, workflow.id, after, "agent_execution_failed").await?;
            }
        }
        validate_record_collections(&workflow)?;
        let actual_sources = source_set(&workflow.result_evidence(actor, &result)?);
        receipt.validate(actor, observed.revision, &digest, &actual_sources)?;
        let action = match &result {
            MutationResult::AgentExecutionRequested { .. } => "agent_execution_requested",
            MutationResult::AgentExecutionCancelled { .. } => "agent_execution_cancelled",
            MutationResult::EvidenceRegistered { .. } => "evidence_registered",
            MutationResult::FindingRegistered { .. } => "finding_registered",
            MutationResult::DecisionRecorded { .. } => "decision_recorded",
            MutationResult::DraftSaved { .. } => "draft_saved",
            MutationResult::ArtifactCreated { .. } => "artifact_created",
            MutationResult::ArtifactContentWritten { .. } => "artifact_content_written",
            MutationResult::ArtifactDiscarded { .. } => "artifact_discarded",
            MutationResult::SubmissionImported { .. } => "submission_imported",
            MutationResult::Claimed { .. } => "claimed",
            MutationResult::Submitted { .. } => "submitted",
            MutationResult::Returned { .. } => "returned",
            MutationResult::Completed { .. } => "completed",
            MutationResult::Held { .. } => "held",
            MutationResult::Resumed { .. } => "resumed",
            MutationResult::Assigned { .. } => "assigned",
            MutationResult::RoleAssignmentCreated { .. }
            | MutationResult::RoleAssignmentRevoked { .. }
            | MutationResult::DelegationCreated { .. }
            | MutationResult::DelegationRevoked { .. } => {
                return Err(WorkError::IntegrityViolation);
            }
        };
        sqlx::query("UPDATE work.workflow_instances SET revision=$2,body=$3 WHERE id=$1")
            .bind(workflow.id)
            .bind(workflow.revision)
            .bind(Json(&workflow))
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        // Instances lock independently: the same operation ID committed first for a
        // different instance is an operation conflict, not an unavailable dependency.
        sqlx::query("INSERT INTO work.operation_ledger(operation_id,workflow_id,principal_id,acting_assignment_id,command_digest,outcome) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(operation_id).bind(workflow.id).bind(actor.principal_id()).bind(command.context().acting_assignment_id).bind(&digest).bind(Json(&result)).execute(&mut *tx).await.map_err(ledger_error)?;
        // Candidate/decision records are not workflow transitions.
        if matches!(
            action,
            "submitted" | "claimed" | "returned" | "completed" | "held" | "resumed" | "assigned"
        ) {
            sqlx::query("INSERT INTO work.workflow_history(id,workflow_id,operation_id,kind,occurred_at) VALUES($1,$2,$3,$4,$5)")
                .bind(Uuid::now_v7()).bind(workflow.id).bind(operation_id).bind(action).bind(now).execute(&mut *tx).await.map_err(database_error)?;
        }
        let payload = work_event_payload(&workflow, operation_id, &command, &result);
        sqlx::query("INSERT INTO work.event_staging(id,operation_id,workflow_id,principal_id,acting_assignment_id,task_id,action,occurred_at,payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(Uuid::now_v7()).bind(operation_id).bind(workflow.id).bind(actor.principal_id()).bind(command.context().acting_assignment_id).bind(command.task_id()).bind(action).bind(now).bind(Json(payload)).execute(&mut *tx).await.map_err(database_error)?;
        // Final local/freshness check under the same lock, with no remote call.
        receipt.validate(actor, observed.revision, &digest, &actual_sources)?;
        if tx.commit().await.is_err() {
            if matches!(&command, Command::RequestAgentExecution { .. }) {
                let _ = self
                    .reconcile_uncertain_request(actor, &command, &digest)
                    .await;
            }
            return Err(WorkError::CommitOutcomeUnknown);
        }
        let dispatch = matches!(&command, Command::RequestAgentExecution { .. });
        match self.disclose_result(actor, workflow.id, result).await {
            Ok(outcome) => Ok(AgentExecutionAcceptance { dispatch, outcome }),
            Err(error) => {
                // Commit is known here, but no owned dispatch has been admitted.
                // Close the undisclosed queue without claiming an execution occurred.
                // A bookkeeping failure remains unknown and never authorizes a retry.
                if dispatch {
                    let _ = self
                        .fail_agent(actor, operation_id, AgentFailureCode::DependencyUnavailable)
                        .await;
                }
                Err(error)
            }
        }
    }
}
type LedgerRow = (String, Vec<u8>, Json<MutationResult>, bool);
impl PostgresWorkRepository {
    async fn ledger(&self, operation_id: Uuid) -> Result<Option<LedgerRow>, WorkError> {
        sqlx::query_as("SELECT principal_id, command_digest, outcome, policy_id IS NOT NULL FROM work.operation_ledger WHERE operation_id=$1")
            .bind(operation_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)
    }
    async fn recover_policy(
        &self,
        actor: VerifiedActor,
        outcome: MutationResult,
    ) -> Result<MutationResult, WorkError> {
        self.load_policy()
            .await?
            .authorize_recovery(actor, &outcome, OffsetDateTime::now_utc())?;
        Ok(outcome)
    }
    /// Separate policy aggregate: update lock, OCC, ledger and mandatory local
    /// staging commit together. Exact replay returns the committed receipt only
    /// while its record remains visible to the same actor.
    async fn execute_policy_command(
        &self,
        actor: VerifiedActor,
        command: PolicyCommand,
    ) -> Result<MutationResult, WorkError> {
        let context = command.context().clone();
        if context.operation_id.get_version_num() != 7 || context.expected_revision < 0 {
            return Err(WorkError::ValidationFailed);
        }
        let digest = policy_command_digest(actor, &command)?;
        let replay = |principal: String, stored: Vec<u8>, outcome: MutationResult, policy: bool| {
            if principal != actor.principal_id() {
                return Err(WorkError::OrganizationRecordNotFound);
            }
            if stored != digest || !policy {
                return Err(WorkError::OperationConflict);
            }
            Ok(outcome)
        };
        if let Some((principal, stored, Json(outcome), policy)) =
            self.ledger(context.operation_id).await?
        {
            let outcome = replay(principal, stored, outcome, policy)?;
            return self.recover_policy(actor, outcome).await;
        }
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let mut policy = lock_policy(&mut tx, "FOR UPDATE").await?;
        let previous: Option<LedgerRow> = sqlx::query_as("SELECT principal_id, command_digest, outcome, policy_id IS NOT NULL FROM work.operation_ledger WHERE operation_id=$1")
            .bind(context.operation_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(database_error)?;
        if let Some((principal, stored, Json(outcome), is_policy)) = previous {
            let outcome = replay(principal, stored, outcome, is_policy)?;
            tx.rollback().await.map_err(database_error)?;
            return self.recover_policy(actor, outcome).await;
        }
        let now = OffsetDateTime::now_utc();
        let timestamp = now
            .format(&Rfc3339)
            .map_err(|_| WorkError::IntegrityViolation)?;
        let result = policy.apply(actor, &command, &timestamp)?;
        let (action, payload) = policy_event(&context, &policy, &result)?;
        sqlx::query("UPDATE work.organization_policies SET revision=$2, body=$3 WHERE id=$1")
            .bind(ORGANIZATION_POLICY_ID)
            .bind(policy.revision)
            .bind(Json(&policy))
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        sqlx::query("INSERT INTO work.operation_ledger(operation_id,workflow_id,policy_id,principal_id,acting_assignment_id,command_digest,outcome) VALUES($1,NULL,$2,$3,$4,$5,$6)")
            .bind(context.operation_id).bind(ORGANIZATION_POLICY_ID).bind(actor.principal_id()).bind(context.acting_assignment_id).bind(&digest).bind(Json(&result))
            .execute(&mut *tx).await.map_err(database_error)?;
        sqlx::query("INSERT INTO work.event_staging(id,operation_id,workflow_id,policy_id,principal_id,acting_assignment_id,task_id,action,occurred_at,payload) VALUES($1,$2,NULL,$3,$4,$5,NULL,$6,$7,$8)")
            .bind(Uuid::now_v7()).bind(context.operation_id).bind(ORGANIZATION_POLICY_ID).bind(actor.principal_id()).bind(context.acting_assignment_id).bind(action).bind(now).bind(Json(payload))
            .execute(&mut *tx).await.map_err(database_error)?;
        tx.commit()
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        Ok(result)
    }
}
/// Structured policy staging without user-authored reason text.
fn policy_event(
    context: &CommandContext,
    policy: &OrganizationPolicy,
    result: &MutationResult,
) -> Result<(&'static str, serde_json::Value), WorkError> {
    let acting = policy.describe(context.acting_assignment_id);
    let base = serde_json::json!({
        "schemaVersion": 1,
        "result": "committed",
        "operationId": context.operation_id,
        "policyRevision": policy.revision,
        "actingResponsibilityKind": acting.as_ref().map(|value| value.kind),
        "actingRoleId": acting.as_ref().map(|value| value.role_id),
    });
    let mut payload = base;
    let action = match result {
        MutationResult::RoleAssignmentCreated { assignment, .. }
        | MutationResult::RoleAssignmentRevoked { assignment, .. } => {
            payload["resourceType"] = "role_assignment".into();
            payload["recordId"] = serde_json::json!(assignment.id);
            payload["subjectPrincipalId"] = assignment.principal.principal_id().into();
            payload["roleId"] = serde_json::json!(assignment.role_id);
            payload["unitId"] = serde_json::json!(assignment.unit_id);
            payload["validFrom"] = assignment.valid_from.clone().into();
            payload["validUntil"] = serde_json::json!(assignment.valid_until);
            if matches!(result, MutationResult::RoleAssignmentCreated { .. }) {
                "role_assignment_created"
            } else {
                "role_assignment_revoked"
            }
        }
        MutationResult::DelegationCreated { delegation, .. }
        | MutationResult::DelegationRevoked { delegation, .. } => {
            payload["resourceType"] = "delegation".into();
            payload["recordId"] = serde_json::json!(delegation.id);
            payload["sourceAssignmentId"] = serde_json::json!(delegation.source_assignment_id);
            payload["delegatorPrincipalId"] = delegation.delegator.principal_id().into();
            payload["recipientPrincipalId"] = delegation.recipient.principal_id().into();
            payload["actions"] = serde_json::json!(delegation.actions);
            payload["validFrom"] = delegation.valid_from.clone().into();
            payload["validUntil"] = delegation.valid_until.clone().into();
            if matches!(result, MutationResult::DelegationCreated { .. }) {
                "delegation_created"
            } else {
                "delegation_revoked"
            }
        }
        _ => return Err(WorkError::IntegrityViolation),
    };
    Ok((action, payload))
}
/// A complete visible collection must stay retrievable after every mutation,
/// including claim/resubmit introducing received membership. This fixed two-person
/// PoC is deliberately bounded, rather than pretending to provide pagination.
fn validate_record_collections(workflow: &Workflow) -> Result<(), WorkError> {
    let tasks: Vec<Uuid> = std::iter::once(workflow.source.id)
        .chain(workflow.next.as_ref().map(|item| item.id))
        .collect();
    for (actor, task) in VerifiedActor::ALL
        .into_iter()
        .flat_map(|actor| tasks.iter().map(move |task| (actor, *task)))
    {
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
/// Lock the single Organization policy row; absence is an unavailable dependency.
async fn lock_policy(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    mode: &str,
) -> Result<OrganizationPolicy, WorkError> {
    let sql = match mode {
        "FOR SHARE" => "SELECT body FROM work.organization_policies WHERE id=$1 FOR SHARE",
        _ => "SELECT body FROM work.organization_policies WHERE id=$1 FOR UPDATE",
    };
    let Json(policy): Json<OrganizationPolicy> = sqlx::query_scalar(sql)
        .bind(ORGANIZATION_POLICY_ID)
        .fetch_optional(&mut **tx)
        .await
        .map_err(database_error)?
        .ok_or(WorkError::DependencyUnavailable)?;
    policy.validate_integrity()?;
    Ok(policy)
}
/// Local mandatory staging only (not Audit delivery). Carries the acting
/// responsibility kind and delegator so a later reviewed Audit extension can
/// distinguish formal assignment from delegation without free text.
fn work_event_payload(
    workflow: &Workflow,
    operation_id: Uuid,
    command: &Command,
    result: &MutationResult,
) -> serde_json::Value {
    let acting = command.context().acting_assignment_id;
    let described = workflow.describe_responsibility(acting);
    let mut payload = serde_json::json!({
        "schemaVersion": 1,
        "resourceType": "work_item",
        "result": "committed",
        "operationId": operation_id,
        "actingResponsibilityKind": described.as_ref().map(|value| value.kind),
        "actingRoleId": described.as_ref().map(|value| value.role_id),
        "delegationId": described.as_ref().and_then(|value| {
            (value.kind == ResponsibilityKind::Delegation).then_some(value.id)
        }),
        "delegatorPrincipalId": described
            .as_ref()
            .and_then(|value| value.delegator)
            .map(|value| value.principal_id()),
    });
    if let MutationResult::Assigned { assignment, .. } = result {
        payload["assigneePrincipalId"] = assignment.principal.principal_id().into();
        payload["assigneeResponsibilityId"] = serde_json::json!(assignment.acting_assignment_id);
        payload["attemptId"] = serde_json::json!(assignment.attempt_id);
    }
    files::stage_payload(&mut payload, result);
    payload
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
fn validate_disclosure_freshness(started: Instant, completed: Instant) -> Result<(), WorkError> {
    if completed.duration_since(started) > PREFLIGHT_LIFETIME {
        diagnostic_phase(Phase::Freshness, Dependency::None);
        Err(WorkError::DependencyUnavailable)
    } else {
        Ok(())
    }
}
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
fn database_error(error: sqlx::Error) -> WorkError {
    let _ = FINDING_DIAGNOSTIC.try_with(|state| {
        let class = match &error {
            sqlx::Error::Io(_) | sqlx::Error::Tls(_) => SqlClass::Connection,
            sqlx::Error::PoolTimedOut => SqlClass::AcquireTimeout,
            sqlx::Error::PoolClosed => SqlClass::PoolClosed,
            sqlx::Error::Decode(_) | sqlx::Error::ColumnDecode { .. } => SqlClass::Decode,
            sqlx::Error::Database(database) => SqlClass::database(database.code().as_deref()),
            _ => SqlClass::Other,
        };
        state.borrow_mut().sql = class;
    });
    WorkError::DependencyUnavailable
}
fn ledger_error(error: sqlx::Error) -> WorkError {
    match error.as_database_error() {
        Some(value) if value.is_unique_violation() => WorkError::OperationConflict,
        _ => WorkError::DependencyUnavailable,
    }
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
    let ready: bool = sqlx::query_scalar("SELECT to_regclass('work.workflow_instances') IS NOT NULL AND to_regclass('work.operation_ledger') IS NOT NULL AND to_regclass('work.workflow_history') IS NOT NULL AND to_regclass('work.event_staging') IS NOT NULL AND to_regclass('work.organization_policies') IS NOT NULL AND to_regclass('work.attention_acknowledgements') IS NOT NULL")
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
    // The separately owned synthetic Organization policy; never overwritten.
    let policy = OrganizationPolicy::synthetic();
    sqlx::query("INSERT INTO work.organization_policies(id,revision,body) VALUES($1,0,$2) ON CONFLICT(id) DO NOTHING")
        .bind(ORGANIZATION_POLICY_ID).bind(Json(policy)).execute(pool).await.map_err(database_error)?;
    insert_fixtures(pool, document_id, &CONTEXT_FIXTURES[..1]).await
}
/// Explicit, separately invoked seed of the additional synthetic WorkContexts
/// (U2). The original single-context fixture and its accepted journeys stay as
/// they were; an existing instance and its progress are never overwritten.
pub async fn seed_synthetic_contexts(
    pool: &PgPool,
    document_id: Option<Uuid>,
) -> Result<(), WorkError> {
    check_schema_compatibility(pool).await?;
    let present: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work.organization_policies WHERE id=$1)")
            .bind(ORGANIZATION_POLICY_ID)
            .fetch_one(pool)
            .await
            .map_err(database_error)?;
    if !present {
        return Err(WorkError::DependencyUnavailable);
    }
    insert_fixtures(pool, document_id, &CONTEXT_FIXTURES[1..]).await
}
async fn insert_fixtures(
    pool: &PgPool,
    document_id: Option<Uuid>,
    fixtures: &[ContextFixture],
) -> Result<(), WorkError> {
    let seeded_at = OffsetDateTime::now_utc();
    for fixture in fixtures {
        let workflow = Workflow::from_fixture(fixture, document_id, seeded_at)?;
        sqlx::query("INSERT INTO work.workflow_instances(id,revision,body) VALUES($1,0,$2) ON CONFLICT(id) DO NOTHING")
            .bind(workflow.id).bind(Json(&workflow)).execute(pool).await.map_err(database_error)?;
    }
    Ok(())
}
impl WorkRepository for PostgresWorkRepository {
    fn request_agent_execution(
        &self,
        actor: VerifiedActor,
        command: Command,
    ) -> WorkFuture<'_, AgentExecutionAcceptance> {
        Box::pin(async move {
            if !matches!(&command, Command::RequestAgentExecution { .. }) {
                return Err(WorkError::ValidationFailed);
            }
            self.execute_command_receipt(actor, command).await
        })
    }
    fn agent_execution(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, AgentExecution> {
        Box::pin(self.read_agent_execution(actor, id))
    }
    fn agent_result(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, AgentResult> {
        Box::pin(async move {
            let e = self.read_agent_execution(actor, id).await?;
            if e.status != AgentExecutionStatus::Succeeded {
                return Err(WorkError::AgentResultNotReady);
            }
            e.result.ok_or(WorkError::IntegrityViolation)
        })
    }
    fn generated_artifact(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, GeneratedArtifact> {
        Box::pin(
            self.read_candidate(actor, WorkTarget::GeneratedArtifact(id), move |w| {
                let record = w.generated_artifact(actor, id)?;
                Ok((record.execution_id, record))
            }),
        )
    }
    fn suggested_action(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, SuggestedAction> {
        Box::pin(
            self.read_candidate(actor, WorkTarget::SuggestedAction(id), move |w| {
                let record = w.suggested_action(actor, id)?;
                Ok((record.execution_id, record))
            }),
        )
    }
    fn start_agent_execution(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, Option<AgentDispatchContext>> {
        Box::pin(self.start_agent(actor, id))
    }
    fn build_agent_context(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, AgentDispatchContext> {
        Box::pin(self.current_agent_context(actor, id))
    }
    fn finish_agent_execution(
        &self,
        context: AgentDispatchContext,
        output: AgentOutput,
    ) -> WorkFuture<'_, AgentExecution> {
        Box::pin(self.finish_agent(context, output))
    }
    fn fail_agent_execution(
        &self,
        actor: VerifiedActor,
        id: Uuid,
        code: AgentFailureCode,
    ) -> WorkFuture<'_, AgentExecution> {
        Box::pin(self.fail_agent(actor, id, code))
    }
    fn interrupt_agent_executions(&self, actor: VerifiedActor) -> WorkFuture<'_, usize> {
        Box::pin(self.interrupt_agents(actor))
    }

    fn list_evidence(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
    ) -> WorkFuture<'_, Vec<EvidenceRecord>> {
        Box::pin(async move {
            let w = self.load_for(actor, WorkTarget::Task(task_id)).await?;
            let evidence = w.list_evidence(actor, task_id)?;
            self.verify_read_sources(actor, observed(&w), evidence.clone())
                .await?;
            Ok(evidence)
        })
    }
    fn evidence(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, EvidenceRecord> {
        Box::pin(async move {
            let w = self.load_for(actor, WorkTarget::Evidence(id)).await?;
            let evidence = w.evidence_record(actor, id)?;
            self.verify_read_sources(actor, observed(&w), vec![evidence.clone()])
                .await?;
            Ok(evidence)
        })
    }
    fn list_findings(&self, actor: VerifiedActor, task_id: Uuid) -> WorkFuture<'_, Vec<Finding>> {
        Box::pin(diagnose_finding(true, async move {
            let started = Instant::now();
            let w = self.load_for(actor, WorkTarget::Task(task_id)).await?;
            let findings = w.list_findings(actor, task_id)?;
            diagnostic_phase(Phase::Agent, Dependency::Agent);
            self.verify_agent_origins(&w, &findings, observed(&w))
                .await?;
            let mut evidence = vec![];
            for f in &findings {
                evidence.extend(w.resolve_evidence(actor, &f.evidence_revision_refs)?);
            }
            diagnostic_phase(Phase::Evidence, Dependency::Document);
            self.verify_read_sources(actor, observed(&w), evidence)
                .await?;
            diagnostic_phase(Phase::Freshness, Dependency::None);
            validate_disclosure_freshness(started, Instant::now())?;
            Ok(findings)
        }))
    }
    fn finding(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, Finding> {
        Box::pin(diagnose_finding(false, async move {
            let started = Instant::now();
            let w = self.load_for(actor, WorkTarget::Finding(id)).await?;
            diagnostic_phase(Phase::Load, Dependency::Work);
            let finding = w.finding(actor, id)?;
            diagnostic_phase(Phase::Agent, Dependency::Agent);
            self.verify_agent_origins(&w, std::slice::from_ref(&finding), observed(&w))
                .await?;
            diagnostic_phase(Phase::Evidence, Dependency::Document);
            self.verify_read_sources(
                actor,
                observed(&w),
                w.resolve_evidence(actor, &finding.evidence_revision_refs)?,
            )
            .await?;
            diagnostic_phase(Phase::Freshness, Dependency::None);
            validate_disclosure_freshness(started, Instant::now())?;
            Ok(finding)
        }))
    }
    fn list_decisions(
        &self,
        actor: VerifiedActor,
        finding_id: Uuid,
    ) -> WorkFuture<'_, Vec<HumanDecision>> {
        Box::pin(async move {
            let started = Instant::now();
            let w = self
                .load_for(actor, WorkTarget::Finding(finding_id))
                .await?;
            let finding = w.finding(actor, finding_id)?;
            self.verify_agent_origins(&w, std::slice::from_ref(&finding), observed(&w))
                .await?;
            let decisions = w.list_decisions(actor, finding_id)?;
            let mut evidence = w.resolve_evidence(actor, &finding.evidence_revision_refs)?;
            for d in &decisions {
                evidence.extend(w.resolve_evidence(actor, &d.evidence_revision_refs)?);
            }
            self.verify_read_sources(actor, observed(&w), evidence)
                .await?;
            validate_disclosure_freshness(started, Instant::now())?;
            Ok(decisions)
        })
    }
    fn organization(&self, actor: VerifiedActor) -> WorkFuture<'_, OrganizationView> {
        Box::pin(async move {
            self.load_policy()
                .await?
                .view(actor, OffsetDateTime::now_utc())
        })
    }
    fn execute_policy(
        &self,
        actor: VerifiedActor,
        command: PolicyCommand,
    ) -> WorkFuture<'_, MutationResult> {
        Box::pin(self.execute_policy_command(actor, command))
    }
    fn list_tasks_in(
        &self,
        actor: VerifiedActor,
        view: TaskView,
        scope: Option<Uuid>,
    ) -> WorkFuture<'_, Vec<TaskSummary>> {
        Box::pin(async move {
            let mut items = vec![];
            for workflow in self.load_all(actor).await? {
                items.extend(workflow.list_tasks_in(actor, view, scope)?);
            }
            Ok(items)
        })
    }
    fn list_tasks(&self, actor: VerifiedActor, view: TaskView) -> WorkFuture<'_, Vec<TaskSummary>> {
        self.list_tasks_in(actor, view, None)
    }
    fn list_work_contexts(
        &self,
        actor: VerifiedActor,
        scope: Option<Uuid>,
    ) -> WorkFuture<'_, Vec<WorkContextView>> {
        Box::pin(async move {
            if let Some(id) = scope
                && self
                    .load_policy()
                    .await?
                    .responsibility(actor, id, OffsetDateTime::now_utc())
                    .is_none()
            {
                return Err(WorkError::Forbidden);
            }
            Ok(self
                .load_all(actor)
                .await?
                .iter()
                .filter_map(|workflow| workflow.context_view(actor, scope))
                .collect())
        })
    }
    fn work_context(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, WorkContextView> {
        Box::pin(async move {
            let workflow = self.load_for(actor, WorkTarget::Context(id)).await?;
            if !workflow.owns(WorkTarget::Context(id)) {
                return Err(WorkError::WorkContextNotFound);
            }
            workflow
                .context_view(actor, None)
                .ok_or(WorkError::WorkContextNotFound)
        })
    }
    fn work_context_history(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, WorkContextHistory> {
        Box::pin(async move {
            let workflow = self.load_for(actor, WorkTarget::Context(id)).await?;
            if !workflow.owns(WorkTarget::Context(id)) {
                return Err(WorkError::WorkContextNotFound);
            }
            workflow.context_history(actor)
        })
    }
    fn task_attention(&self, actor: VerifiedActor, task_id: Uuid) -> WorkFuture<'_, TaskAttention> {
        Box::pin(async move {
            self.load_for(actor, WorkTarget::Task(task_id))
                .await?
                .attention(actor, task_id)
        })
    }
    fn acknowledge_attention(
        &self,
        actor: VerifiedActor,
        task_id: Uuid,
        work_assignment_id: Uuid,
    ) -> WorkFuture<'_, TaskAttention> {
        Box::pin(async move {
            let workflow = self.load_for(actor, WorkTarget::Task(task_id)).await?;
            let acknowledged = workflow.acknowledge(actor, task_id, work_assignment_id)?;
            // Presentation state only: no Work revision, ledger or business staging.
            sqlx::query("INSERT INTO work.attention_acknowledgements(principal_id,work_assignment_id,workflow_id,task_id,attempt_id,acknowledged_at) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT (principal_id, work_assignment_id) DO NOTHING")
                .bind(actor.principal_id()).bind(acknowledged.work_assignment_id).bind(workflow.id).bind(acknowledged.task_id).bind(acknowledged.attempt_id).bind(OffsetDateTime::now_utc())
                .execute(&self.pool).await.map_err(database_error)?;
            self.load_id(actor, workflow.id)
                .await?
                .attention(actor, task_id)
        })
    }
    fn task(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, TaskDetail> {
        Box::pin(async move {
            self.load_for(actor, WorkTarget::Task(id))
                .await?
                .detail(actor, id)
        })
    }
    fn artifact(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, WorkingArtifact> {
        Box::pin(async move {
            self.load_for(actor, WorkTarget::Artifact(id))
                .await?
                .artifact(actor, id)
        })
    }
    fn snapshot(&self, actor: VerifiedActor, id: Uuid) -> WorkFuture<'_, HandoffSnapshot> {
        Box::pin(async move {
            let started = Instant::now();
            let workflow = self.load_for(actor, WorkTarget::Snapshot(id)).await?;
            let snapshot = workflow.snapshot(actor, id)?;
            self.verify_snapshot_agent_sources(&workflow, &snapshot, observed(&workflow))
                .await?;
            self.verify_read_sources(
                actor,
                observed(&workflow),
                workflow.snapshot_evidence(actor, id)?,
            )
            .await?;
            validate_disclosure_freshness(started, Instant::now())?;
            Ok(snapshot)
        })
    }
    fn return_instruction(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> WorkFuture<'_, ReturnInstruction> {
        Box::pin(async move {
            self.load_for(actor, WorkTarget::ReturnInstruction(id))
                .await?
                .return_instruction(actor, id)
        })
    }
    fn artifact_store_available(&self) -> bool {
        self.artifact_store.is_some()
    }
    fn write_artifact_content(
        &self,
        actor: VerifiedActor,
        artifact_id: Uuid,
        context: CommandContext,
        expected_artifact_revision: i64,
        bytes: Vec<u8>,
    ) -> WorkFuture<'_, MutationResult> {
        Box::pin(async move {
            self.write_content(
                actor,
                artifact_id,
                context,
                expected_artifact_revision,
                bytes,
            )
            .await
        })
    }
    fn artifact_content(
        &self,
        actor: VerifiedActor,
        artifact_id: Uuid,
    ) -> WorkFuture<'_, (WorkFile, Vec<u8>)> {
        Box::pin(async move { self.read_artifact_content(actor, artifact_id).await })
    }
    fn snapshot_content(
        &self,
        actor: VerifiedActor,
        snapshot_id: Uuid,
        artifact_id: Uuid,
    ) -> WorkFuture<'_, (WorkFile, Vec<u8>)> {
        Box::pin(async move {
            self.read_snapshot_content(actor, snapshot_id, artifact_id)
                .await
        })
    }
    fn execute(&self, actor: VerifiedActor, command: Command) -> WorkFuture<'_, MutationResult> {
        Box::pin(async move { Ok(self.execute_command_receipt(actor, command).await?.outcome) })
    }
    fn recover(&self, actor: VerifiedActor, operation_id: Uuid) -> WorkFuture<'_, MutationResult> {
        Box::pin(async move {
            if let Some((principal, _, Json(outcome), true)) = self.ledger(operation_id).await? {
                if principal != actor.principal_id() {
                    return Err(WorkError::WorkItemNotFound);
                }
                return self.recover_policy(actor, outcome).await;
            }
            let (workflow, principal, _, outcome) = self
                .operation(actor, operation_id)
                .await?
                .ok_or(WorkError::WorkItemNotFound)?;
            if principal != actor.principal_id() {
                return Err(WorkError::WorkItemNotFound);
            }
            workflow.authorize_recovery(actor, &outcome)?;
            self.disclose_result(actor, workflow.id, outcome).await
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
    #[test]
    fn composed_authorization_phases_require_one_disclosure_deadline() {
        // Deterministic dependency fakes: provider and requester each consume 3s.
        // Either private 5s receipt is valid in isolation; the first is stale at disclosure.
        fn authorize(start: Instant) -> Instant {
            start + Duration::from_secs(3)
        }
        let start = Instant::now();
        let provider_done = authorize(start);
        let requester_done = authorize(provider_done);
        assert_eq!(validate_disclosure_freshness(start, provider_done), Ok(()));
        assert_eq!(
            validate_disclosure_freshness(provider_done, requester_done),
            Ok(())
        );
        assert_eq!(
            validate_disclosure_freshness(start, requester_done),
            Err(WorkError::DependencyUnavailable)
        );
        assert_eq!(
            validate_disclosure_freshness(start, start + PREFLIGHT_LIFETIME),
            Ok(())
        );
    }
}

#[cfg(test)]
mod finding_diagnostic_tests {
    use super::*;
    #[tokio::test]
    async fn success_and_unscoped_calls_are_silent() {
        diagnostic_phase(Phase::Evidence, Dependency::Document);
        let result = diagnose_finding_with(false, async { Ok::<_, WorkError>(7) }, |_| {
            panic!("success emitted diagnostics")
        })
        .await;
        assert_eq!(result, Ok(7));
        assert!(FINDING_DIAGNOSTIC.try_with(|_| ()).is_err());
    }
    #[tokio::test]
    async fn failure_preserves_error_and_projects_sql_provenance() {
        let mut lines = vec![];
        let result = diagnose_finding_with(
            false,
            async {
                diagnostic_phase(Phase::Policy, Dependency::Work);
                Err::<(), _>(database_error(sqlx::Error::PoolTimedOut))
            },
            |line| lines.push(line),
        )
        .await;
        assert_eq!(result, Err(WorkError::DependencyUnavailable));
        assert_eq!(lines.len(), 1);
        let value: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(value["phase"], "policy");
        assert_eq!(value["dependency"], "work");
        assert_eq!(value["sql_class"], "acquire_timeout");
        assert_eq!(value["failure"], "dependency_unavailable");
        assert!(FINDING_DIAGNOSTIC.try_with(|_| ()).is_err());
    }
    #[tokio::test]
    async fn cancelled_pending_read_emits_nothing_and_restores_scope() {
        use std::future::Future;
        let mut future = Box::pin(diagnose_finding_with(
            false,
            async {
                diagnostic_phase(Phase::Agent, Dependency::Agent);
                std::future::pending::<()>().await;
                Ok::<(), WorkError>(())
            },
            |_| panic!("cancelled read emitted diagnostics"),
        ));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_pending());
        assert!(FINDING_DIAGNOSTIC.try_with(|_| ()).is_err());
        drop(future);
        assert!(FINDING_DIAGNOSTIC.try_with(|_| ()).is_err());
    }
    #[tokio::test]
    async fn concurrent_futures_do_not_share_diagnostic_state() {
        let first = diagnose_finding_with(
            false,
            async {
                diagnostic_phase(Phase::Agent, Dependency::Agent);
                tokio::task::yield_now().await;
                assert_eq!(FINDING_DIAGNOSTIC.with(|s| s.borrow().phase), Phase::Agent);
                Err::<(), _>(WorkError::DependencyUnavailable)
            },
            |line| assert!(line.contains("\"phase\":\"agent\"")),
        );
        let second = diagnose_finding_with(
            true,
            async {
                diagnostic_phase(Phase::Evidence, Dependency::Document);
                tokio::task::yield_now().await;
                assert_eq!(
                    FINDING_DIAGNOSTIC.with(|s| s.borrow().phase),
                    Phase::Evidence
                );
                Err::<(), _>(WorkError::FindingNotFound)
            },
            |line| assert!(line.contains("\"operation\":\"finding_list\"")),
        );
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first, Err(WorkError::DependencyUnavailable));
        assert_eq!(second, Err(WorkError::FindingNotFound));
        assert!(FINDING_DIAGNOSTIC.try_with(|_| ()).is_err());
    }
}
