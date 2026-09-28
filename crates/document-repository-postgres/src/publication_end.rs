use document_application::{
    AUDIT_DOCUMENT_PUBLICATION_ENDED, DOCUMENT_PUBLICATION_ENDED, EndDocumentPublicationResult,
    EndPublicationCandidate, EndPublicationOperationRecord, EndPublicationRecord,
    PublicationEndOperationId, PublicationEndRepository, RepositoryError, VerifiedActorContext,
};
use document_domain::{
    Action, Document, DocumentId, DocumentVersion, DocumentVersionId, FolderId, LifecycleState,
    Metadata, PrincipalRef, RestoreDocument, RestoreDocumentVersion, Title, VersionNo,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository,
    access_control::{authorize_document_snapshot, guard_document_mutation},
    error::{map_commit_error, map_statement_error},
};

const AUDIT_SOURCE: &str = "urn:knowledge-platform:document-platform";

impl PublicationEndRepository for PostgresDocumentRepository {
    async fn get_end_operation(
        &self,
        id: PublicationEndOperationId,
    ) -> Result<Option<EndPublicationOperationRecord>, RepositoryError> {
        let operation = get_operation(&self.pool, id).await?;
        if let (Some(ctx), Some(operation)) = (&self.verified_actor, &operation) {
            authorize_document_snapshot(
                &self.pool,
                ctx,
                operation.result().document_id(),
                &[Action::Read, Action::Publish],
            )
            .await?;
        }
        Ok(operation)
    }

    async fn get_end_candidate(
        &self,
        id: DocumentId,
    ) -> Result<Option<EndPublicationCandidate>, RepositoryError> {
        if let Some(ctx) = &self.verified_actor {
            authorize_document_snapshot(&self.pool, ctx, id, &[Action::Read, Action::Publish])
                .await?;
        }
        let row = sqlx::query(
            "SELECT d.document_id, d.folder_id, d.current_version_id, d.revision, \
                    d.metadata AS document_metadata, d.created_at AS document_created_at, \
                    v.document_version_id, v.version_no, v.base_document_version_id, \
                    v.lifecycle_state, v.title, v.revision_reason, v.approved_at, \
                    v.scheduled_publish_at, v.published_at, v.withdrawn_at, \
                    v.effective_from, v.effective_to, v.created_by_identity_provider, \
                    v.created_by_principal_id, v.metadata AS version_metadata, \
                    v.created_at AS version_created_at \
             FROM documents d LEFT JOIN document_versions v \
               ON v.document_version_id = d.current_version_id AND v.document_id = d.document_id \
             WHERE d.document_id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_statement_error)?;
        row.map(candidate_from_row).transpose()
    }

    async fn end_document_publication(
        &self,
        record: EndPublicationRecord,
    ) -> Result<EndDocumentPublicationResult, RepositoryError> {
        end_publication(&self.pool, record, self.verified_actor.as_ref()).await
    }
}

async fn get_operation(
    pool: &PgPool,
    id: PublicationEndOperationId,
) -> Result<Option<EndPublicationOperationRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT document_id, command_digest, former_current_version_id, \
                resulting_document_revision, ended_at \
         FROM document_publication_end_operations WHERE operation_id = $1",
    )
    .bind(id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(map_statement_error)?;
    row.map(|row| operation_from_row(id, row)).transpose()
}

async fn get_operation_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: PublicationEndOperationId,
) -> Result<Option<EndPublicationOperationRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT document_id, command_digest, former_current_version_id, \
                resulting_document_revision, ended_at \
         FROM document_publication_end_operations WHERE operation_id = $1",
    )
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    row.map(|row| operation_from_row(id, row)).transpose()
}

fn operation_from_row(
    id: PublicationEndOperationId,
    row: PgRow,
) -> Result<EndPublicationOperationRecord, RepositoryError> {
    let digest = row
        .get::<Vec<u8>, _>("command_digest")
        .try_into()
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let result = EndDocumentPublicationResult::from_persisted(
        id,
        DocumentId::from_uuid(row.get("document_id")),
        DocumentVersionId::from_uuid(row.get("former_current_version_id")),
        row.get("resulting_document_revision"),
        row.get("ended_at"),
    );
    Ok(EndPublicationOperationRecord::new(digest, result))
}

fn metadata(value: Value) -> Result<Metadata, RepositoryError> {
    match value {
        Value::Object(map) => Ok(Metadata::from_map(map)),
        _ => Err(RepositoryError::IntegrityViolation),
    }
}

fn candidate_from_row(row: PgRow) -> Result<EndPublicationCandidate, RepositoryError> {
    let document_id = DocumentId::from_uuid(row.get("document_id"));
    let current_id: Option<Uuid> = row.get("current_version_id");
    let document = Document::restore(RestoreDocument {
        document_id,
        folder_id: FolderId::from_uuid(row.get("folder_id")),
        current_version_id: current_id.map(DocumentVersionId::from_uuid),
        revision: row.get("revision"),
        metadata: metadata(row.get("document_metadata"))?,
        created_at: row.get("document_created_at"),
    })
    .map_err(|_| RepositoryError::IntegrityViolation)?;
    let version_id: Option<Uuid> = row.get("document_version_id");
    if current_id.is_some() && version_id.is_none() {
        return Err(RepositoryError::IntegrityViolation);
    }
    let current = version_id
        .map(|version_id| {
            let state: String = row.get("lifecycle_state");
            let lifecycle_state = match state.as_str() {
                "WORKING" => LifecycleState::Working,
                "PUBLISHED" => LifecycleState::Published,
                "WITHDRAWN" => LifecycleState::Withdrawn,
                _ => return Err(RepositoryError::IntegrityViolation),
            };
            DocumentVersion::restore(RestoreDocumentVersion {
                document_version_id: DocumentVersionId::from_uuid(version_id),
                document_id,
                version_no: VersionNo::new(row.get("version_no"))
                    .map_err(|_| RepositoryError::IntegrityViolation)?,
                base_document_version_id: row
                    .get::<Option<Uuid>, _>("base_document_version_id")
                    .map(DocumentVersionId::from_uuid),
                lifecycle_state,
                title: Title::new(row.get::<String, _>("title"))
                    .map_err(|_| RepositoryError::IntegrityViolation)?,
                revision_reason: row.get("revision_reason"),
                approved_at: row.get("approved_at"),
                scheduled_publish_at: row.get("scheduled_publish_at"),
                published_at: row.get("published_at"),
                withdrawn_at: row.get("withdrawn_at"),
                effective_from: row.get("effective_from"),
                effective_to: row.get("effective_to"),
                created_by: PrincipalRef::new(
                    row.get::<String, _>("created_by_identity_provider"),
                    row.get::<String, _>("created_by_principal_id"),
                )
                .map_err(|_| RepositoryError::IntegrityViolation)?,
                metadata: metadata(row.get("version_metadata"))?,
                created_at: row.get("version_created_at"),
            })
            .map_err(|_| RepositoryError::IntegrityViolation)
        })
        .transpose()?;
    Ok(EndPublicationCandidate::new(document, current))
}

pub(crate) async fn ensure_not_ended(
    tx: &mut Transaction<'_, Postgres>,
    document_id: DocumentId,
) -> Result<(), RepositoryError> {
    let ended: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM document_publication_end_operations WHERE document_id = $1)",
    )
    .bind(document_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    if ended {
        Err(RepositoryError::BusinessRule)
    } else {
        Ok(())
    }
}

async fn end_publication(
    pool: &PgPool,
    record: EndPublicationRecord,
    ctx: Option<&VerifiedActorContext>,
) -> Result<EndDocumentPublicationResult, RepositoryError> {
    let command = record.command();
    let mut tx = pool.begin().await.map_err(map_statement_error)?;
    let outcome: Result<EndDocumentPublicationResult, RepositoryError> = async {
        guard_document_mutation(&mut tx, ctx, command.document_id(), &[Action::Read, Action::Publish], command.actor()).await?;
        // This lock serializes T10 with Publish, schedule and Version mutations.
        let state = sqlx::query(
            "SELECT current_version_id, revision FROM documents WHERE document_id = $1 FOR UPDATE",
        )
        .bind(command.document_id().as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_statement_error)?
        .ok_or(RepositoryError::DocumentNotFound)?;

        if let Some(stored) = get_operation_in_tx(&mut tx, command.operation_id()).await? {
            return if stored.command_digest() == command.command_digest() {
                Ok(stored.result().clone())
            } else {
                Err(RepositoryError::Conflict)
            };
        }
        let already_ended: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM document_publication_end_operations WHERE document_id = $1)",
        )
        .bind(command.document_id().as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        if already_ended {
            return Err(RepositoryError::BusinessRule);
        }
        let revision: i64 = state.get("revision");
        if revision != command.expected_revision() {
            return Err(RepositoryError::Conflict);
        }
        let Some(current_id) = state.get::<Option<Uuid>, _>("current_version_id") else {
            return Err(RepositoryError::BusinessRule);
        };
        if current_id != command.expected_current_version_id().as_uuid() {
            return Err(RepositoryError::Conflict);
        }
        let version = sqlx::query(
            "SELECT document_id, lifecycle_state FROM document_versions \
             WHERE document_version_id = $1 FOR UPDATE",
        )
        .bind(current_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_statement_error)?
        .ok_or(RepositoryError::IntegrityViolation)?;
        if version.get::<Uuid, _>("document_id") != command.document_id().as_uuid() {
            return Err(RepositoryError::IntegrityViolation);
        }
        if version.get::<String, _>("lifecycle_state") != "PUBLISHED" {
            return Err(RepositoryError::BusinessRule);
        }
        let next_revision = revision
            .checked_add(1)
            .ok_or(RepositoryError::IntegrityViolation)?;
        sqlx::query("UPDATE documents SET current_version_id = NULL, revision = $1 WHERE document_id = $2")
            .bind(next_revision)
            .bind(command.document_id().as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;
        let invalidated: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE document_publish_schedules \
             SET status = 'TERMINAL', terminal_reason = 'document_publication_ended', next_retry_at = NULL, terminal_at = $2, terminal_executor_identity_provider = $3, terminal_executor_principal_id = $4 \
             WHERE document_id = $1 AND status = 'PENDING' \
             RETURNING target_document_version_id",
        )
        .bind(command.document_id().as_uuid())
        .bind(record.ended_at())
        .bind(command.actor().identity_provider())
        .bind(command.actor().principal_id())
        .fetch_all(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        for version_id in &invalidated {
            sqlx::query(
                "UPDATE document_versions SET scheduled_publish_at = NULL WHERE document_version_id = $1",
            )
            .bind(version_id)
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;
        }

        let result = EndDocumentPublicationResult::from_persisted(
            command.operation_id(),
            command.document_id(),
            DocumentVersionId::from_uuid(current_id),
            next_revision,
            record.ended_at(),
        );
        let inserted = sqlx::query(
            "INSERT INTO document_publication_end_operations \
             (operation_id,document_id,command_digest,expected_document_revision, \
              expected_current_version_id,actor_identity_provider,actor_principal_id,reason, \
              former_current_version_id,resulting_document_revision,ended_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT DO NOTHING",
        )
        .bind(command.operation_id().as_uuid())
        .bind(command.document_id().as_uuid())
        .bind(command.command_digest().to_vec())
        .bind(command.expected_revision())
        .bind(current_id)
        .bind(command.actor().identity_provider())
        .bind(command.actor().principal_id())
        .bind(command.reason())
        .bind(current_id)
        .bind(next_revision)
        .bind(record.ended_at())
        .execute(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        if inserted.rows_affected() != 1 {
            return Err(RepositoryError::Conflict);
        }
        let payload = json!({
            "operationId": command.operation_id().as_uuid().to_string(),
            "documentId": command.document_id().as_uuid().to_string(),
            "formerCurrentVersionId": current_id.to_string(),
            "resultingCurrentVersionId": Value::Null,
            "resultingDocumentRevision": next_revision,
            "actor": {
                "identityProvider": command.actor().identity_provider(),
                "principalId": command.actor().principal_id(),
            },
            "reason": command.reason(),
            "invalidatedScheduleCount": invalidated.len(),
            "endedAt": record.ended_at(),
        });
        sqlx::query(
            "INSERT INTO outbox_events \
             (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at,attempt_count,delivered_at) \
             VALUES ($1,$2,'Document',$3,$4,$5,$5,0,NULL)",
        )
        .bind(record.domain_event_id().as_uuid())
        .bind(DOCUMENT_PUBLICATION_ENDED)
        .bind(command.document_id().as_uuid())
        .bind(payload.clone())
        .bind(record.ended_at())
        .execute(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        let subject = format!("document/{}", command.document_id().as_uuid());
        sqlx::query(
            "INSERT INTO audit_outbox_events \
             (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id, \
              resource_id,resource_version_id,result,trace_id,data,occurred_at,attempt_count,delivered_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'success',NULL,$9,$10,0,NULL)",
        )
        .bind(record.audit_event_id().as_uuid())
        .bind(AUDIT_DOCUMENT_PUBLICATION_ENDED)
        .bind(AUDIT_SOURCE)
        .bind(subject)
        .bind(command.actor().identity_provider())
        .bind(command.actor().principal_id())
        .bind(command.document_id().as_uuid())
        .bind(current_id)
        .bind(payload)
        .bind(record.ended_at())
        .execute(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        Ok(result)
    }
    .await;
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
