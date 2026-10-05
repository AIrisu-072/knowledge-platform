//! Coverage and operation states from the P1 frozen failure matrix.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CoverageReason {
    RequiresOcr,
    UnsupportedFormat,
    Encrypted,
    UnsupportedStructure,
    UnsupportedEncoding,
    UnsupportedDialect,
    UnsupportedCodec,
    MissingFormulaCache,
    AmbiguousReadingOrder,
    DynamicVisibility,
    ResourceLimit,
}

impl CoverageReason {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::RequiresOcr => 1,
            Self::UnsupportedFormat => 2,
            Self::Encrypted => 3,
            Self::UnsupportedStructure => 4,
            Self::UnsupportedEncoding => 5,
            Self::UnsupportedDialect => 6,
            Self::UnsupportedCodec => 7,
            Self::MissingFormulaCache => 8,
            Self::AmbiguousReadingOrder => 9,
            Self::DynamicVisibility => 10,
            Self::ResourceLimit => 11,
        }
    }

    pub(crate) fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::RequiresOcr,
            2 => Self::UnsupportedFormat,
            3 => Self::Encrypted,
            4 => Self::UnsupportedStructure,
            5 => Self::UnsupportedEncoding,
            6 => Self::UnsupportedDialect,
            7 => Self::UnsupportedCodec,
            8 => Self::MissingFormulaCache,
            9 => Self::AmbiguousReadingOrder,
            10 => Self::DynamicVisibility,
            11 => Self::ResourceLimit,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyCoverage {
    Supported,
    Partial { reasons: Vec<CoverageReason> },
    Unsupported { reason: CoverageReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermanentFailureCode {
    CorruptDocument,
    MalformedArchive,
    TextExtractionFailed,
    WorkerOutputLimit,
}

impl PermanentFailureCode {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::CorruptDocument => 1,
            Self::MalformedArchive => 2,
            Self::TextExtractionFailed => 3,
            Self::WorkerOutputLimit => 4,
        }
    }

    pub(crate) fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::CorruptDocument,
            2 => Self::MalformedArchive,
            3 => Self::TextExtractionFailed,
            4 => Self::WorkerOutputLimit,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryableFailureCode {
    SourceIo,
    WorkerUnavailable,
    WorkerKilled,
    Timeout,
}

impl RetryableFailureCode {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::SourceIo => 1,
            Self::WorkerUnavailable => 2,
            Self::WorkerKilled => 3,
            Self::Timeout => 4,
        }
    }

    pub(crate) fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::SourceIo,
            2 => Self::WorkerUnavailable,
            3 => Self::WorkerKilled,
            4 => Self::Timeout,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemOperationState {
    Completed,
    FailedPermanent {
        code: PermanentFailureCode,
    },
    /// Journal only. This state must never be staged in a published manifest.
    Retryable {
        code: RetryableFailureCode,
    },
}

impl ItemOperationState {
    pub const fn publishable(self) -> bool {
        !matches!(self, Self::Retryable { .. })
    }
}
