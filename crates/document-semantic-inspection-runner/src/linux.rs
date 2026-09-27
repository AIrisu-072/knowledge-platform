use std::{
    fs::{self, File},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use document_semantic_inspection_core::{
    InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
    decode_worker_response_bounded,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    MAX_INPUT_BYTES, MAX_REQUEST_BYTES, MAX_RESULT_BYTES, MAX_STDERR_BYTES, MAX_TEMP_BYTES,
    MAX_TRUST_BYTES, MAX_WALL_TIMEOUT, RunnerConfig, RunnerError, RunnerInput,
};

const CPU_SECONDS: u64 = 8;
const ADDRESS_SPACE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const OUTPUT_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TEMP_ENTRIES: usize = 20_000;

pub(super) fn validate_config(config: &RunnerConfig) -> Result<(), RunnerError> {
    if !config.worker_executable.is_absolute()
        || !fs::metadata(&config.worker_executable)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
        || config.wall_timeout.is_zero()
        || config.wall_timeout > MAX_WALL_TIMEOUT
        || config
            .signature_trust_bundle
            .as_ref()
            .is_some_and(|bundle| bundle.len() > MAX_TRUST_BYTES)
        || config
            .pdfium_runtime_dir
            .as_ref()
            .is_some_and(|directory| !directory.is_absolute() || !directory.is_dir())
    {
        return Err(RunnerError::ExtractorUnavailable {
            reason: "invalid trusted runner configuration",
        });
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

    let private_root = tempfile::Builder::new()
        .prefix("dsi-inspection-")
        .tempdir()
        .map_err(|_| unavailable("private workspace unavailable"))?;
    let staging = private_root.path().join("staging");
    let scratch = private_root.path().join("scratch");
    fs::create_dir(&staging).map_err(|_| unavailable("private staging unavailable"))?;
    fs::create_dir(&scratch).map_err(|_| unavailable("private scratch unavailable"))?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))
        .map_err(|_| unavailable("private staging permissions unavailable"))?;
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))
        .map_err(|_| unavailable("private scratch permissions unavailable"))?;
    let input_file = stage_read_only(&staging.join("input"), input.bytes)?;
    let trust_file = config
        .signature_trust_bundle
        .as_ref()
        .map(|bundle| stage_read_only(&staging.join("trust"), bundle))
        .transpose()?;

    let (status, stdout, stderr) = run_child(
        config,
        private_root.path(),
        &scratch,
        &input_file,
        trust_file.as_ref(),
        request_bytes,
    )?;
    if !status.success() {
        return Err(map_worker_failure(status, &stderr));
    }
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

fn stage_read_only(path: &Path, bytes: &[u8]) -> Result<File, RunnerError> {
    let mut output =
        File::create_new(path).map_err(|_| unavailable("private input unavailable"))?;
    output
        .write_all(bytes)
        .and_then(|_| output.sync_all())
        .map_err(|_| unavailable("private input write failed"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o400))
        .map_err(|_| unavailable("private input permissions failed"))?;
    File::open(path).map_err(|_| unavailable("read-only input unavailable"))
}

fn run_child(
    config: &RunnerConfig,
    monitor_root: &Path,
    scratch: &Path,
    input_file: &File,
    trust_file: Option<&File>,
    request_bytes: Vec<u8>,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>), RunnerError> {
    let input_dup = duplicate_for_child(input_file)?;
    let trust_dup = trust_file.map(duplicate_for_child).transpose()?;
    let input_fd = input_dup.as_raw_fd();
    let trust_fd = trust_dup.as_ref().map(AsRawFd::as_raw_fd);

    let mut command = Command::new(&config.worker_executable);
    command
        .env_clear()
        .env("DSI_SANDBOX_REQUIRED", "1")
        .env("DSI_INPUT_FD", "3")
        .env("TMPDIR", scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if trust_fd.is_some() {
        command.env("DSI_TRUST_FD", "4");
    }
    if let Some(directory) = &config.pdfium_runtime_dir {
        command.env("PDFIUM_DYNAMIC_LIB_PATH", directory);
    }
    // SAFETY: only async-signal-safe libc calls occur after fork and before exec.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(input_fd, 3) < 0 {
                return Err(io::Error::last_os_error());
            }
            if let Some(trust_fd) = trust_fd
                && libc::dup2(trust_fd, 4) < 0
            {
                return Err(io::Error::last_os_error());
            }
            mark_unlisted_descriptors_close_on_exec(if trust_fd.is_some() { 5 } else { 4 })?;
            set_child_rlimit(libc::RLIMIT_CPU, CPU_SECONDS)?;
            set_child_rlimit(libc::RLIMIT_AS, ADDRESS_SPACE_BYTES)?;
            set_child_rlimit(libc::RLIMIT_FSIZE, OUTPUT_FILE_BYTES)?;
            Ok(())
        });
    }

    let child = command
        .spawn()
        .map_err(|_| unavailable("sandbox worker could not be launched"))?;
    let mut guard = ChildGuard::new(child);
    let stdin = guard
        .child
        .stdin
        .take()
        .ok_or_else(|| unavailable("worker stdin unavailable"))?;
    let stdout = guard
        .child
        .stdout
        .take()
        .ok_or_else(|| unavailable("worker stdout unavailable"))?;
    let stderr = guard
        .child
        .stderr
        .take()
        .ok_or_else(|| unavailable("worker stderr unavailable"))?;
    let writer = thread::spawn(move || {
        let mut stdin = stdin;
        let _ = stdin.write_all(&request_bytes);
    });
    let mut stdout_reader = Some(spawn_bounded_reader(stdout, MAX_RESULT_BYTES));
    let mut stderr_reader = Some(spawn_bounded_reader(stderr, MAX_STDERR_BYTES));
    let mut stdout_bytes = None;
    let mut stderr_bytes = None;
    let mut status = None;
    let started = Instant::now();

    loop {
        if status.is_none() {
            status = guard
                .child
                .try_wait()
                .map_err(|_| unavailable("worker wait failed"))?;
        }
        if stdout_bytes.is_none()
            && stdout_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            stdout_bytes = Some(finish_reader(stdout_reader.take().expect("reader exists"))?);
        }
        if stderr_bytes.is_none()
            && stderr_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            stderr_bytes = Some(finish_reader(stderr_reader.take().expect("reader exists"))?);
        }
        if status.is_some() && stdout_bytes.is_some() && stderr_bytes.is_some() {
            break;
        }
        if temp_tree_bytes(monitor_root)? > MAX_TEMP_BYTES {
            return Err(resource("temporary disk limit"));
        }
        if started.elapsed() >= config.wall_timeout {
            return Err(RunnerError::InspectionTimeout {
                elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = writer.join();
    guard.clean = true;
    Ok((
        status.expect("completed worker status"),
        stdout_bytes.expect("completed stdout"),
        stderr_bytes.expect("completed stderr"),
    ))
}

fn duplicate_for_child(file: &File) -> Result<File, RunnerError> {
    // Keep source descriptors above 3/4 so dup2 cannot overwrite the other
    // source when the operating system originally assigned one of those FDs.
    let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    if fd < 0 {
        return Err(unavailable("inherited descriptor unavailable"));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn mark_unlisted_descriptors_close_on_exec(first_unlisted: i32) -> io::Result<()> {
    // The host may have inheritable sockets or credentials. Preserve only
    // stdio and explicit input/trust FDs. CLOEXEC keeps the spawn error pipe
    // available until exec, including if a later pre-exec operation fails.
    if unsafe {
        libc::syscall(
            libc::SYS_close_range,
            first_unlisted as u32,
            u32::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    } == 0
    {
        return Ok(());
    }

    // Some container seccomp profiles deny close_range. Sweep the complete
    // possible FD table with async-signal-safe syscalls in that case.
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe {
        libc::syscall(
            libc::SYS_prlimit64,
            0,
            libc::RLIMIT_NOFILE,
            std::ptr::null::<libc::rlimit>(),
            &mut limit,
        )
    } < 0
        || limit.rlim_cur > i32::MAX as libc::rlim_t
    {
        return Err(io::Error::last_os_error());
    }
    for fd in first_unlisted..limit.rlim_cur as i32 {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags < 0 {
            if io::Error::last_os_error().raw_os_error() == Some(libc::EBADF) {
                continue;
            }
            return Err(io::Error::last_os_error());
        }
        if flags & libc::FD_CLOEXEC == 0
            && unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn set_child_rlimit(resource: libc::__rlimit_resource_t, value: u64) -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    if unsafe { libc::setrlimit(resource, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

struct ChildGuard {
    child: Child,
    pid: i32,
    clean: bool,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        let pid = child.id() as i32;
        Self {
            child,
            pid,
            clean: false,
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.clean {
            let _ = unsafe { libc::kill(-self.pid, libc::SIGKILL) };
            let _ = self.child.wait();
        }
    }
}

enum ReaderResult {
    Bytes(Vec<u8>),
    Overflow,
    Failed,
}

fn spawn_bounded_reader<R: Read + Send + 'static>(
    mut reader: R,
    limit: usize,
) -> thread::JoinHandle<ReaderResult> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => return ReaderResult::Bytes(bytes),
                Ok(count) if bytes.len().saturating_add(count) > limit => {
                    return ReaderResult::Overflow;
                }
                Ok(count) => bytes.extend_from_slice(&chunk[..count]),
                Err(_) => return ReaderResult::Failed,
            }
        }
    })
}

fn finish_reader(handle: thread::JoinHandle<ReaderResult>) -> Result<Vec<u8>, RunnerError> {
    match handle.join() {
        Ok(ReaderResult::Bytes(bytes)) => Ok(bytes),
        Ok(ReaderResult::Overflow) => Err(invalid("worker output byte limit")),
        _ => Err(unavailable("worker output capture failed")),
    }
}

fn temp_tree_bytes(root: &Path) -> Result<u64, RunnerError> {
    let mut directories = vec![root.to_path_buf()];
    let mut entries = 0usize;
    let mut bytes = 0u64;
    while let Some(directory) = directories.pop() {
        for entry in
            fs::read_dir(directory).map_err(|_| unavailable("scratch monitoring failed"))?
        {
            let entry = entry.map_err(|_| unavailable("scratch monitoring failed"))?;
            entries += 1;
            if entries > MAX_TEMP_ENTRIES {
                return Ok(u64::MAX);
            }
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| unavailable("scratch monitoring failed"))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                directories.push(entry.path());
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(bytes)
}

#[derive(Deserialize)]
struct FailureWire {
    code: String,
}

fn map_worker_failure(status: ExitStatus, stderr: &[u8]) -> RunnerError {
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
