# Organization Client v0 — Phase 0 reconstruction

Snapshot: 2026-10-02 12:22 UTC. Repository and live GitHub observations are implementation SSOT. This document is a read-only reconstruction, not a new product acceptance claim.

## Accepted predecessor

- [PR43](https://github.com/AIrisu-072/knowledge-platform/pull/43) source H2: `6103e4d4e3bb0d45ba03e1d2935492de7f11394a`, tree `f2e13eee0d7e1bfa71952c1da52a72cecb65fc9e`.
- [Owner final acceptance](https://github.com/AIrisu-072/knowledge-platform/pull/43#issuecomment-5952167525) records the owner's 2026-10-02 12:16 UTC approval, including the disclosed visual limitations. This clears the requested predecessor checkpoint. CI alone did not clear it.
- [PR46 report R2](https://github.com/AIrisu-072/knowledge-platform/pull/46): `88628331f32e141c71944e9f7f093deb348f51ac`, tree `92416d3dc7d7cc1c549d5ead5aa8c8b1ef70c43d`. It is evidence, not a substitute product baseline. Its ordered parents are report R1 `f49866f0fef88d2735db062b83c3ad686e9097b6` and H2. The difference from H2 is exactly five Markdown evidence/status/runbook paths.
- Local `4b15a492` and `851f8aee` are tree-equivalent source/report preparation commits, not the hosted commits. Full hosted H2 and R2 objects and ancestry were independently verified in the shared object database.
- New isolated branch `design/organization-client-v0` starts at actual H2. Existing Document, Search and Audit branches/worktrees are untouched. No existing PR is merged or closed.

The source's committed Active/Status records necessarily predate its later hosted checks and owner acceptance. Their pending text remains historical. The linked exact-head receipt resolves that chronology without rewriting the frozen predecessor. Conversely, a later integrated qualification does not upgrade an older, different source head.

## PR36–43 current-head reuse matrix

All listed PRs are open, Draft and unmerged. Workflow IDs below were fetched for each exact head, not copied from a differently qualified branch.

| PR | Classification | Exact head | Current observed gates | Reuse / restriction |
|---|---|---|---|---|
| [36](https://github.com/AIrisu-072/knowledge-platform/pull/36) GUI | reuse with adaptation | `706307b980beb540db0759ad772ed4debd42c289` | CI36946070147, DSI36946070021, Sandbox36946070032 SUCCESS | Frozen GUI semantics and feature foundation; its own historical G9 closure remains pending. Use integrated H2 source, which includes later GUI repairs |
| [37](https://github.com/AIrisu-072/knowledge-platform/pull/37) Runtime design/plan | reuse unchanged | `dc9eb9bde55777934c100ce79c8c2b43ca8430eb` | CI36927672334 SUCCESS | Composition, fixed identity, explicit migration/bootstrap, same-origin and separate-scheduler contracts; design CI is not runtime acceptance |
| [38](https://github.com/AIrisu-072/knowledge-platform/pull/38) MCP design/plan | reuse unchanged | `5a2b114964ddbe7d38dd6a5fe9b70fdad2cb56f1` | CI36927833055 SUCCESS | Generated-client, nine read tools, stdio, fixed Agent endpoint and no direct DB/Storage boundary |
| [39](https://github.com/AIrisu-072/knowledge-platform/pull/39) PoC acceptance plan | reuse unchanged | `7d49a7bbde4d26bd072732c41cebe85a419fd4dd` | CI36928002797 SUCCESS | Same-run/exact-head provenance, real composition, failure/recovery and honest evidence classification |
| [40](https://github.com/AIrisu-072/knowledge-platform/pull/40) Search WIP | blocked / unresolved | `a945fbd32145a3109e35cb9cb056cea052698138` | CI36945866252 and DSI36945866208 FAILURE; Sandbox36945866263 SUCCESS | Read-only future integration contract only; no code dependency, repair, completion claim or rename |
| [41](https://github.com/AIrisu-072/knowledge-platform/pull/41) Runtime implementation | reuse unchanged | `513529f5c256ee6e439dcdd16e746b417912ab2c` | CI36965695310, DSI36965695308, Sandbox36965695318 SUCCESS | Existing real server, explicit commands, fixed profiles, seed, separate scheduler and harness; integrated H2 is the source baseline |
| [42](https://github.com/AIrisu-072/knowledge-platform/pull/42) MCP implementation | reuse unchanged | `143ce4d5a07abbdca076f1f90c8e979234814be1` | CI36972476928, DSI36972476913, Sandbox36972476940 SUCCESS | Existing actual-stdio adapter and runtime acceptance; no Agent write authority or live LLM qualification inferred |
| [43](https://github.com/AIrisu-072/knowledge-platform/pull/43) Integrated acceptance | reuse with adaptation | `6103e4d4e3bb0d45ba03e1d2935492de7f11394a` | Normal CI36987407999, DSI36987408029, Sandbox36987408087; capture CI36989549579 SUCCESS | Owner-accepted exact source. N2/V2 and later report R2 have distinct run subjects; no dataset/evidence mixing |

C0 is not erased: PR36's unchanged own-head G9 record remains an independent outstanding deliverable. It is neither retrospectively passed by H2 nor invented as an additional Organization-start gate after the owner's explicit H2 acceptance.

## Component reuse and qualification

Classification vocabulary: **reuse unchanged** preserves authoritative behavior;
**reuse with adaptation** changes the containing shell/runtime/test composition,
not the inherited business meaning; **superseded** applies to an assumption or
historical status, not permission to delete its source; **blocked / unresolved**
does not qualify an implementation.

| Component | Classification | Present implementation / evidence | Organization treatment |
|---|---|---|---|
| Document GUI | reuse with adaptation | `apps/document-web/src`; H2 real browser journey and disclosed V2 pixel review | Reuse React feature, generated client/BinaryTransportBridge, Version/Revision/OCC, capabilities, Diff and original-file semantics. Existing app shell/landing is Document-specific and needs scoped composition change |
| Document Domain/Application/API | reuse unchanged | Existing crates and OpenAPI3.2.1; frozen approved semantics | Authoritative document boundary remains intact; never clone into Task/Agent UI |
| document-server | reuse unchanged | `crates/document-server/src/composition.rs`; real production adapters and route composition | Reuse as Document composition root, not Organization Domain. No desktop sidecar or duplicate business service |
| migrate/bootstrap | reuse unchanged | `main.rs`, `bootstrap.rs`, schema compatibility | Explicit disposable-database commands only; `serve` never migrates/seeds. Production migration remains unauthorized |
| Static identity | reuse with adaptation | `identity.rs`: fixed `poc-human` / `poc-agent`, refreshed context expiry | Synthetic runtime seam only. Headers/query/body/cookies cannot select identity; do not rename this existing frozen behavior or call it production identity |
| PostgreSQL/FileStorage | reuse unchanged | Real authoritative shared state with restart identity and row/file assertions | Reuse server-owned authoritative providers. Desktop paths do not become shared storage or handoff references |
| Scheduler | reuse unchanged | Separate process; requester current rights and `service/scheduler` audit attribution | Preserve separate process, original requester/executor distinction, OCC and exactly-once business outcome; no authority granted by the attribution name |
| Real-runtime harness | reuse with adaptation | `tools/document-poc-runtime`, actual workers, browser/API/MCP/DB/storage | Extend later only within approved acceptance scope; no mocks counted as runtime proof, no capture policy automatically inherited by new artifacts |
| Document MCP | reuse unchanged | `apps/document-mcp`, nine read tools, actual stdio | Reuse read capability. Agent Chat is a new interaction composition, not permission to add write tools or bypass API authorization |
| Human/Agent acceptance | reuse unchanged | N2/V2 all22 stages, browser11/11, persistence1/1; nine Agent groups | Accepted predecessor evidence only. New Organization source must earn its own exact-head gates and scenarios |
| Static serving | reuse with adaptation | `web.rs`; human-only built dist, reserved API/health, bounded canonical paths | Browser profile stays supported. Tauri assets/origin/transport must be explicitly qualified; do not weaken CORS/CSP/path rules |
| Browser-only assumptions | superseded | `main.tsx` redirects `/` to `/documents`; shell links are absolute; client is same-origin; Webpack publicPath `/`; native browser file input | Isolate runtime/navigation composition while retaining feature semantics. Browser cannot claim desktop local-resource capability |

Explicit disposition: GUI **reuse with adaptation**; document-server,
migrate/bootstrap, PostgreSQL/FileStorage and scheduler **reuse unchanged**;
static identity **reuse with adaptation** only for a separately bounded
multi-principal Organization synthetic fixture, preserving Document's fixed
process identities; harness **reuse with adaptation**; Document MCP **reuse
unchanged**; Human/Agent acceptance **reuse unchanged** as predecessor evidence,
with new Organization evidence required; GUI static-serving assumption **reuse
with adaptation** through a runtime contract; browser-only product/landing
assumption **superseded** by the owner-approved same-React desktop-primary
Organization shell. C0 own-head closure and Search WIP are **blocked /
unresolved**. No component deletion follows from these classifications.

### Evidence limits carried forward

The accepted13 images include selected-detail loading in01 (02 separately shows ready context), a clipped long list title in02, full-page/fixed-viewport scrim behavior in08, WORKING-only post-publication selection fallback in09, and tight trace/reload adjacency in11/12. They were accepted with those limits, not certified flawless or all settled. The text comparator's unsupported multiline alignment remains Unknown/None, not unchanged. Existing non-blocking motion, keyboard/focus, explicit timezone/offset and backend-confirmed success contracts remain mandatory.

## Independent Audit boundary

[PR44](https://github.com/AIrisu-072/knowledge-platform/pull/44) head `82d2150b46e1ac680aa685b6e5e7b0e8b936ce4b` is the separate design track. [PR45](https://github.com/AIrisu-072/knowledge-platform/pull/45) head `f616b7207fc29cc721e7767d78d814307ac24be3` qualifies schema/contract only. Its original normal-ACL/reason preservation limits remain deferred. A3 store/delivery/security qualification is not a dependency or completed capability here.

The future handoff accepts stable actual-principal, acting-responsibility, resource and correlation identifiers plus a separately reviewed versioned closed metadata catalog after Organization Phase1/2. Audit is durable accountability evidence, not business history authority, KPI/management analytics, transcript storage or personal memory. Preserve the current mandatory source-transaction staging rule, distinguish generation/staging/delivery/store/verification, and never copy private draft or Evidence body into generic Audit. No Audit code is imported or repaired by this design branch.

## Local verification boundary

- Clean H2 worktree/tree identity and source/report ancestry: PASS.
- `git diff --check`: PASS before new documents.
- Baseline `node --test tools/api-contract/contract.test.mjs`: BLOCKED by absent declared Redocly dependency in the fresh worktree, before contract assertions. Host default Node24.19.0 differs from pinned24.21.0; no installation or product failure is inferred.
- Rust/build/browser/dependency installation: NOT RUN. Shared disk approximately2.2GiB; preserve1.5GiB floor and serialized heavy-build ownership.
- No product source, lock, workflow, static-serving, identity, database or existing acceptance artifact changed.
