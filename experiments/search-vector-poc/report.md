# P2 weight-free baseline evidence

Observed 2026-09-30 16:43 JST in `feat/search-platform-completion-core` worktree, macOS arm64. The checkout HEAD observed after the run was `80a47960d025e4dfdea1eacade28b15d218725ff`; shared Search files were concurrently dirty, so this is a local working-tree receipt rather than exact-head CI qualification. Fixture seed `20260930`; Resource scales 32 → 256 → 1024; four queries per arm/scale. Source, text, relation, Read, Version, and temporal states are synthetic. Full output is preserved in `baseline-run.log` (SHA-256 `208982f08c2f100c3e929d83c742119e468c50d330a617169142866a6a3d553a`); the isolated `Cargo.lock` SHA-256 is `8d5bdb829c0d5cd322c730d169087cad1f76cefb80edc83c0b7a18660705c81f`. This is a PoC baseline, not P1 parser/body qualification or production Vector selection.

## Executed path and correctness

`Corpus::synthetic` supplies bounded Japanese and mixed-language text and explicit eligible parent qrels. `TantivyLexicalIndex` uses its qualified patched 0.26.2 crate; `MemoryGraphRetriever` traverses typed n-ary relations with required context/authority and a finite budget. `SourceRouter` and `RetrieverPlanner` produce the S1 Lexical→HyperGraph action order. `RetrievalExecutor` checks current Source Read for candidates and each Graph path node. `CandidateFederator::merge(PriorityConcat)` applies hard gates and parent Resource deduplication. The runner checks the immutable Source generation pin and rechecks current Read/Version before emitting final ranks.

For each positive query, the L rank list is the two Lexical parents; LG adds both Graph-only parents. Graph also returns the first Lexical parent, which appears only once after S1 federation. The related Graph distractor has the wrong relation authority; denied and unknown Read hits leave no raw authorized rank; expired Lexical hits occur in the raw stage trace but fail the temporal gate. `qnone` is reported separately with undefined relevance ratios.

| Scale | Arm | Recall@1 | Recall@5 | Recall@10/20 | MRR@10 | nDCG@10 | Positive / no-positive queries |
| ---: | :---: | ---: | ---: | ---: | ---: | ---: | :--- |
| 32 | L | 0.2500 | 0.5000 | 0.5000 | 1.0000 | 0.6663 | 3 / 1 |
| 32 | LG | 0.2500 | 1.0000 | 1.0000 | 1.0000 | 0.9608 | 3 / 1 |
| 256 | L | 0.2500 | 0.5000 | 0.5000 | 1.0000 | 0.6663 | 3 / 1 |
| 256 | LG | 0.2500 | 1.0000 | 1.0000 | 1.0000 | 0.9608 | 3 / 1 |
| 1024 | L | 0.2500 | 0.5000 | 0.5000 | 1.0000 | 0.6663 | 3 / 1 |
| 1024 | LG | 0.2500 | 1.0000 | 1.0000 | 1.0000 | 0.9608 | 3 / 1 |

The same 30 challenge Resources are present at every scale; additional Resources are seeded background records. Stable relevance scores across scales therefore measure harness behavior with larger indexes, not a harder relevance distribution. Recall divides by all four eligible positive parents per positive query. nDCG gain is `2^grade - 1`; denominator sorts all eligible gold grades. Metrics are on final deduplicated parent ranks.

## Timings and memory

One retained sequential run; duration is monotonic `Instant`, milliseconds. Each arm has 4 query and 4 fusion samples. The log labels each arm's first query `cold`: only the L first query is cold immediately after build. LG follows L on the same index, so its first query has a warm Lexical index and a first Graph traversal; its value is a **first-in-arm** sample, not an independent cold LG sample. The other three queries per arm are warm. p50/p95/p99 use nearest-rank sample percentiles, so p95 and p99 are the maximum with n=4. Build and next-generation update are one sample each per scale, shared between L/LG. The update rebuilt immutable Lexical and Graph generations after one Source-supplied canonical-name change. RSS is current process KiB from `ps -o rss= -p PID` after each arm; the process is shared across sequential scales.

| Scale | Arm | Query p50/p95/p99 ms | First-in-arm / remaining-three p95 ms | Fusion p50/p95/p99 ms | RSS KiB |
| ---: | :---: | :--- | :--- | :--- | ---: |
| 32 | L | 6.441 / 8.954 / 8.954 | 8.014 / 8.954 | 0.027 / 0.056 / 0.056 | 23,488 |
| 32 | LG | 6.300 / 6.772 / 6.772 | 6.772 / 6.752 | 0.037 / 0.040 / 0.040 | 23,616 |
| 256 | L | 5.980 / 6.609 / 6.609 | 6.609 / 6.100 | 0.026 / 0.037 / 0.037 | 26,432 |
| 256 | LG | 5.929 / 6.501 / 6.501 | 6.501 / 6.289 | 0.028 / 0.044 / 0.044 | 26,432 |
| 1024 | L | 6.366 / 6.575 / 6.575 | 6.575 / 6.463 | 0.028 / 0.036 / 0.036 | 31,760 |
| 1024 | LG | 6.299 / 7.192 / 7.192 | 6.800 / 7.192 | 0.035 / 0.043 / 0.043 | 31,776 |

| Scale | Initial Lexical build ms | Initial Graph build ms | Next-generation Lexical update ms | Next-generation Graph update ms | File-backed index bytes |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 32 | 24.955 | 0.399 | 20.133 | 0.140 | 0 |
| 256 | 40.848 | 0.584 | 40.189 | 0.550 | 0 |
| 1024 | 112.569 | 2.009 | 114.470 | 1.935 | 0 |

`TantivyLexicalIndex` calls `Index::create_in_ram`; Graph uses an in-process map. File-backed index bytes are therefore zero by storage mode, not a production disk-footprint estimate. The owned disposable Rust `target/` occupied 610 MiB after test, run and Clippy builds; it is separate from index bytes and was cleaned after preserving this report and `baseline-run.log` (`cargo clean --manifest-path ...`, 603.8 MiB removed). Host free disk fell from 3.2 GiB before the retained run to 1.5 GiB after concurrent work, then rose to 1.9 GiB after that cleanup. No host contention instrumentation or repeated distribution is included, so these small-sample timings are not an SLO or comparative engine benchmark.

## Verification receipts

Exact final checks from repository root:

```sh
cargo fmt --manifest-path experiments/search-vector-poc/Cargo.toml
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --manifest-path experiments/search-vector-poc/Cargo.toml --offline --locked --tests
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo run --manifest-path experiments/search-vector-poc/Cargo.toml --offline --locked --quiet | tee experiments/search-vector-poc/baseline-run.log
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo clippy --manifest-path experiments/search-vector-poc/Cargo.toml --offline --locked -p search-vector-poc --tests -- -D warnings
```

Final result: 10/10 focused tests pass; the isolated PoC Clippy check passes. `search-core` emitted two `dead_code` warnings from concurrently in-progress P1 code; this PoC did not edit that crate. The `source_binding_cannot_be_rewritten_even_with_recomputed_unit_id` test first failed because the validator accepted a recomputed UnitId with another Source. The `no_positive_query_is_separate_and_never_labeled_perfect` test first failed because undefined ratios appeared as 0. Both were fixed and then passed. The golden-vector test matches both frozen UnitId digests, Text locator bytes, and normalized text SHA-256.

Security checks cover denied/unknown current Read, current Version revocation, generation pin mismatch, related-but-ineligible Graph paths, temporal exclusion, and Vector contract failures for dimension, NaN, stale binding, scope/lease, `NO_RETENTION`, and `SESSION_ONLY`. Mock Vector tests do not report model quality. Dense model arms, true Vector retrieval, retention lease integration, P1 full-body parser/locator round trips, and production Source authority wiring remain **UNRUN**.
