# P2 canonical executed-input export — current-source bounded receipt

- Observed: 2026-10-01 UTC, Linux x86_64, Rust 1.98.1. Local Search worktree on `feat/search-platform-completion-core`; new PoC and shared code are uncommitted. This is source-snapshot evidence, not exact PR-head CI or production Vector qualification.
- Verdict: **bounded executed-input proof PASS, independent review pending**. The Rust baseline binary exports its concrete synthetic Corpus, Source/Read/filter/judgment/Unit binding, and L/LG planner view. The model protocol verifies all pinned synthetic data artifacts and planner output against that executable. No model inference, dense arm, RunPin, or backend-selection claim follows.

## Current-source bracket

`p2-executed-input-source-inventory-20261001.json` records per-file SHA-256 for every reachable local path package and root inherited manifest/toolchain. Its aggregate `input_sha256` was `b1bf2944f6fd243a841851fd6f6e1e87ac6596fd30909e8361e731d049a3632f` both before and after the following three phases. `p2-executed-input-source-bracket-20261001.json` records all exit codes as 0; `p2-executed-input-source-bracket-20261001.log` has the command output (SHA-256 `0b45cb4a9cc64816591b816692d55c51630cc6765d86aadfaa2b02194a05c741`). The inventory file itself has SHA-256 `585ce49e248478358c72172a2cae355f33cef24a5a94a6ae0a212aea4cc9a516`.

1. `cargo test --manifest-path experiments/search-vector-poc/Cargo.toml --offline --locked --tests`: 14/14 Rust tests PASS, including CLI/in-process exporter equivalence, multi-Part Unit and denied/Unknown regressions.
2. `cargo run --manifest-path experiments/search-vector-poc/Cargo.toml --offline --locked --quiet`: current L/LG binary executed all seven queries at each 32/256/1024-resource scale. L Recall@5 was 0.6250 and LG Recall@5 was 1.0000 at each scale; every unauthorized-disclosure count was 0. The repeated quality scores reflect the shared 32 challenge resources, not difficulty scaling.
3. `cargo clippy --manifest-path experiments/search-vector-poc/Cargo.toml --offline --locked -p search-vector-poc --tests -- -D warnings`: PASS.

The actual executable SHA-256 was `b68b5ffed65e87ff30b7292330d434636d52a2900cc12032f606c4f9dc73bb6b`. `--export-synthetic-input SCALE 20` output SHA-256 values were: 32 `dc3f7895b8ceb023d7c986a95a70496a003ec40515f133ad66accdab8101e034`, 256 `28cc8d19113f291e17566adbebcc554796b88f0e177a7deadbe7f4d51a3e066b`, 1024 `42939eab2b59f1123548cd086ca695bbd5136b91def7df73d88a75d8fb469738`. These outputs are reproducible from the pinned source snapshot and binary, rather than checked-in multi-megabyte fixtures.

With `P2_BASELINE_BIN` set to that executable, `python3 -B -m unittest discover -s experiments/search-vector-model-poc/tests -p 'test_*.py' -v` passed **31/31**, including the previously skipped real-binary test. It copied the executable into the protocol workspace, pinned its bytes as both L and LG and exporter identity, compared all nine artifacts and planner output, then demonstrated that repinned altered Corpus text/Part binding is still rejected. `p2-executed-input-python-tests-20261001.log` records the output (SHA-256 `63baaa2a79ad14d48c4400a54d46c4120c7de9e468a54f0910c30522d1bd5bcd`). `rustfmt --edition 2024 --check` on `src/main.rs`, `src/run.rs`, and `tests/executed_input_export.rs` and `git diff --check` also exited 0.

## Boundaries and next gate

The old accepted macOS L/LG receipt is historical and does not qualify this current source snapshot. This fresh Linux run closes the canonical executed-input exporter comparison only. Independent P2 review must check the exporter/verification seam and this receipt before promotion. Full weights and ONNX assets remain absent, so real two-model Rust parity, D/LD/LDG, exact-vs-ANN, and selection/production adapter gates remain unrun. The protocol metadata currently pins macOS arm64 ORT assets; Linux/x86_64 is not silently substituted as a model/runtime qualification target.
