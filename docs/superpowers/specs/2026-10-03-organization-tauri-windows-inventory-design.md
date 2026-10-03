# Windows-first Tauri inventory reconstruction design

Date: 2026-10-03 UTC. Status: **FIRST-SLICE INDEPENDENT REVIEW PENDING**.

## Baseline and authority

Actual PR51 remote `629822a49657bf447da9061c568dc72558265ce7`, tree
`a85096b2753002943acfafd34091760b692b63a6`, was fetched and independently matched to
surviving local0961043. CI37085364978, DSI37085364940 and Sandbox37085364996 are
SUCCESS; D2 skipped as designed, freshly read 02:16 UTC. New work branches from
that actual remote commit, without modifying any predecessor or captured PR48.

The [approved scope amendment](../../decisions/2026-10-03-organization-tauri-windows-mpl-amendment.md)
is the sole expansion. The [recovery design](2026-10-03-organization-tauri-qualification-design.md)
continues to govern the seven research and fifteen actual-runtime requirements and
frozen Phase2 §12. This subdesign only produces auditable inventory evidence.

## Chosen bounded first slice

Use an isolated private nested workspace at tools/organization-tauri-qualification,
not a root workspace member, with resolver3, edition2024 and rust-version1.98.
Its inert src/lib.rs contains only documentation, and no build.rs or executable.
Direct dependencies: tauri =2.12.1, defaults false, features wry/custom-protocol/
common-controls-v6. Build dependency: tauri-build =2.7.1, defaults false. No CLI,
installer, dialog/plugin-fs, rfd, sidecar or new JavaScript dependency is selected.
The framework base inventory is not the eventual full picker/transport application.

The original locked inventory cannot be recovered. Generate a NEW lock from official
crates.io metadata. If resolution selects cssparser-macros0.7.1, preserve that initial
lock and use Cargo's supported precise update to0.7.0, without executing either.
Any other newly prohibited version/package/use stops progression before build/use.
A single exact approved pin is not a license to downgrade arbitrary dependencies.

After lock correction, download each locked official crates.io archive as data to
the one shared Cargo cache. Match its lock checksum and audit safe members before
metadata/tree may unpack it; any unverified archive blocks that step. Metadata/tree
then run locked/offline against these verified archives.

Only Cargo generate-lockfile/update/metadata/tree and existing-policy scanner commands
are permitted in this slice. They may download official registry source archives as
data, but may not compile/test/run package code, build scripts or proc macros. No
cargo check/build/test/run/install, tauri CLI, native install, rust target install,
frontend build, workflow dispatch or runtime is included. Current Rust tools are
absent; restore the repository-selected official Rust1.98.1 minimal host toolchain
and cargo-deny0.20.2 official prebuilt release only after this plan is reviewed,
with upstream checksums, package/tool versions and license/provenance recorded.
If a trusted prebuilt tool cannot be verified, stop that tool step; do not compile
an unreviewed tool dependency graph. No hidden trust/config/credential changes.

## Roles and evidence

Select x86_64-pc-windows-msvc, while explicitly recording the actual metadata host
x86_64-unknown-linux-gnu. A Linux-host Windows-target projection is not proof of a
native Windows-host graph. Examine actual Cargo semantics/source; capture target-
conditioned build/proc-macro dependencies separately and retain native-Windows-host
unknowns rather than spoofing a host or claiming equivalence. No Linux app graph
build/use or Linux-specific waiver. Other-target lock entries remain inactive data.

Use expanded, non-deduplicated dependency occurrences with dependency-kind and
proc-macro ancestry; record host-only, target-only, both, inactive/other-target and
unresolved roles. Retain unfiltered metadata only as complete package/source lookup, including inactive
other-target data; every selected tree occurrence must match its metadata and source.
Windows-filtered metadata may omit Linux-host-conditioned edges. Cargo1.98.1 source
[tree graph](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/tree/graph.rs#L417-L449)
uses real-host cfg for build ancestry; [metadata](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/cargo_output_metadata.rs#L218-L226)
filters resolve edges differently. Both are source projections, not exact compiled
unit/artifact proof. Metadata's flattened feature union cannot establish roles. Cross-
check normal-target projection with no-proc-macro edges. Record selected features
and paths; apply exceptions only when the exact role predicate passes. An unresolved
host/target role prevents qualification, even if the license scanner passes.

Preserve fresh lock, metadata, tree and raw scanner outputs compressed losslessly
under docs/research/organization-tauri-windows-inventory/, with SHA256 provenance
manifest. The per-package JSON records registry version/checksum, SPDX, selected
roles/features, required-by paths, notice hashes and source verification. Match each
selected archive to Cargo.lock, then compare extracted cache regular-file bytes
directly against the checksum-verified archive members. Record per-file SHA256 and
notice hashes. Do not assume .cargo-checksum.json exists or use a locally generated
checksum list as independent proof. Reject missing/changed/extra source members;
explicitly identify Cargo-created extraction bookkeeping separately. Audit member
confinement, duplicate entries, links and special files before any extraction;
never execute archive content.
Use current official RustSec DB, record its exact commit/time and scanner version.
Keep raw root-policy output. Compare the selected occurrence package set with each
scanner-covered package set explicitly: cargo-deny target filtering is not Cargo
tree host/build activation. Retain a conservative all-target raw-policy scan and
package listing as coverage evidence; distinguish inactive/other-target failures.
Any selected host node absent from the Windows-filtered scan must have unchanged-
policy source/license/advisory coverage in the conservative scan, or remain
unqualified/STOP. A filtered PASS alone is insufficient.
A derived fixture-local scoped deny file may add only
the five approved exact version exceptions using version = "=x.y.z" (not an
unbounded compatible range), no other policy/advisory/source changes,
and only with role-check prerequisite. Inactive lock entries do not become approved.
Generated/source-linkage/distribution obligations remain unverified until later
reviewed builds inspect actual emitted output. Do not infer absence from host-only.

## Execution safety and preservation

Shared Cargo ownership belongs to this track. Start free disk27725MiB (02:16 UTC),
check continuously and before every download/command on every used filesystem
(including /tmp if used); abort below1536MiB anywhere. Store large archives and
expanded tool/cache/evidence data on the workspace filesystem, not /tmp. Use one
shared selected toolchain/cache and no target directory/build output. No unrelated
deletions or duplicate dependency/build trees. A 4GiB new download/evidence budget
counts both downloaded and expanded tool/cache/evidence bytes across all used
filesystems. It is an additional stop, not permission to consume the1536MiB floor.

Root source/locks/deny/workflows/assurance capabilities and all frozen Phase1/2/3
blobs remain unchanged. Before publication, recheck actual CI discovery: nested own
workspace, no root edge, no package manager tools glob, and Rust call analysis off
in existing OSV. Recursive data scans may fail honestly; no new package execution.
No image capture/upload or permanent mirror, userMac, production/merge/deploy,
Search WIP, Audit import or denied probe retry. Parent publishes reviewed bytes.

## Deferred gates and stopping condition

No actual build until independently reviewed fresh inventory, current advisories,
native SDK/loader/WebView2/compiler/driver terms, narrow desktop platform policy,
authorized suitable Windows host, explicit AppManifest ACL, closed Document transport
and trusted native picker subdesign are ready. No broad backend Windows weakening.
Windows10Pro edition/build/ESU and real engine/runtime cases require actual proof.

Deliver this first slice's reviewed inventory or exact STOP, with every unresolved
role/tool/native/artifact/JS/runtime gate explicit. Phase4 remains incomplete and
Phase5/6 unstarted. New prohibited dependencies are bundled for an owner decision
only after distinguishing mandatory paths from optional paths and supported minimal
alternatives; no arbitrary future-graph permission is requested.

## Publication-size amendment, 2026-10-03 02:43 UTC

Later parent publication direction supersedes only the earlier raw-evidence storage
sentence: do not commit dependency archives, extracted third-party source, SDK
payloads or raw logs. Keep actual raw evidence in external local task storage for
independent review; commit bounded reproducible per-package hash/role/license/scan
receipts, exact lock and validation scripts/source references. Member-map digests
bind complete source verification without mirroring source. Original-capture hashes
remain explicit; a future reproduction gets its own provenance instead of pretending
new path-dependent metadata is byte-identical. This changes publication packaging,
not license scope, verification coverage or any execution/build gate.
