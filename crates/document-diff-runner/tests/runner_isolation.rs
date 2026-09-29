use document_diff_core::{
    DiffProfileVersion, FormatId, ResourceProfileVersion, WorkerDiffRequest, WorkerProtocolVersion,
};
use document_diff_runner::{LinuxSandboxRunner, RunnerConfig};
use sha2::{Digest, Sha256};

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn request() -> WorkerDiffRequest {
    WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Txt,
        base_raw_sha256: Sha256::digest(b"base").into(),
        base_size_bytes: 4,
        target_raw_sha256: Sha256::digest(b"target").into(),
        target_size_bytes: 6,
    }
}

#[test]
fn missing_or_mismatched_source_is_rejected_before_launch() {
    let config = RunnerConfig::new(std::env::current_exe().unwrap());
    let runner = LinuxSandboxRunner::new(config);
    #[cfg(not(target_os = "linux"))]
    assert!(runner.is_err());
    #[cfg(target_os = "linux")]
    {
        let runner = runner.unwrap();
        assert!(runner.compare(request(), b"base", b"").is_err());
        assert!(runner.compare(request(), b"wrong", b"target").is_err());
    }
}

#[cfg(target_os = "linux")]
fn synthetic_runner() -> LinuxSandboxRunner {
    LinuxSandboxRunner::new(RunnerConfig::new(env!(
        "CARGO_BIN_EXE_diff-runner-hostile-worker"
    )))
    .unwrap()
}

#[cfg(target_os = "linux")]
#[test]
fn sandbox_is_fresh_and_denies_network_and_exec() {
    let runner = synthetic_runner();
    let first = runner.compare(request(), b"base", b"target").unwrap();
    let second = runner.compare(request(), b"base", b"target").unwrap();
    assert_ne!(first.parser_provenance, second.parser_provenance);
    for format in [
        document_diff_core::FormatId::Csv,
        document_diff_core::FormatId::Html,
    ] {
        let mut request = request();
        request.format = format;
        runner.compare(request, b"base", b"target").unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn timeout_and_oversized_or_malformed_output_fail_closed() {
    let runner = LinuxSandboxRunner::new(
        RunnerConfig::new(env!("CARGO_BIN_EXE_diff-runner-hostile-worker"))
            .with_wall_timeout(std::time::Duration::from_millis(250)),
    )
    .unwrap();
    let mut request = request();
    request.format = document_diff_core::FormatId::Docx;
    assert!(matches!(
        runner.compare(request, b"base", b"target"),
        Err(document_diff_runner::RunnerError::Timeout)
    ));
    let runner = synthetic_runner();
    for format in [
        document_diff_core::FormatId::Xlsx,
        document_diff_core::FormatId::Xlsm,
        document_diff_core::FormatId::Pptx,
    ] {
        let mut request = request();
        request.format = format;
        assert!(runner.compare(request, b"base", b"target").is_err());
    }
}
