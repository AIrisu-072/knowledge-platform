# Organization Client Phase4 Tauri read-only preflight and owner decision

Date: 2026-10-02 UTC. Status: **STOP — HOST DEPENDENCY LICENSE EXCEPTION PENDING**.
This is decision evidence from read-only repository/upstream inspection. It is
not an approved ADR, a qualified dependency graph, a runtime design/plan approval
or permission to resolve/install/build. The owner question sent17:46UTC
(`Sentinel_e9d65db3a324819189a253db3c5fe965`) remains pending. No permission is inferred.

## Qualified predecessor and held actions

[Phase3 authority](../specs/2026-10-02-organization-client-v0-ui-approval.md) and
[final actual20-image review](organization-d2-visual-review-v2.md) are complete.
[Evidence PR49](https://github.com/AIrisu-072/knowledge-platform/pull/49) remote
`0860e34ebd2c2353f6528423a6ebf52cfa70751d`, tree
`a1695e32b8f146dd0132d0aa5c6f8d5995789a4d`, equals local
`351b35d26f41cf2f2322792f0987d8ee7ec5fd27`. Its CI37038588206,
DSI37038588091 and Sandbox37038588073 are SUCCESS; branch-specific D2
37038588261 is correctly skipped, not new renderer proof. Captured PR48 remains
`e6bf24d8afa76a4aa7c66546bd963e4e1a90ffc8` / tree
`204a412ba40211ca052d81cdf79f2b8701c148bc`. Neither frozen subject is edited here.

Original§46 authorizes current Tauri qualification after Phase3; §49 explicitly
requires an owner decision for license/security-policy mismatch. §50's faithful-
formalization authority does not waive that STOP. Dependency resolution, installs,
builds, policy mutations, Windows jobs and runtime implementation remain held.
No production identity/data, merge/deploy or new image-sharing scope is authorized.

## Exact pending decision scope

The question concerns only an individually approved ADR exception for these
pinned MPL-2.0 packages in **Phase4 host build/proc-macro qualification**:

| Package | Pinned version | Immutable explicit license evidence |
|---|---|---|
| cssparser | 0.37.0 | [manifest](https://github.com/servo/rust-cssparser/blob/4c49486494fb24dc01390e3baca9698ef1744c71/Cargo.toml#L1-L13) |
| selectors | 0.38.0 | [manifest](https://github.com/servo/stylo/blob/572ecba2d1600e7c3d490586692a209faf703baa/selectors/Cargo.toml#L1-L12) |
| cssparser-macros | 0.7.0 | [proc-macro manifest](https://github.com/servo/rust-cssparser/blob/4c49486494fb24dc01390e3baca9698ef1744c71/macros/Cargo.toml#L1-L13) |

If approved, source/license/notice and applicable source-availability obligations
must be recorded, with exact resolved host/target/tool/native inventories,
checksums and security/source checks still required. Another prohibited package,
version or use outside this scope returns to STOP. This is not a complete
three-package inventory claim, blanket MPL permission, permission for app-runtime
inclusion or distribution, or production Tauri adoption. CLI's normal dependency
path and native-library/redistribution terms are not silently included.

Current repository policy remains unchanged:

- [Library/tool selection](../../../spec/selection/library-tool-selection-v0.md)
  lines31–56 exclude MPL and require explicit ADR approval; lines58–68 require
  locked transitive license/advisory/source/feature checks.
- [Architecture contract](../../../spec/architecture/architecture-contract-v0.md)
  lines11–13 and755–765 apply the license gate to the dependency tree, without
  a build-time exemption.
- [deny.toml](../../../deny.toml) lines11–37 has no MPL allowance. Existing
  exceptions are exact-version ISC exceptions for libloading/ring/untrusted.

## Current official candidate rather than stale cache

Live official release metadata supersedes the earlier cached2.12.0 observation.
The current core source below is pinned to
`30da1fd6e17de6107ecc850c95dfb16b5729f2dd` (2026-09-30 release):

| Component | Current stable inspected | Official release |
|---|---|---|
| tauri | 2.12.1 | [Rust core](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.1) |
| tauri-build | 2.7.1 | [build](https://github.com/tauri-apps/tauri/releases/tag/tauri-build-v2.7.1) |
| Rust CLI | 2.12.1 | [CLI](https://github.com/tauri-apps/tauri/releases/tag/tauri-cli-v2.12.1) |
| JS API / CLI | 2.12.1 / 2.12.1 | [API](https://github.com/tauri-apps/tauri/releases/tag/%40tauri-apps/api-v2.12.1), [CLI](https://github.com/tauri-apps/tauri/releases/tag/%40tauri-apps/cli-v2.12.1) |
| utils / codegen / macros / plugin | 2.10.1 / 2.7.1 / 2.7.1 / 2.7.1 | [workspace manifest](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/Cargo.toml) |
| runtime / runtime-wry | 2.12.1 / 2.12.1 | Same workspace; Wry requirement0.57 and WebView2-com0.39 are requirements, not a resolved application graph |
| Native dialog Rust / JS | 2.8.1 / 2.8.1 | [Rust](https://github.com/tauri-apps/plugins-workspace/releases/tag/dialog-v2.8.1), [JS](https://github.com/tauri-apps/plugins-workspace/releases/tag/dialog-js-v2.8.1), 2026-10-01 |

Direct framework/plugin licenses are Apache-2.0 OR MIT; the core workspace has
edition2024/MSRV1.90. Repository Rust is1.98.1. These facts do not qualify the
transitive graph or runtime. Registry package checksums/provenance remain
unverified; no project Cargo resolution or binary exists. Stable release status
is not a substitute for the repository's own advisory/security gate.

## Demonstrated feature path and dependency kind

| Edge | Required activation and kind | Immutable source |
|---|---|---|
| tauri2.12.1 → utils~2.10.1 | Required **build-dependency**, explicitly enables build-2, independent of tauri defaults | [tauri:169–173](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri/Cargo.toml#L169-L173) |
| utils → dom_query^0.28 | build-2 → html-manipulation-2 → optional dep:dom_query; dom_query defaults are already disabled | [utils:76–92](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-utils/Cargo.toml#L76-L92) |
| dom_query0.28.0 → cssparser^0.37 / selectors^0.38 | Unconditional normal dependencies, defaults enabled | [dom_query:25–45](https://github.com/niklak/dom_query/blob/186c02d9359762404054fbc8af3a5b07cdf795a9/Cargo.toml#L25-L45) |
| cssparser0.37 → cssparser-macros^0.7 | Default fast_match_byte enables its optional proc-macro | [cssparser defaults](https://github.com/servo/rust-cssparser/blob/4c49486494fb24dc01390e3baca9698ef1744c71/Cargo.toml#L37-L40) |

Independent required host paths also pass through
[tauri-build](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-build/Cargo.toml#L25-L45),
[tauri-macros](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-macros/Cargo.toml#L14-L23)
and [codegen](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-codegen/Cargo.toml#L22).
Current tauri-plugin's required utils dependency also enables build-2; the dialog
plugin uses tauri-plugin as a build dependency. No demonstrated path is solely a
dev dependency.

The current [upstream workspace lock](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/Cargo.lock)
resolves the demonstrated branch to the three pinned versions above. This is
corroboration, not our project's complete selected graph. Its legacy kuchikiki,
cssparser0.29.6/selectors0.24/cssparser-macros0.6.1 entries do not alone establish
activation and are not automatically proposed exceptions.

No inspected supported feature-only configuration removes this host path:
turning tauri defaults off does not remove required build dependencies; disabling
optional tauri-build codegen retains build-2; build-2 avoids kuchikiki by choosing
dom_query, which still has MPL dependencies; mini_selector only adds nom. Turning
CSP off does not change those manifests and is not an acceptable workaround.
Actual CSP parsing/nonce/hash generation uses the HTML path in
[codegen context](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-codegen/src/context.rs#L69-L87).
Unapproved forks or older releases are not silent alternatives.

## Host graph is not a desktop binary conclusion

With resolver3, packaged assets and webview-data-url off, the demonstrated path
is host build/proc-macro work. Normal tauri→utils enables resources; optional
webview-data-url would enable HTML manipulation on the normal target path.
[Cargo's feature separation](https://doc.rust-lang.org/cargo/reference/features.html#feature-resolver-version-2)
supports that host-only expectation. It does not prove absence from generated
output or the eventual desktop executable; those require actual resolved and
artifact evidence.

[tauri-cli](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-cli/Cargo.toml#L63-L69)
enables HTML manipulation as a normal tool dependency; the npm CLI wraps that
Rust tool. Its inventory is separate, and a prebuilt wrapper does not erase
license obligations. No runtime-distribution permission follows from the pending
host-build question.

Mozilla explains file-level copyleft and distinguishes private/internal use from
external distribution. Covered external executables generally require covered-
source availability instructions; covered changes/notices/source rights must be
preserved. Separate files in a larger work do not automatically become MPL.
See [official FAQ Q5–11](https://www.mozilla.org/en-US/MPL/2.0/FAQ/) and
[MPL sections3.1–3.4](https://www.mozilla.org/en-US/MPL/2.0/). Generated-output,
distribution and organizational-use classification still require artifact/legal
review. This is not legal approval and does not override stricter repo policy.

## Other observed prerequisites without new authority

- Windows10 Home/Pro ordinary support ended2025-10-14. WebView2 updates on22H2
  continue to at least October2028 without ESU, but OS security maintenance is
  separate. Record actual edition/build/patch/ESU and loaded WebView2 version;
  do not infer a supported host from WebView2 alone. [OS lifecycle](https://learn.microsoft.com/en-us/lifecycle/products/windows-10-home-and-pro),
  [ESU](https://learn.microsoft.com/en-us/windows/whats-new/extended-security-updates),
  [Edge/WebView2 support](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-supported-operating-systems).
- Existing [native-Windows policy](../../../spec/architecture/development-container-ci-architecture-v0.md)
  and [flags](../../../spec/architecture/dependency-rules.toml) remain unchanged:
  native_windows_supported=false and allow_native_windows=false, enforced by
  ARCH_CI_NATIVE_WINDOWS. Any future desktop-only qualification amendment must
  preserve Linux/backend authority and rejection of unrelated Windows jobs.
- Same React19.3/Router1.170.40/Query5.104/Motion13.4.6/Webpack5.111.1 is source-
  level plausible with Tauri static assets; no rewrite to Vite is warranted.
  Actual packaged routes/chunks/query and emitted JS target remain unproved.
  Existing Babel targets node:current. Motion is declared but not executed in
  current frontend source; CSS reduced-motion proof is not actual Motion proof.
- Existing Document client/bridge are same-origin ('/' and location.origin).
  A packaged asset origin needs a reviewed bounded transport while preserving
  existing provider authorization, typed/binary APIs and backend CORS. No generic
  proxy, arbitrary URL/header/principal selector or remote privileged UI.
- [Custom app commands require ACL opt-in](https://v2.tauri.app/security/capabilities/).
  Use of a main-window capability alone does not constrain unmanifested app
  commands. Exact commands/origin/window restrictions and broker checks remain
  necessary; Tauri ACL does not implement the frozen authorization/confinement.
- Direct JS [dialog.open](https://github.com/tauri-apps/plugins-workspace/blob/d4835d0e947179bac24a383212792d74be3ebe4f/plugins/dialog/src/commands.rs#L113-L172)
  accepts paths, returns FilePath and expands fs/asset scopes. That is unsuitable
  for the frozen opaque-selection interface. Any future picker stays inside
  trusted Rust and returns only a single-use selectionId. No JS fs/shell/opener
  capability, arbitrary executable or sidecar is authorized.
- All frozen broker bounds, handle-relative traversal/reparse/TOCTOU rejection,
  exclusive creates, stable same-inode-safe snapshots, current context checks,
  restart persistence and invalidation need actual runtime proof. File-system
  plugin claims or browser mocks are insufficient.
- Linux GTK/WebKitGTK native libraries have a separate LGPL boundary; this does
  not imply GTK is linked on Windows. Microsoft WebView2 terms/servicing and
  fixed-runtime ACL requirements are separate. No agreement or permission was
  accepted. Local pkg-config finds no GTK3/WebKitGTK4.1 development prerequisites;
  no package installation was attempted. Cloud disk remains subject to1.5GiB
  floor and Toolbox's serialized Cargo ownership.

## Exact next action

Wait for the owner decision on the bounded three-version host exception. Preserve
its exact scope/response in an ADR only if approved; then independently review
that record and the bounded qualification design/plan before any implementation.
Resolve/check host, target, tooling and native inventories only within the granted
scope; any expansion or policy/security conflict returns to STOP. The actual
runtime checklist in original§46 remains NOT RUN. Phase5/6 have not started.

This packet changes documentation only. No dependencies, lockfiles, license
allowlist, OS policy, capabilities, workflow, runtime source or approved Phase1–3
semantic artifacts are changed. No capture/upload/retention exception is extended.
