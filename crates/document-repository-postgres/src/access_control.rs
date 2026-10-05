use std::collections::BTreeMap;

use document_application::{DocumentAccessCheckRepository, RepositoryError, VerifiedActorContext};
use document_domain::{
    Action, DocumentId, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind, PrincipalRef,
    ResourceRef, evaluate_policy,
};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::PostgresDocumentRepository;
use crate::error::map_statement_error;

impl DocumentAccessCheckRepository for PostgresDocumentRepository {
    async fn check_document_access(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
        required: &[Action],
    ) -> Result<(), RepositoryError> {
        authorize_document_snapshot(&self.pool, ctx, document_id, required).await
    }
}

pub(crate) fn verified_subjects_json(ctx: &VerifiedActorContext) -> serde_json::Value {
    serde_json::Value::Array(
        ctx.subjects()
            .iter()
            .map(|subject| {
                let kind = match subject.kind() {
                    PolicySubjectKind::Principal => "principal",
                    PolicySubjectKind::Group => "group",
                    PolicySubjectKind::Role => "role",
                };
                serde_json::json!({
                    "kind": kind,
                    "identity_provider": subject.identity_provider(),
                    "subject_id": subject.subject_id(),
                })
            })
            .collect(),
    )
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AccessLockMode {
    Shared,
    Exclusive,
}

pub(crate) async fn lock_access_state(
    tx: &mut Transaction<'_, Postgres>,
    mode: AccessLockMode,
) -> Result<i64, RepositoryError> {
    let statement = match mode {
        AccessLockMode::Shared => {
            "SELECT access_revision FROM document_access_state WHERE id = 1 FOR SHARE"
        }
        AccessLockMode::Exclusive => {
            "SELECT access_revision FROM document_access_state WHERE id = 1 FOR UPDATE"
        }
    };
    sqlx::query_scalar(statement)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_statement_error)?
        .ok_or(RepositoryError::IntegrityViolation)
}

pub(crate) async fn authorize_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    requirements: &[(ResourceRef, Vec<Action>)],
) -> Result<(), RepositoryError> {
    ctx.ensure_current()
        .map_err(|_| RepositoryError::Forbidden)?;
    if requirements.is_empty() {
        return Err(RepositoryError::Forbidden);
    }
    let root_initialized: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM access_policy_bindings \
         WHERE folder_id = $1 AND mode = 'EXPLICIT')",
    )
    .bind(crate::SYSTEM_ROOT_FOLDER_ID)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    if !root_initialized {
        return Err(RepositoryError::Forbidden);
    }
    for (resource, actions) in requirements {
        let policy_id = nearest_policy_id(tx, *resource).await?;
        let Some(policy_id) = policy_id else {
            return Err(RepositoryError::Forbidden);
        };
        let grants = read_grants(tx, policy_id).await?;
        if !evaluate_policy(ctx.subjects(), &PolicyMode::Explicit(grants), actions) {
            return Err(RepositoryError::Forbidden);
        }
    }
    Ok(())
}

pub(crate) async fn guard_document_mutation(
    tx: &mut Transaction<'_, Postgres>,
    ctx: Option<&VerifiedActorContext>,
    document_id: DocumentId,
    required: &[Action],
    recorded_actor: &PrincipalRef,
) -> Result<(), RepositoryError> {
    let Some(ctx) = ctx else {
        return Ok(());
    };
    lock_access_state(tx, AccessLockMode::Shared).await?;
    if ctx.principal() != recorded_actor {
        return Err(RepositoryError::Forbidden);
    }
    authorize_in_tx(
        tx,
        ctx,
        &[(ResourceRef::Document(document_id), required.to_vec())],
    )
    .await
}

pub(crate) async fn authorize_document_snapshot(
    pool: &PgPool,
    ctx: &VerifiedActorContext,
    document_id: DocumentId,
    required: &[Action],
) -> Result<(), RepositoryError> {
    let mut tx = pool.begin().await.map_err(map_statement_error)?;
    lock_access_state(&mut tx, AccessLockMode::Shared).await?;
    let result = authorize_in_tx(
        &mut tx,
        ctx,
        &[(ResourceRef::Document(document_id), required.to_vec())],
    )
    .await;
    tx.rollback().await.map_err(map_statement_error)?;
    result
}

pub(crate) async fn nearest_policy_id(
    tx: &mut Transaction<'_, Postgres>,
    resource: ResourceRef,
) -> Result<Option<Uuid>, RepositoryError> {
    let folder_id = match resource {
        ResourceRef::Document(document_id) => {
            sqlx::query_scalar::<_, Uuid>("SELECT folder_id FROM documents WHERE document_id = $1")
                .bind(document_id.as_uuid())
                .fetch_optional(&mut **tx)
                .await
                .map_err(map_statement_error)?
        }
        ResourceRef::Folder(folder_id) => Some(folder_id.as_uuid()),
        ResourceRef::AccessPolicy(policy_id) => {
            let row = sqlx::query(
                "SELECT folder_id, document_id FROM access_policy_bindings WHERE policy_id = $1",
            )
            .bind(policy_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_statement_error)?;
            if let Some(row) = row {
                if let Some(id) = row
                    .try_get::<Option<Uuid>, _>("folder_id")
                    .map_err(map_statement_error)?
                {
                    Some(id)
                } else if let Some(id) = row
                    .try_get::<Option<Uuid>, _>("document_id")
                    .map_err(map_statement_error)?
                {
                    return Box::pin(nearest_policy_id(
                        tx,
                        ResourceRef::Document(document_domain::DocumentId::from_uuid(id)),
                    ))
                    .await;
                } else {
                    return Err(RepositoryError::IntegrityViolation);
                }
            } else {
                return Ok(None);
            }
        }
    };
    let Some(folder_id) = folder_id else {
        return Ok(None);
    };
    let rooted: bool = sqlx::query_scalar(
        "WITH RECURSIVE ancestors AS ( \
             SELECT folder_id, parent_folder_id FROM folders WHERE folder_id = $1 \
             UNION \
             SELECT f.folder_id, f.parent_folder_id FROM folders f \
             JOIN ancestors a ON f.folder_id = a.parent_folder_id \
         ) \
         SELECT EXISTS (SELECT 1 FROM ancestors \
                        WHERE folder_id = $2 AND parent_folder_id IS NULL)",
    )
    .bind(folder_id)
    .bind(crate::SYSTEM_ROOT_FOLDER_ID)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    if !rooted {
        return Ok(None);
    }
    if let ResourceRef::Document(document_id) = resource {
        let document_binding: Option<Uuid> = sqlx::query_scalar(
            "SELECT policy_id FROM access_policy_bindings \
             WHERE document_id = $1 AND mode = 'EXPLICIT'",
        )
        .bind(document_id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_statement_error)?;
        if document_binding.is_some() {
            return Ok(document_binding);
        }
    }
    sqlx::query_scalar(
        "WITH RECURSIVE ancestors AS ( \
             SELECT folder_id, parent_folder_id, 0 AS depth FROM folders WHERE folder_id = $1 \
             UNION ALL \
             SELECT f.folder_id, f.parent_folder_id, a.depth + 1 FROM folders f \
             JOIN ancestors a ON f.folder_id = a.parent_folder_id WHERE a.depth < 1024 \
         ) \
         SELECT b.policy_id FROM ancestors a \
         JOIN access_policy_bindings b ON b.folder_id = a.folder_id AND b.mode = 'EXPLICIT' \
         ORDER BY a.depth LIMIT 1",
    )
    .bind(folder_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)
}

pub(crate) async fn read_grants(
    tx: &mut Transaction<'_, Postgres>,
    policy_id: Uuid,
) -> Result<Vec<PolicyGrant>, RepositoryError> {
    let rows = sqlx::query(
        "SELECT subject_kind, identity_provider, subject_id, action \
         FROM access_policy_grants WHERE policy_id = $1",
    )
    .bind(policy_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    let mut grouped: BTreeMap<PolicySubject, Vec<Action>> = BTreeMap::new();
    for row in rows {
        let kind: String = row.try_get("subject_kind").map_err(map_statement_error)?;
        let issuer: String = row
            .try_get("identity_provider")
            .map_err(map_statement_error)?;
        let id: String = row.try_get("subject_id").map_err(map_statement_error)?;
        let action: String = row.try_get("action").map_err(map_statement_error)?;
        let kind = match kind.as_str() {
            "principal" => PolicySubjectKind::Principal,
            "group" => PolicySubjectKind::Group,
            "role" => PolicySubjectKind::Role,
            _ => return Err(RepositoryError::IntegrityViolation),
        };
        let action = match action.as_str() {
            "read" => Action::Read,
            "read_history" => Action::ReadHistory,
            "write" => Action::Write,
            "publish" => Action::Publish,
            "administer" => Action::Administer,
            _ => return Err(RepositoryError::IntegrityViolation),
        };
        let subject = PolicySubject::new(kind, issuer, id)
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        grouped.entry(subject).or_default().push(action);
    }
    grouped
        .into_iter()
        .map(|(subject, actions)| {
            PolicyGrant::new(subject, actions).map_err(|_| RepositoryError::IntegrityViolation)
        })
        .collect()
}
