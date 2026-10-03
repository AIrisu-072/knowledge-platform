# Search Vector PoC — weight-free baseline harness

This isolated `[workspace]` evaluates the existing Search Lexical (L) and Lexical+Graph (LG) paths. It has no model weights, Vector runtime, network data, customer data, or production dependency changes. The `Cargo.toml` points to the existing `search-*` crates and the same reviewed Tantivy 0.26.2 patch as the repository root.

## Stable interfaces

- `Corpus::synthetic(scale, 20260930)` creates exactly 32, 256, or 1024 bounded synthetic Source Resources. `corpus_manifest.json`, `queries.jsonl`, and `qrels.jsonl` define the seed, seven queries, and parent Resource grades. Three positive queries have four eligible parents each, including Graph-only parents; `qpart` finds the second Part of one parent. `qnone`, `qaccess`, and `qfalse` have no eligible positives.
- `validate_units()` and `Corpus::validate()` check the frozen nine-field `UnitId` frame, both published golden vectors, Text locator codec, normalized text/raw digests, Source-owned Version/Part/Resource binding, and pinned generation. Resource 0 has two different Source-owned Parts in one Version, with different raw bytes, representation refs, locator bytes, and Unit IDs. Each proposed Unit is also converted to the actual `search-core::knowledge_unit::KnowledgeUnit` and passed through `validate_part_units` against the separate synthetic Source Part binding. This does not exercise a production P1 parser or native format round trip.
- `Harness::build()` validates each Part, folds their text into one parent Lexical document (the current Tantivy adapter rejects duplicate parent documents), and builds `TantivyLexicalIndex` and `MemoryGraphRetriever` from that Source snapshot. `run_arm(harness, arm, pinned_generation, actor, window)` uses `SourceRouter`, `RetrieverPlanner`, `RetrievalExecutor`, and `CandidateFederator::merge(PriorityConcat)`. Its `QueryRun` keeps one-based stage order, post-Read raw parent ranks, eligible gold misses per stage, and final deduplicated parent ranks. Source Part and current Read/Version are checked again before fusion and publication. Raw scores are never added. The eventual dense Unit-hit-to-parent adapter remains a separate qualification.
- `score_run()` validates deduplicated parent ranks before relevance scoring, counts visible false positives for no-positive queries separately from unauthorized disclosures, and computes macro Recall@1/5/10/20, MRR@10, and graded nDCG@10 over currently eligible positive parent qrels. No-positive relevance ratios remain undefined. `qaccess` matches only denied/unknown Source records before `RetrievalExecutor`, while `qfalse` has one visible false positive.
- `measure_run()` summarizes monotonic `Instant` query/build/next-generation update/fusion samples. RSS comes from `ps -o rss= -p PID` on this macOS host. Both indexes are in-memory, so file-backed index bytes are zero; the Rust build `target/` is a separate artifact.
- `validate_mock_vector_hit()` is only a contract-failure probe for shape, finite values, generation/Unit/parent binding, scope/lease/retention, and current Read/Version. It produces no embeddings or quality scores.

## Reproduce

From the repository root, use the pinned runner for the comparison receipt:

```sh
python3 -m unittest discover -s experiments/search-vector-poc/scripts -p 'test_*.py' -v
python3 experiments/search-vector-poc/scripts/pinned_baseline.py
```

The runner selects the isolated lock and actual Cargo path dependency graph, hashes every local package tree including the patched Tantivy source before and after focused tests, the L/LG run, and focused Clippy, and rejects a changed input. It reuses the repository `target/`, with debug info and incremental compilation disabled and two jobs. The new output and exact input tree digests are in `baseline-refinement-run.log`, `baseline-refinement-run-manifest.json`, and `baseline-refinement-report.md`. The original `report.md` and `baseline-run.log` are retained as earlier receipts. Dense/Vector model arms are explicitly **UNRUN**.
