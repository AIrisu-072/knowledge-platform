use document_application::{
    BootstrapRootPolicy, ManagementCommand, ManagementMutationResult, ManagementOperationId,
    ManagementRepository, ManagementResult, RepositoryError, VerifiedActorContext,
    management_command_digest,
};
use document_domain::{
    Action, FolderId, PolicyGrant, PolicyId, PolicyMode, PolicySubjectKind, PolicyTarget,
    ResourceRef,
};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state, read_grants},
    error::{map_commit_error, map_statement_error},
    targeted_events::{insert_targeted_events, record_authorization_denied, resource_parts},
};

fn target_parts(target: PolicyTarget) -> (Option<Uuid>, Option<Uuid>) {
    match target {
        PolicyTarget::Folder(id) => (Some(id.as_uuid()), None),
        PolicyTarget::Document(id) => (None, Some(id.as_uuid())),
    }
}

fn normalized_mode(mode: &PolicyMode) -> PolicyMode {
    match mode {
        PolicyMode::Inherit => PolicyMode::Inherit,
        PolicyMode::Explicit(grants) => {
            let mut grants = grants.clone();
            grants.sort_by(|a, b| a.subject().cmp(b.subject()));
            PolicyMode::Explicit(grants)
        }
    }
}

async fn load_binding(
    tx: &mut Transaction<'_, Postgres>,
    target: PolicyTarget,
) -> Result<Option<(PolicyId, i64, PolicyMode)>, RepositoryError> {
    let (folder_id, document_id) = target_parts(target);
    let row = sqlx::query(
        "SELECT policy_id, revision, mode FROM access_policy_bindings \
         WHERE folder_id = $1 OR document_id = $2 FOR UPDATE",
    )
    .bind(folder_id)
    .bind(document_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: Uuid = row.try_get("policy_id").map_err(map_statement_error)?;
    let revision: i64 = row.try_get("revision").map_err(map_statement_error)?;
    let mode: String = row.try_get("mode").map_err(map_statement_error)?;
    let mode = match mode.as_str() {
        "INHERIT" => PolicyMode::Inherit,
        "EXPLICIT" => PolicyMode::Explicit(read_grants(tx, id).await?),
        _ => return Err(RepositoryError::IntegrityViolation),
    };
    Ok(Some((PolicyId::from_uuid(id), revision, mode)))
}

async fn insert_grants(
    tx: &mut Transaction<'_, Postgres>,
    policy_id: Uuid,
    grants: &[PolicyGrant],
) -> Result<(), RepositoryError> {
    for grant in grants {
        let kind = match grant.subject().kind() {
            PolicySubjectKind::Principal => "principal",
            PolicySubjectKind::Group => "group",
            PolicySubjectKind::Role => "role",
        };
        for action in grant.actions() {
            let action = match action {
                Action::Read => "read",
                Action::ReadHistory => "read_history",
                Action::Write => "write",
                Action::Publish => "publish",
                Action::Administer => "administer",
            };
            sqlx::query(
                "INSERT INTO access_policy_grants \
                 (policy_id,subject_kind,identity_provider,subject_id,action) \
                 VALUES ($1,$2,$3,$4,$5)",
            )
            .bind(policy_id)
            .bind(kind)
            .bind(grant.subject().identity_provider())
            .bind(grant.subject().subject_id())
            .bind(action)
            .execute(&mut **tx)
            .await
            .map_err(map_statement_error)?;
        }
    }
    Ok(())
}

async fn persist_binding(
    tx: &mut Transaction<'_, Postgres>,
    target: PolicyTarget,
    existing: Option<PolicyId>,
    revision: i64,
    mode: &PolicyMode,
    now: OffsetDateTime,
) -> Result<Option<PolicyId>, RepositoryError> {
    if existing.is_none() && matches!(mode, PolicyMode::Inherit) {
        return Ok(None);
    }
    let policy_id = existing.unwrap_or_else(|| PolicyId::from_uuid(Uuid::now_v7()));
    let mode_code = if matches!(mode, PolicyMode::Inherit) {
        "INHERIT"
    } else {
        "EXPLICIT"
    };
    if existing.is_some() {
        sqlx::query(
            "UPDATE access_policy_bindings SET mode = $1, revision = $2, updated_at = $3 \
             WHERE policy_id = $4",
        )
        .bind(mode_code)
        .bind(revision)
        .bind(now)
        .bind(policy_id.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(map_statement_error)?;
        sqlx::query("DELETE FROM access_policy_grants WHERE policy_id = $1")
            .bind(policy_id.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(map_statement_error)?;
    } else {
        let (folder_id, document_id) = target_parts(target);
        sqlx::query(
            "INSERT INTO access_policy_bindings \
             (policy_id,folder_id,document_id,mode,revision,created_at,updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$6)",
        )
        .bind(policy_id.as_uuid())
        .bind(folder_id)
        .bind(document_id)
        .bind(mode_code)
        .bind(revision)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(map_statement_error)?;
    }
    if let PolicyMode::Explicit(grants) = mode {
        insert_grants(tx, policy_id.as_uuid(), grants).await?;
    }
    Ok(Some(policy_id))
}

fn row_resource(row: &sqlx::postgres::PgRow) -> Result<ResourceRef, RepositoryError> {
    let resource_type: String = row.try_get("resource_type").map_err(map_statement_error)?;
    let resource_id: Uuid = row.try_get("resource_id").map_err(map_statement_error)?;
    let resource = match resource_type.as_str() {
        "Document" => ResourceRef::Document(document_domain::DocumentId::from_uuid(resource_id)),
        "Folder" => ResourceRef::Folder(FolderId::from_uuid(resource_id)),
        "AccessPolicy" => ResourceRef::AccessPolicy(PolicyId::from_uuid(resource_id)),
        _ => return Err(RepositoryError::IntegrityViolation),
    };
    Ok(resource)
}

fn decode_result(row: &sqlx::postgres::PgRow) -> Result<ManagementResult, RepositoryError> {
    let operation_id: Uuid = row.try_get("operation_id").map_err(map_statement_error)?;
    let operation_id = ManagementOperationId::try_from_uuid(operation_id)
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let resource = row_resource(row)?;
    let data: Value = row.try_get("result").map_err(map_statement_error)?;
    let policy_id = data
        .get("policy_id")
        .and_then(Value::as_str)
        .map(|value| Uuid::parse_str(value).map(PolicyId::from_uuid))
        .transpose()
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let access_revision = data.get("access_revision").and_then(Value::as_i64);
    let result = ManagementMutationResult {
        operation_id,
        resource,
        resulting_revision: row
            .try_get("resulting_revision")
            .map_err(map_statement_error)?,
        access_revision,
        policy_id,
        changed: row.try_get("changed").map_err(map_statement_error)?,
        occurred_at: row.try_get("occurred_at").map_err(map_statement_error)?,
        document_metadata: None,
    };
    Ok(ManagementResult::PolicyMutation(result))
}

async fn insert_operation(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    command: &ManagementCommand,
    digest: [u8; 32],
    result: &ManagementMutationResult,
) -> Result<(), RepositoryError> {
    let (resource_type, resource_id) = resource_parts(result.resource);
    sqlx::query(
        "INSERT INTO document_management_operations \
         (operation_id,operation_kind,resource_type,resource_id,expected_revision, \
          actor_identity_provider,actor_principal_id,command_digest,changed,result, \
          resulting_revision,occurred_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(command.operation_id().as_uuid())
    .bind(command.operation_kind())
    .bind(resource_type)
    .bind(resource_id)
    .bind(command.expected_revision())
    .bind(ctx.principal().identity_provider())
    .bind(ctx.principal().principal_id())
    .bind(digest.to_vec())
    .bind(result.changed)
    .bind(json!({
        "policy_id": result.policy_id.map(|id| id.as_uuid().to_string()),
        "access_revision": result.access_revision,
    }))
    .bind(result.resulting_revision)
    .bind(result.occurred_at)
    .execute(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    Ok(())
}

impl PostgresDocumentRepository {
    pub async fn authorize_resource(
        &self,
        ctx: &VerifiedActorContext,
        resource: ResourceRef,
        actions: &[Action],
    ) -> Result<bool, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        let result = authorize_in_tx(&mut tx, ctx, &[(resource, actions.to_vec())]).await;
        tx.rollback().await.map_err(map_statement_error)?;
        match result {
            Ok(()) => Ok(true),
            Err(RepositoryError::Forbidden) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn execute_policy(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementResult, RepositoryError> {
        let ManagementCommand::SetAccessPolicy {
            target,
            expected_policy_revision,
            mode,
            ..
        } = &command
        else {
            return Err(RepositoryError::BusinessRule);
        };
        let digest =
            management_command_digest(ctx, &command).map_err(|_| RepositoryError::BusinessRule)?;
        let target = *target;
        let expected_policy_revision = *expected_policy_revision;
        let mode = normalized_mode(mode);
        let resource: ResourceRef = target.into();

        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<ManagementResult, RepositoryError> = async {
            let access_revision = lock_access_state(&mut tx, AccessLockMode::Exclusive).await?;
            let existing_operation = sqlx::query(
                "SELECT operation_id,operation_kind,resource_type,resource_id,command_digest, \
                        changed,result,resulting_revision,occurred_at \
                 FROM document_management_operations WHERE operation_id = $1",
            )
            .bind(command.operation_id().as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if let Some(row) = existing_operation {
                let saved_resource = row_resource(&row)?;
                authorize_in_tx(&mut tx, ctx, &[(saved_resource, vec![Action::Administer])])
                    .await?;
                let saved_digest: Vec<u8> =
                    row.try_get("command_digest").map_err(map_statement_error)?;
                if saved_digest != digest {
                    return Err(RepositoryError::Conflict);
                }
                return decode_result(&row);
            }

            authorize_in_tx(&mut tx, ctx, &[(resource, vec![Action::Administer])]).await?;
            if target == PolicyTarget::Folder(FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID)) {
                return Err(RepositoryError::BusinessRule);
            }

            let existing = load_binding(&mut tx, target).await?;
            let current_revision = existing.as_ref().map_or(0, |(_, revision, _)| *revision);
            if current_revision != expected_policy_revision {
                return Err(RepositoryError::Conflict);
            }
            let changed = existing
                .as_ref()
                .is_none_or(|(_, _, old)| normalized_mode(old) != mode)
                && !(existing.is_none() && matches!(mode, PolicyMode::Inherit));
            let now = OffsetDateTime::now_utc();
            let new_revision = if changed {
                current_revision + 1
            } else {
                current_revision
            };
            let policy_id = if changed {
                persist_binding(
                    &mut tx,
                    target,
                    existing.as_ref().map(|(id, _, _)| *id),
                    new_revision,
                    &mode,
                    now,
                )
                .await?
            } else {
                existing.as_ref().map(|(id, _, _)| *id)
            };
            let new_access_revision = if changed {
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
            let mutation = ManagementMutationResult {
                operation_id: command.operation_id(),
                resource,
                resulting_revision: new_revision,
                access_revision: Some(new_access_revision),
                policy_id,
                changed,
                occurred_at: now,
                document_metadata: None,
            };
            insert_operation(&mut tx, ctx, &command, digest, &mutation).await?;
            if changed {
                insert_targeted_events(
                    &mut tx,
                    resource,
                    "AccessPolicyChanged",
                    "access_policy.changed",
                    ctx.principal(),
                    now,
                    json!({
                        "operation_id": command.operation_id().as_uuid().to_string(),
                        "target_type": resource_parts(resource).0,
                        "target_id": resource_parts(resource).1.to_string(),
                        "policy_id": policy_id.map(|id| id.as_uuid().to_string()),
                        "policy_revision": new_revision,
                        "access_revision": new_access_revision,
                    }),
                )
                .await?;
            }
            Ok(ManagementResult::PolicyMutation(mutation))
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

impl ManagementRepository for PostgresDocumentRepository {
    async fn execute(
        &self,
        ctx: &VerifiedActorContext,
        command: ManagementCommand,
    ) -> Result<ManagementResult, RepositoryError> {
        let result = self.execute_policy(ctx, command).await;
        if matches!(result, Err(RepositoryError::Forbidden))
            && record_authorization_denied(&self.pool, ctx.principal(), "set_access_policy")
                .await
                .is_err()
        {
            eprintln!("authorization denial audit staging failed");
        }
        result
    }

    async fn lookup(
        &self,
        ctx: &VerifiedActorContext,
        operation_id: ManagementOperationId,
    ) -> Result<Option<ManagementResult>, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<Option<ManagementResult>, RepositoryError> = async {
            lock_access_state(&mut tx, AccessLockMode::Shared).await?;
            let row = sqlx::query(
                "SELECT operation_id,operation_kind,resource_type,resource_id,changed,result, \
                    resulting_revision,occurred_at \
             FROM document_management_operations WHERE operation_id = $1",
            )
            .bind(operation_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if let Some(row) = row {
                let resource = row_resource(&row)?;
                authorize_in_tx(&mut tx, ctx, &[(resource, vec![Action::Administer])]).await?;
                Ok(Some(decode_result(&row)?))
            } else {
                Ok(None)
            }
        }
        .await;
        let _ = tx.rollback().await;
        if matches!(result, Err(RepositoryError::Forbidden))
            && record_authorization_denied(
                &self.pool,
                ctx.principal(),
                "lookup_management_operation",
            )
            .await
            .is_err()
        {
            eprintln!("authorization denial audit staging failed");
        }
        result
    }
}

impl BootstrapRootPolicy for PostgresDocumentRepository {
    async fn initialize_root_policy(
        &self,
        trusted_bootstrap_actor: &VerifiedActorContext,
        grants: Vec<PolicyGrant>,
    ) -> Result<ManagementResult, RepositoryError> {
        if self.bootstrap_actor.as_ref() != Some(trusted_bootstrap_actor.principal()) {
            return Err(RepositoryError::Forbidden);
        }
        trusted_bootstrap_actor
            .ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        PolicyMode::validate_explicit(&grants).map_err(|_| RepositoryError::BusinessRule)?;
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<ManagementResult, RepositoryError> = async {
            let prior_revision = lock_access_state(&mut tx, AccessLockMode::Exclusive).await?;
            let root = PolicyTarget::Folder(FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID));
            if load_binding(&mut tx, root).await?.is_some() {
                return Err(RepositoryError::BusinessRule);
            }
            let now = OffsetDateTime::now_utc();
            let policy_id =
                persist_binding(&mut tx, root, None, 1, &PolicyMode::Explicit(grants), now)
                    .await?
                    .ok_or(RepositoryError::IntegrityViolation)?;
            let access_revision: i64 = sqlx::query_scalar(
                "UPDATE document_access_state SET access_revision = access_revision + 1 \
                 WHERE id = 1 RETURNING access_revision",
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if access_revision != prior_revision + 1 {
                return Err(RepositoryError::IntegrityViolation);
            }
            let operation_id = ManagementOperationId::try_from_uuid(Uuid::now_v7())
                .map_err(|_| RepositoryError::IntegrityViolation)?;
            insert_targeted_events(
                &mut tx,
                ResourceRef::Folder(FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID)),
                "AccessPolicyChanged",
                "access_policy.changed",
                trusted_bootstrap_actor.principal(),
                now,
                json!({
                    "operation_id": operation_id.as_uuid().to_string(),
                    "target_type": "Folder", "target_id": SYSTEM_ROOT_FOLDER_ID.to_string(),
                    "policy_id": policy_id.as_uuid().to_string(),
                    "policy_revision": 1, "access_revision": access_revision,
                    "bootstrap": true,
                }),
            )
            .await?;
            Ok(ManagementResult::PolicyMutation(ManagementMutationResult {
                operation_id,
                resource: ResourceRef::Folder(FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID)),
                resulting_revision: 1,
                access_revision: Some(access_revision),
                policy_id: Some(policy_id),
                changed: true,
                occurred_at: now,
                document_metadata: None,
            }))
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
