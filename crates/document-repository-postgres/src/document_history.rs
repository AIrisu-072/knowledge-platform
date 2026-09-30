use document_application::{
    CursorBinding, CursorPosition, DocumentHistoryEntry, DocumentHistoryRepository, DocumentSort,
    HistoryPageQuery, Page, ProvenanceQuality, QueryKind, RepositoryError, VerifiedActorContext,
    VersionDetail, VersionFileSummary, VersionPageQuery, VersionPurpose, VersionRequest,
    VersionSummary, decode_cursor, encode_cursor, fingerprint_json, principal_fingerprint,
};
use document_domain::{Action, DocumentId, DocumentVersionId, PrincipalRef, ResourceRef};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state, verified_subjects_json},
    error::map_statement_error,
};

const HISTORY_SQL: &str = r#"
WITH records AS (
    SELECT 'version_operation'::text AS source_kind,
           'version_operation:' || operation_id::text AS source_key,
           created_at AS occurred_at, actor_identity_provider AS actor_provider,
           actor_principal_id AS actor_id,
           CASE operation_kind
             WHEN 'CREATE' THEN 'document.version.created'
             WHEN 'UPDATE' THEN 'document.version.updated'
             WHEN 'REBASE' THEN 'document.version.rebased'
             WHEN 'WITHDRAW' THEN 'document.version.withdrawn'
             WHEN 'CANCEL_SCHEDULE' THEN 'document.version.publication.cancelled'
           END AS action_code,
           jsonb_build_object('document_version_id', target_document_version_id,
                              'resulting_revision', resulting_document_revision) AS details,
           'operation_ledger'::text AS quality
    FROM document_version_operations WHERE document_id = $1::uuid
    UNION ALL
    SELECT 'publish_operation', 'publish_operation:' || publish_operation_id::text,
           published_at, actor_identity_provider, actor_principal_id,
           'document.version.published',
           jsonb_build_object('document_version_id', target_document_version_id,
                              'resulting_revision', resulting_document_revision),
           'operation_ledger'
    FROM document_publish_operations WHERE document_id = $1::uuid
    UNION ALL
    SELECT 'schedule', 'schedule:' || publish_operation_id::text,
           created_at, actor_identity_provider, actor_principal_id,
           'document.version.publication.scheduled',
           jsonb_build_object('document_version_id', target_document_version_id),
           'operation_ledger'
    FROM document_publish_schedules WHERE document_id = $1::uuid
    UNION ALL
    SELECT 'schedule_terminal', 'schedule_terminal:' || publish_operation_id::text,
           terminal_at, terminal_executor_identity_provider, terminal_executor_principal_id,
           'document.version.publication.terminal',
           jsonb_build_object('document_version_id', target_document_version_id),
           CASE WHEN terminal_at IS NULL THEN 'legacy_unknown' ELSE 'operation_ledger' END
    FROM document_publish_schedules
    WHERE document_id = $1::uuid AND status = 'TERMINAL'
    UNION ALL
    SELECT 'publication_end', 'publication_end:' || operation_id::text,
           ended_at, actor_identity_provider, actor_principal_id,
           'document.publication.ended',
           jsonb_build_object('former_current_version_id', former_current_version_id,
                              'resulting_revision', resulting_document_revision),
           'operation_ledger'
    FROM document_publication_end_operations WHERE document_id = $1::uuid
    UNION ALL
    SELECT 'management', 'management:' || operation_id::text,
           occurred_at, actor_identity_provider, actor_principal_id,
           CASE operation_kind
             WHEN 'update_document_metadata' THEN 'document.metadata.changed'
             WHEN 'move_document' THEN 'document.moved'
           END,
           jsonb_build_object('changed', changed,
                              'resulting_revision', resulting_revision),
           'operation_ledger'
    FROM document_management_operations
    WHERE resource_type = 'Document' AND resource_id = $1::uuid
      AND operation_kind IN ('update_document_metadata','move_document')
    UNION ALL
    SELECT 'version_fallback', 'version_created:' || document_version_id::text,
           created_at, created_by_identity_provider, created_by_principal_id,
           'document.version.created',
           jsonb_build_object('document_version_id', document_version_id,
                              'version_no', version_no), 'version_fallback'
    FROM document_versions v
    WHERE v.document_id = $1::uuid
      AND NOT EXISTS (
        SELECT 1 FROM document_version_operations op
        WHERE op.target_document_version_id = v.document_version_id
          AND op.operation_kind = 'CREATE')
    UNION ALL
    SELECT 'version_fallback', 'version_published:' || document_version_id::text,
           published_at, NULL::text, NULL::text,
           'document.version.published',
           jsonb_build_object('document_version_id', document_version_id),
           'version_fallback'
    FROM document_versions v
    WHERE v.document_id = $1::uuid AND v.published_at IS NOT NULL
      AND NOT EXISTS (
        SELECT 1 FROM document_publish_operations op
        WHERE op.target_document_version_id = v.document_version_id)
    UNION ALL
    SELECT 'version_fallback', 'version_withdrawn:' || document_version_id::text,
           withdrawn_at, NULL::text, NULL::text,
           'document.version.withdrawn',
           jsonb_build_object('document_version_id', document_version_id),
           'version_fallback'
    FROM document_versions v
    WHERE v.document_id = $1::uuid AND v.withdrawn_at IS NOT NULL
      AND NOT EXISTS (
        SELECT 1 FROM document_version_operations op
        WHERE op.target_document_version_id = v.document_version_id
          AND op.operation_kind = 'WITHDRAW')
)
SELECT * FROM records
ORDER BY occurred_at DESC NULLS LAST, source_key ASC
LIMIT $2::bigint OFFSET $3::bigint
"#;

pub(crate) async fn begin_snapshot(
    pool: &sqlx::PgPool,
) -> Result<Transaction<'static, Postgres>, RepositoryError> {
    let mut tx = pool.begin().await.map_err(map_statement_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .map_err(map_statement_error)?;
    Ok(tx)
}

async fn authorize_history(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    document_id: DocumentId,
) -> Result<(), RepositoryError> {
    let resource = ResourceRef::Document(document_id);
    match authorize_in_tx(tx, ctx, &[(resource, vec![Action::Read])]).await {
        Err(RepositoryError::Forbidden) => return Err(RepositoryError::DocumentNotFound),
        other => other?,
    }
    authorize_in_tx(
        tx,
        ctx,
        &[(resource, vec![Action::Read, Action::ReadHistory])],
    )
    .await
}

pub(crate) async fn authorize_version_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    request: VersionRequest,
) -> Result<PgRow, RepositoryError> {
    let resource = ResourceRef::Document(request.document_id);
    match authorize_in_tx(tx, ctx, &[(resource, vec![Action::Read])]).await {
        Err(RepositoryError::Forbidden) => return Err(RepositoryError::DocumentNotFound),
        other => other?,
    }
    let row = sqlx::query(
        "SELECT v.*, d.current_version_id, \
                EXISTS (SELECT 1 FROM document_publication_end_operations ended \
                        WHERE ended.document_id = d.document_id) AS ended \
         FROM documents d JOIN document_versions v ON v.document_id = d.document_id \
         WHERE d.document_id = $1 AND v.document_version_id = $2",
    )
    .bind(request.document_id.as_uuid())
    .bind(request.document_version_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?
    .ok_or(RepositoryError::DocumentVersionNotFound)?;
    let current: Option<Uuid> = row
        .try_get("current_version_id")
        .map_err(map_statement_error)?;
    let state: String = row
        .try_get("lifecycle_state")
        .map_err(map_statement_error)?;
    let ended: bool = row.try_get("ended").map_err(map_statement_error)?;
    match request.purpose {
        VersionPurpose::Published => {
            if current != Some(request.document_version_id.as_uuid())
                || state != "PUBLISHED"
                || ended
            {
                return Err(RepositoryError::DocumentVersionNotFound);
            }
        }
        VersionPurpose::Authoring => {
            if state != "WORKING" || ended {
                return Err(RepositoryError::DocumentVersionNotFound);
            }
            match authorize_in_tx(tx, ctx, &[(resource, vec![Action::Read, Action::Write])]).await {
                Err(RepositoryError::Forbidden) => {
                    return Err(RepositoryError::DocumentVersionNotFound);
                }
                other => other?,
            }
        }
        VersionPurpose::History => {
            authorize_history(tx, ctx, request.document_id).await?;
            if state == "WORKING" {
                match authorize_in_tx(tx, ctx, &[(resource, vec![Action::Read, Action::Write])])
                    .await
                {
                    Err(RepositoryError::Forbidden) => {
                        return Err(RepositoryError::DocumentVersionNotFound);
                    }
                    other => other?,
                }
            }
        }
    }
    Ok(row)
}

fn page_binding(
    ctx: &VerifiedActorContext,
    document_id: DocumentId,
    kind: QueryKind,
    revision: i64,
    purpose: &str,
) -> Result<CursorBinding, RepositoryError> {
    Ok(CursorBinding {
        kind,
        sort: DocumentSort::CreatedAtDesc,
        filter_fingerprint: fingerprint_json(&json!({
            "document_id": document_id.as_uuid(),
            "purpose": purpose,
        }))
        .map_err(|_| RepositoryError::InvalidCursor)?,
        principal_fingerprint: principal_fingerprint(ctx)
            .map_err(|_| RepositoryError::Forbidden)?,
        access_revision: revision,
    })
}

fn page_offset(
    cursor: Option<&str>,
    binding: &CursorBinding,
    document_id: DocumentId,
) -> Result<i64, RepositoryError> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let position = decode_cursor(cursor, binding).map_err(|error| match error {
        document_application::ApplicationError::CursorStale => RepositoryError::CursorStale,
        _ => RepositoryError::InvalidCursor,
    })?;
    if position.document_id != document_id.as_uuid() {
        return Err(RepositoryError::CursorStale);
    }
    let offset = position
        .sort_time_micros
        .ok_or(RepositoryError::InvalidCursor)?;
    if offset < 0 {
        return Err(RepositoryError::InvalidCursor);
    }
    Ok(offset)
}

fn next_cursor(
    binding: &CursorBinding,
    document_id: DocumentId,
    next_offset: i64,
) -> Result<String, RepositoryError> {
    encode_cursor(
        binding,
        &CursorPosition {
            document_id: document_id.as_uuid(),
            sort_time_micros: Some(next_offset),
            sort_title: None,
        },
    )
    .map_err(|_| RepositoryError::InvalidCursor)
}

fn decode_version(row: &PgRow) -> Result<VersionSummary, RepositoryError> {
    let id: Uuid = row
        .try_get("document_version_id")
        .map_err(map_statement_error)?;
    let current: Option<Uuid> = row
        .try_get("current_version_id")
        .map_err(map_statement_error)?;
    Ok(VersionSummary {
        document_version_id: DocumentVersionId::from_uuid(id),
        version_no: row.try_get("version_no").map_err(map_statement_error)?,
        lifecycle_state: row
            .try_get("lifecycle_state")
            .map_err(map_statement_error)?,
        is_current: current == Some(id),
        created_at: row.try_get("created_at").map_err(map_statement_error)?,
        published_at: row.try_get("published_at").map_err(map_statement_error)?,
        withdrawn_at: row.try_get("withdrawn_at").map_err(map_statement_error)?,
        first_read_at: row.try_get("first_read_at").map_err(map_statement_error)?,
    })
}

pub(crate) fn safe_display_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let value = base
        .chars()
        .filter(|ch| !ch.is_control())
        .take(255)
        .collect::<String>();
    if value.trim().is_empty() || value == "." || value == ".." {
        "document.bin".into()
    } else {
        value
    }
}

impl DocumentHistoryRepository for PostgresDocumentRepository {
    async fn list_document_versions(
        &self,
        ctx: &VerifiedActorContext,
        query: VersionPageQuery,
    ) -> Result<Page<VersionSummary>, RepositoryError> {
        let mut tx = begin_snapshot(&self.pool).await?;
        let revision = lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        let resource = ResourceRef::Document(query.document_id);
        match query.purpose {
            VersionPurpose::Published => {
                match authorize_in_tx(&mut tx, ctx, &[(resource, vec![Action::Read])]).await {
                    Err(RepositoryError::Forbidden) => {
                        return Err(RepositoryError::DocumentNotFound);
                    }
                    other => other?,
                }
            }
            VersionPurpose::Authoring => match authorize_in_tx(
                &mut tx,
                ctx,
                &[(resource, vec![Action::Read, Action::Write])],
            )
            .await
            {
                Err(RepositoryError::Forbidden) => return Err(RepositoryError::DocumentNotFound),
                other => other?,
            },
            VersionPurpose::History => authorize_history(&mut tx, ctx, query.document_id).await?,
        }
        let purpose = match query.purpose {
            VersionPurpose::Published => "published",
            VersionPurpose::Authoring => "authoring",
            VersionPurpose::History => "history",
        };
        let binding = page_binding(
            ctx,
            query.document_id,
            QueryKind::Versions,
            revision,
            purpose,
        )?;
        let offset = page_offset(query.cursor.as_deref(), &binding, query.document_id)?;
        let size = i64::from(query.page_size.unwrap_or(50));
        let rows = sqlx::query(
            "SELECT v.document_version_id,v.version_no,v.lifecycle_state,v.created_at, \
                    v.published_at,v.withdrawn_at,d.current_version_id,rs.first_read_at \
             FROM documents d JOIN document_versions v ON v.document_id = d.document_id \
             LEFT JOIN document_read_states rs \
               ON rs.document_version_id = v.document_version_id \
              AND rs.identity_provider = $3 AND rs.principal_id = $4 \
             WHERE d.document_id = $1 AND ( \
                 ($5 = 'published' AND v.document_version_id = d.current_version_id \
                    AND v.lifecycle_state = 'PUBLISHED' \
                    AND NOT EXISTS (SELECT 1 FROM document_publication_end_operations ended \
                                    WHERE ended.document_id = d.document_id)) \
                 OR ($5 = 'authoring' AND v.lifecycle_state = 'WORKING' \
                    AND NOT EXISTS (SELECT 1 FROM document_publication_end_operations ended \
                                    WHERE ended.document_id = d.document_id)) \
                 OR ($5 = 'history' AND (v.lifecycle_state <> 'WORKING' \
                    OR dmb_allows_document(d.document_id,$2,ARRAY['read','write']::text[])))) \
             ORDER BY v.version_no DESC,v.document_version_id DESC LIMIT $6 OFFSET $7",
        )
        .bind(query.document_id.as_uuid())
        .bind(verified_subjects_json(ctx))
        .bind(ctx.principal().identity_provider())
        .bind(ctx.principal().principal_id())
        .bind(purpose)
        .bind(size + 1)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        tx.rollback().await.map_err(map_statement_error)?;
        let mut items = rows
            .iter()
            .map(decode_version)
            .collect::<Result<Vec<_>, _>>()?;
        let has_next = items.len() > size as usize;
        if has_next {
            items.pop();
        }
        Ok(Page {
            items,
            next_cursor: if has_next {
                Some(next_cursor(
                    &binding,
                    query.document_id,
                    offset
                        .checked_add(size)
                        .ok_or(RepositoryError::InvalidCursor)?,
                )?)
            } else {
                None
            },
        })
    }

    async fn list_document_history(
        &self,
        ctx: &VerifiedActorContext,
        query: HistoryPageQuery,
    ) -> Result<Page<DocumentHistoryEntry>, RepositoryError> {
        let mut tx = begin_snapshot(&self.pool).await?;
        let revision = lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        authorize_history(&mut tx, ctx, query.document_id).await?;
        let binding = page_binding(
            ctx,
            query.document_id,
            QueryKind::DocumentHistory,
            revision,
            "history",
        )?;
        let offset = page_offset(query.cursor.as_deref(), &binding, query.document_id)?;
        let size = i64::from(query.page_size.unwrap_or(50));
        let rows = sqlx::query(HISTORY_SQL)
            .bind(query.document_id.as_uuid())
            .bind(size + 1)
            .bind(offset)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_statement_error)?;
        tx.rollback().await.map_err(map_statement_error)?;
        let mut items = rows
            .into_iter()
            .map(|row| {
                let provider: Option<String> =
                    row.try_get("actor_provider").map_err(map_statement_error)?;
                let id: Option<String> = row.try_get("actor_id").map_err(map_statement_error)?;
                let actor = match (provider, id) {
                    (Some(provider), Some(id)) => PrincipalRef::new(provider, id).ok(),
                    _ => None,
                };
                let quality: String = row.try_get("quality").map_err(map_statement_error)?;
                let provenance_quality = match quality.as_str() {
                    "operation_ledger" => ProvenanceQuality::OperationLedger,
                    "version_fallback" => ProvenanceQuality::VersionFallback,
                    "legacy_unknown" => ProvenanceQuality::LegacyUnknown,
                    _ => return Err(RepositoryError::IntegrityViolation),
                };
                Ok(DocumentHistoryEntry {
                    source_kind: row.try_get("source_kind").map_err(map_statement_error)?,
                    source_key: row.try_get("source_key").map_err(map_statement_error)?,
                    occurred_at: row.try_get("occurred_at").map_err(map_statement_error)?,
                    actor,
                    action_code: row.try_get("action_code").map_err(map_statement_error)?,
                    details: row.try_get("details").map_err(map_statement_error)?,
                    provenance_quality,
                })
            })
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        let has_next = items.len() > size as usize;
        if has_next {
            items.pop();
        }
        Ok(Page {
            items,
            next_cursor: if has_next {
                Some(next_cursor(
                    &binding,
                    query.document_id,
                    offset
                        .checked_add(size)
                        .ok_or(RepositoryError::InvalidCursor)?,
                )?)
            } else {
                None
            },
        })
    }

    async fn get_document_version(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> Result<VersionDetail, RepositoryError> {
        let mut tx = begin_snapshot(&self.pool).await?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        let row = authorize_version_in_tx(&mut tx, ctx, request).await?;
        let first_read_at: Option<OffsetDateTime> = sqlx::query_scalar(
            "SELECT first_read_at FROM document_read_states WHERE identity_provider = $1 \
             AND principal_id = $2 AND document_version_id = $3",
        )
        .bind(ctx.principal().identity_provider())
        .bind(ctx.principal().principal_id())
        .bind(request.document_version_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        let version_id: Uuid = row
            .try_get("document_version_id")
            .map_err(map_statement_error)?;
        let current: Option<Uuid> = row
            .try_get("current_version_id")
            .map_err(map_statement_error)?;
        let detail = VersionDetail {
            summary: VersionSummary {
                document_version_id: DocumentVersionId::from_uuid(version_id),
                version_no: row.try_get("version_no").map_err(map_statement_error)?,
                lifecycle_state: row
                    .try_get("lifecycle_state")
                    .map_err(map_statement_error)?,
                is_current: current == Some(version_id),
                created_at: row.try_get("created_at").map_err(map_statement_error)?,
                published_at: row.try_get("published_at").map_err(map_statement_error)?,
                withdrawn_at: row.try_get("withdrawn_at").map_err(map_statement_error)?,
                first_read_at,
            },
            title: row.try_get("title").map_err(map_statement_error)?,
            metadata: row.try_get("metadata").map_err(map_statement_error)?,
        };
        tx.rollback().await.map_err(map_statement_error)?;
        Ok(detail)
    }

    async fn list_version_files(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> Result<Vec<VersionFileSummary>, RepositoryError> {
        let mut tx = begin_snapshot(&self.pool).await?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        authorize_version_in_tx(&mut tx, ctx, request).await?;
        let rows = sqlx::query(
            "SELECT item.content_item_id,rep.content_representation_id,item.logical_path, \
                    item.ordinal,rep.role,rep.original_filename,file.media_type,file.size_bytes \
             FROM content_items item \
             JOIN content_representations rep ON rep.content_item_id = item.content_item_id \
             JOIN file_objects file ON file.file_id = rep.file_id \
             WHERE item.document_version_id = $1 \
             ORDER BY item.ordinal,item.logical_path,rep.role,rep.content_representation_id",
        )
        .bind(request.document_version_id.as_uuid())
        .fetch_all(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        tx.rollback().await.map_err(map_statement_error)?;
        rows.into_iter()
            .map(|row| {
                let name: String = row
                    .try_get("original_filename")
                    .map_err(map_statement_error)?;
                Ok(VersionFileSummary {
                    content_item_id: row
                        .try_get("content_item_id")
                        .map_err(map_statement_error)?,
                    representation_id: row
                        .try_get("content_representation_id")
                        .map_err(map_statement_error)?,
                    logical_path: row.try_get("logical_path").map_err(map_statement_error)?,
                    ordinal: row.try_get("ordinal").map_err(map_statement_error)?,
                    role: row.try_get("role").map_err(map_statement_error)?,
                    safe_display_name: safe_display_name(&name),
                    media_type: row.try_get("media_type").map_err(map_statement_error)?,
                    size_bytes: row.try_get("size_bytes").map_err(map_statement_error)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn display_name_never_contains_a_path_or_control_character() {
        assert_eq!(
            super::safe_display_name("../../hidden\\report\n.txt"),
            "report.txt"
        );
        assert_eq!(super::safe_display_name(".."), "document.bin");
        assert_eq!(super::safe_display_name("\n"), "document.bin");
    }
}
