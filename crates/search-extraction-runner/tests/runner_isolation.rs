#[cfg(target_os = "linux")]
use std::{path::PathBuf, time::Duration};

use document_sandbox_runner::{SandboxProcessRunner, SandboxRunConfig, SandboxRunErrorKind};

#[cfg(target_os = "linux")]
fn hostile_worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_search-extraction-hostile-worker"))
}

#[cfg(target_os = "linux")]
fn runner() -> SandboxProcessRunner {
    SandboxProcessRunner::new(SandboxRunConfig::for_search()).expect("Linux runner")
}

#[test]
#[cfg(target_os = "linux")]
fn fresh_process_and_mandatory_seal() {
    let first = runner()
        .run(&hostile_worker(), b"pid", b"request")
        .expect("first sealed worker");
    let second = runner()
        .run(&hostile_worker(), b"pid", b"request")
        .expect("second sealed worker");
    assert_ne!(first, second, "worker process must be fresh");

    let error = runner()
        .run(&hostile_worker(), b"no-marker", b"request")
        .expect_err("a worker without mandatory seal cannot succeed");
    assert_eq!(error.kind(), SandboxRunErrorKind::WorkerExited);
}

#[test]
#[cfg(target_os = "linux")]
fn sealed_worker_cannot_read_outside_scratch_spawn_or_open_network() {
    for action in [b"outside-read".as_slice(), b"spawn", b"network"] {
        assert_eq!(
            runner().run(&hostile_worker(), action, b"request").unwrap(),
            b"sealed",
            "sandbox violation was accepted for {action:?}"
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn inherited_parent_fd_is_closed_and_scratch_is_private() {
    use std::os::fd::AsRawFd;

    if std::env::var_os("SEARCH_RUNNER_FD_CHILD").is_some() {
        let private = tempfile::tempdir().expect("marker directory");
        let marker =
            std::fs::File::create(private.path().join("secret-marker")).expect("marker file");
        assert_eq!(unsafe { libc::dup2(marker.as_raw_fd(), 200) }, 200);
        assert_eq!(unsafe { libc::fcntl(200, libc::F_SETFD, 0) }, 0);
        assert_eq!(
            runner()
                .run(&hostile_worker(), b"inherited-fd", b"request")
                .unwrap(),
            b"sealed"
        );
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inherited_parent_fd_is_closed_and_scratch_is_private",
        ])
        .env("SEARCH_RUNNER_FD_CHILD", "1")
        .env("DATABASE_URL", "postgres://synthetic@example.invalid/test")
        .env("AWS_SECRET_ACCESS_KEY", "synthetic-marker")
        .env("SEARCH_STORAGE_CREDENTIAL", "synthetic-marker")
        .output()
        .expect("nested descriptor test");
    assert!(
        output.status.success(),
        "descriptor escaped: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn timeout_output_and_scratch_limits_fail_closed() {
    let short = SandboxProcessRunner::new(
        SandboxRunConfig::for_search().with_wall_timeout(Duration::from_millis(250)),
    )
    .unwrap();
    assert_eq!(
        short
            .run(&hostile_worker(), b"timeout", b"request")
            .expect_err("timed out worker")
            .kind(),
        SandboxRunErrorKind::Timeout
    );
    for action in [b"stdout-overflow".as_slice(), b"stderr-overflow"] {
        assert_eq!(
            runner()
                .run(&hostile_worker(), action, b"request")
                .expect_err("oversized worker output")
                .kind(),
            SandboxRunErrorKind::OutputLimit
        );
    }
    assert_eq!(
        runner()
            .run(&hostile_worker(), b"scratch-overflow", b"request")
            .expect_err("scratch over 1 GiB")
            .kind(),
        SandboxRunErrorKind::ResourceLimit
    );
}

#[test]
#[cfg(target_os = "linux")]
fn worker_stderr_never_appears_in_public_error() {
    let error = runner()
        .run(&hostile_worker(), b"stderr-leak", b"request")
        .expect_err("worker exited after writing sensitive stderr");
    assert!(!format!("{error:?}").contains("sensitive-body-marker"));
    assert!(!error.to_string().contains("sensitive-body-marker"));
}

#[test]
#[cfg(not(target_os = "linux"))]
fn non_linux_is_explicitly_unavailable() {
    let error = SandboxProcessRunner::new(SandboxRunConfig::for_search())
        .expect_err("Linux enforcement is mandatory");
    assert_eq!(error.kind(), SandboxRunErrorKind::Unavailable);
}
