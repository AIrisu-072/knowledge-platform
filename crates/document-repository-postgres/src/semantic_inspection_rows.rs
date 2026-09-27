use document_application::{RepositoryError, SemanticInspectionRecord};
use document_domain::{
    ContentHash, FileId, FileObject, FileSize, MediaType, StorageKey, StoredFileDescriptor,
};
use document_semantic_inspection_core::{
    ExtractorProvenance, FormatId, InspectionProfileVersion, SemanticFingerprint,
    WorkerProtocolVersion, WorkerResponse,
};
use serde_json::Value;
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub(crate) struct FileObjectRow {
    pub file_id: Uuid,
    pub content_hash: Vec<u8>,
    pub media_type: String,
    pub size_bytes: i64,
    pub storage_locator: String,
    pub created_at: OffsetDateTime,
}

impl FileObjectRow {
    pub fn restore(self) -> Result<FileObject, RepositoryError> {
        let key = StorageKey::new(self.storage_locator)
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        let hash = ContentHash::from_slice(&self.content_hash)
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        let size =
            FileSize::new(self.size_bytes).map_err(|_| RepositoryError::IntegrityViolation)?;
        let media_type =
            MediaType::new(self.media_type).map_err(|_| RepositoryError::IntegrityViolation)?;
        Ok(FileObject::restore(
            FileId::from_uuid(self.file_id),
            StoredFileDescriptor::new(key, hash, size, media_type),
            self.created_at,
        ))
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct SemanticInspectionRow {
    pub file_id: Uuid,
    pub inspection_profile_version: String,
    pub worker_protocol_version: String,
    pub observed_raw_content_hash: Vec<u8>,
    pub observed_size_bytes: i64,
    pub detected_format: String,
    pub fingerprint_algorithm: String,
    pub fingerprint_digest: Vec<u8>,
    pub semantic_capabilities: Value,
    pub editorial_provenance: Value,
    pub external_dependencies: Value,
    pub digital_signature_evidence: Value,
    pub worker_build_id: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub parser_libraries: Value,
    pub native_dependency_identity: Value,
    pub diagnostics: Value,
    pub inspected_at: OffsetDateTime,
    pub authoritative_hash: Vec<u8>,
    pub authoritative_size: i64,
}

impl SemanticInspectionRow {
    pub fn restore(self) -> Result<SemanticInspectionRecord, RepositoryError> {
        if self.inspection_profile_version != InspectionProfileVersion::DsiV0.as_str()
            || self.worker_protocol_version != WorkerProtocolVersion::V0.as_str()
            || self.fingerprint_algorithm != "sha256"
            || self.observed_raw_content_hash != self.authoritative_hash
            || self.observed_size_bytes != self.authoritative_size
        {
            return Err(RepositoryError::IntegrityViolation);
        }
        let raw_hash: [u8; 32] = self
            .observed_raw_content_hash
            .try_into()
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        let size = u64::try_from(self.observed_size_bytes)
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        let fingerprint = SemanticFingerprint::sha256_from_slice(&self.fingerprint_digest)
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        let response = WorkerResponse {
            protocol_version: WorkerProtocolVersion::V0,
            inspection_profile_version: InspectionProfileVersion::DsiV0,
            observed_raw_content_hash: raw_hash,
            observed_size_bytes: size,
            detected_format: format_from_db(&self.detected_format)?,
            semantic_fingerprint: fingerprint,
            semantic_capabilities: decode(self.semantic_capabilities)?,
            editorial_provenance: decode(self.editorial_provenance)?,
            external_dependencies: decode(self.external_dependencies)?,
            digital_signature_evidence: decode(self.digital_signature_evidence)?,
            extractor_provenance: ExtractorProvenance {
                worker_build_id: self.worker_build_id,
                adapter_id: self.adapter_id,
                adapter_version: self.adapter_version,
                parser_libraries: decode(self.parser_libraries)?,
                native_dependency_identity: decode(self.native_dependency_identity)?,
            },
            diagnostics: decode(self.diagnostics)?,
        };
        let bytes =
            serde_json::to_vec(&response).map_err(|_| RepositoryError::IntegrityViolation)?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(RepositoryError::IntegrityViolation);
        }
        SemanticInspectionRecord::restore(
            FileId::from_uuid(self.file_id),
            response,
            self.inspected_at,
        )
        .map_err(|_| RepositoryError::IntegrityViolation)
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, RepositoryError> {
    serde_json::from_value(value).map_err(|_| RepositoryError::IntegrityViolation)
}

pub(crate) const fn format_to_db(format: FormatId) -> &'static str {
    match format {
        FormatId::Docx => "docx",
        FormatId::Xlsx => "xlsx",
        FormatId::Xlsm => "xlsm",
        FormatId::Pptx => "pptx",
        FormatId::Pdf => "pdf",
        FormatId::Txt => "txt",
        FormatId::Csv => "csv",
        FormatId::Html => "html",
    }
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
