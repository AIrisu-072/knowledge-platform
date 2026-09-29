#![cfg(target_os = "linux")]

use document_diff_core::{
    DiffCoverage, DiffProfileVersion, FormatId, ResourceProfileVersion, WorkerDiffRequest,
    WorkerProtocolVersion,
};
use document_diff_runner::{LinuxSandboxRunner, RunnerConfig, RunnerError};
use sha2::{Digest, Sha256};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf");
const TEXT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pdf/text-change.pdf"
);

#[test]
fn qualified_pdfium_is_bound_before_linux_sandbox_seals() {
    let request = WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Pdf,
        base_raw_sha256: Sha256::digest(BASE).into(),
        base_size_bytes: BASE.len() as u64,
        target_raw_sha256: Sha256::digest(TEXT).into(),
        target_size_bytes: TEXT.len() as u64,
    };
    let unconfigured = LinuxSandboxRunner::new(RunnerConfig::new(env!(
        "CARGO_BIN_EXE_document-diff-worker"
    )))
    .unwrap();
    assert_eq!(
        unconfigured.compare(request, BASE, TEXT),
        Err(RunnerError::Unavailable("qualified PDFium runtime path"))
    );

    let directory = std::env::var("PDFIUM_DYNAMIC_LIB_PATH")
        .expect("CI must provide the qualified pinned PDFium library directory");
    let runner = LinuxSandboxRunner::new(
        RunnerConfig::new(env!("CARGO_BIN_EXE_document-diff-worker"))
            .with_pdfium_runtime_dir(directory),
    )
    .unwrap();
    let result = runner.compare(request, BASE, TEXT).unwrap();
    result.validate_against(&request).unwrap();
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert!(
        result
            .changes
            .iter()
            .any(|change| change.facet == "pdf_text")
    );
}
