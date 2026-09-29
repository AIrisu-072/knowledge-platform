use std::{
    io::{Read, Write},
    process::{Command, exit},
    thread,
    time::Duration,
};

use document_diff_core::{
    DiffCoverage, SourceLocator, UnverifiedReason, WorkerDiffRequest, WorkerDiffResponse,
    WorkerUnverifiedRegion,
};

fn main() {
    let mut input = Vec::new();
    if std::io::stdin().read_to_end(&mut input).is_err() {
        exit(71);
    }
    let request: WorkerDiffRequest = match serde_json::from_slice(&input) {
        Ok(request) => request,
        Err(_) => exit(72),
    };
    let base = std::fs::read("/proc/self/fd/3").unwrap_or_default();
    let target = std::fs::read("/proc/self/fd/4").unwrap_or_default();
    if base != b"base" || target != b"target" {
        exit(73);
    }
    if [
        "DATABASE_URL",
        "AWS_SECRET_ACCESS_KEY",
        "DIFF_TEST_STORAGE_CREDENTIAL",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some())
    {
        exit(74);
    }
    if std::env::var("DIFF_SANDBOX_REQUIRED").as_deref() != Ok("1")
        || std::env::var("DSI_SANDBOX_REQUIRED").as_deref() != Ok("1")
    {
        exit(75);
    }
    if std::fs::read_link("/proc/self/fd/200").is_ok() {
        exit(76);
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("DIFF_TEST_SKIP_SEAL").is_none()
        && document_semantic_inspection_runner::seal_worker_sandbox().is_err()
    {
        exit(77);
    }
    use document_diff_core::FormatId;
    match request.format {
        FormatId::Csv => {
            if std::net::UdpSocket::bind("127.0.0.1:0").is_ok() {
                exit(78);
            }
        }
        FormatId::Html => {
            if Command::new("/bin/true").status().is_ok() {
                exit(79);
            }
        }
        FormatId::Docx => thread::sleep(Duration::from_secs(40)),
        FormatId::Xlsx => {
            let _ = std::io::stdout().write_all(&vec![b'x'; 16 * 1024 * 1024 + 1]);
            return;
        }
        FormatId::Xlsm => {
            let _ = std::io::stderr().write_all(&vec![b'x'; 1024 * 1024 + 1]);
            return;
        }
        FormatId::Pptx => {
            let _ = std::io::stdout().write_all(b"malformed");
            return;
        }
        _ => {}
    }
    let response = WorkerDiffResponse {
        protocol_version: request.protocol_version,
        diff_profile_version: request.diff_profile_version,
        resource_profile_version: request.resource_profile_version,
        base_raw_sha256: request.base_raw_sha256,
        base_size_bytes: request.base_size_bytes,
        target_raw_sha256: request.target_raw_sha256,
        target_size_bytes: request.target_size_bytes,
        format: request.format,
        coverage: DiffCoverage::None,
        changes: Vec::new(),
        unverified_regions: vec![WorkerUnverifiedRegion {
            base: Some(SourceLocator::ContentItem),
            target: Some(SourceLocator::ContentItem),
            reason: UnverifiedReason::UnsupportedSemanticConstruct,
            navigation_hint: Some("原本確認".to_owned()),
        }],
        parser_provenance: format!("synthetic-pid-{}", std::process::id()),
    };
    let _ = serde_json::to_writer(std::io::stdout(), &response);
}
