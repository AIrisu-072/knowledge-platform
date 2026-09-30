use document_application::{
    CursorBinding, CursorPosition, DocumentRevisionDetail, DocumentRevisionDetailQuery,
    DocumentRevisionPageQuery, DocumentRevisionReadRepository, DocumentRevisionSummary,
    DocumentSort, Page, QueryKind, RepositoryError, VerifiedActorContext, decode_cursor,
    encode_cursor, fingerprint_json, principal_fingerprint,
};
use document_domain::{DocumentId, DocumentVersionId, PrincipalRef};
use serde_json::json;
use sqlx::{Row, postgres::PgRow};

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, lock_access_state},
    document_history::{authorize_history, begin_snapshot},
    error::map_statement_error,
};

fn revision_binding(
    ctx: &VerifiedActorContext,
    document_id: DocumentId,
    access_revision: i64,
) -> Result<CursorBinding, RepositoryError> {
    Ok(CursorBinding {
        kind: QueryKind::DocumentRevisions,
        sort: DocumentSort::RevisionNumberDesc,
        filter_fingerprint: fingerprint_json(&json!({
            "document_id": document_id.as_uuid(),
        }))
        .map_err(|_| RepositoryError::InvalidCursor)?,
        principal_fingerprint: principal_fingerprint(ctx)
            .map_err(|_| RepositoryError::Forbidden)?,
        access_revision,
    })
}

fn revision_summary(row: &PgRow) -> Result<DocumentRevisionSummary, RepositoryError> {
    Ok(DocumentRevisionSummary {
        revision_id: row.try_get("revision_id").map_err(map_statement_error)?,
        document_version_id: DocumentVersionId::from_uuid(
            row.try_get("document_version_id")
                .map_err(map_statement_error)?,
        ),
        major_no: row.try_get("major_no").map_err(map_statement_error)?,
        minor_no: row.try_get("minor_no").map_err(map_statement_error)?,
        metadata_snapshot_status: row
            .try_get("metadata_snapshot_status")
            .map_err(map_statement_error)?,
        source_kind: row.try_get("source_kind").map_err(map_statement_error)?,
        created_at: row.try_get("created_at").map_err(map_statement_error)?,
    })
}

fn actor_from_row(row: &PgRow) -> Result<Option<PrincipalRef>, RepositoryError> {
    let identity_provider: Option<String> = row
        .try_get("actor_identity_provider")
        .map_err(map_statement_error)?;
    let principal_id: Option<String> = row
        .try_get("actor_principal_id")
        .map_err(map_statement_error)?;
    match (identity_provider, principal_id) {
        (Some(provider), Some(principal)) => PrincipalRef::new(provider, principal)
            .map(Some)
            .map_err(|_| RepositoryError::IntegrityViolation),
        (None, None) => Ok(None),
        _ => Err(RepositoryError::IntegrityViolation),
    }
}

impl DocumentRevisionReadRepository for PostgresDocumentRepository {
    async fn list_document_revisions(
        &self,
        ctx: &VerifiedActorContext,
        query: DocumentRevisionPageQuery,
    ) -> Result<Page<DocumentRevisionSummary>, RepositoryError> {
        ctx.ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        let mut tx = begin_snapshot(&self.pool).await?;
        let access_revision = lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        authorize_history(&mut tx, ctx, query.document_id).await?;
        let binding = revision_binding(ctx, query.document_id, access_revision)?;
        let position = query
            .cursor
            .as_deref()
            .map(|token| decode_cursor(token, &binding))
            .transpose()
            .map_err(|error| match error {
                document_application::ApplicationError::CursorStale => RepositoryError::CursorStale,
                _ => RepositoryError::InvalidCursor,
            })?;
        if position
            .as_ref()
            .is_some_and(|position| position.document_id != query.document_id.as_uuid())
        {
            return Err(RepositoryError::CursorStale);
        }
        let revision_key = position
            .as_ref()
            .and_then(|position| position.sort_revision_key);
        let page_size = i64::from(query.page_size.unwrap_or(50));
        let rows = sqlx::query(
            "SELECT revision_id, document_version_id, major_no, minor_no, \
                    metadata_snapshot_status, source_kind, created_at \
             FROM document_revisions \
             WHERE document_id = $1 \
               AND ($2::bigint IS NULL OR (major_no, minor_no) < ($2, $3)) \
             ORDER BY major_no DESC, minor_no DESC LIMIT $4",
        )
        .bind(query.document_id.as_uuid())
        .bind(revision_key.map(|key| key.major))
        .bind(revision_key.map(|key| key.minor))
        .bind(page_size + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        tx.rollback().await.map_err(map_statement_error)?;

        let mut items = rows
            .into_iter()
            .map(|row| revision_summary(&row))
            .collect::<Result<Vec<_>, _>>()?;
        let has_next = items.len() > page_size as usize;
        if has_next {
            items.pop();
        }
        let next_cursor = if has_next {
            let last = items.last().ok_or(RepositoryError::IntegrityViolation)?;
            Some(
                encode_cursor(
                    &binding,
                    &CursorPosition {
                        document_id: query.document_id.as_uuid(),
                        sort_time_micros: None,
                        sort_title: None,
                        sort_revision_key: Some(document_application::RevisionSortKey {
                            major: last.major_no,
                            minor: last.minor_no,
                        }),
                    },
                )
                .map_err(|_| RepositoryError::InvalidCursor)?,
            )
        } else {
            None
        };
        Ok(Page { items, next_cursor })
    }

    async fn get_document_revision(
        &self,
        ctx: &VerifiedActorContext,
        query: DocumentRevisionDetailQuery,
    ) -> Result<DocumentRevisionDetail, RepositoryError> {
        ctx.ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        let mut tx = begin_snapshot(&self.pool).await?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        authorize_history(&mut tx, ctx, query.document_id).await?;
        let row = sqlx::query(
            "SELECT revision_id, document_version_id, major_no, minor_no, \
                    metadata_snapshot, metadata_snapshot_status, source_kind, created_at, \
                    actor_identity_provider, actor_principal_id, reason \
             FROM document_revisions WHERE document_id = $1 AND revision_id = $2",
        )
        .bind(query.document_id.as_uuid())
        .bind(query.revision_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_statement_error)?
        .ok_or(RepositoryError::DocumentRevisionNotFound)?;
        tx.rollback().await.map_err(map_statement_error)?;
        Ok(DocumentRevisionDetail {
            summary: revision_summary(&row)?,
            metadata_snapshot: row
                .try_get("metadata_snapshot")
                .map_err(map_statement_error)?,
            actor: actor_from_row(&row)?,
            reason: row.try_get("reason").map_err(map_statement_error)?,
        })
    }
}
