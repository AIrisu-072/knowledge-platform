# P2 L/LG baseline refinement — pinned PoC receipt

Observed 2026-09-30 18:31–18:33 JST on macOS arm64, branch `feat/search-platform-completion-core`, Git HEAD `80a47960d025e4dfdea1eacade28b15d218725ff` with dirty shared inputs. The PoC run manifest is **ACCEPTED** for the exact file snapshot below. The earlier `report.md` and `baseline-run.log` remain unchanged and are not reused for dense comparison.

## F1 — exact execution inputs

`scripts/pinned_baseline.py` resolves the actual offline/locked Cargo graph from the isolated PoC manifest, requires the patched Tantivy 0.26.2 path, and hashes every file in each reachable local package tree plus the root `Cargo.toml` inherited by path crates. It records package tree SHA-256 digests, file SHA-256 entries, isolated lock, toolchain, forced build settings, commands, phase exit codes, and log digest. It compares snapshots before/after each focused test, L/LG run, and focused Clippy phase; any changed source or path package rejects the run. `scripts/test_pinned_baseline.py` verifies that changed Rust bytes and a newly added `build.rs` change the tree digest.

| Actual local input | Tree SHA-256 |
| --- | --- |
| PoC package (source, fixtures, scripts, isolated lock) | `84ff15be57ef2e80c550a534115c6684f4292a523af28c811f23c7336d15d8e4` |
| `search-core` | `7e9c8e63cb2915485c044efdad087c1efb666eaacf1bd805b9b106f7307316e0` |
| `search-application` | `2928558fcf4086a621ba0f85af555226d20e028b6c2cf19b219e89e6ab6c97f2` |
| `search-graph-memory` | `5df5973f886b674b9c649a37cecb6e0bb477fda153533e8acd0785b5585dca06` |
| `search-tantivy` | `4ec91f8b3470fbde45a8617db93fdc38587f7f426bede6fda94c56d2aefe0441` |
| patched Tantivy 0.26.2, 331 files | `95d5001932577d2895e44aa6e65d6034511826838b67dbf7f9408baba32b44e8` |
| root inherited `Cargo.toml` | `ba1da79c8e2689e4efa709ecf590c5d584c32c6eb60ea86e6dd3f6e3fb7fc3a3` |

The aggregate input SHA-256 is `e491c50e915deca95756065f673b5c5ab875c724e391b00819840926c3a74de1` over 431 local package files and the root manifest. The run manifest SHA-256 is `605113e14ce8d69755014a784b056223939625288db31bea3ac1d1994cfd6d22`, and the combined test/run/Clippy log SHA-256 is `c5f04793d5d48fb6f540f2e6d68b40e6d4d3cc38df378d269f8b46bd33c05257`. The isolated `Cargo.lock` stayed at `8d5bdb829c0d5cd322c730d169087cad1f76cefb80edc83c0b7a18660705c81f`. Rust/Cargo were 1.98.1. Each of the three phase exit codes is 0, and each before/after aggregate SHA matches. The manifest contains per-file hashes and exact commands. Registry crate source is pinned by the isolated lock rather than copied into this receipt; transient edit-then-restore within one phase is outside the pre/post detection model.

## F2 — two Units, one parent

Synthetic Resource 0 has two `Text` Units from different Source-owned Parts in the same Version. The Parts have distinct raw bytes, authoritative representation refs, Part IDs/logical paths, Text locator bytes (one versus two lines), and Unit IDs. Each Unit is converted to the actual `search-core::knowledge_unit::KnowledgeUnit` and checked with `validate_part_units` against a separate synthetic Source Part binding. Recomputed Unit IDs do not allow Part/raw/representation/locator tampering. The current Tantivy adapter indexes one document per parent, so the harness coalesces validated Unit text into that parent document. `qpart` uses a term unique to the second Part; actual Tantivy retrieves parent 0 once. For `q0`, Lexical and typed Graph overlap on parent 0, and `PriorityConcat` emits it once. This verifies this parent-indexed L/LG baseline; actual dense Unit-hit-to-parent folding remains a separate implementation gate.

## F3 — no-positive access stratum

`score_run` checks duplicate parent ranks before checking for positive qrels. It takes a Source-current visible-parent set, counts visible false positives on no-positive queries and unauthorized disclosures separately, and leaves no-positive Recall/MRR/nDCG undefined. `qaccess` matches only denied/unknown synthetic records in Tantivy; the `RetrievalExecutor` trace and final rank list must be empty. `qfalse` matches one visible, nonrelevant parent to prove the false-positive metric is nonzero. The runner rejects a nonzero disclosure count.

## Final paired replay

The same synthetic seed `20260930`, seven queries, gold parents and authorization state were used at all three scales. Four queries have eligible positive parent qrels (`q0`, `q1`, `q2`, `qpart`); three have none. Scores are macro averages across the four positive queries, with all eligible positive parents in each Recall denominator. The same 32 challenge Resources occur at all scales, so unchanged quality scores do not show scale robustness for harder relevance cases.

| Resources | Arm | Recall@1 | Recall@5/10/20 | MRR@10 | nDCG@10 | No-positive visible false positives | Unauthorized disclosures |
| ---: | :---: | ---: | ---: | ---: | ---: | :--- | ---: |
| 32 | L | 0.4375 | 0.6250 | 1.0000 | 0.7497 | `qnone=0, qaccess=0, qfalse=1` | 0 |
| 32 | LG | 0.4375 | 1.0000 | 1.0000 | 0.9706 | `qnone=0, qaccess=0, qfalse=1` | 0 |
| 256 | L | 0.4375 | 0.6250 | 1.0000 | 0.7497 | same | 0 |
| 256 | LG | 0.4375 | 1.0000 | 1.0000 | 0.9706 | same | 0 |
| 1024 | L | 0.4375 | 0.6250 | 1.0000 | 0.7497 | same | 0 |
| 1024 | LG | 0.4375 | 1.0000 | 1.0000 | 0.9706 | same | 0 |

All no-positive relevance ratios are undefined. The 32-resource trace shows `qpart` returns `[0]`, `q0` L returns `[0,1]`, and `q0` LG returns `[0,1,2,3]`; parent 0 appears in both retrievers but once after S1. The test-only preauthorization Tantivy probe finds exactly six denied/unknown `qaccess` records, while both arms' post-Read stage traces and final ranks are empty for that query. A separate metric fixture proves that a hypothetical leaked rank increments unauthorized disclosure without being conflated with a visible false positive.

Durations are monotonic `Instant` samples in milliseconds. Each arm has seven query and seven fusion samples per scale, so nearest-rank p95/p99 are maxima. The first LG query follows L on a shared index; it is first-in-arm, not independently cold. Build and next-generation update are one sample per scale shared by L/LG. Update rebuilds the whole immutable generation after one Source field change, not incremental indexing. RSS is the process's `ps -o rss= -p PID` sample after each arm; both indexes are RAM-only, so file-backed index bytes are 0.

| Resources | Arm | Query p50/p95 ms | Fusion p50/p95 ms | RSS KiB |
| ---: | :---: | :--- | :--- | ---: |
| 32 | L | 6.424 / 8.232 | 0.150 / 0.275 | 22,928 |
| 32 | LG | 7.153 / 8.449 | 0.168 / 0.404 | 23,056 |
| 256 | L | 6.312 / 8.137 | 0.142 / 0.251 | 26,048 |
| 256 | LG | 6.437 / 7.192 | 0.123 / 0.342 | 26,064 |
| 1024 | L | 6.918 / 7.792 | 0.142 / 0.206 | 32,816 |
| 1024 | LG | 6.571 / 7.139 | 0.142 / 0.338 | 32,832 |

| Resources | Initial Lexical/Graph build ms | Next-generation Lexical/Graph rebuild ms |
| ---: | :--- | :--- |
| 32 | 27.394 / 0.287 | 29.046 / 0.142 |
| 256 | 43.244 / 0.629 | 45.562 / 0.514 |
| 1024 | 128.551 / 2.184 | 120.643 / 2.041 |

## Verification and limits

The F1 snapshot test was first RED because the helper did not exist, then 1/1 GREEN. F2/F3 focused tests were first RED against the old corpus/metric interfaces, then the final pinned run passed 13/13 Rust tests (including the actual Tantivy restricted-query probe) and PoC strict Clippy. `rustfmt` completed before the pinned run. The run used debug info 0, incremental compilation off, two Cargo jobs, the existing root `target/`, and no model download. The stored first baseline remains a separate earlier receipt.

This is a synthetic parent-indexed L/LG baseline. It does not qualify production P1 body parsers, native file round trips, real Source ownership wiring, Unit-hit Vector ranking, incremental index equivalence, engine durability, or any model. Only the defined F1–F3 PoC blockers were repaired; independent review is still needed before promoting this baseline as the dense comparison reference. Dense/Vector arms remain **UNRUN**.
