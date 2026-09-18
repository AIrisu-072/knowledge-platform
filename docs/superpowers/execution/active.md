# Active Execution Pointer

- Status: **ACTIVE**
- Execution mode: **Inline Execution**
- Active capability: `Document Publish v0`
- Current phase: **IMPLEMENTATION COMPLETE / PR #6 FINAL REVIEW + MERGE GATE**
- Implementation branch: `feat/document-publish-v0`
- Implementation PR: `#6`
- Approved Design Spec: `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
- Design approval record: `docs/superpowers/specs/2026-09-17-document-publish-v0-design-approval.md`
- Implementation Plan: `docs/superpowers/plans/2026-09-17-document-publish-v0-implementation.md`
- Execution Status: `docs/superpowers/execution/document-publish-v0-status.md`
- Implementation baseline: `main@51fb06ae2c62886ab032fb2767104b65cc33c8ee`
- Latest verified runtime head: `a89a7d6e1f1f85722521b8ef89b43c183f9ee9d9`
- Runtime CI: `#211` — **SUCCESS**
- Runtime Rust tests: **81/81 PASS, 0 skipped**

## Mandatory resume order

When resuming this repository, do **not** reconstruct state from conversation history.

Read in this order:

1. `AGENTS.md`
2. this file
3. `docs/superpowers/execution/document-publish-v0-status.md`
4. `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
5. `docs/superpowers/specs/2026-09-17-document-publish-v0-design-approval.md`
6. `docs/superpowers/plans/2026-09-17-document-publish-v0-implementation.md`
7. current GitHub state of branch `feat/document-publish-v0`, PR `#6`, review state, and exact-head CI

Repository state and fresh GitHub evidence override remembered/chat state.

## Completed scope

Tasks 1–7 of the approved Plan are implemented and verified. Task 8 runtime verification and self-review are complete; only final documentation/evidence head CI and PR metadata transition remain.

Frozen Design coverage is complete with no approved amendments required.

## Current hard gate

PR #6 remains the only active implementation PR.

Required order:

1. fetch PR #6 exact current head;
2. require hosted CI success for that exact head;
3. require zero unresolved blocking review findings;
4. mark PR #6 Ready only after those conditions hold;
5. **do not merge PR #6 without an explicit user merge instruction.**

A documentation-only evidence commit may make the PR head newer than the verified runtime head. This is expected; fetch exact-head CI rather than assuming the runtime CI covers it.

## Resume command

> `AIrisu-072/knowledge-platform` の `AGENTS.md` と Active Execution Pointer に従い、Document Publish v0のPR #6最終ゲートから再開してください。Execution Status・Approved Design・Implementation Plan・現在のPR head・exact-head CIを正本にしてください。

## End-of-session rule

Before intentional session switch/context exhaustion, record the exact PR head, CI evidence, review state, blockers, and next action.

Do not mark PR #6 merged unless GitHub reports it merged.
