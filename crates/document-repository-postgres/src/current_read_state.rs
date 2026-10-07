use document_application::{
    CurrentReadProjection, CurrentReadState, CurrentReadStateRepository, InvocationKind,
    MAX_READ_STATE_REVISION, ReadStateMutation, ReadStateMutationKind, ReadStateMutationResult,
    RepositoryError, VerifiedActorContext, read_state_command_digest,
};
use document_domain::{Action, DocumentId, DocumentVersionId, ResourceRef};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state},
    error::{map_commit_error, map_statement_error},
    targeted_events::record_authorization_denied,
};

fn require_human(ctx: &VerifiedActorContext) -> Result<(), RepositoryError> {
    ctx.ensure_current()
        .map_err(|_| RepositoryError::Forbidden)?;
    if ctx.invocation_kind() != InvocationKind::HumanInteractive {
        return Err(RepositoryError::Forbidden);
    }
    Ok(())
}

async fn lock_authorized_target(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    document_id: DocumentId,
    version_id: DocumentVersionId,
    write: bool,
) -> Result<bool, RepositoryError> {
    lock_access_state(tx, AccessLockMode::Shared).await?;
    let statement = if write {
        "SELECT current_version_id FROM documents WHERE document_id=$1 FOR UPDATE"
    } else {
        "SELECT current_version_id FROM documents WHERE document_id=$1 FOR SHARE"
    };
    let current_id: Option<Uuid> = sqlx::query_scalar(statement)
        .bind(document_id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_statement_error)?
        .ok_or(RepositoryError::DocumentNotFound)?;
    // authorize_in_tx rechecks identity freshness after waiting for both locks.
    match authorize_in_tx(
        tx,
        ctx,
        &[(ResourceRef::Document(document_id), vec![Action::Read])],
    )
    .await
    {
        Err(RepositoryError::Forbidden) => return Err(RepositoryError::DocumentNotFound),
        other => other?,
    }
    let lifecycle: String = sqlx::query_scalar(
        "SELECT lifecycle_state FROM document_versions \
         WHERE document_id=$1 AND document_version_id=$2",
    )
    .bind(document_id.as_uuid())
    .bind(version_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?
    .ok_or(RepositoryError::DocumentVersionNotFound)?;
    let ended: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM document_publication_end_operations WHERE document_id=$1)",
    )
    .bind(document_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    let current = current_id == Some(version_id.as_uuid()) && lifecycle == "PUBLISHED" && !ended;
    if !current {
        authorize_in_tx(
            tx,
            ctx,
            &[(
                ResourceRef::Document(document_id),
                vec![Action::Read, Action::ReadHistory],
            )],
        )
        .await?;
    }
    Ok(current)
}

pub(crate) fn decode_projection(row: &PgRow) -> Result<CurrentReadProjection, RepositoryError> {
    Ok(CurrentReadProjection {
        first_read_at: row.try_get("first_read_at").map_err(map_statement_error)?,
        needs_recheck: row
            .try_get::<Option<bool>, _>("needs_recheck")
            .map_err(map_statement_error)?
            .unwrap_or(false),
        read_state_revision: row
            .try_get::<Option<i64>, _>("read_state_revision")
            .map_err(map_statement_error)?
            .unwrap_or(0),
    })
}

async fn own_projection(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    version_id: DocumentVersionId,
) -> Result<CurrentReadProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT first_read_at,needs_recheck,read_state_revision FROM document_read_states \
         WHERE identity_provider=$1 AND principal_id=$2 AND document_version_id=$3",
    )
    .bind(ctx.principal().identity_provider())
    .bind(ctx.principal().principal_id())
    .bind(version_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    row.as_ref()
        .map(decode_projection)
        .transpose()
        .map(|state| state.unwrap_or_default())
}

enum MutationOutcome {
    Saved(ReadStateMutationResult),
    ReceiptCollision,
}

async fn mutate_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    command: ReadStateMutation,
    digest: &[u8; 32],
) -> Result<MutationOutcome, RepositoryError> {
    let current = lock_authorized_target(
        tx,
        ctx,
        command.document_id,
        command.document_version_id,
        true,
    )
    .await?;
    let receipt = sqlx::query(
        "SELECT * FROM document_read_state_operations \
         WHERE identity_provider=$1 AND principal_id=$2 AND operation_id=$3",
    )
    .bind(ctx.principal().identity_provider())
    .bind(ctx.principal().principal_id())
    .bind(command.operation_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    if let Some(row) = receipt {
        let saved_digest: Vec<u8> = row.try_get("command_digest").map_err(map_statement_error)?;
        if saved_digest.as_slice() != digest {
            return Err(RepositoryError::OperationConflict);
        }
        return Ok(MutationOutcome::Saved(ReadStateMutationResult {
            operation_id: command.operation_id,
            document_id: command.document_id,
            document_version_id: command.document_version_id,
            kind: command.kind,
            expected_read_state_revision: row
                .try_get("expected_read_state_revision")
                .map_err(map_statement_error)?,
            changed: row.try_get("changed").map_err(map_statement_error)?,
            occurred_at: row.try_get("occurred_at").map_err(map_statement_error)?,
            resulting_read_state: CurrentReadProjection {
                first_read_at: Some(row.try_get("first_read_at").map_err(map_statement_error)?),
                needs_recheck: row.try_get("needs_recheck").map_err(map_statement_error)?,
                read_state_revision: row
                    .try_get("resulting_read_state_revision")
                    .map_err(map_statement_error)?,
            },
        }));
    }
    if !current {
        return Err(RepositoryError::StaleVersion);
    }
    let before = own_projection(tx, ctx, command.document_version_id).await?;
    if before.read_state_revision != command.expected_read_state_revision {
        return Err(RepositoryError::ReadStateRevisionConflict);
    }
    let changed = match command.kind {
        ReadStateMutationKind::View => !before.is_read(),
        ReadStateMutationKind::Reset => {
            if !before.is_read() {
                return Err(RepositoryError::BusinessRule);
            }
            true
        }
    };
    if changed && before.read_state_revision == MAX_READ_STATE_REVISION {
        return Err(RepositoryError::BusinessRule);
    }
    let occurred_at: OffsetDateTime = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut **tx)
        .await
        .map_err(map_statement_error)?;
    let after = CurrentReadProjection {
        first_read_at: Some(before.first_read_at.unwrap_or(occurred_at)),
        needs_recheck: command.kind == ReadStateMutationKind::Reset,
        read_state_revision: before.read_state_revision + i64::from(changed),
    };
    if changed {
        if before.first_read_at.is_none() {
            sqlx::query(
                "INSERT INTO document_read_states \
                 (identity_provider,principal_id,document_version_id,first_read_at,needs_recheck,read_state_revision) \
                 VALUES($1,$2,$3,$4,$5,$6)",
            )
            .bind(ctx.principal().identity_provider())
            .bind(ctx.principal().principal_id())
            .bind(command.document_version_id.as_uuid())
            .bind(after.first_read_at)
            .bind(after.needs_recheck)
            .bind(after.read_state_revision)
            .execute(&mut **tx)
            .await
            .map_err(map_statement_error)?;
        } else {
            sqlx::query(
                "UPDATE document_read_states SET needs_recheck=$4,read_state_revision=$5 \
                 WHERE identity_provider=$1 AND principal_id=$2 AND document_version_id=$3",
            )
            .bind(ctx.principal().identity_provider())
            .bind(ctx.principal().principal_id())
            .bind(command.document_version_id.as_uuid())
            .bind(after.needs_recheck)
            .bind(after.read_state_revision)
            .execute(&mut **tx)
            .await
            .map_err(map_statement_error)?;
        }
        let (event_type, trigger) = match command.kind {
            ReadStateMutationKind::View => ("document.version.detail_viewed", "detail_display"),
            ReadStateMutationKind::Reset => ("document.version.marked_unread", "user_reset"),
        };
        let mut data = json!({
            "document_version_id": command.document_version_id.as_uuid(),
            "operation_id": command.operation_id.as_uuid(),
            "expected_read_state_revision": command.expected_read_state_revision,
            "resulting_read_state_revision": after.read_state_revision,
            "trigger": trigger
        });
        if command.kind == ReadStateMutationKind::View {
            data["first_record"] = json!(before.first_read_at.is_none());
        }
        sqlx::query(
            "INSERT INTO audit_outbox_events \
             (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id, \
              resource_type,resource_id,resource_version_id,result,data,occurred_at) \
             VALUES($1,$2,'urn:knowledge-platform:document-platform',$3,$4,$5,'Document',$6,$7,'success',$8,$9)",
        )
        .bind(Uuid::now_v7())
        .bind(event_type)
        .bind(format!("document/{}", command.document_id.as_uuid()))
        .bind(ctx.principal().identity_provider())
        .bind(ctx.principal().principal_id())
        .bind(command.document_id.as_uuid())
        .bind(command.document_version_id.as_uuid())
        .bind(data)
        .bind(occurred_at)
        .execute(&mut **tx)
        .await
        .map_err(map_statement_error)?;
    }
    // Different Document locks cannot serialize a principal-wide operation ID.
    // Any losing INSERT must roll back the preceding state and Audit writes.
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO document_read_state_operations \
         (identity_provider,principal_id,operation_id,document_id,document_version_id,operation_kind, \
          command_digest,expected_read_state_revision,resulting_read_state_revision, \
          first_read_at,needs_recheck,changed,occurred_at) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) \
         ON CONFLICT DO NOTHING RETURNING operation_id",
    )
    .bind(ctx.principal().identity_provider())
    .bind(ctx.principal().principal_id())
    .bind(command.operation_id.as_uuid())
    .bind(command.document_id.as_uuid())
    .bind(command.document_version_id.as_uuid())
    .bind(command.kind.as_str())
    .bind(digest.as_slice())
    .bind(command.expected_read_state_revision)
    .bind(after.read_state_revision)
    .bind(after.first_read_at)
    .bind(after.needs_recheck)
    .bind(changed)
    .bind(occurred_at)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    if inserted.is_none() {
        return Ok(MutationOutcome::ReceiptCollision);
    }
    Ok(MutationOutcome::Saved(ReadStateMutationResult {
        operation_id: command.operation_id,
        document_id: command.document_id,
        document_version_id: command.document_version_id,
        kind: command.kind,
        expected_read_state_revision: command.expected_read_state_revision,
        changed,
        occurred_at,
        resulting_read_state: after,
    }))
}

impl CurrentReadStateRepository for PostgresDocumentRepository {
    async fn get_current_read_state(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
    ) -> Result<CurrentReadState, RepositoryError> {
        require_human(ctx)?;
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result = async {
            if !lock_authorized_target(&mut tx, ctx, document_id, document_version_id, false)
                .await?
            {
                return Err(RepositoryError::StaleVersion);
            }
            Ok(CurrentReadState {
                document_id,
                document_version_id,
                state: own_projection(&mut tx, ctx, document_version_id).await?,
            })
        }
        .await;
        tx.rollback().await.map_err(map_statement_error)?;
        if result.as_ref().err() == Some(&RepositoryError::Forbidden) {
            record_authorization_denied(&self.pool, ctx.principal(), "get_current_read_state")
                .await?;
        }
        result
    }

    async fn mutate_read_state(
        &self,
        ctx: &VerifiedActorContext,
        command: ReadStateMutation,
    ) -> Result<ReadStateMutationResult, RepositoryError> {
        require_human(ctx)?;
        let digest =
            read_state_command_digest(ctx, &command).map_err(|_| RepositoryError::BusinessRule)?;
        for _ in 0..2 {
            let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
            match mutate_in_tx(&mut tx, ctx, command, &digest).await {
                Ok(MutationOutcome::Saved(result)) => {
                    tx.commit().await.map_err(map_commit_error)?;
                    return Ok(result);
                }
                Ok(MutationOutcome::ReceiptCollision) => {
                    tx.rollback().await.map_err(map_statement_error)?;
                }
                Err(error) => {
                    tx.rollback().await.map_err(map_statement_error)?;
                    if error == RepositoryError::Forbidden {
                        record_authorization_denied(
                            &self.pool,
                            ctx.principal(),
                            "mutate_read_state",
                        )
                        .await?;
                    }
                    return Err(error);
                }
            }
        }
        Err(RepositoryError::IntegrityViolation)
    }
}
