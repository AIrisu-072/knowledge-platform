//! Read-side Document snapshots. Every snapshot read stays in one PostgreSQL
//! REPEATABLE READ, READ ONLY transaction; policy checks remain in Document.

use std::collections::BTreeSet;

use document_application::{ApplicationError, DocumentAccessCheckService, VerifiedActorContext};
use document_domain::{Action, DocumentId, DocumentVersionId, FolderId, LifecycleState, Title};
use document_repository_postgres::PostgresDocumentRepository;
use document_semantic_inspection_core::{CapabilityEvidence, CapabilityState};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, CurrentCandidateAccessEvaluatorPort,
};
use search_core::discovery::{CandidateIdentityClass, FederatedCandidate};
use search_core::id::{ResourceId, SourceId};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::model::{
    DocumentAccessProjectionInput, DocumentSourceSnapshot, DsiEvidenceRefs,
    PermittedDocumentMetadata, PublicationEndRecord,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsiReadState {
    Verified,
    UnknownMissing,
    UnknownInvalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSnapshotRecord {
    pub snapshot: DocumentSourceSnapshot,
    pub document_revision: i64,
    pub access_revision: i64,
    pub dsi_state: DsiReadState,
}

/// One authoritative enumeration for the Search Live generation. Both tiers
/// and the empty-state token come from one read-only PostgreSQL snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentOutboxSnapshot {
    pub source_snapshot: String,
    pub live: Vec<VersionSnapshotRecord>,
    pub historical: Vec<VersionSnapshotRecord>,
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotReadError {
    #[error("Document snapshot query failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Document snapshot violates authoritative schema: {0}")]
    Integrity(&'static str),
}

#[allow(async_fn_in_trait)]
pub trait DocumentSnapshotReader: Send + Sync {
    async fn load_document_version(
        &self,
        version_id: DocumentVersionId,
    ) -> Result<Option<VersionSnapshotRecord>, SnapshotReadError>;

    async fn enumerate_live(&self) -> Result<Vec<VersionSnapshotRecord>, SnapshotReadError>;

    async fn enumerate_historical(&self) -> Result<Vec<VersionSnapshotRecord>, SnapshotReadError>;
}

#[derive(Clone)]
pub struct PostgresDocumentSnapshotReader {
    pool: PgPool,
}

impl PostgresDocumentSnapshotReader {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn read(
        &self,
        version_id: Option<DocumentVersionId>,
        mode: &'static str,
    ) -> Result<(String, Vec<VersionSnapshotRecord>), SnapshotReadError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let (source_snapshot, access_revision): (String, i64) = sqlx::query_as(
            "SELECT pg_current_snapshot()::text, access_revision \
             FROM document_access_state WHERE id = 1",
        )
        .fetch_one(&mut *tx)
        .await?;

        let rows: Vec<DocumentVersionRow> = sqlx::query_as(
            "SELECT d.document_id, d.folder_id, d.current_version_id, \
                    d.revision AS document_revision, d.metadata AS document_metadata, \
                    v.created_at AS version_created_at, \
                    v.document_version_id, v.lifecycle_state, v.title, \
                    v.published_at, v.withdrawn_at, v.effective_from, v.effective_to, \
                    e.operation_id AS publication_end_operation_id, \
                    e.ended_at AS publication_ended_at \
             FROM document_versions v \
             JOIN documents d ON d.document_id = v.document_id \
             LEFT JOIN document_publication_end_operations e ON e.document_id = d.document_id \
             WHERE ($1::uuid IS NULL OR v.document_version_id = $1) \
               AND (\
                   $2::text = 'one' \
                   OR ($2::text = 'live' AND d.current_version_id = v.document_version_id \
                       AND v.lifecycle_state = 'PUBLISHED' AND e.operation_id IS NULL) \
                   OR ($2::text = 'historical' AND v.lifecycle_state IN ('PUBLISHED', 'WITHDRAWN') \
                       AND (d.current_version_id IS DISTINCT FROM v.document_version_id \
                            OR e.operation_id IS NOT NULL)) \
                   OR ($2::text = 'indexable' AND v.lifecycle_state IN ('PUBLISHED', 'WITHDRAWN'))\
               ) \
             ORDER BY d.document_id, v.version_no",
        )
        .bind(version_id.map(DocumentVersionId::as_uuid))
        .bind(mode)
        .fetch_all(&mut *tx)
        .await?;

        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let (dsi_state, dsi) = read_dsi(&mut tx, row.document_version_id).await?;
            records.push(row.restore(&source_snapshot, access_revision, dsi_state, dsi)?);
        }
        tx.rollback().await?;
        Ok((source_snapshot, records))
    }

    pub async fn enumerate_outbox_snapshot(
        &self,
    ) -> Result<DocumentOutboxSnapshot, SnapshotReadError> {
        let (source_snapshot, records) = self.read(None, "indexable").await?;
        let (live, historical) = records.into_iter().partition(|record| {
            let snapshot = &record.snapshot;
            snapshot.lifecycle_state == LifecycleState::Published
                && snapshot.current_version_id == Some(snapshot.document_version_id)
                && snapshot.publication_end.is_none()
        });
        Ok(DocumentOutboxSnapshot {
            source_snapshot,
            live,
            historical,
        })
    }
}

impl DocumentSnapshotReader for PostgresDocumentSnapshotReader {
    async fn load_document_version(
        &self,
        version_id: DocumentVersionId,
    ) -> Result<Option<VersionSnapshotRecord>, SnapshotReadError> {
        Ok(self
            .read(Some(version_id), "one")
            .await?
            .1
            .into_iter()
            .next())
    }

    async fn enumerate_live(&self) -> Result<Vec<VersionSnapshotRecord>, SnapshotReadError> {
        Ok(self.read(None, "live").await?.1)
    }

    async fn enumerate_historical(&self) -> Result<Vec<VersionSnapshotRecord>, SnapshotReadError> {
        Ok(self.read(None, "historical").await?.1)
    }
}

#[derive(FromRow)]
struct DocumentVersionRow {
    document_id: Uuid,
    folder_id: Uuid,
    current_version_id: Option<Uuid>,
    document_revision: i64,
    document_metadata: Value,
    version_created_at: OffsetDateTime,
    document_version_id: Uuid,
    lifecycle_state: String,
    title: String,
    published_at: Option<OffsetDateTime>,
    withdrawn_at: Option<OffsetDateTime>,
    effective_from: Option<OffsetDateTime>,
    effective_to: Option<OffsetDateTime>,
    publication_end_operation_id: Option<Uuid>,
    publication_ended_at: Option<OffsetDateTime>,
}

impl DocumentVersionRow {
    fn restore(
        self,
        source_snapshot: &str,
        access_revision: i64,
        dsi_state: DsiReadState,
        dsi: Option<DsiEvidenceRefs>,
    ) -> Result<VersionSnapshotRecord, SnapshotReadError> {
        let publication_end = match (self.publication_end_operation_id, self.publication_ended_at) {
            (Some(operation_id), Some(ended_at)) => {
                if self.current_version_id.is_some() {
                    return Err(SnapshotReadError::Integrity("T10 has a current version"));
                }
                Some(PublicationEndRecord {
                    operation_id,
                    ended_at,
                })
            }
            (None, None) => None,
            _ => return Err(SnapshotReadError::Integrity("incomplete T10 operation")),
        };
        let lifecycle_state = match self.lifecycle_state.as_str() {
            "WORKING" => LifecycleState::Working,
            "PUBLISHED" => LifecycleState::Published,
            "WITHDRAWN" => LifecycleState::Withdrawn,
            _ => return Err(SnapshotReadError::Integrity("unknown lifecycle state")),
        };
        if self.current_version_id == Some(self.document_version_id)
            && lifecycle_state != LifecycleState::Published
        {
            return Err(SnapshotReadError::Integrity(
                "current version is not published",
            ));
        }
        let title = Title::new(self.title)
            .map_err(|_| SnapshotReadError::Integrity("blank version title"))?;
        let metadata = self
            .document_metadata
            .as_object()
            .ok_or(SnapshotReadError::Integrity(
                "document metadata is not an object",
            ))?;
        let permitted = PermittedDocumentMetadata {
            document_type: metadata
                .get("document_type")
                .and_then(Value::as_str)
                .map(str::to_owned),
            category: metadata
                .get("category")
                .and_then(Value::as_str)
                .map(str::to_owned),
        };
        let document_id = DocumentId::from_uuid(self.document_id);
        let snapshot = DocumentSourceSnapshot {
            source_snapshot: source_snapshot.to_owned(),
            document_id,
            document_version_id: DocumentVersionId::from_uuid(self.document_version_id),
            current_version_id: self.current_version_id.map(DocumentVersionId::from_uuid),
            publication_end,
            lifecycle_state,
            title,
            metadata: permitted,
            folder_id: FolderId::from_uuid(self.folder_id),
            created_at: self.version_created_at,
            published_at: self.published_at,
            withdrawn_at: self.withdrawn_at,
            effective_from: self.effective_from,
            effective_to: self.effective_to,
            access: DocumentAccessProjectionInput {
                access_scope: Some(format!(
                    "document:{}:access-revision:{access_revision}",
                    document_id.as_uuid()
                )),
            },
            dsi,
        };
        Ok(VersionSnapshotRecord {
            snapshot,
            document_revision: self.document_revision,
            access_revision,
            dsi_state,
        })
    }
}

#[derive(FromRow)]
struct AuthoritativeDsiRow {
    file_id: Option<Uuid>,
    representation_format: Option<String>,
    representation_profile: Option<String>,
    representation_fingerprint: Option<Vec<u8>>,
    content_hash: Option<Vec<u8>>,
    size_bytes: Option<i64>,
    inspection_profile_version: Option<String>,
    worker_protocol_version: Option<String>,
    observed_raw_content_hash: Option<Vec<u8>>,
    observed_size_bytes: Option<i64>,
    detected_format: Option<String>,
    fingerprint_algorithm: Option<String>,
    fingerprint_digest: Option<Vec<u8>>,
    semantic_capabilities: Option<Value>,
}

async fn read_dsi(
    tx: &mut Transaction<'_, Postgres>,
    version_id: Uuid,
) -> Result<(DsiReadState, Option<DsiEvidenceRefs>), SnapshotReadError> {
    let rows: Vec<AuthoritativeDsiRow> = sqlx::query_as(
        "SELECT cr.file_id, \
                cr.detected_format AS representation_format, \
                cr.inspection_profile_version AS representation_profile, \
                cr.semantic_fingerprint AS representation_fingerprint, \
                f.content_hash, f.size_bytes, \
                s.inspection_profile_version, s.worker_protocol_version, \
                s.observed_raw_content_hash, s.observed_size_bytes, s.detected_format, \
                s.fingerprint_algorithm, s.fingerprint_digest, s.semantic_capabilities \
         FROM content_items ci \
         LEFT JOIN content_representations cr \
           ON cr.content_representation_id = ci.authoritative_representation_id \
          AND cr.content_item_id = ci.content_item_id AND cr.role = 'AUTHORITATIVE' \
         LEFT JOIN file_objects f ON f.file_id = cr.file_id \
         LEFT JOIN document_semantic_inspections s ON s.file_id = f.file_id \
          AND s.inspection_profile_version = 'dsi-v0' \
         WHERE ci.document_version_id = $1 \
         ORDER BY ci.ordinal, ci.logical_path",
    )
    .bind(version_id)
    .fetch_all(&mut **tx)
    .await?;
    if rows.is_empty() {
        return Ok((DsiReadState::UnknownMissing, None));
    }
    let mut missing = false;
    let mut invalid = false;
    let mut capabilities = BTreeSet::new();
    let mut evidence = Vec::with_capacity(rows.len());
    for row in rows {
        let Some(file_id) = row.file_id else {
            invalid = true;
            continue;
        };
        let Some(profile) = row.inspection_profile_version.as_deref() else {
            missing = true;
            continue;
        };
        if !valid_dsi_binding(&row) {
            invalid = true;
            continue;
        }
        let Some(items) = row.semantic_capabilities.as_ref().and_then(|value| {
            serde_json::from_value::<Vec<CapabilityEvidence>>(value.clone()).ok()
        }) else {
            invalid = true;
            continue;
        };
        let mut item_capabilities = BTreeSet::new();
        let mut item_valid = true;
        for item in items {
            if item.capability_id.trim().is_empty()
                || !item_capabilities.insert(item.capability_id.clone())
            {
                item_valid = false;
                break;
            }
            if item.presence == CapabilityState::Present {
                capabilities.insert(format!("dsi-capability:{}", item.capability_id));
            }
        }
        if !item_valid {
            invalid = true;
            continue;
        }
        evidence.push(format!("dsi:{file_id}:{profile}"));
    }
    if invalid {
        Ok((DsiReadState::UnknownInvalid, None))
    } else if missing {
        Ok((DsiReadState::UnknownMissing, None))
    } else {
        Ok((
            DsiReadState::Verified,
            Some(DsiEvidenceRefs {
                capability_refs: capabilities.into_iter().collect(),
                evidence_refs: evidence,
            }),
        ))
    }
}

fn valid_dsi_binding(row: &AuthoritativeDsiRow) -> bool {
    row.inspection_profile_version.as_deref() == Some("dsi-v0")
        && row.worker_protocol_version.as_deref() == Some("dsi-worker-v0")
        && row.fingerprint_algorithm.as_deref() == Some("sha256")
        && row.content_hash.as_ref().is_some_and(|hash| {
            hash.len() == 32 && row.observed_raw_content_hash.as_ref() == Some(hash)
        })
        && row
            .size_bytes
            .is_some_and(|size| size >= 0 && row.observed_size_bytes == Some(size))
        && row.detected_format.as_ref() == row.representation_format.as_ref()
        && row.inspection_profile_version.as_ref() == row.representation_profile.as_ref()
        && row.fingerprint_digest.as_ref().is_some_and(|digest| {
            digest.len() == 32 && row.representation_fingerprint.as_ref() == Some(digest)
        })
}

/// Session-scoped adapter. The string from Search is only an exact binding key;
/// it never becomes a principal or a set of claims.
pub struct DocumentCurrentAccessAdapter {
    expected_source_id: SourceId,
    pool: PgPool,
    service: DocumentAccessCheckService<PostgresDocumentRepository>,
    actor: VerifiedActorContext,
    access_context_binding: String,
}

impl DocumentCurrentAccessAdapter {
    pub fn new(
        expected_source_id: SourceId,
        pool: PgPool,
        service: DocumentAccessCheckService<PostgresDocumentRepository>,
        actor: VerifiedActorContext,
        access_context_binding: String,
    ) -> Self {
        Self {
            expected_source_id,
            pool,
            service,
            actor,
            access_context_binding,
        }
    }

    async fn current_version(&self, version_id: Uuid) -> Result<Option<(Uuid, i64)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT d.document_id, a.access_revision \
             FROM document_versions v \
             JOIN documents d ON d.document_id = v.document_id \
             CROSS JOIN document_access_state a \
             WHERE v.document_version_id = $1 \
               AND d.current_version_id = v.document_version_id \
               AND v.lifecycle_state = 'PUBLISHED' AND a.id = 1 \
               AND NOT EXISTS (SELECT 1 FROM document_publication_end_operations e \
                               WHERE e.document_id = d.document_id)",
        )
        .bind(version_id)
        .fetch_optional(&self.pool)
        .await
    }

    /// A graph-only structural node resolves its owning Document and checks
    /// current publication plus the existing Document Read authorization.
    /// No Version pointer is cached in the graph ownership map.
    pub(crate) async fn evaluate_owned_document(
        &self,
        owner: DocumentId,
        access_context: &str,
    ) -> Result<AccessDecision, search_application::SearchError> {
        if self.access_context_binding.is_empty()
            || access_context != self.access_context_binding
            || self.actor.ensure_current().is_err()
        {
            return Ok(AccessDecision::Unknown);
        }
        let current = self.current_document(owner).await.map_err(|error| {
            search_application::SearchError::SourceUnavailable(error.to_string())
        })?;
        let Some(revision) = current else {
            return Ok(AccessDecision::Denied);
        };
        match self
            .service
            .check(&self.actor, owner, &[Action::Read])
            .await
        {
            Ok(()) => {}
            Err(
                ApplicationError::Forbidden
                | ApplicationError::DocumentNotFound
                | ApplicationError::DocumentVersionNotFound
                | ApplicationError::StaleVersion,
            ) => return Ok(AccessDecision::Denied),
            Err(_) => return Ok(AccessDecision::Unknown),
        }
        let after = self.current_document(owner).await.map_err(|error| {
            search_application::SearchError::SourceUnavailable(error.to_string())
        })?;
        Ok(if after == Some(revision) {
            AccessDecision::Allowed
        } else {
            AccessDecision::Denied
        })
    }

    async fn current_document(
        &self,
        owner: DocumentId,
    ) -> Result<Option<(Uuid, i64)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT d.current_version_id, a.access_revision FROM documents d \
             JOIN document_versions v ON v.document_version_id = d.current_version_id \
             CROSS JOIN document_access_state a \
             WHERE d.document_id = $1 AND v.lifecycle_state = 'PUBLISHED' AND a.id = 1 \
               AND NOT EXISTS (SELECT 1 FROM document_publication_end_operations e \
                               WHERE e.document_id = d.document_id)",
        )
        .bind(owner.as_uuid())
        .fetch_optional(&self.pool)
        .await
    }
}

impl CurrentCandidateAccessEvaluatorPort for DocumentCurrentAccessAdapter {
    fn evaluate<'a>(
        &'a self,
        candidate: &'a FederatedCandidate,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            let Some(resource) = candidate.resource_ref else {
                return Ok(AccessDecision::Denied);
            };
            // The two identifiers are Source-owned bindings, not request-supplied
            // actor material. Only a Document Version with the canonical durable
            // identity can reach the current Document authorization service.
            if candidate.source_ref != self.expected_source_id
                || candidate.identity_class != CandidateIdentityClass::DurableResource
                || candidate.candidate_id
                    != format!(
                        "{}:{}",
                        self.expected_source_id.as_uuid(),
                        resource.as_uuid()
                    )
            {
                return Ok(AccessDecision::Denied);
            }
            CurrentAccessEvaluatorPort::evaluate(self, resource, access_context).await
        })
    }
}

impl CurrentAccessEvaluatorPort for DocumentCurrentAccessAdapter {
    fn evaluate<'a>(
        &'a self,
        resource_ref: ResourceId,
        access_context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if self.access_context_binding.is_empty()
                || access_context != self.access_context_binding
                || self.actor.ensure_current().is_err()
            {
                return Ok(AccessDecision::Unknown);
            }
            let Some((document_id, revision)) = self
                .current_version(resource_ref.as_uuid())
                .await
                .map_err(|error| {
                    search_application::error::SearchError::SourceUnavailable(error.to_string())
                })?
            else {
                return Ok(AccessDecision::Denied);
            };
            match self
                .service
                .check(
                    &self.actor,
                    DocumentId::from_uuid(document_id),
                    &[Action::Read],
                )
                .await
            {
                Ok(()) => {}
                Err(
                    ApplicationError::Forbidden
                    | ApplicationError::DocumentNotFound
                    | ApplicationError::DocumentVersionNotFound
                    | ApplicationError::StaleVersion,
                ) => return Ok(AccessDecision::Denied),
                Err(_) => return Ok(AccessDecision::Unknown),
            }
            // Publication and policy may have changed while the Document service ran.
            let after = self
                .current_version(resource_ref.as_uuid())
                .await
                .map_err(|error| {
                    search_application::error::SearchError::SourceUnavailable(error.to_string())
                })?;
            Ok(match after {
                Some((same_document, same_revision))
                    if same_document == document_id && same_revision == revision =>
                {
                    AccessDecision::Allowed
                }
                _ => AccessDecision::Denied,
            })
        })
    }
}
