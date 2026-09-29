//! Trusted two-source Document Diff sandbox runner.

use std::{path::PathBuf, time::Duration};

use document_diff_core::{WorkerDiffRequest, WorkerDiffResponse};
use thiserror::Error;

mod executor;
#[cfg(target_os = "linux")]
mod linux;

pub use executor::RunnerDiffExecutor;

pub const MAX_SOURCE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_STDERR_BYTES: usize = 1024 * 1024;
pub const MAX_TEMP_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_WALL_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    worker_executable: PathBuf,
    wall_timeout: Duration,
}

impl RunnerConfig {
    pub fn new(worker_executable: impl Into<PathBuf>) -> Self {
        Self {
            worker_executable: worker_executable.into(),
            wall_timeout: MAX_WALL_TIMEOUT,
        }
    }

    pub fn with_wall_timeout(mut self, timeout: Duration) -> Self {
        self.wall_timeout = timeout;
        self
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RunnerError {
    #[error("document diff timed out")]
    Timeout,
    #[error("document diff resource limit: {0}")]
    ResourceLimit(&'static str),
    #[error("document diff sandbox unavailable: {0}")]
    Unavailable(&'static str),
    #[error("document diff worker result invalid: {0}")]
    InvalidResult(&'static str),
    #[error("document diff raw binding mismatch")]
    RawBindingMismatch,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct LinuxSandboxRunner {
    config: RunnerConfig,
}

impl LinuxSandboxRunner {
    pub fn new(config: RunnerConfig) -> Result<Self, RunnerError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            Err(RunnerError::Unavailable("Linux sandbox is required"))
        }
        #[cfg(target_os = "linux")]
        {
            linux::validate_config(&config)?;
            Ok(Self { config })
        }
    }

    pub fn compare(
        &self,
        request: WorkerDiffRequest,
        base: &[u8],
        target: &[u8],
    ) -> Result<WorkerDiffResponse, RunnerError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (self, request, base, target);
            Err(RunnerError::Unavailable("Linux sandbox is required"))
        }
        #[cfg(target_os = "linux")]
        {
            linux::compare(&self.config, request, base, target)
        }
    }
}
