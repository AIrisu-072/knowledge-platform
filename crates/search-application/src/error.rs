//! Errors returned by Search application ports.

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("invalid search request: {0}")]
    InvalidRequest(String),
    #[error("search source unavailable: {0}")]
    SourceUnavailable(String),
    #[error("search operation failed: {0}")]
    OperationFailed(String),
    #[error("search delivery fence lost")]
    FenceLost,
    #[error("search completion outcome unknown")]
    CompletionUnknown,
}
