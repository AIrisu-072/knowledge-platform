use document_application::{
    RepositoryError, SemanticInspectionRecord, SemanticInspectionRepository,
};
use document_domain::{FileId, FileObject};
use document_semantic_inspection_core::{
    InspectionProfileVersion, WorkerProtocolVersion, WorkerResponse,
};
use serde::Serialize;
use serde_json::Value;

use crate::{
    error::map_statement_error,
    repository::PostgresDocumentRepository,
    semantic_inspection_rows::{FileObjectRow, SemanticInspectionRow, format_to_db},
};

const MAX_RESULT_BYTES: usize = 16 * 1024 * 1024;

impl SemanticInspectionRepository for PostgresDocumentRepository {
    async fn get_file_object(
        &self,
        file_id: FileId,
    ) -> Result<Option<FileObject>, RepositoryError> {
        let row = sqlx::query_as::<_, FileObjectRow>(
            "SELECT file_id, content_hash, media_type, size_bytes, storage_locator, created_at \
             FROM file_objects WHERE file_id = $1",
        )
        .bind(file_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_statement_error)?;
        row.map(FileObjectRow::restore).transpose()
    }

    async fn get_semantic_inspection(
        &self,
        file_id: FileId,
        profile: InspectionProfileVersion,
    ) -> Result<Option<SemanticInspectionRecord>, RepositoryError> {
        let row = sqlx::query_as::<_, SemanticInspectionRow>(
            "SELECT s.file_id, s.inspection_profile_version, s.worker_protocol_version, \
                 s.observed_raw_content_hash, s.observed_size_bytes, s.detected_format, \
                 s.fingerprint_algorithm, s.fingerprint_digest, s.semantic_capabilities, \
                 s.editorial_provenance, s.external_dependencies, s.digital_signature_evidence, \
                 s.worker_build_id, s.adapter_id, s.adapter_version, s.parser_libraries, \
                 s.native_dependency_identity, s.diagnostics, s.inspected_at, \
                 f.content_hash AS authoritative_hash, f.size_bytes AS authoritative_size \
             FROM document_semantic_inspections s \
             JOIN file_objects f ON f.file_id = s.file_id \
             WHERE s.file_id = $1 AND s.inspection_profile_version = $2",
        )
        .bind(file_id.as_uuid())
        .bind(profile.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_statement_error)?;
        row.map(SemanticInspectionRow::restore).transpose()
    }

    async fn insert_or_converge_semantic_inspection(
        &self,
        record: SemanticInspectionRecord,
    ) -> Result<SemanticInspectionRecord, RepositoryError> {
        let response = record.response();
        let file = self
            .get_file_object(record.file_id())
            .await?
            .ok_or(RepositoryError::IntegrityViolation)?;
        if response.protocol_version != WorkerProtocolVersion::V0
            || response.inspection_profile_version != InspectionProfileVersion::DsiV0
            || response.observed_raw_content_hash != *file.content_hash().as_bytes()
            || response.observed_size_bytes != file.size_bytes().get() as u64
            || response.validate().is_err()
        {
            return Err(RepositoryError::IntegrityViolation);
        }
        let serialized =
            serde_json::to_vec(response).map_err(|_| RepositoryError::IntegrityViolation)?;
        if serialized.len() > MAX_RESULT_BYTES {
            return Err(RepositoryError::IntegrityViolation);
        }
        let size = i64::try_from(response.observed_size_bytes)
            .map_err(|_| RepositoryError::IntegrityViolation)?;

        sqlx::query(
            "INSERT INTO document_semantic_inspections (
                file_id, inspection_profile_version, worker_protocol_version,
                observed_raw_content_hash, observed_size_bytes, detected_format,
                fingerprint_algorithm, fingerprint_digest, semantic_capabilities,
                editorial_provenance, external_dependencies, digital_signature_evidence,
                worker_build_id, adapter_id, adapter_version, parser_libraries,
                native_dependency_identity, diagnostics, inspected_at
             ) VALUES (
                $1, $2, $3, $4, $5, $6, 'sha256', $7, $8, $9, $10, $11,
                $12, $13, $14, $15, $16, $17, $18
             ) ON CONFLICT (file_id, inspection_profile_version) DO NOTHING",
        )
        .bind(record.file_id().as_uuid())
        .bind(response.inspection_profile_version.as_str())
        .bind(response.protocol_version.as_str())
        .bind(response.observed_raw_content_hash.to_vec())
        .bind(size)
        .bind(format_to_db(response.detected_format))
        .bind(response.semantic_fingerprint.digest().to_vec())
        .bind(json(&response.semantic_capabilities)?)
        .bind(json(&response.editorial_provenance)?)
        .bind(json(&response.external_dependencies)?)
        .bind(json(&response.digital_signature_evidence)?)
        .bind(&response.extractor_provenance.worker_build_id)
        .bind(&response.extractor_provenance.adapter_id)
        .bind(&response.extractor_provenance.adapter_version)
        .bind(json(&response.extractor_provenance.parser_libraries)?)
        .bind(json(
            &response.extractor_provenance.native_dependency_identity,
        )?)
        .bind(json(&response.diagnostics)?)
        .bind(record.inspected_at())
        .execute(&self.pool)
        .await
        .map_err(map_statement_error)?;

        let persisted = self
            .get_semantic_inspection(record.file_id(), response.inspection_profile_version)
            .await?
            .ok_or(RepositoryError::IntegrityViolation)?;
        if !same_deterministic_result(persisted.response(), response) {
            return Err(RepositoryError::SemanticInspectionDeterminismViolation);
        }
        Ok(persisted)
    }
}

fn json<T: Serialize>(value: &T) -> Result<Value, RepositoryError> {
    serde_json::to_value(value).map_err(|_| RepositoryError::IntegrityViolation)
}

fn same_deterministic_result(a: &WorkerResponse, b: &WorkerResponse) -> bool {
    a.protocol_version == b.protocol_version
        && a.inspection_profile_version == b.inspection_profile_version
        && a.observed_raw_content_hash == b.observed_raw_content_hash
        && a.observed_size_bytes == b.observed_size_bytes
        && a.detected_format == b.detected_format
        && a.semantic_fingerprint == b.semantic_fingerprint
        && a.semantic_capabilities == b.semantic_capabilities
        && a.editorial_provenance == b.editorial_provenance
        && a.external_dependencies == b.external_dependencies
        && a.digital_signature_evidence == b.digital_signature_evidence
}
