use std::process::Command;

use search_discovery_poc::report::{GateVerdict, QualificationReport};

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_search-discovery-poc"))
        .args(args)
        .output()
        .expect("PoC CLI starts")
}

#[test]
fn verify_subcommand_checks_the_harness_without_claiming_selection() {
    let output = run(&["verify"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "harness verified\n"
    );
}

#[test]
fn json_report_has_deterministic_qualification_schema() {
    let first = run(&["report", "--format", "json"]);
    let second = run(&["report", "--format", "json"]);
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);

    let report: QualificationReport = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report.fixture_set, "harness-v0");
    assert!(report.candidate_versions.is_empty());
    assert!(report.lexical_metrics.is_empty());
    assert!(report.graph_correctness_metrics.is_empty());
    assert!(report.graph_traversal_measurements.is_empty());
    assert_eq!(
        report.dependency_license_gate.dependency,
        GateVerdict::Pending
    );
    assert_eq!(report.dependency_license_gate.license, GateVerdict::Pending);
}
