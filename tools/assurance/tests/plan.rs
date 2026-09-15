use assurance_cli::capability::Capability;
use assurance_cli::graph::Graph;
use assurance_cli::planner::plan;
use assurance_cli::requirement::{Criticality, Requirement};
use std::path::PathBuf;

fn requirement(id: &str) -> Requirement {
    Requirement {
        id: id.into(),
        kind: "architecture".into(),
        criticality: Criticality::Medium,
        domain: "development".into(),
        source: PathBuf::from("spec/architecture/development-container-ci-architecture-v0.md"),
        section: None,
    }
}

fn capability() -> Capability {
    Capability {
        id: "CAP-ARCH-BOOTSTRAP-POLICY".into(),
        controls: vec![
            "CTRL-DEV-GITHUB-HOSTED-CI".into(),
            "CTRL-DEV-MISE-TASK-SSOT".into(),
            "CTRL-DEV-PRODUCTION-LINUX-AMD64".into(),
        ],
        provider: "command".into(),
        mechanism: "static".into(),
        oracle: "architecture-contract".into(),
        cost: "fast".into(),
        command: vec!["architecture-lint".into(), "check".into()],
        scope_paths: vec![
            ".github/".into(),
            "mise.toml".into(),
            "Dockerfile".into(),
            "spec/architecture/".into(),
        ],
    }
}

#[test]
fn ci_change_plans_hosted_ci_requirement_and_bootstrap_capability() {
    let graph = Graph::empty();
    let requirements = vec![
        requirement("REQ-DEV-GITHUB-HOSTED-CI"),
        requirement("REQ-DEV-MISE-TASK-SSOT"),
        requirement("REQ-DEV-PRODUCTION-LINUX-AMD64"),
    ];
    let plan = plan(
        &graph,
        &requirements,
        &[capability()],
        &[PathBuf::from(".github/workflows/ci.yml")],
    );
    assert_eq!(plan.requirements, vec!["REQ-DEV-GITHUB-HOSTED-CI"]);
    assert_eq!(plan.capabilities, vec!["CAP-ARCH-BOOTSTRAP-POLICY"]);
    assert!(plan.gaps.is_empty());
}

#[test]
fn unrelated_docs_change_does_not_plan_bootstrap_capability() {
    let graph = Graph::empty();
    let requirements = vec![requirement("REQ-DEV-GITHUB-HOSTED-CI")];
    let plan = plan(
        &graph,
        &requirements,
        &[capability()],
        &[PathBuf::from("docs/research/note.md")],
    );
    assert!(plan.requirements.is_empty());
    assert!(plan.capabilities.is_empty());
}
