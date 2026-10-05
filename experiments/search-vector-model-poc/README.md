# P2 real-model PoC

**2026-10-05:** both pinned models ran on Candle CPU with reference parity, and the paired synthetic-lane measurement decided `DISABLED`; see [report.md](report.md). The isolated Rust workspace is `Cargo.toml`/`src/`/`tests/embedding_parity.rs`; fetch the pinned assets into the ignored `assets/`, regenerate the oracle with `scripts/reference_vectors.py`, then run `cargo test --release --test embedding_parity` and `cargo run --release`. The protocol-only notes below still describe the unrun `ort`, public-lane and RunPin parts.

`assets-manifest.json` pins two actual 384-dimensional model repositories, their required file bytes, exact revisions and two CPU runtime candidates. `metadata/` holds only the small, immutable configuration files from those revisions. `public-ja-slice.json` pins six MIRACL Japanese query IDs and all their published judgments; corpus passages and model weights are absent. `run-manifest.json` is `PROTOCOL_ONLY_UNRUN`; it contains no admitted current capacity, actual RunPin or measured model arm.

Run the offline preflight:

```sh
python3 -B -m unittest discover -s experiments/search-vector-model-poc/tests -p test_asset_contract.py -v
```

`asset_contract.verify_assets(..., full=False)` verifies the local metadata bytes. `full=True` intentionally fails until a later P2-02 worker has acquired and hash-checked the selected model assets and, for `ort`, the native arm64 archive and extracted shared library. `model_id()` returns a **provisional preflight ID**; the production `EmbeddingModelId` must also bind the actual built code and native binary hash. `freeze_run()` intentionally rejects this protocol-only run until the isolated Rust scaffold, both Cargo locks, full selected assets, actual data artifacts and fresh capacity admission exist. Its `public_ja` lane additionally requires selected raw row bytes, complete Source/Version/Part/Unit/locator mapping and the pinned topic/qrel source files; the untouched holdout remains closed.

For a future actual run, use `capture_source_pins(workspace_root)` to derive the seven code categories from the isolated and baseline Cargo manifests, every reachable local path/workspace dependency and patch tree, full crate trees, locks, build scripts, local includes and Cargo configuration. `freeze_run()` and `verify_frozen_run()` recalculate this closure. `build_identity` additionally pins the compiler/Cargo executables, Rust target, selected package features, build environment, native library and each arm executable. Output receipts belong outside those frozen source trees.

The `data_artifacts` JSON files are byte-pinned and schema-checked for corpus rows, query rows, judgments, split, Source snapshot, current Read, filter, eligible parent qrels, Unit bindings and planner output. The validator recalculates eligible parent grades from the actual rows and judgments. Unknown judgments remain unknown. The synthetic lane also checks the actual baseline corpus/query/qrel files; the public lane checks selected JSONL row bytes and the source topic/qrel files. Caller-supplied digest strings alone cannot open either lane.

Call `capture_capacity_receipt()` at freeze and before each acquisition or build; `verify_capacity_before_action()` checks the same selected immutable asset/build/index projection against fresh `statvfs` disk and available RAM. Before each arm, `begin_arm()` calls `verify_frozen_run()` and writes an exclusive preflight artifact. Post-run validators check nonfuture times, the pinned query/sample count, every scored Unit/parent/route/planner binding, the executable SHA, a matching trace, and recalculated numeric metrics. These receipts are structural evidence and do not independently prove that inference executed.

No Cargo workspace or runtime scaffold is created in P2-01. Under `p2-protocol-scaffold-ruling.md`, the P2-02 worker must add and qualify that scaffold before any actual RunPin can be sealed. Keep model bytes and temporary build output under the ignored `assets/` and `target/` paths. The historical L/LG input and report in `../search-vector-poc/` are read-only.

See `protocol-receipt.md` for source links, numerical and ranking tolerances, acquisition limits, measurement gates and unresolved checks.
