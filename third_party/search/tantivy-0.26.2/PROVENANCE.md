# Tantivy 0.26.2 PoC dependency patch

- Source: published crates.io `tantivy 0.26.2` archive, SHA-256 `861facfabd71044968f364837f9a083b56464ba5a59079f88706ee5c451ca069` (matches the original PoC Cargo.lock checksum).
- Upstream: <https://github.com/quickwit-oss/tantivy>; archive `.cargo_vcs_info.json` records commit `72d1ef9a6468aa68bbc69dcc80cdf60aaf64364d`.
- License: MIT; upstream `LICENSE` is retained here.
- Scope: `experiments/search-discovery-poc/Cargo.toml` redirects Tantivy to this copy. No production workspace crate or root `[patch.crates-io]` uses it.
- Only source change: `Cargo.toml` `[dependencies.lru]` changes `version = "0.16.3"` to `version = "=0.18.2"`. The published 0.26.2 requirement allows `0.16.x` and excludes fixed `0.18.2`, so a direct Cargo lock update cannot resolve the advisory. `Cargo.toml.orig` remains unmodified for comparison.
- Reason: replace transitive `lru 0.16.4` affected by `RUSTSEC-2026-0253` with fixed `0.18.2` while retaining the Phase B Tantivy baseline. This is a dependency patch, not a production security approval.
- Packaging: retain manifests, library source, explicitly declared examples/tests/benches, README, authors, license, and registry VCS metadata. Omit upstream automation, documentation, and its package-local Cargo.lock; a dependency's own lockfile is not used by the PoC build and still resolved vulnerable `lru 0.16.4`.
- Removal condition: replace this patch with a published Tantivy release whose dependency graph resolves a fixed `lru`, after repeating the PoC lexical and dependency checks. Production adoption remains a separate decision.
