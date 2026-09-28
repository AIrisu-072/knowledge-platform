use std::collections::HashMap;

use document_application::{
    ManagementCommand, ManagementMoveDetails, ManagementMutationResult, ManagementResult,
    RepositoryError, VerifiedActorContext, management_command_digest,
};
use document_domain::{
    Action, DocumentId, FolderId, PolicyMode, ResourceRef, evaluate_policy, normalize_folder_name,
};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID,
    access_control::{
        AccessLockMode, authorize_in_tx, lock_access_state, nearest_policy_id, read_grants,
    },
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

async fn policy_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    resources: &[ResourceRef],
) -> Result<Vec<Uuid>, RepositoryError> {
    let mut policies = Vec::with_capacity(resources.len());
    for resource in resources {
        policies.push(
            nearest_policy_id(tx, *resource)
                .await?
                .ok_or(RepositoryError::IntegrityViolation)?,
        );
    }
    Ok(policies)
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
                movement: None,
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

impl PostgresDocumentRepository {
    pub(crate) async fn execute_folder_move(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementResult, RepositoryError> {
        let ManagementCommand::MoveFolder {
            folder_id,
            from_parent_id,
            to_parent_id,
            expected_folder_revision,
            reason,
            ..
        } = &command
        else {
            return Err(RepositoryError::BusinessRule);
        };
        let digest =
            management_command_digest(ctx, &command).map_err(|_| RepositoryError::BusinessRule)?;
        let folder_id = *folder_id;
        let from = *from_parent_id;
        let to = *to_parent_id;
        let expected_revision = *expected_folder_revision;
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<ManagementResult, RepositoryError> = async {
            sqlx::query("SET LOCAL statement_timeout = '15s'")
                .execute(&mut *tx)
                .await
                .map_err(map_statement_error)?;
            let access_revision = lock_access_state(&mut tx, AccessLockMode::Exclusive).await?;
            if let Some(saved) = saved_operation(&mut tx, command.operation_id().as_uuid()).await? {
                return replay_management(&mut tx, ctx, &saved, digest).await;
            }
            if folder_id.as_uuid() == SYSTEM_ROOT_FOLDER_ID {
                return Err(RepositoryError::BusinessRule);
            }
            let subtree_ids: Vec<Uuid> = sqlx::query_scalar(
                "WITH RECURSIVE subtree(folder_id) AS ( \
                   SELECT folder_id FROM folders WHERE folder_id = $1 \
                   UNION \
                   SELECT child.folder_id FROM folders child \
                   JOIN subtree parent ON child.parent_folder_id = parent.folder_id \
                 ) SELECT folder_id FROM subtree ORDER BY folder_id",
            )
            .bind(folder_id.as_uuid())
            .fetch_all(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if subtree_ids.is_empty() {
                return Err(RepositoryError::FolderNotFound);
            }
            if subtree_ids.contains(&to.as_uuid()) {
                return Err(RepositoryError::BusinessRule);
            }
            let mut lock_ids = subtree_ids.clone();
            lock_ids.extend([from.as_uuid(), to.as_uuid()]);
            lock_ids.sort();
            lock_ids.dedup();
            let locked = sqlx::query(
                "SELECT folder_id,parent_folder_id,status,revision FROM folders \
                 WHERE folder_id = ANY($1) ORDER BY folder_id FOR UPDATE",
            )
            .bind(&lock_ids)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if locked.len() != lock_ids.len() {
                return Err(RepositoryError::FolderNotFound);
            }
            let moved = locked
                .iter()
                .find(|row| row.get::<Uuid, _>("folder_id") == folder_id.as_uuid())
                .ok_or(RepositoryError::FolderNotFound)?;
            if locked
                .iter()
                .any(|row| row.get::<String, _>("status") != "ACTIVE")
            {
                return Err(RepositoryError::BusinessRule);
            }
            let actual_parent: Option<Uuid> = moved
                .try_get("parent_folder_id")
                .map_err(map_statement_error)?;
            let old_revision: i64 = moved.try_get("revision").map_err(map_statement_error)?;
            if actual_parent != Some(from.as_uuid()) || old_revision != expected_revision {
                return Err(RepositoryError::Conflict);
            }
            let document_ids: Vec<Uuid> = sqlx::query_scalar(
                "SELECT document_id FROM documents WHERE folder_id = ANY($1) \
                 ORDER BY document_id FOR UPDATE",
            )
            .bind(&subtree_ids)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if let Some(saved) = saved_operation(&mut tx, command.operation_id().as_uuid()).await? {
                return replay_management(&mut tx, ctx, &saved, digest).await;
            }
            authorize_in_tx(
                &mut tx,
                ctx,
                &[
                    (ResourceRef::Folder(folder_id), vec![Action::Administer]),
                    (ResourceRef::Folder(from), vec![Action::Administer]),
                    (ResourceRef::Folder(to), vec![Action::Administer]),
                ],
            )
            .await?;
            let changed = from != to;
            let revision = old_revision
                .checked_add(i64::from(changed))
                .ok_or(RepositoryError::BusinessRule)?;
            let mut affected = 0_u64;
            let new_access_revision = if changed {
                let pending: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM document_publish_schedules \
                     WHERE document_id = ANY($1) AND status = 'PENDING')",
                )
                .bind(&document_ids)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_statement_error)?;
                if pending {
                    return Err(RepositoryError::BusinessRule);
                }
                let resources: Vec<ResourceRef> = subtree_ids
                    .iter()
                    .map(|id| ResourceRef::Folder(FolderId::from_uuid(*id)))
                    .chain(
                        document_ids
                            .iter()
                            .map(|id| ResourceRef::Document(DocumentId::from_uuid(*id))),
                    )
                    .collect();
                let before = policy_snapshot(&mut tx, &resources).await?;
                sqlx::query(
                    "UPDATE folders SET parent_folder_id = $1, revision = $2 WHERE folder_id = $3",
                )
                .bind(to.as_uuid())
                .bind(revision)
                .bind(folder_id.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(map_statement_error)?;
                let after = policy_snapshot(&mut tx, &resources).await?;
                let mut administer_cache = HashMap::new();
                for (prior_policy, next_policy) in before.into_iter().zip(after) {
                    if prior_policy != next_policy {
                        affected += 1;
                        let has_administer =
                            if let Some(allowed) = administer_cache.get(&prior_policy) {
                                *allowed
                            } else {
                                let grants = read_grants(&mut tx, prior_policy).await?;
                                let allowed = evaluate_policy(
                                    ctx.subjects(),
                                    &PolicyMode::Explicit(grants),
                                    &[Action::Administer],
                                );
                                administer_cache.insert(prior_policy, allowed);
                                allowed
                            };
                        if !has_administer {
                            return Err(RepositoryError::Forbidden);
                        }
                    }
                }
                ctx.ensure_current()
                    .map_err(|_| RepositoryError::Forbidden)?;
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
                resource: ResourceRef::Folder(folder_id),
                resulting_revision: revision,
                access_revision: Some(new_access_revision),
                policy_id: None,
                changed,
                occurred_at: now,
                document_metadata: None,
                movement: Some(ManagementMoveDetails {
                    from_folder_id: from,
                    to_folder_id: to,
                    subtree_affected: Some(affected),
                }),
            };
            insert_operation(&mut tx, ctx, &command, digest, &mutation).await?;
            if changed {
                insert_targeted_events(
                    &mut tx,
                    ResourceRef::Folder(folder_id),
                    "FolderMoved",
                    "folder.moved",
                    ctx.principal(),
                    now,
                    json!({
                        "operation_id": command.operation_id().as_uuid().to_string(),
                        "folder_id": folder_id.as_uuid().to_string(),
                        "from_parent_id": from.as_uuid().to_string(),
                        "to_parent_id": to.as_uuid().to_string(),
                        "folder_revision": revision,
                        "access_revision": new_access_revision,
                        "subtree_affected": affected,
                        "reason": reason,
                    }),
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
