//! Trusted host boundary for Search body extraction.

use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use document_sandbox_runner::{
    SandboxProcessRunner, SandboxRunConfig, SandboxRunError, SandboxRunErrorKind,
};
use search_core::knowledge_unit::{BudgetKey, FormatId};
use search_extraction_core::{
    BodyCoverage, ExtractionError, PermanentFailureCode, ReaderFailure, RegisteredProfile,
    RetryableFailureCode, WorkerFragment, WorkerOperation, WorkerReport, WorkerRequest,
    WorkerResponse, decode_response, encode_request, validate_worker_report,
    validate_worker_request,
};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct SearchRunnerConfig {
    worker_executable: PathBuf,
    pdfium_runtime_dir: Option<PathBuf>,
    wall_timeout: Duration,
}

impl SearchRunnerConfig {
    pub fn new(worker_executable: impl Into<PathBuf>) -> Self {
        Self {
            worker_executable: worker_executable.into(),
            pdfium_runtime_dir: None,
            wall_timeout: Duration::from_secs(10),
        }
    }
    pub fn with_pdfium_runtime_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.pdfium_runtime_dir = Some(directory.into());
        self
    }
    pub fn with_wall_timeout(mut self, timeout: Duration) -> Self {
        self.wall_timeout = timeout;
        self
    }
}

#[derive(Debug)]
pub struct SearchExtractionRunner {
    config: SearchRunnerConfig,
    process: SandboxProcessRunner,
    profiles: BTreeMap<String, RegisteredProfile>,
}

impl SearchExtractionRunner {
    pub fn new(
        config: SearchRunnerConfig,
        profiles: Vec<RegisteredProfile>,
    ) -> Result<Self, ExtractionError> {
        if !config.worker_executable.is_absolute() {
            return Err(ExtractionError::Configuration(
                "absolute worker path required",
            ));
        }
        if profiles.is_empty() {
            return Err(ExtractionError::Configuration("registered profile absent"));
        }
        let mut registry = BTreeMap::new();
        for profile in profiles {
            if registry
                .insert(profile.id().as_str().to_owned(), profile)
                .is_some()
            {
                return Err(ExtractionError::Configuration(
                    "duplicate registered profile",
                ));
            }
        }
        let mut sandbox = SandboxRunConfig::for_search().with_wall_timeout(config.wall_timeout);
        if let Some(dir) = &config.pdfium_runtime_dir {
            sandbox = sandbox.with_pdfium_runtime_dir(dir);
        }
        let process = SandboxProcessRunner::new(sandbox)
            .map_err(|_| ExtractionError::Configuration("Linux sandbox unavailable"))?;
        Ok(Self {
            config,
            process,
            profiles: registry,
        })
    }

    pub fn extract(
        &self,
        raw: &[u8],
        request: WorkerRequest,
    ) -> Result<WorkerReport, ExtractionError> {
        if !matches!(request.operation, WorkerOperation::Extract) {
            return Err(ExtractionError::Configuration("extract operation"));
        }
        self.execute(raw, request)
    }

    pub fn resolve_locators(
        &self,
        raw: &[u8],
        request: WorkerRequest,
    ) -> Result<Vec<WorkerFragment>, ExtractionError> {
        if !matches!(request.operation, WorkerOperation::ResolveLocators(_)) {
            return Err(ExtractionError::Configuration("resolve operation"));
        }
        let report = self.execute(raw, request)?;
        if matches!(report.coverage, BodyCoverage::Unsupported { .. }) {
            return Err(ExtractionError::Integrity("resolve unsupported"));
        }
        Ok(report.fragments)
    }

    /// Extract with a profile the trusted host registered for this item, such as a
    /// ZIP composite plan built from host-owned leaf definitions. Worker bytes never
    /// create or select the profile.
    pub fn extract_registered(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<WorkerReport, ExtractionError> {
        if !matches!(request.operation, WorkerOperation::Extract) {
            return Err(ExtractionError::Configuration("extract operation"));
        }
        self.execute_with(raw, request, profile)
    }

    /// Resolve locators with a host-registered profile; see [`Self::extract_registered`].
    pub fn resolve_registered(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<Vec<WorkerFragment>, ExtractionError> {
        if !matches!(request.operation, WorkerOperation::ResolveLocators(_)) {
            return Err(ExtractionError::Configuration("resolve operation"));
        }
        let report = self.execute_with(raw, request, profile)?;
        if matches!(report.coverage, BodyCoverage::Unsupported { .. }) {
            return Err(ExtractionError::Integrity("resolve unsupported"));
        }
        Ok(report.fragments)
    }

    fn execute(&self, raw: &[u8], request: WorkerRequest) -> Result<WorkerReport, ExtractionError> {
        let profile = self
            .profiles
            .get(request.profile.as_str())
            .ok_or(ExtractionError::Configuration("unregistered profile"))?;
        self.execute_with(raw, request, profile)
    }

    fn execute_with(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<WorkerReport, ExtractionError> {
        validate_worker_request(&request, profile)?;
        if raw.len() as u64 != request.expected_raw.size_bytes
            || Sha256::digest(raw).as_slice() != request.expected_raw.sha256
        {
            return Err(ExtractionError::Integrity("raw binding"));
        }
        verify_native_pin(profile, self.config.pdfium_runtime_dir.as_deref())?;
        let wire = encode_request(&request)?;
        let output = self
            .process
            .run(&self.config.worker_executable, raw, &wire)
            .map_err(map_sandbox_error)?;
        if output.len() as u64 > request.budgets.get(BudgetKey::WorkerOutputBytes) {
            return Err(ExtractionError::Permanent(
                PermanentFailureCode::WorkerOutputLimit,
            ));
        }
        let response =
            decode_response(&output).map_err(|_| ExtractionError::Integrity("worker response"))?;
        match response {
            WorkerResponse::Report(report) => {
                validate_worker_report(&report, profile)
                    .map_err(|_| ExtractionError::Integrity("worker report"))?;
                Ok(report)
            }
            WorkerResponse::Failure(ReaderFailure::Unsupported(reason)) => {
                // A decoded failure carries no fragments or reader-use witness.
                // The request/profile, raw binding and mandatory process boundary
                // have already been checked. Do not apply the completed archive
                // traversal witness contract or invent visited plan nodes here.
                let report = WorkerReport {
                    coverage: BodyCoverage::Unsupported { reason },
                    fragments: Vec::new(),
                    reader_use: Vec::new(),
                    scope_items: 0,
                    known_omissions: Vec::new(),
                    traversal_complete: false,
                };
                validate_worker_report(&report, profile)?;
                Ok(report)
            }
            WorkerResponse::Failure(ReaderFailure::Permanent(code)) => {
                Err(ExtractionError::Permanent(code))
            }
            WorkerResponse::Failure(ReaderFailure::Retryable(code)) => {
                Err(ExtractionError::Retryable(code))
            }
        }
    }
}

fn verify_native_pin(
    profile: &RegisteredProfile,
    pdfium_runtime_dir: Option<&Path>,
) -> Result<(), ExtractionError> {
    let mut required = Vec::new();
    if let Some(plan) = profile.archive_plan() {
        for node in &plan.nodes {
            if node.definition.format == FormatId::Pdf {
                required.push(
                    node.definition
                        .native_binary_sha256
                        .ok_or(ExtractionError::Configuration("PDFium native pin"))?,
                );
            }
        }
    } else if profile.definition().format == FormatId::Pdf {
        required.push(
            *profile
                .native_binary_sha256()
                .ok_or(ExtractionError::Configuration("PDFium native pin"))?,
        );
    }
    if required.is_empty() {
        return Ok(());
    }
    if required.iter().any(|pin| pin != &required[0]) {
        return Err(ExtractionError::Configuration(
            "conflicting PDFium native pins",
        ));
    }
    let dir = pdfium_runtime_dir.ok_or(ExtractionError::Configuration("PDFium runtime missing"))?;
    let path = dir.join("libpdfium.so");
    let mut file = File::open(path)
        .map_err(|_| ExtractionError::Configuration("PDFium runtime unavailable"))?;
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= 268_435_456)
    {
        return Err(ExtractionError::Configuration("PDFium runtime size"));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| ExtractionError::Configuration("PDFium runtime unreadable"))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let observed: [u8; 32] = hash.finalize().into();
    if observed != required[0] {
        return Err(ExtractionError::Configuration("PDFium native pin mismatch"));
    }
    Ok(())
}

fn map_sandbox_error(error: SandboxRunError) -> ExtractionError {
    match error.kind() {
        SandboxRunErrorKind::OutputLimit => {
            ExtractionError::Permanent(PermanentFailureCode::WorkerOutputLimit)
        }
        SandboxRunErrorKind::Timeout => ExtractionError::Retryable(RetryableFailureCode::Timeout),
        SandboxRunErrorKind::ResourceLimit => {
            ExtractionError::Retryable(RetryableFailureCode::WorkerKilled)
        }
        SandboxRunErrorKind::Unavailable => {
            ExtractionError::Retryable(RetryableFailureCode::WorkerUnavailable)
        }
        SandboxRunErrorKind::InvalidRequest => ExtractionError::Configuration("sandbox request"),
        SandboxRunErrorKind::WorkerExited => {
            if let Some((status, stderr)) = error.worker_failure() {
                if status.code() == Some(79) {
                    return ExtractionError::Integrity("worker protocol failure");
                }
                if status.code() == Some(78) && stderr == b"SEARCH_SANDBOX_UNAVAILABLE\n" {
                    return ExtractionError::Configuration("mandatory Linux sandbox unavailable");
                }
                if status.code() == Some(77) && stderr == b"SEARCH_NATIVE_PIN_MISMATCH\n" {
                    return ExtractionError::Configuration("PDFium native pin mismatch");
                }
                if status.code() == Some(76) && stderr == b"SEARCH_RAW_BINDING_MISMATCH\n" {
                    return ExtractionError::Integrity("worker raw binding mismatch");
                }
            }
            ExtractionError::Retryable(RetryableFailureCode::WorkerKilled)
        }
    }
}

impl search_extraction_core::ContentExtractor for SearchExtractionRunner {
    fn extract(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<WorkerReport, ExtractionError> {
        self.extract_registered(raw, request, profile)
    }

    fn resolve_locators(
        &self,
        raw: &[u8],
        request: WorkerRequest,
        profile: &RegisteredProfile,
    ) -> Result<Vec<WorkerFragment>, ExtractionError> {
        self.resolve_registered(raw, request, profile)
    }
}
