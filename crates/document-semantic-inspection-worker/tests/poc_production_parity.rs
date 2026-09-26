//! Task 13 RED: production must preserve the qualified 91-case relation/error corpus.

use std::{collections::BTreeMap, fs, io::Cursor, path::PathBuf};

#[cfg(unix)]
use std::{
    io::Write,
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Command, Stdio},
};

use document_semantic_inspection_core::{
    AuthorityMigrationDecision, CapabilityEvidence, CapabilityState, InspectionProfileVersion,
    SemanticFingerprint, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
    assess_authority_migration,
};
use document_semantic_inspection_worker::{WorkerFailure, WorkerFailureCode, run_worker_shell};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_REQUEST: usize = 64 * 1024;
const MAX_INPUT: usize = 256 * 1024 * 1024;
#[cfg(unix)]
const CHILD_INPUT_FD: i32 = 100;

#[cfg(unix)]
unsafe extern "C" {
    fn dup2(old_fd: i32, new_fd: i32) -> i32;
}

#[derive(Deserialize)]
struct Manifest {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    path: String,
    format: String,
    sha256: String,
    size: u64,
    #[serde(default)]
    delimiter: Option<String>,
    #[serde(default)]
    text_encoding: Option<String>,
    #[serde(default)]
    script_required: bool,
    expected: Expected,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Expected {
    Success,
    SameAs { case_id: String },
    DifferentFrom { case_id: String },
    Error { code: WorkerFailureCode },
}

fn corpus() -> (PathBuf, Manifest) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/document-semantic-inspection-v0-corrected");
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest.cases.len(), 91, "the qualified corpus is fixed");
    (root, manifest)
}

fn media_type(case: &Case) -> String {
    let base = match case.format.as_str() {
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" => "text/html",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsm" => "application/vnd.ms-excel.sheet.macroenabled.12",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pdf" => "application/pdf",
        other => panic!("unexpected qualified format {other}"),
    };
    let mut media = base.to_owned();
    if let Some(delimiter) = &case.delimiter {
        media.push_str(&format!("; delimiter=\"{delimiter}\""));
    }
    if let Some(encoding) = &case.text_encoding {
        media.push_str(&format!("; charset={encoding}"));
    }
    if case.script_required {
        media.push_str("; script-required=true");
    }
    media
}

fn inspect(root: &std::path::Path, case: &Case) -> Result<WorkerResponse, WorkerFailureCode> {
    let bytes = fs::read(root.join(&case.path)).unwrap();
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    let observed_hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(observed_hex, case.sha256, "{} raw hash drifted", case.id);
    assert_eq!(
        bytes.len() as u64,
        case.size,
        "{} raw size drifted",
        case.id
    );
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: media_type(case),
        expected_raw_content_hash: hash,
        expected_size_bytes: case.size,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_worker_shell(
        &request_bytes,
        &mut Cursor::new(bytes),
        &mut stdout,
        &mut stderr,
        MAX_REQUEST,
        MAX_INPUT,
    );
    if code == 0 {
        assert!(stderr.is_empty(), "{} success emitted diagnostics", case.id);
        let response: WorkerResponse = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(
            response.observed_raw_content_hash, hash,
            "{} hash binding",
            case.id
        );
        assert_eq!(
            response.observed_size_bytes, case.size,
            "{} size binding",
            case.id
        );
        Ok(response)
    } else {
        assert!(
            stdout.is_empty(),
            "{} failure emitted partial success",
            case.id
        );
        let failure: WorkerFailure = serde_json::from_slice(&stderr).unwrap();
        if std::env::var_os("DSI_PARITY_DEBUG").is_some() {
            eprintln!("{}: {}", case.id, failure);
        }
        Err(failure.code())
    }
}

#[test]
fn explicit_text_csv_and_html_profile_cases_match_qualified_outcomes() {
    let (root, manifest) = corpus();
    let by_id: BTreeMap<_, _> = manifest
        .cases
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect();
    let ambiguous = inspect(&root, by_id["txt/ambiguous-decode"]);
    assert!(matches!(
        ambiguous,
        Err(WorkerFailureCode::SemanticExtractionFailed)
    ));
    let csv_base = inspect(&root, by_id["csv/base"]).expect("qualified CSV base");
    let csv_noise = inspect(&root, by_id["csv/quote-noise"]).expect("qualified CSV syntax noise");
    assert_eq!(
        csv_base.semantic_fingerprint,
        csv_noise.semantic_fingerprint
    );
    let script = inspect(&root, by_id["html/js-only"]);
    assert!(matches!(
        script,
        Err(WorkerFailureCode::UnsupportedSemanticConstruct)
    ));
}

#[test]
fn original_unrepaired_docx_png_is_rejected_without_weakening_checksum_validation() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/document-semantic-inspection-v0");
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let case = manifest
        .cases
        .iter()
        .find(|case| case.id == "docx/base")
        .unwrap();
    assert!(matches!(
        inspect(&root, case),
        Err(WorkerFailureCode::SemanticExtractionFailed)
    ));
}

#[test]
fn cross_format_capability_gate_preserves_qualified_authority_decisions() {
    let reader = capability(
        "reader_content",
        CapabilityState::Present,
        true,
        Some(b"reader"),
    );
    let matching_pdf = vec![
        reader.clone(),
        capability("footnotes", CapabilityState::NotRepresentable, true, None),
    ];
    assert_eq!(
        assess_authority_migration(std::slice::from_ref(&reader), &matching_pdf),
        AuthorityMigrationDecision::Eligible
    );
    assert_eq!(
        assess_authority_migration(
            &[
                reader.clone(),
                capability(
                    "footnotes",
                    CapabilityState::Present,
                    true,
                    Some(b"footnote")
                ),
            ],
            &matching_pdf,
        ),
        AuthorityMigrationDecision::Denied {
            missing: vec!["footnotes".to_owned()],
        }
    );
    assert_eq!(
        assess_authority_migration(
            &[reader],
            &[capability(
                "reader_content",
                CapabilityState::Present,
                true,
                Some(b"other")
            )],
        ),
        AuthorityMigrationDecision::Denied {
            missing: vec!["reader_content".to_owned()],
        }
    );
    let reader = capability(
        "reader_content",
        CapabilityState::Present,
        true,
        Some(b"reader"),
    );
    assert_eq!(
        assess_authority_migration(
            &[
                reader.clone(),
                capability("comments", CapabilityState::Present, false, Some(b"source")),
            ],
            &[
                reader.clone(),
                capability("comments", CapabilityState::Absent, false, None),
            ],
        ),
        AuthorityMigrationDecision::Eligible
    );
    assert_eq!(
        assess_authority_migration(
            &[
                reader.clone(),
                capability(
                    "formula_logic",
                    CapabilityState::Present,
                    true,
                    Some(b"formula")
                ),
                capability("vba_logic", CapabilityState::Present, true, Some(b"vba")),
                capability(
                    "hidden_content",
                    CapabilityState::Present,
                    true,
                    Some(b"hidden")
                ),
            ],
            &[
                reader,
                capability(
                    "formula_logic",
                    CapabilityState::NotRepresentable,
                    true,
                    None
                ),
                capability("vba_logic", CapabilityState::NotRepresentable, true, None),
                capability(
                    "hidden_content",
                    CapabilityState::NotRepresentable,
                    true,
                    None
                ),
            ],
        ),
        AuthorityMigrationDecision::Denied {
            missing: vec![
                "formula_logic".to_owned(),
                "hidden_content".to_owned(),
                "vba_logic".to_owned(),
            ],
        }
    );

    let (root, manifest) = corpus();
    let by_id: BTreeMap<_, _> = manifest
        .cases
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect();
    let xlsm = inspect(&root, by_id["xlsm/base"]).unwrap();
    let xlsx = inspect(&root, by_id["xlsx/base"]).unwrap();
    let docx = inspect(&root, by_id["docx/footnote-change"]).unwrap();
    let pdf = inspect(&root, by_id["pdf/base"]).unwrap();
    assert!(xlsm.semantic_capabilities.iter().any(|capability| {
        capability.capability_id == "vba_logic" && capability.presence == CapabilityState::Present
    }));
    let AuthorityMigrationDecision::Denied { missing } =
        assess_authority_migration(&xlsm.semantic_capabilities, &pdf.semantic_capabilities)
    else {
        panic!("an XLSM with VBA cannot become an authoritative PDF");
    };
    assert!(missing.contains(&"vba_logic".to_owned()));
    for (source, capability_id) in [(&xlsx, "formula_logic"), (&docx, "footnotes")] {
        assert!(source.semantic_capabilities.iter().any(|capability| {
            capability.capability_id == capability_id
                && capability.presence == CapabilityState::Present
        }));
        let AuthorityMigrationDecision::Denied { missing } =
            assess_authority_migration(&source.semantic_capabilities, &pdf.semantic_capabilities)
        else {
            panic!("{capability_id} cannot be silently lost in a PDF");
        };
        assert!(missing.contains(&capability_id.to_owned()));
    }
}

fn capability(
    id: &str,
    presence: CapabilityState,
    version_significant: bool,
    fingerprint: Option<&[u8]>,
) -> CapabilityEvidence {
    CapabilityEvidence {
        capability_id: id.to_owned(),
        presence,
        version_significant,
        equivalence_fingerprint: fingerprint.map(SemanticFingerprint::sha256),
    }
}

#[test]
fn all_91_qualified_relations_and_errors_match_production() {
    let (root, manifest) = corpus();
    let mut actual = BTreeMap::new();
    for case in &manifest.cases {
        actual.insert(case.id.as_str(), inspect(&root, case));
    }
    for case in &manifest.cases {
        let result = &actual[case.id.as_str()];
        match &case.expected {
            Expected::Success => assert!(
                result.is_ok(),
                "{} expected success, got {result:?}",
                case.id
            ),
            Expected::Error { code } => {
                assert_eq!(result.as_ref().unwrap_err(), code, "{} error", case.id)
            }
            Expected::SameAs { case_id } => {
                let current = result
                    .as_ref()
                    .unwrap_or_else(|error| panic!("{}: {error:?}", case.id));
                let reference = actual[case_id.as_str()].as_ref().unwrap();
                assert_eq!(
                    current.semantic_fingerprint, reference.semantic_fingerprint,
                    "{} same relation",
                    case.id
                );
            }
            Expected::DifferentFrom { case_id } => {
                let current = result
                    .as_ref()
                    .unwrap_or_else(|error| panic!("{}: {error:?}", case.id));
                let reference = actual[case_id.as_str()].as_ref().unwrap();
                assert_ne!(
                    current.semantic_fingerprint, reference.semantic_fingerprint,
                    "{} different relation",
                    case.id
                );
            }
        }
    }
}

#[test]
fn every_successful_qualified_case_is_identical_across_20_repeated_inspections() {
    let (root, manifest) = corpus();
    let mut successful = 0;
    for case in &manifest.cases {
        if let Ok(expected) = inspect(&root, case) {
            successful += 1;
            for repeat in 0..20 {
                assert_eq!(
                    inspect(&root, case),
                    Ok(expected.clone()),
                    "{} in-process repeat {repeat}",
                    case.id
                );
            }
        }
    }
    assert!(
        successful > 0,
        "the qualified corpus contains success cases"
    );
}

#[cfg(unix)]
#[test]
fn every_successful_qualified_case_is_identical_across_five_fresh_worker_processes() {
    let (root, manifest) = corpus();
    let environments = [
        ("C", "UTC"),
        ("C.UTF-8", "Asia/Tokyo"),
        ("en_US.UTF-8", "America/Los_Angeles"),
        ("ja_JP.UTF-8", "Europe/London"),
        ("POSIX", "Pacific/Auckland"),
    ];
    let mut successful = 0;
    for case in &manifest.cases {
        if let Ok(expected) = inspect(&root, case) {
            successful += 1;
            for (index, &(locale, timezone)) in environments.iter().enumerate() {
                let actual = inspect_fresh_process(&root, case, locale, timezone);
                assert_eq!(
                    actual, expected,
                    "{} fresh process {index}, LC_ALL={locale}, TZ={timezone}",
                    case.id
                );
            }
        }
    }
    assert!(
        successful > 0,
        "the qualified corpus contains success cases"
    );
}

#[cfg(unix)]
fn inspect_fresh_process(
    root: &std::path::Path,
    case: &Case,
    locale: &str,
    timezone: &str,
) -> WorkerResponse {
    let path = root.join(&case.path);
    let bytes = fs::read(&path).unwrap();
    let input_file = fs::File::open(&path).unwrap();
    let request = WorkerRequest {
        protocol_version: WorkerProtocolVersion::V0,
        inspection_profile_version: InspectionProfileVersion::DsiV0,
        declared_media_type: media_type(case),
        expected_raw_content_hash: Sha256::digest(&bytes).into(),
        expected_size_bytes: bytes.len() as u64,
        trace_context: None,
    };
    let request_bytes = serde_json::to_vec(&request).unwrap();
    let input_fd = input_file.as_raw_fd();
    let mut command = Command::new(env!("CARGO_BIN_EXE_document-semantic-inspection-worker"));
    command
        .env("DSI_INPUT_FD", CHILD_INPUT_FD.to_string())
        .env_remove("DSI_SANDBOX_REQUIRED")
        .env("LC_ALL", locale)
        .env("TZ", timezone)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The production runner also passes the input as an inherited read-only descriptor.
    unsafe {
        command.pre_exec(move || {
            if dup2(input_fd, CHILD_INPUT_FD) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().expect("spawn fresh production worker");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&request_bytes)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{} fresh worker failed: {}",
        case.id,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "{} worker diagnostics", case.id);
    serde_json::from_slice(&output.stdout).unwrap()
}
