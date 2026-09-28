use document_application::{
    InvocationKind, MarkVersionRead, ReadStateRepository, ReadStateResult, RepositoryError,
    VerifiedActorContext,
};
use document_domain::{Action, ResourceRef};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state},
    error::{map_commit_error, map_statement_error},
    targeted_events::record_authorization_denied,
};

impl ReadStateRepository for PostgresDocumentRepository {
    async fn mark_version_read(
        &self,
        ctx: &VerifiedActorContext,
        command: MarkVersionRead,
    ) -> Result<ReadStateResult, RepositoryError> {
        ctx.ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        if ctx.invocation_kind() != InvocationKind::HumanInteractive {
            return Err(RepositoryError::Forbidden);
        }

        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<ReadStateResult, RepositoryError> = async {
            // Policy mutations hold the exclusive guard; version switches hold the
            // Document row. Both must be ordered before the first-read decision.
            lock_access_state(&mut tx, AccessLockMode::Shared).await?;
            let current_version_id: Option<Uuid> = sqlx::query_scalar(
                "SELECT current_version_id FROM documents WHERE document_id = $1 FOR UPDATE",
            )
            .bind(command.document_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?
            .ok_or(RepositoryError::DocumentNotFound)?;

            // Do not disclose whether a Version exists under an unreadable Document.
            match authorize_in_tx(
                &mut tx,
                ctx,
                &[(
                    ResourceRef::Document(command.document_id),
                    vec![Action::Read],
                )],
            )
            .await
            {
                Err(RepositoryError::Forbidden) => return Err(RepositoryError::DocumentNotFound),
                other => other?,
            }

            let lifecycle_state: String = sqlx::query_scalar(
                "SELECT lifecycle_state FROM document_versions \
                 WHERE document_id = $1 AND document_version_id = $2",
            )
            .bind(command.document_id.as_uuid())
            .bind(command.document_version_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?
            .ok_or(RepositoryError::DocumentVersionNotFound)?;

            let ended: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM document_publication_end_operations \
                 WHERE document_id = $1)",
            )
            .bind(command.document_id.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            let current = current_version_id == Some(command.document_version_id.as_uuid())
                && lifecycle_state == "PUBLISHED"
                && !ended;
            if !current {
                authorize_in_tx(
                    &mut tx,
                    ctx,
                    &[(
                        ResourceRef::Document(command.document_id),
                        vec![Action::Read, Action::ReadHistory],
                    )],
                )
                .await?;
            }

            let existing: Option<OffsetDateTime> = sqlx::query_scalar(
                "SELECT first_read_at FROM document_read_states \
                 WHERE identity_provider = $1 AND principal_id = $2 \
                   AND document_version_id = $3",
            )
            .bind(ctx.principal().identity_provider())
            .bind(ctx.principal().principal_id())
            .bind(command.document_version_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if let Some(first_read_at) = existing {
                return Ok(ReadStateResult {
                    principal: ctx.principal().clone(),
                    document_version_id: command.document_version_id,
                    first_read_at,
                    inserted: false,
                });
            }
            if !current {
                return Err(RepositoryError::StaleVersion);
            }

            let first_read_at: Option<OffsetDateTime> = sqlx::query_scalar(
                "INSERT INTO document_read_states \
                 (identity_provider,principal_id,document_version_id,first_read_at) \
                 VALUES ($1,$2,$3,now()) ON CONFLICT DO NOTHING RETURNING first_read_at",
            )
            .bind(ctx.principal().identity_provider())
            .bind(ctx.principal().principal_id())
            .bind(command.document_version_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            let Some(first_read_at) = first_read_at else {
                let saved: OffsetDateTime = sqlx::query_scalar(
                    "SELECT first_read_at FROM document_read_states \
                     WHERE identity_provider = $1 AND principal_id = $2 \
                       AND document_version_id = $3",
                )
                .bind(ctx.principal().identity_provider())
                .bind(ctx.principal().principal_id())
                .bind(command.document_version_id.as_uuid())
                .fetch_one(&mut *tx)
                .await
                .map_err(map_statement_error)?;
                return Ok(ReadStateResult {
                    principal: ctx.principal().clone(),
                    document_version_id: command.document_version_id,
                    first_read_at: saved,
                    inserted: false,
                });
            };

            sqlx::query(
                "INSERT INTO audit_outbox_events \
                 (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id, \
                  resource_type,resource_id,resource_version_id,result,data,occurred_at) \
                 VALUES ($1,'document.version.read_confirmed', \
                         'urn:knowledge-platform:document-platform',$2,$3,$4, \
                         'Document',$5,$6,'success',$7,$8)",
            )
            .bind(Uuid::now_v7())
            .bind(format!("document/{}", command.document_id.as_uuid()))
            .bind(ctx.principal().identity_provider())
            .bind(ctx.principal().principal_id())
            .bind(command.document_id.as_uuid())
            .bind(command.document_version_id.as_uuid())
            .bind(json!({"document_version_id": command.document_version_id.as_uuid()}))
            .bind(first_read_at)
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;

            Ok(ReadStateResult {
                principal: ctx.principal().clone(),
                document_version_id: command.document_version_id,
                first_read_at,
                inserted: true,
            })
        }
        .await;

        match result {
            Ok(result) => {
                tx.commit().await.map_err(map_commit_error)?;
                Ok(result)
            }
            Err(error) => {
                tx.rollback().await.map_err(map_statement_error)?;
                if error == RepositoryError::Forbidden {
                    record_authorization_denied(&self.pool, ctx.principal(), "mark_version_read")
                        .await?;
                }
                Err(error)
            }
        }
    }
}
