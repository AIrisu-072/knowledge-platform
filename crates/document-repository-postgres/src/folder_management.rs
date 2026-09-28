use document_application::{
    ManagementCommand, ManagementMutationResult, ManagementResult, RepositoryError,
    VerifiedActorContext, management_command_digest,
};
use document_domain::{Action, ResourceRef, normalize_folder_name};
use serde_json::json;
use sqlx::Row;

use crate::{
    PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state},
    access_policy::{insert_operation, postgres_timestamp_now},
    document_management::{replay_management, saved_operation},
    error::{map_commit_error, map_statement_error},
    targeted_events::insert_targeted_events,
};

fn map_folder_write_error(error: sqlx::Error) -> RepositoryError {
    if let sqlx::Error::Database(database) = &error
        && database.code().as_deref() == Some("23505")
    {
        return RepositoryError::Conflict;
    }
    map_statement_error(error)
}

impl PostgresDocumentRepository {
    pub(crate) async fn execute_folder_mutation(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementResult, RepositoryError> {
        let digest =
            management_command_digest(ctx, &command).map_err(|_| RepositoryError::BusinessRule)?;
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<ManagementResult, RepositoryError> = async {
            lock_access_state(&mut tx, AccessLockMode::Shared).await?;
            if let Some(saved) = saved_operation(&mut tx, command.operation_id().as_uuid()).await? {
                return replay_management(&mut tx, ctx, &saved, digest).await;
            }
            let (resource, revision, changed, event) = match &command {
                ManagementCommand::CreateFolder {
                    folder_id,
                    parent_folder_id,
                    expected_parent_revision,
                    name,
                    ..
                } => {
                    if folder_id.as_uuid() == SYSTEM_ROOT_FOLDER_ID {
                        return Err(RepositoryError::BusinessRule);
                    }
                    let parent = sqlx::query(
                        "SELECT revision,status FROM folders WHERE folder_id = $1 FOR UPDATE",
                    )
                    .bind(parent_folder_id.as_uuid())
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(map_statement_error)?
                    .ok_or(RepositoryError::FolderNotFound)?;
                    if let Some(saved) =
                        saved_operation(&mut tx, command.operation_id().as_uuid()).await?
                    {
                        return replay_management(&mut tx, ctx, &saved, digest).await;
                    }
                    authorize_in_tx(
                        &mut tx,
                        ctx,
                        &[(
                            ResourceRef::Folder(*parent_folder_id),
                            vec![Action::Administer],
                        )],
                    )
                    .await?;
                    let parent_revision: i64 =
                        parent.try_get("revision").map_err(map_statement_error)?;
                    let status: String = parent.try_get("status").map_err(map_statement_error)?;
                    if status != "ACTIVE" {
                        return Err(RepositoryError::BusinessRule);
                    }
                    if parent_revision != *expected_parent_revision {
                        return Err(RepositoryError::Conflict);
                    }
                    let normalized =
                        normalize_folder_name(name).map_err(|_| RepositoryError::BusinessRule)?;
                    let now = postgres_timestamp_now();
                    sqlx::query(
                        "INSERT INTO folders \
                         (folder_id,parent_folder_id,name,status,revision,created_at) \
                         VALUES ($1,$2,$3,'ACTIVE',0,$4)",
                    )
                    .bind(folder_id.as_uuid())
                    .bind(parent_folder_id.as_uuid())
                    .bind(normalized)
                    .bind(now)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_folder_write_error)?;
                    (
                        ResourceRef::Folder(*folder_id),
                        0,
                        true,
                        Some((
                            "FolderCreated",
                            "folder.created",
                            json!({
                                "folder_id": folder_id.as_uuid().to_string(),
                                "parent_folder_id": parent_folder_id.as_uuid().to_string(),
                                "folder_revision": 0,
                            }),
                        )),
                    )
                }
                ManagementCommand::RenameFolder {
                    folder_id,
                    expected_folder_revision,
                    name,
                    ..
                } => {
                    let row = sqlx::query(
                        "SELECT revision,status,name FROM folders WHERE folder_id = $1 FOR UPDATE",
                    )
                    .bind(folder_id.as_uuid())
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(map_statement_error)?
                    .ok_or(RepositoryError::FolderNotFound)?;
                    if let Some(saved) =
                        saved_operation(&mut tx, command.operation_id().as_uuid()).await?
                    {
                        return replay_management(&mut tx, ctx, &saved, digest).await;
                    }
                    authorize_in_tx(
                        &mut tx,
                        ctx,
                        &[(ResourceRef::Folder(*folder_id), vec![Action::Administer])],
                    )
                    .await?;
                    if folder_id.as_uuid() == SYSTEM_ROOT_FOLDER_ID {
                        return Err(RepositoryError::BusinessRule);
                    }
                    let status: String = row.try_get("status").map_err(map_statement_error)?;
                    if status != "ACTIVE" {
                        return Err(RepositoryError::BusinessRule);
                    }
                    let old_revision: i64 = row.try_get("revision").map_err(map_statement_error)?;
                    if old_revision != *expected_folder_revision {
                        return Err(RepositoryError::Conflict);
                    }
                    let old_name: String = row.try_get("name").map_err(map_statement_error)?;
                    let normalized =
                        normalize_folder_name(name).map_err(|_| RepositoryError::BusinessRule)?;
                    let changed = old_name != normalized;
                    let revision = old_revision
                        .checked_add(i64::from(changed))
                        .ok_or(RepositoryError::BusinessRule)?;
                    if changed {
                        sqlx::query(
                            "UPDATE folders SET name = $1, revision = $2 WHERE folder_id = $3",
                        )
                        .bind(normalized)
                        .bind(revision)
                        .bind(folder_id.as_uuid())
                        .execute(&mut *tx)
                        .await
                        .map_err(map_folder_write_error)?;
                    }
                    let event = changed.then(|| {
                        (
                            "FolderRenamed",
                            "folder.renamed",
                            json!({
                                "folder_id": folder_id.as_uuid().to_string(),
                                "folder_revision": revision,
                            }),
                        )
                    });
                    (ResourceRef::Folder(*folder_id), revision, changed, event)
                }
                _ => return Err(RepositoryError::BusinessRule),
            };
            let now = postgres_timestamp_now();
            let mutation = ManagementMutationResult {
                operation_id: command.operation_id(),
                resource,
                resulting_revision: revision,
                access_revision: None,
                policy_id: None,
                changed,
                occurred_at: now,
                document_metadata: None,
            };
            insert_operation(&mut tx, ctx, &command, digest, &mutation).await?;
            if let Some((domain_type, audit_type, mut payload)) = event {
                payload["operation_id"] = json!(command.operation_id().as_uuid().to_string());
                payload["reason"] = json!(command.reason());
                insert_targeted_events(
                    &mut tx,
                    resource,
                    domain_type,
                    audit_type,
                    ctx.principal(),
                    now,
                    payload,
                )
                .await?;
            }
            Ok(ManagementResult::FolderMutation(mutation))
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
