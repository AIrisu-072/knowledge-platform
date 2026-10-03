use std::fs;

use document_sandbox_runner::{
    SandboxProcessRunner, SandboxRunConfig, SandboxRunError, SandboxRunErrorKind,
};
use document_semantic_inspection_core::{
    InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
    decode_worker_response_bounded,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    MAX_INPUT_BYTES, MAX_REQUEST_BYTES, MAX_RESULT_BYTES, MAX_TRUST_BYTES, MAX_WALL_TIMEOUT,
    RunnerConfig, RunnerError, RunnerInput,
};

pub(super) fn validate_config(config: &RunnerConfig) -> Result<(), RunnerError> {
    if !config.worker_executable.is_absolute()
        || !fs::metadata(&config.worker_executable).is_ok_and(|metadata| metadata.is_file())
        || config.wall_timeout.is_zero()
        || config.wall_timeout > MAX_WALL_TIMEOUT
        || config
            .signature_trust_bundle
            .as_ref()
            .is_some_and(|bundle| bundle.len() > MAX_TRUST_BYTES)
        || config
            .pdfium_runtime_dir
            .as_ref()
            .is_some_and(|dir| !dir.is_absolute() || !dir.is_dir())
    {
        return Err(unavailable("invalid trusted runner configuration"));
    }
    Ok(())
}

pub(super) fn inspect(
    config: &RunnerConfig,
    input: RunnerInput<'_>,
) -> Result<WorkerResponse, RunnerError> {
    if input.bytes.len() > MAX_INPUT_BYTES {
        return Err(resource("input byte limit"));
    }
    if input.bytes.len() as u64 != input.expected_size_bytes {
        return Err(RunnerError::RawBindingMismatch {
            reason: "authoritative bytes differ from FileObject size",
        });
    }
    if input.declared_media_type.trim().is_empty() || input.declared_media_type.len() > 256 {
        return Err(invalid("declared media type is invalid"));
    }
    if let Some(trace) = &input.trace_context
        && (trace.traceparent.len() > 128
            || trace
                .tracestate
                .as_ref()
                .is_some_and(|value| value.len() > 512))
    {
        return Err(invalid("trace context exceeds its bound"));
    }
    let observed_hash: [u8; 32] = Sha256::digest(input.bytes).into();
    if observed_hash != input.expected_raw_content_hash {
        return Err(RunnerError::RawBindingMismatch {
            reason: "authoritative bytes differ from FileObject hash",
        });
    }
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: input.declared_media_type.to_owned(),
        expected_raw_content_hash: input.expected_raw_content_hash,
        expected_size_bytes: input.expected_size_bytes,
        trace_context: input.trace_context,
    };
    let request_bytes =
        serde_json::to_vec(&request).map_err(|_| invalid("request encoding failed"))?;
    if request_bytes.len() > MAX_REQUEST_BYTES {
        return Err(invalid("request byte limit"));
    }

    let mut sandbox = SandboxRunConfig::for_dsi().with_wall_timeout(config.wall_timeout);
    if let Some(trust) = &config.signature_trust_bundle {
        sandbox = sandbox.with_signature_trust_bundle(trust.clone());
    }
    if let Some(dir) = &config.pdfium_runtime_dir {
        sandbox = sandbox.with_pdfium_runtime_dir(dir);
    }
    let process = SandboxProcessRunner::new(sandbox).map_err(map_sandbox_error)?;
    let stdout = process
        .run(&config.worker_executable, input.bytes, &request_bytes)
        .map_err(map_sandbox_error)?;
    let response = decode_worker_response_bounded(&stdout, MAX_RESULT_BYTES)
        .map_err(|_| invalid("worker response cannot be decoded"))?;
    if response.inspection_profile_version != InspectionProfileVersion::DsiV0 {
        return Err(invalid("worker profile differs from request"));
    }
    if response.observed_raw_content_hash != input.expected_raw_content_hash
        || response.observed_size_bytes != input.expected_size_bytes
    {
        return Err(RunnerError::RawBindingMismatch {
            reason: "worker observed a different raw binding",
        });
    }
    Ok(response)
}

fn map_sandbox_error(error: SandboxRunError) -> RunnerError {
    match error.kind() {
        SandboxRunErrorKind::Timeout => RunnerError::InspectionTimeout {
            elapsed_ms: error.elapsed_ms().unwrap_or(0),
        },
        SandboxRunErrorKind::ResourceLimit => resource("worker resource limit"),
        SandboxRunErrorKind::OutputLimit | SandboxRunErrorKind::InvalidRequest => {
            invalid("worker output/request byte limit")
        }
        SandboxRunErrorKind::Unavailable => unavailable("sandbox worker unavailable"),
        SandboxRunErrorKind::WorkerExited => {
            let (status, stderr) = error.worker_failure().expect("exited status exists");
            map_worker_failure(status, stderr)
        }
    }
}

#[derive(Deserialize)]
struct FailureWire {
    code: String,
}

fn map_worker_failure(status: std::process::ExitStatus, stderr: &[u8]) -> RunnerError {
    use std::os::unix::process::ExitStatusExt;
    if status
        .signal()
        .is_some_and(|signal| [libc::SIGXCPU, libc::SIGXFSZ, libc::SIGKILL].contains(&signal))
    {
        return resource("worker was terminated by a resource limit");
    }
    let code = serde_json::from_slice::<FailureWire>(stderr)
        .ok()
        .map(|wire| wire.code);
    match code.as_deref() {
        Some("inspection_timeout") => RunnerError::InspectionTimeout { elapsed_ms: 0 },
        Some("inspection_resource_limit_exceeded") => resource("worker resource limit"),
        Some("extractor_unavailable") => unavailable("worker extractor unavailable"),
        Some("raw_binding_mismatch") => RunnerError::RawBindingMismatch {
            reason: "worker reported a raw mismatch",
        },
        Some("invalid_worker_result" | "malformed_request") => invalid("worker rejected protocol"),
        Some("unsupported_document_format") => RunnerError::WorkerFailure {
            code: "unsupported_document_format",
        },
        Some("requires_ocr") => RunnerError::WorkerFailure {
            code: "requires_ocr",
        },
        Some("encrypted_content_unsupported") => RunnerError::WorkerFailure {
            code: "encrypted_content_unsupported",
        },
        Some("format_mismatch") => RunnerError::WorkerFailure {
            code: "format_mismatch",
        },
        Some("semantic_extraction_failed") => RunnerError::WorkerFailure {
            code: "semantic_extraction_failed",
        },
        Some("parser_disagreement") => RunnerError::WorkerFailure {
            code: "parser_disagreement",
        },
        Some("unsupported_semantic_construct") => RunnerError::WorkerFailure {
            code: "unsupported_semantic_construct",
        },
        Some("worker_panicked") => unavailable("worker panicked"),
        _ => unavailable("worker exited without a valid failure classification"),
    }
}

fn invalid(reason: &'static str) -> RunnerError {
    RunnerError::InvalidWorkerResult { reason }
}
fn resource(reason: &'static str) -> RunnerError {
    RunnerError::InspectionResourceLimitExceeded { reason }
}
fn unavailable(reason: &'static str) -> RunnerError {
    RunnerError::ExtractorUnavailable { reason }
}
