# Organization Phase4 recovery, CI execution scope and pending STOP

Date:2026-10-03 UTC. Status: **RECOVERY DOCUMENTATION ONLY; EXECUTION STOP**.
The additional owner question sent00:56UTC remains pending. This new document is
not the lost unpublished packet and is not evidence that its exact bytes survived.

## 1. Recovered versus unavailable evidence

Fresh filesystem/object checks found the previous worktree, publication bundle,
localb4a64a0a/3950c57e/06887d0a commits, fixture lock, metadata, archives and scanner
logs unavailable. The visible checkout had reverted to PR35-era source. No cause
of that environment change is asserted. The parent independently confirmed the
missing worktree and bundle; no attempt was made to publish absent bytes.

Configured read-only Git transport successfully fetched actual remote PR50:
`8927886bbbcb02bfa2098f38155572028b31bd07`, tree
`2d0ed10254fcb505ab8f1580088e36b36d32409c`. This is the verified recovered base of
this fresh isolated branch. The old captured PR48 source and Phase1/2/3 freezes
are inherited without modification. All new files here are reconstructed prose
or explicitly historical-summary JSON, independently reviewed anew before publication.

Historical worker and independent-review reports described a406-registry-entry
fixture,273 active packages and11435 verified source files; additional licenses and
Linux advisory failures were found, with no build/runtime performed. The
[historical summary](organization-tauri-qualification-history.json) preserves
those reported identities/counts but explicitly marks original artifacts unavailable.
These are not fresh/reproducible scan results in this recovery. The old GO cannot
qualify newly reconstructed bytes. No old full lock or per-package inventory is
fabricated; this packet adds no Cargo/package/build/runtime/workflow files.

## 2. Fresh source corroboration of the pending decision

The original [ADR](../decisions/2026-10-03-organization-tauri-host-mpl-qualification.md)
still covers only cssparser0.37.0, selectors0.38.0 and cssparser-macros0.7.0 in host
build/proc-macro qualification. The following newly required scopes remain unapproved:

| Package | Source-declared license | Required path / scope |
|---|---|---|
| dtoa-short0.3.5 |MPL-2.0|cssparser0.37 → dtoa-short^0.3, behind mandatory host build-2/HTML manipulation |
| option-ext0.2.0 |MPL-2.0|dirs7 → dirs-sys0.5 → option-ext0.2, both tauri-build host and tauri normal-target dependency |

Fresh immutable source reads confirm each version/license and the unconditional
dirs-sys declaration:
[dtoa-short manifest](https://github.com/upsuper/dtoa-short/blob/2d905cdb8b2e08163dc0d015f529877fe657b4ef/Cargo.toml),
[option-ext manifest](https://github.com/soc/option-ext/blob/272f22fc9ea1ac6b08f01704af52c4ac338df4e2/Cargo.toml),
[dirs-sys manifest](https://github.com/dirs-dev/dirs-sys-rs/blob/8bcd4aa2c35990d57a2cff2953793525fc42709c/Cargo.toml).
The established upstream framework/host path is pinned in the unchanged
[preflight](../superpowers/execution/organization-client-tauri-v2-phase4-preflight.md).
Source declarations corroborate the STOP; fresh full resolution, archive verification,
feature classification and generated/binary inclusion are still required later.
Normal target dependency membership does not establish emitted covered code.

The parent reports pending question `Sentinel_59383b2800ac8191a2fe76e8e0e22d0a`,
2026-10-03 00:56UTC, for exactly these two versions/scopes, recommending Windows-first
direct Cargo, with no Linux exceptions, production adoption or distribution.
There is no owner reply/expanded approval in this checkpoint.

## 3. Recommended decision route and what it avoids

Recommend **Windows-first qualification, direct Cargo, no CLI or installer**, after
applicable decisions and platform readiness. This fits the original Windows10Pro
question and avoids asking for Linux-specific license/security exceptions solely
to exercise the cloud host. It is not permission to change computers, Windows policy,
licenses or current code, and it is not evidence that Windows qualification will pass.

Source-supported direct Cargo uses prebuilt frontendDist, tauri/custom-protocol,
tauri-build's normal responsibilities and generate_context. It can omit CLI/bundler/
installer tooling but cannot remove the two mandatory MPL paths. Source:
[Cargo invocation](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-cli/src/interface/rust/desktop.rs#L233-L272),
[build responsibilities](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-build/src/lib.rs#L5-L23),
[embedded assets/CSP](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-codegen/src/context.rs#L162-L201).
Direct Cargo does not automatically reproduce CLI hooks/environment behavior;
removeUnusedCommands cannot substitute for explicit AppManifest ACLs and denial tests.

Linux's GTK/WebKit native LGPL boundary remains outside approval. Historical
selected-graph checks also found target-lexicon0.12.16/Apache-2.0 WITH LLVM-exception
outside the exact allowlist, and proc-macro-error1.0.4 blocked by the existing scanner.
Fresh [immutable advisory](https://github.com/RustSec/advisory-db/blob/f8dee89e1b2f2f1eaf548312df7655fe5202a302/crates/proc-macro-error/RUSTSEC-2024-0370.md)
confirms RUSTSEC-2024-0370 is an unmaintained informational advisory, with no patched
versions; it is not a disclosed-exploit claim. Earlier official-registry research
found all compatible GTK/glib0.18 macro candidates retain proc-macro-error^1.0;
newer GTK0.19 is outside Tauri2.12.1/Wry0.57 requirements. That compatible-version
survey is historical, not rerun here. No fork, dependency patch, advisory waiver,
CSP weakening or broad policy alteration is recommended.

Ordinary CLI2.12.1 independently puts HTML/MPL dependencies on normal tool-runtime
paths; its previously inspected upstream default graph also contained ISC/CDLA
entries. They are excluded optional candidates for the recommended no-CLI route,
not an arbitrary future approval request. WiX/NSIS/signing remain excluded.

Native picker capability is mandatory, but a particular dialog plugin is not.
Trusted-Rust dialog2.8.1 wraps rfd and also adds a filesystem-plugin dependency;
direct upstream rfd could narrow that wrapper graph, subject to its own exact
inventory/security/subdesign. JavaScript dialog.open returns paths and expands
scopes, so it is unsuitable for the opaque-selection contract. XDG-portal picker
failure can spawn Zenity in rfd0.16.0 and is not an approved executable escape:
[Rust picker](https://github.com/tauri-apps/plugins-workspace/blob/d4835d0e947179bac24a383212792d74be3ebe4f/plugins/dialog/src/desktop.rs#L172-L181),
[JS scope expansion](https://github.com/tauri-apps/plugins-workspace/blob/d4835d0e947179bac24a383212792d74be3ebe4f/plugins/dialog/src/commands.rs#L152-L182),
[portal fallback](https://github.com/PolyMeilex/rfd/blob/5d32eec3a7930eb43b7e864eb773831bbd3d91b4/src/backend/xdg_desktop_portal.rs#L145-L181).
No alternative picker was adopted or executed.

## 4. Exact scopes still unknown or unqualified

- Fresh full selected host/normal-target/picker/JS/test-tool graph, exact source
  archive checksums/notices and current advisory status after evidence recovery
- Actual generated-output/linkage/covered-source inclusion and applicable notices/
  source-availability obligations; no production or distribution approval
- Concrete Windows-only architecture amendment preserving existing Linux/backend
  enforcement and rejection of unrelated Windows jobs; both current flags remain false
- Suitable authorized Windows10Pro host, edition/build/security patches/ESU; a
  Windows Server runner or Linux cross-target metadata cannot substitute for that proof
- Loaded WebView2 version, SDK/loader/runtime/compiler/driver terms and security,
  plus telemetry/crash data behavior described by
  [Microsoft](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy)
- Native picker/event-loop/cancellation/errors, immutable snapshot/confinement,
  restart persistence and current context invalidation on the actual OS
- Reviewed closed Document transport under unchanged provider auth/CORS and actual
  existing React/Router/Query/Motion/reduced-motion/keyboard/browser-parity cases

All original§46 research/runtime rows remain in the
[recovery design](../superpowers/specs/2026-10-03-organization-tauri-qualification-design.md).
These unknowns are gates, not requests to approve an unrestricted future graph.
Phase4 remains incomplete; Phase5/6 have not started.

## 5. Hosted CI does not execute the proposed nested fixture

An independent read-only source trace checked exact remote PR50 plus the previously
confirmed12-path inert-fixture/documentation delta. It found no path selecting,
building or executing that nested Tauri graph. This recovery is more restrictive:
it contains documentation only and does not restore the fixture at all.

- Draft creation does run [CI opened events](https://github.com/AIrisu-072/knowledge-platform/blob/8927886bbbcb02bfa2098f38155572028b31bd07/.github/workflows/ci.yml#L3-L9).
  No claim that Draft status suppresses CI
- [Root Cargo](https://github.com/AIrisu-072/knowledge-platform/blob/8927886bbbcb02bfa2098f38155572028b31bd07/Cargo.toml#L1-L19)
  names15 explicit existing members, no tools wildcard. Rust checks/test/SQLx/
  portability tasks select the root workspace or explicit existing packages.
  A nested own workspace has no new root edge; publish=false alone is not isolation
- Assurance [loads only capability TOMLs](https://github.com/AIrisu-072/knowledge-platform/blob/8927886bbbcb02bfa2098f38155572028b31bd07/tools/assurance/src/capability.rs#L33-L50)
  immediately inside spec/assurance/capabilities. The only existing manifest,
  bootstrap.toml, executes root architecture-lint. Recursive file inventory does
  not create executable capabilities. [Graph metadata](https://github.com/AIrisu-072/knowledge-platform/blob/8927886bbbcb02bfa2098f38155572028b31bd07/tools/assurance/src/graph.rs#L60-L113)
  runs root cargo metadata --no-deps and filters workspace membership
- [Dockerfile](https://github.com/AIrisu-072/knowledge-platform/blob/8927886bbbcb02bfa2098f38155572028b31bd07/Dockerfile#L4-L19)
  copies tools as input data but builds only the root workspace; output stages copy
  named existing binaries. Document runtime/scheduler and DSI tasks select explicit
  existing packages/manifests. pnpm globs exclude tools; Node globs are named suites
- Organization D2 is restricted to design/organization-client-v0-ui and its separate
  capture event. No capture/upload permission follows from this new Draft
- Recursive OSV may inspect a nested lock as metadata and fail CI. Pinned2.5.1
  [defaults Rust call analysis off](https://github.com/google/osv-scanner/blob/v2.5.1/cmd/osv-scanner/internal/helper/callanalysis_parser.go#L3-L25),
  [guards Rust execution on that flag](https://github.com/google/osv-scanner/blob/v2.5.1/internal/sourceanalysis/sourceanalysis.go#L27-L36),
  and its pinned [Cargo.lock extractor](https://github.com/google/osv-scalibr/blob/23fa66ca68dd/extractor/filesystem/language/rust/cargolock/cargolock.go#L64-L111)
  reads TOML names/versions. Unchanged command is osv-scanner scan source -r .;
  no call-analysis opt-in or new capability/configuration is added

This is static execution-routing evidence, not a promise of CI success or a
license approval. Existing authorized CI jobs still execute existing project/tool
code. The bounded claim is that this delta introduces no Tauri execution path.
Any new root edge, capability, manifest-executing discovery, build/config file or
analysis flag requires reassessment. No workflow/security check was disabled.

## 6. Current verification and next action

Fresh checks are remote-base identity, recovered file reads, immutable source
corroboration, CI routing, no product/workflow/policy delta, links/JSON/whitespace
and independent review of these new bytes. No Cargo resolution/scanner/build,
package/native installation, runtime, screenshot, upload, merge or deploy runs in
this recovery. Original old scanner/metadata results remain historical only.

The [ordered plan](../superpowers/plans/2026-10-03-organization-tauri-qualification.md)
permits documentation/reconstruction only. Independently review, parent durably
publish/verify the exact remote tree, qualify its current-head applicable CI, and
await the exact pending owner response before affected qualification work.
