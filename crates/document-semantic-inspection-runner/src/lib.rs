//! Trusted process boundary for Document Semantic Inspection v0.
//!
//! The runner receives authoritative bytes and metadata from the Application.
//! A worker receives only a minimal request, read-only input/trust descriptors,
//! and a private scratch directory. All errors deliberately omit document data.

use std::{path::PathBuf, time::Duration};

use document_semantic_inspection_core::{TraceContext, WorkerResponse};
use thiserror::Error;

mod executor;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod sandbox;

pub use executor::RunnerInspectionExecutor;

#[cfg(target_os = "linux")]
const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_REQUEST_BYTES: usize = 64 * 1024;
#[cfg(target_os = "linux")]
const MAX_RESULT_BYTES: usize = 16 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_STDERR_BYTES: usize = 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_TRUST_BYTES: usize = 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_TEMP_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_WALL_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct RunnerConfig {
    worker_executable: PathBuf,
    signature_trust_bundle: Option<Vec<u8>>,
    pdfium_runtime_dir: Option<PathBuf>,
    wall_timeout: Duration,
}

impl RunnerConfig {
    pub fn new(worker_executable: impl Into<PathBuf>) -> Self {
        Self {
            worker_executable: worker_executable.into(),
            signature_trust_bundle: None,
            pdfium_runtime_dir: None,
            wall_timeout: MAX_WALL_TIMEOUT,
        }
    }

    pub fn with_signature_trust_bundle(mut self, bundle: Vec<u8>) -> Self {
        self.signature_trust_bundle = Some(bundle);
        self
    }

    pub fn with_pdfium_runtime_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.pdfium_runtime_dir = Some(directory.into());
        self
    }

    /// Only a shorter timeout is permitted; the frozen production ceiling is 10 seconds.
    pub fn with_wall_timeout(mut self, timeout: Duration) -> Self {
        self.wall_timeout = timeout;
        self
    }
}

pub struct RunnerInput<'a> {
    pub bytes: &'a [u8],
    pub declared_media_type: &'a str,
    pub expected_raw_content_hash: [u8; 32],
    pub expected_size_bytes: u64,
    pub trace_context: Option<TraceContext>,
}

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("inspection exceeded its wall-clock limit")]
    InspectionTimeout { elapsed_ms: u64 },
    #[error("inspection exceeded a resource limit")]
    InspectionResourceLimitExceeded { reason: &'static str },
    #[error("inspection extractor or mandatory sandbox is unavailable")]
    ExtractorUnavailable { reason: &'static str },
    #[error("worker returned an invalid result")]
    InvalidWorkerResult { reason: &'static str },
    #[error("authoritative raw binding did not match")]
    RawBindingMismatch { reason: &'static str },
    #[error("inspection failed with a classified worker error")]
    WorkerFailure { code: &'static str },
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
            Err(RunnerError::ExtractorUnavailable {
                reason: "Linux sandbox is required",
            })
        }
        #[cfg(target_os = "linux")]
        {
            linux::validate_config(&config)?;
            Ok(Self { config })
        }
    }

    pub fn inspect(&self, input: RunnerInput<'_>) -> Result<WorkerResponse, RunnerError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (self, input);
            Err(RunnerError::ExtractorUnavailable {
                reason: "Linux sandbox is required",
            })
        }
        #[cfg(target_os = "linux")]
        {
            linux::inspect(&self.config, input)
        }
    }
}

/// Seal the current trusted worker after native library initialization.
/// The production runner always sets `DSI_SANDBOX_REQUIRED=1`; direct semantic
/// parity tests may run the portable worker without invoking this function.
pub fn seal_worker_sandbox() -> Result<(), RunnerError> {
    #[cfg(target_os = "linux")]
    {
        sandbox::seal()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(RunnerError::ExtractorUnavailable {
            reason: "Linux sandbox is required",
        })
    }
}
