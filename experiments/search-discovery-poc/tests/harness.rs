use std::process::Command;

use search_discovery_poc::report::{GateVerdict, QualificationReport};
use sha2::{Digest, Sha256};

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_search-discovery-poc"))
        .args(args)
        .output()
        .expect("PoC CLI starts")
}

#[test]
fn verify_subcommand_checks_the_recorded_evidence_without_claiming_selection() {
    let output = run(&["verify"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "qualification receipt source and stable quality verified; captured timings not rerun\n"
    );
}

#[test]
fn receipt_rejects_stale_quality_version_and_failed_license() {
    let original = QualificationReport::evidence().unwrap();
    let mut stale_quality = original.clone();
    stale_quality.lexical_metrics[0]["mrr"] = serde_json::json!(0.0);
    assert!(stale_quality.verify_evidence().is_err());

    let mut stale_version = original.clone();
    stale_version
        .candidate_versions
        .insert("tantivy".into(), "0.0.0".into());
    assert!(stale_version.verify_evidence().is_err());

    let mut failed_license = original;
    failed_license.dependency_license_gate.license = GateVerdict::Fail;
    assert!(failed_license.verify_evidence().is_err());

    let mut undisclosed_exception = QualificationReport::evidence().unwrap();
    undisclosed_exception
        .dependency_license_gate
        .advisory_exception = None;
    assert!(undisclosed_exception.verify_evidence().is_err());
}

#[test]
fn json_report_has_deterministic_qualification_schema() {
    let first = run(&["report", "--format", "json"]);
    let second = run(&["report", "--format", "json"]);
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);

    let report: QualificationReport = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report.fixture_set, "search-discovery-synthetic-v0");
    assert_eq!(report.candidate_versions["tantivy"], "0.26.2");
    assert!(!report.lexical_metrics.is_empty());
    assert!(!report.graph_correctness_metrics.is_empty());
    assert!(!report.graph_traversal_measurements.is_empty());
    assert!(!report.fusion_metrics.is_empty());
    assert_eq!(report.dependency_license_gate.dependency, GateVerdict::Pass);
    assert_eq!(report.dependency_license_gate.license, GateVerdict::Pending);
    assert_eq!(
        report.dependency_license_gate.advisory_exception.as_deref(),
        Some("RUSTSEC-2026-0253")
    );
}

#[test]
fn recorded_fixture_hashes_match_the_inputs() {
    let output = run(&["report", "--format", "json"]);
    let report: QualificationReport = serde_json::from_slice(&output.stdout).unwrap();
    for (name, content) in [
        (
            "lexical/resources.json",
            include_bytes!("../fixtures/lexical/resources.json").as_slice(),
        ),
        (
            "lexical/queries.json",
            include_bytes!("../fixtures/lexical/queries.json").as_slice(),
        ),
        (
            "graph/relations.json",
            include_bytes!("../fixtures/graph/relations.json").as_slice(),
        ),
        (
            "graph/generate.rs",
            include_bytes!("../fixtures/graph/generate.rs").as_slice(),
        ),
        (
            "fusion/cases.json",
            include_bytes!("../fixtures/fusion/cases.json").as_slice(),
        ),
    ] {
        let observed = Sha256::digest(content)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(report.fixture_sha256[name], observed);
    }
}
