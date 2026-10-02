# Search checkpoint: exact secret-scan exceptions

## Scope and rationale

The requester approved these 31 exact secret-scan exceptions on 2026-10-02. This security-only change is based on main `d71753d46590bb4406a1c0b74894ab90a27a6c88`. Repository policy `spec/architecture/development-container-ci-architecture-v0.md` section 28 requires repository-managed, reasoned, narrowly scoped false-positive exceptions.

The root `.gitleaksignore` contains exactly 31 `commit:path:generic-api-key:line` fingerprints, all from immutable Search checkpoint `99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a`:

- 14 findings in `docs/superpowers/programs/search-platform-completion/p2-executed-input-source-inventory-20261001.json`
- 14 findings in `experiments/search-vector-poc/baseline-refinement-run-manifest.json`
- One finding each at `experiments/search-graph-poc/qualification_runtime.py:175`, `experiments/search-graph-poc/qualification_recovery.py:358`, and `experiments/search-graph-poc/report.md:60`

The first 28 are SHA-256 integrity digests of vendored Tantivy source files. Each referenced file was independently rehashed from that exact Git commit; all 28 match. Examples are `examples/custom_tokenizer.rs`, `examples/pre_tokenized_text.rs`, and `src/collector/sort_key/sort_key_computer.rs` under `third_party/search/tantivy-0.26.2`. Token/key substrings in the manifest filenames accompany long hexadecimal digests; these fields are source-integrity metadata, not authentication tokens.

The last three occurrences are the same synthetic Neo4j fixture authentication setting. Static inspection confirms disposable candidate/restore Docker containers, with the published port bound to `127.0.0.1`; the report explicitly identifies synthetic local credentials. No authentication values are reproduced in this note. This bounded classification does not establish that the repository is free of other secrets.

## Fresh bounded verification

Used repository-pinned Gitleaks **8.30.1**. The official release archive SHA-256 was rechecked as `551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb` before execution.

Both history scans used the same command and revision, with redacted JSON reports outside Git:

```sh
gitleaks git --redact --exit-code 1 --report-format json \
  --report-path <outside-repository-report> \
  --log-opts=99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a .
```

- Before `.gitleaksignore`: exit 1, 678 commits, 31 findings
- After `.gitleaksignore`: exit 0, the same 678 commits, 0 findings
- Before-scan fingerprint set exactly equals the 31 unique ignore entries; no other finding was suppressed in this comparison
- Executed the existing, unchanged `security:secrets:self-test` shell body from `mise.toml` with the pinned scanner on PATH. Its synthetic non-live PAT-shaped probe still produces one finding and scanner exit 42; the self-test succeeds only for that expected rejecting exit

The history range is intentionally identical to the original bounded triage, narrower than CI's all-ref history. These results do not substitute for exact-head hosted security checks on each receiving PR or full program qualification.

## Unchanged protections and next action

No scanner rule, version, workflow, scan scope, fixture value, source hash, history, or existing synthetic self-test is changed. There is no whole-path, whole-commit, regex, baseline, or rule-wide exclusion. A new finding with a different fingerprint remains subject to detection. The scanner's exact-fingerprint matching is version-sensitive and must continue to be verified using the repository pin.

Only this note and `.gitleaksignore` are included. Search implementation, its existing safety hold, and all unrelated CI failures remain unchanged. The next action is independent review of these two files and the bounded verification, followed by separately authorized publication and exact-head hosted checks. No merge or deployment is included.
