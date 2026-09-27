#![cfg(unix)]

use std::{
    fs,
    io::{ErrorKind, Write},
    os::fd::AsRawFd,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

use document_semantic_inspection_core::{
    InspectionProfileVersion, SignatureValidity, WorkerProtocolVersion, WorkerRequest,
    WorkerResponse,
};
use sha2::{Digest, Sha256};

const VALID_PDF: &[u8] = include_bytes!("fixtures/signatures/valid.pdf");
const PDF_TRUST: &[u8] = include_bytes!("fixtures/signatures/test-only-trust.der");
const MAX_TRUST_BUNDLE_BYTES: usize = 1024 * 1024;
const CHILD_INPUT_FD: i32 = 100;
const CHILD_TRUST_FD: i32 = 101;

unsafe extern "C" {
    fn dup2(old_fd: i32, new_fd: i32) -> i32;
}

#[test]
fn binary_uses_explicit_trust_from_a_separate_inherited_descriptor() {
    let trust_bundle = serde_json::to_vec(&serde_json::json!({
        "trusted_certificates_der": [PDF_TRUST],
        "crls_der": [],
    }))
    .expect("serialize test-only trust bundle");

    let output = run_worker(VALID_PDF, &trust_bundle);
    assert!(
        output.status.success(),
        "worker failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: WorkerResponse =
        serde_json::from_slice(&output.stdout).expect("structured worker response");
    assert_eq!(response.digital_signature_evidence.len(), 1);
    assert_eq!(
        response.digital_signature_evidence[0].cryptographic_validity,
        SignatureValidity::Valid,
        "the executable must use the explicit offline trust anchor"
    );
}

#[test]
fn binary_rejects_malformed_unknown_and_oversized_trust_bundles() {
    let unknown_field = br#"{"trusted_certificates_der":[],"crls_der":[],"unexpected":true}"#;
    let oversized = vec![b' '; MAX_TRUST_BUNDLE_BYTES + 1];

    for (label, trust_bundle, expected_code) in [
        ("malformed JSON", b"{".as_slice(), "malformed_request"),
        ("unknown field", unknown_field, "malformed_request"),
        (
            "oversized input",
            oversized.as_slice(),
            "inspection_resource_limit_exceeded",
        ),
    ] {
        let output = run_worker(VALID_PDF, trust_bundle);
        assert!(
            !output.status.success(),
            "{label} trust input must fail closed"
        );
        let failure: serde_json::Value =
            serde_json::from_slice(&output.stderr).expect("structured worker failure");
        assert_eq!(failure["code"], expected_code, "{label}");
    }
}

fn run_worker(pdf: &[u8], trust_bundle: &[u8]) -> Output {
    let directory = tempfile::tempdir().expect("temporary worker inputs");
    let input_path = directory.path().join("document.pdf");
    let trust_path = directory.path().join("trust.json");
    fs::write(&input_path, pdf).expect("write PDF fixture");
    fs::write(&trust_path, trust_bundle).expect("write trust bundle");
    let input_file = fs::File::open(input_path).expect("open inherited PDF descriptor");
    let trust_file = fs::File::open(trust_path).expect("open inherited trust descriptor");
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: "application/pdf".to_owned(),
        expected_raw_content_hash: Sha256::digest(pdf).into(),
        expected_size_bytes: pdf.len() as u64,
        trace_context: None,
    };
    let request = serde_json::to_vec(&request).expect("serialize worker request");
    let input_fd = input_file.as_raw_fd();
    let trust_fd = trust_file.as_raw_fd();
    let mut command = Command::new(env!("CARGO_BIN_EXE_document-semantic-inspection-worker"));
    command
        .env("DSI_INPUT_FD", CHILD_INPUT_FD.to_string())
        .env("DSI_TRUST_FD", CHILD_TRUST_FD.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The runner contract passes read-only descriptors separately from the request.
    unsafe {
        command.pre_exec(move || {
            if dup2(input_fd, CHILD_INPUT_FD) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if dup2(trust_fd, CHILD_TRUST_FD) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().expect("spawn production worker binary");
    if let Err(error) = child
        .stdin
        .take()
        .expect("worker stdin")
        .write_all(&request)
    {
        // An invalid trust bundle can close stdin before the request is written.
        assert_eq!(
            error.kind(),
            ErrorKind::BrokenPipe,
            "unexpected WorkerRequest write failure: {error}"
        );
    }
    child
        .wait_with_output()
        .expect("wait for production worker")
}
