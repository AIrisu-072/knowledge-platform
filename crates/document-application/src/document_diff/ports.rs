use std::future::Future;

use document_diff_core::{
    WorkerDiffRequest, WorkerDiffResponse, WorkerDisplayRequest, WorkerDisplayResponse,
};
use uuid::Uuid;

use crate::{ContentReader, RepositoryError, VerifiedActorContext};

use super::{DiffCacheKey, DiffPairSnapshot, DiffRequest, DiffResult};

pub trait DiffCache: Send + Sync {
    fn get(
        &self,
        key: &DiffCacheKey,
    ) -> impl Future<Output = Result<Option<DiffResult>, RepositoryError>> + Send;
    fn put(
        &self,
        key: DiffCacheKey,
        result: DiffResult,
        expected_digest: [u8; 32],
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}

pub trait DocumentDiffRepository: Send + Sync {
    fn capture_pair(
        &self,
        actor: &VerifiedActorContext,
        request: DiffRequest,
    ) -> impl Future<Output = Result<DiffPairSnapshot, RepositoryError>> + Send;

    fn authorize_and_audit_result(
        &self,
        actor: &VerifiedActorContext,
        pair: &DiffPairSnapshot,
        result: &DiffResult,
        cache_hit: bool,
        correlation_id: Option<&str>,
    ) -> impl Future<Output = Result<Uuid, RepositoryError>> + Send;
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

pub trait DiffExecutor: Send + Sync {
    fn compare(
        &self,
        request: WorkerDiffRequest,
        base: ContentReader,
        target: ContentReader,
    ) -> impl Future<Output = Result<WorkerDiffResponse, DiffExecutionError>> + Send;

    fn extract_display(
        &self,
        request: WorkerDisplayRequest,
        source: ContentReader,
    ) -> impl Future<Output = Result<WorkerDisplayResponse, DiffExecutionError>> + Send {
        let _ = (request, source);
        async { Err(DiffExecutionError::Unavailable) }
    }
}
