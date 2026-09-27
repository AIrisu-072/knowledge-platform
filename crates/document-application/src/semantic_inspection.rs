use std::sync::Arc;

use document_domain::{FileId, FileObject};
use document_semantic_inspection_core::{
    CapabilityState, FingerprintAlgorithm, FormatId, InspectionProfileVersion,
    WorkerProtocolVersion, WorkerRequest, WorkerResponse,
};
use time::OffsetDateTime;

use crate::{
    ApplicationError, Clock, FileStorage, SemanticInspectionExecutor, SemanticInspectionRepository,
};

const MAX_STRUCTURED_RESULT_BYTES: usize = 16 * 1024 * 1024;

/// One immutable derived result for an authoritative FileObject and profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticInspectionRecord {
    file_id: FileId,
    response: WorkerResponse,
    inspected_at: OffsetDateTime,
}

impl SemanticInspectionRecord {
    /// Rehydrate typed persisted values. Application `ensure` separately checks
    /// the authoritative FileObject's current raw binding on every cache hit.
    pub fn restore(
        file_id: FileId,
        response: WorkerResponse,
        inspected_at: OffsetDateTime,
    ) -> Result<Self, ApplicationError> {
        response
            .validate()
            .map_err(|_| ApplicationError::InvalidWorkerResult)?;
        Ok(Self {
            file_id,
            response,
            inspected_at,
        })
    }

    pub const fn file_id(&self) -> FileId {
        self.file_id
    }

    pub fn response(&self) -> &WorkerResponse {
        &self.response
    }

    pub const fn inspected_at(&self) -> OffsetDateTime {
        self.inspected_at
    }
}

/// Cache-first Application orchestration. The executor receives no FileId,
/// storage key, principal, or persistence capability.
pub struct EnsureSemanticInspection<R, F, E, C> {
    repository: Arc<R>,
    storage: Arc<F>,
    executor: Arc<E>,
    clock: Arc<C>,
}

impl<R, F, E, C> EnsureSemanticInspection<R, F, E, C>
where
    R: SemanticInspectionRepository,
    F: FileStorage,
    E: SemanticInspectionExecutor,
    C: Clock,
{
    pub fn new(repository: Arc<R>, storage: Arc<F>, executor: Arc<E>, clock: Arc<C>) -> Self {
        Self {
            repository,
            storage,
            executor,
            clock,
        }
    }

    pub async fn ensure(
        &self,
        file_id: FileId,
        profile: InspectionProfileVersion,
    ) -> Result<SemanticInspectionRecord, ApplicationError> {
        validate_profile(profile)?;
        let file = self
            .repository
            .get_file_object(file_id)
            .await?
            .ok_or(ApplicationError::FileObjectNotFound)?;

        if let Some(cached) = self
            .repository
            .get_semantic_inspection(file_id, profile)
            .await?
        {
            validate_cached(&cached, &file, file_id, profile)?;
            return Ok(cached);
        }

        let content = self.storage.open(file.storage_key()).await?;
        let request = WorkerRequest {
            protocol_version: WorkerProtocolVersion::V0,
            inspection_profile_version: profile,
            declared_media_type: file.media_type().as_str().to_owned(),
            expected_raw_content_hash: *file.content_hash().as_bytes(),
            expected_size_bytes: file.size_bytes().get() as u64,
            trace_context: None,
        };
        let response = self.executor.inspect(request, content).await?;
        validate_worker_result(&response, &file, profile)?;

        let candidate = SemanticInspectionRecord::restore(file_id, response, self.clock.now())?;
        let persisted = self
            .repository
            .insert_or_converge_semantic_inspection(candidate.clone())
            .await?;
        validate_cached(&persisted, &file, file_id, profile)?;
        if persisted.response.detected_format != candidate.response.detected_format
            || persisted.response.semantic_fingerprint != candidate.response.semantic_fingerprint
            || persisted.response.semantic_capabilities != candidate.response.semantic_capabilities
            || persisted.response.editorial_provenance != candidate.response.editorial_provenance
            || persisted.response.external_dependencies != candidate.response.external_dependencies
            || persisted.response.digital_signature_evidence
                != candidate.response.digital_signature_evidence
        {
            return Err(ApplicationError::SemanticInspectionDeterminismViolation);
        }
        Ok(persisted)
    }
}

fn validate_profile(profile: InspectionProfileVersion) -> Result<(), ApplicationError> {
    if profile == InspectionProfileVersion::DsiV0 {
        Ok(())
    } else {
        Err(ApplicationError::Validation(
            "unsupported semantic inspection profile".to_owned(),
        ))
    }
}

fn validate_cached(
    record: &SemanticInspectionRecord,
    file: &FileObject,
    file_id: FileId,
    profile: InspectionProfileVersion,
) -> Result<(), ApplicationError> {
    if record.file_id != file_id
        || file.file_id() != file_id
        || record.response.inspection_profile_version != profile
        || record.response.observed_raw_content_hash != *file.content_hash().as_bytes()
        || record.response.observed_size_bytes != file.size_bytes().get() as u64
        || record.response.validate().is_err()
    {
        return Err(ApplicationError::IntegrityViolation);
    }
    Ok(())
}

fn validate_worker_result(
    response: &WorkerResponse,
    file: &FileObject,
    profile: InspectionProfileVersion,
) -> Result<(), ApplicationError> {
    if response.protocol_version != WorkerProtocolVersion::V0
        || response.inspection_profile_version != profile
    {
        return Err(ApplicationError::InvalidWorkerResult);
    }
    if response.observed_raw_content_hash != *file.content_hash().as_bytes()
        || response.observed_size_bytes != file.size_bytes().get() as u64
    {
        return Err(ApplicationError::IntegrityViolation);
    }
    if declared_format(file.media_type().as_str()) != Some(response.detected_format)
        || response.semantic_fingerprint.algorithm() != FingerprintAlgorithm::Sha256
        || response.validate().is_err()
    {
        return Err(ApplicationError::InvalidWorkerResult);
    }
    for capability in &response.semantic_capabilities {
        if (capability.presence == CapabilityState::Present)
            != capability.equivalence_fingerprint.is_some()
        {
            return Err(ApplicationError::InvalidWorkerResult);
        }
    }
    let result_bytes =
        serde_json::to_vec(response).map_err(|_| ApplicationError::InvalidWorkerResult)?;
    if result_bytes.len() > MAX_STRUCTURED_RESULT_BYTES {
        return Err(ApplicationError::InvalidWorkerResult);
    }
    Ok(())
}

fn declared_format(value: &str) -> Option<FormatId> {
    let normalized = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            Some(FormatId::Docx)
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some(FormatId::Xlsx),
        "application/vnd.ms-excel.sheet.macroenabled.12" => Some(FormatId::Xlsm),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            Some(FormatId::Pptx)
        }
        "application/pdf" => Some(FormatId::Pdf),
        "text/plain" => Some(FormatId::Txt),
        "text/csv" | "application/csv" => Some(FormatId::Csv),
        "text/html" | "application/xhtml+xml" => Some(FormatId::Html),
        _ => None,
    }
}
