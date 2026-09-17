# Active Execution Pointer

- Status: **ACTIVE**
- Execution mode: **Inline Execution**
- Active capability: `Document Publish v0`
- Current phase: **DESIGN APPROVED / IMPLEMENTATION PLAN READY / DESIGN PR MERGE GATE**
- Design branch: `design/document-publish-v0`
- Design PR: `#5`
- Approved Design Spec: `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
- Design approval record: `docs/superpowers/specs/2026-09-17-document-publish-v0-design-approval.md`
- Implementation Plan: `docs/superpowers/plans/2026-09-17-document-publish-v0-implementation.md`
- Execution Status: `docs/superpowers/execution/document-publish-v0-status.md`
- Design baseline: `main@2ead1e21222c3b122704b6e0ce31f3f902b93659`
- Planned implementation branch: `feat/document-publish-v0` after PR #5 merge

## Mandatory resume order

When resuming this repository in a new chat/session/agent context, do **not** reconstruct state from conversation history.

Read in this order:

1. `AGENTS.md`
2. this file
3. `docs/superpowers/execution/document-publish-v0-status.md`
4. `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
5. `docs/superpowers/specs/2026-09-17-document-publish-v0-design-approval.md`
6. `docs/superpowers/plans/2026-09-17-document-publish-v0-implementation.md`
7. current GitHub state of `design/document-publish-v0`, PR `#5`, and exact-head CI

Repository state and fresh GitHub evidence override remembered/chat state.

## Resume command

> `AIrisu-072/knowledge-platform` の `AGENTS.md` と Active Execution Pointer に従い、Document Publish v0を再開してください。repositoryのExecution Status・Approved Design・Approval Record・Implementation Plan・PR #5・exact-head CIを正本にしてください。

## Current hard gate

The written Design is approved and the Implementation Plan is ready, but production implementation has not started.

Required order:

1. exact-head CI for Design PR #5 must be green;
2. Design PR #5 must be merged only after an explicit user merge instruction;
3. create `feat/document-publish-v0` from the exact merged `main` head;
4. execute the Implementation Plan with TDD and Inline Execution.

Do not implement production code on `design/document-publish-v0`.

## End-of-session rule

Before intentional session switch or context exhaustion, update the capability Execution Status with completed state, exact verification/CI evidence, branch/PR/head SHA, blockers, next exact action, and any approved Design amendment.

Do not mark work complete without fresh verification evidence.
