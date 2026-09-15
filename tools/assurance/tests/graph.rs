use assurance_cli::graph::Graph;
use std::fs;

#[test]
fn builds_level_zero_and_one_graph_from_repository_and_cargo_metadata() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("mise.toml"), "[tools]\n").unwrap();
    fs::create_dir_all(dir.path().join("tools/architecture-lint")).unwrap();
    fs::create_dir_all(dir.path().join("tools/assurance")).unwrap();
    fs::write(dir.path().join("tools/architecture-lint/Cargo.toml"), "[package]\nname='architecture-lint'\n").unwrap();
    fs::write(dir.path().join("tools/assurance/Cargo.toml"), "[package]\nname='assurance-cli'\n").unwrap();

    let root = dir.path().canonicalize().unwrap();
    let metadata = format!(r#"{{
      "packages": [
        {{"name":"architecture-lint","id":"pkg-arch","manifest_path":"{}","dependencies":[]}},
        {{"name":"assurance-cli","id":"pkg-assure","manifest_path":"{}","dependencies":[{{"name":"architecture-lint"}}]}}
      ],
      "workspace_members": ["pkg-arch", "pkg-assure"]
    }}"#,
        root.join("tools/architecture-lint/Cargo.toml").display(),
        root.join("tools/assurance/Cargo.toml").display(),
    );

    let graph = Graph::from_metadata_json(&root, &metadata).unwrap();
    assert!(graph.component("tools/architecture-lint").is_some());
    assert!(graph.component("tools/assurance").is_some());
    assert!(graph.files().any(|p| p.ends_with("mise.toml")));
    assert!(graph.depends_on("tools/assurance", "tools/architecture-lint"));
}
