use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid SHA-256 digest length: expected 32 bytes, observed {observed}")]
    InvalidSha256Length { observed: usize },

    #[error("worker result exceeds configured bound: observed {observed} bytes, limit {limit}")]
    ResultTooLarge { observed: usize, limit: usize },

    #[error("unknown worker protocol version: {0}")]
    UnknownProtocolVersion(String),

    #[error("invalid worker result: {0}")]
    InvalidWorkerResult(String),

    #[error("worker wire JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
