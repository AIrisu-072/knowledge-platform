use thiserror::Error;

use document_domain::{DocumentId, DocumentVersionId, DomainError, FileId};

use crate::command::PublishOperationId;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StorageError {
    #[error("stored object not found")]
    NotFound,
    #[error("stored object cannot be opened")]
    ObjectUnreadable,
    #[error("storage write failed")]
    WriteFailed,
    #[error("storage sync failed")]
    SyncFailed,
    #[error("storage finalize failed")]
    FinalizeFailed,
    #[error("storage unavailable")]
    Unavailable,
    #[error("storage internal failure: {0}")]
    Internal(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RepositoryError {
    #[error("folder not found")]
    FolderNotFound,
    #[error("document not found")]
    DocumentNotFound,
    #[error("document version not found")]
    DocumentVersionNotFound,
    #[error("repository conflict")]
    Conflict,
    #[error("business rule rejected operation")]
    BusinessRule,
    #[error("repository unavailable")]
    Unavailable,
    #[error("commit outcome is unknown")]
    CommitOutcomeUnknown,
    #[error("authoritative integrity violation")]
    IntegrityViolation,
    #[error("semantic inspection determinism violation")]
    SemanticInspectionDeterminismViolation,
    #[error("repository internal failure: {0}")]
    Internal(String),
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum InspectionExecutionError {
    #[error("unsupported document format")]
    UnsupportedDocumentFormat,
    #[error("document requires OCR")]
    RequiresOcr,
    #[error("encrypted content is unsupported")]
    EncryptedContentUnsupported,
    #[error("declared and detected formats differ")]
    FormatMismatch,
    #[error("raw binding mismatch")]
    RawBindingMismatch,
    #[error("semantic extraction failed")]
    SemanticExtractionFailed,
    #[error("inspection timed out")]
    InspectionTimeout,
    #[error("inspection resource limit exceeded")]
    InspectionResourceLimitExceeded,
    #[error("extractor unavailable")]
    ExtractorUnavailable,
    #[error("invalid worker result")]
    InvalidWorkerResult,
    #[error("parser disagreement")]
    ParserDisagreement,
    #[error("unsupported semantic construct")]
    UnsupportedSemanticConstruct,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApplicationError {
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("folder not found")]
    FolderNotFound,
    #[error("document not found")]
    DocumentNotFound,
    #[error("document version not found")]
    DocumentVersionNotFound,
    #[error("file object not found")]
    FileObjectNotFound,
    #[error("operation conflicts with current authoritative state")]
    Conflict,
    #[error("business rule rejected operation")]
    BusinessRule,
    #[error("storage write failed")]
    StorageWriteFailed,
    #[error("storage sync failed")]
    StorageSyncFailed,
    #[error("storage finalize failed")]
    StorageFinalizeFailed,
    #[error("storage unavailable")]
    StorageUnavailable,
    #[error("repository unavailable")]
    RepositoryUnavailable,
    #[error("authoritative integrity violation")]
    IntegrityViolation,
    #[error("invalid semantic inspection worker result")]
    InvalidWorkerResult,
    #[error("semantic inspection determinism violation")]
    SemanticInspectionDeterminismViolation,
    #[error("semantic inspection failed: {0}")]
    InspectionFailed(InspectionExecutionError),
    #[error("commit outcome is unknown")]
    CommitOutcomeUnknown {
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
        file_id: FileId,
    },
    #[error("publish commit outcome is unknown")]
    PublishCommitOutcomeUnknown {
        publish_operation_id: PublishOperationId,
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
    },
    #[error("internal failure: {0}")]
    Internal(String),
}

impl From<DomainError> for ApplicationError {
    fn from(error: DomainError) -> Self {
        Self::Validation(error.to_string())
    }
}

impl From<StorageError> for ApplicationError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::NotFound | StorageError::ObjectUnreadable => Self::IntegrityViolation,
            StorageError::WriteFailed => Self::StorageWriteFailed,
            StorageError::SyncFailed => Self::StorageSyncFailed,
            StorageError::FinalizeFailed => Self::StorageFinalizeFailed,
            StorageError::Unavailable => Self::StorageUnavailable,
            StorageError::Internal(message) => Self::Internal(message),
        }
    }
}

impl From<RepositoryError> for ApplicationError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::FolderNotFound => Self::FolderNotFound,
            RepositoryError::DocumentNotFound => Self::DocumentNotFound,
            RepositoryError::DocumentVersionNotFound => Self::DocumentVersionNotFound,
            RepositoryError::Conflict => Self::Conflict,
            RepositoryError::BusinessRule => Self::BusinessRule,
            RepositoryError::Unavailable => Self::RepositoryUnavailable,
            RepositoryError::CommitOutcomeUnknown => Self::Internal(
                "commit outcome unknown outside create operation identity context".to_owned(),
            ),
            RepositoryError::IntegrityViolation => Self::IntegrityViolation,
            RepositoryError::SemanticInspectionDeterminismViolation => {
                Self::SemanticInspectionDeterminismViolation
            }
            RepositoryError::Internal(message) => Self::Internal(message),
        }
    }
}

impl From<InspectionExecutionError> for ApplicationError {
    fn from(error: InspectionExecutionError) -> Self {
        match error {
            InspectionExecutionError::RawBindingMismatch => Self::IntegrityViolation,
            InspectionExecutionError::InvalidWorkerResult => Self::InvalidWorkerResult,
            other => Self::InspectionFailed(other),
        }
    }
}
