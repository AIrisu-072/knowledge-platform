use thiserror::Error;

use document_domain::{DocumentId, DocumentVersionId, DomainError, FileId};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StorageError {
    #[error("stored object not found")]
    NotFound,
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
    #[error("repository unavailable")]
    Unavailable,
    #[error("commit outcome is unknown")]
    CommitOutcomeUnknown {
        document_id: DocumentId,
        document_version_id: DocumentVersionId,
        file_id: FileId,
    },
    #[error("authoritative integrity violation")]
    IntegrityViolation,
    #[error("repository internal failure: {0}")]
    Internal(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApplicationError {
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("folder not found")]
    FolderNotFound,
    #[error("document not found")]
    DocumentNotFound,
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
    #[error("commit outcome is unknown")]
    CommitOutcomeUnknown,
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
            StorageError::NotFound => Self::IntegrityViolation,
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
            RepositoryError::Unavailable => Self::RepositoryUnavailable,
            RepositoryError::CommitOutcomeUnknown => Self::Internal(
                "commit outcome unknown outside create operation identity context".to_owned(),
            ),
            RepositoryError::IntegrityViolation => Self::IntegrityViolation,
            RepositoryError::Internal(message) => Self::Internal(message),
        }
    }
}
