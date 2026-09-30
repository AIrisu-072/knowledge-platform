use document_application::{
    AuthoringDocumentSummary, AuthoringQuery, CursorBinding, CursorPosition, DocumentListFilter,
    DocumentQueryRepository, DocumentSort, FolderPageQuery, FolderSummary, HistoryDocumentSummary,
    HistoryQuery, Page, PublishedDocumentSummary, PublishedQuery, QueryKind, RepositoryError,
    RootFolderSummary, VerifiedActorContext, decode_cursor, encode_cursor, fingerprint_json,
    principal_fingerprint,
};
use document_domain::{Action, DocumentId, DocumentVersionId, FolderId, ResourceRef};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state, verified_subjects_json},
    error::map_statement_error,
};

const DOCUMENT_QUERY_SQL: &str = r#"
WITH RECURSIVE selected_folders(folder_id, depth) AS (
    SELECT f.folder_id, 0 FROM folders f WHERE f.folder_id = $4::uuid
    UNION ALL
    SELECT f.folder_id, selected_folders.depth + 1
    FROM folders f JOIN selected_folders ON f.parent_folder_id = selected_folders.folder_id
    WHERE selected_folders.depth < 1024
), authorized AS (
    SELECT d.document_id, d.folder_id, d.current_version_id, d.revision,
           d.metadata, d.created_at, v.document_version_id, v.lifecycle_state,
           v.title, normalize(v.title, NFC) COLLATE "C" AS title_key,
           v.published_at, rs.first_read_at,
           EXISTS (SELECT 1 FROM document_publication_end_operations ended
                   WHERE ended.document_id = d.document_id) AS ended,
           CASE WHEN dmb_allows_folder(d.folder_id, $2::jsonb, ARRAY['read']::text[])
                THEN d.folder_id ELSE NULL END AS visible_folder_id,
           CASE WHEN dmb_allows_folder(d.folder_id, $2::jsonb, ARRAY['read']::text[])
                THEN f.name ELSE NULL END AS visible_folder_name
    FROM documents d
    JOIN folders f ON f.folder_id = d.folder_id
    JOIN LATERAL (
        SELECT selected.* FROM document_versions selected
        WHERE selected.document_id = d.document_id
          AND (
              ($1::text = 'published'
               AND selected.document_version_id = d.current_version_id
               AND selected.lifecycle_state = 'PUBLISHED')
              OR ($1::text = 'authoring'
                  AND (selected.lifecycle_state = 'WORKING'
                       OR selected.document_version_id = d.current_version_id))
              OR ($1::text = 'history'
                  AND (selected.lifecycle_state <> 'WORKING'
                       OR dmb_allows_document(
                           d.document_id, $2::jsonb,
                           ARRAY['read','write']::text[])))
          )
        ORDER BY CASE WHEN selected.lifecycle_state = 'WORKING' THEN 1 ELSE 0 END DESC,
                 selected.version_no DESC
        LIMIT 1
    ) v ON TRUE
    LEFT JOIN document_read_states rs
      ON rs.document_version_id = v.document_version_id
     AND rs.identity_provider = $17::text AND rs.principal_id = $18::text
    WHERE dmb_allows_document(d.document_id, $2::jsonb, $3::text[])
      AND ($1::text = 'history' OR NOT EXISTS (
          SELECT 1 FROM document_publication_end_operations ended
          WHERE ended.document_id = d.document_id))
      AND ($4::uuid IS NULL
           OR ($5::boolean AND d.folder_id IN (SELECT folder_id FROM selected_folders))
           OR (NOT $5::boolean AND d.folder_id = $4::uuid))
)
SELECT authorized.* FROM authorized
WHERE ($6::text IS NULL OR strpos(title_key, $6::text) > 0)
  AND ($20::uuid IS NULL OR document_id = $20::uuid)
  AND ($7::text IS NULL OR metadata->>'document_type' = $7::text)
  AND ($8::text IS NULL OR metadata->>'owning_department' = $8::text)
  AND ($9::text IS NULL OR metadata->>'category' = $9::text)
  AND ($10::timestamptz IS NULL OR created_at >= $10::timestamptz)
  AND ($11::timestamptz IS NULL OR created_at < $11::timestamptz)
  AND (NOT $12::boolean OR first_read_at IS NULL)
  AND ($14::uuid IS NULL
       OR ($13::text = 'created_at_desc'
           AND (created_at, document_id) < ($15::timestamptz, $14::uuid))
       OR ($13::text = 'published_at_desc'
           AND (published_at, document_id) < ($15::timestamptz, $14::uuid))
       OR ($13::text = 'title_asc'
           AND (title_key, document_id) > ($16::text COLLATE "C", $14::uuid)))
ORDER BY CASE WHEN $13::text = 'created_at_desc' THEN created_at END DESC,
         CASE WHEN $13::text = 'published_at_desc' THEN published_at END DESC,
         CASE WHEN $13::text = 'title_asc' THEN title_key END ASC,
         CASE WHEN $13::text = 'title_asc' THEN document_id END ASC,
         CASE WHEN $13::text <> 'title_asc' THEN document_id END DESC
LIMIT $19::bigint
"#;

const FOLDER_QUERY_SQL: &str = r#"
SELECT folder_id, parent_folder_id, name, revision
FROM folders
WHERE parent_folder_id = $1::uuid AND status = 'ACTIVE'
  AND dmb_allows_folder(folder_id, $2::jsonb, ARRAY['read']::text[])
  AND ($3::uuid IS NULL OR ((normalize(name, NFC) COLLATE "C"), folder_id)
                         > ($4::text COLLATE "C", $3::uuid))
ORDER BY normalize(name, NFC) COLLATE "C" ASC, folder_id ASC
LIMIT $5::bigint
"#;

#[derive(Debug)]
struct QueryRow {
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
    title: String,
    title_key: String,
    lifecycle_state: String,
    current_version_id: Option<DocumentVersionId>,
    folder_id: Option<FolderId>,
    folder_name: Option<String>,
    metadata: Value,
    created_at: OffsetDateTime,
    published_at: Option<OffsetDateTime>,
    first_read_at: Option<OffsetDateTime>,
    revision: i64,
    ended: bool,
}

impl QueryRow {
    fn decode(row: PgRow) -> Result<Self, RepositoryError> {
        Ok(Self {
            document_id: DocumentId::from_uuid(
                row.try_get("document_id").map_err(map_statement_error)?,
            ),
            document_version_id: DocumentVersionId::from_uuid(
                row.try_get("document_version_id")
                    .map_err(map_statement_error)?,
            ),
            title: row.try_get("title").map_err(map_statement_error)?,
            title_key: row.try_get("title_key").map_err(map_statement_error)?,
            lifecycle_state: row
                .try_get("lifecycle_state")
                .map_err(map_statement_error)?,
            current_version_id: row
                .try_get::<Option<Uuid>, _>("current_version_id")
                .map_err(map_statement_error)?
                .map(DocumentVersionId::from_uuid),
            folder_id: row
                .try_get::<Option<Uuid>, _>("visible_folder_id")
                .map_err(map_statement_error)?
                .map(FolderId::from_uuid),
            folder_name: row
                .try_get("visible_folder_name")
                .map_err(map_statement_error)?,
            metadata: row.try_get("metadata").map_err(map_statement_error)?,
            created_at: row.try_get("created_at").map_err(map_statement_error)?,
            published_at: row.try_get("published_at").map_err(map_statement_error)?,
            first_read_at: row.try_get("first_read_at").map_err(map_statement_error)?,
            revision: row.try_get("revision").map_err(map_statement_error)?,
            ended: row.try_get("ended").map_err(map_statement_error)?,
        })
    }

    fn position(&self, sort: DocumentSort) -> Result<CursorPosition, RepositoryError> {
        let (sort_time_micros, sort_title) = match sort {
            DocumentSort::CreatedAtDesc => (Some(to_micros(self.created_at)?), None),
            DocumentSort::PublishedAtDesc => (
                Some(to_micros(
                    self.published_at
                        .ok_or(RepositoryError::IntegrityViolation)?,
                )?),
                None,
            ),
            DocumentSort::TitleAsc => (None, Some(self.title_key.clone())),
        };
        Ok(CursorPosition {
            document_id: self.document_id.as_uuid(),
            sort_time_micros,
            sort_title,
        })
    }
}

fn to_micros(date: OffsetDateTime) -> Result<i64, RepositoryError> {
    i64::try_from(date.unix_timestamp_nanos() / 1_000)
        .map_err(|_| RepositoryError::IntegrityViolation)
}

fn cursor_error(error: document_application::ApplicationError) -> RepositoryError {
    match error {
        document_application::ApplicationError::CursorStale => RepositoryError::CursorStale,
        _ => RepositoryError::InvalidCursor,
    }
}

fn cursor_time(position: &CursorPosition) -> Result<Option<OffsetDateTime>, RepositoryError> {
    position
        .sort_time_micros
        .map(|micros| {
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(micros) * 1_000)
                .map_err(|_| RepositoryError::InvalidCursor)
        })
        .transpose()
}

fn binding(
    ctx: &VerifiedActorContext,
    kind: QueryKind,
    sort: DocumentSort,
    filter: Value,
    access_revision: i64,
) -> Result<CursorBinding, RepositoryError> {
    Ok(CursorBinding {
        kind,
        sort,
        filter_fingerprint: fingerprint_json(&filter).map_err(cursor_error)?,
        principal_fingerprint: principal_fingerprint(ctx).map_err(cursor_error)?,
        access_revision,
    })
}

async fn ensure_folder_visible(
    tx: &mut Transaction<'_, Postgres>,
    folder_id: Uuid,
    subjects: &Value,
) -> Result<(), RepositoryError> {
    let visible: bool =
        sqlx::query_scalar("SELECT dmb_allows_folder($1::uuid, $2::jsonb, ARRAY['read']::text[])")
            .bind(folder_id)
            .bind(subjects)
            .fetch_one(&mut **tx)
            .await
            .map_err(map_statement_error)?;
    if visible {
        Ok(())
    } else {
        Err(RepositoryError::FolderNotFound)
    }
}

enum Scope {
    Published,
    Authoring,
    History,
}

struct DocumentPageRequest {
    scope: Scope,
    filter: DocumentListFilter,
    sort: DocumentSort,
    page_size: Option<u16>,
    cursor: Option<String>,
    unread_only: bool,
}

impl Scope {
    const fn as_sql(&self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Authoring => "authoring",
            Self::History => "history",
        }
    }

    const fn kind(&self) -> QueryKind {
        match self {
            Self::Published => QueryKind::Published,
            Self::Authoring => QueryKind::Authoring,
            Self::History => QueryKind::History,
        }
    }

    fn required_actions(&self) -> Vec<String> {
        match self {
            Self::Published => vec!["read".into()],
            Self::Authoring => vec!["read".into(), "write".into()],
            Self::History => vec!["read".into(), "read_history".into()],
        }
    }
}

impl PostgresDocumentRepository {
    async fn document_page(
        &self,
        ctx: &VerifiedActorContext,
        request: DocumentPageRequest,
    ) -> Result<Page<QueryRow>, RepositoryError> {
        let DocumentPageRequest {
            scope,
            filter,
            sort,
            page_size,
            cursor,
            unread_only,
        } = request;
        ctx.ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        let subjects = verified_subjects_json(ctx);
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let access_revision = lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        if let Some(folder_id) = filter.folder_id {
            ensure_folder_visible(&mut tx, folder_id.as_uuid(), &subjects).await?;
        }
        let binding = binding(
            ctx,
            scope.kind(),
            sort,
            json!({"filter": filter.fingerprint_value(), "unread_only": unread_only}),
            access_revision,
        )?;
        let position = cursor
            .as_deref()
            .map(|raw| decode_cursor(raw, &binding).map_err(cursor_error))
            .transpose()?;
        let cursor_id = position.as_ref().map(|position| position.document_id);
        let cursor_time = position.as_ref().map(cursor_time).transpose()?.flatten();
        let cursor_title = position
            .as_ref()
            .and_then(|position| position.sort_title.as_deref());
        let page_size = i64::from(page_size.unwrap_or(50));
        let rows = sqlx::query(DOCUMENT_QUERY_SQL)
            .bind(scope.as_sql())
            .bind(subjects)
            .bind(scope.required_actions())
            .bind(filter.folder_id.map(|id| id.as_uuid()))
            .bind(filter.include_descendants)
            .bind(filter.title_contains.as_deref())
            .bind(filter.document_type.as_deref())
            .bind(filter.owning_department.as_deref())
            .bind(filter.category.as_deref())
            .bind(filter.created_from)
            .bind(filter.created_before)
            .bind(unread_only)
            .bind(sort.as_sql())
            .bind(cursor_id)
            .bind(cursor_time)
            .bind(cursor_title)
            .bind(ctx.principal().identity_provider())
            .bind(ctx.principal().principal_id())
            .bind(page_size + 1)
            .bind(filter.exact_document_id.map(|id| id.as_uuid()))
            .fetch_all(&mut *tx)
            .await
            .map_err(map_statement_error)?;
        tx.rollback().await.map_err(map_statement_error)?;
        let mut items = rows
            .into_iter()
            .map(QueryRow::decode)
            .collect::<Result<Vec<_>, _>>()?;
        let has_next = items.len() > page_size as usize;
        if has_next {
            items.pop();
        }
        let next_cursor = if has_next {
            Some(
                encode_cursor(
                    &binding,
                    &items
                        .last()
                        .ok_or(RepositoryError::IntegrityViolation)?
                        .position(sort)?,
                )
                .map_err(cursor_error)?,
            )
        } else {
            None
        };
        Ok(Page { items, next_cursor })
    }
}

impl DocumentQueryRepository for PostgresDocumentRepository {
    async fn get_root_folder(
        &self,
        ctx: &VerifiedActorContext,
    ) -> Result<RootFolderSummary, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let result: Result<RootFolderSummary, RepositoryError> = async {
            lock_access_state(&mut tx, AccessLockMode::Shared).await?;
            let root_id = FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID);
            match authorize_in_tx(
                &mut tx,
                ctx,
                &[(ResourceRef::Folder(root_id), vec![Action::Read])],
            )
            .await
            {
                Ok(()) => {}
                Err(RepositoryError::Forbidden) => return Err(RepositoryError::FolderNotFound),
                Err(error) => return Err(error),
            }
            let row = sqlx::query(
                "SELECT name,revision FROM folders WHERE folder_id = $1 AND parent_folder_id IS NULL AND status = 'ACTIVE'",
            )
            .bind(SYSTEM_ROOT_FOLDER_ID)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?
            .ok_or(RepositoryError::FolderNotFound)?;
            Ok(RootFolderSummary {
                folder_id: root_id,
                name: row.try_get("name").map_err(map_statement_error)?,
                revision: row.try_get("revision").map_err(map_statement_error)?,
            })
        }
        .await;
        tx.rollback().await.map_err(map_statement_error)?;
        result
    }

    async fn list_published_documents(
        &self,
        ctx: &VerifiedActorContext,
        query: PublishedQuery,
    ) -> Result<Page<PublishedDocumentSummary>, RepositoryError> {
        let page = self
            .document_page(
                ctx,
                DocumentPageRequest {
                    scope: Scope::Published,
                    filter: query.filter,
                    sort: query.sort,
                    page_size: query.page_size,
                    cursor: query.cursor,
                    unread_only: query.unread_only,
                },
            )
            .await?;
        let items = page
            .items
            .into_iter()
            .map(|row| {
                Ok(PublishedDocumentSummary {
                    document_id: row.document_id,
                    document_version_id: row.document_version_id,
                    title: row.title,
                    folder_id: row.folder_id,
                    folder_name: row.folder_name,
                    document_metadata: row.metadata,
                    created_at: row.created_at,
                    published_at: row
                        .published_at
                        .ok_or(RepositoryError::IntegrityViolation)?,
                    first_read_at: row.first_read_at,
                    document_revision: row.revision,
                })
            })
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        Ok(Page {
            items,
            next_cursor: page.next_cursor,
        })
    }

    async fn list_authoring_documents(
        &self,
        ctx: &VerifiedActorContext,
        query: AuthoringQuery,
    ) -> Result<Page<AuthoringDocumentSummary>, RepositoryError> {
        let page = self
            .document_page(
                ctx,
                DocumentPageRequest {
                    scope: Scope::Authoring,
                    filter: query.filter,
                    sort: query.sort,
                    page_size: query.page_size,
                    cursor: query.cursor,
                    unread_only: false,
                },
            )
            .await?;
        Ok(Page {
            items: page
                .items
                .into_iter()
                .map(|row| AuthoringDocumentSummary {
                    document_id: row.document_id,
                    document_version_id: row.document_version_id,
                    title: row.title,
                    lifecycle_state: row.lifecycle_state,
                    current_version_id: row.current_version_id,
                    folder_id: row.folder_id,
                    folder_name: row.folder_name,
                    document_metadata: row.metadata,
                    created_at: row.created_at,
                    document_revision: row.revision,
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    async fn list_history_documents(
        &self,
        ctx: &VerifiedActorContext,
        query: HistoryQuery,
    ) -> Result<Page<HistoryDocumentSummary>, RepositoryError> {
        let page = self
            .document_page(
                ctx,
                DocumentPageRequest {
                    scope: Scope::History,
                    filter: query.filter,
                    sort: query.sort,
                    page_size: query.page_size,
                    cursor: query.cursor,
                    unread_only: false,
                },
            )
            .await?;
        Ok(Page {
            items: page
                .items
                .into_iter()
                .map(|row| HistoryDocumentSummary {
                    document_id: row.document_id,
                    document_version_id: row.document_version_id,
                    title: row.title,
                    lifecycle_state: row.lifecycle_state,
                    ended: row.ended,
                    folder_id: row.folder_id,
                    folder_name: row.folder_name,
                    document_metadata: row.metadata,
                    created_at: row.created_at,
                    document_revision: row.revision,
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    async fn list_child_folders(
        &self,
        ctx: &VerifiedActorContext,
        query: FolderPageQuery,
    ) -> Result<Page<FolderSummary>, RepositoryError> {
        ctx.ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        let subjects = verified_subjects_json(ctx);
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        let access_revision = lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        ensure_folder_visible(&mut tx, query.parent_folder_id.as_uuid(), &subjects).await?;
        let binding = binding(
            ctx,
            QueryKind::Folders,
            DocumentSort::TitleAsc,
            json!({"parent_folder_id": query.parent_folder_id.as_uuid()}),
            access_revision,
        )?;
        let position = query
            .cursor
            .as_deref()
            .map(|raw| decode_cursor(raw, &binding).map_err(cursor_error))
            .transpose()?;
        let page_size = i64::from(query.page_size.unwrap_or(50));
        let rows = sqlx::query(FOLDER_QUERY_SQL)
            .bind(query.parent_folder_id.as_uuid())
            .bind(subjects)
            .bind(position.as_ref().map(|position| position.document_id))
            .bind(
                position
                    .as_ref()
                    .and_then(|position| position.sort_title.as_deref()),
            )
            .bind(page_size + 1)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_statement_error)?;
        tx.rollback().await.map_err(map_statement_error)?;
        let mut items = rows
            .into_iter()
            .map(|row| {
                Ok(FolderSummary {
                    folder_id: FolderId::from_uuid(
                        row.try_get("folder_id").map_err(map_statement_error)?,
                    ),
                    parent_folder_id: FolderId::from_uuid(
                        row.try_get("parent_folder_id")
                            .map_err(map_statement_error)?,
                    ),
                    name: row.try_get("name").map_err(map_statement_error)?,
                    revision: row.try_get("revision").map_err(map_statement_error)?,
                })
            })
            .collect::<Result<Vec<_>, RepositoryError>>()?;
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
                        document_id: last.folder_id.as_uuid(),
                        sort_time_micros: None,
                        sort_title: Some(last.name.clone()),
                    },
                )
                .map_err(cursor_error)?,
            )
        } else {
            None
        };
        Ok(Page { items, next_cursor })
    }
}
