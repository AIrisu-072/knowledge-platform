use serde::{Deserialize, Serialize};

/// Typed safe outcome codes of the bounded Runtime Contract.
///
/// They never carry physical paths or raw OS diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorCode {
    Unavailable,
    InvalidLocator,
    Denied,
    StaleContext,
    NotFound,
    Conflict,
    Limit,
    Cancelled,
    OutcomeUnknown,
}

impl RuntimeErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::InvalidLocator => "invalid_locator",
            Self::Denied => "denied",
            Self::StaleContext => "stale_context",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Limit => "limit",
            Self::Cancelled => "cancelled",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }
}

/// Safe, path-free reason detail for presentation. The set is closed so a
/// caller can never smuggle OS text through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorReason {
    UnsupportedPlatform,
    InstanceLocked,
    RegistryUnreadable,
    RegistryWriteFailed,
    FolderReplaced,
    SymbolicLink,
    LinkedFile,
    SpecialFile,
    ProtectedLocation,
    ManagedBinding,
    AlreadyBound,
    AlreadyExists,
    ConcurrentChange,
    SafeCaptureUnavailable,
    OperationMismatch,
    PickerBusy,
    SelectionExpired,
    HandleExpired,
    TooLarge,
    TooMany,
    InvalidName,
    InvalidCursor,
    InvalidRequest,
    UnknownCommand,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{}", code.as_str())]
pub struct RuntimeError {
    pub code: RuntimeErrorCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<RuntimeErrorReason>,
}

impl RuntimeError {
    pub const fn new(code: RuntimeErrorCode) -> Self {
        Self { code, reason: None }
    }

    pub const fn with(code: RuntimeErrorCode, reason: RuntimeErrorReason) -> Self {
        Self {
            code,
            reason: Some(reason),
        }
    }
}

pub type RuntimeResult<T> = Result<T, RuntimeError>;

pub(crate) fn err<T>(code: RuntimeErrorCode, reason: RuntimeErrorReason) -> RuntimeResult<T> {
    Err(RuntimeError::with(code, reason))
}
