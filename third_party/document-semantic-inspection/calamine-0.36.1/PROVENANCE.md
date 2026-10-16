# `calamine 0.36.1` zip 9 patch

- Source: published crates.io `calamine 0.36.1` archive, SHA-256 `5fa68281b1a76b54a62156474adb06bb380a67e07dd60656e3217152b42183f3` (matches the root Cargo.lock checksum).
- Upstream: <https://github.com/tafia/calamine>; archive `.cargo_vcs_info.json` records commit `0a24c2a9f1e38c0932c1299e633270dc730db505` (tag `v0.36.1`).
- License: MIT; upstream `LICENSE-MIT.md` is retained here.
- Use: the independent SpreadsheetML oracle in Document Semantic Inspection v0 (`crates/document-semantic-inspection-worker`). The version and semantic projection remain the qualified composition.
- Change: `ZIP9.diff` is the complete diff against the archive. `Cargo.toml` `[dependencies.zip]` changes `version = "8.6"` to `version = "=9.0.3"`. Three picture readers take the entry name with `zfile.name()?.into_owned()`, and the path cache skips names zip 9 cannot decode. `Cargo.toml.orig` remains unmodified for comparison.
- Reason: zip 8.6.0 is yanked and the root `cargo deny` denies yanked crates. calamine 0.36.1 is the latest release and still requires zip `^8.6`. zip 9 returns `Result` from `ZipFile::name()` and `file_names()`; nothing else in calamine changes.
- Packaging: retain the archive contents. Omit upstream automation (`.github`, `.gitignore`, `.typos.toml`) and the package-local Cargo.lock.
- Local verification (2026-10-16, linux/arm64 VM, not hosted CI): the same change on the upstream `v0.36.1` checkout passed `cargo test --all-features`, 272 tests (161 + 44 + 67).
- Removal condition: replace this patch with a published calamine release whose zip requirement accepts a non-yanked zip, after repeating the DSI SpreadsheetML qualification for that version.
