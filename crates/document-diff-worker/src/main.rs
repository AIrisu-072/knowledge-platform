use std::{
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};

use document_diff_core::FormatId;
#[cfg(target_os = "linux")]
use document_diff_worker::run_display_worker_shell;
use document_diff_worker::{
    MAX_REQUEST_BYTES, WorkerError, decode_display_request_bounded, decode_request_bounded,
    run_worker_shell,
};
use document_semantic_inspection_worker::PdfAdapter;

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(
                std::io::stderr(),
                "{}",
                serde_json::json!({"code": error.code()})
            );
            ExitCode::from(65)
        }
    }
}

fn execute() -> Result<(), WorkerError> {
    let base_fd = descriptor("DIFF_BASE_FD", 3)?;
    let target_fd = descriptor("DIFF_TARGET_FD", 4)?;
    if base_fd == target_fd {
        return Err(WorkerError::InvalidRequest);
    }
    let base =
        File::open(format!("/proc/self/fd/{base_fd}")).map_err(|_| WorkerError::UnreadableInput)?;
    let target = File::open(format!("/proc/self/fd/{target_fd}"))
        .map_err(|_| WorkerError::UnreadableInput)?;
    let operation = std::env::var("DIFF_OPERATION").unwrap_or_else(|_| "compare".into());
    let mut request_bytes = Vec::new();
    std::io::stdin()
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut request_bytes)
        .map_err(|_| WorkerError::InvalidRequest)?;
    if operation == "display" {
        let request = decode_display_request_bounded(&request_bytes)?;
        if std::env::var("DIFF_SANDBOX_REQUIRED").as_deref() != Ok("1") {
            return Err(WorkerError::UnreadableInput);
        }
        if request.format == FormatId::Pdf
            && matches!(
                request.locator,
                document_diff_core::SourceLocator::PdfPage { region: None, .. }
            )
        {
            PdfAdapter::warm_up_native_runtime().map_err(|_| WorkerError::UnreadableInput)?;
        }
        #[cfg(target_os = "linux")]
        {
            document_semantic_inspection_runner::seal_worker_sandbox()
                .map_err(|_| WorkerError::UnreadableInput)?;
            let response = run_display_worker_shell(&request_bytes, base)?;
            let bytes = serde_json::to_vec(&response).map_err(|_| WorkerError::InvalidResult)?;
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|_| WorkerError::UnreadableInput)?;
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        return Err(WorkerError::UnreadableInput);
    }
    if operation != "compare" {
        return Err(WorkerError::InvalidRequest);
    }
    if std::env::var_os("DIFF_SANDBOX_REQUIRED").is_some() {
        if std::env::var("DIFF_SANDBOX_REQUIRED").as_deref() != Ok("1") {
            return Err(WorkerError::UnreadableInput);
        }
        if decode_request_bounded(&request_bytes)?.format == FormatId::Pdf {
            PdfAdapter::warm_up_native_runtime().map_err(|_| WorkerError::UnreadableInput)?;
        }
        #[cfg(target_os = "linux")]
        document_semantic_inspection_runner::seal_worker_sandbox()
            .map_err(|_| WorkerError::UnreadableInput)?;
        #[cfg(not(target_os = "linux"))]
        return Err(WorkerError::UnreadableInput);
    }
    let response = run_worker_shell(&request_bytes, base, target)?;
    let bytes = serde_json::to_vec(&response).map_err(|_| WorkerError::InvalidResult)?;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|_| WorkerError::UnreadableInput)?;
    Ok(())
}

fn descriptor(name: &str, expected: i32) -> Result<i32, WorkerError> {
    match std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
    {
        Some(fd) if fd == expected => Ok(fd),
        _ => Err(WorkerError::InvalidRequest),
    }
}
