# Audit Infrastructure v1 — capability status

## 2026-10-02 UTC — A2 Management compatibility qualification reopened

- Prior Draft PR45 exact-head CI/Sandbox/DSI remain successful, but producer cross-check found a real normal-ACL reason-preservation gap. Normal SetAccessPolicy takes a required reason; neither its outbox payload nor the common Management ledger retains it. The prior synthetic bootstrap-as-Document fixture was not source-backed.
- Correction branch `fix/audit-schema-legacy-management-compat`, base `a34bfae6ad6f25a66875184e282e9b62b7d8bee1`. Bounded amendment: `../specs/2026-10-02-audit-infrastructure-v1-management-compatibility-amendment.md`. Two regression tests observed RED, then local24/24 Rust and Node4/4 plus strict Clippy/generation checks passed; fresh correction review is pending. No full producer compatibility claim.
- All six T5–T8 reason-required commands were cross-checked: five changed-event types retain source reason and are already deferred; normal ACL omits it. All share the missing ledger reason, including no-op outcomes. No historical reconstruction/backfill or Document branch edit is authorized by this correction.
- Next exact action: independently review the immutable amendment/schema/fixture packet, then parent may update PR45 and verify new exact-head hosted gates. Genuine bootstrap must match the actual Root Folder-only producer; source/store Task3 work stays separate.

The following is the previous review checkpoint; the newly found conditional-variant gap supersedes its completeness claim.

## 2026-10-02 UTC — A2 code re-review GO; exact-head hosted qualification next

- A0 complete; A1 frozen and independently approved. A2 review1 on `c177ca0c` found two Important/P2: contradictory ACL target/resource evidence and last-wins duplicate JSON admission. Both have genuine RED→GREEN repairs; current audit-core21/21 tests (67 catalog vectors), strict Clippy, Node4/4, schema regeneration and workspace fmt pass. Independent re-review GO at `dd17021ec0938fe77430769d3ee5a2c1d2b6d97a` confirmed both repairs and seven parser edge probes with no remaining Critical/Important finding. Exact-head A-AUD1 hosted gates are pending. A3–A6 NOT STARTED.
- Branch `feat/audit-infrastructure-v1-schema`; baseline is D-AUD1 hygiene head `3ccd86dd4592dfffbc9baca735f3e76b82417dbf` (remote PR44 `82d2150b46e1ac680aa685b6e5e7b0e8b936ce4b`, identical tree). Only approved exact31 metadata and root/experiment yoke-derive0.8.4 version/checksum corrections carry across; no PR43/Search code dependency.
- Scope: pure audit-core,20-type catalog (13 eligible/7 deferred), generated CloudEvents/profile schema, bounded unique-key raw parsing, source-backed legacy adapter/canonicalization and contract CI. Source SQL admission/store/dispatcher/roles/retention/recovery remain NOT IMPLEMENTED. Mixed-sign timestamp tuples are rejected rather than silently normalized.
- Frozen design/plan blobs: `a1d2002afb7525f19f40138a82365bc975f493ed` / `3fd2d2ef2b8891dd63345233c5f67f69412dfbed`. Approved bounded compatibility addendum: `c932988f7d9aadd8d1d77271fe184a9ca250026a`. All unchanged. SQL512/513 qualification remains future work.
- Details/limits: `audit-infrastructure-v1-schema-qualification.md`. `mise run verify:fast` is unavailable (attempted exit127); no full workspace completion claim. D-AUD1 remote6e0058f9 had Standard CI/Sandbox SUCCESS, then the isolated experiment lock fix addressed its sole DSI yank failure; fresh remote82d2150b gates remain pending.
- Next exact action: parent publishes the reviewed Draft schema layer and checks exact remote tree/head/CI. Prepare A3 on a separate delivery branch under the frozen plan; do not claim backend/integration/erasure from schema tests or hosted success before exact receipts.

The following is the prior A1 checkpoint.

## 2026-10-02 UTC — A0 complete, A1 FROZEN; A2 next

- Status: A0 COMPLETE; A1 FROZEN after independent review3 GO; all six Important/P2 resolved at design level, no global STOP. A2–A6 NOT STARTED. No product code, migration, dependency or runtime change.
- Source/base: `d71753d46590bb4406a1c0b74894ab90a27a6c88`; tree `0d5e1b473c9c0324b55d1171396a17d65e2a925c`. Independent branch `design/audit-infrastructure-v1`, Draft publication not yet performed.
- A0 report: `audit-infrastructure-v1-a0-reconstruction.md`. Base's own CI36718016267 is SUCCESS; identical-tree PR35 has CI/Sandbox/PoC SUCCESS. Other Document/Search Draft qualification is not imported.
- Design: `../specs/2026-10-02-audit-infrastructure-v1-design.md`; plan: `../plans/2026-10-02-audit-infrastructure-v1-implementation.md`.
- Authorization: `audit-infrastructure-v1-scope-authorization.md` records current user-approved independent autonomous track and STOP boundaries, not a false retrospective artifact approval.
- Review resolutions proposed: defer/quarantine reason-bearing variants and all source cleanup; serialize source/sink publication counters through commit plus bootstrap completeness barrier; retain permanent identity/expired receipt registry; fence expiration to current locked policy revision; fail-close restore until current trusted evidence reconciles; procedural physical-size/compression, internal JSONB structure/numeric, bounded-render and wire-size admission before client materialization; sink lock order fixed. Audit-only ownership/convergence remains unchanged.
- Tests/builds: NOT RUN; repository/source/GitHub evidence inspection and documentation only. Shared cloud capacity is bounded; coordinate heavy Cargo work before building.
- Next exact action: create `feat/audit-infrastructure-v1-schema` from this docs freeze receipt and write genuine failing schema/core tests before implementation. Independent review3 GO at `e48acee7bf6d63fe3352698cd8de5be0dd1fb8cf` permits A2; backend/runtime remain unqualified.

- Frozen design blob: `a1d2002afb7525f19f40138a82365bc975f493ed`; frozen plan blob: `3fd2d2ef2b8891dd63345233c5f67f69412dfbed`. Approval source remains the bounded current user instruction; technical gate is independent review3 GO.
