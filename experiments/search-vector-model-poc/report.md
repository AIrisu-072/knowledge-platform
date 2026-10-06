# P2 real-model measurement and Vector decision — 2026-10-05

**Decision: `DISABLED`.** Vector stays an optional retriever behind `VectorActivationPolicy::Disabled` (the default). P2-07 (production adapter) is `SKIPPED_DISABLED`. The provider-neutral core (P2-05) and application contract (P2-06) are complete.

## What ran

| Stage | Result |
| --- | --- |
| `SOURCE_INSPECTED` / `BASELINE_FROZEN` | The frozen synthetic L/LG harness (`../search-vector-poc`) on the current head, with three mechanical compile updates for fields the shared crates gained since (`remote: None`, `body_query: None`, `..RetrievalInputs::default()`) and a lock refresh. Its 14 tests pass. |
| Assets | Both pinned models fetched at their exact revisions; all four files (`model.safetensors`, `tokenizer.json` per model) match `assets-manifest.json` bytes and SHA-256. Bytes stay under the ignored `assets/`. |
| `RUST_PARITY_PASS` (Candle CPU) | `tests/embedding_parity.rs`: both models against the test-only `transformers` oracle (`scripts/reference_vectors.py`, torch 2.14.1 / transformers 5.18.0, fixture `fixtures/reference-vectors.json`). Exact token IDs including the 512 (E5) and 128 (MiniLM) truncation boundary; raw pool and normalized vector within component error 0.002 and cosine error 0.0001; L2 norm within [0.9999, 1.0001]; padded batch equals unpadded; every reference order with margin > 0.0002 kept. Candle `bert::BertModel` loads both checkpoints directly. |
| `MODEL_QUALITY_MEASURED` (synthetic lane) | `src/main.rs`, log `measurement-run.log`: L, LG (the baseline crate's own `run_arm`), D (exact cosine over every Unit), LD and LDG (stage lists concatenated in the actual planner S1 order, verified as Lexical → Vector → HyperGraph) on one Source pin, actor, window 20 and scorer, at 32/256/1024 Resources. |
| `ENGINE_PASS` | Not run: with no adoption, an ANN engine has nothing to serve. Exact scan was the dense engine here. |
| `NEUTRAL_CONTRACT_PASS` | P2-05 `search-core` `vector_contract`; P2-06 `search-application` `vector_contract` (7) and `vector_lifecycle_contract` (7). |

Host: Apple M5, macOS 26.5.2, 16 GiB, Rust 1.98.1, release build, CPU only. Timings are n=7 per arm/scale and descriptive only.

## Decision sheet (fixed before the run)

Adopt only if LD or LDG gains at least +0.05 nDCG@10 or Recall@10 over **both** L and LG at all three scales, with no more visible false positives on the no-positive queries (`qnone`, `qaccess`, `qfalse`) and zero unauthorized disclosure. The synthetic ranks were viewed earlier (development diagnostics, not a holdout), so even a pass here would also need the untouched public lane before adoption.

## Results (window 20; macro over the four positive queries)

| Model | Scale | L nDCG@10 / R@10 | LG | D | LD | LDG | visible FP on no-positive queries (L/LG → D/LD/LDG) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E5-small | 32 | 0.750 / 0.625 | 0.971 / 1.000 | 0.779 / 0.938 | 0.862 / 0.938 | 0.862 / 0.938 | 0–1 → 11–12 |
| E5-small | 256 | 0.750 / 0.625 | 0.971 / 1.000 | 0.705 / 0.688 | 0.788 / 0.688 | 0.788 / 0.688 | 0–1 → 15–17 |
| E5-small | 1024 | 0.750 / 0.625 | 0.971 / 1.000 | 0.667 / 0.625 | 0.750 / 0.625 | 0.750 / 0.625 | 0–1 → 17–18 |
| MiniLM-L12 | 32 | 0.750 / 0.625 | 0.971 / 1.000 | 0.828 / 0.875 | 0.855 / 0.875 | 0.905 / 1.000 | 0–1 → 8–9 |
| MiniLM-L12 | 256 | 0.750 / 0.625 | 0.971 / 1.000 | 0.828 / 0.875 | 0.855 / 0.875 | 0.905 / 1.000 | 0–1 → 12–15 |
| MiniLM-L12 | 1024 | 0.750 / 0.625 | 0.971 / 1.000 | 0.735 / 0.875 | 0.855 / 0.875 | 0.905 / 1.000 | 0–1 → 14–18 |

Unauthorized disclosures: 0 in every arm, scale and model. Dense cost: model load 0.20–0.23 s and ~0.86–0.91 GiB resident; corpus embedding 0.19–0.21 s (33 Units) to 4.6–5.4 s (1,025 Units); query embed + exact scan p50 10–12 ms.

## Why `DISABLED`

1. **No gain over LG.** LDG is below LG at every scale for both models (nDCG@10 0.75–0.91 vs 0.97). In S1 the Vector stage precedes HyperGraph, so dense neighbors push the Graph-only gold parents below rank 10. LD beats L only by recovering what LG already recovers.
2. **Visible false positives.** Dense retrieval always returns nearest neighbors. The no-positive queries go from 0–1 visible parents to 8–18; the current Read gate keeps them authorized, but they are irrelevant results shown to the actor.
3. **Cost.** About 0.9 GiB resident per model and seconds of embedding per thousand Units, for no measured benefit.

The synthetic corpus is lexical by construction (its Graph-only golds share no text with their queries), so this does not measure semantic recall on natural text. That needs the public lane below; until it shows a repeatable gain, the decision rule keeps Vector disabled.

## Not run (recorded, not inferred)

- `ort` / ONNX Runtime parity (the second CPU runtime) and CoreML.
- The MIRACL Japanese public lane: corpus passages were not fetched; the pinned topic/qrel IDs in `public-ja-slice.json` remain unused.
- Any ANN engine (`hnsw_rs`, pgvector, Qdrant), capacity distractors beyond 1,024 Resources, and the strict `RunPin`/capacity receipts of `asset_contract.py`.

Re-opening the decision needs the public holdout first, then ANN only if a model is adopted.

The Python protocol suite (`tests/test_asset_contract.py`) has two time-bound failures (`deadline exceeded`) whose fixture deadline has passed; they occur without this work and are outside CI.

## E re-measurement (2026-10-06)

The gate was fixed before the run in `docs/superpowers/programs/search-platform-production/plan.md` (section E). Changes measured: the planner's Exploratory S1 order is now Lexical → HyperGraph → Vector, and Vector candidates need cosine ≥ τ. Model: pinned E5-small on Candle CPU. Full output: `measurement-e-run.log`; public lane IDs and digests: `public-ja-lane-manifest.json` (passage text stays outside the repository; `scripts/prepare_public_ja.py` rebuilds it from the hash-pinned MIRACL ja files).

- Calibration (40 MIRACL ja dev queries, never evaluated): τ = 0.890.
- G1 PASS: synthetic nDCG@10 LGD = LG = 0.9706 at 32, 256 and 1,024 (L 0.7497).
- G2 PASS: synthetic visible false positives LGD = LG at every scale.
- G3 PASS: 100 untouched MIRACL ja dev queries, nDCG@10 L 0.0348 (production lexical: default tokenizer, literal phrase) → LD 0.5523, gain +0.5175, paired bootstrap 95% [0.4333, 0.6025]. Sensitivity against character-bigram BM25: 0.2486 → 0.5601, +0.3114 [0.2347, 0.3925].
- G4 PASS: with each query's positives removed, mean FP@10 L 0.43 → LD 1.15 (limit L + 1).
- G5 PASS: no unauthorized disclosure in any synthetic arm.
- G6 PASS: query embedding plus exact scan p95 18.80 ms at 1,024 Units (public lane 24.06 ms at 1,215 passages).

Decision: `DEFAULT_ENABLED`, τ = 0.890. The synthetic lane shows no gain because Vector now follows Graph; the gain is on natural-language queries the current lexical arm cannot segment.

