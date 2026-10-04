use document_application::{
    AUDIT_DOCUMENT_VERSION_WITHDRAWN, DOCUMENT_VERSION_WITHDRAWN, RepositoryError,
    VerifiedActorContext, VersionOperationId, WithdrawOperationRecord, WithdrawVersionRecord,
    WithdrawVersionResult,
};
use document_domain::{Action, DocumentId, DocumentVersionId};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    access_control::guard_document_mutation,
    document_revision::{WithdrawFallbackRevisionInput, issue_withdraw_fallback_revision},
    error::{map_commit_error, map_statement_error},
    versioning_mutation,
};

const AUDIT_SOURCE: &str = "urn:knowledge-platform:document-platform";

pub(crate) async fn get_operation(
    pool: &PgPool,
    id: VersionOperationId,
) -> Result<Option<WithdrawOperationRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT operation_kind, command_digest, document_id, target_document_version_id, \
                result, resulting_document_revision FROM document_version_operations WHERE operation_id = $1",
    )
    .bind(id.as_uuid()).fetch_optional(pool).await.map_err(map_statement_error)?;
    row.map(|row| map_row(id, row)).transpose()
}

async fn get_operation_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: VersionOperationId,
) -> Result<Option<WithdrawOperationRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT operation_kind, command_digest, document_id, target_document_version_id, \
                result, resulting_document_revision FROM document_version_operations WHERE operation_id = $1",
    )
    .bind(id.as_uuid()).fetch_optional(&mut **tx).await.map_err(map_statement_error)?;
    row.map(|row| map_row(id, row)).transpose()
}

fn map_row(
    id: VersionOperationId,
    row: sqlx::postgres::PgRow,
) -> Result<WithdrawOperationRecord, RepositoryError> {
    let kind: String = row.get("operation_kind");
    if kind != "WITHDRAW" {
        return Err(RepositoryError::Conflict);
    }
    let digest: [u8; 32] = row
        .get::<Vec<u8>, _>("command_digest")
        .try_into()
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let result: Value = row.get("result");
    let former = optional_id(&result, "former_current_version_id")?;
    let current = optional_id(&result, "resulting_current_version_id")?;
    let withheld = result
        .get("restoration_withheld_reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(WithdrawOperationRecord {
        command_digest: digest,
        result: WithdrawVersionResult {
            operation_id: id,
            document_id: DocumentId::from_uuid(row.get("document_id")),
            target_version_id: DocumentVersionId::from_uuid(row.get("target_document_version_id")),
            former_current_version_id: former,
            resulting_current_version_id: current,
            resulting_revision: row.get("resulting_document_revision"),
            restoration_withheld_reason: withheld,
        },
    })
}

fn optional_id(result: &Value, key: &str) -> Result<Option<DocumentVersionId>, RepositoryError> {
    match result.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => Uuid::parse_str(raw)
            .map(DocumentVersionId::from_uuid)
            .map(Some)
            .map_err(|_| RepositoryError::IntegrityViolation),
        _ => Err(RepositoryError::IntegrityViolation),
    }
}

pub(crate) async fn withdraw(
    pool: &PgPool,
    record: WithdrawVersionRecord,
    ctx: Option<&VerifiedActorContext>,
) -> Result<WithdrawVersionResult, RepositoryError> {
    let command = &record.command;
    let mut tx = pool.begin().await.map_err(map_statement_error)?;
    let outcome: Result<WithdrawVersionResult, RepositoryError> = async {
        guard_document_mutation(&mut tx, ctx, command.document_id(), &[Action::Read, Action::Publish], command.actor()).await?;
        let state = sqlx::query("SELECT current_version_id, revision, metadata FROM documents WHERE document_id = $1 FOR UPDATE")
            .bind(command.document_id().as_uuid()).fetch_optional(&mut *tx).await.map_err(map_statement_error)?
            .ok_or(RepositoryError::DocumentNotFound)?;
        if let Some(stored) = get_operation_in_tx(&mut tx, command.operation_id()).await? {
            return if stored.command_digest == command.command_digest() { Ok(stored.result) } else { Err(RepositoryError::Conflict) };
        }
        let current: Option<Uuid> = state.get("current_version_id");
        let revision: i64 = state.get("revision");
        let metadata: Value = state.get("metadata");
        if revision != command.expected_revision() { return Err(RepositoryError::Conflict); }
        let target = sqlx::query(
            "SELECT document_id, base_document_version_id, lifecycle_state \
             FROM document_versions WHERE document_version_id = $1 FOR UPDATE",
        )
        .bind(command.target_version_id().as_uuid()).fetch_optional(&mut *tx).await.map_err(map_statement_error)?
        .ok_or(RepositoryError::DocumentVersionNotFound)?;
        if target.get::<Uuid, _>("document_id") != command.document_id().as_uuid() {
            return Err(RepositoryError::IntegrityViolation);
        }
        if target.get::<String, _>("lifecycle_state") != "PUBLISHED" { return Err(RepositoryError::BusinessRule); }
        let base_id: Option<Uuid> = target.get("base_document_version_id");
        let is_current = current == Some(command.target_version_id().as_uuid());
        let mut resulting_current = current;
        let mut withheld_reason = None;
        if is_current {
            resulting_current = None;
            if let Some(base_id) = base_id {
                if record.eligible_base_id.map(|id| id.as_uuid()) == Some(base_id) {
                    let candidate = sqlx::query(
                        "SELECT document_id, lifecycle_state, requires_content_classification \
                         FROM document_versions WHERE document_version_id = $1 FOR UPDATE",
                    )
                    .bind(base_id).fetch_optional(&mut *tx).await.map_err(map_statement_error)?;
                    let candidate_state_safe = candidate.as_ref().is_some_and(|row| {
                        row.get::<Uuid, _>("document_id") == command.document_id().as_uuid()
                            && row.get::<String, _>("lifecycle_state") == "PUBLISHED"
                            && !row.get::<bool, _>("requires_content_classification")
                    });
                    if candidate_state_safe && record.eligible_base_manifest_digest.is_some() {
                        let manifest_safe = versioning_mutation::load_manifest(&mut tx, base_id).await
                            .map(|manifest| Some(manifest.identity_digest()) == record.eligible_base_manifest_digest)
                            .unwrap_or(false);
                        let quality_safe = base_quality_safe(&mut tx, base_id).await.unwrap_or(false);
                        if manifest_safe && quality_safe { resulting_current = Some(base_id); }
                    }
                    if resulting_current.is_none() { withheld_reason = Some("base_database_evidence_changed".to_owned()); }
                } else {
                    withheld_reason = record.restoration_withheld_reason.clone().or(Some("base_not_qualified".to_owned()));
                }
            }
        }
        let next_revision = revision.checked_add(1).ok_or(RepositoryError::IntegrityViolation)?;
        sqlx::query("UPDATE document_versions SET lifecycle_state = 'WITHDRAWN', withdrawn_at = $1 WHERE document_version_id = $2")
            .bind(record.withdrawn_at).bind(command.target_version_id().as_uuid())
            .execute(&mut *tx).await.map_err(map_statement_error)?;
        sqlx::query("UPDATE documents SET current_version_id = $1, revision = $2 WHERE document_id = $3")
            .bind(resulting_current).bind(next_revision).bind(command.document_id().as_uuid())
            .execute(&mut *tx).await.map_err(map_statement_error)?;
        if is_current && let Some(fallback_id) = resulting_current {
            issue_withdraw_fallback_revision(
                &mut tx,
                WithdrawFallbackRevisionInput {
                    document_id: command.document_id(),
                    document_version_id: DocumentVersionId::from_uuid(fallback_id),
                    metadata: &metadata,
                    operation_id: command.operation_id().as_uuid(),
                    actor: command.actor(),
                    reason: command.reason(),
                    created_at: record.withdrawn_at,
                },
            )
            .await?;
        }
        let invalidated: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE document_publish_schedules SET status = 'TERMINAL', terminal_reason = 'withdrawal invalidated intent', terminal_at = $3, terminal_executor_identity_provider = $4, terminal_executor_principal_id = $5 \
             WHERE document_id = $1 AND status = 'PENDING' \
               AND (target_document_version_id = $2 OR current_version_id = $2 OR base_document_version_id = $2) \
             RETURNING target_document_version_id",
        )
        .bind(command.document_id().as_uuid()).bind(command.target_version_id().as_uuid())
        .bind(record.withdrawn_at)
        .bind(command.actor().identity_provider())
        .bind(command.actor().principal_id())
        .fetch_all(&mut *tx).await.map_err(map_statement_error)?;
        for version_id in &invalidated {
            sqlx::query("UPDATE document_versions SET scheduled_publish_at = NULL WHERE document_version_id = $1")
                .bind(version_id).execute(&mut *tx).await.map_err(map_statement_error)?;
        }
        let result = WithdrawVersionResult {
            operation_id: command.operation_id(), document_id: command.document_id(),
            target_version_id: command.target_version_id(),
            former_current_version_id: current.map(DocumentVersionId::from_uuid),
            resulting_current_version_id: resulting_current.map(DocumentVersionId::from_uuid),
            resulting_revision: next_revision, restoration_withheld_reason: withheld_reason,
        };
        let stored_result = json!({
            "former_current_version_id": result.former_current_version_id.map(|id| id.as_uuid().to_string()),
            "resulting_current_version_id": result.resulting_current_version_id.map(|id| id.as_uuid().to_string()),
            "restoration_withheld_reason": result.restoration_withheld_reason,
            "reason": command.reason(),
        });
        sqlx::query(
            "INSERT INTO document_version_operations \
             (operation_id,operation_kind,document_id,target_document_version_id,expected_document_revision, \
              actor_identity_provider,actor_principal_id,command_digest,result,resulting_document_revision,created_at) \
             VALUES ($1,'WITHDRAW',$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(command.operation_id().as_uuid()).bind(command.document_id().as_uuid())
        .bind(command.target_version_id().as_uuid()).bind(command.expected_revision())
        .bind(command.actor().identity_provider()).bind(command.actor().principal_id())
        .bind(command.command_digest().to_vec()).bind(stored_result.clone())
        .bind(next_revision).bind(record.withdrawn_at)
        .execute(&mut *tx).await.map_err(map_statement_error)?;
        let payload = json!({
            "documentId": command.document_id().as_uuid().to_string(),
            "withdrawnDocumentVersionId": command.target_version_id().as_uuid().to_string(),
            "formerCurrentVersionId": result.former_current_version_id.map(|id| id.as_uuid().to_string()),
            "resultingCurrentVersionId": result.resulting_current_version_id.map(|id| id.as_uuid().to_string()),
            "actor": {"identityProvider": command.actor().identity_provider(), "principalId": command.actor().principal_id()},
            "reason": command.reason(),
            "restorationWithheldByValidation": result.restoration_withheld_reason.is_some(),
            "restorationWithheldReason": result.restoration_withheld_reason,
            "invalidatedScheduleCount": invalidated.len(),
        });
        sqlx::query(
            "INSERT INTO outbox_events \
             (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at,attempt_count,delivered_at) \
             VALUES ($1,$2,'Document',$3,$4,$5,$5,0,NULL)",
        )
        .bind(record.domain_event_id.as_uuid()).bind(DOCUMENT_VERSION_WITHDRAWN)
        .bind(command.document_id().as_uuid()).bind(payload.clone()).bind(record.withdrawn_at)
        .execute(&mut *tx).await.map_err(map_statement_error)?;
        let subject = format!("document/{}/version/{}", command.document_id().as_uuid(), command.target_version_id().as_uuid());
        sqlx::query(
            "INSERT INTO audit_outbox_events \
             (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id,resource_id, \
              resource_version_id,result,trace_id,data,occurred_at,attempt_count,delivered_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'success',NULL,$9,$10,0,NULL)",
        )
        .bind(record.audit_event_id.as_uuid()).bind(AUDIT_DOCUMENT_VERSION_WITHDRAWN).bind(AUDIT_SOURCE)
        .bind(subject).bind(command.actor().identity_provider()).bind(command.actor().principal_id())
        .bind(command.document_id().as_uuid()).bind(command.target_version_id().as_uuid())
        .bind(payload).bind(record.withdrawn_at).execute(&mut *tx).await.map_err(map_statement_error)?;
        Ok(result)
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

async fn base_quality_safe(
    tx: &mut Transaction<'_, Postgres>,
    version_id: Uuid,
) -> Result<bool, RepositoryError> {
    let safe: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS ( \
             SELECT 1 FROM content_items ci \
             JOIN content_representations cr ON cr.content_representation_id = ci.authoritative_representation_id \
             JOIN document_semantic_inspections s ON s.file_id = cr.file_id AND s.inspection_profile_version = 'dsi-v0' \
             WHERE ci.document_version_id = $1 AND ( \
               jsonb_typeof(s.editorial_provenance->'comments') IS DISTINCT FROM 'array' \
               OR jsonb_typeof(s.editorial_provenance->'tracked_changes') IS DISTINCT FROM 'array' \
               OR jsonb_typeof(s.digital_signature_evidence) IS DISTINCT FROM 'array' \
               OR \
               jsonb_array_length(s.editorial_provenance->'comments') > 0 \
               OR EXISTS (SELECT 1 FROM jsonb_array_elements(s.editorial_provenance->'tracked_changes') change \
                          WHERE change->>'unresolved' = 'true') \
               OR EXISTS (SELECT 1 FROM jsonb_array_elements(s.digital_signature_evidence) signature \
                          WHERE signature->>'cryptographic_validity' IN ('invalid','unverifiable')) \
             ) \
         )",
    )
    .bind(version_id).fetch_one(&mut **tx).await.map_err(map_statement_error)?;
    Ok(safe)
}
