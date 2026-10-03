# Isolated metadata-only Tauri qualification

This private nested workspace is deliberately outside root Cargo/pnpm members and
has no build.rs, executable, runtime configuration, capability or CI hook. It has
not been compiled. Do not build/run it merely because its lock exists.

Authority: [exact five-version ADR](../../docs/decisions/2026-10-03-organization-tauri-windows-mpl-amendment.md),
[reviewed design](../../docs/superpowers/specs/2026-10-03-organization-tauri-windows-inventory-design.md),
[plan](../../docs/superpowers/plans/2026-10-03-organization-tauri-windows-inventory.md).
The Windows-only scoped deny configuration is conditional on exact role validation,
not a replacement for root policy. Never use it as a Linux or CLI license waiver.

## Data-only reproduction

Use verified official Rust/Cargo1.98.1 and cargo-deny0.20.2; record actual host.
Set CARGO_HOME to one dedicated shared cache and RAW to an external evidence
folder outside the repository. Set P to this Cargo.toml's absolute path and REPO
to the repository root. Check every used filesystem keeps1536MiB free and combined
downloaded/expanded tools/cache/evidence stays below4GiB. Do not install a target,
run package code, change root policy or run an unqualified tool to reproduce data.

1. Keep the committed lock. A fresh re-resolution is a NEW evidence subject, not
   this capture; initial resolution here selected cssparser-macros0.7.1. Only the
   reviewed precise update to0.7.0 changed its checksum and syn dependency edge.
2. Before Cargo may unpack anything, download every locked crates.io archive as
   data to the official cache location. Match each Cargo.lock SHA256 and call
   inventory.inspect_archive(path, checksum, name + '-' + version). It rejects
   path escape, duplicates, symlinks/hardlinks and special members. Reserve expanded
   regular-member sizes within the same4GiB budget before extraction.
3. Run these data-only commands serially, recording exit status and tool versions:

```sh
cargo metadata --locked --offline --format-version 1 --manifest-path "$P" > "$RAW/metadata-all.json"
cargo metadata --locked --offline --format-version 1 --filter-platform x86_64-pc-windows-msvc --manifest-path "$P" > "$RAW/metadata-windows.json"
cargo tree --locked --offline --target x86_64-pc-windows-msvc --manifest-path "$P" --edges normal,build --no-dedupe --charset ascii --format '{p}|{l}|{f}' > "$RAW/tree-windows.txt"
cargo tree --locked --offline --target x86_64-pc-windows-msvc --manifest-path "$P" --edges normal,no-proc-macro --no-dedupe --charset ascii --format '{p}|{l}|{f}' > "$RAW/tree-normal-no-proc.txt"
```

4. Use unfiltered metadata for package/proc-macro lookup. Classify expanded tree
   occurrences using inventory.classify_tree; prove its normal target set equals
   the independently captured no-proc-macro projection. Check every selected ID
   has matching lock/metadata/archive/source. inventory.verify_cache compares all
   extracted regular-file bytes to verified archive members; only Cargo's own
   .cargo-ok marker is separately reported. Record canonical member-map SHA256
   using sorted-key compact JSON, plus counts, license declarations and notice hashes.
5. inventory.check_exception_roles(roles, 'x86_64-pc-windows-msvc') must return no
   violation before using the scoped deny file. It requires exact versions and
   approved host/target uses; another target is rejected. Native Windows-host
   evidence remains separate from this actual Linux-host projection.
6. Fetch only the current official advisory DB as data with cargo-deny fetch db;
   record its immutable Git SHA/time. This capture used f8dee89e1b2f2f1eaf548312df7655fe5202a302.
   Use explicit manifest, unfiltered --metadata-path and --locked --offline for
   each scan/list. Run root deny.toml Windows-filtered and all-target raw checks;
   then the five-exception scoped Windows check. Do not ignore advisories.

```sh
cargo-deny --manifest-path "$P" --metadata-path "$RAW/metadata-all.json" --config "$REPO/deny.toml" --locked --offline --target x86_64-pc-windows-msvc list --format json --layout crate
cargo-deny --manifest-path "$P" --metadata-path "$RAW/metadata-all.json" --config "$REPO/deny.toml" --locked --offline list --format json --layout crate
cargo-deny --format json --manifest-path "$P" --metadata-path "$RAW/metadata-all.json" --config "$REPO/deny.toml" --locked --offline --target x86_64-pc-windows-msvc check --show-stats
cargo-deny --format json --manifest-path "$P" --metadata-path "$RAW/metadata-all.json" --config "$REPO/deny.toml" --locked --offline check --show-stats
cargo-deny --format json --manifest-path "$P" --metadata-path "$RAW/metadata-all.json" --config "$REPO/tools/organization-tauri-qualification/deny.windows-qualification.toml" --locked --offline --target x86_64-pc-windows-msvc check --show-stats
```

7. Compare scanner-covered IDs with all selected occurrence IDs. A filtered pass
   alone is insufficient. Preserve every warning/failure and identify inactive
   all-target errors separately. Original exits4/5/0 and their bounded diagnostics
   are in the [scan receipt](../../docs/research/organization-tauri-windows-inventory/scan-receipt.json).

Run only the standard-library helper suite with:
`PYTHONDONTWRITEBYTECODE=1 python -m unittest discover -s tools/organization-tauri-qualification -p 'test_*.py'`.
The suite executes our parsers on synthetic data, not dependency code.
For the original capture, run verify_capture.py --raw "$RAW" --cargo-home "$CARGO_HOME".
It verifies raw-input hashes, lock/role/features, all archive/member/cache equality,
policy hashes and scanner coverage against the bounded receipts. It performs no
network, extraction, package compilation or execution. After a fresh regenerated
capture has new path-dependent metadata bytes, review its own new provenance;
never overwrite the old receipt just to make the original-capture check pass.

## Evidence and limits

[Bounded inventory](../../docs/research/organization-tauri-windows-inventory/inventory/index.json)
records all406 registry packages, selected/inactive roles, per-role features,
representative paths, archive checksum, source-member map digest, notice hashes and
embedded-artifact classification. [Receipt](../../docs/research/organization-tauri-windows-inventory/verification-receipt.json)
pins raw capture hashes and resource/test provenance. Raw bytes include absolute
capture paths and can differ when reproduced in another directory; compare locked
package identities, roles, features and archive/source digests as well as provenance.

Dependency archives, extracted source, SDK binaries and raw logs are not committed.
No generated app output, Windows-native graph/runtime, native vulnerability assurance,
picker/transport, frontend/JS graph, tooling outside the selected baseline, production
or distribution approval follows. See [remaining Windows gates](../../docs/research/organization-tauri-windows-runtime-gates.md).

## Lossless publication shards

The inventory index and role-grouped JSON files are each strictly below40KiB.
The default deterministic writer targets32KiB, retains whole package objects and
all original summary values, and records every shard size/SHA256/count. The index
also records the original543,017-byte canonical JSON SHA256 and Git blob identity.
No field, inactive package, warning or source/notice digest was omitted.

Reconstruct and verify the original bytes without Cargo, network or raw archives:

```sh
PYTHONDONTWRITEBYTECODE=1 python tools/organization-tauri-qualification/inventory_shards.py docs/research/organization-tauri-windows-inventory/inventory/index.json > /tmp/organization-inventory-reconstructed.json
sha256sum /tmp/organization-inventory-reconstructed.json
```

Expected SHA256:24df20bea49d6b7b13e958b22ead62f67b98015864f31abff49d42cf61d986fa.
The loader rejects duplicate keys/package IDs, unsafe/symlink paths, missing or
unindexed files, oversize shards, wrong group/count/size/hash and changed original
identity. The capture verifier uses this same checked reconstruction and still
independently rechecks all original archive/source/role/scanner capture evidence.
Cargo.lock stays a normal unsplit lock; active.md is also unchanged from the last
local design packet. Their larger blobs require separate connector publication,
not lock hiding, workflow changes, new permissions or a different credential route.
