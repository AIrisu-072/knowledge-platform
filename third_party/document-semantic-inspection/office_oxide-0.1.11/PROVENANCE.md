# `office_oxide 0.1.11` zip 9 patch

- Source: published crates.io `office_oxide 0.1.11` archive, SHA-256 `25caa1da7142070699529cfc8f276a739101c22ffccc572644d3c59278f19b56` (matches the root Cargo.lock checksum).
- Upstream: <https://github.com/yfedoseev/office_oxide>; archive `.cargo_vcs_info.json` records commit `faae2d4460aa01332628fe7580a970889861f59c` (tag `v0.1.11`).
- License: MIT OR Apache-2.0; upstream `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` and `TRADEMARKS.md` are retained here.
- Use: the typed DOCX/PPTX parser in Document Semantic Inspection v0 and Document Diff v0. The version and semantic projection remain the qualified composition.
- Change: `ZIP9.diff` is the complete diff against the archive. `Cargo.toml` `[dependencies.zip]` changes `version = "8.6"` to `version = "=9.0.3"`. Source changes are limited to the zip 9 API: `ZipFile::name()` and `file_names()` now return `Result`, and the OPC part lookup and XLSX chart/font listings skip an entry whose name zip 9 cannot decode, as an absent part. Test-only call sites unwrap the name. `Cargo.toml.orig` remains unmodified for comparison.
- Reason: zip 8.6.0 is yanked and the root `cargo deny` denies yanked crates. office_oxide 0.1.15 requires zip `^9`, but it changes PPTX parsing (structured speaker notes, OLE objects, picture bullets), which would need a new PPTX qualification. This patch keeps the qualified 0.1.11 behaviour.
- Packaging: retain the archive contents. Omit `cbindgen.toml` and `include/` (C FFI packaging, unused by the Rust build) and the package-local Cargo.lock.
- Local verification (2026-10-16, linux/arm64 VM, not hosted CI): on the upstream `v0.1.11` checkout, `cargo test --no-fail-fast` passed 880 tests. The per-target results were identical before and after the change.
- Removal condition: move to a published office_oxide release on a non-yanked zip after repeating the DSI DOCX/PPTX and Document Diff qualification for that version.
