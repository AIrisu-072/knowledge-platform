# Search / Discovery Platform v0 — Tantivy dependency patch candidate

Date: 2026-09-29 JST. Branch: `feat/search-discovery-platform-v0-tantivy-patch`, based on Phase B head `2985dc1d0fa5b9f56de9434e64513b00e35f3e4f`. This is a PoC-only dependency remediation candidate. It does not add Tantivy to a production crate or select the S1 fusion policy.

## Decision and source

The requester chose to evaluate a narrow Tantivy patch instead of replacing the lexical engine with a composition of other libraries. The published Tantivy 0.26.2 manifest requires `lru 0.16.x`, while [RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html) is fixed in `lru 0.18.2`. [Tantivy upstream made the same dependency-line change](https://github.com/quickwit-oss/tantivy/commit/5ca39332002c2c87fb5d2abc707cf527b3319d42), but that change is not in the published 0.26.2 crate.

The local copy at `third_party/search/tantivy-0.26.2` comes from the published crates.io archive with SHA-256 `861facfabd71044968f364837f9a083b56464ba5a59079f88706ee5c451ca069`. An archive comparison found one changed retained source file: `Cargo.toml`, where the `lru` requirement is `=0.18.2` instead of `0.16.3`. The 44 omitted archive files are upstream automation, documentation, and its package-local lockfile; the retained 330 archive files are otherwise byte-identical. `PROVENANCE.md` records the source, license, scope and removal condition. The PoC manifest redirects only Tantivy to this copy, and its lockfile resolves `lru 0.18.2`. The old PoC-only OSV advisory exception is removed.

## Local verification

- `cargo test --locked --manifest-path experiments/search-discovery-poc/Cargo.toml`: passed; lexical 3/3, fusion 8/8, HyperEdge 5/5, PostgreSQL backend 2/2, receipt 6/6, report unit 1/1.
- `cargo deny --manifest-path experiments/search-discovery-poc/Cargo.toml --config experiments/search-discovery-poc/deny.toml check`: advisories, bans, licenses and sources passed. Existing duplicate-version warnings remain.
- OSV Scanner 2.5.1 `scan source -r .`: scanned all repository lockfiles with no issues and no Search advisory exception.
- PoC `cargo clippy --locked --all-targets -- -D warnings` and `cargo fmt --check`: passed. A staged diff whitespace check scoped to authored files passed. The byte-identical upstream source retains five pre-existing whitespace findings, so the whole staged `git diff --check` exits 2; those source lines were not reformatted.
- Receipt verification now records locked `lru 0.18.2`, requires no advisory exception, and rejects the old version or old exception. Captured Phase B timing measurements were not rerun; the deterministic quality values were rechecked by the test suite.

An independent read-only review was dispatched with the requester-specified `gpt-6-sol / max`, but it could not start because that model hit its usage limit. No independent review result is claimed. The earlier Phase B independent reviews covered the pre-patch head, not this patch.

## Remaining gates

This patch establishes a locally verified way to eliminate the known `lru` advisory from the isolated PoC. A production `crates/search-tantivy` dependency and root Cargo patch have not been added; their source, license and security checks must be repeated on the exact production adapter head. S1 has no selected initial fusion policy, and Lindera dictionary asset rights/packaging remain unresolved. Phase C must not start from this patch alone. Keep the patch PR Draft and stacked on Phase B until independent review and hosted exact-head gates are complete.
