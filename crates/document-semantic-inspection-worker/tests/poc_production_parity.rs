//! Task 13 RED: production must preserve the qualified 91-case relation/error corpus.

use std::{collections::BTreeMap, fs, io::Cursor, path::PathBuf};

use document_semantic_inspection_core::{
    InspectionProfileVersion, WorkerProtocolVersion, WorkerRequest, WorkerResponse,
};
use document_semantic_inspection_worker::{WorkerFailure, WorkerFailureCode, run_worker_shell};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_REQUEST: usize = 64 * 1024;
const MAX_INPUT: usize = 256 * 1024 * 1024;

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
        .join("../../testdata/document-semantic-inspection-v0");
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
