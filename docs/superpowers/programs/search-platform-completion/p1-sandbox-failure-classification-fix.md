# P1-I02 failure classification correction — 2026-10-01

Status: local current-source GREEN; independent bounded GO. Whole P1, real format readers, Source integration and hosted exact-head qualification remain open. Branch `feat/search-platform-completion-core`, baseline HEAD `80a47960d025e4dfdea1eacade28b15d218725ff`; these changes are uncommitted.

## RED and correction

The first Linux compilation exposed an existing test variable shadowing the `profile` helper (E0618). Renaming only that test local allowed the named regression RED: six existing cases passed, and exactly ZIP ResourceLimit, exit 79 and relative worker path failed. Evidence: `/tmp/search-completion-resume-20261001/p1-red.log`, SHA-256 `1ed475c76791bd3c178050379565778e9f15b95ba41f61bf882c8c82e6d834f2`.

The constructor now rejects a nonabsolute worker executable as Configuration. Worker exit 79 is Integrity even with empty stderr; an unclassified kill remains Retryable. Structured Unsupported failures produce zero Units and do not invent visited archive reader nodes.

Ruling: extend the originally executor-only correction to the pure report validator and its tests. Independent review found that skipping report validation returned a noncomposable archive report and falsely claimed complete traversal. Frozen §6 explicitly permits a parse-time structured Unsupported terminal outcome with Unit 0; the protocol already distinguishes full traversal from interrupted Supported/Partial. The correction truthfully sets `traversal_complete=false` and accepts only Unsupported + zero fragments, reader-use, scope and omissions, after registered-profile and serialized-size checks. All positive and nonzero incomplete reports retain the existing rejection. This is a semantic clarification of the frozen terminal outcome, with no positive/absence authority. Cost if wrong: a Source adapter would misclassify an unsupported item; the returned report now passes the same host validator and has no positive witness.

The new false-traversal regression failed for Text and ZIP, and the new pure contract failed with `Integrity("incomplete traversal")` before the validator correction. Logs: `p1-composable-red.log` (`d2478bc4d0777042c0f573425e9646782fe1bd8afe4e38b577bfa83dd91fe833`) and `p1-terminal-red.log` (`0c9c2d99ba48f2e06b1ae1e6c52798986930d7dc3c6eba76d604912ccc9e6871`) in that same local evidence directory. Encrypted ZIP also revalidates; forged scope, fragments, omissions, reader-use and Supported coverage cannot use the terminal exception.

## Current-source verification

Existing pinned arm64 `rust:1.98.1-bookworm` image, source read-only, offline Cargo registry, debug=0/incremental=0/jobs=2 and one advisory Cargo lock were used. No existing container or persistent data was deleted. These are real local Linux process tests, not hosted CI or production readiness.

- `cargo test --offline --locked -p search-extraction-runner -p search-extraction-core -p document-sandbox-runner -p search-extraction-worker -- --test-threads=1`: Core 12/12, matrix 10/10, isolation 5/5 PASS. Final log `p1-final-test.log`, SHA-256 `e7f6c940844cdd2af7e366d603c3862faa9294dbcb464e9ae131b909362b91a8`.
- Existing DSI Linux `runner_baseline` 1/1 and `runner_isolation` 6/6 PASS. Log `dsi-regression.log`, SHA-256 `be30067489940930f8f3516ac170a5dcb39f639520a570e553dd2fcbd16a4808`.
- Final terminal-contract Linux strict Clippy PASS for the four P1 crates plus `document-semantic-inspection-runner`, all targets with `-D warnings`; `p1-final-clippy.log` records exit 0.
- Current workspace `cargo fmt --all -- --check` and `git diff --check` PASS.

| File | SHA-256 |
| --- | --- |
| `search-extraction-runner/src/executor.rs` | `b08731eb198be25de3ec83e5d1259fd55e6be35a9816a1064afb9ceeea34b06e` |
| `search-extraction-core/src/validation.rs` | `adc4918f6422704578780b9dc35e9683511eb08c63246f2e29505e7b8a191b6b` |
| `search-extraction-core/tests/extraction_contract.rs` | `6bd7e55f73da93020c31f4836f56ec46e9d32b80dc485a7198059af518f9b327` |
| `search-extraction-runner/tests/runner_failure_matrix.rs` | `c6897ee78a586d43b5604936764cb952d3816a8b80339111b6d99473faf9f1b1` |
| `search-extraction-runner/tests/support/hostile_worker.rs` | `432fb41954b361773a2ba1d061dc9858d9cf2321156d4dd5e712a4651e63a022` |

No commit, merge, deploy or production migration is claimed. The logs prove this source snapshot, not the baseline Git HEAD.
