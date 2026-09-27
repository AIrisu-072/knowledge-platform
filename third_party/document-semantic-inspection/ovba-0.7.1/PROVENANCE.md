# `ovba 0.7.1` production safety patch

- Source: crates.io `ovba 0.7.1` archive, SHA-256 `c44e6f22d4d563c2ef2225c55f49f79a48c6606303659b39fbc1892a3e5bfc42`.
- Upstream repository: `tim-weis/ovba`; upstream license: `LICENSE` in this directory.
- Use: static extraction of VBA source and project metadata in Document Semantic Inspection v0. VBA is never executed.
- Cargo patch: the workspace redirects only `ovba 0.7.1` to this local copy. The version and semantic projection remain the PoC-qualified composition; the local patch adds a bounded module-source API.

The unmodified crate can decompress VBA module source above the approved 16 MiB budget before the caller can inspect its length. A local release probe against the qualified `calamine-vba.xlsm` seed observed 89 source bytes; a crafted compressed variant returned 16,781,378 source bytes. The unmodified release build also accepted trailing decoded `/VBA/dir` bytes. These are local RED observations, not hosted CI evidence.

The patch is limited to bounded CFB reads, bounded MS-OVBA decompression and module source decoding, the approved 1,024 module limit, strict decoded directory consumption, and fallible code-page handling. The 64 MiB decoded-directory ceiling is derived from the approved OOXML per-entry byte ceiling; it is an internal parser allocation bound, not a new semantic normalization rule. Resource-limit failures map to the worker's `InspectionResourceLimitExceeded`; malformed project failures remain generic and body-free.

On the current uncommitted tree, the focused worker resource regression passed **2/2 in debug and 2/2 in release** after a debug-only parser assertion was replaced with a controlled error. An isolated copy of this patched crate, locked to `encoding_rs 0.8.41`, passed its **9/9 unit tests**. The decompression test accepts exactly three output bytes and rejects the fourth with a two-byte ceiling; a separate assertion binds the decoded-directory constant to the approved 64 MiB per-entry ceiling. The worker's XLSM semantic suite passed **6/6**. These are local results and must be rerun after any source change.

Production GREEN qualification is pending the focused resource-boundary tests, workspace checks, `cargo deny`, and exact-head standard CI, DSI Sandbox Preflight, and DSI PoC regression. Record the final head and run IDs in the execution status before declaring Task 6 complete.
