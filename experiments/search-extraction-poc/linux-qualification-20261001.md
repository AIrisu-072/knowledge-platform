# P1-Q01 cloud Linux reader requalification (bounded PoC)

**Review status, 2026-10-01:** independent review reproduced a missing hidden worksheet being reported as a located `Partial` omission and issued NO-GO on the preceding binary. A named three-case integrity RED is captured in `/tmp/p1-linux-admission-review-red2.log`. The current rebuilt binary now verifies the referenced part exists, has the declared worksheet content type, and parses as a worksheet with `sheetData` before minting the hidden-sheet omission; missing/malformed bytes become `FailedPermanent(CorruptDocument)` and wrong declared type becomes `Unsupported(UnsupportedStructure)`, all Unit zero. The full suite and all-45 corpus were freshly rerun below. Independent recheck of this correction remains pending, so production admission is still NO-GO.

**Verdict:** the restored 45-case synthetic corpus and the named Linux admission probes qualify the current in-process PoC as a **bounded reader candidate**. They do **not** admit production readers or close P1. No Search reader was added to production Cargo, and `search-extraction-worker` still contains bootstrap only.

## Exact input and execution

- Environment: cloud Linux x86_64, Rust/Cargo 1.98.1, isolated PoC `[workspace]` / `Cargo.lock` SHA-256 `05c98ded3092d7a599977b1bfe76c122f95fa4438984117f03f251d636fee7d6`.
- PDFium: existing repository-pinned 7881 Linux `libpdfium.so`, SHA-256 `f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64`. The previous macOS pin remains separate. The binary checked the Linux library bytes before PDF and ZIP extraction.
- Parser source bundle (`src/main.rs`, `src/readers.rs`, PoC `Cargo.toml`, `Cargo.lock`, same name+byte hash algorithm as `main.rs`): `9011680817f4b5fb26f54ef982abc6d9b5e975347ad94b2db91aedd78c6b3888`.
- Freshly built Linux binary SHA-256 `9af6b5e7dc79f0bb66f1d9c8a385711945387c0045101202a55c8956b8d8801e`. This byte hash and the build command, rather than the runtime `parser_build_sha256` field alone, identify the executed binary.
- Regenerated `manifest.json` SHA-256 `d3baeed8aca85d314255f488b156f441a1895062e630a13318fea9511e3ae47a`. Its 45 raw fixture files were byte-for-byte identical to the restored reference. Package/hidden-sheet omission expectations changed; the PDF ambiguous-order and HTML dynamic-visibility expectations became Unit-zero `Unsupported`.
- `qualification-results-linux-20261001.jsonl` SHA-256 `70d08b5c2cbbe8df05c205c3b5408c2080aca70f020e48f486a7783a045c1362`.

The isolated locked PoC binary was rebuilt from the current source with this environment and command (repository root as working directory):

```sh
source /workspace/shared/search-toolchain/env.sh
export PDFIUM_DYNAMIC_LIB_PATH="$(cat /workspace/shared/search-toolchain/pdfium-path.log)"
CARGO_TARGET_DIR="$PWD/experiments/search-extraction-poc/target" \
  cargo build --manifest-path experiments/search-extraction-poc/Cargo.toml --locked --bin extraction-qualify
export P1_QUALIFIER_BIN="$PWD/experiments/search-extraction-poc/target/debug/extraction-qualify"
python3 -B -m unittest discover -s experiments/search-extraction-poc/tests -v
```

The manifest was checked with `python3 -B experiments/search-extraction-poc/run.py --verify-manifest`. The binary received all 45 manifest rows in one process; every emitted row was compared with the independent raw-byte oracle expectation, including units, locators, coverage, reasons, and omissions. `P1_QUALIFIER_BIN` alone is insufficient for PDF/ZIP: without the exact `PDFIUM_DYNAMIC_LIB_PATH`, those rows correctly fail closed. This is an in-process PoC invocation, not the mandatory Search production sandbox.

## RED → GREEN

The original freshly built Linux source failed 7 assertions in the first 10-test suite: native-pin mismatch for PDF and ZIP; related XLSX pivot package falsely `Supported`; and DOCX/PPTX/XLSM `Partial` missing physically located package omissions. A second test-first pass failed three assertions: hidden worksheet omission lacked its package path, and unreferenced worksheet/slide packages falsely remained `Supported`. Subsequent named RED cases found a hard-coded false `cargo-deny:pass`, PDF ambiguous reading order and HTML dynamic visibility incorrectly `Partial` without physical omissions, and an unlocated PPTX shape incorrectly borrowing an unrelated notes omission. RED logs are `/tmp/p1-linux-admission-red.log`, `red2.log`, `red3.log`, `red4.log`, and `red5.log` in this execution workspace.

PoC-only corrections: choose the pinned PDFium digest by OS; reject unknown/unreferenced OOXML package parts instead of silently calling them `Supported`; enumerate known package-level omissions and hidden-sheet path; validate referenced hidden sheet bytes/structure/content type before the omission; propagate omissions through ZIP member chains. Unlocated shape gaps and PDF/HTML uncertainty without locatable unread scope now fail closed as Unit-zero `Unsupported` rather than `Partial`. The binary says `license_security: not-executed`; the actual dependency scan has its own external receipt. The independent Python raw oracle and manifest expectations were updated for those coverage decisions and the hidden-sheet integrity probe. The formula cache policy remained unchanged: formula cells still never mint positive units from unverified cache values.

Final commands and observed result:

- Python admission + manifest suite with `P1_QUALIFIER_BIN` set to the fresh Linux binary: **15/15 PASS**, zero skips (`/tmp/p1-linux-admission-green-review-final.log`). This includes independent mutation of the hidden sheet to absent, malformed, and wrong-MIME states.
- Direct all-format manifest qualification: **45/45 qualified**; `Supported 10`, `Partial 11`, `Unsupported 21`, `FailedPermanent 3`; missed/unexpected/locator failures zero. Every `Partial` row has at least one raw-located known omission. Highest reported row wall 251 ms, process peak RSS 33,562,624 bytes, structured result 3,803 bytes. These are small synthetic fixtures in one process, not per-format capacity or p95/p99 claims.
- `cargo fmt --manifest-path experiments/search-extraction-poc/Cargo.toml -- --check`: PASS.
- `cargo clippy --manifest-path experiments/search-extraction-poc/Cargo.toml --all-targets --locked --offline -- -D warnings`: PASS.
- `cargo test --manifest-path experiments/search-extraction-poc/Cargo.toml --locked --offline`: PASS, 0 Rust tests; the 11 actual behavior tests are Python invoking the compiled binary.
- Scoped changed-file trailing-whitespace check: PASS; all 46 fixture raw files match the restored reference byte-for-byte. The whole PoC directory is untracked in this checkout, so a plain `git diff --check` alone is not a meaningful review of it.
- `cargo deny --manifest-path experiments/search-extraction-poc/Cargo.toml --config experiments/search-extraction-poc/deny.toml check` under installed cargo-deny **0.20.2**: exit 0, `advisories ok, bans ok, licenses ok, sources ok` (`/tmp/p1-linux-cargo-deny-20261001.log`). Non-blocking warnings: `syn` 2/3 duplicate and two unmatched license allow entries. This is a separate current external scan; the qualifier binary does not self-attest it.

## Admission boundary and next gate

The 45 fixed inputs and named admission probes are much narrower than general DOCX/XLSX/XLSM/PPTX/PDF/Text/CSV/HTML/ZIP. In particular, unknown visible structures, all `Partial` omission variants, full archive reader-use/profile pinning, 1/10/50 MiB and upper-limit stress, CPU/AS/scratch/output enforcement, and formatwise native-library/security qualification are unqualified. Search-specific fresh-process Landlock/seccomp, admission/worker/host three-layer budget and locator validation, Source parent/Version/raw/current-Read binding, DB/FS vertical proof, independent read-only review, and hosted exact-head gates are still missing. Production dependency promotion and `Supported` claims outside the bounded synthetic subset remain **NO-GO**.

The independent read-only recheck of this repaired PoC is bounded GO. Next exact action: execute the [production admission checklist](production-admission-checklist.md), beginning with valid Office relationship topology and its missing/wrong-part negative controls, then formatwise adversarial corpus and resource/profile measurements. Only a freshly reviewed bounded profile may be considered for production reader implementation under the frozen P1-I03–I05 order. Do not treat this receipt as a sandbox or P1 completion receipt.
