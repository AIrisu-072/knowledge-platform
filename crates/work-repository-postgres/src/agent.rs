//! Owned one-shot execution transactions. Provider work always precedes row locks.
use super::*;
impl PostgresWorkRepository {
    pub fn with_agent_source(
        pool: PgPool,
        evidence_source: Arc<dyn EvidenceSourcePort>,
        agent_source: Arc<dyn AgentSourcePort>,
    ) -> Self {
        Self {
            pool,
            evidence_source: Some(evidence_source),
            agent_source: Some(agent_source),
        }
    }
    pub(super) async fn authorize_agent_context(
        &self,
        context: AgentDispatchContext,
        revision: i64,
    ) -> Result<(), WorkError> {
        self.authorize_agent_context_started(context, revision, Instant::now())
            .await
    }
    async fn authorize_agent_context_started(
        &self,
        context: AgentDispatchContext,
        revision: i64,
        started: Instant,
    ) -> Result<(), WorkError> {
        context.validate_scope()?;
        for reference in &context.execution.evidence_revision_refs {
            if self.load().await?.revision != revision {
                return Err(WorkError::WorkContextStale);
            }
            let remaining = PREFLIGHT_LIFETIME
                .checked_sub(started.elapsed())
                .ok_or(WorkError::DependencyUnavailable)?;
            self.agent_source
                .as_ref()
                .ok_or(WorkError::DependencyUnavailable)?
                .authorize(context.clone(), reference.clone(), remaining)
                .await?;
            if self.load().await?.revision != revision {
                return Err(WorkError::WorkContextStale);
            }
            fresh(started)?;
        }
        Ok(())
    }
    pub(super) async fn verify_agent_origins(
        &self,
        w: &Workflow,
        findings: &[Finding],
        revision: i64,
    ) -> Result<(), WorkError> {
        let started = Instant::now();
        let ids: std::collections::BTreeSet<_> = findings
            .iter()
            .filter_map(|f| f.origin_execution_id)
            .collect();
        for id in ids {
            fresh(started)?;
            self.authorize_agent_context_started(
                w.agent_disclosure_context(id)?,
                revision,
                started,
            )
            .await?;
        }
        fresh(started)
    }
    pub(super) async fn verify_snapshot_agent_sources(
        &self,
        w: &Workflow,
        snapshot: &HandoffSnapshot,
        revision: i64,
    ) -> Result<(), WorkError> {
        let findings = w
            .findings
            .iter()
            .filter(|f| {
                snapshot.finding_revision_refs.contains(&RevisionRef {
                    id: f.id,
                    revision: f.revision,
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        self.verify_agent_origins(w, &findings, revision).await
    }
    pub(super) async fn verify_result_agent_sources(
        &self,
        w: &Workflow,
        result: &MutationResult,
        revision: i64,
    ) -> Result<(), WorkError> {
        match result {
            MutationResult::AgentExecutionRequested { execution, .. }
            | MutationResult::AgentExecutionCancelled { execution, .. } => {
                self.authorize_agent_context(w.agent_disclosure_context(execution.id)?, revision)
                    .await
            }
            MutationResult::FindingRegistered { finding, .. } => {
                self.verify_agent_origins(w, std::slice::from_ref(finding), revision)
                    .await
            }
            MutationResult::DecisionRecorded { decision, .. } => {
                let finding = w
                    .findings
                    .iter()
                    .find(|f| f.id == decision.finding_id)
                    .ok_or(WorkError::IntegrityViolation)?;
                self.verify_agent_origins(w, std::slice::from_ref(finding), revision)
                    .await
            }
            MutationResult::Submitted { snapshot, .. } => {
                self.verify_snapshot_agent_sources(w, snapshot, revision)
                    .await
            }
            _ => Ok(()),
        }
    }
    pub(super) async fn read_agent_execution(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<AgentExecution, WorkError> {
        let w = self.load().await?;
        let e = w.agent_execution(actor, id)?;
        let started = Instant::now();
        self.authorize_agent_context(w.agent_disclosure_context(id)?, w.revision)
            .await?;
        let current = self.load().await?;
        if current.revision != w.revision || current.agent_execution(actor, id)? != e {
            return Err(WorkError::WorkContextStale);
        }
        fresh(started)?;
        Ok(e)
    }
    pub(super) async fn current_agent_context(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<AgentDispatchContext, WorkError> {
        let observed = self.load().await?;
        let context = observed.build_agent_context(actor, id)?;
        let started = Instant::now();
        self.authorize_agent_context(context.clone(), observed.revision)
            .await?;
        if self.load().await?.build_agent_context(actor, id)? != context {
            return Err(WorkError::WorkContextStale);
        }
        fresh(started)?;
        Ok(context)
    }
    pub(super) async fn start_agent(
        &self,
        actor: VerifiedActor,
        id: Uuid,
    ) -> Result<Option<AgentDispatchContext>, WorkError> {
        let observed = self.load().await?;
        let mut preview = observed.clone();
        let Some(context) = preview.start_agent_execution(actor, id, &timestamp()?)? else {
            return Ok(None);
        };
        let started = Instant::now();
        self.authorize_agent_context(context, observed.revision)
            .await?;
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let mut w = locked(&mut tx).await?;
        if w.revision != observed.revision {
            return Err(WorkError::WorkContextStale);
        }
        let Some(context) = w.start_agent_execution(actor, id, &timestamp()?)? else {
            return Ok(None);
        };
        fresh(started)?;
        persist(&mut tx, &w, &context.execution, "agent_execution_running").await?;
        fresh(started)?;
        tx.commit()
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        Ok(Some(context))
    }
    pub(super) async fn finish_agent(
        &self,
        context: AgentDispatchContext,
        output: AgentFindingOutput,
    ) -> Result<AgentExecution, WorkError> {
        let actor = context.execution.requested_by;
        let id = context.execution.id;
        let observed = self.load().await?;
        if observed.build_agent_context(actor, id)? != context {
            return Err(WorkError::WorkContextStale);
        }
        let started = Instant::now();
        self.authorize_agent_context(context.clone(), observed.revision)
            .await?;
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let mut w = locked(&mut tx).await?;
        if w.revision != observed.revision {
            return Err(WorkError::WorkContextStale);
        }
        let execution = w.finish_agent_execution(&context, output, &timestamp()?)?;
        validate_record_collections(&w)?;
        fresh(started)?;
        persist(&mut tx, &w, &execution, "agent_execution_succeeded").await?;
        fresh(started)?;
        tx.commit()
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        // This internal return is not an HTTP disclosure; every public read reauthorizes.
        Ok(execution)
    }
    /// A lost request commit acknowledgement may have persisted its queue. Reconcile
    /// only this exact ledger actor/digest/target; absence/other requests stay untouched.
    pub(super) async fn reconcile_uncertain_request(
        &self,
        actor: VerifiedActor,
        command: &Command,
        digest: &[u8],
    ) -> Result<(), WorkError> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let mut w = locked(&mut tx).await?;
        let receipt:Option<(String,Vec<u8>,Json<MutationResult>)>=sqlx::query_as("SELECT principal_id,command_digest,outcome FROM work.operation_ledger WHERE operation_id=$1 AND workflow_id=$2").bind(command.context().operation_id).bind(WORKFLOW_ID).fetch_optional(&mut *tx).await.map_err(database_error)?;
        let Some((principal, stored_digest, Json(outcome))) = receipt else {
            return Ok(());
        };
        let Some(id) =
            matching_request_receipt(actor, command, digest, &principal, &stored_digest, &outcome)
        else {
            return Ok(());
        };
        let before = w.revision;
        let execution = w.fail_agent_execution(
            actor,
            id,
            AgentFailureCode::CommitOutcomeUnknown,
            &timestamp()?,
        )?;
        if w.revision != before {
            persist(&mut tx, &w, &execution, "agent_execution_outcome_unknown").await?;
        }
        tx.commit()
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)
    }
    pub(super) async fn fail_agent(
        &self,
        actor: VerifiedActor,
        id: Uuid,
        code: AgentFailureCode,
    ) -> Result<AgentExecution, WorkError> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let mut w = locked(&mut tx).await?;
        let before = w.revision;
        let execution = w.fail_agent_execution(actor, id, code, &timestamp()?)?;
        if w.revision != before {
            persist(
                &mut tx,
                &w,
                &execution,
                match code {
                    AgentFailureCode::Interrupted => "agent_execution_interrupted",
                    AgentFailureCode::CommitOutcomeUnknown => "agent_execution_outcome_unknown",
                    _ => "agent_execution_failed",
                },
            )
            .await?;
        }
        tx.commit()
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        Ok(execution)
    }
    pub(super) async fn interrupt_agents(&self, actor: VerifiedActor) -> Result<usize, WorkError> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        // Bootstrap starts the servers before the explicit seed-work command.
        // Only this interruption sweep treats an absent fixture as no work to stop.
        let Some(mut w) = locked_optional(&mut tx).await? else {
            tx.rollback().await.map_err(database_error)?;
            return Ok(0);
        };
        let ids: Vec<_> = w
            .agent_executions
            .iter()
            .filter(|e| e.requested_by == actor && e.status.is_active())
            .map(|e| e.id)
            .collect();
        for id in &ids {
            let e =
                w.fail_agent_execution(actor, *id, AgentFailureCode::Interrupted, &timestamp()?)?;
            persist(&mut tx, &w, &e, "agent_execution_interrupted").await?;
        }
        tx.commit()
            .await
            .map_err(|_| WorkError::CommitOutcomeUnknown)?;
        Ok(ids.len())
    }
}
fn fresh(started: Instant) -> Result<(), WorkError> {
    if started.elapsed() > PREFLIGHT_LIFETIME {
        Err(WorkError::DependencyUnavailable)
    } else {
        Ok(())
    }
}
fn timestamp() -> Result<String, WorkError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| WorkError::IntegrityViolation)
}
async fn locked(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<Workflow, WorkError> {
    locked_optional(tx)
        .await?
        .ok_or(WorkError::DependencyUnavailable)
}
async fn locked_optional(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Option<Workflow>, WorkError> {
    let row: Option<Json<Workflow>> =
        sqlx::query_scalar("SELECT body FROM work.workflow_instances WHERE id=$1 FOR UPDATE")
            .bind(WORKFLOW_ID)
            .fetch_optional(&mut **tx)
            .await
            .map_err(database_error)?;
    row.map(|Json(w)| {
        w.validate_integrity()?;
        Ok(w)
    })
    .transpose()
}
async fn persist(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    w: &Workflow,
    e: &AgentExecution,
    action: &str,
) -> Result<(), WorkError> {
    sqlx::query("UPDATE work.workflow_instances SET revision=$2,body=$3 WHERE id=$1")
        .bind(WORKFLOW_ID)
        .bind(w.revision)
        .bind(Json(w))
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
    stage(tx, e, action).await
}
pub(super) async fn stage(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    e: &AgentExecution,
    action: &str,
) -> Result<(), WorkError> {
    let payload = serde_json::json!({"schemaVersion":1,"resourceType":"agent_execution","executionId":e.id,"status":e.status,"executedBy":e.executed_by,"executorInvocationKind":e.executor_invocation_kind,"providerPrincipalBindings":e.provider_principal_bindings});
    sqlx::query("INSERT INTO work.event_staging(id,operation_id,workflow_id,principal_id,acting_assignment_id,task_id,action,occurred_at,payload) VALUES($1,NULL,$2,$3,$4,$5,$6,$7,$8)")
        .bind(Uuid::now_v7()).bind(WORKFLOW_ID).bind(e.requested_by.principal_id()).bind(e.requester_responsibility).bind(e.work_item_id).bind(action).bind(OffsetDateTime::now_utc()).bind(Json(payload)).execute(&mut **tx).await.map_err(database_error)?;
    Ok(())
}

fn matching_request_receipt(
    actor: VerifiedActor,
    command: &Command,
    digest: &[u8],
    principal: &str,
    stored_digest: &[u8],
    outcome: &MutationResult,
) -> Option<Uuid> {
    let Command::RequestAgentExecution {
        task_id,
        context,
        expected_attempt_id,
        purpose,
        evidence_revision_refs,
    } = command
    else {
        return None;
    };
    let MutationResult::AgentExecutionRequested { task, execution } = outcome else {
        return None;
    };
    (context.authorize(actor).is_ok()
        && principal == actor.principal_id()
        && stored_digest == digest
        && execution.id == context.operation_id
        && execution.requested_by == actor
        && execution.requester_responsibility == context.acting_assignment_id
        && task.id == *task_id
        && task.attempt_id == *expected_attempt_id
        && execution.work_item_id == *task_id
        && execution.attempt_id == *expected_attempt_id
        && execution.purpose == *purpose
        && execution.evidence_revision_refs == *evidence_revision_refs)
        .then_some(execution.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertain_request_reconciliation_requires_exact_actor_digest_and_target() {
        let actor = VerifiedActor::Sales01;
        let mut w = Workflow::synthetic(Some(Uuid::from_u128(71)));
        let ctx = |revision| CommandContext {
            operation_id: Uuid::now_v7(),
            expected_revision: revision,
            acting_assignment_id: SALES_ASSIGNMENT_ID,
        };
        w.apply(
            actor,
            &Command::RegisterEvidence {
                task_id: SALES_TASK_ID,
                context: ctx(0),
                expected_attempt_id: SALES_ATTEMPT_ID,
                source: EvidenceSource {
                    source_ref: SourceRef {
                        provider_id: "document".into(),
                        resource_id: Uuid::from_u128(71),
                        revision_id: Uuid::from_u128(72),
                        version_id: Uuid::from_u128(73),
                    },
                    authoritative_locator: AuthoritativeLocator {
                        kind: "contentItem".into(),
                        content_item_id: Uuid::from_u128(74),
                        representation_id: Uuid::from_u128(75),
                    },
                },
                relevant_location: "reference".into(),
            },
            "2026-10-04T00:00:00Z",
        )
        .unwrap();
        let request = Command::RequestAgentExecution {
            task_id: SALES_TASK_ID,
            context: ctx(1),
            expected_attempt_id: SALES_ATTEMPT_ID,
            purpose: "bounded request".into(),
            evidence_revision_refs: vec![RevisionRef {
                id: w.evidence[0].id,
                revision: 1,
            }],
        };
        let outcome = w.apply(actor, &request, "2026-10-04T00:00:00Z").unwrap();
        let digest = command_digest(actor, &request).unwrap();
        assert_eq!(
            matching_request_receipt(
                actor,
                &request,
                &digest,
                actor.principal_id(),
                &digest,
                &outcome
            ),
            Some(request.context().operation_id)
        );
        assert_eq!(
            matching_request_receipt(
                VerifiedActor::Office01,
                &request,
                &digest,
                actor.principal_id(),
                &digest,
                &outcome
            ),
            None
        );
        assert_eq!(
            matching_request_receipt(actor, &request, &digest, "office-01", &digest, &outcome),
            None
        );
        assert_eq!(
            matching_request_receipt(
                actor,
                &request,
                &digest,
                actor.principal_id(),
                &[0; 32],
                &outcome
            ),
            None
        );
        let mut foreign = outcome.clone();
        if let MutationResult::AgentExecutionRequested { execution, .. } = &mut foreign {
            execution.work_item_id = OFFICE_TASK_ID;
        }
        assert_eq!(
            matching_request_receipt(
                actor,
                &request,
                &digest,
                actor.principal_id(),
                &digest,
                &foreign
            ),
            None
        );
        let mut different_id = outcome.clone();
        if let MutationResult::AgentExecutionRequested { execution, .. } = &mut different_id {
            execution.id = Uuid::now_v7();
        }
        assert_eq!(
            matching_request_receipt(
                actor,
                &request,
                &digest,
                actor.principal_id(),
                &digest,
                &different_id
            ),
            None
        );
        let mut changed = request.clone();
        if let Command::RequestAgentExecution { purpose, .. } = &mut changed {
            *purpose = "changed".into();
        }
        assert_eq!(
            matching_request_receipt(
                actor,
                &changed,
                &digest,
                actor.principal_id(),
                &digest,
                &outcome
            ),
            None
        );
    }
}
