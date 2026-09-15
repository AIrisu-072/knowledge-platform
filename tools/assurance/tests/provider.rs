use assurance_cli::capability::Capability;
use assurance_cli::evidence::Outcome;
use assurance_cli::provider::{CommandProvider, Provider};

#[test]
fn command_provider_records_pass_evidence_and_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let capability = Capability {
        id: "CAP-TEST".into(),
        controls: vec!["CTRL-TEST".into()],
        provider: "command".into(),
        mechanism: "static".into(),
        oracle: "test".into(),
        cost: "fast".into(),
        command: vec!["git".into(), "--version".into()],
        scope_paths: vec![".".into()],
    };
    let provider = CommandProvider::new(dir.path(), "deadbeef");

    let evidence = provider.run(&capability).unwrap();

    assert_eq!(evidence.capability_id, "CAP-TEST");
    assert_eq!(evidence.outcome, Outcome::Pass);
    assert_eq!(evidence.commit, "deadbeef");
    let artifact = evidence.artifact_path.unwrap();
    assert!(dir.path().join(artifact).is_file());
    assert!(dir.path().join("target/assurance/evidence/CAP-TEST.json").is_file());
}
