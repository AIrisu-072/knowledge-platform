use assurance_cli::requirement::extract;

#[test]
fn extracts_requirement_metadata_from_normative_markdown() {
    let markdown = r#"
## CI runner policy

```requirement
id = "REQ-DEV-GITHUB-HOSTED-CI"
kind = "architecture"
criticality = "medium"
domain = "development"
```

CIはGitHub-hosted runnerのみを正式利用する。
"#;

    let reqs = extract(markdown, "spec/example.md").unwrap();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].id, "REQ-DEV-GITHUB-HOSTED-CI");
    assert_eq!(reqs[0].domain, "development");
    assert_eq!(reqs[0].section.as_deref(), Some("CI runner policy"));
}

#[test]
fn rejects_duplicate_requirement_ids_during_scan() {
    let markdown = r#"
```requirement
id = "REQ-DUP"
kind = "architecture"
criticality = "medium"
domain = "development"
```
```requirement
id = "REQ-DUP"
kind = "architecture"
criticality = "medium"
domain = "development"
```
"#;
    let reqs = extract(markdown, "spec/example.md").unwrap();
    assert_eq!(reqs.len(), 2);
}
