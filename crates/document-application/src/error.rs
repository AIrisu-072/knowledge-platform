use thiserror::Error;

use document_domain::{DocumentId, DocumentVersionId, DomainError, FileId};

use crate::command::PublishOperationId;
use crate::management_command::{ManagementErrorCode, ManagementOperationId};
use crate::publication_end::PublicationEndOperationId;
use crate::versioning_command::VersionOperationId;

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
    #[error("access denied")]
    Forbidden,
    #[error("folder not found")]
    FolderNotFound,
    #[error("document not found")]
    DocumentNotFound,
    #[error("document version not found")]
    DocumentVersionNotFound,
    #[error("file object not found")]
    FileObjectNotFound,
    #[error("target version is no longer the current published version")]
    StaleVersion,
    #[error("query cursor no longer matches the current access or query context")]
    CursorStale,
    #[error("document diff input changed during comparison")]
    StaleComparisonInput,
    #[error("invalid query cursor")]
    InvalidCursor,
    #[error("repository conflict")]
    Conflict,
    #[error("business rule rejected operation")]
    BusinessRule,
    #[error("management operation rejected: {0:?}")]
    Management(ManagementErrorCode),
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
    #[error("access denied")]
    Forbidden,
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("folder not found")]
    FolderNotFound,
    #[error("document not found")]
    DocumentNotFound,
    #[error("document version not found")]
    DocumentVersionNotFound,
    #[error("target version is no longer the current published version")]
    StaleVersion,
    #[error("query cursor no longer matches the current access or query context")]
    CursorStale,
    #[error("document diff input changed during comparison")]
    StaleComparisonInput,
    #[error("file object not found")]
    FileObjectNotFound,
    #[error("operation conflicts with current authoritative state")]
    Conflict,
    #[error("business rule rejected operation")]
    BusinessRule,
    #[error("management operation rejected: {0:?}")]
    Management(ManagementErrorCode),
    #[error("publication quality rejected content: {0}")]
    PublishQualityRejected(String),
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
    #[error("schedule commit outcome is unknown; retry the same publish operation id")]
    ScheduleCommitOutcomeUnknown {
        publish_operation_id: PublishOperationId,
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
    },
    #[error("version commit outcome is unknown; retry the same operation id")]
    VersionCommitOutcomeUnknown {
        operation_id: VersionOperationId,
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
    },
    #[error("publication end commit outcome is unknown; retry the same operation id")]
    PublicationEndCommitOutcomeUnknown {
        operation_id: PublicationEndOperationId,
        document_id: DocumentId,
    },
    #[error("management commit outcome is unknown; retry the same operation id")]
    ManagementCommitOutcomeUnknown { operation_id: ManagementOperationId },
    #[error("read confirmation commit outcome is unknown; retry the same version")]
    ReadStateCommitOutcomeUnknown {
        document_version_id: DocumentVersionId,
    },
    #[error("file access audit commit outcome is unknown; content was not opened")]
    FileAccessAuditCommitOutcomeUnknown {
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
            RepositoryError::Forbidden => Self::Forbidden,
            RepositoryError::FolderNotFound => Self::FolderNotFound,
            RepositoryError::DocumentNotFound => Self::DocumentNotFound,
            RepositoryError::DocumentVersionNotFound => Self::DocumentVersionNotFound,
            RepositoryError::FileObjectNotFound => Self::FileObjectNotFound,
            RepositoryError::StaleVersion => Self::StaleVersion,
            RepositoryError::CursorStale => Self::CursorStale,
            RepositoryError::StaleComparisonInput => Self::StaleComparisonInput,
            RepositoryError::InvalidCursor => Self::Validation("invalid query cursor".into()),
            RepositoryError::Conflict => Self::Conflict,
            RepositoryError::BusinessRule => Self::BusinessRule,
            RepositoryError::Management(code) => Self::Management(code),
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
