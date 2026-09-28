use document_application::{
    AUDIT_DOCUMENT_VERSION_PUBLICATION_CANCELLED, AUDIT_DOCUMENT_VERSION_PUBLICATION_SCHEDULED,
    CancelOperationRecord, CancelScheduleRecord, CancelScheduleResult,
    DOCUMENT_VERSION_PUBLICATION_CANCELLED, DOCUMENT_VERSION_PUBLICATION_SCHEDULED,
    DueTerminalRecord, PublicationScheduleRepository, PublishOperationId, RepositoryError,
    ScheduleOperationRecord, SchedulePublishCommand, SchedulePublishRecord, SchedulePublishResult,
    VerifiedActorContext, VersionOperationId,
};
use document_domain::{Action, DocumentId, DocumentVersionId, PrincipalRef, ResourceRef};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    access_control::{authorize_document_snapshot, guard_document_mutation},
    error::{map_commit_error, map_statement_error},
    publication_end,
    repository::PostgresDocumentRepository,
    versioning_mutation,
};

const AUDIT_SOURCE: &str = "urn:knowledge-platform:document-platform";

impl PublicationScheduleRepository for PostgresDocumentRepository {
    async fn authorize_due_document(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
    ) -> Result<bool, RepositoryError> {
        self.authorize_resource(
            ctx,
            ResourceRef::Document(document_id),
            &[Action::Read, Action::Publish],
        )
        .await
    }

    async fn get_schedule(
        &self,
        id: PublishOperationId,
    ) -> Result<Option<ScheduleOperationRecord>, RepositoryError> {
        let operation = get_schedule(&self.pool, id).await?;
        if let (Some(ctx), Some(operation)) = (&self.verified_actor, &operation) {
            authorize_document_snapshot(
                &self.pool,
                ctx,
                operation.command.document_id(),
                &[Action::Read, Action::Publish],
            )
            .await?;
        }
        Ok(operation)
    }
    async fn reserve(
        &self,
        record: SchedulePublishRecord,
    ) -> Result<SchedulePublishResult, RepositoryError> {
        reserve(&self.pool, record, self.verified_actor.as_ref()).await
    }
    async fn get_cancel_operation(
        &self,
        id: VersionOperationId,
    ) -> Result<Option<CancelOperationRecord>, RepositoryError> {
        let operation = get_cancel_operation(&self.pool, id).await?;
        if let (Some(ctx), Some(operation)) = (&self.verified_actor, &operation) {
            authorize_document_snapshot(
                &self.pool,
                ctx,
                operation.result.document_id,
                &[Action::Read, Action::Publish],
            )
            .await?;
        }
        Ok(operation)
    }
    async fn cancel(
        &self,
        record: CancelScheduleRecord,
    ) -> Result<CancelScheduleResult, RepositoryError> {
        cancel(&self.pool, record, self.verified_actor.as_ref()).await
    }

    async fn database_now(&self) -> Result<time::OffsetDateTime, RepositoryError> {
        sqlx::query_scalar("SELECT now()")
            .fetch_one(&self.pool)
            .await
            .map_err(map_statement_error)
    }

    async fn list_due(
        &self,
        database_now: time::OffsetDateTime,
        limit: i64,
    ) -> Result<Vec<PublishOperationId>, RepositoryError> {
        if !(1..=1000).contains(&limit) {
            return Err(RepositoryError::BusinessRule);
        }
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT publish_operation_id FROM document_publish_schedules \
             WHERE status = 'PENDING' AND scheduled_publish_at <= $1 \
               AND (next_retry_at IS NULL OR next_retry_at <= $1) \
             ORDER BY scheduled_publish_at, publish_operation_id LIMIT $2",
        )
        .bind(database_now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_statement_error)?;
        ids.into_iter()
            .map(|id| {
                PublishOperationId::try_from_uuid(id)
                    .map_err(|_| RepositoryError::IntegrityViolation)
            })
            .collect()
    }

    async fn is_due(&self, id: PublishOperationId) -> Result<bool, RepositoryError> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM document_publish_schedules WHERE publish_operation_id = $1 \
             AND status = 'PENDING' AND scheduled_publish_at <= now() \
             AND (next_retry_at IS NULL OR next_retry_at <= now()))",
        )
        .bind(id.as_uuid()).fetch_one(&self.pool).await.map_err(map_statement_error)
    }

    async fn record_retry(
        &self,
        id: PublishOperationId,
    ) -> Result<time::OffsetDateTime, RepositoryError> {
        record_retry(&self.pool, id).await
    }

    async fn terminalize(&self, record: DueTerminalRecord) -> Result<(), RepositoryError> {
        terminalize(&self.pool, record).await
    }
}

pub(crate) async fn get_schedule(
    pool: &PgPool,
    id: PublishOperationId,
) -> Result<Option<ScheduleOperationRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT document_id,target_document_version_id,expected_document_revision,accepted_document_revision, \
                scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status \
         FROM document_publish_schedules WHERE publish_operation_id = $1",
    )
    .bind(id.as_uuid()).fetch_optional(pool).await.map_err(map_statement_error)?;
    row.map(|row| map_schedule(id, row)).transpose()
}

async fn get_schedule_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: PublishOperationId,
) -> Result<Option<ScheduleOperationRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT document_id,target_document_version_id,expected_document_revision,accepted_document_revision, \
                scheduled_publish_at,actor_identity_provider,actor_principal_id,manifest_digest,status \
         FROM document_publish_schedules WHERE publish_operation_id = $1",
    )
    .bind(id.as_uuid()).fetch_optional(&mut **tx).await.map_err(map_statement_error)?;
    row.map(|row| map_schedule(id, row)).transpose()
}

fn map_schedule(
    id: PublishOperationId,
    row: sqlx::postgres::PgRow,
) -> Result<ScheduleOperationRecord, RepositoryError> {
    let document_id = DocumentId::from_uuid(row.get("document_id"));
    let target_id = DocumentVersionId::from_uuid(row.get("target_document_version_id"));
    let revision: i64 = row.get("expected_document_revision");
    let accepted: i64 = row.get("accepted_document_revision");
    let due = row.get("scheduled_publish_at");
    let actor = PrincipalRef::new(
        row.get::<String, _>("actor_identity_provider"),
        row.get::<String, _>("actor_principal_id"),
    )
    .map_err(|_| RepositoryError::IntegrityViolation)?;
    let command = SchedulePublishCommand::new(id, document_id, target_id, revision, actor, due)
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let digest: [u8; 32] = row
        .get::<Vec<u8>, _>("manifest_digest")
        .try_into()
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    Ok(ScheduleOperationRecord {
        command,
        result: SchedulePublishResult {
            publish_operation_id: id,
            document_id,
            target_version_id: target_id,
            accepted_revision: accepted,
            scheduled_publish_at: due,
        },
        manifest_digest: digest,
        status: row.get("status"),
    })
}

pub(crate) async fn reserve(
    pool: &PgPool,
    record: SchedulePublishRecord,
    ctx: Option<&VerifiedActorContext>,
) -> Result<SchedulePublishResult, RepositoryError> {
    let command = &record.command;
    let mut tx = pool.begin().await.map_err(map_statement_error)?;
    let outcome: Result<SchedulePublishResult, RepositoryError> = async {
        guard_document_mutation(&mut tx, ctx, command.document_id(), &[Action::Read, Action::Publish], command.actor()).await?;
        let state = sqlx::query("SELECT current_version_id, revision FROM documents WHERE document_id = $1 FOR UPDATE")
            .bind(command.document_id().as_uuid()).fetch_optional(&mut *tx).await.map_err(map_statement_error)?
            .ok_or(RepositoryError::DocumentNotFound)?;
        if let Some(stored) = get_schedule_in_tx(&mut tx, command.publish_operation_id()).await? {
            return if stored.command == *command { Ok(stored.result) } else { Err(RepositoryError::Conflict) };
        }
        publication_end::ensure_not_ended(&mut tx, command.document_id()).await?;
        let publish_id_claimed: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM document_publish_operations WHERE publish_operation_id = $1)",
        )
        .bind(command.publish_operation_id().as_uuid()).fetch_one(&mut *tx).await.map_err(map_statement_error)?;
        if publish_id_claimed { return Err(RepositoryError::Conflict); }
        let current: Option<Uuid> = state.get("current_version_id");
        let revision: i64 = state.get("revision");
        if revision != command.expected_revision()
            || current != record.expected_current_version_id.map(|id| id.as_uuid()) {
            return Err(RepositoryError::Conflict);
        }
        let database_now: time::OffsetDateTime = sqlx::query_scalar("SELECT now()")
            .fetch_one(&mut *tx).await.map_err(map_statement_error)?;
        if command.scheduled_publish_at() <= database_now { return Err(RepositoryError::BusinessRule); }
        let target = sqlx::query(
            "SELECT document_id,version_no,base_document_version_id,lifecycle_state,requires_content_classification,scheduled_publish_at \
             FROM document_versions WHERE document_version_id = $1 FOR UPDATE",
        )
        .bind(command.target_version_id().as_uuid()).fetch_optional(&mut *tx).await.map_err(map_statement_error)?
        .ok_or(RepositoryError::DocumentVersionNotFound)?;
        if target.get::<Uuid, _>("document_id") != command.document_id().as_uuid() { return Err(RepositoryError::IntegrityViolation); }
        if target.get::<String, _>("lifecycle_state") != "WORKING"
            || target.get::<bool, _>("requires_content_classification")
            || target.get::<Option<time::OffsetDateTime>, _>("scheduled_publish_at").is_some() {
            return Err(RepositoryError::BusinessRule);
        }
        let version_no: i64 = target.get("version_no");
        let base: Option<Uuid> = target.get("base_document_version_id");
        if (version_no == 1 && (current.is_some() || base.is_some()))
            || (version_no > 1 && (current.is_none() || base != current)) {
            return Err(RepositoryError::Conflict);
        }
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM document_publish_schedules WHERE document_id = $1 AND status = 'PENDING')",
        )
        .bind(command.document_id().as_uuid()).fetch_one(&mut *tx).await.map_err(map_statement_error)?;
        if pending { return Err(RepositoryError::BusinessRule); }
        let target_manifest = versioning_mutation::load_manifest(&mut tx, command.target_version_id().as_uuid()).await?;
        if target_manifest.identity_digest() != record.manifest_digest { return Err(RepositoryError::Conflict); }
        if let Some(current_id) = current {
            let base_state: Option<(String, bool)> = sqlx::query_as(
                "SELECT lifecycle_state, requires_content_classification FROM document_versions WHERE document_version_id = $1",
            )
            .bind(current_id).fetch_optional(&mut *tx).await.map_err(map_statement_error)?;
            if base_state != Some(("PUBLISHED".to_owned(), false)) { return Err(RepositoryError::BusinessRule); }
            let base_manifest = versioning_mutation::load_manifest(&mut tx, current_id).await?;
            versioning_mutation::ensure_semantic_change(&base_manifest, &target_manifest)?;
        }
        let accepted = revision.checked_add(1).ok_or(RepositoryError::IntegrityViolation)?;
        sqlx::query(
            "INSERT INTO document_publish_schedules \
             (publish_operation_id,document_id,target_document_version_id,base_document_version_id,current_version_id, \
              expected_document_revision,accepted_document_revision,scheduled_publish_at,actor_identity_provider, \
              actor_principal_id,manifest_digest,status,created_at) \
             VALUES ($1,$2,$3,$4,$4,$5,$6,$7,$8,$9,$10,'PENDING',$11)",
        )
        .bind(command.publish_operation_id().as_uuid()).bind(command.document_id().as_uuid())
        .bind(command.target_version_id().as_uuid()).bind(current).bind(revision).bind(accepted)
        .bind(command.scheduled_publish_at()).bind(command.actor().identity_provider())
        .bind(command.actor().principal_id()).bind(record.manifest_digest.to_vec()).bind(record.occurred_at)
        .execute(&mut *tx).await.map_err(map_schedule_error)?;
        sqlx::query(
            "UPDATE document_versions SET approved_at = COALESCE(approved_at,$1), scheduled_publish_at = $2 \
             WHERE document_version_id = $3",
        )
        .bind(record.occurred_at).bind(command.scheduled_publish_at()).bind(command.target_version_id().as_uuid())
        .execute(&mut *tx).await.map_err(map_statement_error)?;
        sqlx::query("UPDATE documents SET revision = $1 WHERE document_id = $2")
            .bind(accepted).bind(command.document_id().as_uuid())
            .execute(&mut *tx).await.map_err(map_statement_error)?;
        let payload = json!({
            "documentId": command.document_id().as_uuid().to_string(),
            "documentVersionId": command.target_version_id().as_uuid().to_string(),
            "publishOperationId": command.publish_operation_id().as_uuid().to_string(),
            "scheduledPublishAt": command.scheduled_publish_at(),
            "acceptedDocumentRevision": accepted,
        });
        insert_events(&mut tx, ScheduleEvent {
            domain_id: record.domain_event_id.as_uuid(), audit_id: record.audit_event_id.as_uuid(),
            domain_type: DOCUMENT_VERSION_PUBLICATION_SCHEDULED,
            audit_type: AUDIT_DOCUMENT_VERSION_PUBLICATION_SCHEDULED,
            audit_result: "success",
            document_id: command.document_id(), target_id: command.target_version_id(),
            actor: command.actor(), payload, at: record.occurred_at,
        }).await?;
        Ok(SchedulePublishResult { publish_operation_id: command.publish_operation_id(), document_id: command.document_id(),
            target_version_id: command.target_version_id(), accepted_revision: accepted,
            scheduled_publish_at: command.scheduled_publish_at() })
    }.await;
    match outcome {
        Ok(result) => {
            tx.commit().await.map_err(map_commit_error)?;
            Ok(result)
        }
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}

pub(crate) async fn get_cancel_operation(
    pool: &PgPool,
    id: VersionOperationId,
) -> Result<Option<CancelOperationRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT operation_kind,command_digest,document_id,target_document_version_id,result,resulting_document_revision \
         FROM document_version_operations WHERE operation_id = $1",
    )
    .bind(id.as_uuid()).fetch_optional(pool).await.map_err(map_statement_error)?;
    row.map(|row| map_cancel(id, row)).transpose()
}

async fn get_cancel_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: VersionOperationId,
) -> Result<Option<CancelOperationRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT operation_kind,command_digest,document_id,target_document_version_id,result,resulting_document_revision \
         FROM document_version_operations WHERE operation_id = $1",
    )
    .bind(id.as_uuid()).fetch_optional(&mut **tx).await.map_err(map_statement_error)?;
    row.map(|row| map_cancel(id, row)).transpose()
}

fn map_cancel(
    id: VersionOperationId,
    row: sqlx::postgres::PgRow,
) -> Result<CancelOperationRecord, RepositoryError> {
    if row.get::<String, _>("operation_kind") != "CANCEL_SCHEDULE" {
        return Err(RepositoryError::Conflict);
    }
    let digest = row
        .get::<Vec<u8>, _>("command_digest")
        .try_into()
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let result: Value = row.get("result");
    let publish_id = result
        .get("publish_operation_id")
        .and_then(Value::as_str)
        .and_then(|raw| Uuid::parse_str(raw).ok())
        .and_then(|raw| PublishOperationId::try_from_uuid(raw).ok())
        .ok_or(RepositoryError::IntegrityViolation)?;
    Ok(CancelOperationRecord {
        command_digest: digest,
        result: CancelScheduleResult {
            operation_id: id,
            publish_operation_id: publish_id,
            document_id: DocumentId::from_uuid(row.get("document_id")),
            target_version_id: DocumentVersionId::from_uuid(row.get("target_document_version_id")),
            resulting_revision: row.get("resulting_document_revision"),
        },
    })
}

pub(crate) async fn cancel(
    pool: &PgPool,
    record: CancelScheduleRecord,
    ctx: Option<&VerifiedActorContext>,
) -> Result<CancelScheduleResult, RepositoryError> {
    let command = &record.command;
    let mut tx = pool.begin().await.map_err(map_statement_error)?;
    let outcome: Result<CancelScheduleResult, RepositoryError> = async {
        guard_document_mutation(&mut tx, ctx, command.document_id(), &[Action::Read, Action::Publish], command.actor()).await?;
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1 FOR UPDATE")
            .bind(command.document_id().as_uuid()).fetch_optional(&mut *tx).await.map_err(map_statement_error)?
            .ok_or(RepositoryError::DocumentNotFound)?;
        if let Some(stored) = get_cancel_in_tx(&mut tx, command.operation_id()).await? {
            return if stored.command_digest == command.command_digest() { Ok(stored.result) } else { Err(RepositoryError::Conflict) };
        }
        if revision != command.expected_revision() { return Err(RepositoryError::Conflict); }
        let schedule = sqlx::query(
            "SELECT document_id,target_document_version_id,status FROM document_publish_schedules \
             WHERE publish_operation_id = $1 FOR UPDATE",
        )
        .bind(command.publish_operation_id().as_uuid()).fetch_optional(&mut *tx).await.map_err(map_statement_error)?
        .ok_or(RepositoryError::BusinessRule)?;
        if schedule.get::<Uuid, _>("document_id") != command.document_id().as_uuid()
            || schedule.get::<Uuid, _>("target_document_version_id") != command.target_version_id().as_uuid() {
            return Err(RepositoryError::Conflict);
        }
        if schedule.get::<String, _>("status") != "PENDING" { return Err(RepositoryError::BusinessRule); }
        let next = revision.checked_add(1).ok_or(RepositoryError::IntegrityViolation)?;
        sqlx::query("UPDATE document_publish_schedules SET status = 'CANCELLED', cancelled_at = $1 WHERE publish_operation_id = $2")
            .bind(record.occurred_at).bind(command.publish_operation_id().as_uuid())
            .execute(&mut *tx).await.map_err(map_statement_error)?;
        sqlx::query("UPDATE document_versions SET scheduled_publish_at = NULL WHERE document_version_id = $1")
            .bind(command.target_version_id().as_uuid()).execute(&mut *tx).await.map_err(map_statement_error)?;
        sqlx::query("UPDATE documents SET revision = $1 WHERE document_id = $2")
            .bind(next).bind(command.document_id().as_uuid()).execute(&mut *tx).await.map_err(map_statement_error)?;
        sqlx::query(
            "INSERT INTO document_version_operations \
             (operation_id,operation_kind,document_id,target_document_version_id,expected_document_revision, \
              actor_identity_provider,actor_principal_id,command_digest,result,resulting_document_revision,created_at) \
             VALUES ($1,'CANCEL_SCHEDULE',$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(command.operation_id().as_uuid()).bind(command.document_id().as_uuid())
        .bind(command.target_version_id().as_uuid()).bind(command.expected_revision())
        .bind(command.actor().identity_provider()).bind(command.actor().principal_id())
        .bind(command.command_digest().to_vec())
        .bind(json!({"publish_operation_id": command.publish_operation_id().as_uuid().to_string()}))
        .bind(next).bind(record.occurred_at).execute(&mut *tx).await.map_err(map_schedule_error)?;
        let payload = json!({
            "documentId": command.document_id().as_uuid().to_string(),
            "documentVersionId": command.target_version_id().as_uuid().to_string(),
            "publishOperationId": command.publish_operation_id().as_uuid().to_string(),
            "resultingDocumentRevision": next,
        });
        insert_events(&mut tx, ScheduleEvent {
            domain_id: record.domain_event_id.as_uuid(), audit_id: record.audit_event_id.as_uuid(),
            domain_type: DOCUMENT_VERSION_PUBLICATION_CANCELLED,
            audit_type: AUDIT_DOCUMENT_VERSION_PUBLICATION_CANCELLED,
            audit_result: "success",
            document_id: command.document_id(), target_id: command.target_version_id(),
            actor: command.actor(), payload, at: record.occurred_at,
        }).await?;
        Ok(CancelScheduleResult { operation_id: command.operation_id(), publish_operation_id: command.publish_operation_id(),
            document_id: command.document_id(), target_version_id: command.target_version_id(), resulting_revision: next })
    }.await;
    match outcome {
        Ok(result) => {
            tx.commit().await.map_err(map_commit_error)?;
            Ok(result)
        }
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}

struct ScheduleEvent<'a> {
    domain_id: Uuid,
    audit_id: Uuid,
    domain_type: &'a str,
    audit_type: &'a str,
    audit_result: &'a str,
    document_id: DocumentId,
    target_id: DocumentVersionId,
    actor: &'a PrincipalRef,
    payload: Value,
    at: time::OffsetDateTime,
}

async fn insert_events(
    tx: &mut Transaction<'_, Postgres>,
    event: ScheduleEvent<'_>,
) -> Result<(), RepositoryError> {
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at,attempt_count,delivered_at) \
         VALUES ($1,$2,'Document',$3,$4,$5,$5,0,NULL)",
    )
    .bind(event.domain_id).bind(event.domain_type).bind(event.document_id.as_uuid()).bind(event.payload.clone()).bind(event.at)
    .execute(&mut **tx).await.map_err(map_statement_error)?;
    let subject = format!(
        "document/{}/version/{}",
        event.document_id.as_uuid(),
        event.target_id.as_uuid()
    );
    sqlx::query(
        "INSERT INTO audit_outbox_events \
         (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id,resource_id, \
          resource_version_id,result,trace_id,data,occurred_at,attempt_count,delivered_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,NULL,$10,$11,0,NULL)",
    )
    .bind(event.audit_id).bind(event.audit_type).bind(AUDIT_SOURCE).bind(subject)
    .bind(event.actor.identity_provider()).bind(event.actor.principal_id()).bind(event.document_id.as_uuid())
    .bind(event.target_id.as_uuid()).bind(event.audit_result).bind(event.payload).bind(event.at)
    .execute(&mut **tx).await.map_err(map_statement_error)?;
    Ok(())
}

fn map_schedule_error(error: sqlx::Error) -> RepositoryError {
    if let sqlx::Error::Database(database) = &error
        && matches!(
            database.code().as_deref(),
            Some("23505" | "23503" | "23514")
        )
    {
        return RepositoryError::Conflict;
    }
    map_statement_error(error)
}

async fn lock_schedule_document(
    tx: &mut Transaction<'_, Postgres>,
    id: PublishOperationId,
) -> Result<(Uuid, i64), RepositoryError> {
    let document_id: Uuid = sqlx::query_scalar(
        "SELECT document_id FROM document_publish_schedules WHERE publish_operation_id = $1",
    )
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?
    .ok_or(RepositoryError::DocumentVersionNotFound)?;
    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1 FOR UPDATE")
            .bind(document_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(map_statement_error)?;
    Ok((document_id, revision))
}

async fn record_retry(
    pool: &PgPool,
    id: PublishOperationId,
) -> Result<time::OffsetDateTime, RepositoryError> {
    let mut tx = pool.begin().await.map_err(map_statement_error)?;
    let result: Result<time::OffsetDateTime, RepositoryError> = async {
        let _ = lock_schedule_document(&mut tx, id).await?;
        let row = sqlx::query("SELECT status,attempt_count,scheduled_publish_at,next_retry_at FROM document_publish_schedules WHERE publish_operation_id = $1 FOR UPDATE")
            .bind(id.as_uuid()).fetch_one(&mut *tx).await.map_err(map_statement_error)?;
        if row.get::<String, _>("status") != "PENDING" { return Err(RepositoryError::BusinessRule); }
        let now: time::OffsetDateTime = sqlx::query_scalar("SELECT now()")
            .fetch_one(&mut *tx).await.map_err(map_statement_error)?;
        if row.get::<time::OffsetDateTime, _>("scheduled_publish_at") > now { return Err(RepositoryError::BusinessRule); }
        if let Some(next) = row.get::<Option<time::OffsetDateTime>, _>("next_retry_at")
            && next > now
        {
            return Ok(next);
        }
        let count: i32 = row.get("attempt_count");
        let seconds = 2_i64.pow(u32::try_from(count.clamp(0, 11)).unwrap()).min(3600);
        let next = now + time::Duration::seconds(seconds);
        sqlx::query("UPDATE document_publish_schedules SET attempt_count = attempt_count + 1, last_attempt_at = $1, next_retry_at = $2 WHERE publish_operation_id = $3")
            .bind(now).bind(next).bind(id.as_uuid()).execute(&mut *tx).await.map_err(map_statement_error)?;
        Ok(next)
    }.await;
    match result {
        Ok(next) => {
            tx.commit().await.map_err(map_commit_error)?;
            Ok(next)
        }
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}

async fn terminalize(pool: &PgPool, record: DueTerminalRecord) -> Result<(), RepositoryError> {
    let mut tx = pool.begin().await.map_err(map_statement_error)?;
    let result: Result<(), RepositoryError> = async {
        let (document_id, revision) = lock_schedule_document(&mut tx, record.publish_operation_id).await?;
        let row = sqlx::query("SELECT target_document_version_id,status,scheduled_publish_at,actor_identity_provider,actor_principal_id FROM document_publish_schedules WHERE publish_operation_id = $1 FOR UPDATE")
            .bind(record.publish_operation_id.as_uuid()).fetch_one(&mut *tx).await.map_err(map_statement_error)?;
        if row.get::<String, _>("status") != "PENDING" { return Err(RepositoryError::BusinessRule); }
        let now: time::OffsetDateTime = sqlx::query_scalar("SELECT now()")
            .fetch_one(&mut *tx).await.map_err(map_statement_error)?;
        if row.get::<time::OffsetDateTime, _>("scheduled_publish_at") > now { return Err(RepositoryError::BusinessRule); }
        let target_id: Uuid = row.get("target_document_version_id");
        let next = revision.checked_add(1).ok_or(RepositoryError::IntegrityViolation)?;
        sqlx::query("UPDATE document_publish_schedules SET status = 'TERMINAL', terminal_reason = $1, last_attempt_at = $2, next_retry_at = NULL, attempt_count = attempt_count + 1, terminal_at = $2, terminal_executor_identity_provider = $3, terminal_executor_principal_id = $4 WHERE publish_operation_id = $5")
            .bind(&record.reason).bind(now)
            .bind(record.service_executor.as_ref().map(|actor| actor.identity_provider()))
            .bind(record.service_executor.as_ref().map(|actor| actor.principal_id()))
            .bind(record.publish_operation_id.as_uuid())
            .execute(&mut *tx).await.map_err(map_statement_error)?;
        sqlx::query("UPDATE document_versions SET scheduled_publish_at = NULL WHERE document_version_id = $1")
            .bind(target_id).execute(&mut *tx).await.map_err(map_statement_error)?;
        sqlx::query("UPDATE documents SET revision = $1 WHERE document_id = $2")
            .bind(next).bind(document_id).execute(&mut *tx).await.map_err(map_statement_error)?;
        let actor = PrincipalRef::new(row.get::<String, _>("actor_identity_provider"), row.get::<String, _>("actor_principal_id"))
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        let mut payload = json!({
            "documentId": document_id.to_string(), "documentVersionId": target_id.to_string(),
            "publishOperationId": record.publish_operation_id.as_uuid().to_string(),
            "terminalReason": record.reason, "resultingDocumentRevision": next,
        });
        if let Some(executor) = record.service_executor.as_ref() {
            payload["serviceExecutor"] = json!({
                "identityProvider": executor.identity_provider(),
                "principalId": executor.principal_id(),
            });
        }
        insert_events(&mut tx, ScheduleEvent {
            domain_id: record.domain_event_id.as_uuid(), audit_id: record.audit_event_id.as_uuid(),
            domain_type: "DocumentVersionPublicationTerminal",
            audit_type: "document.version.publication.terminal",
            audit_result: "failure",
            document_id: DocumentId::from_uuid(document_id),
            target_id: DocumentVersionId::from_uuid(target_id),
            actor: &actor, payload, at: record.occurred_at,
        }).await?;
        Ok(())
    }.await;
    match result {
        Ok(()) => {
            tx.commit().await.map_err(map_commit_error)?;
            Ok(())
        }
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}
