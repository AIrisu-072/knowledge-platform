use crate::document_diff_cache::BoundedDiffCache;
use document_application::{
    AuthoritativeDocument, CreateInitialDocumentRecord, CurrentPublishedVersionRef,
    DocumentPublishRepository, DocumentRepository, PublishCandidate, PublishDocumentResult,
    PublishInitialVersionRecord, PublishOperationId, PublishOperationRecord, PublishVersionRecord,
    RepositoryError, VerifiedActorContext,
};
use document_domain::{Action, ResourceRef};
use document_domain::{DocumentId, DocumentVersionId, FileId, PrincipalRef};
use serde_json::Value;
use sqlx::PgPool;
use std::sync::{Arc, Mutex};

use crate::{
    access_control::{
        AccessLockMode, authorize_document_snapshot, authorize_in_tx, lock_access_state,
        verified_subjects_json,
    },
    error::{map_commit_error, map_statement_error},
    publish, versioning_rows,
};

const AUDIT_SOURCE: &str = "urn:knowledge-platform:document-platform";

#[derive(Debug, Clone)]
pub struct PostgresDocumentRepository {
    pub(crate) pool: PgPool,
    pub(crate) diff_cache: Arc<Mutex<BoundedDiffCache>>,
    pub(crate) bootstrap_actor: Option<PrincipalRef>,
    pub(crate) verified_actor: Option<VerifiedActorContext>,
}

impl PostgresDocumentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            diff_cache: Arc::new(Mutex::new(BoundedDiffCache::default())),
            bootstrap_actor: None,
            verified_actor: None,
        }
    }

    pub fn new_with_bootstrap_actor(pool: PgPool, bootstrap_actor: PrincipalRef) -> Self {
        Self {
            pool,
            diff_cache: Arc::new(Mutex::new(BoundedDiffCache::default())),
            bootstrap_actor: Some(bootstrap_actor),
            verified_actor: None,
        }
    }

    pub fn with_verified_actor(&self, actor: VerifiedActorContext) -> Self {
        Self {
            pool: self.pool.clone(),
            diff_cache: self.diff_cache.clone(),
            bootstrap_actor: None,
            verified_actor: Some(actor),
        }
    }
}

impl DocumentRepository for PostgresDocumentRepository {
    async fn create_initial_document(
        &self,
        record: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        let (authoritative, domain_events, audit_events) = record.into_parts();
        let document = authoritative.document();
        let version = authoritative.version();

        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;

        let result: Result<(), RepositoryError> = async {
            if let Some(ctx) = &self.verified_actor {
                lock_access_state(&mut tx, AccessLockMode::Shared).await?;
                if version.created_by() != ctx.principal() {
                    return Err(RepositoryError::Forbidden);
                }
                authorize_in_tx(
                    &mut tx,
                    ctx,
                    &[(ResourceRef::Folder(document.folder_id()), vec![Action::Read, Action::Write])],
                ).await?;
            }
            let folder_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM folders WHERE folder_id = $1)",
            )
            .bind(document.folder_id().as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            if !folder_exists {
                return Err(RepositoryError::FolderNotFound);
            }

            for item in authoritative.content_items() {
                let file = item.file();
            sqlx::query(
                "INSERT INTO file_objects \
                 (file_id, content_hash, media_type, size_bytes, storage_locator, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(file.file_id().as_uuid())
            .bind(file.content_hash().as_bytes().to_vec())
            .bind(file.media_type().as_str())
            .bind(file.size_bytes().get())
            .bind(file.storage_key().as_str())
            .bind(file.created_at())
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;

            }

            sqlx::query(
                "INSERT INTO documents \
                 (document_id, folder_id, current_version_id, revision, metadata, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(document.document_id().as_uuid())
            .bind(document.folder_id().as_uuid())
            .bind(document.current_version_id().map(|id| id.as_uuid()))
            .bind(document.revision())
            .bind(Value::Object(document.metadata().as_map().clone()))
            .bind(document.created_at())
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;

            sqlx::query(
                "INSERT INTO document_versions \
                 (document_version_id, document_id, version_no, lifecycle_state, title, \
                  revision_reason, approved_at, scheduled_publish_at, published_at, withdrawn_at, \
                  effective_from, effective_to, created_by_identity_provider, \
                  created_by_principal_id, metadata, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
            )
            .bind(version.document_version_id().as_uuid())
            .bind(version.document_id().as_uuid())
            .bind(version.version_no().get())
            .bind(lifecycle_state(version.lifecycle_state()))
            .bind(version.title().as_str())
            .bind(version.revision_reason())
            .bind(version.approved_at())
            .bind(version.scheduled_publish_at())
            .bind(version.published_at())
            .bind(version.withdrawn_at())
            .bind(version.effective_from())
            .bind(version.effective_to())
            .bind(version.created_by().identity_provider())
            .bind(version.created_by().principal_id())
            .bind(Value::Object(version.metadata().as_map().clone()))
            .bind(version.created_at())
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;

            for item in authoritative.content_items() {
            let content_item_id = uuid::Uuid::now_v7();
            let representation_id = uuid::Uuid::now_v7();
            sqlx::query(
                "INSERT INTO content_items \
                 (content_item_id, document_version_id, logical_path, ordinal, \
                  authoritative_representation_id) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(content_item_id)
            .bind(version.document_version_id().as_uuid())
            .bind(item.logical_path().as_str())
            .bind(i64::from(item.ordinal()))
            .bind(representation_id)
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;
            sqlx::query(
                "INSERT INTO content_representations \
                 (content_representation_id, content_item_id, file_id, role, original_filename) \
                 VALUES ($1, $2, $3, 'AUTHORITATIVE', $4)",
            )
            .bind(representation_id)
            .bind(content_item_id)
            .bind(item.file().file_id().as_uuid())
            .bind(item.original_filename())
            .execute(&mut *tx)
            .await
            .map_err(map_statement_error)?;

            }

            for event in &domain_events {
                sqlx::query(
                    "INSERT INTO outbox_events \
                     (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at, \
                      available_at, attempt_count, delivered_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $6, 0, NULL)",
                )
                .bind(event.event_id().as_uuid())
                .bind(event.event_type())
                .bind(event.aggregate_type())
                .bind(event.aggregate_id().as_uuid())
                .bind(event.payload().clone())
                .bind(event.occurred_at())
                .execute(&mut *tx)
                .await
                .map_err(map_statement_error)?;
            }

            for event in &audit_events {
                let subject = format!("document/{}", event.resource_id().as_uuid());
                sqlx::query(
                    "INSERT INTO audit_outbox_events \
                     (event_id, event_type, source, subject, actor_identity_provider, \
                      actor_principal_id, resource_id, resource_version_id, result, trace_id, data, \
                      occurred_at, attempt_count, delivered_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10, $11, 0, NULL)",
                )
                .bind(event.event_id().as_uuid())
                .bind(event.event_type())
                .bind(AUDIT_SOURCE)
                .bind(subject)
                .bind(event.actor().identity_provider())
                .bind(event.actor().principal_id())
                .bind(event.resource_id().as_uuid())
                .bind(event.resource_version_id().map(|id| id.as_uuid()))
                .bind(event.result())
                .bind(event.data().clone())
                .bind(event.occurred_at())
                .execute(&mut *tx)
                .await
                .map_err(map_statement_error)?;
            }

            Ok(())
        }
        .await;

        if let Err(error) = result {
            let _ = tx.rollback().await;
            return Err(error);
        }

        tx.commit().await.map_err(map_commit_error)
    }

    async fn get_authoritative_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        if let Some(ctx) = &self.verified_actor {
            versioning_rows::load_current_scoped(&self.pool, id, ctx).await
        } else {
            versioning_rows::load_current(&self.pool, id).await
        }
    }

    async fn get_authoring_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        if let Some(ctx) = &self.verified_actor {
            versioning_rows::load_authoring_scoped(&self.pool, id, ctx).await
        } else {
            versioning_rows::load_authoring(&self.pool, id).await
        }
    }

    async fn get_current_published_document(
        &self,
        id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        if let Some(ctx) = &self.verified_actor {
            versioning_rows::load_current_published_scoped(&self.pool, id, ctx).await
        } else {
            versioning_rows::load_current_published(&self.pool, id).await
        }
    }

    async fn is_current_published_version(
        &self,
        document_id: DocumentId,
        version_id: DocumentVersionId,
    ) -> Result<bool, RepositoryError> {
        if self
            .verified_actor
            .as_ref()
            .is_some_and(|ctx| ctx.ensure_current().is_err())
        {
            return Ok(false);
        }
        let subjects = self.verified_actor.as_ref().map(verified_subjects_json);
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM documents d \
             JOIN document_versions v ON v.document_version_id = d.current_version_id \
                                    AND v.document_id = d.document_id \
             WHERE d.document_id = $1 AND v.document_version_id = $2 \
               AND v.lifecycle_state = 'PUBLISHED' \
               AND ($3::jsonb IS NULL OR dmb_allows_document(d.document_id, $3, ARRAY['read']::text[])) \
               AND NOT EXISTS (SELECT 1 FROM document_publication_end_operations e \
                               WHERE e.document_id = d.document_id))",
        )
        .bind(document_id.as_uuid())
        .bind(version_id.as_uuid())
        .bind(subjects)
        .fetch_one(&self.pool)
        .await
        .map_err(map_statement_error)
    }

    async fn list_current_published_versions(
        &self,
        after: Option<DocumentId>,
        limit: i64,
    ) -> Result<Vec<CurrentPublishedVersionRef>, RepositoryError> {
        if !(1..=1000).contains(&limit) {
            return Err(RepositoryError::BusinessRule);
        }
        if self
            .verified_actor
            .as_ref()
            .is_some_and(|ctx| ctx.ensure_current().is_err())
        {
            return Ok(Vec::new());
        }
        let subjects = self.verified_actor.as_ref().map(verified_subjects_json);
        let rows: Vec<(uuid::Uuid, uuid::Uuid, i64)> = sqlx::query_as(
            "SELECT d.document_id, v.document_version_id, d.revision FROM documents d \
             JOIN document_versions v ON v.document_version_id = d.current_version_id \
                                    AND v.document_id = d.document_id \
             WHERE v.lifecycle_state = 'PUBLISHED' \
               AND ($1::uuid IS NULL OR d.document_id > $1) \
               AND ($3::jsonb IS NULL OR dmb_allows_document(d.document_id, $3, ARRAY['read']::text[])) \
               AND NOT EXISTS (SELECT 1 FROM document_publication_end_operations e \
                               WHERE e.document_id = d.document_id) \
             ORDER BY d.document_id LIMIT $2",
        )
        .bind(after.map(|id| id.as_uuid()))
        .bind(limit)
        .bind(subjects)
        .fetch_all(&self.pool)
        .await
        .map_err(map_statement_error)?;
        Ok(rows
            .into_iter()
            .map(|(document_id, version_id, revision)| {
                CurrentPublishedVersionRef::new(
                    DocumentId::from_uuid(document_id),
                    DocumentVersionId::from_uuid(version_id),
                    revision,
                )
            })
            .collect())
    }

    async fn file_reference_exists(&self, file_id: FileId) -> Result<bool, RepositoryError> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM content_representations WHERE file_id = $1) \
                 OR EXISTS (SELECT 1 FROM version_files WHERE file_id = $1)",
        )
        .bind(file_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_statement_error)
    }

    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError> {
        let ids: Vec<uuid::Uuid> = sqlx::query_scalar(
            "SELECT file_id FROM ( \
                 SELECT file_id FROM content_representations \
                 UNION SELECT file_id FROM version_files \
             ) refs ORDER BY file_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_statement_error)?;
        Ok(ids.into_iter().map(FileId::from_uuid).collect())
    }
}

fn lifecycle_state(state: document_domain::LifecycleState) -> &'static str {
    match state {
        document_domain::LifecycleState::Working => "WORKING",
        document_domain::LifecycleState::Published => "PUBLISHED",
        document_domain::LifecycleState::Withdrawn => "WITHDRAWN",
    }
}

impl DocumentPublishRepository for PostgresDocumentRepository {
    async fn get_publish_operation(
        &self,
        operation_id: PublishOperationId,
    ) -> Result<Option<PublishOperationRecord>, RepositoryError> {
        let operation = publish::get_publish_operation(&self.pool, operation_id).await?;
        if let (Some(ctx), Some(operation)) = (&self.verified_actor, &operation) {
            authorize_document_snapshot(
                &self.pool,
                ctx,
                operation.identity().document_id(),
                &[Action::Read, Action::Publish],
            )
            .await?;
        }
        Ok(operation)
    }

    async fn get_publish_candidate(
        &self,
        document_id: DocumentId,
        target_version_id: document_domain::DocumentVersionId,
    ) -> Result<PublishCandidate, RepositoryError> {
        if let Some(ctx) = &self.verified_actor {
            authorize_document_snapshot(
                &self.pool,
                ctx,
                document_id,
                &[Action::Read, Action::Publish],
            )
            .await?;
        }
        publish::get_publish_candidate(&self.pool, document_id, target_version_id).await
    }

    async fn publish_initial_version(
        &self,
        record: PublishInitialVersionRecord,
    ) -> Result<PublishDocumentResult, RepositoryError> {
        publish::publish_initial_version(&self.pool, record, self.verified_actor.as_ref()).await
    }

    async fn publish_next_version(
        &self,
        record: PublishVersionRecord,
    ) -> Result<PublishDocumentResult, RepositoryError> {
        publish::publish_next_version(&self.pool, record, self.verified_actor.as_ref()).await
    }
}
