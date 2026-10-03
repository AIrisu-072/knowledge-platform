# Organization Client Phase4 qualification design — recovery edition

Date:2026-10-03 UTC. Status: **DOCUMENTATION REVIEW PENDING; EXECUTION STOP**.
Newly reconstructed written design; no claim of recovering the lost original
artifact or its qualified dependency lock. The [recovery record](../../research/organization-tauri-qualification-recovery.md)
separates fresh source checks from historical unavailable evidence.

## Authority and immutable baseline

Original owner message `Sentinel_1df724f240248191864d194a0538819c` §§46,49,50 and
[all52-section map](../execution/organization-client-v0-requirements-map.md) govern.
§50 permits faithful formalization and ordered progression without another routine
approval, not a claim that the owner reviewed a later Git blob. The
[three-package ADR](../../decisions/2026-10-03-organization-tauri-host-mpl-qualification.md)
records only the existing narrow permission; the new two-package question is pending.

Fresh recovered repository base is actual remote PR50
8927886bbbcb02bfa2098f38155572028b31bd07/tree2d0ed10254fcb505ab8f1580088e36b36d32409c.
The [Phase1](2026-10-02-organization-client-v0-product-ux-approval.md),
[Phase2](2026-10-02-organization-client-v0-domain-api-approval.md) and
[Phase3](2026-10-02-organization-client-v0-ui-approval.md) frozen artifacts and
captured PR48e6bf24d8 remain unchanged. Product backend work is Phase5, not a
substitute for Phase4 qualification. Search WIP and unqualified Audit remain outside.

## Selected approach, pending relevant decisions

Recommend Windows-first, direct Cargo, embedded existing React/Webpack assets,
single main window and no installer/CLI/sidecar. Direct Cargo can exclude optional
CLI/bundler dependencies; it cannot remove mandatory dtoa-short or option-ext.
This is a recommendation, not permission to alter Windows policy or use a different
host. No Linux exception or security waiver is proposed. Current native-Windows
flags remain false; an exact desktop-only amendment and suitable authorized Windows
host are separate gates. Preserve Linux/backend authority and negative unrelated-job
checks rather than globally weakening the platform rule.

Inventory stages remain distinct: host build/proc macros; each selected normal
target; inactive/other-target lock entries; JS frontend/API; CLI/test/packaging tools;
native SDK/loader/system libraries; generated outputs/executable/distribution.
Only exact package/version/use exceptions apply. Flattened Cargo metadata features
or Cargo.lock membership alone do not establish active role or binary inclusion.
No unqualified code runs to discover whether its license is acceptable.

The current recovery has no new Cargo.toml/Cargo.lock, package.json, build script,
capability manifest or executable source. It does not resolve/install/build anything.
Later inventory recreation requires an exact reviewed metadata-only subplan after
the owner decision; compare fresh results against historical reports without
relabelling them identical. Serial Cargo ownership, shared cache and1536MiB free
floor remain mandatory; no unrelated output deletion or duplicate build trees.

## Runtime security and existing-feature boundary

Reuse the existing React application, TanStack Router/Query, Motion and Webpack.
Presentation remains independent of Tauri APIs; runtime adapters implement the
frozen Phase2§12 interfaces. A declared Motion dependency or CSS media query is
not proof of actual Motion/reduced-motion execution. Packaged chunks, deep links,
reload/history and emitted JS must be tested in the actual recorded WebView2 engine.

Explicit AppManifest::commands opt-in and a single selected main-window capability
are required. No remote origin, broad fs/shell/opener permission, arbitrary URL,
path, executable, principal or caller-supplied privilege switch. Keep generated CSP
active and verify actual command/origin/window denials. Do not rely on CLI-only
removeUnusedCommands when using direct Cargo.

The existing Document bridge is same-origin. A future packaged adapter needs its
own reviewed closed method/path transport to a process-fixed owned synthetic
Document endpoint, with typed/binary limits and unchanged provider authorization,
identity/CORS/business logic. No generic HTTP proxy or frontend-only authorization.
No new Organization backend is built merely to supply the qualification fixture.

A mandatory actual native picker stays inside trusted Rust and returns only a
single-use selectionId bound to main window/current principal/device/context.
Trusted-Rust dialog-plugin and direct-rfd are unadopted alternatives requiring
separate graph/cancellation/error/event-loop checks. No JavaScript path return or
implicit filesystem-scope grant. XDG-portal fallback spawning Zenity is not an
approved no-executable alternative.

Keep Phase2§12 exact interfaces and every bound: page100, read range1MiB,
create8MiB, pending operations4, depth32, name255 UTF-8 bytes, snapshot8MiB,
handles4/total32MiB and lease5 minutes. Enforce current authority each use,
handle-relative no-follow confinement and exclusive creates. Reject absolute,
UNC/device/ADS, decoded traversal, symlink/junction/reparse and parent-swap races.
A stable immutable snapshot must reject same-inode mutation/truncate/replacement;
mtime/size alone is insufficient. No safe OS method means unavailable/STOP.
Detach/revoke/context change/process exit invalidates handles; restart never
resurrects old read handles. Persist actual roots/bindings/operation receipts,
not reseeded substitutes. Browser native capabilities remain honestly unavailable.

## Original§46 coverage, all execution proof still outstanding

| Required research | Required evidence |
|---|---|
| stable version | current official stable v2, excluding alpha SemVer even if metadata flag is wrong |
| license | exact source/notice/obligation evidence |
| transitive licenses | complete classified selected graph and exact exception intersections |
| security model | CSP/origin/window/command/broker tests and current advisories |
| Windows10Pro viability | actual edition/build/patch/ESU and supported authorized host |
| WebView2 requirement | actual loaded version, SDK/loader/runtime terms/servicing/privacy |
| React/Webpack compatibility | same feature source/assets in actual target engine |

| Required actual-runtime item | Required proof |
|---|---|
| existing React UI | existing Document React UI mounted |
| TanStack Router | navigation/deep link/reload/back-forward |
| TanStack Query | actual fetch/error/retry/invalidation/current authority |
| Motion | actual library transition execution |
| reduced motion | actual preference branch and finite settlement |
| keyboard / focus | Tab/Shift-Tab/Escape/return and interrupted/repeated flows |
| Document API | existing typed/binary provider calls and denial semantics |
| native folder picker | actual choose/cancel, bound opaque receipt |
| managed Workspace | same-operation managed root create/recovery |
| directory binding | opaque attach/detach/current scope and persistence |
| scoped read/create | real bounded stable read/exclusive create and limit+1 |
| path traversal rejection | broker denial plus unchanged outside sentinel |
| symlink escape rejection | actual OS links/reparse/races rejected |
| restart persistence | same actual roots/registry/receipts; stale handles rejected |
| same-source browser build | same React features, normal HTTP, no fake native resources |

No source/unit/Linux cross-target evidence substitutes for actual Windows or native
runtime proof. No new image capture/upload permission is inferred. Phase4 cannot
be GO, and Phase5/6 cannot begin, until every original gate is satisfied.
