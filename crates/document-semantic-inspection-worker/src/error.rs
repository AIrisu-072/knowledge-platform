use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerFailureCode {
    UnsupportedDocumentFormat,
    RequiresOcr,
    EncryptedContentUnsupported,
    FormatMismatch,
    RawBindingMismatch,
    SemanticExtractionFailed,
    InspectionTimeout,
    InspectionResourceLimitExceeded,
    ExtractorUnavailable,
    InvalidWorkerResult,
    ParserDisagreement,
    UnsupportedSemanticConstruct,
    MalformedRequest,
    WorkerPanicked,
}

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{code:?}: {message}")]
pub struct WorkerFailure {
    code: WorkerFailureCode,
    message: String,
}

impl WorkerFailure {
    pub fn new(code: WorkerFailureCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub const fn code(&self) -> WorkerFailureCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}
