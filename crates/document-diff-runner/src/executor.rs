use std::sync::Arc;

use document_application::{
    ContentReader,
    document_diff::{DiffExecutionError, DiffExecutor},
};
use document_diff_core::{WorkerDiffRequest, WorkerDiffResponse};
use tokio::io::AsyncReadExt;

use crate::{LinuxSandboxRunner, MAX_SOURCE_BYTES, RunnerConfig, RunnerError};

pub struct RunnerDiffExecutor {
    runner: Arc<LinuxSandboxRunner>,
}

impl RunnerDiffExecutor {
    pub fn new(config: RunnerConfig) -> Result<Self, RunnerError> {
        Ok(Self {
            runner: Arc::new(LinuxSandboxRunner::new(config)?),
        })
    }
}

impl DiffExecutor for RunnerDiffExecutor {
    async fn compare(
        &self,
        request: WorkerDiffRequest,
        base: ContentReader,
        target: ContentReader,
    ) -> Result<WorkerDiffResponse, DiffExecutionError> {
        let base = read_bounded(base).await?;
        let target = read_bounded(target).await?;
        let runner = self.runner.clone();
        tokio::task::spawn_blocking(move || runner.compare(request, &base, &target))
            .await
            .map_err(|_| DiffExecutionError::Unavailable)?
            .map_err(map_error)
    }
}

async fn read_bounded(content: ContentReader) -> Result<Vec<u8>, DiffExecutionError> {
    let mut bounded = content.take((MAX_SOURCE_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    bounded
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| DiffExecutionError::Unavailable)?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(DiffExecutionError::ResourceLimit);
    }
    Ok(bytes)
}

fn map_error(error: RunnerError) -> DiffExecutionError {
    match error {
        RunnerError::Timeout => DiffExecutionError::Timeout,
        RunnerError::ResourceLimit(_) => DiffExecutionError::ResourceLimit,
        RunnerError::Unavailable(_) => DiffExecutionError::Unavailable,
        RunnerError::InvalidResult(_) => DiffExecutionError::InvalidWorkerResult,
        RunnerError::RawBindingMismatch => DiffExecutionError::RawBindingMismatch,
    }
}
