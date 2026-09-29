use std::collections::HashMap;

use document_application::{
    RepositoryError, VerifiedActorContext, VersionPurpose, VersionRequest,
    document_diff::{
        DiffPairSnapshot, DiffRequest, DocumentDiffRepository, SnapshotItem, VersionSnapshot,
        manifest_fingerprint,
    },
};
use document_domain::{Action, DocumentVersionId, FileId, ResourceRef};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state},
    document_history::{authorize_version_in_tx, begin_snapshot},
    error::map_statement_error,
    semantic_inspection_rows::SemanticInspectionRow,
};

impl DocumentDiffRepository for PostgresDocumentRepository {
    async fn capture_pair(
        &self,
        actor: &VerifiedActorContext,
        request: DiffRequest,
    ) -> Result<DiffPairSnapshot, RepositoryError> {
        request
            .validate()
            .map_err(|_| RepositoryError::BusinessRule)?;
        let mut tx = begin_snapshot(&self.pool).await?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        match authorize_in_tx(
            &mut tx,
            actor,
            &[(
                ResourceRef::Document(request.document_id),
                vec![Action::Read],
            )],
        )
        .await
        {
            Err(RepositoryError::Forbidden) => return Err(RepositoryError::DocumentNotFound),
            other => other?,
        }
        let base = capture_version(&mut tx, actor, &request, request.base_version_id).await?;
        let target = capture_version(&mut tx, actor, &request, request.target_version_id).await?;
        let pair = DiffPairSnapshot {
            document_id: request.document_id,
            base,
            target,
        };
        pair.validate()
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        tx.commit().await.map_err(crate::error::map_commit_error)?;
        Ok(pair)
    }
}

async fn capture_version(
    tx: &mut Transaction<'_, Postgres>,
    actor: &VerifiedActorContext,
    request: &DiffRequest,
    version_id: DocumentVersionId,
) -> Result<VersionSnapshot, RepositoryError> {
    let state = sqlx::query(
        "SELECT v.lifecycle_state, d.current_version_id, \
                EXISTS (SELECT 1 FROM document_publication_end_operations e \
                        WHERE e.document_id = d.document_id) AS ended \
         FROM document_versions v JOIN documents d ON d.document_id = v.document_id \
         WHERE v.document_id = $1 AND v.document_version_id = $2",
    )
    .bind(request.document_id.as_uuid())
    .bind(version_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?
    .ok_or(RepositoryError::DocumentVersionNotFound)?;
    let lifecycle: String = state
        .try_get("lifecycle_state")
        .map_err(map_statement_error)?;
    let current: Option<Uuid> = state
        .try_get("current_version_id")
        .map_err(map_statement_error)?;
    let ended: bool = state.try_get("ended").map_err(map_statement_error)?;
    let purpose = match lifecycle.as_str() {
        "WORKING" => VersionPurpose::Authoring,
        "PUBLISHED" if current == Some(version_id.as_uuid()) && !ended => VersionPurpose::Published,
        "PUBLISHED" | "WITHDRAWN" => VersionPurpose::History,
        _ => return Err(RepositoryError::IntegrityViolation),
    };
    let version = authorize_version_in_tx(
        tx,
        actor,
        VersionRequest {
            document_id: request.document_id,
            document_version_id: version_id,
            purpose,
        },
    )
    .await?;
    if version
        .try_get::<bool, _>("requires_content_classification")
        .map_err(map_statement_error)?
    {
        return Err(RepositoryError::BusinessRule);
    }
    let title: String = version.try_get("title").map_err(map_statement_error)?;
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
        .bind(request.document_id.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .map_err(map_statement_error)?;
    let document_revision =
        u64::try_from(revision).map_err(|_| RepositoryError::IntegrityViolation)?;
    let metadata_digest = digest(
        b"document-diff-version-metadata-v0\0",
        &serde_json::to_vec(&json!({
            "version_no": version.try_get::<i64, _>("version_no").map_err(map_statement_error)?,
            "lifecycle_state": lifecycle,
            "revision_reason": version.try_get::<Option<String>, _>("revision_reason").map_err(map_statement_error)?,
            "approved_at": version.try_get::<Option<time::OffsetDateTime>, _>("approved_at").map_err(map_statement_error)?,
            "scheduled_publish_at": version.try_get::<Option<time::OffsetDateTime>, _>("scheduled_publish_at").map_err(map_statement_error)?,
            "published_at": version.try_get::<Option<time::OffsetDateTime>, _>("published_at").map_err(map_statement_error)?,
            "withdrawn_at": version.try_get::<Option<time::OffsetDateTime>, _>("withdrawn_at").map_err(map_statement_error)?,
            "effective_from": version.try_get::<Option<time::OffsetDateTime>, _>("effective_from").map_err(map_statement_error)?,
            "effective_to": version.try_get::<Option<time::OffsetDateTime>, _>("effective_to").map_err(map_statement_error)?,
            "metadata": version.try_get::<serde_json::Value, _>("metadata").map_err(map_statement_error)?,
        }))
        .map_err(|_| RepositoryError::IntegrityViolation)?,
    );

    let rows = sqlx::query(
        "SELECT ci.content_item_id,ci.logical_path,ci.ordinal, \
                ci.authoritative_representation_id,cr.file_id,cr.detected_format, \
                cr.inspection_profile_version,cr.semantic_fingerprint, \
                f.content_hash,f.size_bytes \
         FROM content_items ci \
         JOIN content_representations cr \
           ON cr.content_item_id = ci.content_item_id \
          AND cr.content_representation_id = ci.authoritative_representation_id \
          AND cr.role = 'AUTHORITATIVE' \
         JOIN file_objects f ON f.file_id = cr.file_id \
         WHERE ci.document_version_id = $1 \
         ORDER BY ci.ordinal,ci.logical_path",
    )
    .bind(version_id.as_uuid())
    .fetch_all(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    if rows.is_empty() {
        return Err(RepositoryError::IntegrityViolation);
    }
    let file_ids: Vec<Uuid> = rows
        .iter()
        .map(|row| row.try_get("file_id").map_err(map_statement_error))
        .collect::<Result<_, _>>()?;
    let inspections = sqlx::query_as::<_, SemanticInspectionRow>(
        "SELECT s.file_id, s.inspection_profile_version, s.worker_protocol_version, \
                s.observed_raw_content_hash, s.observed_size_bytes, s.detected_format, \
                s.fingerprint_algorithm, s.fingerprint_digest, s.semantic_capabilities, \
                s.editorial_provenance, s.external_dependencies, s.digital_signature_evidence, \
                s.worker_build_id, s.adapter_id, s.adapter_version, s.parser_libraries, \
                s.native_dependency_identity, s.diagnostics, s.inspected_at, \
                f.content_hash AS authoritative_hash, f.size_bytes AS authoritative_size \
         FROM document_semantic_inspections s \
         JOIN file_objects f ON f.file_id = s.file_id \
         WHERE s.file_id = ANY($1::uuid[]) AND s.inspection_profile_version = 'dsi-v0'",
    )
    .bind(&file_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    let mut evidence = HashMap::new();
    for row in inspections {
        let file_id = row.file_id;
        evidence.insert(file_id, row.restore()?);
    }
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let file_id: Uuid = row.try_get("file_id").map_err(map_statement_error)?;
        let raw_sha256 = array32(row.try_get("content_hash").map_err(map_statement_error)?)?;
        let size = row
            .try_get::<i64, _>("size_bytes")
            .map_err(map_statement_error)?;
        let size_bytes = u64::try_from(size).map_err(|_| RepositoryError::IntegrityViolation)?;
        let stored_format: Option<String> = row
            .try_get("detected_format")
            .map_err(map_statement_error)?;
        let stored_profile: Option<String> = row
            .try_get("inspection_profile_version")
            .map_err(map_statement_error)?;
        let stored_fingerprint: Option<Vec<u8>> = row
            .try_get("semantic_fingerprint")
            .map_err(map_statement_error)?;
        let (format, fingerprint, binding) = if let Some(record) = evidence.get(&file_id) {
            let response = record.response();
            let expected_format = format_to_db(response.detected_format);
            if stored_format
                .as_deref()
                .is_some_and(|value| value != expected_format)
                || stored_profile
                    .as_deref()
                    .is_some_and(|value| value != InspectionProfileVersion::DsiV0.as_str())
                || stored_fingerprint
                    .as_ref()
                    .is_some_and(|value| value.as_slice() != response.semantic_fingerprint.digest())
                || response.observed_raw_content_hash != raw_sha256
                || response.observed_size_bytes != size_bytes
            {
                return Err(RepositoryError::IntegrityViolation);
            }
            let serialized =
                serde_json::to_vec(response).map_err(|_| RepositoryError::IntegrityViolation)?;
            (
                Some(response.detected_format),
                Some(*response.semantic_fingerprint.digest()),
                Some(digest(b"document-diff-dsi-binding-v0\0", &serialized)),
            )
        } else {
            if stored_profile
                .as_deref()
                .is_some_and(|value| value != InspectionProfileVersion::DsiV0.as_str())
            {
                return Err(RepositoryError::IntegrityViolation);
            }
            (
                stored_format.as_deref().map(format_from_db).transpose()?,
                None,
                None,
            )
        };
        let ordinal = u32::try_from(
            row.try_get::<i32, _>("ordinal")
                .map_err(map_statement_error)?,
        )
        .map_err(|_| RepositoryError::IntegrityViolation)?;
        items.push(SnapshotItem {
            content_item_id: row
                .try_get("content_item_id")
                .map_err(map_statement_error)?,
            logical_path: row.try_get("logical_path").map_err(map_statement_error)?,
            ordinal,
            authoritative_representation_id: row
                .try_get("authoritative_representation_id")
                .map_err(map_statement_error)?,
            file_id: FileId::from_uuid(file_id),
            format,
            inspection_profile: InspectionProfileVersion::DsiV0,
            semantic_fingerprint: fingerprint,
            inspection_binding_digest: binding,
            raw_sha256,
            size_bytes,
        });
    }
    let manifest_fingerprint = manifest_fingerprint(&title, &items);
    Ok(VersionSnapshot {
        document_id: request.document_id,
        version_id,
        document_revision,
        title,
        items,
        manifest_fingerprint,
        version_metadata_digest: metadata_digest,
    })
}

fn digest(domain: &[u8], data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(data);
    hasher.finalize().into()
}

fn array32(bytes: Vec<u8>) -> Result<[u8; 32], RepositoryError> {
    bytes
        .try_into()
        .map_err(|_| RepositoryError::IntegrityViolation)
}

fn format_from_db(value: &str) -> Result<FormatId, RepositoryError> {
    match value {
        "docx" => Ok(FormatId::Docx),
        "xlsx" => Ok(FormatId::Xlsx),
        "xlsm" => Ok(FormatId::Xlsm),
        "pptx" => Ok(FormatId::Pptx),
        "pdf" => Ok(FormatId::Pdf),
        "txt" => Ok(FormatId::Txt),
        "csv" => Ok(FormatId::Csv),
        "html" => Ok(FormatId::Html),
        _ => Err(RepositoryError::IntegrityViolation),
    }
}

fn format_to_db(value: FormatId) -> &'static str {
    crate::semantic_inspection_rows::format_to_db(value)
}
