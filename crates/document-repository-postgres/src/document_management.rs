use document_application::{
    ManagementCommand, ManagementErrorCode, ManagementMoveDetails, ManagementMutationResult,
    ManagementResult, RepositoryError, VerifiedActorContext, management_command_digest,
};
use document_domain::{Action, DocumentId, ResourceRef};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state},
    access_policy::{decode_result, insert_operation, postgres_timestamp_now, row_resource},
    document_revision::{MetadataRevisionInput, issue_metadata_revision},
    error::{map_commit_error, map_statement_error},
    targeted_events::insert_targeted_events,
};

async fn document_ended(
    tx: &mut Transaction<'_, Postgres>,
    document_id: DocumentId,
) -> Result<bool, RepositoryError> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM document_publication_end_operations WHERE document_id = $1)",
    )
    .bind(document_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(map_statement_error)
}

async fn authorize_metadata(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    document_id: DocumentId,
) -> Result<(), RepositoryError> {
    let mut actions = vec![Action::Read, Action::Write];
    if document_ended(tx, document_id).await? {
        actions.extend([Action::ReadHistory, Action::Administer]);
    }
    authorize_in_tx(tx, ctx, &[(ResourceRef::Document(document_id), actions)]).await
}

pub(crate) async fn authorize_management_operation(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    resource: ResourceRef,
    row: &PgRow,
) -> Result<(), RepositoryError> {
    let kind: String = row.try_get("operation_kind").map_err(map_statement_error)?;
    match (kind.as_str(), resource) {
        ("update_document_metadata", ResourceRef::Document(document_id)) => {
            sqlx::query("SELECT document_id FROM documents WHERE document_id = $1 FOR SHARE")
                .bind(document_id.as_uuid())
                .fetch_optional(&mut **tx)
                .await
                .map_err(map_statement_error)?
                .ok_or(RepositoryError::DocumentNotFound)?;
            authorize_metadata(tx, ctx, document_id).await
        }
        ("move_document", ResourceRef::Document(document_id)) => {
            sqlx::query("SELECT document_id FROM documents WHERE document_id = $1 FOR SHARE")
                .bind(document_id.as_uuid())
                .fetch_optional(&mut **tx)
                .await
                .map_err(map_statement_error)?
                .ok_or(RepositoryError::DocumentNotFound)?;
            let ManagementResult::DocumentMove(result) = decode_result(row)? else {
                return Err(RepositoryError::IntegrityViolation);
            };
            let movement = result.movement.ok_or(RepositoryError::IntegrityViolation)?;
            let mut actions = vec![Action::Read, Action::Write, Action::Administer];
            if document_ended(tx, document_id).await? {
                actions.push(Action::ReadHistory);
            }
            authorize_in_tx(
                tx,
                ctx,
                &[
                    (ResourceRef::Document(document_id), actions),
                    (
                        ResourceRef::Folder(movement.from_folder_id),
                        vec![Action::Administer],
                    ),
                    (
                        ResourceRef::Folder(movement.to_folder_id),
                        vec![Action::Administer],
                    ),
                ],
            )
            .await
        }
        ("set_access_policy", _) => {
            authorize_in_tx(tx, ctx, &[(resource, vec![Action::Administer])]).await
        }
        ("move_folder", ResourceRef::Folder(_)) => {
            let ManagementResult::FolderMutation(result) = decode_result(row)? else {
                return Err(RepositoryError::IntegrityViolation);
            };
            let movement = result.movement.ok_or(RepositoryError::IntegrityViolation)?;
            authorize_in_tx(
                tx,
                ctx,
                &[
                    (resource, vec![Action::Administer]),
                    (
                        ResourceRef::Folder(movement.from_folder_id),
                        vec![Action::Administer],
                    ),
                    (
                        ResourceRef::Folder(movement.to_folder_id),
                        vec![Action::Administer],
                    ),
                ],
            )
            .await
        }
        ("create_folder" | "rename_folder", ResourceRef::Folder(_)) => {
            authorize_in_tx(tx, ctx, &[(resource, vec![Action::Administer])]).await
        }
        _ => Err(RepositoryError::IntegrityViolation),
    }
}

pub(crate) async fn saved_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: uuid::Uuid,
) -> Result<Option<PgRow>, RepositoryError> {
    sqlx::query(
        "SELECT operation_id,operation_kind,resource_type,resource_id,command_digest, \
         changed,result,resulting_revision,occurred_at \
         FROM document_management_operations WHERE operation_id = $1",
    )
    .bind(operation_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)
}

pub(crate) async fn replay_management(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    row: &PgRow,
    digest: [u8; 32],
) -> Result<ManagementResult, RepositoryError> {
    let saved_resource = row_resource(row)?;
    authorize_management_operation(tx, ctx, saved_resource, row).await?;
    let saved_digest: Vec<u8> = row.try_get("command_digest").map_err(map_statement_error)?;
    if saved_digest != digest {
        return Err(RepositoryError::Management(
            ManagementErrorCode::OperationConflict,
        ));
    }
    decode_result(row)
}

impl PostgresDocumentRepository {
    pub(crate) async fn execute_metadata_update(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementResult, RepositoryError> {
        let ManagementCommand::UpdateDocumentMetadata {
            document_id,
            expected_document_revision,
            set,
            unset,
            reason,
            ..
        } = &command
        else {
            return Err(RepositoryError::BusinessRule);
        };
        let digest =
            management_command_digest(ctx, &command).map_err(|_| RepositoryError::BusinessRule)?;
        let document_id = *document_id;
        let expected_revision = *expected_document_revision;
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<ManagementResult, RepositoryError> = async {
            lock_access_state(&mut tx, AccessLockMode::Shared).await?;
            if let Some(row) = saved_operation(&mut tx, command.operation_id().as_uuid()).await? {
                return replay_management(&mut tx, ctx, &row, digest).await;
            }
            let row = sqlx::query(
                "SELECT revision, metadata, current_version_id FROM documents WHERE document_id = $1 FOR UPDATE",
            )
            .bind(document_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?
            .ok_or(RepositoryError::DocumentNotFound)?;
            // A concurrent request with the same operation ID may have committed
            // while this transaction waited for the Document row.
            if let Some(saved) = saved_operation(&mut tx, command.operation_id().as_uuid()).await? {
                return replay_management(&mut tx, ctx, &saved, digest).await;
            }
            authorize_metadata(&mut tx, ctx, document_id).await?;
            let current_revision: i64 = row.try_get("revision").map_err(map_statement_error)?;
            let current_version_id: Option<Uuid> = row
                .try_get("current_version_id")
                .map_err(map_statement_error)?;
            if current_revision != expected_revision {
                return Err(RepositoryError::Management(
                    ManagementErrorCode::RevisionConflict,
                ));
            }
            let previous: Value = row.try_get("metadata").map_err(map_statement_error)?;
            let mut metadata = previous
                .as_object()
                .cloned()
                .ok_or(RepositoryError::IntegrityViolation)?;
            for key in unset {
                metadata.remove(key);
            }
            for (key, value) in set {
                metadata.insert(key.clone(), value.clone());
            }
            let updated = Value::Object(metadata);
            let changed = updated != previous;
            let revision = current_revision
                .checked_add(i64::from(changed))
                .ok_or(RepositoryError::BusinessRule)?;
            if changed {
                let pending: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM document_publish_schedules \
                     WHERE document_id = $1 AND status = 'PENDING')",
                )
                .bind(document_id.as_uuid())
                .fetch_one(&mut *tx)
                .await
                .map_err(map_statement_error)?;
                if pending {
                    return Err(RepositoryError::Management(
                        ManagementErrorCode::ReservedDocument,
                    ));
                }
                sqlx::query(
                    "UPDATE documents SET metadata = $1, revision = $2 \
                     WHERE document_id = $3",
                )
                .bind(&updated)
                .bind(revision)
                .bind(document_id.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(map_statement_error)?;
            }
            let now = postgres_timestamp_now();
            let mutation = ManagementMutationResult {
                operation_id: command.operation_id(),
                resource: ResourceRef::Document(document_id),
                resulting_revision: revision,
                access_revision: None,
                policy_id: None,
                changed,
                occurred_at: now,
                document_metadata: Some(updated.clone()),
                movement: None,
            };
            insert_operation(&mut tx, ctx, &command, digest, &mutation).await?;
            if changed {
                issue_metadata_revision(
                    &mut tx,
                    MetadataRevisionInput {
                        document_id,
                        current_document_version_id: current_version_id
                            .map(document_domain::DocumentVersionId::from_uuid),
                        metadata: &updated,
                        operation_id: command.operation_id().as_uuid(),
                        actor: ctx.principal(),
                        reason,
                        created_at: now,
                    },
                )
                .await?;
                let mut keys: Vec<_> = set.keys().chain(unset.iter()).cloned().collect();
                keys.sort();
                keys.dedup();
                insert_targeted_events(
                    &mut tx,
                    ResourceRef::Document(document_id),
                    "DocumentMetadataChanged",
                    "document.metadata.changed",
                    ctx.principal(),
                    now,
                    json!({
                        "operation_id": command.operation_id().as_uuid().to_string(),
                        "document_id": document_id.as_uuid().to_string(),
                        "changed_keys": keys,
                        "document_revision": revision,
                        "reason": reason,
                    }),
                )
                .await?;
            }
            Ok(ManagementResult::MetadataUpdate(mutation))
        }
        .await;
        match result {
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

    pub(crate) async fn execute_document_move(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementResult, RepositoryError> {
        let ManagementCommand::MoveDocument {
            document_id,
            from_folder_id,
            to_folder_id,
            expected_document_revision,
            reason,
            ..
        } = &command
        else {
            return Err(RepositoryError::BusinessRule);
        };
        let digest =
            management_command_digest(ctx, &command).map_err(|_| RepositoryError::BusinessRule)?;
        let document_id = *document_id;
        let from = *from_folder_id;
        let to = *to_folder_id;
        let expected_revision = *expected_document_revision;
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<ManagementResult, RepositoryError> = async {
            let access_revision = lock_access_state(&mut tx, AccessLockMode::Exclusive).await?;
            if let Some(saved) = saved_operation(&mut tx, command.operation_id().as_uuid()).await? {
                return replay_management(&mut tx, ctx, &saved, digest).await;
            }
            let mut folder_ids = vec![from.as_uuid(), to.as_uuid()];
            folder_ids.sort();
            folder_ids.dedup();
            let folders = sqlx::query(
                "SELECT folder_id,status FROM folders WHERE folder_id = ANY($1) \
                 ORDER BY folder_id FOR UPDATE",
            )
            .bind(folder_ids)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if folders.len() != usize::from(from != to) + 1 {
                return Err(RepositoryError::FolderNotFound);
            }
            if folders
                .iter()
                .any(|row| row.get::<String, _>("status") != "ACTIVE")
            {
                return Err(RepositoryError::BusinessRule);
            }
            let document = sqlx::query(
                "SELECT folder_id,revision FROM documents WHERE document_id = $1 FOR UPDATE",
            )
            .bind(document_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?
            .ok_or(RepositoryError::DocumentNotFound)?;
            if let Some(saved) = saved_operation(&mut tx, command.operation_id().as_uuid()).await? {
                return replay_management(&mut tx, ctx, &saved, digest).await;
            }
            let mut document_actions = vec![Action::Read, Action::Write, Action::Administer];
            if document_ended(&mut tx, document_id).await? {
                document_actions.push(Action::ReadHistory);
            }
            authorize_in_tx(
                &mut tx,
                ctx,
                &[
                    (ResourceRef::Document(document_id), document_actions),
                    (ResourceRef::Folder(from), vec![Action::Administer]),
                    (ResourceRef::Folder(to), vec![Action::Administer]),
                ],
            )
            .await?;
            let actual_from: Uuid = document.try_get("folder_id").map_err(map_statement_error)?;
            let old_revision: i64 = document.try_get("revision").map_err(map_statement_error)?;
            if actual_from != from.as_uuid() || old_revision != expected_revision {
                return Err(RepositoryError::Management(
                    ManagementErrorCode::RevisionConflict,
                ));
            }
            let changed = from != to;
            let revision = old_revision
                .checked_add(i64::from(changed))
                .ok_or(RepositoryError::BusinessRule)?;
            let new_access_revision = if changed {
                let pending: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM document_publish_schedules \
                     WHERE document_id = $1 AND status = 'PENDING')",
                )
                .bind(document_id.as_uuid())
                .fetch_one(&mut *tx)
                .await
                .map_err(map_statement_error)?;
                if pending {
                    return Err(RepositoryError::Management(
                        ManagementErrorCode::ReservedDocument,
                    ));
                }
                sqlx::query(
                    "UPDATE documents SET folder_id = $1, revision = $2 WHERE document_id = $3",
                )
                .bind(to.as_uuid())
                .bind(revision)
                .bind(document_id.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(map_statement_error)?;
                sqlx::query_scalar::<_, i64>(
                    "UPDATE document_access_state SET access_revision = access_revision + 1 \
                     WHERE id = 1 RETURNING access_revision",
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(map_statement_error)?
            } else {
                access_revision
            };
            let now = postgres_timestamp_now();
            let mutation = ManagementMutationResult {
                operation_id: command.operation_id(),
                resource: ResourceRef::Document(document_id),
                resulting_revision: revision,
                access_revision: Some(new_access_revision),
                policy_id: None,
                changed,
                occurred_at: now,
                document_metadata: None,
                movement: Some(ManagementMoveDetails {
                    from_folder_id: from,
                    to_folder_id: to,
                    subtree_affected: None,
                }),
            };
            insert_operation(&mut tx, ctx, &command, digest, &mutation).await?;
            if changed {
                insert_targeted_events(
                    &mut tx,
                    ResourceRef::Document(document_id),
                    "DocumentMoved",
                    "document.moved",
                    ctx.principal(),
                    now,
                    json!({
                        "operation_id": command.operation_id().as_uuid().to_string(),
                        "document_id": document_id.as_uuid().to_string(),
                        "from_folder_id": from.as_uuid().to_string(),
                        "to_folder_id": to.as_uuid().to_string(),
                        "document_revision": revision,
                        "access_revision": new_access_revision,
                        "visibility_changed": true,
                        "reason": reason,
                    }),
                )
                .await?;
            }
            Ok(ManagementResult::DocumentMove(mutation))
        }
        .await;
        match result {
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
}
