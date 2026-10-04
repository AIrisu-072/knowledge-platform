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
    time::Instant,
};

use document_diff_core::{
    FormatId, WorkerDiffRequest, WorkerDiffResponse, WorkerDisplayRequest, WorkerDisplayResponse,
    decode_worker_response_bounded,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    MAX_RESULT_BYTES, MAX_SOURCE_BYTES, MAX_STDERR_BYTES, MAX_TEMP_BYTES, MAX_WALL_TIMEOUT,
    RunnerConfig, RunnerError,
};

const CPU_SECONDS: u64 = 25;
const ADDRESS_SPACE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TEMP_ENTRIES: usize = 40_000;

pub(super) fn validate_config(config: &RunnerConfig) -> Result<(), RunnerError> {
    if !config.worker_executable.is_absolute()
        || !fs::metadata(&config.worker_executable).is_ok_and(|m| m.is_file())
        || config.wall_timeout.is_zero()
        || config.wall_timeout > MAX_WALL_TIMEOUT
        || config
            .pdfium_runtime_dir
            .as_ref()
            .is_some_and(|directory| !directory.is_absolute() || !directory.is_dir())
    {
        return Err(RunnerError::Unavailable(
            "invalid trusted runner configuration",
        ));
    }
    Ok(())
}

pub(super) fn compare(
    config: &RunnerConfig,
    request: WorkerDiffRequest,
    base: &[u8],
    target: &[u8],
) -> Result<WorkerDiffResponse, RunnerError> {
    request.validate().map_err(|_| resource("source bytes"))?;
    if request.format == FormatId::Pdf && config.pdfium_runtime_dir.is_none() {
        return Err(unavailable("qualified PDFium runtime path"));
    }
    check_raw(base, request.base_size_bytes, request.base_raw_sha256)?;
    check_raw(target, request.target_size_bytes, request.target_raw_sha256)?;
    let request_bytes = serde_json::to_vec(&request)
        .map_err(|_| RunnerError::InvalidResult("request serialization"))?;
    if request_bytes.len() > 64 * 1024 {
        return Err(resource("request bytes"));
    }
    let private_root = tempfile::Builder::new()
        .prefix("diff-compare-")
        .tempdir()
        .map_err(|_| unavailable("private workspace"))?;
    let staging = private_root.path().join("staging");
    let scratch = private_root.path().join("scratch");
    fs::create_dir(&staging).map_err(|_| unavailable("private staging"))?;
    fs::create_dir(&scratch).map_err(|_| unavailable("private scratch"))?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))
        .map_err(|_| unavailable("staging permissions"))?;
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))
        .map_err(|_| unavailable("scratch permissions"))?;
    let base_file = stage_read_only(&staging.join("base"), base)?;
    let target_file = stage_read_only(&staging.join("target"), target)?;
    let (status, stdout, stderr) = run_child(
        config,
        private_root.path(),
        &scratch,
        &base_file,
        &target_file,
        request_bytes,
        "compare",
    )?;
    if !status.success() {
        return Err(map_worker_failure(status, &stderr));
    }
    let response = decode_worker_response_bounded(&stdout, MAX_RESULT_BYTES)
        .map_err(|_| RunnerError::InvalidResult("worker response decoding"))?;
    response
        .validate_against(&request)
        .map_err(|_| RunnerError::InvalidResult("worker response binding"))?;
    Ok(response)
}

pub(super) fn extract_display(
    config: &RunnerConfig,
    request: WorkerDisplayRequest,
    source: &[u8],
) -> Result<WorkerDisplayResponse, RunnerError> {
    request
        .validate()
        .map_err(|_| resource("display request bounds"))?;
    check_raw(source, request.size_bytes, request.raw_sha256)?;
    let request_bytes = serde_json::to_vec(&request)
        .map_err(|_| RunnerError::InvalidResult("display request serialization"))?;
    if request_bytes.len() > 64 * 1024 {
        return Err(resource("display request bytes"));
    }
    let private_root = tempfile::Builder::new()
        .prefix("diff-display-")
        .tempdir()
        .map_err(|_| unavailable("private workspace"))?;
    let staging = private_root.path().join("staging");
    let scratch = private_root.path().join("scratch");
    fs::create_dir(&staging).map_err(|_| unavailable("private staging"))?;
    fs::create_dir(&scratch).map_err(|_| unavailable("private scratch"))?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))
        .map_err(|_| unavailable("staging permissions"))?;
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))
        .map_err(|_| unavailable("scratch permissions"))?;
    let source_file = stage_read_only(&staging.join("source"), source)?;
    let empty_target = stage_read_only(&staging.join("unused-target"), b"")?;
    let (status, stdout, stderr) = run_child(
        config,
        private_root.path(),
        &scratch,
        &source_file,
        &empty_target,
        request_bytes,
        "display",
    )?;
    if !status.success() {
        return Err(map_worker_failure(status, &stderr));
    }
    if stdout.len() > MAX_RESULT_BYTES {
        return Err(resource("display response bytes"));
    }
    let response: WorkerDisplayResponse = serde_json::from_slice(&stdout)
        .map_err(|_| RunnerError::InvalidResult("display response decoding"))?;
    response
        .validate_against(&request)
        .map_err(|_| RunnerError::InvalidResult("display response binding"))?;
    Ok(response)
}

fn check_raw(bytes: &[u8], expected_size: u64, expected_hash: [u8; 32]) -> Result<(), RunnerError> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(resource("source bytes"));
    }
    if bytes.len() as u64 != expected_size
        || <[u8; 32]>::from(Sha256::digest(bytes)) != expected_hash
    {
        return Err(RunnerError::RawBindingMismatch);
    }
    Ok(())
}

fn stage_read_only(path: &Path, bytes: &[u8]) -> Result<File, RunnerError> {
    let mut output = File::create_new(path).map_err(|_| unavailable("staging file"))?;
    output
        .write_all(bytes)
        .and_then(|_| output.sync_all())
        .map_err(|_| unavailable("staging write"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o400))
        .map_err(|_| unavailable("staging permissions"))?;
    File::open(path).map_err(|_| unavailable("read-only source"))
}

fn run_child(
    config: &RunnerConfig,
    monitor_root: &Path,
    scratch: &Path,
    base_file: &File,
    target_file: &File,
    request_bytes: Vec<u8>,
    operation: &'static str,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>), RunnerError> {
    let base_dup = duplicate_for_child(base_file)?;
    let target_dup = duplicate_for_child(target_file)?;
    let base_fd = base_dup.as_raw_fd();
    let target_fd = target_dup.as_raw_fd();
    let mut command = Command::new(&config.worker_executable);
    command
        .env_clear()
        .env("DIFF_OPERATION", operation)
        .env("DIFF_SANDBOX_REQUIRED", "1")
        .env("DSI_SANDBOX_REQUIRED", "1")
        .env("DIFF_BASE_FD", "3")
        .env("DIFF_TARGET_FD", "4")
        .env("TMPDIR", scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let Some(directory) = &config.pdfium_runtime_dir {
        command.env("PDFIUM_DYNAMIC_LIB_PATH", directory);
    }
    // SAFETY: only async-signal-safe libc calls are made between fork and exec.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(base_fd, 3) < 0 || libc::dup2(target_fd, 4) < 0 {
                return Err(io::Error::last_os_error());
            }
            close_unlisted_on_exec(5)?;
            set_limit(libc::RLIMIT_CPU, CPU_SECONDS)?;
            set_limit(libc::RLIMIT_AS, ADDRESS_SPACE_BYTES)?;
            set_limit(libc::RLIMIT_FSIZE, MAX_RESULT_BYTES as u64)?;
            Ok(())
        });
    }
    let child = command.spawn().map_err(|_| unavailable("worker launch"))?;
    let mut guard = ChildGuard::new(child);
    let stdin = guard
        .child
        .stdin
        .take()
        .ok_or_else(|| unavailable("worker stdin"))?;
    let stdout = guard
        .child
        .stdout
        .take()
        .ok_or_else(|| unavailable("worker stdout"))?;
    let stderr = guard
        .child
        .stderr
        .take()
        .ok_or_else(|| unavailable("worker stderr"))?;
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
                .map_err(|_| unavailable("worker wait"))?;
        }
        if stdout_bytes.is_none()
            && stdout_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            stdout_bytes = Some(finish_reader(stdout_reader.take().expect("stdout reader"))?);
        }
        if stderr_bytes.is_none()
            && stderr_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            stderr_bytes = Some(finish_reader(stderr_reader.take().expect("stderr reader"))?);
        }
        if status.is_some() && stdout_bytes.is_some() && stderr_bytes.is_some() {
            break;
        }
        if temp_tree_bytes(monitor_root)? > MAX_TEMP_BYTES {
            return Err(resource("temporary disk"));
        }
        if started.elapsed() > config.wall_timeout {
            return Err(RunnerError::Timeout);
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = writer.join();
    guard.clean = true;
    Ok((
        status.expect("child status"),
        stdout_bytes.expect("stdout"),
        stderr_bytes.expect("stderr"),
    ))
}

fn duplicate_for_child(file: &File) -> Result<File, RunnerError> {
    let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    if fd < 0 {
        return Err(unavailable("source descriptor"));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn close_unlisted_on_exec(first: i32) -> io::Result<()> {
    if unsafe {
        libc::syscall(
            libc::SYS_close_range,
            first as u32,
            u32::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    } == 0
    {
        return Ok(());
    }
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
    for fd in first..limit.rlim_cur as i32 {
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

fn set_limit(resource: libc::__rlimit_resource_t, value: u64) -> io::Result<()> {
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
        let mut chunk = [0_u8; 8192];
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
        Ok(ReaderResult::Overflow) => Err(RunnerError::InvalidResult("worker output byte limit")),
        _ => Err(unavailable("worker output capture")),
    }
}
fn temp_tree_bytes(root: &Path) -> Result<u64, RunnerError> {
    let mut pending = vec![root.to_path_buf()];
    let mut bytes = 0_u64;
    let mut entries = 0_usize;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|_| unavailable("scratch monitor"))? {
            let entry = entry.map_err(|_| unavailable("scratch monitor"))?;
            entries += 1;
            if entries > MAX_TEMP_ENTRIES {
                return Ok(u64::MAX);
            }
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|_| unavailable("scratch monitor"))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
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
        return resource("worker signal");
    }
    let code = serde_json::from_slice::<FailureWire>(stderr)
        .ok()
        .map(|wire| wire.code);
    match code.as_deref() {
        Some("raw_binding_mismatch") => RunnerError::RawBindingMismatch,
        Some("inspection_resource_limit_exceeded") => resource("worker limit"),
        Some("malformed_request" | "invalid_worker_result") => {
            RunnerError::InvalidResult("worker protocol")
        }
        _ => unavailable("worker failure"),
    }
}
fn resource(reason: &'static str) -> RunnerError {
    RunnerError::ResourceLimit(reason)
}
fn unavailable(reason: &'static str) -> RunnerError {
    RunnerError::Unavailable(reason)
}
