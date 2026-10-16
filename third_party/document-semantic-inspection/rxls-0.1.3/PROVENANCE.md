# `rxls 0.1.3` zip 9 patch

- Source: published crates.io `rxls 0.1.3` archive, SHA-256 `b17c6efadd55c170da83de483e84bdaa0728563996baa5b9967a3b67f43733ce` (matches the root Cargo.lock checksum).
- Upstream: <https://github.com/HyunjoJung/rxls>; archive `.cargo_vcs_info.json` records commit `e1390e5aa349fbf933c39bccda400a4a2ee1d814` (tag `v0.1.3`).
- License: MIT; upstream `LICENSE` and `THIRD_PARTY_LICENSES.md` are retained here.
- Use: the typed XLSX/XLSM workbook reader in Document Semantic Inspection v0 and Document Diff v0. The version and semantic projection remain the qualified composition.
- Change: `ZIP9.diff` is the complete diff against the archive. `Cargo.toml` `[dependencies.zip]` changes `version = "2.2"` to `version = "=9.0.3"`. Source changes are limited to the zip 9 API:
  - `ZipFile::name()` now returns `Result`. An undecodable entry name fails with the existing `Error::Zip` package errors, is treated as a non-matching part, or ends the CLI with the existing exit code 65.
  - The entry-name length limit reads `name_raw()`, so it still bounds the stored bytes.
  - zip 9 adds `typed-path`, whose `AsRef` impls make four `.as_ref()` calls on quick-xml byte events ambiguous; they deref explicitly (`&*text`).
  - `Cargo.toml.orig` remains unmodified for comparison.
- Reason: zip 2.4.2 is yanked and the root `cargo deny` denies yanked crates. The latest rxls (0.1.4) still requires zip `^2.2`.
- Packaging: retain the archive contents. Omit upstream automation (`.github`, `.gitignore`, `.gitattributes`, `scripts`, `deny.toml`) and the package-local Cargo.lock.
- Local verification (2026-10-16, linux/arm64 VM, not hosted CI): the same change on the upstream `v0.1.3` checkout passed `cargo test --no-fail-fast`, 929 tests.
- Removal condition: replace this patch with a published rxls release whose zip requirement accepts a non-yanked zip, after repeating the DSI and Document Diff spreadsheet qualification for that version.
