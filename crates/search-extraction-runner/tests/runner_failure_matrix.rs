#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};
use std::{collections::BTreeMap, path::PathBuf};

#[cfg(target_os = "linux")]
use search_core::knowledge_unit::{ArchiveProfilePlan, ArchiveReaderNode, RawBinding};
use search_core::knowledge_unit::{
    BudgetKey, ExtractionProfileDefinitionV1, FormatId, FormatSettings,
};
#[cfg(target_os = "linux")]
use search_extraction_core::{
    BodyCoverage, CoverageReason, PermanentFailureCode, RetryableFailureCode, WorkerOperation,
    WorkerRequest, validate_worker_report,
};
use search_extraction_core::{ExtractionError, RegisteredProfile};
use search_extraction_runner::{SearchExtractionRunner, SearchRunnerConfig};
#[cfg(target_os = "linux")]
use sha2::{Digest, Sha256};

fn hostile_worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_search-extraction-hostile-worker"))
}

fn definition(format: FormatId) -> ExtractionProfileDefinitionV1 {
    let mut limits: BTreeMap<_, _> = BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect();
    limits.insert(BudgetKey::InputBytes, 1024);
    limits.insert(BudgetKey::Units, 8);
    limits.insert(BudgetKey::UnitUtf8Bytes, 1024);
    limits.insert(BudgetKey::WorkerOutputBytes, 16_777_216);
    if format == FormatId::Zip {
        limits.insert(BudgetKey::ZipEntries, 2);
        limits.insert(BudgetKey::ZipEntryBytes, 1024);
        limits.insert(BudgetKey::ZipTotalBytes, 1024);
        limits.insert(BudgetKey::ZipDepth, 1);
    }
    if format == FormatId::Pdf {
        limits.insert(BudgetKey::PdfPages, 10);
        limits.insert(BudgetKey::PdfOperations, 100);
    }
    ExtractionProfileDefinitionV1 {
        format,
        parser_name: "qualified-reader".into(),
        parser_version: "1".into(),
        parser_build_sha256: [7; 32],
        native_binary_sha256: (format == FormatId::Pdf).then_some([8; 32]),
        scope_revision: 1,
        segmentation_revision: 1,
        normalization_revision: 1,
        locator_revision: 1,
        format_settings: match format {
            FormatId::Text => FormatSettings::Text {
                charset: "utf-8".into(),
            },
            FormatId::Zip => FormatSettings::Archive {
                member_decoder: "utf-8".into(),
            },
            _ => FormatSettings::None,
        },
        limits,
    }
}

fn profile(format: FormatId) -> RegisteredProfile {
    RegisteredProfile::register_definition(definition(format)).unwrap()
}

#[cfg(target_os = "linux")]
fn archive_profile() -> RegisteredProfile {
    RegisteredProfile::register_archive(ArchiveProfilePlan {
        nodes: vec![
            ArchiveReaderNode {
                members: vec![],
                parser_build_id: "outer".into(),
                definition: definition(FormatId::Zip),
            },
            ArchiveReaderNode {
                members: vec!["leaf.txt".into()],
                parser_build_id: "inner".into(),
                definition: definition(FormatId::Text),
            },
        ],
        used_leaf_chains: vec![vec!["leaf.txt".into()]],
    })
    .unwrap()
}

#[cfg(target_os = "linux")]
fn request(profile: &RegisteredProfile, raw: &[u8]) -> WorkerRequest {
    WorkerRequest {
        operation: WorkerOperation::Extract,
        format: profile.definition().format,
        profile: profile.id().clone(),
        profile_bytes: profile.profile_bytes().to_vec(),
        expected_raw: RawBinding {
            sha256: Sha256::digest(raw).into(),
            size_bytes: raw.len() as u64,
            media_type: match profile.definition().format {
                FormatId::Pdf => "application/pdf",
                FormatId::Zip => "application/zip",
                _ => "text/plain",
            }
            .into(),
        },
        budgets: profile.budgets().clone(),
    }
}

#[cfg(target_os = "linux")]
fn runner(profile: RegisteredProfile) -> SearchExtractionRunner {
    SearchExtractionRunner::new(SearchRunnerConfig::new(hostile_worker()), vec![profile]).unwrap()
}

#[test]
#[cfg(target_os = "linux")]
fn structured_resource_limit_is_unsupported_with_zero_units() {
    let profile = profile(FormatId::Text);
    let raw = b"protocol-resource";
    let report = runner(profile.clone())
        .extract(raw, request(&profile, raw))
        .unwrap();
    assert_eq!(
        report.coverage,
        BodyCoverage::Unsupported {
            reason: CoverageReason::ResourceLimit
        }
    );
    assert!(report.fragments.is_empty());
    assert!(!report.traversal_complete);
    validate_worker_report(&report, &profile).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn archive_plan_resource_limit_is_unsupported_with_zero_units() {
    let profile = archive_profile();
    assert_eq!(profile.definition().format, FormatId::Zip);
    assert_eq!(profile.archive_plan().unwrap().nodes.len(), 2);
    let raw = b"protocol-resource";
    let report = runner(profile.clone())
        .extract(raw, request(&profile, raw))
        .expect("structured ZIP resource limit must complete without Units");
    assert_eq!(
        report.coverage,
        BodyCoverage::Unsupported {
            reason: CoverageReason::ResourceLimit
        }
    );
    assert!(report.fragments.is_empty());
    assert_eq!(report.scope_items, 0);
    assert!(report.known_omissions.is_empty());
    assert!(!report.traversal_complete);
    validate_worker_report(&report, &profile).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn archive_encrypted_failure_is_zero_output_and_revalidates() {
    let profile = archive_profile();
    let raw = b"protocol-encrypted";
    let report = runner(profile.clone())
        .extract(raw, request(&profile, raw))
        .unwrap();
    assert_eq!(
        report.coverage,
        BodyCoverage::Unsupported {
            reason: CoverageReason::Encrypted
        }
    );
    assert!(!report.traversal_complete);
    assert!(report.reader_use.is_empty());
    validate_worker_report(&report, &profile).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn worker_protocol_exit_79_is_non_retryable_incident() {
    let profile = profile(FormatId::Text);
    let raw = b"protocol-exit-79";
    let error = runner(profile.clone())
        .extract(raw, request(&profile, raw))
        .expect_err("worker protocol exit cannot be a completed report");
    assert!(
        matches!(
            &error,
            ExtractionError::Integrity(_) | ExtractionError::Configuration(_)
        ),
        "protocol exit 79 must be an integrity/configuration incident: {error:?}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn relative_worker_path_is_rejected_as_host_configuration_before_run() {
    let error = SearchExtractionRunner::new(
        SearchRunnerConfig::new(PathBuf::from("relative/hostile-worker")),
        vec![profile(FormatId::Text)],
    )
    .expect_err("relative worker path must be rejected at construction");
    assert!(matches!(error, ExtractionError::Configuration(_)));
}

#[test]
#[cfg(target_os = "linux")]
fn complete_report_and_locator_resolution_return_only_validated_fragments() {
    let profile = profile(FormatId::Text);
    let runner = runner(profile.clone());
    let raw = b"protocol-report";
    let report = runner.extract(raw, request(&profile, raw)).unwrap();
    assert_eq!(report.coverage, BodyCoverage::Supported);
    assert!(report.fragments.is_empty());
    assert!(report.traversal_complete);

    let mut resolve = request(&profile, raw);
    resolve.operation = WorkerOperation::ResolveLocators(Vec::new());
    assert!(runner.resolve_locators(raw, resolve).unwrap().is_empty());
}

#[test]
#[cfg(target_os = "linux")]
fn typed_permanent_retryable_and_worker_failures_never_return_partial_report() {
    let profile = profile(FormatId::Text);
    let runner = runner(profile.clone());
    for (raw, expected) in [
        (
            b"protocol-permanent".as_slice(),
            ExtractionError::Permanent(PermanentFailureCode::CorruptDocument),
        ),
        (
            b"protocol-retryable".as_slice(),
            ExtractionError::Retryable(RetryableFailureCode::WorkerUnavailable),
        ),
        (
            b"kill".as_slice(),
            ExtractionError::Retryable(RetryableFailureCode::WorkerKilled),
        ),
        (
            b"panic".as_slice(),
            ExtractionError::Retryable(RetryableFailureCode::WorkerKilled),
        ),
        (
            b"protocol-partial".as_slice(),
            ExtractionError::Integrity("worker report"),
        ),
        (
            b"protocol-truncated".as_slice(),
            ExtractionError::Integrity("worker response"),
        ),
    ] {
        let error = runner
            .extract(raw, request(&profile, raw))
            .expect_err("no success report");
        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&expected),
            "{raw:?}: {error:?}"
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn result_cap_is_permanent_timeout_retryable_and_no_unbounded_retries() {
    let profile = profile(FormatId::Text);
    let runner = SearchExtractionRunner::new(
        SearchRunnerConfig::new(hostile_worker()).with_wall_timeout(Duration::from_millis(250)),
        vec![profile.clone()],
    )
    .unwrap();
    let raw = b"stdout-overflow";
    assert!(matches!(
        runner.extract(raw, request(&profile, raw)),
        Err(ExtractionError::Permanent(
            PermanentFailureCode::WorkerOutputLimit
        ))
    ));

    let raw = b"timeout";
    let started = Instant::now();
    assert!(matches!(
        runner.extract(raw, request(&profile, raw)),
        Err(ExtractionError::Retryable(RetryableFailureCode::Timeout))
    ));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "runner replayed a timed out worker"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn host_checks_raw_binding_and_pdfium_pin_before_worker_launch() {
    let text_profile = profile(FormatId::Text);
    let mut forged = request(&text_profile, b"protocol-report");
    forged.expected_raw.sha256 = [0; 32];
    assert!(matches!(
        runner(text_profile).extract(b"protocol-report", forged),
        Err(ExtractionError::Integrity(_))
    ));

    let pdf_profile = profile(FormatId::Pdf);
    let private = tempfile::tempdir().unwrap();
    std::fs::write(private.path().join("libpdfium.so"), b"wrong-native-binary").unwrap();
    let runner = SearchExtractionRunner::new(
        SearchRunnerConfig::new(hostile_worker()).with_pdfium_runtime_dir(private.path()),
        vec![pdf_profile.clone()],
    )
    .unwrap();
    let raw = b"%PDF-1.7";
    assert!(matches!(
        runner.extract(raw, request(&pdf_profile, raw)),
        Err(ExtractionError::Configuration(_))
    ));
}

#[test]
#[cfg(target_os = "linux")]
fn failed_mandatory_seal_is_configuration_incident_not_item_success() {
    let profile = profile(FormatId::Text);
    let raw = b"protocol-no-marker";
    assert!(matches!(
        runner(profile.clone()).extract(raw, request(&profile, raw)),
        Err(ExtractionError::Configuration(_))
    ));
}

#[test]
#[cfg(not(target_os = "linux"))]
fn search_runner_refuses_non_linux_host() {
    let error = SearchExtractionRunner::new(
        SearchRunnerConfig::new(hostile_worker()),
        vec![profile(FormatId::Text)],
    )
    .expect_err("Linux sandbox required");
    assert!(matches!(error, ExtractionError::Configuration(_)));
}
