use std::{
    io::{self, Read, Write},
    process::{Command, exit},
    thread,
    time::Duration,
};

#[cfg(target_os = "linux")]
use document_semantic_inspection_runner::seal_worker_sandbox;

fn main() {
    let mut request = String::new();
    if io::stdin().read_to_string(&mut request).is_err() {
        exit(90);
    }
    if !input_descriptor_matches() {
        exit(95);
    }
    let inherited_fd_probe = request.contains("dsi-test=inherited-fd");
    if inherited_fd_probe
        && [4, 200].iter().any(|fd| {
            std::fs::read_link(format!("/proc/self/fd/{fd}"))
                .is_ok_and(|path| path.ends_with("inheritable-marker"))
        })
    {
        exit(97);
    }
    #[cfg(target_os = "linux")]
    if !inherited_fd_probe
        && std::env::var_os("DSI_TEST_SKIP_SANDBOX_SEAL").is_none()
        && seal_worker_sandbox().is_err()
    {
        exit(96);
    }

    if request.contains("dsi-test=network") {
        if std::net::UdpSocket::bind("127.0.0.1:0").is_ok() {
            exit(91);
        }
    } else if request.contains("dsi-test=child-process") {
        if Command::new("/bin/true").status().is_ok() {
            exit(92);
        }
    } else if request.contains("dsi-test=timeout") {
        thread::sleep(Duration::from_secs(30));
    } else if request.contains("dsi-test=stdout-overflow") {
        let _ = io::stdout().write_all(&vec![b'x'; 16 * 1024 * 1024 + 1]);
        exit(0);
    } else if request.contains("dsi-test=stderr-overflow") {
        let _ = io::stderr().write_all(&vec![b'x'; 1024 * 1024 + 1]);
        exit(0);
    } else if request.contains("dsi-test=malformed-result") {
        let _ = io::stderr().write_all(b"synthetic-body-marker must never enter runner errors");
        let _ = io::stdout().write_all(b"not a WorkerResponse");
        exit(0);
    } else if request.contains("dsi-test=raw-binding-mismatch") {
        let response = response(std::process::id(), true);
        let _ = io::stdout().write_all(response.as_bytes());
        return;
    } else if request.contains("dsi-test=boundary") && !boundary_contract_holds(&request) {
        exit(93);
    } else if request.contains("dsi-test=boundary") && !filesystem_boundary_holds() {
        exit(94);
    }

    let response = response(std::process::id(), false);
    let _ = io::stdout().write_all(response.as_bytes());
}

fn response(pid: u32, raw_binding_mismatch: bool) -> String {
    let (hash, size) = if raw_binding_mismatch {
        (
            "0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0",
            11,
        )
    } else {
        (
            "61,175,105,143,24,37,217,20,236,52,172,159,15,171,246,231,227,50,120,44,179,226,28,95,160,0,214,106,238,91,41,29",
            12,
        )
    };
    let response = format!(
        r#"{{"protocol_version":"dsi-worker-v0","inspection_profile_version":"dsi-v0","observed_raw_content_hash":[{hash}],"observed_size_bytes":{size},"detected_format":"txt","semantic_fingerprint":{{"algorithm":"sha256","digest":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}},"semantic_capabilities":[],"editorial_provenance":{{"tracked_changes":[],"comments":[],"document_author_labels":[],"last_modified_by":null,"modification_metadata":{{}}}},"external_dependencies":[],"digital_signature_evidence":[],"extractor_provenance":{{"worker_build_id":"dsi-test","adapter_id":"dsi-runner-test","adapter_version":"0","parser_libraries":[],"native_dependency_identity":[]}},"diagnostics":[{{"code":"synthetic_worker_pid","message":"{pid}"}}]}}"#
    );
    response
}

fn boundary_contract_holds(request: &str) -> bool {
    if [
        "DATABASE_URL",
        "AWS_SECRET_ACCESS_KEY",
        "DSI_TEST_STORAGE_CREDENTIAL",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some())
    {
        return false;
    }
    if [
        "file_id",
        "document_id",
        "document_version_id",
        "principal",
        "storage_key",
    ]
    .iter()
    .any(|name| request.contains(name))
        || request.contains("/authoritative/")
    {
        return false;
    }

    let input_fd = match std::env::var("DSI_INPUT_FD")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
    {
        Some(fd) if fd >= 3 => fd,
        _ => return false,
    };
    let trust_fd = match std::env::var("DSI_TRUST_FD")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
    {
        Some(fd) if fd >= 3 && fd != input_fd => fd,
        _ => return false,
    };
    if !is_read_only(input_fd) {
        return false;
    }

    let temp_dir = match std::env::var_os("TMPDIR") {
        Some(path) => std::path::PathBuf::from(path),
        None => return false,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = match std::fs::metadata(&temp_dir) {
            Ok(metadata) => metadata.permissions().mode() & 0o777,
            Err(_) => return false,
        };
        if mode != 0o700 {
            return false;
        }
    }
    let probe = temp_dir.join("runner-private-temp-probe");
    if std::fs::write(&probe, b"private").is_err() {
        return false;
    }
    let _ = std::fs::remove_file(probe);

    read_descriptor(trust_fd)
        .map(|bytes| bytes == br#"{"trusted_certificates_der":[],"crls_der":[]}"#)
        .unwrap_or(false)
}

fn input_descriptor_matches() -> bool {
    let fd = match std::env::var("DSI_INPUT_FD")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
    {
        Some(fd) if fd >= 3 => fd,
        _ => return false,
    };
    is_read_only(fd) && matches!(read_descriptor(fd), Ok(bytes) if bytes == b"input-vector")
}

fn filesystem_boundary_holds() -> bool {
    let parent_pid = parent_pid();
    let temp = std::path::PathBuf::from("/tmp");
    let read_sentinel = temp.join(format!("dsi-runner-outside-read-{parent_pid}"));
    let write_sentinel = temp.join(format!("dsi-runner-outside-write-{parent_pid}"));

    if std::fs::read(&read_sentinel).is_ok() {
        return false;
    }
    let write_was_allowed = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&write_sentinel)
        .is_ok();
    if write_was_allowed {
        let _ = std::fs::remove_file(write_sentinel);
        return false;
    }

    let sentinel_fragment = format!("dsi-runner-outside-{}", parent_pid);
    if std::env::args_os().any(|arg| arg.to_string_lossy().contains(&sentinel_fragment)) {
        return false;
    }
    !std::env::vars_os().any(|(name, value)| {
        name.to_string_lossy().contains(&sentinel_fragment)
            || value.to_string_lossy().contains(&sentinel_fragment)
    })
}

#[cfg(target_os = "linux")]
fn parent_pid() -> u32 {
    unsafe extern "C" {
        fn getppid() -> i32;
    }
    unsafe { getppid() as u32 }
}

#[cfg(not(target_os = "linux"))]
fn parent_pid() -> u32 {
    0
}

#[cfg(unix)]
fn is_read_only(fd: i32) -> bool {
    use std::os::raw::c_int;

    const F_GETFL: c_int = 3;
    const O_ACCMODE: c_int = 3;
    unsafe extern "C" {
        fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    }

    let flags = unsafe { fcntl(fd, F_GETFL) };
    flags >= 0 && flags & O_ACCMODE == 0
}

#[cfg(not(unix))]
fn is_read_only(_: i32) -> bool {
    false
}

#[cfg(unix)]
fn read_descriptor(fd: i32) -> io::Result<Vec<u8>> {
    use std::os::fd::FromRawFd;

    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    std::mem::forget(file);
    Ok(bytes)
}

#[cfg(not(unix))]
fn read_descriptor(_: i32) -> io::Result<Vec<u8>> {
    Err(io::Error::other("descriptor transport requires Unix"))
}
