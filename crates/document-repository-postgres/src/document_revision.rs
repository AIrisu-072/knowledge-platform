use document_application::RepositoryError;
use document_domain::{
    DocumentId, DocumentMetadataSnapshot, DocumentRevision, DocumentRevisionId,
    DocumentRevisionMetadataStatus, DocumentRevisionNumber, DocumentRevisionSourceKind,
    DocumentVersionId, Metadata, PrincipalRef,
};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_statement_error;

pub(crate) struct PublicationRevisionInput<'a> {
    pub(crate) document_id: DocumentId,
    pub(crate) document_version_id: DocumentVersionId,
    pub(crate) metadata: &'a Value,
    pub(crate) operation_id: Uuid,
    pub(crate) actor: &'a PrincipalRef,
    pub(crate) created_at: OffsetDateTime,
    pub(crate) is_initial_path: bool,
}

pub(crate) struct MetadataRevisionInput<'a> {
    pub(crate) document_id: DocumentId,
    pub(crate) current_document_version_id: Option<DocumentVersionId>,
    pub(crate) metadata: &'a Value,
    pub(crate) operation_id: Uuid,
    pub(crate) actor: &'a PrincipalRef,
    pub(crate) reason: &'a str,
    pub(crate) created_at: OffsetDateTime,
}

pub(crate) struct WithdrawFallbackRevisionInput<'a> {
    pub(crate) document_id: DocumentId,
    pub(crate) document_version_id: DocumentVersionId,
    pub(crate) metadata: &'a Value,
    pub(crate) operation_id: Uuid,
    pub(crate) actor: &'a PrincipalRef,
    pub(crate) reason: &'a str,
    pub(crate) created_at: OffsetDateTime,
}

pub(crate) async fn issue_publication_revision(
    tx: &mut Transaction<'_, Postgres>,
    input: PublicationRevisionInput<'_>,
) -> Result<(), RepositoryError> {
    let PublicationRevisionInput {
        document_id,
        document_version_id,
        metadata,
        operation_id,
        actor,
        created_at,
        is_initial_path,
    } = input;
    let previous_major = max_major(tx, document_id).await?;
    let major = previous_major
        .checked_add(1)
        .ok_or(RepositoryError::IntegrityViolation)?;
    let source_kind = if is_initial_path && previous_major == 0 {
        DocumentRevisionSourceKind::InitialPublication
    } else {
        DocumentRevisionSourceKind::ContentPublication
    };
    insert_revision(
        tx,
        document_id,
        document_version_id,
        DocumentRevisionNumber::new(major, 0).map_err(|_| RepositoryError::IntegrityViolation)?,
        metadata,
        source_kind,
        Some(operation_id),
        created_at,
        Some(actor.clone()),
        None,
    )
    .await
}

pub(crate) async fn issue_metadata_revision(
    tx: &mut Transaction<'_, Postgres>,
    input: MetadataRevisionInput<'_>,
) -> Result<(), RepositoryError> {
    let MetadataRevisionInput {
        document_id,
        current_document_version_id,
        metadata,
        operation_id,
        actor,
        reason,
        created_at,
    } = input;
    let latest: Option<(i64, i64, Uuid)> = sqlx::query_as(
        "SELECT major_no, minor_no, document_version_id FROM document_revisions \
         WHERE document_id = $1 ORDER BY major_no DESC, minor_no DESC LIMIT 1",
    )
    .bind(document_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    let Some((major, minor, latest_version_id)) = latest else {
        return Ok(());
    };
    let document_version_id = current_document_version_id
        .unwrap_or_else(|| DocumentVersionId::from_uuid(latest_version_id));
    let minor = minor
        .checked_add(1)
        .ok_or(RepositoryError::IntegrityViolation)?;
    insert_revision(
        tx,
        document_id,
        document_version_id,
        DocumentRevisionNumber::new(major, minor)
            .map_err(|_| RepositoryError::IntegrityViolation)?,
        metadata,
        DocumentRevisionSourceKind::MetadataRevision,
        Some(operation_id),
        created_at,
        Some(actor.clone()),
        Some(reason.to_owned()),
    )
    .await
}

pub(crate) async fn issue_withdraw_fallback_revision(
    tx: &mut Transaction<'_, Postgres>,
    input: WithdrawFallbackRevisionInput<'_>,
) -> Result<(), RepositoryError> {
    let WithdrawFallbackRevisionInput {
        document_id,
        document_version_id,
        metadata,
        operation_id,
        actor,
        reason,
        created_at,
    } = input;
    let major = max_major(tx, document_id)
        .await?
        .checked_add(1)
        .ok_or(RepositoryError::IntegrityViolation)?;
    insert_revision(
        tx,
        document_id,
        document_version_id,
        DocumentRevisionNumber::new(major, 0).map_err(|_| RepositoryError::IntegrityViolation)?,
        metadata,
        DocumentRevisionSourceKind::WithdrawFallback,
        Some(operation_id),
        created_at,
        Some(actor.clone()),
        Some(reason.to_owned()),
    )
    .await
}

async fn max_major(
    tx: &mut Transaction<'_, Postgres>,
    document_id: DocumentId,
) -> Result<i64, RepositoryError> {
    sqlx::query_scalar(
        "SELECT COALESCE(MAX(major_no), 0) FROM document_revisions WHERE document_id = $1",
    )
    .bind(document_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(map_statement_error)
}

#[allow(clippy::too_many_arguments)]
async fn insert_revision(
    tx: &mut Transaction<'_, Postgres>,
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
    number: DocumentRevisionNumber,
    metadata: &Value,
    source_kind: DocumentRevisionSourceKind,
    operation_id: Option<Uuid>,
    created_at: OffsetDateTime,
    actor: Option<PrincipalRef>,
    reason: Option<String>,
) -> Result<(), RepositoryError> {
    let metadata = metadata
        .as_object()
        .cloned()
        .map(Metadata::from_map)
        .ok_or(RepositoryError::IntegrityViolation)?;
    let snapshot = DocumentMetadataSnapshot::from_metadata(&metadata)
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let revision = DocumentRevision::new(
        DocumentRevisionId::from_uuid(Uuid::now_v7()),
        document_id,
        document_version_id,
        number,
        Some(snapshot),
        DocumentRevisionMetadataStatus::Complete,
        source_kind,
        operation_id,
        created_at,
        actor,
        reason,
    )
    .map_err(|_| RepositoryError::IntegrityViolation)?;

    let snapshot = revision
        .metadata_snapshot()
        .ok_or(RepositoryError::IntegrityViolation)?
        .as_json();
    sqlx::query(
        "INSERT INTO document_revisions \
         (revision_id, document_id, document_version_id, major_no, minor_no, \
          metadata_snapshot, metadata_snapshot_status, source_kind, operation_id, created_at, \
          actor_identity_provider, actor_principal_id, reason) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(revision.revision_id().as_uuid())
    .bind(revision.document_id().as_uuid())
    .bind(revision.document_version_id().as_uuid())
    .bind(revision.number().major_no())
    .bind(revision.number().minor_no())
    .bind(snapshot)
    .bind(revision.metadata_snapshot_status().as_str())
    .bind(revision.source_kind().as_str())
    .bind(revision.operation_id())
    .bind(revision.created_at())
    .bind(revision.actor().map(PrincipalRef::identity_provider))
    .bind(revision.actor().map(PrincipalRef::principal_id))
    .bind(revision.reason())
    .execute(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    Ok(())
}
