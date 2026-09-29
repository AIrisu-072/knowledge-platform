use document_diff_core::{WorkerDiffRequest, WorkerDiffResponse};
use uuid::Uuid;

use crate::{ContentReader, RepositoryError, VerifiedActorContext};

use super::{DiffCacheKey, DiffPairSnapshot, DiffRequest, DiffResult};

#[allow(async_fn_in_trait)]
pub trait DiffCache: Send + Sync {
    async fn get(&self, key: &DiffCacheKey) -> Result<Option<DiffResult>, RepositoryError>;
    async fn put(
        &self,
        key: DiffCacheKey,
        result: DiffResult,
        expected_digest: [u8; 32],
    ) -> Result<(), RepositoryError>;
}

#[allow(async_fn_in_trait)]
pub trait DocumentDiffRepository: Send + Sync {
    async fn capture_pair(
        &self,
        actor: &VerifiedActorContext,
        request: DiffRequest,
    ) -> Result<DiffPairSnapshot, RepositoryError>;

    async fn authorize_and_audit_result(
        &self,
        actor: &VerifiedActorContext,
        pair: &DiffPairSnapshot,
        result: &DiffResult,
        cache_hit: bool,
        correlation_id: Option<&str>,
    ) -> Result<Uuid, RepositoryError>;
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum DiffExecutionError {
    #[error("diff raw binding mismatch")]
    RawBindingMismatch,
    #[error("diff resource limit exceeded")]
    ResourceLimit,
    #[error("diff timed out")]
    Timeout,
    #[error("diff worker unavailable")]
    Unavailable,
    #[error("diff worker returned an invalid result")]
    InvalidWorkerResult,
}

#[allow(async_fn_in_trait)]
pub trait DiffExecutor: Send + Sync {
    async fn compare(
        &self,
        request: WorkerDiffRequest,
        base: ContentReader,
        target: ContentReader,
    ) -> Result<WorkerDiffResponse, DiffExecutionError>;
}
