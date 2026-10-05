//! Shared process boundary for DSI and Search extraction.

use std::{
    fmt,
    path::{Path, PathBuf},
    process::ExitStatus,
    time::Duration,
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod process;
mod sandbox;

pub use sandbox::seal_worker_sandbox;

pub const MAX_WALL_TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxRunErrorKind {
    Unavailable,
    InvalidRequest,
    ResourceLimit,
    OutputLimit,
    Timeout,
    WorkerExited,
}

pub struct SandboxRunError {
    kind: SandboxRunErrorKind,
    reason: &'static str,
    elapsed_ms: Option<u64>,
    worker_status: Option<ExitStatus>,
    worker_stderr: Vec<u8>,
}

impl SandboxRunError {
    pub fn kind(&self) -> SandboxRunErrorKind {
        self.kind
    }
    pub fn elapsed_ms(&self) -> Option<u64> {
        self.elapsed_ms
    }
    pub fn worker_failure(&self) -> Option<(ExitStatus, &[u8])> {
        self.worker_status
            .map(|status| (status, self.worker_stderr.as_slice()))
    }
    pub(crate) fn new(kind: SandboxRunErrorKind, reason: &'static str) -> Self {
        Self {
            kind,
            reason,
            elapsed_ms: None,
            worker_status: None,
            worker_stderr: Vec::new(),
        }
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn timeout(elapsed_ms: u64) -> Self {
        Self {
            elapsed_ms: Some(elapsed_ms),
            ..Self::new(SandboxRunErrorKind::Timeout, "worker timeout")
        }
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn exited(status: ExitStatus, stderr: Vec<u8>) -> Self {
        Self {
            worker_status: Some(status),
            worker_stderr: stderr,
            ..Self::new(SandboxRunErrorKind::WorkerExited, "worker exited")
        }
    }
}

impl fmt::Debug for SandboxRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxRunError")
            .field("kind", &self.kind)
            .field("reason", &self.reason)
            .field("elapsed_ms", &self.elapsed_ms)
            .finish()
    }
}

impl fmt::Display for SandboxRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sandbox run failed: {}", self.reason)
    }
}
impl std::error::Error for SandboxRunError {}

#[derive(Debug, Clone, Copy)]
pub enum SandboxWorkload {
    Dsi,
    Search,
}

#[derive(Debug, Clone)]
pub struct SandboxRunConfig {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    workload: SandboxWorkload,
    wall_timeout: Duration,
    signature_trust_bundle: Option<Vec<u8>>,
    pdfium_runtime_dir: Option<PathBuf>,
}

impl SandboxRunConfig {
    pub fn for_dsi() -> Self {
        Self::new(SandboxWorkload::Dsi)
    }
    pub fn for_search() -> Self {
        Self::new(SandboxWorkload::Search)
    }
    fn new(workload: SandboxWorkload) -> Self {
        Self {
            workload,
            wall_timeout: MAX_WALL_TIMEOUT,
            signature_trust_bundle: None,
            pdfium_runtime_dir: None,
        }
    }
    pub fn with_wall_timeout(mut self, wall_timeout: Duration) -> Self {
        self.wall_timeout = wall_timeout;
        self
    }
    pub fn with_signature_trust_bundle(mut self, bundle: Vec<u8>) -> Self {
        self.signature_trust_bundle = Some(bundle);
        self
    }
    pub fn with_pdfium_runtime_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.pdfium_runtime_dir = Some(dir.into());
        self
    }
}

#[derive(Debug)]
pub struct SandboxProcessRunner {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    config: SandboxRunConfig,
}

impl SandboxProcessRunner {
    pub fn new(config: SandboxRunConfig) -> Result<Self, SandboxRunError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            Err(SandboxRunError::new(
                SandboxRunErrorKind::Unavailable,
                "Linux sandbox required",
            ))
        }
        #[cfg(target_os = "linux")]
        {
            process::validate_config(&config)?;
            Ok(Self { config })
        }
    }
    pub fn run(
        &self,
        worker: &Path,
        raw: &[u8],
        request: &[u8],
    ) -> Result<Vec<u8>, SandboxRunError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (self, worker, raw, request);
            Err(SandboxRunError::new(
                SandboxRunErrorKind::Unavailable,
                "Linux sandbox required",
            ))
        }
        #[cfg(target_os = "linux")]
        {
            process::run(&self.config, worker, raw, request)
        }
    }
}
