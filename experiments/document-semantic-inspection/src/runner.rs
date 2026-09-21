use crate::manifest::{ExpectedOutcome, FixtureClass};
use crate::{
    fingerprint, AdapterOutput, CaseReport, CaseVerdict, ErrorCode, FixtureCase, FixtureManifest,
    FormatId, InspectionAdapter, InspectionResult, PocError, VerificationReport,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct AdapterRegistry {
    adapters: BTreeMap<FormatId, Box<dyn InspectionAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self {
            adapters: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, adapter: Box<dyn InspectionAdapter>) {
        self.adapters.insert(adapter.format(), adapter);
    }

    fn get(&self, format: FormatId) -> Option<&dyn InspectionAdapter> {
        self.adapters.get(&format).map(Box::as_ref)
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_case(
    case: &FixtureCase,
    adapter: &dyn InspectionAdapter,
) -> Result<InspectionResult, PocError> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    run_case_at(case, &root, adapter)
}

pub fn run_case_at(
    case: &FixtureCase,
    fixture_root: &Path,
    adapter: &dyn InspectionAdapter,
) -> Result<InspectionResult, PocError> {
    let path = fixture_root.join(&case.path);
    let input = fs::read(&path).map_err(|error| {
        PocError::SemanticExtractionFailed(format!("cannot read fixture {}: {error}", case.path))
    })?;

    let observed_size = input.len() as u64;
    let observed_sha256 = hex::encode(Sha256::digest(&input));
    if observed_size != case.size || !observed_sha256.eq_ignore_ascii_case(&case.sha256) {
        return Err(PocError::RawBindingMismatch {
            path: case.path.clone(),
            expected_sha256: case.sha256.clone(),
            observed_sha256,
            expected_size: case.size,
            observed_size,
        });
    }

    let observed_format = detect_format(&input);
    if observed_format != Some(case.format) {
        return Err(PocError::FormatMismatch {
            expected: case.format,
            observed: observed_format,
        });
    }

    if adapter.format() != case.format {
        return Err(PocError::InvalidWorkerResult(format!(
            "adapter format {:?} does not match case format {:?}",
            adapter.format(),
            case.format
        )));
    }

    let profile = case.inspection_profile()?;
    let output: AdapterOutput = adapter.inspect(&input, &profile)?;
    let semantic_fingerprint = fingerprint(&output.semantic_projection);

    Ok(InspectionResult {
        semantic_fingerprint,
        output,
    })
}

pub fn verify_manifest(
    manifest: &FixtureManifest,
    fixture_root: &Path,
    registry: &AdapterRegistry,
) -> VerificationReport {
    let mut actual: BTreeMap<String, Result<InspectionResult, PocError>> = BTreeMap::new();

    for case in &manifest.cases {
        let result = match registry.get(case.format) {
            Some(adapter) => run_case_at(case, fixture_root, adapter),
            None => Err(PocError::UnsupportedDocumentFormat(format!(
                "{:?}",
                case.format
            ))),
        };
        actual.insert(case.id.clone(), result);
    }

    let mut cases = Vec::with_capacity(manifest.cases.len());

    for case in &manifest.cases {
        let current = actual
            .get(&case.id)
            .expect("manifest validation guarantees current case presence");

        let (verdict, message) = evaluate_expected(case, current, &actual);
        let (semantic_fingerprint, error_code) = match current {
            Ok(result) => (Some(hex::encode(result.semantic_fingerprint)), None),
            Err(error) => (None, Some(error.code())),
        };

        cases.push(CaseReport {
            id: case.id.clone(),
            class: case.class,
            format: case.format,
            verdict,
            semantic_fingerprint,
            error_code,
            message,
        });
    }

    VerificationReport {
        passed: cases.iter().all(|case| case.verdict == CaseVerdict::Pass),
        cases,
    }
}

fn evaluate_expected(
    case: &FixtureCase,
    current: &Result<InspectionResult, PocError>,
    actual: &BTreeMap<String, Result<InspectionResult, PocError>>,
) -> (CaseVerdict, String) {
    match (&case.expected, current) {
        (ExpectedOutcome::Success, Ok(_)) => (CaseVerdict::Pass, "success".into()),
        (ExpectedOutcome::Success, Err(error)) => (
            CaseVerdict::Fail,
            format!("expected success, observed {:?}", error.code()),
        ),
        (ExpectedOutcome::Error { code }, Err(error)) if *code == error.code() => {
            (CaseVerdict::Pass, format!("expected error {code:?}"))
        }
        (ExpectedOutcome::Error { code }, Err(error)) => (
            CaseVerdict::Fail,
            format!("expected error {code:?}, observed {:?}", error.code()),
        ),
        (ExpectedOutcome::Error { code }, Ok(_)) => (
            CaseVerdict::Fail,
            format!("expected error {code:?}, observed success"),
        ),
        (ExpectedOutcome::SameAs { case_id }, Ok(result)) => {
            compare_relation(case_id, result, actual, true)
        }
        (ExpectedOutcome::DifferentFrom { case_id }, Ok(result)) => {
            compare_relation(case_id, result, actual, false)
        }
        (ExpectedOutcome::SameAs { case_id }, Err(error))
        | (ExpectedOutcome::DifferentFrom { case_id }, Err(error)) => (
            CaseVerdict::Fail,
            format!(
                "relation to {case_id} expected success, observed {:?}",
                error.code()
            ),
        ),
    }
}

fn compare_relation(
    reference_id: &str,
    current: &InspectionResult,
    actual: &BTreeMap<String, Result<InspectionResult, PocError>>,
    expect_equal: bool,
) -> (CaseVerdict, String) {
    let Some(reference) = actual.get(reference_id) else {
        return (CaseVerdict::Fail, format!("missing reference {reference_id}"));
    };
    let Ok(reference) = reference else {
        return (
            CaseVerdict::Fail,
            format!("reference {reference_id} did not inspect successfully"),
        );
    };

    let equal = current.semantic_fingerprint == reference.semantic_fingerprint;
    if equal == expect_equal {
        (
            CaseVerdict::Pass,
            if expect_equal {
                format!("same as {reference_id}")
            } else {
                format!("different from {reference_id}")
            },
        )
    } else {
        (
            CaseVerdict::Fail,
            if expect_equal {
                format!("expected same as {reference_id}")
            } else {
                format!("expected different from {reference_id}")
            },
        )
    }
}

fn detect_format(input: &[u8]) -> Option<FormatId> {
    if input.starts_with(b"%PDF-") {
        return Some(FormatId::Pdf);
    }
    if input.starts_with(b"PK\x03\x04") {
        if crate::adapters::is_docx_package(input) {
            return Some(FormatId::Docx);
        }
        if let Some(format) = crate::adapters::spreadsheet_format(input) {
            return Some(format);
        }
        return None;
    }

    let text = std::str::from_utf8(input).ok()?;
    if text.contains('\0') {
        return None;
    }

    let trimmed = text.trim_start().to_ascii_lowercase();
    if trimmed.starts_with("<!doctype html")
        || trimmed.starts_with("<html")
        || trimmed.starts_with("<head")
        || trimmed.starts_with("<body")
    {
        return Some(FormatId::Html);
    }

    if text.lines().any(|line| line.contains(',')) {
        return Some(FormatId::Csv);
    }

    Some(FormatId::Txt)
}

#[allow(dead_code)]
fn _class_is_known(class: FixtureClass) -> bool {
    matches!(
        class,
        FixtureClass::Base
            | FixtureClass::Semantic
            | FixtureClass::Noise
            | FixtureClass::Editorial
            | FixtureClass::Hostile
    )
}

#[allow(dead_code)]
fn _error_code_is_known(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::InvalidManifest
            | ErrorCode::RawBindingMismatch
            | ErrorCode::FormatMismatch
            | ErrorCode::UnsupportedDocumentFormat
            | ErrorCode::RequiresOcr
            | ErrorCode::EncryptedContentUnsupported
            | ErrorCode::UnsupportedSemanticConstruct
            | ErrorCode::SemanticExtractionFailed
            | ErrorCode::ParserDisagreement
            | ErrorCode::InspectionTimeout
            | ErrorCode::InspectionResourceLimitExceeded
            | ErrorCode::ExtractorUnavailable
            | ErrorCode::InvalidWorkerResult
            | ErrorCode::SemanticInspectionDeterminismViolation
    )
}
