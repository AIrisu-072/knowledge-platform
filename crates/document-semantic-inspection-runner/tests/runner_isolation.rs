#![cfg(target_os = "linux")]

use std::os::fd::AsRawFd;
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::time::Duration;

use document_semantic_inspection_core::TraceContext;
use document_semantic_inspection_runner::{
    LinuxSandboxRunner, RunnerConfig, RunnerError, RunnerInput,
};

const INPUT_SHA256: [u8; 32] = [
    0x3d, 0xaf, 0x69, 0x8f, 0x18, 0x25, 0xd9, 0x14, 0xec, 0x34, 0xac, 0x9f, 0x0f, 0xab, 0xf6, 0xe7,
    0xe3, 0x32, 0x78, 0x2c, 0xb3, 0xe2, 0x1c, 0x5f, 0xa0, 0x00, 0xd6, 0x6a, 0xee, 0x5b, 0x29, 0x1d,
];
const TRUST_BUNDLE: &[u8] = br#"{"trusted_certificates_der":[],"crls_der":[]}"#;
const UNLISTED_PARENT_FDS: [i32; 2] = [4, 200];

fn synthetic_worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_dsi-runner-hostile-worker"))
}

fn runner(trust: Option<&[u8]>) -> LinuxSandboxRunner {
    let mut config = RunnerConfig::new(synthetic_worker());
    if let Some(trust) = trust {
        config = config.with_signature_trust_bundle(trust.to_vec());
    }
    LinuxSandboxRunner::new(config).expect("construct Linux sandbox runner")
}

fn input(action: &str) -> RunnerInput<'static> {
    RunnerInput {
        bytes: b"input-vector",
        declared_media_type: "text/plain",
        expected_raw_content_hash: INPUT_SHA256,
        expected_size_bytes: 12,
        trace_context: Some(TraceContext {
            traceparent: "00-00000000000000000000000000000001-0000000000000001-01".into(),
            tracestate: Some(format!("dsi-test={action}")),
        }),
    }
}

#[test]
#[cfg(target_os = "linux")]
fn each_inspection_gets_a_fresh_child_process() {
    let runner = runner(None);
    let first = runner.inspect(input("baseline")).expect("first result");
    let second = runner.inspect(input("baseline")).expect("second result");

    assert_eq!(
        first.detected_format,
        document_semantic_inspection_core::FormatId::Txt
    );
    assert_ne!(
        first.diagnostics[0].message, second.diagnostics[0].message,
        "runner reused the worker process"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn sandbox_denies_network_and_worker_child_process_creation() {
    let runner = runner(None);

    for action in ["network", "child-process"] {
        runner.inspect(input(action)).unwrap_or_else(|error| {
            panic!("synthetic worker escaped {action} restriction: {error:?}")
        });
    }
}

#[test]
fn runner_does_not_pass_an_unlisted_inheritable_parent_descriptor() {
    if std::env::var_os("DSI_RUNNER_FD_CHILD").is_some() {
        let directory = tempfile::tempdir().expect("private marker directory");
        let marker = std::fs::File::create(directory.path().join("inheritable-marker"))
            .expect("open synthetic parent descriptor");
        // dup2 deliberately creates an inheritable FD, as a host process may have.
        for fd in UNLISTED_PARENT_FDS {
            assert_eq!(unsafe { libc::dup2(marker.as_raw_fd(), fd) }, fd);
            assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
        }
        let result = runner(None).inspect(input("inherited-fd"));
        result.expect("worker must not inherit an unlisted parent descriptor");
        runner(Some(TRUST_BUNDLE))
            .inspect(input("inherited-fd"))
            .expect("worker with explicit trust must not inherit unrelated descriptors");
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "runner_does_not_pass_an_unlisted_inheritable_parent_descriptor",
            "--nocapture",
        ])
        .env("DSI_RUNNER_FD_CHILD", "1")
        .output()
        .expect("run isolated inherited-descriptor test");
    assert!(
        output.status.success(),
        "an unlisted parent descriptor reached the worker: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn worker_gets_only_read_only_input_private_temp_and_explicit_trust_fd() {
    if std::env::var_os("DSI_RUNNER_BOUNDARY_CHILD").is_some() {
        let parent_pid = std::process::id();
        let read_sentinel =
            PathBuf::from("/tmp").join(format!("dsi-runner-outside-read-{parent_pid}"));
        let write_sentinel =
            PathBuf::from("/tmp").join(format!("dsi-runner-outside-write-{parent_pid}"));
        std::fs::write(&read_sentinel, b"outside the private scratch root")
            .expect("create outside read sentinel");
        let _ = std::fs::remove_file(&write_sentinel);

        let runner = runner(Some(TRUST_BUNDLE));
        runner
            .inspect(input("boundary"))
            .unwrap_or_else(|error| panic!("worker boundary contract failed: {error:?}"));
        std::fs::remove_file(read_sentinel).expect("remove outside read sentinel");
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "worker_gets_only_read_only_input_private_temp_and_explicit_trust_fd",
            "--nocapture",
        ])
        .env("DSI_RUNNER_BOUNDARY_CHILD", "1")
        .env(
            "DATABASE_URL",
            "postgres://synthetic:marker@example.invalid/db",
        )
        .env("AWS_SECRET_ACCESS_KEY", "synthetic-credential-marker")
        .env("DSI_TEST_STORAGE_CREDENTIAL", "synthetic-storage-marker")
        .output()
        .expect("run nested test with synthetic credential environment");
    assert!(
        output.status.success(),
        "runner passed the synthetic credential environment or lost its trust/input descriptors"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn wall_timeout_and_oversized_result_fail_with_typed_errors() {
    let timeout_runner = LinuxSandboxRunner::new(
        RunnerConfig::new(synthetic_worker()).with_wall_timeout(Duration::from_millis(250)),
    )
    .expect("construct timeout runner");
    assert!(matches!(
        timeout_runner.inspect(input("timeout")),
        Err(RunnerError::InspectionTimeout { .. })
    ));

    let runner = runner(None);
    assert!(matches!(
        runner.inspect(input("stdout-overflow")),
        Err(RunnerError::InvalidWorkerResult { .. })
    ));
    assert!(runner.inspect(input("stderr-overflow")).is_err());

    let malformed = runner
        .inspect(input("malformed-result"))
        .expect_err("malformed response must fail closed");
    assert!(matches!(
        &malformed,
        RunnerError::InvalidWorkerResult { .. }
    ));
    assert!(!malformed.to_string().contains("synthetic-body-marker"));
    assert!(!format!("{malformed:?}").contains("synthetic-body-marker"));
    assert!(matches!(
        runner.inspect(input("raw-binding-mismatch")),
        Err(RunnerError::RawBindingMismatch { .. })
    ));
}

#[test]
#[cfg(target_os = "linux")]
fn authoritative_size_mismatch_fails_before_worker_launch() {
    let runner = runner(None);
    let mut wrong_size = input("baseline");
    wrong_size.expected_size_bytes += 1;
    assert!(matches!(
        runner.inspect(wrong_size),
        Err(RunnerError::RawBindingMismatch { .. })
    ));
}
