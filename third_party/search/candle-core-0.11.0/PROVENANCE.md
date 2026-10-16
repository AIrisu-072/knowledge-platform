# `candle-core 0.11.0` zip 9 patch

- Source: published crates.io `candle-core 0.11.0` archive, SHA-256 `5ecb245093b0f791b89d3420c3df9c6d49c60ab63ba54db896bf8a3baf486706` (matches the root Cargo.lock checksum).
- Upstream: <https://github.com/huggingface/candle> (`candle-core/`); archive `.cargo_vcs_info.json` records commit `31f35b147389700ed2a178ee66a91c3cc25cc80d`.
- License: MIT OR Apache-2.0; upstream `LICENSE` is retained here.
- Use: the CPU embedding runtime behind `crates/search-vector-adapter` (selected Candle 0.11.0). The adapter loads safetensors; zip is used only by candle's NumPy `.npz` and PyTorch pickle readers.
- Change: `ZIP9.diff` is the complete diff against the archive. `Cargo.toml` `[dependencies.zip]` changes `version = "8.6.0"` to `version = "=9.0.3"`. In the `.npz` and pickle readers, an entry name zip 9 cannot decode returns the existing zip error instead of being used. `Cargo.toml.orig` remains unmodified for comparison.
- Reason: zip 8.6.0 is yanked and the root `cargo deny` denies yanked crates. candle-core 0.11.0 is the latest release and upstream `main` still requires zip `8.6.0`.
- Packaging: retain the archive contents and omit the package-local Cargo.lock.
- Local verification (2026-10-16, linux/arm64 VM, not hosted CI): `cargo check` of the patched crate passed. The archive tests (including its `.npz` and `.pt` fixtures, inert data that this repository never loads) were not run; the arm64 debug build of candle's `gemm-f16` dependency does not compile on that VM.
- Removal condition: replace this patch with a published candle release on a non-yanked zip, after repeating the vector model protocol checks for that version.
