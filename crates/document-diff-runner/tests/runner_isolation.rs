use document_diff_core::{
    DiffProfileVersion, FormatId, ResourceProfileVersion, WorkerDiffRequest, WorkerProtocolVersion,
};
use document_diff_runner::{LinuxSandboxRunner, RunnerConfig};
use sha2::{Digest, Sha256};

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
