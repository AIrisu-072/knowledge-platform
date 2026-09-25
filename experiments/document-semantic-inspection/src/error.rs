use crate::FormatId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidManifest,
    RawBindingMismatch,
    FormatMismatch,
    UnsupportedDocumentFormat,
    RequiresOcr,
    EncryptedContentUnsupported,
    UnsupportedSemanticConstruct,
    SemanticExtractionFailed,
    ParserDisagreement,
    InspectionTimeout,
    InspectionResourceLimitExceeded,
    ExtractorUnavailable,
    InvalidWorkerResult,
    SemanticInspectionDeterminismViolation,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PocError {
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("raw binding mismatch for {path}")]
    RawBindingMismatch {
        path: String,
        expected_sha256: String,
        observed_sha256: String,
        expected_size: u64,
        observed_size: u64,
    },

    #[error("format mismatch: expected {expected:?}, observed {observed:?}")]
    FormatMismatch {
        expected: FormatId,
        observed: Option<FormatId>,
    },

    #[error("unsupported document format: {0}")]
    UnsupportedDocumentFormat(String),

    #[error("requires OCR")]
    RequiresOcr,

    #[error("encrypted content is unsupported")]
    EncryptedContentUnsupported,

    #[error("unsupported semantic construct: {0}")]
    UnsupportedSemanticConstruct(String),

    #[error("semantic extraction failed: {0}")]
    SemanticExtractionFailed(String),

    #[error("parser disagreement: {0}")]
    ParserDisagreement(String),

    #[error("inspection timeout")]
    InspectionTimeout,

    #[error("inspection resource limit exceeded")]
    InspectionResourceLimitExceeded,

    #[error("extractor unavailable: {0}")]
    ExtractorUnavailable(String),

    #[error("invalid worker result: {0}")]
    InvalidWorkerResult(String),

    #[error("semantic inspection determinism violation: {0}")]
    SemanticInspectionDeterminismViolation(String),
}

impl PocError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidManifest(_) => ErrorCode::InvalidManifest,
            Self::RawBindingMismatch { .. } => ErrorCode::RawBindingMismatch,
            Self::FormatMismatch { .. } => ErrorCode::FormatMismatch,
            Self::UnsupportedDocumentFormat(_) => ErrorCode::UnsupportedDocumentFormat,
            Self::RequiresOcr => ErrorCode::RequiresOcr,
            Self::EncryptedContentUnsupported => ErrorCode::EncryptedContentUnsupported,
            Self::UnsupportedSemanticConstruct(_) => ErrorCode::UnsupportedSemanticConstruct,
            Self::SemanticExtractionFailed(_) => ErrorCode::SemanticExtractionFailed,
            Self::ParserDisagreement(_) => ErrorCode::ParserDisagreement,
            Self::InspectionTimeout => ErrorCode::InspectionTimeout,
            Self::InspectionResourceLimitExceeded => ErrorCode::InspectionResourceLimitExceeded,
            Self::ExtractorUnavailable(_) => ErrorCode::ExtractorUnavailable,
            Self::InvalidWorkerResult(_) => ErrorCode::InvalidWorkerResult,
            Self::SemanticInspectionDeterminismViolation(_) => {
                ErrorCode::SemanticInspectionDeterminismViolation
            }
        }
    }
}
